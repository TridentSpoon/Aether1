//! The sandbox's way to Aether1's own model server, and nothing else.
//!
//! `code_sandbox` gives a command its own network namespace. That is the whole point: the
//! box has a loopback interface and no route anywhere, so a compromised process cannot
//! reach an address however hard it tries. It also means the box's `127.0.0.1` is *the
//! box's*, and the Ollama or LM Studio server listening on the operator's loopback is not
//! in it. A coding agent running inside the box therefore cannot reach the model that is
//! supposed to be driving it -- and `code_proxy` correctly refuses `127.0.0.1`, because a
//! proxy that allowed loopback would open every unauthenticated service on the machine,
//! not just this one.
//!
//! So the brain gets a door of its own, the same shape as the proxy's and narrower:
//!
//! ```text
//!   inside the sandbox                      outside it
//!   ┌──────────────────────────┐
//!   │ the engine               │
//!   │      ↓ OPENAI_BASE_URL   │
//!   │ 127.0.0.1:8081  (relay)  │──unix socket──▶  this forwarder ──▶ 127.0.0.1:11434
//!   │                          │
//!   │ no route anywhere else   │            one address, fixed when the box was built
//!   └──────────────────────────┘
//! ```
//!
//! The difference from `code_proxy` is the whole design: that one reads a request line and
//! decides per host, because a build tool asks for many hosts. This one reads nothing. It
//! has a single destination, chosen by Aether1 from its own settings before the box
//! existed, and every byte on the socket goes there. There is no request to parse, no host
//! to compare, and nothing a program inside the box can say that changes where its
//! connection lands.
//!
//! Three rules keep that destination honest, and each fails closed:
//!
//!   1. **An endpoint-addressed provider, or no door.** Ollama and LM Studio are addressed
//!      by `llm_endpoint`. The four cloud providers are not, and a tunnel to one of them
//!      would be an unaudited way off the machine sitting next to the proxy that exists to
//!      audit exactly that. With Aether1 on a cloud provider there is no socket at all.
//!   2. **A local address, by name and then again by number.** `local_only::is_local_endpoint`
//!      judges the setting, which is the boundary the rest of the program already uses. But
//!      a name is not an address: `brain.local` passes that check and could answer with a
//!      public one, so every address the name resolves to is checked with
//!      `local_only::is_local_ip` as well, and one that is not local refuses the lot.
//!   3. **Only for a command that is meant to have it.** The socket is bind-mounted by
//!      `code_sandbox::command` only when its caller asks, and `code_workspace::run` --
//!      every command the operator or the companion starts -- does not. A test suite has no
//!      business asking the brain questions; a door that opens for one caller is one fewer
//!      thing to reason about later.
//!
//! What this is not: an authenticated channel. Whatever is in the box can talk to the model
//! server as freely as anything else on the operator's machine can, which is what makes
//! rule 3 the one that matters.

#[cfg(unix)]
use std::collections::HashMap;
use std::net::IpAddr;
#[cfg(unix)]
use std::net::SocketAddr;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
#[cfg(unix)]
use std::sync::OnceLock;

use crate::llm::{MemoryDb, Provider};

/// Where the relay inside the sandbox listens for the model, and therefore what the engine's
/// base URL says.
///
/// One past `code_proxy::RELAY_PORT` so the two are read together, and fixed for the same
/// reason that one is: the box has a network namespace of its own, so this loopback belongs
/// to one command and nothing else can be holding the port.
pub const RELAY_PORT: u16 = 8081;

/// Where the socket is bind-mounted inside the sandbox.
///
/// Under `/tmp` for the same reason as the proxy's: that is the tmpfs the box makes for
/// itself, and bubblewrap cannot create a mount point anywhere on the read-only host.
pub const SOCKET_IN_SANDBOX: &str = "/tmp/aether1-model.sock";

/// The model server as the forwarder needs it: one address, and the path prefix the engine
/// has to keep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// The host as the setting spells it, for the message when something refuses.
    pub host: String,
    pub port: u16,
    /// What follows the authority in `llm_endpoint`, with no trailing slash: `/v1` for an
    /// LM Studio install, empty for Ollama. The engine is given this back, because a base
    /// URL that drops it points at an API the server does not speak.
    pub prefix: String,
}

impl Endpoint {
    /// The base URL to hand a program inside the box.
    pub fn in_box(&self) -> String {
        format!("http://127.0.0.1:{RELAY_PORT}{}", self.prefix)
    }
}

/// A running forwarder: the socket to bind-mount, and where it goes.
pub struct Brain {
    pub socket: PathBuf,
    pub endpoint: Endpoint,
}

/// Which model server Aether1 is configured to talk to, if it is one the box may be given.
///
/// A pure function of the settings, so every refusal below can be tested with no server and
/// no sandbox anywhere near the test.
pub fn configured(db: &MemoryDb) -> Result<Endpoint, String> {
    let provider = Provider::from_key(&db.get_setting_string("llm_provider", "offline"));
    match provider {
        Provider::Ollama | Provider::LmStudio => {}
        Provider::Offline => {
            return Err(
                "AETHER1 is not pointed at a model server, so there is nothing in \
                        the sandbox to reach. Choose Ollama or LM Studio in Settings."
                    .to_string(),
            );
        }
        cloud => {
            // Named rather than printed from the key, because `Provider`'s own Display is
            // the wire spelling and "AETHER1 is on anthropic" reads like a typo on a card.
            let cloud = match cloud {
                Provider::OpenAi => "OpenAI",
                Provider::Groq => "Groq",
                Provider::Gemini => "Gemini",
                Provider::Anthropic => "Anthropic",
                // The two local providers and Offline are answered above.
                _ => "a cloud provider",
            };
            return Err(format!(
                "AETHER1 is on {cloud}, which is reached by API rather than by address, so \
                 there is no local model server to open to the sandbox. A tunnel to a cloud \
                 API would also be a way off this machine that the network policy knows \
                 nothing about. Point AETHER1 at Ollama or LM Studio to give a coding agent \
                 a brain it can reach."
            ));
        }
    }

    let endpoint = db.get_setting_string("llm_endpoint", "http://localhost:11434");
    if !crate::local_only::is_local_endpoint(&endpoint) {
        return Err(format!(
            "{endpoint} is not on this machine or the local network, so it is not opened to \
             the sandbox. Everything the sandbox sends anywhere else goes through the \
             network policy, where you can see it and say no."
        ));
    }
    parse(&endpoint)
}

/// Splits `llm_endpoint` into the address to dial and the prefix to keep.
fn parse(endpoint: &str) -> Result<Endpoint, String> {
    let parsed = url::Url::parse(endpoint.trim())
        .map_err(|e| format!("{endpoint} is not a URL AETHER1 can read: {e}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| format!("{endpoint} names no host to reach"))?
        .to_string();
    // `Url` fills in 80 and 443 for the two schemes it knows and nothing for the rest, and
    // a model server addressed without a port is on neither -- so the default here is the
    // scheme's, and a URL with no port and no known scheme is refused rather than guessed.
    let port = parsed.port_or_known_default().ok_or_else(|| {
        format!(
            "{endpoint} names no port, and {} has no default",
            parsed.scheme()
        )
    })?;
    let prefix = parsed.path().trim_end_matches('/').to_string();
    Ok(Endpoint { host, port, prefix })
}

/// The one address the socket forwards to.
///
/// Resolution happens here, outside the box, because the box has no DNS -- and the answer
/// is checked against the same boundary the name was, since a name that passes and an
/// address that does not is exactly the hole rule 2 is about.
fn dial_address(endpoint: &Endpoint) -> Result<SocketAddr, String> {
    use std::net::ToSocketAddrs;
    let found: Vec<SocketAddr> = (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()
        .map_err(|e| {
            format!(
                "{}:{} could not be looked up: {e}",
                endpoint.host, endpoint.port
            )
        })?
        .collect();
    let remote: Vec<IpAddr> = found
        .iter()
        .map(|addr| addr.ip())
        .filter(|ip| !crate::local_only::is_local_ip(*ip))
        .collect();
    if let Some(ip) = remote.first() {
        return Err(format!(
            "{} resolves to {ip}, which is not on this machine or the local network, so it \
             is not opened to the sandbox. A name that looks local and answers with a public \
             address is the one case this check exists for.",
            endpoint.host
        ));
    }
    found
        .into_iter()
        .next()
        .ok_or_else(|| format!("{} resolved to no address at all", endpoint.host))
}

/// Starts the forwarder once for this process and hands back its socket.
///
/// Keyed by database and started at most once, like `code_proxy::start`, and for the same
/// reasons: a command is short, a listener is cheap, and a socket whose lifetime matches the
/// process is one fewer thing to tear down when a run is killed at its timeout.
///
/// The destination is fixed here, at start. An operator who repoints `llm_endpoint` gets the
/// new address on the next launch; the alternative is a socket whose destination can change
/// under a connection, which is a worse thing to own than a stale one.
///
/// **Only a working forwarder is remembered.** A refusal is not, and that is the difference
/// between this table and a cache. Every reason `spawn` can refuse is a condition on the
/// machine rather than a fact about the settings: a model server that is not up yet, so its
/// name does not resolve; a directory that could not be made; a thread that could not be
/// started. Remembering those made the first attempt the only attempt -- an operator who
/// gave AETHER CODE a task before starting Ollama got that same refusal from every task
/// afterwards, with nothing they could do but restart AETHER1 and nothing telling them so.
/// So a refusal is handed back and forgotten, and the next task asks the machine again.
#[cfg(unix)]
pub fn start(db: &MemoryDb) -> Result<Brain, String> {
    static RUNNING: OnceLock<std::sync::Mutex<HashMap<PathBuf, Brain>>> = OnceLock::new();
    let endpoint = configured(db)?;
    let db_path = db.path().to_path_buf();
    let mut running = RUNNING
        .get_or_init(|| std::sync::Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "the model socket table is poisoned".to_string())?;
    // A socket that is no longer on disk is not a forwarder, whatever the table says. A
    // `/tmp` sweep or an operator clearing the directory would otherwise leave every later
    // task bind-mounting a path that is not there, which bubblewrap refuses with a message
    // about a mount point rather than about the brain.
    if running
        .get(&db_path)
        .is_some_and(|brain| !brain.socket.exists())
    {
        running.remove(&db_path);
    }
    let brain = match running.entry(db_path.clone()) {
        std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(spawn(&db_path, endpoint)?)
        }
    };
    Ok(Brain {
        socket: brain.socket.clone(),
        endpoint: brain.endpoint.clone(),
    })
}

/// Windows has no door, and says so.
///
/// The door is a unix socket because that is the one kind of address that crosses a network
/// namespace, and the namespace is bubblewrap's. Neither exists here: `code_sandbox` already
/// refuses to run commands on this platform for want of a sandbox, so a command that could
/// use this socket cannot be started in the first place. Refusing with the reason beats a
/// named pipe that looks like confinement and is not -- see S6 in docs/SECURITY_MODEL.md.
#[cfg(not(unix))]
pub fn start(db: &MemoryDb) -> Result<Brain, String> {
    // Still checked, so an operator on Windows gets the same answer about their settings as
    // one on Linux before they get the answer about their platform.
    configured(db)?;
    Err(
        "AETHER1 cannot open its model server to a sandboxed command on Windows: the door is \
         a unix socket crossing a network namespace, and this platform has neither. Commands \
         are already refused here for want of a sandbox."
            .to_string(),
    )
}

/// The listener thread, with a runtime of its own so this works whether or not the rest of
/// the program happens to be running one.
#[cfg(unix)]
fn spawn(db_path: &Path, endpoint: Endpoint) -> Result<Brain, String> {
    let address = dial_address(&endpoint)?;
    let dir = db_path
        .parent()
        .ok_or_else(|| "there is nowhere to put the model socket".to_string())?
        .to_path_buf();
    let _ = std::fs::create_dir_all(&dir);
    let socket = dir.join("model.sock");
    // A stale socket from a crash would refuse to bind; it names this process's own path
    // and nothing else uses it.
    let _ = std::fs::remove_file(&socket);

    let listen_on = socket.clone();
    std::thread::Builder::new()
        .name("aether1-model-socket".to_string())
        .spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            runtime.block_on(serve(listen_on, address));
        })
        .map_err(|e| format!("the model socket thread could not be started: {e}"))?;

    // The socket has to exist before the sandbox is told to bind-mount it.
    for _ in 0..100 {
        if socket.exists() {
            return Ok(Brain { socket, endpoint });
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    Err("the model socket did not start listening".to_string())
}

/// Every connection to one address. Nothing is read before the copy begins, because there
/// is no decision left to take: the destination was settled before this socket existed.
#[cfg(unix)]
async fn serve(socket: PathBuf, address: SocketAddr) {
    let Ok(listener) = tokio::net::UnixListener::bind(&socket) else {
        return;
    };
    loop {
        let Ok((mut inbound, _)) = listener.accept().await else {
            continue;
        };
        tokio::spawn(async move {
            if let Ok(mut outbound) = tokio::net::TcpStream::connect(address).await {
                let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> MemoryDb {
        let path = std::env::temp_dir().join(format!(
            "aether1_code_model_{}_{:?}.db",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&path);
        MemoryDb::open(&path).expect("open test db")
    }

    fn pointed_at(db: &MemoryDb, provider: &str, endpoint: &str) {
        db.set_setting("llm_provider", &serde_json::json!(provider))
            .expect("provider");
        db.set_setting("llm_endpoint", &serde_json::json!(endpoint))
            .expect("endpoint");
    }

    #[test]
    fn an_endpoint_addressed_provider_on_loopback_is_the_ordinary_case() {
        let db = db();
        pointed_at(&db, "ollama", "http://localhost:11434");
        let endpoint = configured(&db).expect("loopback ollama is reachable");
        assert_eq!(endpoint.host, "localhost");
        assert_eq!(endpoint.port, 11434);
        assert_eq!(endpoint.prefix, "");
        assert_eq!(endpoint.in_box(), "http://127.0.0.1:8081");
    }

    /// LM Studio's address carries the API version, and a base URL that drops it points the
    /// engine at a path the server does not answer on. The prefix survives the swap.
    #[test]
    fn the_path_the_server_answers_on_survives() {
        let db = db();
        pointed_at(&db, "lmstudio", "http://127.0.0.1:1234/v1");
        let endpoint = configured(&db).expect("lmstudio is reachable");
        assert_eq!(endpoint.prefix, "/v1");
        assert_eq!(endpoint.in_box(), "http://127.0.0.1:8081/v1");
    }

    /// Rule 1. A tunnel to a cloud API next to the proxy that exists to audit exactly that
    /// traffic would be the hole in the middle of the design.
    #[test]
    fn a_cloud_provider_gets_no_door_at_all() {
        for provider in ["openai", "groq", "gemini", "anthropic"] {
            let db = db();
            // Pointed at a perfectly local address, which must not be what decides it: the
            // provider is reached by API, and the endpoint setting is not even consulted.
            pointed_at(&db, provider, "http://localhost:11434");
            let why = configured(&db).expect_err("a cloud provider has no local server");
            assert!(
                why.contains("cloud") || why.contains("API"),
                "{provider} should be refused as a cloud provider: {why}"
            );
        }
    }

    #[test]
    fn being_pointed_at_nothing_says_so_rather_than_guessing_at_ollama() {
        let db = db();
        pointed_at(&db, "offline", "http://localhost:11434");
        let why = configured(&db).expect_err("offline has no model server");
        assert!(
            why.contains("Settings"),
            "the refusal should say where to fix it: {why}"
        );
    }

    /// Rule 2, first half: the same boundary the rest of the program uses. "ollama" pointed
    /// at a rented box is a remote call wearing a local provider's name, and the sandbox
    /// does not get a tunnel to it.
    #[test]
    fn a_local_provider_pointed_at_the_internet_is_refused() {
        let db = db();
        pointed_at(&db, "ollama", "http://models.example.com:11434");
        let why = configured(&db).expect_err("a public endpoint is not a local server");
        assert!(
            why.contains("local network"),
            "the refusal should name the boundary: {why}"
        );
        // And the reason it is refused is the policy that would otherwise have no say.
        assert!(
            why.contains("network policy"),
            "the refusal should say where such traffic does go: {why}"
        );
    }

    /// Rule 2, second half. The name check and the address check are different questions,
    /// and this is the case that proves the second one is asked: a loopback name resolves to
    /// a loopback address and is allowed, while a public address is refused even though
    /// nothing about the name said so.
    #[test]
    fn the_address_a_name_resolves_to_is_checked_as_well_as_the_name() {
        let local = Endpoint {
            host: "localhost".to_string(),
            port: 11434,
            prefix: String::new(),
        };
        let address = dial_address(&local).expect("localhost resolves to a local address");
        assert!(
            crate::local_only::is_local_ip(address.ip()),
            "{address} should be local"
        );

        // 93.184.216.34 is a public address, reached here as an address so the test needs no
        // DNS: the name check above would not have caught it either way, which is the point.
        let public = Endpoint {
            host: "93.184.216.34".to_string(),
            port: 11434,
            prefix: String::new(),
        };
        let why = dial_address(&public).expect_err("a public address is not opened");
        assert!(
            why.contains("local network"),
            "the refusal should name the boundary: {why}"
        );
    }

    /// The relay's port is the engine's base URL, and the proxy's is a different door. Two
    /// bridges sharing a port inside the box would send model traffic through the host
    /// policy and build traffic to the model server.
    #[test]
    fn the_two_doors_into_the_box_are_not_the_same_door() {
        assert_ne!(RELAY_PORT, crate::code_proxy::RELAY_PORT);
        assert_ne!(SOCKET_IN_SANDBOX, crate::code_proxy::SOCKET_IN_SANDBOX);
    }

    #[test]
    fn a_url_with_no_port_and_no_known_scheme_is_refused_rather_than_guessed() {
        let why = parse("tcp://localhost").expect_err("no port to dial");
        assert!(why.contains("no port"), "{why}");
    }

    #[test]
    fn a_trailing_slash_does_not_become_part_of_the_prefix() {
        assert_eq!(
            parse("http://localhost:1234/v1/").expect("parses").prefix,
            "/v1"
        );
        assert_eq!(parse("http://localhost:11434/").expect("parses").prefix, "");
    }
    /// The whole point, proved through the real `code_sandbox::command` rather than against
    /// a description of it: a stand-in model server on the host's loopback, a command inside
    /// a box that has no route to it, and the socket as the only thing in between.
    ///
    /// Three answers in one run, because each is only meaningful next to the others:
    /// direct is refused, the model port answers, and the proxy port is not open -- the two
    /// doors are separate, and a box given one has not quietly been given both.
    ///
    /// Skips itself where it cannot be run honestly: no bubblewrap that can cut the network,
    /// no python3, or a test run with no built binary beside the harness to re-enter as the
    /// relay.
    #[cfg(unix)]
    #[test]
    fn inside_the_sandbox_the_model_socket_is_the_way_to_the_brain() {
        use std::io::{Read, Write};

        let sandbox = crate::code_sandbox::detect();
        if !sandbox.can_cut_network() {
            return;
        }
        // The relay is Aether1 itself; under `cargo test` the running binary is the test
        // harness, so the real one has to exist beside it or one directory up.
        let here = std::env::current_exe().ok();
        let found = here.iter().any(|exe| {
            exe.parent()
                .into_iter()
                .chain(exe.parent().and_then(Path::parent))
                .any(|dir| dir.join("aether1").is_file())
        });
        if !found {
            return;
        }
        let Some(python) = crate::paths::find_installed_binary(&["python3"]) else {
            return;
        };

        // A stand-in for Ollama on the host's loopback, which the box has no route to.
        let server = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = server.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            while let Ok((mut client, _)) = server.accept() {
                let mut buf = [0u8; 512];
                if client.read(&mut buf).is_ok() {
                    let _ =
                        client.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\n\r\nTHOUGHT");
                }
            }
        });

        let db = db();
        pointed_at(&db, "ollama", &format!("http://127.0.0.1:{port}"));
        let brain = start(&db).expect("the model socket starts");
        assert_eq!(
            brain.endpoint.in_box(),
            format!("http://127.0.0.1:{RELAY_PORT}")
        );

        let project =
            std::env::temp_dir().join(format!("aether1_model_e2e_{}", std::process::id()));
        std::fs::create_dir_all(&project).expect("project");

        let script = format!(
            "import socket,urllib.request\n\
             try:\n\
             \x20 socket.create_connection(('127.0.0.1',{port}),timeout=3)\n\
             \x20 print('DIRECT'+'-OK')\n\
             except OSError:\n\
             \x20 print('DIRECT'+'-BLOCKED')\n\
             try:\n\
             \x20 print('BRAIN'+'-'+urllib.request.urlopen('{base}/api/tags', \
             timeout=5).read().decode())\n\
             except Exception as e:\n\
             \x20 print('BRAIN'+'-FAILED:'+type(e).__name__)\n\
             try:\n\
             \x20 socket.create_connection(('127.0.0.1',{proxy_port}),timeout=2)\n\
             \x20 print('PROXY_PORT'+'-OPEN')\n\
             except OSError:\n\
             \x20 print('PROXY_PORT'+'-CLOSED')\n",
            base = brain.endpoint.in_box(),
            proxy_port = crate::code_proxy::RELAY_PORT,
        );

        let out = crate::code_sandbox::command(
            &sandbox,
            &python.to_string_lossy(),
            &["-c".to_string(), script],
            &project,
            &project,
            &crate::code_sandbox::Bridges {
                net: crate::code_sandbox::Net::None,
                model: Some(brain.socket.clone()),
            },
            &crate::code_sandbox::Access::project_only(),
        )
        .output()
        .expect("the sandbox runs");
        let spoken = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        let _ = std::fs::remove_dir_all(&project);

        assert!(
            spoken.contains("DIRECT-BLOCKED"),
            "the box must have no route to the host's loopback: {spoken}"
        );
        assert!(
            spoken.contains("BRAIN-THOUGHT"),
            "and must reach the model server through the socket: {spoken}"
        );
        assert!(
            spoken.contains("PROXY_PORT-CLOSED"),
            "a box given the model socket has not been given the network: {spoken}"
        );
    }
    /// The regression this is here for: a refusal was remembered, so the first attempt was
    /// the only attempt. An operator who gave AETHER CODE a task before starting their model
    /// server got that refusal from every task afterwards -- the settings were right by
    /// then, the server was up, and the answer never changed until AETHER1 was restarted.
    ///
    /// Proved in the order it happens to somebody: a name that resolves to nothing is
    /// refused, then the same database is pointed at a port that is really there and has to
    /// start. Skips itself if that first endpoint unexpectedly resolves, because a resolver
    /// that answers for `.local` would make this test pass for the wrong reason.
    #[cfg(unix)]
    #[test]
    fn a_refusal_is_not_remembered_the_way_a_running_socket_is() {
        let db = db();

        // `.local` passes the name half of rule 2, so this gets as far as resolution and
        // fails there -- which is exactly where an absent model server fails.
        pointed_at(
            &db,
            "ollama",
            "http://aether1-no-such-model-host.local:11434",
        );
        if start(&db).is_ok() {
            return;
        }

        // The server arrives, as it does when the operator starts Ollama and tries again.
        let server = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = server.local_addr().expect("addr").port();
        pointed_at(&db, "ollama", &format!("http://127.0.0.1:{port}"));
        let brain = start(&db).expect("the second attempt is a fresh question, not a cache hit");
        assert_eq!(brain.endpoint.port, port);
    }

    /// And the other half of that table: a forwarder that *is* running is handed back as it
    /// was, rather than a second one being started on a second path every time a task runs.
    #[cfg(unix)]
    #[test]
    fn a_running_socket_is_handed_back_rather_than_started_again() {
        let db = db();
        let server = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = server.local_addr().expect("addr").port();
        pointed_at(&db, "ollama", &format!("http://127.0.0.1:{port}"));

        let first = start(&db).expect("starts");
        let again = start(&db).expect("starts again");
        assert_eq!(first.socket, again.socket);
        assert!(first.socket.exists(), "and it is really there");
    }

    /// Local-only mode does not close this door, and that is deliberate rather than an
    /// oversight: reaching a model server on loopback or the LAN is the whole point of the
    /// platform, and the mode has always allowed it (see `scripts/check_egress_gate.sh`).
    /// A mode that stopped an agent thinking locally would be protecting nothing.
    #[test]
    fn local_only_mode_leaves_the_local_brain_reachable() {
        let db = db();
        pointed_at(&db, "ollama", "http://127.0.0.1:11434");
        db.set_setting(crate::local_only::SETTING, &serde_json::json!(true))
            .expect("local-only");
        assert!(
            crate::local_only::enabled(&db),
            "the mode has to be on for this test to mean anything"
        );
        configured(&db).expect("a local model server is what local-only mode exists to allow");
    }
}
