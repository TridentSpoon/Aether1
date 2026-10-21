//! The box `code_workspace::run` spawns a command inside, and what it does when there
//! isn't one.
//!
//! This module exists because of a security review that got the previous answer exactly
//! right. `run` used to be described as running commands "in the project folder and
//! nowhere else", and the mechanism behind that sentence was
//! [`std::process::Command::current_dir`] plus a list of allowed program names. Neither is
//! a boundary. `current_dir` sets where a process *starts*, not what it may touch, and the
//! starter allowlist is a list of general-purpose interpreters: `python3` will read
//! `~/.ssh/id_ed25519` if it is asked to, `node -e` will open a socket, `cargo` runs
//! `build.rs`, `make` runs whatever the Makefile says. The allowlist is a good policy
//! layer -- it stops a model reaching for `curl` or `rm` by name -- and it was standing in
//! for a boundary it cannot be.
//!
//! So the promise is now kept by the operating system, not by a list:
//!
//!   * **Linux, with `bwrap` installed.** The command runs in a user, mount, PID, IPC, UTS
//!     and (by default) network namespace. The whole host filesystem is mounted read-only,
//!     the workspace is bind-mounted read-write on top, and `$HOME` is replaced by an empty
//!     tmpfs so SSH keys, browser profiles, cloud credentials and dotfiles are not merely
//!     unwritten but absent. The build caches a toolchain genuinely needs (`~/.cargo`,
//!     `~/.npm`, `~/.gradle` and the rest) are bound back in over that tmpfs, with the
//!     credential files that live inside them masked by `/dev/null`. The environment is
//!     cleared and rebuilt from a short list, so an API key in Aether1's own environment
//!     cannot be read by a build script.
//!   * **Anywhere else** -- Windows, a Linux box without bubblewrap, a platform this does
//!     not know -- there is no confinement, and `run` says so and refuses. The operator can
//!     switch `code_run_unconfined` on, which is a deliberate act with a sentence attached
//!     saying what it means. Fail closed, and never claim a boundary that isn't there.
//!
//! Windows deserves a real implementation (a restricted token or an AppContainer, with an
//! explicit ACL boundary) and does not have one yet. Until it does, the honest answer on
//! Windows is the refusal, not a weaker sandbox described in the same words as a strong one.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::llm::MemoryDb;

/// Whether the command may reach the network from inside the sandbox. Off by default: a
/// test suite does not need it, and a build script that wants it is the case worth seeing.
pub const NETWORK_SETTING: &str = "code_run_network";

/// Whether `run` is allowed at all on a machine with no sandbox available.
pub const UNCONFINED_SETTING: &str = "code_run_unconfined";

/// The `$HOME` subdirectories bound back in read-write over the tmpfs, because a toolchain
/// keeps its downloads there and a build with no cache is a build that fetches the internet
/// every time. Each is only bound if it already exists -- bubblewrap will not invent one.
const CACHE_DIRS: &[&str] = &[
    ".cargo",
    ".rustup",
    ".npm",
    ".cache",
    ".yarn",
    ".config/yarn",
    ".local/share/pnpm",
    ".bun",
    "go",
    ".gradle",
    ".m2",
    ".nuget",
    ".dotnet",
    ".pub-cache",
    ".ivy2",
    ".sbt",
];

/// Credential files that live *inside* those cache directories, and so survive the `$HOME`
/// tmpfs. Each is masked with `/dev/null`, which reads as an empty file: a registry token
/// is not something a test run needs, and publishing is the operator's to do.
const MASKED_IN_CACHES: &[&str] = &[
    ".cargo/credentials",
    ".cargo/credentials.toml",
    ".npmrc",
    ".gradle/gradle.properties",
    ".m2/settings.xml",
    ".nuget/NuGet/NuGet.Config",
];

/// Environment variables passed through unchanged. Everything else is dropped, so the
/// child sees nothing Aether1 happens to be holding -- an LLM API key above all.
const ENV_PASSTHROUGH: &[&str] = &[
    "PATH",
    "HOME",
    "LANG",
    "LC_ALL",
    "TERM",
    "TZ",
    "USER",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "GOPATH",
    "GOMODCACHE",
    "GOCACHE",
    "JAVA_HOME",
    "GRADLE_USER_HOME",
    "DOTNET_CLI_HOME",
    "npm_config_cache",
    "PYTHONDONTWRITEBYTECODE",
];

/// What the command may see of the filesystem beyond the read-only host: whether the
/// project folder itself is writable, and which other folders are bound in.
///
/// Decided by `code_policy` from the project's own file, and passed in rather than read here,
/// so this module keeps one job -- building a box -- and the question of what a project is
/// trusted with has one home.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Access {
    /// False at the Assistant level, where the agent reads and changes nothing.
    pub write_workspace: bool,
    pub extra: Vec<crate::code_policy::Mount>,
    /// The engine's own state directory on this machine, when the caller wants the command
    /// to keep anything between runs.
    ///
    /// Writable whatever the workspace is, and deliberately so: at the Assistant level the
    /// project is read-only, and an engine that cannot write its own session database cannot
    /// run there at all. This is AETHER1's state about the project rather than a file of it,
    /// and it is somewhere else entirely -- see `code_engine`. `None` for every command the
    /// operator or the companion starts, which is what keeps that true.
    pub engine_state: Option<PathBuf>,
}

impl Access {
    /// The ordinary case: the project folder, writable, and nothing else.
    pub fn project_only() -> Access {
        Access {
            write_workspace: true,
            extra: Vec::new(),
            engine_state: None,
        }
    }
}

/// How a command reaches the network, which is three states rather than a switch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Net {
    /// No network namespace but the sandbox's own, and nothing bridged into it. The box has
    /// a loopback interface and no route anywhere.
    #[default]
    None,
    /// The same isolation, plus one unix socket bind-mounted in. A relay started inside the
    /// box listens on loopback and hands every connection down that socket to Aether1's
    /// proxy, which allows or refuses it by host. A program that ignores `HTTPS_PROXY` does
    /// not get out by ignoring it -- there is nowhere for it to go.
    Proxied { socket: PathBuf },
    /// This machine will not let bubblewrap unshare a network namespace, so the box has the
    /// host's network whatever anyone sets. The proxy variables are still pointed at the
    /// relay, which well-behaved tools honour, but nothing enforces it and the Confinement
    /// line says so.
    Unenforced,
}

/// What is bridged into the box besides the filesystem: the way out, and the way to
/// AETHER1's own model server.
///
/// Two separate questions, deliberately. The network is the operator's switch and a per-host
/// policy. The model server is neither: it is this machine's own compute, it has to work
/// with the network switch off -- an engine that cannot think offline is not much of an
/// offline engine -- and the only thing it can reach is one address AETHER1 chose. Folding
/// them into one flag would mean turning the internet on to let an agent think.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bridges {
    pub net: Net,
    /// The model socket, when this command is one that may talk to the model server.
    ///
    /// `None` for everything the operator or the companion runs through
    /// `code_workspace::run`: a test suite has no business asking the brain questions, and a
    /// door that opens for one caller is one fewer thing to reason about. See `code_model`.
    pub model: Option<PathBuf>,
}

impl Bridges {
    /// The ordinary case: whatever the network settings say, and no model server.
    pub fn net_only(net: Net) -> Bridges {
        Bridges { net, model: None }
    }

    /// Whether anything inside the box needs the relay started in front of the command.
    fn needs_relay(&self) -> bool {
        matches!(self.net, Net::Proxied { .. }) || self.model.is_some()
    }
}

/// What this machine can actually do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sandbox {
    /// Real confinement, through bubblewrap at this path.
    Bubblewrap {
        bwrap: PathBuf,
        /// Whether a network namespace can be unshared here.
        ///
        /// Not every machine that runs bubblewrap will let it cut the network. Unsharing
        /// the network makes bubblewrap bring up a loopback interface in the new
        /// namespace, and a host whose policy forbids that -- a container, a CI runner, an
        /// AppArmor profile on unprivileged user namespaces -- fails the whole spawn with
        /// `Failed RTM_NEWADDR`. The filesystem confinement still works perfectly there, so
        /// the answer is to keep it, drop the part that cannot work, and say so, rather
        /// than either refusing to run or silently claiming an isolation that is not there.
        can_unshare_net: bool,
    },
    /// None available, with the reason to put in front of the operator.
    Unavailable(&'static str),
}

impl Sandbox {
    /// Whether commands run confined.
    pub fn confines(&self) -> bool {
        matches!(self, Sandbox::Bubblewrap { .. })
    }

    /// Whether the network can be cut off, whatever the operator's setting says. A machine
    /// that cannot unshare a network namespace reaches the network from inside the sandbox
    /// no matter what is switched off.
    pub fn can_cut_network(&self) -> bool {
        matches!(
            self,
            Sandbox::Bubblewrap {
                can_unshare_net: true,
                ..
            }
        )
    }

    /// One line for a settings row, `aether1 code perms`, or a refusal.
    pub fn description(&self) -> String {
        match self {
            Sandbox::Bubblewrap {
                bwrap,
                can_unshare_net,
            } => {
                let base = format!(
                    "confined by bubblewrap ({}): the host filesystem is read-only, your \
                     home folder is hidden, and only the project folder can be written",
                    bwrap.display()
                );
                if *can_unshare_net {
                    base
                } else {
                    format!(
                        "{base}. This machine will not let it cut the network, so commands \
                         can reach the network from inside it"
                    )
                }
            }
            Sandbox::Unavailable(why) => format!(
                "not confined: {why}. Commands would run as you, with access to everything \
                 your account can reach"
            ),
        }
    }
}

/// What is available here, asked freshly each time: bubblewrap can be installed while
/// Aether1 is running, and an operator who installs it to get the sandbox should not have
/// to restart to be given it.
///
/// Installed is not the same as working, so this does not stop at finding the binary. It
/// starts `true` inside the real argument list and looks at whether that succeeded, because
/// the failure modes here are all environmental -- a kernel with user namespaces turned off,
/// a container, a policy on loopback -- and every one of them would otherwise turn into a
/// confusing failure of the operator's first command rather than an honest line in Settings.
pub fn detect() -> Sandbox {
    // Cached for a short while. The probe spawns a process, `detect` is asked on every
    // command and by every settings read, and the answer changes only when somebody
    // installs or removes bubblewrap -- so a few seconds of memory turns a per-command
    // spawn into a per-minute one while still picking up an install without a restart.
    static CACHE: std::sync::Mutex<Option<(std::time::Instant, Sandbox)>> =
        std::sync::Mutex::new(None);
    const REMEMBER_FOR: std::time::Duration = std::time::Duration::from_secs(30);

    if let Ok(cache) = CACHE.lock() {
        if let Some((asked, answer)) = cache.as_ref() {
            if asked.elapsed() < REMEMBER_FOR {
                return answer.clone();
            }
        }
    }
    let answer = detect_uncached();
    if let Ok(mut cache) = CACHE.lock() {
        *cache = Some((std::time::Instant::now(), answer.clone()));
    }
    answer
}

fn detect_uncached() -> Sandbox {
    if !cfg!(target_os = "linux") {
        return Sandbox::Unavailable(
            "there is no sandbox for this platform yet (Windows needs a restricted token or \
             an AppContainer, which is not written)",
        );
    }
    let Some(bwrap) = crate::paths::find_installed_binary(&["bwrap"]) else {
        return Sandbox::Unavailable(
            "bubblewrap is not installed -- install it (apt install bubblewrap, pacman -S \
             bubblewrap) and commands will be confined",
        );
    };
    // The stricter arrangement first, so a machine that can have everything gets it.
    if probe(&bwrap, true) {
        return Sandbox::Bubblewrap {
            bwrap,
            can_unshare_net: true,
        };
    }
    if probe(&bwrap, false) {
        return Sandbox::Bubblewrap {
            bwrap,
            can_unshare_net: false,
        };
    }
    Sandbox::Unavailable(
        "bubblewrap is installed but will not start here -- usually unprivileged user \
         namespaces are disabled on this kernel or container",
    )
}

/// Starts `true` inside the real argument list, in a throwaway directory. The cheapest
/// possible question that has the same answer as "will the operator's next command run".
fn probe(bwrap: &Path, network_off: bool) -> bool {
    // A directory of this call's own. Named for the process *and* the thread and a
    // counter, because the probe's own cleanup would otherwise pull the ground out from
    // under another thread's probe -- which is exactly what happened in CI, where enough
    // tests ask at once for two to collide, and it read as "bubblewrap will not start
    // here".
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "aether1_sandbox_probe_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    if std::fs::create_dir_all(&dir).is_err() {
        return false;
    }
    let args = bwrap_args(
        "/bin/true",
        &[],
        &dir,
        &dir,
        &Bridges::net_only(if network_off {
            Net::None
        } else {
            Net::Unenforced
        }),
        &Access::project_only(),
    );
    let ok = Command::new(bwrap)
        .args(&args)
        .env_clear()
        .envs(passthrough_env())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    let _ = std::fs::remove_dir_all(&dir);
    ok
}

/// Whether the network is reachable from inside the sandbox.
///
/// The operator's switch, and the machine's own answer. A box that cannot cut the network
/// does not pretend to: asking for it to be off there would build an argument list that
/// fails to start at all, which is a worse answer than an honest description.
pub fn network_allowed(db: &MemoryDb) -> bool {
    db.get_setting_bool(NETWORK_SETTING, false)
}

/// How this command reaches the network: the operator's switch, what the machine can
/// enforce, and whether the proxy started.
///
/// The proxy is only started when the network is actually wanted, so an operator who never
/// turns it on never has a listening socket at all.
pub fn network_for(sandbox: &Sandbox, db: &MemoryDb) -> Net {
    if !sandbox.can_cut_network() {
        // Nothing to decide: the box has the host's network either way. The variables are
        // still pointed at the relay, so a tool that honours them goes through the policy;
        // `description` is where the honesty about that lives.
        return Net::Unenforced;
    }
    // Local-only mode outranks the sandbox's own network switch, for the same reason it
    // outranks the internet permission in `code_perms::check_url`: it is the operator's
    // stated "nothing leaves this machine", and a setting they left on months ago must not
    // quietly beat the switch whose entire job is to say otherwise.
    //
    // Without this the two disagreed in the worst direction. `code_effective`'s web row
    // reported "Local-only mode is on, so nothing reaches the internet" while a command in
    // the sandbox could still reach every host on the starter list -- github.com,
    // crates.io, registry.npmjs.org, pypi.org and the rest -- without being asked, because
    // the proxy's allow list is the only thing it consults and nothing in that path had
    // ever heard of local-only mode. A build fetching its dependencies is traffic leaving a
    // machine whose operator was told nothing would.
    //
    // Checked here rather than in `network_allowed` so the setting still reads back as what
    // the operator chose: the switch is not being rewritten, it is being overruled, and the
    // network row (which asks this function, not that setting) says so.
    //
    // The model bridge is deliberately untouched: it is a separate socket in `Bridges`, and
    // reaching a model server on this machine or this network is what local-only mode is
    // *for*.
    if crate::local_only::enabled(db) || !network_allowed(db) {
        return Net::None;
    }
    match crate::code_proxy::start(db) {
        Ok(proxy) => Net::Proxied {
            socket: proxy.socket.clone(),
        },
        // A proxy that will not start is not a reason to hand the box the whole network.
        Err(_) => Net::None,
    }
}

/// Whether the operator has said to run commands unconfined anyway.
pub fn unconfined_allowed(db: &MemoryDb) -> bool {
    db.get_setting_bool(UNCONFINED_SETTING, false)
}

/// The refusal a machine with no sandbox gets, which is also the whole explanation of what
/// switching it off would mean.
pub fn unconfined_refusal(sandbox: &Sandbox) -> String {
    format!(
        "run is off because this machine cannot confine it: {}.\n\n\
         The programs on the allowlist -- python3, node, cargo, make and the rest -- are \
         general-purpose ways to execute code, so without an OS-level sandbox a command \
         can read, write and send anything your account can. The allowlist narrows what \
         gets started; it cannot narrow what a started program does.\n\n\
         To run commands anyway, knowing that: `aether1 code run-unconfined on`, or the \
         matching switch in Settings.",
        match sandbox {
            Sandbox::Unavailable(why) => *why,
            // Only reachable if a caller asks for this while confinement is available.
            Sandbox::Bubblewrap { .. } => "…it can, and this message is a bug",
        }
    )
}

/// The command to spawn, either wrapped in a sandbox or bare.
///
/// `root` is the workspace, the one writable place. `cwd` is where the command starts, which
/// `code_workspace` has already proved is inside `root`.
pub fn command(
    sandbox: &Sandbox,
    program: &str,
    rest: &[String],
    cwd: &Path,
    root: &Path,
    bridges: &Bridges,
    access: &Access,
) -> Command {
    let net = &bridges.net;
    match sandbox {
        Sandbox::Unavailable(_) => {
            let mut cmd = Command::new(program);
            cmd.args(rest).current_dir(cwd);
            cmd
        }
        Sandbox::Bubblewrap { bwrap, .. } => {
            let mut cmd = Command::new(bwrap);
            // With a socket bridged in the command is wrapped: Aether1 itself runs inside
            // the box, listens on loopback in front of each socket it was given, and then
            // runs what was asked for. Two processes rather than a shell line, so nothing in
            // the argv is ever parsed. Each bridge is named, so the relay opens exactly the
            // doors this box has and no others.
            let (program, rest) = if bridges.needs_relay() {
                let me = aether1_binary().to_string_lossy().to_string();
                let mut wrapped = vec!["--net-relay".to_string()];
                if matches!(net, Net::Proxied { .. }) {
                    wrapped.push("--net-socket".to_string());
                    wrapped.push(crate::code_proxy::SOCKET_IN_SANDBOX.to_string());
                }
                if bridges.model.is_some() {
                    wrapped.push("--model-socket".to_string());
                    wrapped.push(crate::code_model::SOCKET_IN_SANDBOX.to_string());
                }
                wrapped.push("--".to_string());
                wrapped.push(program.to_string());
                wrapped.extend(rest.iter().cloned());
                (me, wrapped)
            } else {
                (program.to_string(), rest.to_vec())
            };
            for arg in bwrap_args(&program, &rest, cwd, root, bridges, access) {
                cmd.arg(arg);
            }
            // The sandbox's own environment, not this process's. `env_clear` covers what
            // bubblewrap's `--clearenv` would, and doing it here keeps the list in one
            // place and readable in a test.
            cmd.env_clear();
            for (key, value) in passthrough_env() {
                cmd.env(key, value);
            }
            // Both spellings, because build tools are split on which they read.
            if !matches!(net, Net::None) {
                let proxy = format!("http://127.0.0.1:{}", crate::code_proxy::RELAY_PORT);
                for key in ["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy"] {
                    cmd.env(key, &proxy);
                }
                // Node's built-in `fetch` is the one common client that reads none of the
                // four above unless told to, so inside a proxied box it resolves the host
                // itself, gets nowhere, and reports `EAI_AGAIN` -- which reads to an
                // operator as "this machine is offline" rather than "your tool ignored the
                // only way out". curl, npm and python all honour the variables, which is
                // why it goes unnoticed. Harmless where it is not understood.
                cmd.env("NODE_USE_ENV_PROXY", "1");
            }
            // The engine's roots, named as the box sees them. Set only when the directory is
            // actually bound in: an `XDG_DATA_HOME` pointing into a tmpfs that goes when the
            // command returns would be worse than not setting it, because the engine would
            // believe it had somewhere to keep things.
            if access.engine_state.is_some() {
                let home = std::env::var("HOME").unwrap_or_default();
                if !home.is_empty() {
                    for (key, value) in crate::code_engine::env_in_sandbox(&home) {
                        cmd.env(key, value);
                    }
                }
            }
            cmd.current_dir(cwd);
            cmd
        }
    }
}

/// Aether1's own binary, which is what runs inside the box as the relay.
///
/// `current_exe` almost always answers this. The exception is a test run, where the running
/// executable is the test harness in `target/debug/deps/` -- and a test that could not
/// start the relay would be a test of everything except the part that matters, so the
/// binary beside it is used instead.
fn aether1_binary() -> PathBuf {
    let Ok(exe) = std::env::current_exe() else {
        return PathBuf::from("aether1");
    };
    if exe.parent().and_then(Path::file_name) == Some(std::ffi::OsStr::new("deps")) {
        if let Some(beside) = exe
            .parent()
            .and_then(Path::parent)
            .map(|dir| dir.join("aether1"))
            .filter(|path| path.is_file())
        {
            return beside;
        }
    }
    exe
}

/// The environment the child is given: the passthrough list, as this process has it.
fn passthrough_env() -> BTreeMap<String, String> {
    ENV_PASSTHROUGH
        .iter()
        .filter_map(|key| std::env::var(key).ok().map(|v| ((*key).to_string(), v)))
        .collect()
}

/// Every bubblewrap argument, in the order they must appear: the mounts are applied in
/// sequence, so the read-only host comes first, the `$HOME` tmpfs over it, the caches back
/// over that, and the workspace last of all -- otherwise a project inside `$HOME` would be
/// hidden by the tmpfs meant to hide everything around it.
///
/// Split out from [`command`] so a test can read the arguments without needing bubblewrap
/// installed to look at them.
pub fn bwrap_args(
    program: &str,
    rest: &[String],
    cwd: &Path,
    root: &Path,
    bridges: &Bridges,
    access: &Access,
) -> Vec<String> {
    let net = &bridges.net;
    let home = std::env::var("HOME").unwrap_or_default();
    let mut args: Vec<String> = Vec::new();
    let mut push = |parts: &[&str]| args.extend(parts.iter().map(|s| (*s).to_string()));

    // Namespaces. Everything is unshared; the network is put back only when asked, which
    // is the one of these that a real build sometimes needs.
    push(&[
        "--unshare-user",
        "--unshare-ipc",
        "--unshare-pid",
        "--unshare-uts",
        "--unshare-cgroup-try",
    ]);
    if !matches!(net, Net::Unenforced) {
        push(&["--unshare-net"]);
    }
    // A process group of its own, so the child cannot push characters back into the
    // terminal Aether1 was started from, and dies with the parent rather than outliving it.
    push(&["--new-session", "--die-with-parent"]);

    // The host, readable so a toolchain works, writable nowhere.
    push(&["--ro-bind", "/", "/"]);
    push(&["--dev", "/dev", "--proc", "/proc"]);
    push(&["--tmpfs", "/tmp"]);

    // Home: gone, then the caches back.
    if !home.is_empty() {
        push(&["--tmpfs", &home]);
        for dir in CACHE_DIRS {
            let path = format!("{home}/{dir}");
            if Path::new(&path).is_dir() {
                push(&["--bind-try", &path, &path]);
            }
        }
        for file in MASKED_IN_CACHES {
            let path = format!("{home}/{file}");
            push(&["--ro-bind-try", "/dev/null", &path]);
        }
    }

    // The relay binary, bound back over the tmpfs that has just hidden it.
    //
    // When a socket is bridged in, the program the box runs is AETHER1 itself (see
    // `command`), and a very ordinary install puts AETHER1 under the operator's own home
    // directory -- `~/.local/bin/aether1`, a build tree in `~/src`, an AppImage in
    // `~/Applications`. The `--tmpfs` above hides all of it, so `bwrap` could not execute
    // the one program it had been told to run and failed with
    // `execvp ...: No such file or directory`: not a message about the model server, and not
    // one that names the home directory it is really about. Every command with a bridge
    // refused, on exactly the machines where AETHER1 was not installed system-wide.
    //
    // `~/.cargo` and the other cache directories come back above, which is why a
    // `cargo install`ed AETHER1 worked and hid how general the fault was.
    //
    // Read-only, and only for a box that is already running this binary as its entry point:
    // it is the program the kernel is about to execute either way, so this grants the box
    // nothing it did not already have. Placed after the caches so a cache bind cannot hide
    // it, and before the workspace so a project containing it still wins.
    if bridges.needs_relay() && !home.is_empty() {
        let relay = Path::new(program);
        if relay.is_absolute() && relay.starts_with(&home) {
            push(&["--ro-bind-try", program, program]);
        }
    }

    // The engine's own directory, after the caches so a cache path cannot hide it and
    // before the workspace so the workspace still wins. Writable regardless of
    // `write_workspace`: it is not part of the project. See `code_engine`.
    if let (Some(state), false) = (&access.engine_state, home.is_empty()) {
        push(&[
            "--bind",
            &state.to_string_lossy(),
            &format!("{home}/{}", crate::code_engine::DIR_NAME),
        ]);
    }

    // The way out, if there is one: a socket, not a route. It crosses the network
    // namespace because it is a file.
    if let Net::Proxied { socket } = net {
        push(&[
            "--bind",
            &socket.to_string_lossy(),
            crate::code_proxy::SOCKET_IN_SANDBOX,
        ]);
    }

    // The way to AETHER1's own model server, which is a second socket and not the same
    // door: this one carries no decision, because its single destination was fixed before
    // the box existed. See `code_model`.
    if let Some(socket) = &bridges.model {
        push(&[
            "--bind",
            &socket.to_string_lossy(),
            crate::code_model::SOCKET_IN_SANDBOX,
        ]);
    }

    // The folders the project named, before the workspace so the workspace still wins if
    // one of them contains it. Each is read-only unless the level said otherwise, which is
    // what makes "read my notes" a different answer from "reorganise my notes".
    for mount in &access.extra {
        let path = mount.path.to_string_lossy().to_string();
        push(&[
            if mount.write {
                "--bind-try"
            } else {
                "--ro-bind-try"
            },
            &path,
            &path,
        ]);
    }

    // The workspace, last so nothing above can hide it. Read-only at the Assistant level:
    // the agent can look at the project and run its tests, and cannot change it.
    let root_s = root.to_string_lossy().to_string();
    push(&[
        if access.write_workspace {
            "--bind"
        } else {
            "--ro-bind"
        },
        &root_s,
        &root_s,
    ]);

    let cwd_s = cwd.to_string_lossy().to_string();
    push(&["--chdir", &cwd_s]);

    push(&["--", program]);
    args.extend(rest.iter().cloned());
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> MemoryDb {
        let path = std::env::temp_dir().join(format!(
            "aether1_code_sandbox_{}_{:?}.db",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&path);
        MemoryDb::open(&path).expect("open test db")
    }

    /// The hole this closes: the sandbox had its own network switch and local-only mode was
    /// not one of the things it asked. An operator who turned the sandbox network on months
    /// ago to let builds fetch dependencies, and local-only mode on this morning, got a web
    /// row reading "nothing reaches the internet" and a `cargo build` reaching crates.io.
    ///
    /// Asserted through `network_for` rather than `network_allowed`, because that is the
    /// question every caller actually asks -- including `code_effective`'s network row,
    /// which is why the report now agrees with the behaviour instead of contradicting it.
    #[test]
    fn local_only_mode_cuts_the_sandbox_network_whatever_the_switch_says() {
        let db = test_db();
        db.set_setting(NETWORK_SETTING, &serde_json::json!(true))
            .unwrap();
        db.set_setting(crate::local_only::SETTING, &serde_json::json!(true))
            .unwrap();

        // The operator's own switch reads back as they left it -- it is overruled, not
        // rewritten, and a report that showed it as off would be lying in the other
        // direction.
        assert!(
            network_allowed(&db),
            "the sandbox network setting is still what the operator chose"
        );

        // A box that cannot cut the network is a different conversation (see Net::Unenforced
        // and the row that owns up to it), so this pins the case where enforcement is real.
        let sandbox = Sandbox::Bubblewrap {
            bwrap: PathBuf::from("/usr/bin/bwrap"),
            can_unshare_net: true,
        };
        assert!(
            matches!(network_for(&sandbox, &db), Net::None),
            "local-only mode outranks the sandbox's network switch"
        );
    }

    fn args_for(network: bool) -> Vec<String> {
        args_for_net(if network {
            &Net::Unenforced
        } else {
            &Net::None
        })
    }

    fn args_for_net(net: &Net) -> Vec<String> {
        args_for_bridges(&Bridges::net_only(net.clone()))
    }

    fn args_for_bridges(bridges: &Bridges) -> Vec<String> {
        bwrap_args(
            "python3",
            &["-c".to_string(), "print(1)".to_string()],
            Path::new("/w/sub"),
            Path::new("/w"),
            bridges,
            &Access::project_only(),
        )
    }

    /// The four properties the whole module exists for, read straight off the argument
    /// list. This runs on a machine with no bubblewrap, and on Windows.
    #[test]
    fn the_arguments_say_read_only_host_writable_workspace_hidden_home_no_network() {
        let args = args_for(false);
        let joined = args.join(" ");
        assert!(joined.contains("--ro-bind / /"), "{joined}");
        assert!(joined.contains("--bind /w /w"), "{joined}");
        assert!(joined.contains("--unshare-net"), "{joined}");
        assert!(joined.contains("--chdir /w/sub"), "{joined}");
        assert!(args.ends_with(&[
            "python3".to_string(),
            "-c".to_string(),
            "print(1)".to_string()
        ]));

        // The workspace bind comes after every mount that could hide it.
        let workspace = args.iter().position(|a| a == "/w").expect("workspace bind");
        let ro_host = args
            .iter()
            .position(|a| a == "--ro-bind")
            .expect("host bind");
        assert!(
            ro_host < workspace,
            "the host must be mounted before the workspace"
        );
    }

    /// The shape the whole proxy rests on: a box that can reach the network through the
    /// proxy still has no network namespace of its own to route from. The socket is the
    /// only way out, and it is a file rather than a route, which is why it crosses.
    #[test]
    fn a_proxied_box_keeps_its_network_namespace_and_gets_only_a_socket() {
        let args = args_for_net(&Net::Proxied {
            socket: PathBuf::from("/var/aether1/net.sock"),
        });
        let joined = args.join(" ");
        assert!(
            joined.contains("--unshare-net"),
            "a proxied box is still cut off from the network: {joined}"
        );
        assert!(
            joined.contains(&format!(
                "--bind /var/aether1/net.sock {}",
                crate::code_proxy::SOCKET_IN_SANDBOX
            )),
            "and reaches the proxy through one socket: {joined}"
        );
    }

    /// A box with a way out is no use to a tool that cannot see it. Node's `fetch` reads
    /// `NODE_USE_ENV_PROXY`, not the proxy variables, so without this the Proxied and
    /// Unenforced states look offline to it while the proxy sits there waiting.
    #[test]
    fn a_box_with_a_way_out_points_node_at_it_too() {
        let root = std::env::temp_dir();
        let sandbox = Sandbox::Bubblewrap {
            bwrap: PathBuf::from("/usr/bin/bwrap"),
            can_unshare_net: true,
        };
        for net in [
            Net::Proxied {
                socket: PathBuf::from("/var/aether1/net.sock"),
            },
            Net::Unenforced,
        ] {
            let cmd = command(
                &sandbox,
                "node",
                &[],
                &root,
                &root,
                &Bridges::net_only(net.clone()),
                &Access::default(),
            );
            let env: Vec<_> = cmd.get_envs().collect();
            assert!(
                env.contains(&(
                    std::ffi::OsStr::new("NODE_USE_ENV_PROXY"),
                    Some(std::ffi::OsStr::new("1"))
                )),
                "{net:?} sets the proxy variables, so it must set this one too: {env:?}"
            );
        }
    }

    /// The other half of the rule above: a box with no way out must not claim one. A
    /// variable pointing at a relay that is not there would have Node reporting a proxy
    /// failure where the honest answer is that the network is off.
    #[test]
    fn a_box_with_no_way_out_claims_none() {
        let root = std::env::temp_dir();
        let sandbox = Sandbox::Bubblewrap {
            bwrap: PathBuf::from("/usr/bin/bwrap"),
            can_unshare_net: true,
        };
        let cmd = command(
            &sandbox,
            "node",
            &[],
            &root,
            &root,
            &Bridges::default(),
            &Access::default(),
        );
        let keys: Vec<_> = cmd.get_envs().map(|(k, _)| k).collect();
        for key in [
            "NODE_USE_ENV_PROXY",
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "http_proxy",
            "https_proxy",
        ] {
            assert!(
                !keys.contains(&std::ffi::OsStr::new(key)),
                "a box with no network must not set {key}"
            );
        }
    }

    /// The model socket is bridged in the same way the proxy's is -- a file, which crosses
    /// a network namespace -- and the box still has no route. See `code_model`.
    #[test]
    fn the_model_socket_crosses_into_a_box_that_still_has_no_network() {
        let args = args_for_bridges(&Bridges {
            net: Net::None,
            model: Some(PathBuf::from("/var/aether1/model.sock")),
        });
        let joined = args.join(" ");
        assert!(
            joined.contains("--unshare-net"),
            "the model socket is not a network: {joined}"
        );
        assert!(
            joined.contains(&format!(
                "--bind /var/aether1/model.sock {}",
                crate::code_model::SOCKET_IN_SANDBOX
            )),
            "and the socket is bound in: {joined}"
        );
        assert!(
            !joined.contains(crate::code_proxy::SOCKET_IN_SANDBOX),
            "a box given the brain has not been given the network: {joined}"
        );
    }

    /// An AETHER1 installed inside the operator's own home directory is still executable
    /// inside the box.
    ///
    /// The failure this closes took out every bridged command on an ordinary install.
    /// `command` runs AETHER1 itself as the box's entry point whenever a socket is bridged,
    /// and `$HOME` is replaced by a tmpfs -- so an AETHER1 at `~/.local/bin/aether1`, or in a
    /// build tree under `~`, was hidden from the one program the box had been told to run.
    /// `bwrap` reported `execvp ...: No such file or directory`, which says nothing about the
    /// home directory it is really about and nothing about the model server the operator was
    /// trying to reach. `~/.cargo` comes back with the caches, so a `cargo install`ed AETHER1
    /// worked and hid how general the fault was. CI met it as three sandbox tests failing,
    /// because a GitHub runner checks the repository out underneath `/home/runner`.
    #[test]
    fn an_aether1_inside_the_home_directory_is_still_runnable_in_the_box() {
        let home = std::env::var("HOME").unwrap_or_default();
        if home.is_empty() {
            return;
        }
        let root = std::env::temp_dir();
        let inside = format!("{home}/.local/bin/aether1");
        let model = Some(PathBuf::from("/var/aether1/model.sock"));

        let args = bwrap_args(
            &inside,
            &[],
            &root,
            &root,
            &Bridges {
                net: Net::None,
                model: model.clone(),
            },
            &Access::project_only(),
        );

        // The tmpfs hides it, so the bind that brings it back has to come after.
        let tmpfs = args
            .windows(2)
            .position(|w| w[0] == "--tmpfs" && w[1] == home)
            .expect("the home tmpfs");
        let bound = args
            .windows(3)
            .position(|w| w[0] == "--ro-bind-try" && w[1] == inside && w[2] == inside)
            .unwrap_or_else(|| panic!("the relay binary should be bound back: {args:?}"));
        assert!(
            tmpfs < bound,
            "and after the tmpfs, or it would be hidden again: {args:?}"
        );

        // Read-only: the box runs this binary, it does not get to rewrite it.
        assert!(
            !args
                .windows(2)
                .any(|w| (w[0] == "--bind" || w[0] == "--bind-try") && w[1] == inside),
            "the relay binary is never writable: {args:?}"
        );

        // A box with nothing bridged runs the operator's program directly, so there is no
        // relay binary to bind and nothing extra is opened up.
        let plain = bwrap_args(
            &inside,
            &[],
            &root,
            &root,
            &Bridges::default(),
            &Access::project_only(),
        );
        // It is still the program at the end of the argv -- that is what the box runs. What
        // must not be there is the bind, because without a bridge the box runs the
        // operator's own program and AETHER1 is not its entry point.
        assert!(
            !plain
                .windows(3)
                .any(|w| w[0] == "--ro-bind-try" && w[1] == inside && w[2] == inside),
            "nothing bridged means no relay, so nothing to bind back: {plain:?}"
        );
    }

    /// The relay is told which doors to open, one flag per socket, so a box with one bridge
    /// does not get a port with nothing behind it -- and a box with neither is not wrapped
    /// at all, which is how `code run` has always worked with the network off.
    #[test]
    fn the_relay_opens_exactly_the_doors_the_box_has() {
        let root = std::env::temp_dir();
        let sandbox = Sandbox::Bubblewrap {
            bwrap: PathBuf::from("/usr/bin/bwrap"),
            can_unshare_net: true,
        };
        let argv_for = |bridges: &Bridges| -> String {
            let cmd = command(
                &sandbox,
                "node",
                &[],
                &root,
                &root,
                bridges,
                &Access::default(),
            );
            cmd.get_args()
                .map(|a| a.to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join(" ")
        };

        let net = Net::Proxied {
            socket: PathBuf::from("/var/aether1/net.sock"),
        };
        let model = Some(PathBuf::from("/var/aether1/model.sock"));

        let both = argv_for(&Bridges {
            net: net.clone(),
            model: model.clone(),
        });
        assert!(both.contains("--net-relay"), "{both}");
        assert!(both.contains("--net-socket"), "{both}");
        assert!(both.contains("--model-socket"), "{both}");

        let brain_only = argv_for(&Bridges {
            net: Net::None,
            model,
        });
        assert!(brain_only.contains("--net-relay"), "{brain_only}");
        assert!(brain_only.contains("--model-socket"), "{brain_only}");
        assert!(
            !brain_only.contains("--net-socket"),
            "no proxy, so no proxy port: {brain_only}"
        );

        let network_only = argv_for(&Bridges { net, model: None });
        assert!(network_only.contains("--net-socket"), "{network_only}");
        assert!(
            !network_only.contains("--model-socket"),
            "no brain, so no brain port: {network_only}"
        );

        let neither = argv_for(&Bridges::default());
        assert!(
            !neither.contains("--net-relay"),
            "nothing to bridge means the command is run directly: {neither}"
        );
    }

    #[test]
    fn the_network_comes_back_only_when_asked() {
        assert!(!args_for(true).iter().any(|a| a == "--unshare-net"));
    }

    /// `HOME` is replaced before the caches are bound back, and the masks come last of the
    /// three, so a token inside a bound cache is still hidden.
    #[test]
    fn home_is_hidden_before_anything_is_bound_back() {
        let home = std::env::var("HOME").unwrap_or_default();
        if home.is_empty() {
            return;
        }
        let args = args_for(false);
        let tmpfs = args
            .windows(2)
            .position(|w| w[0] == "--tmpfs" && w[1] == home)
            .expect("home tmpfs");
        let masked = args
            .iter()
            .position(|a| a.starts_with(&format!("{home}/.cargo/credentials")))
            .expect("masked credential");
        assert!(tmpfs < masked);
    }

    /// Nothing but the passthrough list reaches the child -- the test that stops an API key
    /// in Aether1's environment from being readable by a build script.
    #[test]
    fn the_environment_is_rebuilt_from_a_short_list() {
        std::env::set_var("AETHER1_TEST_SECRET", "sk-not-for-build-scripts");
        assert!(!passthrough_env().contains_key("AETHER1_TEST_SECRET"));
        std::env::remove_var("AETHER1_TEST_SECRET");
    }

    /// A box that cannot cut the network says so in the same line that says it confines,
    /// because an operator reading "confined" should not have to ask a second question.
    #[test]
    fn a_box_that_cannot_cut_the_network_says_that_too() {
        let limited = Sandbox::Bubblewrap {
            bwrap: PathBuf::from("/usr/bin/bwrap"),
            can_unshare_net: false,
        };
        assert!(limited.confines());
        assert!(!limited.can_cut_network());
        assert!(limited.description().contains("cut the network"));
    }

    #[test]
    fn an_unavailable_sandbox_says_so_plainly() {
        let none = Sandbox::Unavailable("bubblewrap is not installed");
        assert!(!none.confines());
        assert!(none.description().contains("not confined"));
        assert!(unconfined_refusal(&none).contains("bubblewrap is not installed"));
    }
}
