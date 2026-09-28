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

/// Adds a domain to the project's policy file, creating it if this is the first.
pub fn allow_domain(root: &Path, domain: &str) -> Result<(), String> {
    let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    if domain.is_empty() || domain.contains('/') {
        return Err(format!("{domain:?} is not a domain name"));
    }
    let path = root.join(POLICY_FILE);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("{} could not be made: {e}", parent.display()))?;
    }
    let mut policy: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let entry = policy
        .as_object_mut()
        .ok_or_else(|| format!("{} is not a JSON object", path.display()))?
        .entry("allow")
        .or_insert_with(|| serde_json::json!([]));
    let list = entry
        .as_array_mut()
        .ok_or_else(|| "the `allow` entry must be a list of domains".to_string())?;
    if !list.iter().any(|v| v.as_str() == Some(domain.as_str())) {
        list.push(Value::String(domain));
    }
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&policy).unwrap_or_default() + "\n",
    )
    .map_err(|e| format!("{} could not be written: {e}", path.display()))
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
        remember_pending(&db, &target.host);
        inbound
            .write_all(refusal_response(&target.host).as_bytes())
            .await?;
        return Ok(());
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

    #[test]
    fn a_refusal_says_the_one_command_that_fixes_it() {
        let response = refusal_response("example.com");
        assert!(response.starts_with("HTTP/1.1 403"));
        assert!(response.contains("aether1 code net-allow example.com"));
    }
}
