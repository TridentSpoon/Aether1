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
    let bin_dir = if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("Aether1").join("bin"))
    } else {
        home_dir().map(|home| home.join(".local").join("bin"))
    }?;
    let exe_suffix = if cfg!(target_os = "windows") {
        ".exe"
    } else {
        ""
    };
    names
        .iter()
        .map(|name| bin_dir.join(format!("{name}{exe_suffix}")))
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
