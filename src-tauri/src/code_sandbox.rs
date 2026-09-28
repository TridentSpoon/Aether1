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
    let args = bwrap_args("/bin/true", &[], &dir, &dir, !network_off);
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
    network_for(&detect(), db)
}

/// The same question asked of a sandbox already in hand, so spawning a command does not
/// probe bubblewrap twice.
pub fn network_for(sandbox: &Sandbox, db: &MemoryDb) -> bool {
    db.get_setting_bool(NETWORK_SETTING, false) || !sandbox.can_cut_network()
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
    network: bool,
) -> Command {
    match sandbox {
        Sandbox::Unavailable(_) => {
            let mut cmd = Command::new(program);
            cmd.args(rest).current_dir(cwd);
            cmd
        }
        Sandbox::Bubblewrap { bwrap, .. } => {
            let mut cmd = Command::new(bwrap);
            for arg in bwrap_args(program, rest, cwd, root, network) {
                cmd.arg(arg);
            }
            // The sandbox's own environment, not this process's. `env_clear` covers what
            // bubblewrap's `--clearenv` would, and doing it here keeps the list in one
            // place and readable in a test.
            cmd.env_clear();
            for (key, value) in passthrough_env() {
                cmd.env(key, value);
            }
            cmd.current_dir(cwd);
            cmd
        }
    }
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
    network: bool,
) -> Vec<String> {
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
    if !network {
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

    // The one writable place, last so nothing above can hide it.
    let root_s = root.to_string_lossy().to_string();
    push(&["--bind", &root_s, &root_s]);

    let cwd_s = cwd.to_string_lossy().to_string();
    push(&["--chdir", &cwd_s]);

    push(&["--", program]);
    args.extend(rest.iter().cloned());
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_for(network: bool) -> Vec<String> {
        bwrap_args(
            "python3",
            &["-c".to_string(), "print(1)".to_string()],
            Path::new("/w/sub"),
            Path::new("/w"),
            network,
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
