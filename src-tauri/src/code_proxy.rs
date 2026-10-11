//! The sandbox's only way out, and the policy it answers to.
//!
//! The sandbox made the filesystem honest. The network was still all or nothing: either the
//! box had no route at all, which is safe and means `npm install` cannot work, or the
//! operator switched the network on and the box could reach anything -- including whatever
//! a fetched web page told the model to send a copy of your source tree to. Neither is the
//! mode anybody wants to work in.
//!
//! So the network becomes a *policy* instead of a switch, and the enforcement is structural.
//!
//! ```text
//!   inside the sandbox                    outside it
//!   ┌──────────────────────────┐
//!   │ cargo / npm / curl       │
//!   │      ↓ HTTP(S)_PROXY     │
//!   │ 127.0.0.1:8080  (relay)  │──unix socket──▶  this proxy ──▶ registry.npmjs.org  ✓
//!   │                          │                      │
//!   │ no route anywhere else   │                      └──▶  unknown.example        ✗
//!   └──────────────────────────┘
//! ```
//!
//! The box keeps its own network namespace, so it has a loopback interface and *nothing
//! else*: no default route, no DNS, no way to reach an address directly however hard a
//! compromised process tries. What it does have is one unix socket bind-mounted in, which
//! crosses a network namespace because it is a file rather than a route. A relay started
//! inside the box (`aether1 --net-relay`, see `command`) listens on loopback, hands every
//! connection down that socket, and this proxy -- running outside, where the real network
//! is -- decides one question per connection: **is this host one the operator agreed to?**
//!
//! Three properties follow, and they are the reason for the shape:
//!
//!   1. **Refusing is the default and bypassing is not possible.** A program that ignores
//!      `HTTPS_PROXY` does not reach the internet by ignoring it; it reaches nothing.
//!   2. **No interception, no certificate.** The decision is taken on the `CONNECT` line,
//!      which names the host in the clear before TLS begins. Aether1 never sees inside the
//!      tunnel, never terminates TLS, and installs no certificate anywhere. What it can
//!      enforce is *where a connection goes*, which is exactly what the policy is about.
//!   3. **The question is asked once, about a domain, not every time about a command.** A
//!      project that trusts crates.io says so in `.aether/policy.json`, and no later `cargo
//!      fetch` asks again.
//!
//! What this is not: a content filter. An allowed host is allowed entirely, so trusting a
//! domain means trusting it with whatever the sandbox can read -- which, thanks to the
//! sandbox, is the project folder and nothing more.

use std::collections::{BTreeSet, HashMap};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;

use crate::llm::MemoryDb;

/// The per-project file. JSON rather than TOML only because `serde_json` is already here
/// and a second parser is a dependency to maintain for the shape of one list.
pub const POLICY_FILE: &str = ".aether/policy.json";

/// Domains a fresh project trusts before anybody is asked anything: the package registries
/// and source hosts a build reaches on its own. Each is the operator's to remove.
///
/// The test for this list is narrower than "well known": it is *would a build of this
/// project have contacted this host anyway, as part of doing what was asked*. A search
/// engine is not on it. Nor is anywhere a file can be posted to.
pub const STARTER_DOMAINS: &[&str] = &[
    "github.com",
    "api.github.com",
    "codeload.github.com",
    "raw.githubusercontent.com",
    "objects.githubusercontent.com",
    "crates.io",
    "static.crates.io",
    "index.crates.io",
    "registry.npmjs.org",
    "pypi.org",
    "files.pythonhosted.org",
    "proxy.golang.org",
    "sum.golang.org",
    "repo.maven.apache.org",
    "nuget.org",
    "api.nuget.org",
];

/// Where the relay inside the sandbox listens, and therefore what `HTTPS_PROXY` says. A
/// fixed port is safe here in a way it never is on a real machine: the sandbox has a
/// network namespace of its own, so this loopback belongs to one command and nothing else
/// can be holding it.
pub const RELAY_PORT: u16 = 8080;

/// Where the socket is bind-mounted inside the sandbox.
///
/// Under `/tmp`, which is the tmpfs the sandbox makes for itself, because everything else
/// in there is the read-only host and bubblewrap cannot create a mount point on it. The
/// command can see the socket, which is the point: it is what the command is meant to use.
pub const SOCKET_IN_SANDBOX: &str = "/tmp/aether1-net.sock";

/// Domains asked for and refused, kept so the operator can be shown what a build wanted
/// rather than having to read a failure and guess.
pub const PENDING_SETTING: &str = "code_net_pending";

// ------------------------------------------------------------------ the policy

/// The domains this project trusts: the starter list, plus whatever `.aether/policy.json`
/// adds, minus whatever it removes.
pub fn allowed(root: &Path) -> BTreeSet<String> {
    let mut domains: BTreeSet<String> = STARTER_DOMAINS.iter().map(|d| d.to_string()).collect();
    let Ok(text) = std::fs::read_to_string(root.join(POLICY_FILE)) else {
        return domains;
    };
    let Ok(policy) = serde_json::from_str::<Value>(&text) else {
        return domains;
    };
    // `allow` adds, `deny` takes away, and deny wins -- an operator who wrote a host into
    // `deny` has said something more deliberate than the default list ever did.
    if let Some(list) = policy.get("allow").and_then(Value::as_array) {
        for entry in list.iter().filter_map(Value::as_str) {
            domains.insert(entry.trim().to_ascii_lowercase());
        }
    }
    if let Some(list) = policy.get("deny").and_then(Value::as_array) {
        for entry in list.iter().filter_map(Value::as_str) {
            domains.remove(&entry.trim().to_ascii_lowercase());
        }
    }
    domains
}

/// Whether one host is covered, by exact match or as a subdomain.
///
/// `crates.io` covers `static.crates.io`; it does not cover `crates.io.evil.example`, which
/// is why this checks a dot-delimited suffix rather than `ends_with`.
pub fn covers(domains: &BTreeSet<String>, host: &str) -> bool {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    domains
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
}

/// Checks a string is a domain name this file can hold, and normalises it.
///
/// Rejected rather than accepted-and-ignored, because a domain that silently does not
/// match is a policy row the operator believes in and the proxy does not. A path, a scheme
/// and a port are all signs the operator pasted a URL, which is the common mistake, so each
/// is named rather than lumped into "invalid".
fn domain_name(domain: &str) -> Result<String, String> {
    let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    if domain.is_empty() {
        return Err("a domain name is needed".to_string());
    }
    if domain.contains("://") {
        return Err(format!(
            "{domain:?} is a URL. The policy is about hosts, so write just the host part."
        ));
    }
    if domain.contains('/') {
        return Err(format!(
            "{domain:?} has a path in it. A domain is allowed entirely or not at all, so \
             write just the host part."
        ));
    }
    if domain.contains(':') {
        return Err(format!(
            "{domain:?} has a port in it. The policy is about hosts, not ports."
        ));
    }
    if domain.contains(|c: char| c.is_whitespace()) {
        return Err(format!("{domain:?} is not a domain name"));
    }
    if !domain.contains('.') && domain != "localhost" {
        return Err(format!(
            "{domain:?} is not a domain name -- it has no dot in it."
        ));
    }
    Ok(domain)
}

/// Reads the project's policy file, or an empty object if there is not one yet.
fn read_policy(root: &Path) -> Result<(PathBuf, Value), String> {
    let path = root.join(POLICY_FILE);
    let policy: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if !policy.is_object() {
        return Err(format!("{} is not a JSON object", path.display()));
    }
    Ok((path, policy))
}

fn write_policy(path: &Path, policy: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("{} could not be made: {e}", parent.display()))?;
    }
    std::fs::write(
        path,
        serde_json::to_string_pretty(policy).unwrap_or_default() + "\n",
    )
    .map_err(|e| format!("{} could not be written: {e}", path.display()))
}

/// Adds `domain` to one of the file's two lists and takes it out of the other.
///
/// Both halves matter. `deny` wins over `allow` when the policy is read, so adding a domain
/// to `allow` while it sits in `deny` would write a row that changes nothing -- which is
/// exactly what happens when an operator removes a starter domain and then puts it back.
fn move_domain(root: &Path, domain: &str, into: &str, out_of: &str) -> Result<String, String> {
    let domain = domain_name(domain)?;
    let (path, mut policy) = read_policy(root)?;
    let object = policy
        .as_object_mut()
        .ok_or_else(|| format!("{} is not a JSON object", path.display()))?;

    let list = object
        .entry(into)
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .ok_or_else(|| format!("the `{into}` entry must be a list of domains"))?;
    if !list.iter().any(|v| v.as_str() == Some(domain.as_str())) {
        list.push(Value::String(domain.clone()));
    }

    if let Some(list) = object.get_mut(out_of).and_then(Value::as_array_mut) {
        list.retain(|v| v.as_str() != Some(domain.as_str()));
    }

    write_policy(&path, &policy)?;
    Ok(domain)
}

/// Adds a domain to the project's policy file, creating it if this is the first.
pub fn allow_domain(root: &Path, domain: &str) -> Result<(), String> {
    move_domain(root, domain, "allow", "deny").map(|_| ())
}

/// Takes a domain back out of the project's policy.
///
/// Removing from `allow` is not enough on its own: the starter list is not in the file, so
/// a domain that came from there would still be covered after being removed from a list it
/// was never in. So this removes the `allow` row *and* writes a `deny` row, which is the
/// only way to say no to a starter domain -- and then reports which of the two it was, so
/// the Settings page can tell the operator whether they removed a row or added one.
pub fn forget_domain(root: &Path, domain: &str) -> Result<(), String> {
    move_domain(root, domain, "deny", "allow").map(|_| ())
}

/// The domains the project's own file adds, and the ones it takes away.
///
/// The Settings page needs these apart from [`allowed`]: a starter domain and one the
/// operator typed look the same in a merged list, but only one of them can be removed by
/// deleting a row.
pub fn policy_lists(root: &Path) -> (Vec<String>, Vec<String>) {
    let Ok((_, policy)) = read_policy(root) else {
        return (Vec::new(), Vec::new());
    };
    let list = |key: &str| -> Vec<String> {
        policy
            .get(key)
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|entry| entry.trim().to_ascii_lowercase())
                    .collect()
            })
            .unwrap_or_default()
    };
    (list("allow"), list("deny"))
}

/// Records a host a command asked for and did not get, newest last, capped.
fn remember_pending(db: &MemoryDb, host: &str) {
    let mut seen: Vec<String> = db
        .get_setting(PENDING_SETTING)
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    if seen.iter().any(|entry| entry == host) {
        return;
    }
    seen.push(host.to_string());
    // The last twenty is plenty to answer "what did it want"; an unbounded list is a
    // settings row that grows forever because a loop retried a refused host.
    while seen.len() > 20 {
        seen.remove(0);
    }
    let _ = db.set_setting(PENDING_SETTING, &serde_json::json!(seen));
}

/// The hosts commands have asked for and been refused.
pub fn pending(db: &MemoryDb) -> Vec<String> {
    db.get_setting(PENDING_SETTING)
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

/// Forgets the refused list, which `allow` does after adding one.
pub fn clear_pending(db: &MemoryDb) {
    let _ = db.set_setting(PENDING_SETTING, &serde_json::json!(Vec::<String>::new()));
}

// ------------------------------------------------------------------ asking the operator

/// Hosts refused without anybody being asked, kept for the diagnostics the Settings page
/// shows. Separate from [`PENDING_SETTING`]: that is "a build wanted this", this is "and
/// the answer was no", and an operator reading the page needs to tell those apart.
pub const DENIED_SETTING: &str = "code_net_denied";

/// How long a connection waits for an answer before it is refused.
///
/// Long enough that a card noticed a few seconds late is still useful, short enough that a
/// build tool's own timeout is not what reports the problem. A refusal at the end of it
/// says nobody answered, which is a different sentence from "the policy says no".
const ASK_TIMEOUT: Duration = Duration::from_secs(90);

/// How recently the HUD must have asked for open questions before the proxy will hold a
/// connection open waiting for one to be answered.
///
/// This is the gate that keeps a headless Aether1 honest. `aether1 --serve` with nobody
/// watching, a `run` from the CLI, the test suite: in all of those there is no card and no
/// operator, so waiting 90 seconds would turn a clean refusal into a hang. The HUD marks
/// itself present every time it polls; absent that mark, the proxy refuses at once exactly
/// as it did before any of this existed.
const WATCHER_WINDOW: Duration = Duration::from_secs(30);

/// How long a settled question is kept so the connections waiting on it can read the
/// answer. New connections never find it -- they look by host, and a settled question is
/// no longer listed under one.
const SETTLED_GRACE: Duration = Duration::from_secs(30);

/// What the operator chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// This attempt only. Nothing is written, and the next connection asks again.
    Once,
    /// Written into the project's `.aether/policy.json`; nothing asks again.
    Project,
    /// Refused, and recorded in the diagnostics.
    Deny,
}

impl Decision {
    pub fn key(self) -> &'static str {
        match self {
            Decision::Once => "once",
            Decision::Project => "project",
            Decision::Deny => "deny",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key.trim().to_ascii_lowercase().as_str() {
            "once" => Some(Decision::Once),
            "project" => Some(Decision::Project),
            "deny" => Some(Decision::Deny),
            _ => None,
        }
    }
}

/// A question on screen: a host a command asked for that the policy does not cover.
#[derive(Debug, Clone, Serialize)]
pub struct Ask {
    /// Opaque, and what a decision names. Not the host: a host can be asked about twice,
    /// and answering the first question must not answer the second.
    pub id: String,
    pub host: String,
    pub port: u16,
    /// Unix seconds, so the card can say how long it has been waiting.
    pub asked_at: u64,
    /// How many connections are waiting on this one question. A `cargo fetch` opens
    /// several at once; they share a card rather than each raising one.
    pub waiting: usize,
}

struct Open {
    id: String,
    host: String,
    port: u16,
    asked_at: u64,
    waiting: usize,
}

#[derive(Default)]
struct Asks {
    /// Keyed by host, which is what makes concurrent connections to the same host share
    /// one card instead of raising a stack of identical ones.
    open: HashMap<String, Open>,
    /// Answered questions, by ask id, until the connections waiting on them have read the
    /// answer. Keyed by id and not host so an `Once` grant cannot be picked up by a
    /// connection that arrives after the decision.
    settled: HashMap<String, (Decision, Instant)>,
    /// When the HUD last asked what is waiting.
    watcher: Option<Instant>,
}

fn asks() -> &'static std::sync::Mutex<Asks> {
    static ASKS: OnceLock<std::sync::Mutex<Asks>> = OnceLock::new();
    ASKS.get_or_init(|| std::sync::Mutex::new(Asks::default()))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Marks the operator as present. Called by whatever surface draws the cards, every time it
/// looks; the proxy will only hold a connection open while this is recent.
pub fn note_watcher() {
    if let Ok(mut table) = asks().lock() {
        table.watcher = Some(Instant::now());
    }
}

/// Puts the table back to its startup state: nobody watching, nothing asked. Only the
/// tests need this, and they need it because the table is process-wide -- a test that
/// marks a watcher would otherwise leave every later test waiting 90 seconds for an
/// operator who does not exist.
#[cfg(test)]
fn reset_asks() {
    if let Ok(mut table) = asks().lock() {
        table.open.clear();
        table.settled.clear();
        table.watcher = None;
    }
}

/// Whether a card would actually be seen by anybody right now.
pub fn watched() -> bool {
    asks()
        .lock()
        .ok()
        .and_then(|table| table.watcher)
        .is_some_and(|seen| seen.elapsed() < WATCHER_WINDOW)
}

/// The questions currently on screen.
pub fn open_asks() -> Vec<Ask> {
    let Ok(table) = asks().lock() else {
        return Vec::new();
    };
    let mut list: Vec<Ask> = table
        .open
        .values()
        .map(|ask| Ask {
            id: ask.id.clone(),
            host: ask.host.clone(),
            port: ask.port,
            asked_at: ask.asked_at,
            waiting: ask.waiting,
        })
        .collect();
    list.sort_by(|a, b| a.asked_at.cmp(&b.asked_at).then(a.host.cmp(&b.host)));
    list
}

/// Answers one question.
///
/// Returns the host it was about, so the caller can say what it just allowed, or `None` if
/// the question is already gone -- answered by another window, or timed out while the card
/// sat on screen. Writing the policy file for `Project` is the caller's job and must happen
/// *before* this, because this is what releases the waiting connections.
pub fn settle(id: &str, decision: Decision) -> Option<String> {
    let mut table = asks().lock().ok()?;
    let host = table
        .open
        .iter()
        .find(|(_, ask)| ask.id == id)
        .map(|(host, _)| host.clone())?;
    let ask = table.open.remove(&host)?;
    table.settled.insert(ask.id, (decision, Instant::now()));
    Some(host)
}

/// Records a host that was refused, for the diagnostics the Settings page shows.
fn remember_denied(db: &MemoryDb, host: &str, why: &str) {
    let mut log: Vec<Value> = db
        .get_setting(DENIED_SETTING)
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    log.push(serde_json::json!({ "host": host, "at": now_secs(), "why": why }));
    while log.len() > 20 {
        log.remove(0);
    }
    let _ = db.set_setting(DENIED_SETTING, &serde_json::json!(log));
}

/// The refusals shown in the Settings page's diagnostics, newest last.
pub fn denied(db: &MemoryDb) -> Vec<Value> {
    db.get_setting(DENIED_SETTING)
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

/// Forgets the refusal log.
pub fn clear_denied(db: &MemoryDb) {
    let _ = db.set_setting(DENIED_SETTING, &serde_json::json!(Vec::<Value>::new()));
}

/// Puts a host in front of the operator and waits for an answer.
///
/// `None` means no answer arrived: either there was nobody there to ask, or the card went
/// unanswered for [`ASK_TIMEOUT`]. Both are a refusal, and the caller distinguishes them
/// for the message only.
async fn ask_operator(host: &str, port: u16) -> Option<Decision> {
    if !watched() {
        return None;
    }
    // Join the question already on screen for this host, or raise it.
    let id = {
        let mut table = asks().lock().ok()?;
        // Reap answers nobody came back for, so a long-lived process does not accumulate
        // them.
        table
            .settled
            .retain(|_, (_, settled_at)| settled_at.elapsed() < SETTLED_GRACE);
        match table.open.get_mut(host) {
            Some(open) => {
                open.waiting += 1;
                open.id.clone()
            }
            None => {
                let id = format!("{host}-{}", now_secs());
                table.open.insert(
                    host.to_string(),
                    Open {
                        id: id.clone(),
                        host: host.to_string(),
                        port,
                        asked_at: now_secs(),
                        waiting: 1,
                    },
                );
                id
            }
        }
    };

    let deadline = Instant::now() + ASK_TIMEOUT;
    let answer = loop {
        if let Ok(mut table) = asks().lock() {
            if let Some((decision, _)) = table.settled.get(&id).copied() {
                break Some(decision);
            }
            // The operator closed the HUD while the card was up. Nothing is coming.
            let still_open = table.open.values().any(|open| open.id == id);
            if !still_open {
                break None;
            }
            if Instant::now() >= deadline {
                table.open.retain(|_, open| open.id != id);
                break None;
            }
        } else {
            break None;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    };

    // Stop counting this connection against the question, whether it got an answer or not.
    if let Ok(mut table) = asks().lock() {
        if let Some(open) = table.open.get_mut(host) {
            if open.id == id {
                open.waiting = open.waiting.saturating_sub(1);
            }
        }
    }
    answer
}

// ------------------------------------------------------------------ the request line

/// What a proxy request asked for: the host, the port, and whether it was a tunnel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub host: String,
    pub port: u16,
    /// `CONNECT` (an HTTPS tunnel) rather than a plain forwarded request.
    pub tunnel: bool,
}

/// Reads the host out of a proxy request's first line.
///
/// Two forms, because those are the two a proxy is given: `CONNECT host:443 HTTP/1.1`, which
/// is every HTTPS request, and the absolute form `GET http://host/path HTTP/1.1`, which is
/// how a plain HTTP request reaches a proxy. Nothing else is accepted -- an origin-form
/// request line means something is talking to this socket that does not think it is a
/// proxy, and guessing its intent is how a filter gets walked around.
pub fn parse_request_line(line: &str) -> Option<Target> {
    let mut parts = line.split_whitespace();
    let method = parts.next()?;
    let target = parts.next()?;
    if method.eq_ignore_ascii_case("CONNECT") {
        let (host, port) = target.rsplit_once(':')?;
        return Some(Target {
            host: host.trim_matches(['[', ']']).to_ascii_lowercase(),
            port: port.parse().ok()?,
            tunnel: true,
        });
    }
    let rest = target
        .strip_prefix("http://")
        .or_else(|| target.strip_prefix("HTTP://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    // Credentials in the authority are stripped before the host is read, so
    // `http://crates.io@evil.example/` is judged on `evil.example`, which is where it goes.
    let authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
            (host, port.parse().ok()?)
        }
        _ => (authority, 80),
    };
    Some(Target {
        host: host.trim_matches(['[', ']']).to_ascii_lowercase(),
        port,
        tunnel: false,
    })
}

/// What the proxy answers a refused request with. A body as well as a status, because the
/// thing reading it is a build tool's error output and then a model, and "403" alone sends
/// a coding agent looking for a bug in its own code.
fn refusal_response(host: &str) -> String {
    let body = format!(
        "{host} is not on this project's allowed list.\n\n\
         Aether1 runs this command in a sandbox whose only way to the network is a proxy \
         that checks the destination. To let this through:\n\
         \n  aether1 code net-allow {host}\n\n\
         That records it in .aether/policy.json and nothing asks again.\n"
    );
    format!(
        "HTTP/1.1 403 Forbidden\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    )
}

/// What a connection gets when no answer came.
///
/// Two different problems wear the same 403, so they get different sentences. A card that
/// went up and was ignored is a thing the operator can still go and answer. A `run` with no
/// HUD anywhere -- the CLI, `--serve` with nobody logged in, a test -- was never going to
/// get a card, and telling that operator to "answer the card" would send them looking for
/// something that does not exist; the one command is what they need.
fn unanswered_response(host: &str, watched: bool) -> String {
    let body = if watched {
        format!(
            "{host} is not on this project's allowed list, and the card asking about it \
             was not answered.\n\n\
             Aether1 put the question in the HUD and waited. To let this through, run the \
             command again and choose on the card, or allow it up front:\n\
             \n  aether1 code net-allow {host}\n"
        )
    } else {
        format!(
            "{host} is not on this project's allowed list, and there was no Aether1 window \
             open to ask.\n\n\
             Aether1 runs this command in a sandbox whose only way to the network is a \
             proxy that checks the destination. With the HUD open you are asked and can \
             allow it once or for the project; without it, an unlisted host is refused. To \
             allow it from here:\n\
             \n  aether1 code net-allow {host}\n\n\
             That records it in .aether/policy.json and nothing asks again.\n"
        )
    };
    format!(
        "HTTP/1.1 403 Forbidden\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    )
}

// ------------------------------------------------------------------ the server

/// A running proxy: the socket the sandbox is given.
pub struct Proxy {
    pub socket: PathBuf,
}

/// Starts the proxy once for this process and hands back its socket.
///
/// One listener for the whole program rather than one per command: a command is short, the
/// listener is cheap, and a socket whose lifetime matches the process is one fewer thing to
/// tear down when a run is killed at its timeout.
pub fn start(db: &MemoryDb) -> Result<Proxy, String> {
    static RUNNING: OnceLock<std::sync::Mutex<HashMap<PathBuf, Result<PathBuf, String>>>> =
        OnceLock::new();
    let db_path = db.path().to_path_buf();
    let mut running = RUNNING
        .get_or_init(|| std::sync::Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "the proxy table is poisoned".to_string())?;
    // Keyed by database rather than by process: one Aether1 has one, and a test that
    // wires up a second workspace gets a second rather than quietly sharing the first's
    // policy. The socket outlives the command, so this starts at most once per database.
    let socket = running
        .entry(db_path.clone())
        .or_insert_with(|| spawn(db_path).map(|proxy| proxy.socket));
    socket
        .as_ref()
        .map(|socket| Proxy {
            socket: socket.clone(),
        })
        .map_err(|why| why.clone())
}

/// A handle of this thread's own on the same database. `MemoryDb` is a path and opens a
/// connection per call, so this is a re-open rather than anything shared -- which is what
/// makes it safe to hand one to the proxy's own runtime.
fn reopen(db_path: &Path) -> Option<MemoryDb> {
    MemoryDb::open(db_path).ok()
}

/// The listener thread, with a runtime of its own so this works whether or not the rest of
/// the program happens to be running one.
fn spawn(db_path: PathBuf) -> Result<Proxy, String> {
    let dir = db_path
        .parent()
        .ok_or_else(|| "there is nowhere to put the proxy socket".to_string())?
        .to_path_buf();
    let _ = std::fs::create_dir_all(&dir);
    let socket = dir.join("net-proxy.sock");
    // A stale socket from a crash would refuse to bind; it names this process's own path
    // and nothing else uses it.
    let _ = std::fs::remove_file(&socket);

    let listen_on = socket.clone();
    std::thread::Builder::new()
        .name("aether1-net-proxy".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(_) => return,
            };
            runtime.block_on(serve(listen_on, db_path));
        })
        .map_err(|e| format!("the proxy thread could not be started: {e}"))?;

    // The socket has to exist before the sandbox is told to bind-mount it.
    for _ in 0..100 {
        if socket.exists() {
            return Ok(Proxy { socket });
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    Err("the proxy did not start listening".to_string())
}

async fn serve(socket: PathBuf, db_path: PathBuf) {
    let Ok(listener) = tokio::net::UnixListener::bind(&socket) else {
        return;
    };
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let Some(db) = reopen(&db_path) else {
            continue;
        };
        tokio::spawn(async move {
            let _ = handle(stream, db).await;
        });
    }
}

/// One connection: read the request line, decide, then either tunnel or refuse.
async fn handle(mut inbound: tokio::net::UnixStream, db: MemoryDb) -> std::io::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // Enough for a request line and headers. A client that sends more than this before the
    // blank line is not a build tool, and the read stops at the headers either way.
    let mut head = Vec::with_capacity(8 * 1024);
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") && head.len() < 16 * 1024 {
        match inbound.read(&mut byte).await {
            Ok(0) => break,
            Ok(_) => head.push(byte[0]),
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    let text = String::from_utf8_lossy(&head).to_string();
    let first = text.lines().next().unwrap_or_default().to_string();

    let Some(target) = parse_request_line(&first) else {
        inbound
            .write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n")
            .await?;
        return Ok(());
    };

    // The policy is read per connection, from the workspace, so allowing a domain takes
    // effect on the next request rather than the next restart.
    let root = crate::code_workspace::root(&db).unwrap_or_default();
    if !covers(&allowed(&root), &target.host) {
        // Not on the list is where the operator comes in. The host is recorded either way
        // -- "what did the build want" is the question asked after the fact, and it is
        // worth answering whether or not anybody was there to answer it at the time.
        remember_pending(&db, &target.host);
        match ask_operator(&target.host, target.port).await {
            // Through, this once. Nothing was written, so the next connection asks again.
            Some(Decision::Once) => {}
            // The policy file was written by whoever settled the question, before the
            // waiters were released, so `allowed` would cover this host now. Re-reading it
            // to prove that would be a second answer to a question already answered.
            Some(Decision::Project) => clear_pending(&db),
            Some(Decision::Deny) => {
                remember_denied(&db, &target.host, "refused");
                inbound
                    .write_all(refusal_response(&target.host).as_bytes())
                    .await?;
                return Ok(());
            }
            // Nobody answered: no HUD was watching, or the card sat there. Refused, and
            // said as a different sentence -- "nobody answered" sends the operator to the
            // HUD, "the policy says no" sends them to the policy.
            None => {
                let watched = watched();
                remember_denied(
                    &db,
                    &target.host,
                    if watched { "unanswered" } else { "not asked" },
                );
                inbound
                    .write_all(unanswered_response(&target.host, watched).as_bytes())
                    .await?;
                return Ok(());
            }
        }
    }

    let mut outbound =
        match tokio::net::TcpStream::connect((target.host.as_str(), target.port)).await {
            Ok(stream) => stream,
            Err(e) => {
                let body = format!("{} could not be reached: {e}\n", target.host);
                let _ = inbound
                    .write_all(
                        format!(
                            "HTTP/1.1 502 Bad Gateway\r\nContent-Length: {}\r\n\
                             Connection: close\r\n\r\n{body}",
                            body.len()
                        )
                        .as_bytes(),
                    )
                    .await;
                return Ok(());
            }
        };

    if target.tunnel {
        // From here Aether1 is a pipe. It never sees inside the TLS it is carrying, which
        // is the point: the destination was the decision, and the contents are not its
        // business.
        inbound
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await?;
    } else {
        // A plain request is forwarded exactly as it arrived, absolute form and all. HTTP/1.1
        // requires a server to accept that form, and rewriting the request line would mean
        // parsing and re-emitting something a build tool composed, which is more ways to be
        // wrong than it is worth.
        outbound.write_all(&head).await?;
    }
    let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
    Ok(())
}

// ------------------------------------------------------------------ the relay

/// `aether1 --net-relay -- <program> <args...>`, which runs *inside* the sandbox.
///
/// It listens on loopback -- the only interface the box has -- and hands every connection
/// down the unix socket bind-mounted in from outside, then runs the real command and exits
/// with its status. Two processes rather than a shell line, so nothing in the argv is ever
/// parsed by anything.
pub fn relay_main(socket: &str, argv: &[String]) -> i32 {
    let Some((program, rest)) = argv.split_first() else {
        eprintln!("--net-relay needs a command to run after --");
        return 2;
    };
    let socket = socket.to_string();
    std::thread::Builder::new()
        .name("aether1-net-relay".to_string())
        .spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            runtime.block_on(async move {
                let Ok(listener) = tokio::net::TcpListener::bind(("127.0.0.1", RELAY_PORT)).await
                else {
                    return;
                };
                loop {
                    let Ok((mut inbound, _)) = listener.accept().await else {
                        continue;
                    };
                    let socket = socket.clone();
                    tokio::spawn(async move {
                        if let Ok(mut outbound) = tokio::net::UnixStream::connect(&socket).await {
                            let _ =
                                tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                        }
                    });
                }
            });
        })
        .ok();

    match std::process::Command::new(program).args(rest).status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            eprintln!("{program} could not be started: {e}");
            127
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn domains(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|d| d.to_string()).collect()
    }

    #[test]
    fn a_domain_covers_its_subdomains_and_nothing_that_merely_ends_with_it() {
        let allowed = domains(&["crates.io"]);
        assert!(covers(&allowed, "crates.io"));
        assert!(covers(&allowed, "static.crates.io"));
        assert!(covers(&allowed, "CRATES.IO"));
        // The one that matters: a lookalike registered by somebody else.
        assert!(!covers(&allowed, "crates.io.evil.example"));
        assert!(!covers(&allowed, "notcrates.io"));
        assert!(!covers(&allowed, "evil.example"));
    }

    /// The host is read from where the connection will actually go, not from the part of
    /// the URL that looks most reassuring.
    #[test]
    fn the_request_line_is_read_for_where_it_goes() {
        assert_eq!(
            parse_request_line("CONNECT registry.npmjs.org:443 HTTP/1.1"),
            Some(Target {
                host: "registry.npmjs.org".into(),
                port: 443,
                tunnel: true
            })
        );
        assert_eq!(
            parse_request_line("GET http://crates.io/api/v1/crates HTTP/1.1"),
            Some(Target {
                host: "crates.io".into(),
                port: 80,
                tunnel: false
            })
        );
        // Credentials in the authority are not the host.
        assert_eq!(
            parse_request_line("GET http://crates.io@evil.example/x HTTP/1.1").map(|t| t.host),
            Some("evil.example".into())
        );
        // Anything that is not a proxy request is refused rather than guessed at.
        assert!(parse_request_line("GET /api/v1/crates HTTP/1.1").is_none());
        assert!(parse_request_line("").is_none());
    }

    #[test]
    fn a_project_can_add_a_domain_and_take_one_away() {
        let root = std::env::temp_dir().join(format!("aether1_policy_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        assert!(
            covers(&allowed(&root), "crates.io"),
            "the starter list applies"
        );
        assert!(!covers(&allowed(&root), "example.com"));

        allow_domain(&root, "Example.COM.").unwrap();
        assert!(
            covers(&allowed(&root), "example.com"),
            "and is added lowercased"
        );

        // Adding twice does not write it twice.
        allow_domain(&root, "example.com").unwrap();
        let text = std::fs::read_to_string(root.join(POLICY_FILE)).unwrap();
        assert_eq!(text.matches("example.com").count(), 1, "{text}");

        // Deny wins over the starter list, which is the only way to take a default away.
        std::fs::write(
            root.join(POLICY_FILE),
            serde_json::json!({"deny": ["crates.io"]}).to_string(),
        )
        .unwrap();
        assert!(!covers(&allowed(&root), "crates.io"));
    }

    // ------------------------------------------------------------- over a real socket

    /// A database and a workspace wired to each other, as `run` would have them.
    fn wired(name: &str) -> (MemoryDb, PathBuf) {
        let home = std::env::temp_dir().join(format!(
            "aether1_proxy_{name}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&home);
        let project = home.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let db = MemoryDb::open(home.join("aether1_memory.db")).unwrap();
        db.set_setting(
            crate::code_workspace::ROOT_SETTING,
            &serde_json::json!(project.to_string_lossy().to_string()),
        )
        .unwrap();
        (db, project)
    }

    /// Speaks one proxy request over the socket and returns what came back, having first
    /// written whatever `then` says down the tunnel.
    fn ask(socket: &Path, request: &str) -> String {
        use std::io::{Read, Write};
        let mut stream = std::os::unix::net::UnixStream::connect(socket).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut back = Vec::new();
        let mut chunk = [0u8; 1024];
        // One read is enough: the proxy answers with its status line before anything else.
        if let Ok(n) = stream.read(&mut chunk) {
            back.extend_from_slice(&chunk[..n]);
        }
        String::from_utf8_lossy(&back).to_string()
    }

    /// The proxy over its real socket, refusing and allowing for real -- including carrying
    /// bytes both ways once it has said yes, which is the part an argument list cannot
    /// prove.
    #[test]
    fn the_proxy_refuses_an_unknown_host_and_carries_an_allowed_one() {
        use std::io::{Read, Write};
        // Held against the ask-table tests: a watcher left marked by one of those
        // would have the proxy hold this connection open instead of refusing it.
        let _guard = alone();

        let (db, project) = wired("live");
        let socket = start(&db).expect("the proxy starts").socket;

        // Something for an allowed host to be: a server on loopback that echoes a line.
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut client, _)) = server.accept() {
                let mut buf = [0u8; 64];
                if let Ok(n) = client.read(&mut buf) {
                    let _ = client.write_all(&buf[..n]);
                }
            }
        });

        // Refused, because nothing in the starter list covers it.
        let refused = ask(&socket, "CONNECT nowhere.example:443 HTTP/1.1\r\n\r\n");
        assert!(refused.starts_with("HTTP/1.1 403"), "{refused}");
        assert!(
            pending(&db).contains(&"nowhere.example".to_string()),
            "and the operator is told what it wanted: {:?}",
            pending(&db)
        );

        // Allowed, once the project says so -- read per connection, so no restart.
        allow_domain(&project, "localhost").unwrap();
        let mut stream = std::os::unix::net::UnixStream::connect(&socket).unwrap();
        stream
            .write_all(format!("CONNECT localhost:{port} HTTP/1.1\r\n\r\n").as_bytes())
            .unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut head = [0u8; 39];
        stream.read_exact(&mut head).unwrap();
        let head = String::from_utf8_lossy(&head).to_string();
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");

        // And from here it is a pipe: what goes down comes back from the far end.
        stream.write_all(b"through\n").unwrap();
        let mut back = [0u8; 8];
        stream.read_exact(&mut back).unwrap();
        assert_eq!(&back, b"through\n");
    }

    /// The whole claim of this module, tested the only way it can honestly be tested: run a
    /// real command in a real sandbox with a real network namespace, and see that the
    /// proxy is the only way anything leaves.
    ///
    /// Skipped where the machine cannot do it -- no bubblewrap, no network namespace, or a
    /// test run that has no built binary to re-enter as the relay.
    #[test]
    fn inside_the_sandbox_the_proxy_is_the_only_way_out() {
        // Same reason as the test above: this one asserts a refusal, and a refusal is
        // only immediate while nobody is watching.
        let _guard = alone();
        use std::io::{Read, Write};

        let sandbox = crate::code_sandbox::detect();
        if !sandbox.can_cut_network() {
            return;
        }
        // The relay is Aether1 itself; under `cargo test` the running binary is the test
        // harness, so the real one has to be found beside it.
        // Under `cargo test` the harness lives in `target/debug/deps/`, so the binary is
        // one directory up from it; under a release layout it sits beside it.
        let here = std::env::current_exe().ok();
        let Some(binary) = here
            .iter()
            .flat_map(|exe| {
                exe.parent()
                    .into_iter()
                    .chain(exe.parent().and_then(Path::parent))
            })
            .map(|dir| dir.join("aether1"))
            .find(|p| p.is_file())
        else {
            return;
        };
        let Some(python) = crate::paths::find_installed_binary(&["python3"]) else {
            return;
        };
        // The relay is that binary; `command` finds it the same way, through current_exe.
        let _ = &binary;

        let (db, project) = wired("e2e");
        let proxy = start(&db).expect("the proxy starts").socket;

        // A server on the host, which the sandbox has no route to reach directly.
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        std::thread::spawn(move || {
            while let Ok((mut client, _)) = server.accept() {
                let mut buf = [0u8; 256];
                if let Ok(n) = client.read(&mut buf) {
                    let _ = n;
                    let _ =
                        client.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\n\r\nREACHED");
                }
            }
        });

        // Run a command in the box that tries the host directly, then through the proxy.
        let script = format!(
            "import socket,urllib.request,os\n\
             try:\n\
             \x20 socket.create_connection(('127.0.0.1',{port}),timeout=3)\n\
             \x20 print('DIRECT'+'-OK')\n\
             except OSError:\n\
             \x20 print('DIRECT'+'-BLOCKED')\n\
             print('PROXY_ENV='+os.environ.get('HTTPS_PROXY','none'))\n\
             try:\n\
             \x20 print('VIA'+'-'+urllib.request.urlopen('http://localhost:{port}/', \
             timeout=5).read().decode())\n\
             except Exception as e:\n\
             \x20 print('VIA'+'-FAILED:'+type(e).__name__)\n"
        );
        let net = crate::code_sandbox::Net::Proxied {
            socket: proxy.clone(),
        };
        let run = |allowed: bool| {
            if allowed {
                allow_domain(&project, "localhost").unwrap();
            }
            // Not wrapped here: `command` puts the relay in front of whatever it is given,
            // which is the path a real `run` takes and therefore the one worth testing.
            let out = crate::code_sandbox::command(
                &sandbox,
                &python.to_string_lossy(),
                &["-c".to_string(), script.clone()],
                &project,
                &project,
                &crate::code_sandbox::Net::Proxied {
                    socket: proxy.clone(),
                },
                &crate::code_sandbox::Access::project_only(),
            )
            .output()
            .expect("the sandbox runs");
            String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr)
        };
        let _ = &net;

        // Before the domain is allowed: no route at all, and the proxy says no.
        let refused = run(false);
        assert!(
            refused.contains("DIRECT-BLOCKED"),
            "the box must have no route of its own: {refused}"
        );
        assert!(
            !refused.contains("VIA-REACHED"),
            "and the proxy must refuse an unallowed host: {refused}"
        );

        // After: the same command gets through, and only through the proxy.
        let allowed = run(true);
        assert!(
            allowed.contains("DIRECT-BLOCKED"),
            "still no route of its own: {allowed}"
        );
        assert!(
            allowed.contains("VIA-REACHED"),
            "but the allowed host is reachable through the proxy: {allowed}"
        );
    }

    /// Serialises every test that touches the ask table or the live proxy.
    ///
    /// The table is process-wide, as it has to be -- the proxy thread and whatever is
    /// drawing cards are not in the same call stack. Under `cargo test` that makes it
    /// shared mutable state between tests running in parallel, and the specific accident
    /// worth preventing is a test marking a watcher and leaving it marked: the next test
    /// to speak to the proxy would then be held for ASK_TIMEOUT waiting for an operator
    /// who was never there.
    fn alone() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
        let guard = LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reset_asks();
        guard
    }

    /// A URL pasted where a host belongs is the common mistake, and each shape of it is
    /// named rather than lumped into "invalid" -- the operator has to be able to see what
    /// to delete.
    #[test]
    fn a_domain_is_a_host_and_not_a_url() {
        assert_eq!(domain_name(" Crates.IO. ").unwrap(), "crates.io");
        assert_eq!(domain_name("localhost").unwrap(), "localhost");
        for (bad, says) in [
            ("https://crates.io", "URL"),
            ("crates.io/api", "path"),
            ("crates.io:443", "port"),
            ("", "needed"),
            ("not a domain", "not a domain name"),
            ("registry", "no dot"),
        ] {
            let refused = domain_name(bad).expect_err(&format!("{bad:?} is not a domain"));
            assert!(
                refused.contains(says),
                "{bad:?} should be refused for the reason it was refused for, got: {refused}"
            );
        }
    }

    /// The one that was a bug waiting to happen. `deny` wins over `allow` when the policy
    /// is read, so putting back a starter domain the operator had taken away has to clear
    /// the `deny` row -- otherwise the Settings page writes a row that changes nothing and
    /// reports success.
    #[test]
    fn a_starter_domain_can_be_taken_away_and_put_back() {
        let root = std::env::temp_dir().join(format!(
            "aether1_policy_back_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        assert!(
            covers(&allowed(&root), "crates.io"),
            "the starter list applies"
        );

        forget_domain(&root, "crates.io").unwrap();
        assert!(
            !covers(&allowed(&root), "crates.io"),
            "a starter domain is taken away by a deny row, since it is in no allow row"
        );
        assert_eq!(policy_lists(&root).1, vec!["crates.io".to_string()]);

        allow_domain(&root, "crates.io").unwrap();
        assert!(
            covers(&allowed(&root), "crates.io"),
            "and putting it back has to clear that deny row, or it changes nothing"
        );
        assert!(policy_lists(&root).1.is_empty(), "the deny row is gone");

        // And the other direction, for a domain that was never a starter.
        allow_domain(&root, "registry.yarnpkg.com").unwrap();
        assert_eq!(
            policy_lists(&root).0,
            vec!["crates.io".to_string(), "registry.yarnpkg.com".to_string()]
        );
        forget_domain(&root, "registry.yarnpkg.com").unwrap();
        assert!(!covers(&allowed(&root), "registry.yarnpkg.com"));

        let _ = std::fs::remove_dir_all(&root);
    }

    /// With nobody watching, an unlisted host is refused at once rather than held for
    /// ninety seconds. This is the gate the CLI, `--serve` with nobody logged in, and this
    /// test suite all rely on, and it is the fail-closed direction: no operator means no,
    /// not maybe.
    #[test]
    fn with_no_window_open_nobody_is_asked() {
        let _guard = alone();
        let started = Instant::now();
        let answer = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(ask_operator("nowhere.example", 443));
        assert_eq!(answer, None, "no watcher means no question");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "and it must refuse at once, not wait out the timeout"
        );
        assert!(
            open_asks().is_empty(),
            "and leave no card behind for a window that may open later"
        );
    }

    /// The whole of the card's mechanism, with the proxy's own waiting loop in it: a host
    /// that is not on the list raises one question, the answer releases the connection,
    /// and `once` writes nothing.
    #[test]
    fn a_watched_question_waits_for_an_answer_and_once_writes_nothing() {
        let _guard = alone();
        let root = std::env::temp_dir().join(format!(
            "aether1_policy_ask_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        note_watcher();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();

        // Two connections to the same host at once. They must share one card: a `cargo
        // fetch` opens several, and a stack of identical questions is a card nobody reads.
        let first = runtime.spawn(ask_operator("unlisted.example", 443));
        let second = runtime.spawn(ask_operator("unlisted.example", 443));

        let card = runtime.block_on(async {
            for _ in 0..50 {
                let open = open_asks();
                if open.first().is_some_and(|ask| ask.waiting == 2) {
                    return open;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            open_asks()
        });
        assert_eq!(card.len(), 1, "two connections, one question: {card:?}");
        assert_eq!(card[0].host, "unlisted.example");
        assert_eq!(card[0].waiting, 2, "and the card says how many are waiting");

        assert_eq!(
            settle(&card[0].id, Decision::Once).as_deref(),
            Some("unlisted.example")
        );
        let (a, b) = runtime.block_on(async { (first.await.unwrap(), second.await.unwrap()) });
        assert_eq!(a, Some(Decision::Once), "both waiters get the answer");
        assert_eq!(b, Some(Decision::Once));

        // `once` means once. Nothing was written, and the question is gone -- so the next
        // connection raises a new one rather than finding this answer lying around.
        assert!(
            !covers(&allowed(&root), "unlisted.example"),
            "allow once writes no policy row"
        );
        assert!(open_asks().is_empty(), "and the card is off the screen");
        assert_eq!(
            settle(&card[0].id, Decision::Once),
            None,
            "answering it twice does nothing"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A card the operator never answers is a refusal, and it says that rather than
    /// claiming the policy refused it -- the two send you to different places.
    #[test]
    fn an_unanswered_card_says_so_and_a_missing_one_says_something_else() {
        let unanswered = unanswered_response("example.com", true);
        assert!(unanswered.starts_with("HTTP/1.1 403"));
        assert!(unanswered.contains("was not answered"));

        let unwatched = unanswered_response("example.com", false);
        assert!(unwatched.contains("no Aether1 window"));
        assert!(
            unwatched.contains("aether1 code net-allow example.com"),
            "with no HUD, the command is the only way through, so it is the thing said"
        );
    }

    #[test]
    fn a_decision_survives_the_round_trip_through_the_wire() {
        for decision in [Decision::Once, Decision::Project, Decision::Deny] {
            assert_eq!(Decision::from_key(decision.key()), Some(decision));
        }
        assert_eq!(Decision::from_key("PROJECT"), Some(Decision::Project));
        assert_eq!(Decision::from_key("always"), None);
    }

    /// The refusal log is what the Settings page shows when an operator arrives asking
    /// "something could not be reached, what was it". It keeps why, not just what.
    #[test]
    fn refusals_are_kept_with_the_reason_and_capped() {
        let (db, _project) = wired("denied");
        remember_denied(&db, "one.example", "refused");
        remember_denied(&db, "two.example", "unanswered");
        let log = denied(&db);
        assert_eq!(log.len(), 2);
        assert_eq!(log[0]["host"], "one.example");
        assert_eq!(log[1]["why"], "unanswered");

        for i in 0..30 {
            remember_denied(&db, &format!("host{i}.example"), "refused");
        }
        let log = denied(&db);
        assert_eq!(
            log.len(),
            20,
            "capped, so it is not a row that grows forever"
        );
        assert_eq!(log[19]["host"], "host29.example", "newest last");

        clear_denied(&db);
        assert!(denied(&db).is_empty());
    }

    /// The flow the card exists for, over the proxy's real socket: a command asks for a
    /// host that is not on the list, the connection is *held* instead of refused, a
    /// question appears, answering it writes the policy and lets the connection through.
    ///
    /// Worth having as well as the unit test above because the thing being proved is that
    /// `handle` consults the operator at all -- the waiting loop can be correct while the
    /// proxy still refuses before reaching it.
    #[test]
    fn a_card_answered_lets_the_held_connection_through() {
        use std::io::{Read, Write};

        let _guard = alone();
        let (db, project) = wired("card");
        let socket = start(&db).expect("the proxy starts").socket;

        // Something real on the other side, so "through" means bytes arrived rather than
        // just a status line.
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        std::thread::spawn(move || {
            while let Ok((mut client, _)) = server.accept() {
                let mut buf = [0u8; 256];
                if client.read(&mut buf).is_ok() {
                    let _ =
                        client.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\n\r\nREACHED");
                }
            }
        });

        note_watcher();
        let request = format!("CONNECT localhost:{port} HTTP/1.1\r\n\r\n");
        let talking = {
            let socket = socket.clone();
            std::thread::spawn(move || {
                let mut stream = std::os::unix::net::UnixStream::connect(&socket).unwrap();
                stream.write_all(request.as_bytes()).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(20)))
                    .unwrap();
                let mut back = Vec::new();
                let mut chunk = [0u8; 1024];
                if let Ok(n) = stream.read(&mut chunk) {
                    back.extend_from_slice(&chunk[..n]);
                }
                // Past the tunnel's own reply, the real exchange.
                if back.starts_with(b"HTTP/1.1 200 Connection Established") {
                    let _ = stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n");
                    if let Ok(n) = stream.read(&mut chunk) {
                        back.extend_from_slice(&chunk[..n]);
                    }
                }
                String::from_utf8_lossy(&back).to_string()
            })
        };

        // The question, which must appear rather than the connection being refused.
        let mut card = None;
        for _ in 0..100 {
            if let Some(ask) = open_asks().into_iter().next() {
                card = Some(ask);
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let card = card.expect("an unlisted host with a window open raises a question");
        assert_eq!(card.host, "localhost");
        assert_eq!(card.port, port);

        // The order the real decision path uses, and the reason `code_net_decide` is one
        // function: the policy is written first, the waiters are released second. The
        // other way round, a released connection could re-read the policy and not find
        // the domain in it yet.
        allow_domain(&project, "localhost").unwrap();
        assert_eq!(
            settle(&card.id, Decision::Project).as_deref(),
            Some("localhost")
        );

        let spoken = talking.join().expect("the connection was answered");
        assert!(
            spoken.contains("200 Connection Established"),
            "the held connection goes through once the card is answered: {spoken}"
        );
        assert!(
            spoken.contains("REACHED"),
            "and carries bytes both ways afterwards: {spoken}"
        );
        assert!(
            covers(&allowed(&project), "localhost"),
            "allow-for-project wrote the row, so nothing asks again"
        );
    }

    /// And the other answer: refusing says no, says why in a way a build log can be read
    /// for, and records it where the Settings page looks.
    #[test]
    fn a_card_refused_refuses_the_held_connection() {
        use std::io::{Read, Write};

        let _guard = alone();
        let (db, _project) = wired("refuse");
        let socket = start(&db).expect("the proxy starts").socket;

        note_watcher();
        let talking = {
            let socket = socket.clone();
            std::thread::spawn(move || {
                let mut stream = std::os::unix::net::UnixStream::connect(&socket).unwrap();
                stream
                    .write_all(b"CONNECT nowhere.example:443 HTTP/1.1\r\n\r\n")
                    .unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(20)))
                    .unwrap();
                let mut back = Vec::new();
                let mut chunk = [0u8; 2048];
                if let Ok(n) = stream.read(&mut chunk) {
                    back.extend_from_slice(&chunk[..n]);
                }
                String::from_utf8_lossy(&back).to_string()
            })
        };

        let mut card = None;
        for _ in 0..100 {
            if let Some(ask) = open_asks().into_iter().next() {
                card = Some(ask);
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let card = card.expect("the question is raised");
        assert_eq!(
            settle(&card.id, Decision::Deny).as_deref(),
            Some("nowhere.example")
        );

        let spoken = talking.join().expect("the connection was answered");
        assert!(spoken.starts_with("HTTP/1.1 403"), "refused: {spoken}");
        assert!(
            spoken.contains("nowhere.example"),
            "and names the host, so a build log says which one: {spoken}"
        );

        let log = denied(&db);
        assert!(
            log.iter()
                .any(|entry| entry["host"] == "nowhere.example" && entry["why"] == "refused"),
            "and it is recorded where the Settings page looks: {log:?}"
        );
    }

    /// The whole chain, with a real command in a real sandbox: a build tool reaches for a
    /// host that is not on the list, the card appears, something answers it, and the
    /// command's own fetch succeeds.
    ///
    /// This is the one that proves the feature rather than its parts. The tests above hold
    /// either end of it -- the proxy's socket, and the sandbox -- but the claim being made
    /// to an operator is that *their build* carries on after they click, and nothing short
    /// of running one says that.
    ///
    /// Skipped, loudly, where the machine cannot do it: no bubblewrap, no network
    /// namespace, no python, or a test run with no built binary to re-enter as the relay.
    #[test]
    fn a_real_command_in_the_sandbox_is_released_by_answering_the_card() {
        use std::io::{Read, Write};

        let _guard = alone();
        let sandbox = crate::code_sandbox::detect();
        let skip = |why: &str| eprintln!("SKIPPED a_real_command_in_the_sandbox...: {why}");
        if !sandbox.can_cut_network() {
            skip(&format!(
                "no network namespace -- {}",
                sandbox.description()
            ));
            return;
        }
        let here = std::env::current_exe().ok();
        if !here
            .iter()
            .flat_map(|exe| {
                exe.parent()
                    .into_iter()
                    .chain(exe.parent().and_then(Path::parent))
            })
            .any(|dir| dir.join("aether1").is_file())
        {
            skip("no built aether1 binary beside the harness to act as the relay");
            return;
        }
        let Some(python) = crate::paths::find_installed_binary(&["python3"]) else {
            skip("no python3");
            return;
        };

        let (db, project) = wired("chain");
        let proxy = start(&db).expect("the proxy starts").socket;

        // The host the command will reach for. `localhost` is not on the starter list, so
        // it is exactly the unlisted-host case, and a listener here makes "through" mean
        // bytes arrived.
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        std::thread::spawn(move || {
            while let Ok((mut client, _)) = server.accept() {
                let mut buf = [0u8; 512];
                if client.read(&mut buf).is_ok() {
                    let _ =
                        client.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\n\r\nFETCHED");
                }
            }
        });

        // Whatever draws the cards, standing in for the HUD: it marks itself present, waits
        // for a question, and allows the domain for the project -- the same order
        // `commands::code_net_decide` uses, policy first and release second.
        let watcher = {
            let project = project.clone();
            std::thread::spawn(move || {
                for _ in 0..200 {
                    note_watcher();
                    if let Some(ask) = open_asks().into_iter().next() {
                        allow_domain(&project, &ask.host).expect("the policy is written");
                        settle(&ask.id, Decision::Project);
                        return Some(ask.host);
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                None
            })
        };

        // Markers assembled at runtime: `run`'s report echoes the argv, so a marker spelled
        // out in the script would be found in the output whether or not anything happened.
        let script = format!(
            "import urllib.request\n\
             try:\n\
             \x20 body=urllib.request.urlopen('http://localhost:{port}/',timeout=30).read().decode()\n\
             \x20 print('VIA'+'-'+body)\n\
             except Exception as e:\n\
             \x20 print('VIA'+'-FAILED:'+type(e).__name__+':'+str(e))\n"
        );
        let out = crate::code_sandbox::command(
            &sandbox,
            &python.to_string_lossy(),
            &["-c".to_string(), script],
            &project,
            &project,
            &crate::code_sandbox::Net::Proxied {
                socket: proxy.clone(),
            },
            &crate::code_sandbox::Access::project_only(),
        )
        .output()
        .expect("the sandbox runs");
        let spoken = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);

        let asked = watcher.join().expect("the watcher thread finished");
        assert_eq!(
            asked.as_deref(),
            Some("localhost"),
            "a command reaching an unlisted host must raise a card: {spoken}"
        );
        assert!(
            spoken.contains("VIA-FETCHED"),
            "and answering it must let the command's own fetch finish: {spoken}"
        );
        assert!(
            covers(&allowed(&project), "localhost"),
            "allow-for-project wrote the row, so the next run does not ask"
        );
    }

    #[test]
    fn a_refusal_says_the_one_command_that_fixes_it() {
        let response = refusal_response("example.com");
        assert!(response.starts_with("HTTP/1.1 403"));
        assert!(response.contains("aether1 code net-allow example.com"));
    }
}
