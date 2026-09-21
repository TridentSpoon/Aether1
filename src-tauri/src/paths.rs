// Where "home" is, and how to compare paths, on every platform this runs on.
//
// This exists because the answer differs and getting it wrong is silent. Unix sets HOME;
// Windows sets USERPROFILE and usually leaves HOME unset entirely, so code that reads only
// HOME does not fail on Windows -- it quietly decides the operator has no home directory,
// puts the memory vault in whatever the working directory happens to be, and refuses every
// file the operator asks about because nothing is inside an allowed root any more.

use std::path::PathBuf;

/// The operator's home directory.
///
/// HOME first, because on Unix it is the answer and on Windows it is what a Git Bash or
/// MSYS shell sets when it wants to be believed. Then the native Windows variables.
pub fn home_dir() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("HOME").filter(|h| !h.is_empty()) {
        return Some(PathBuf::from(home));
    }
    if let Some(profile) = std::env::var_os("USERPROFILE").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(profile));
    }
    // The last resort on older Windows: HOMEDRIVE ("C:") + HOMEPATH ("\Users\Trident").
    match (
        std::env::var_os("HOMEDRIVE").filter(|d| !d.is_empty()),
        std::env::var_os("HOMEPATH").filter(|p| !p.is_empty()),
    ) {
        (Some(drive), Some(path)) => {
            let mut joined = drive.to_string_lossy().to_string();
            joined.push_str(&path.to_string_lossy());
            Some(PathBuf::from(joined))
        }
        _ => None,
    }
}

/// Expands a leading `~` against the home directory. Left alone if there is no home to
/// expand against, so the caller sees a path that visibly did not resolve rather than one
/// silently rooted somewhere unexpected.
pub fn expand_home(path: &str) -> PathBuf {
    let trimmed = path.trim();
    if trimmed == "~" {
        return home_dir().unwrap_or_else(|| PathBuf::from(trimmed));
    }
    match trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"))
    {
        Some(rest) => match home_dir() {
            Some(home) => home.join(rest),
            None => PathBuf::from(trimmed),
        },
        None => PathBuf::from(trimmed),
    }
}

/// The Python environment Aether1 calls its own, which is where the wizard tells people to
/// put `faster-whisper` and, on Linux, `piper-tts`.
///
/// **It exists because a bare `pip install` is no longer a thing an operator can be told to
/// run.** Arch, Debian 12+, Ubuntu 23.04+, Fedora and Homebrew all mark their system Python
/// as externally managed (PEP 668), so `pip install faster-whisper` stops with
/// `error: externally-managed-environment` and a paragraph about virtual environments. The
/// override, `--break-system-packages`, does what it says and is never worth suggesting to
/// somebody who only wanted their companion to hear them.
///
/// A virtual environment is what the error message itself recommends, and putting it here
/// rather than somewhere the operator has to remember means two things: nothing is added to
/// PATH, and this code can find what was installed into it without being told where. It is
/// still the operator who creates it -- the wizard shows the command and says what it is
/// for. Aether1 does not install runtime dependencies behind anyone's back.
pub fn managed_python_env() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("Aether1").join("pyenv"))
    } else {
        home_dir().map(|home| {
            home.join(".local")
                .join("share")
                .join("aether1")
                .join("pyenv")
        })
    }
}

/// Where a venv keeps its executables: `Scripts` on Windows, `bin` everywhere else.
pub fn managed_python_bin_dir() -> Option<PathBuf> {
    let sub = if cfg!(target_os = "windows") {
        "Scripts"
    } else {
        "bin"
    };
    managed_python_env().map(|env| env.join(sub))
}

/// The interpreter inside that environment, if it has been created. None when it has not,
/// which is the normal state and not an error -- callers fall back to the system Python.
pub fn managed_python() -> Option<PathBuf> {
    let name = if cfg!(target_os = "windows") {
        "python.exe"
    } else {
        "python"
    };
    managed_python_bin_dir()
        .map(|bin| bin.join(name))
        .filter(|python| python.is_file())
}

/// Finds one of `names` as an executable: first on PATH (`which`), then directly inside
/// this platform's offline-installer bin directory (`%LOCALAPPDATA%\Aether1\bin` on
/// Windows, `~/.local/bin` on Linux/macOS -- exactly where aether1.iss / setup.sh's
/// scripts/offline_install_linux.sh put Piper and whisper-cli).
///
/// The second step exists because a per-user Windows install adds that directory to
/// `HKCU\Environment\Path` and broadcasts `WM_SETTINGCHANGE`, but an already-running
/// Explorer session does not reliably pick that up for processes it launches until the
/// next logon -- so a binary that is right there in the install directory can still be
/// invisible to a plain PATH lookup for as long as the operator has not logged out and
/// back in (or rebooted) since installing. Checking the well-known directory directly
/// means speech works immediately after install, not "after your next reboot".
pub fn find_installed_binary(names: &[&str]) -> Option<PathBuf> {
    for name in names {
        if let Ok(found) = which::which(name) {
            return Some(found);
        }
    }
    let exe_suffix = if cfg!(target_os = "windows") {
        ".exe"
    } else {
        ""
    };
    let installer_bin = if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("Aether1").join("bin"))
    } else {
        home_dir().map(|home| home.join(".local").join("bin"))
    };
    // PATH first, then the installer's directory, then Aether1's own Python environment --
    // a `pip install piper-tts` into that venv leaves `piper` in its bin directory and
    // nowhere else, which a PATH lookup can never see. Last of the three because a piper
    // the operator installed through their package manager is the one they chose.
    [installer_bin, managed_python_bin_dir()]
        .into_iter()
        .flatten()
        .flat_map(|dir| {
            names
                .iter()
                .map(|name| dir.join(format!("{name}{exe_suffix}")))
                .collect::<Vec<_>>()
        })
        .find(|candidate| candidate.is_file())
}

/// Where Windows is installed, from %SystemRoot% (or %WinDir%).
///
/// Read from the environment rather than hardcoded to `C:\\Windows`, because it is not
/// always C: -- a second OS on another volume, or an imaged machine, puts it elsewhere, and
/// a hardcoded guess would silently resolve to nothing on exactly the machines that are
/// hardest to debug. None on every other platform, and None on a Windows box with the
/// variable unset, which resolves to "this root contains nothing" rather than a wrong guess.
pub fn system_root() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    std::env::var_os("SystemRoot")
        .or_else(|| std::env::var_os("WinDir"))
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Stops a spawned console-subsystem process (PowerShell, Piper, whisper-cli) from
/// flashing a visible console window on Windows. This app has no console of its own
/// (`windows_subsystem = "windows"` in main.rs), so Windows' default for a console-mode
/// child spawned from a windowed parent is to allocate it a brand new console -- which
/// appears and disappears for every single synthesis/transcription call, exactly the
/// "a bunch of terminal windows that open then close" symptom. `CREATE_NO_WINDOW` (a Win32
/// `CreateProcess` flag) suppresses that allocation; it does nothing on other platforms, so
/// this is a no-op there rather than something call sites need to `#[cfg]` around.
#[cfg(target_os = "windows")]
pub fn suppress_console_window(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
pub fn suppress_console_window(_cmd: &mut std::process::Command) {}

/// Opens `path` in the OS's own file manager -- Explorer, Finder, or whichever handler
/// `xdg-open` resolves to on Linux. Fire-and-forget: this only has to confirm the file
/// manager *launched*, the same way a desktop icon double-click does not wait around for
/// the window it opened.
pub fn open_in_file_manager(path: &std::path::Path) -> Result<(), String> {
    let mut cmd = if cfg!(target_os = "windows") {
        std::process::Command::new("explorer")
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else {
        std::process::Command::new("xdg-open")
    };
    cmd.arg(path);
    suppress_console_window(&mut cmd);
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("could not open {}: {e}", path.display()))
}

/// A path as a lowercase, forward-slashed string, for comparing against the deny lists.
///
/// Windows paths arrive with backslashes and arbitrary case, so `"/.ssh/"` would never
/// match `C:\Users\Trident\.ssh\id_rsa` without this -- the deny list would be decorative
/// on the platform where it still matters exactly as much.
pub fn comparable(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Env vars are process-global, so these tests serialise and restore what they touch.
    fn with_env<T>(vars: &[(&str, Option<&str>)], body: impl FnOnce() -> T) -> T {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let saved: Vec<(String, Option<std::ffi::OsString>)> = vars
            .iter()
            .map(|(name, _)| (name.to_string(), std::env::var_os(name)))
            .collect();
        for (name, value) in vars {
            match value {
                Some(v) => std::env::set_var(name, v),
                None => std::env::remove_var(name),
            }
        }

        let result = body();

        for (name, value) in saved {
            match value {
                Some(v) => std::env::set_var(&name, v),
                None => std::env::remove_var(&name),
            }
        }
        result
    }

    #[test]
    fn home_wins_when_it_is_set() {
        with_env(
            &[
                ("HOME", Some("/home/operator")),
                ("USERPROFILE", Some(r"C:\Users\Operator")),
            ],
            || assert_eq!(home_dir().unwrap(), PathBuf::from("/home/operator")),
        );
    }

    #[test]
    fn userprofile_is_used_when_home_is_unset() {
        // The Windows default, and the case that was broken: HOME simply is not there.
        with_env(
            &[("HOME", None), ("USERPROFILE", Some(r"C:\Users\Operator"))],
            || {
                assert_eq!(home_dir().unwrap(), PathBuf::from(r"C:\Users\Operator"));
            },
        );
    }

    #[test]
    fn homedrive_and_homepath_are_the_last_resort() {
        with_env(
            &[
                ("HOME", None),
                ("USERPROFILE", None),
                ("HOMEDRIVE", Some("C:")),
                ("HOMEPATH", Some(r"\Users\Operator")),
            ],
            || assert_eq!(home_dir().unwrap(), PathBuf::from(r"C:\Users\Operator")),
        );
    }

    #[test]
    fn an_empty_variable_does_not_count_as_a_home() {
        with_env(
            &[
                ("HOME", Some("")),
                ("USERPROFILE", Some(r"C:\Users\Operator")),
            ],
            || assert_eq!(home_dir().unwrap(), PathBuf::from(r"C:\Users\Operator")),
        );
        with_env(
            &[
                ("HOME", Some("")),
                ("USERPROFILE", None),
                ("HOMEDRIVE", None),
                ("HOMEPATH", None),
            ],
            || assert!(home_dir().is_none()),
        );
    }

    #[test]
    fn tilde_expands_with_either_slash() {
        with_env(
            &[("HOME", Some("/home/operator")), ("USERPROFILE", None)],
            || {
                assert_eq!(
                    expand_home("~/notes.md"),
                    PathBuf::from("/home/operator/notes.md")
                );
                assert_eq!(
                    expand_home(r"~\notes.md"),
                    PathBuf::from("/home/operator/notes.md")
                );
                assert_eq!(expand_home("~"), PathBuf::from("/home/operator"));
                assert_eq!(expand_home("/etc/hostname"), PathBuf::from("/etc/hostname"));
            },
        );
    }

    #[test]
    fn comparison_normalises_windows_paths() {
        // Without this the deny lists match nothing on Windows.
        assert_eq!(
            comparable(std::path::Path::new(r"C:\Users\Trident\.ssh\id_rsa")),
            "c:/users/trident/.ssh/id_rsa"
        );
        assert!(comparable(std::path::Path::new(r"C:\Users\T\.ssh\x")).contains("/.ssh/"));
    }
}
