// Where the companion is allowed to look.
//
// Every filesystem tool resolves its argument through here first. The rules are
// deliberately blunt, because a subtle access-control policy is one nobody can hold in
// their head while reading a tool's source:
//
//   1. Resolve the path fully -- symlinks included -- before deciding anything. Judging a
//      path by its spelling is how `~/notes/../.ssh/id_rsa` and a symlink pointing at
//      /etc/shadow get through.
//   2. It must sit under an allowed root.
//   3. Unless it matches a denied pattern, which wins over any allowed root.
//
// Read-only is assumed throughout: nothing here grants write access, because nothing can
// write yet. When mutating tools arrive they get their own, narrower resolver rather than
// widening this one.

use std::path::{Path, PathBuf};

use crate::paths;

/// Roots the companion may read from. Everything else on the disk is invisible to it --
/// no /root, no other users' home directories, no arbitrary system paths.
///
/// The system roots are Unix-only on purpose. Their Windows counterparts (C:\Windows,
/// the registry hives, ProgramData) are not the sort of thing a companion needs to read to
/// answer a question, and the equivalent of "/etc tells you how this machine is
/// configured" simply is not a directory over there.
fn allowed_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = paths::home_dir() {
        roots.push(home);
    }
    if cfg!(unix) {
        roots.push(PathBuf::from("/etc"));
        roots.push(PathBuf::from("/proc"));
        roots.push(PathBuf::from("/var/log"));
    }
    roots
}

/// Path fragments that are refused wherever they appear, allowed root or not. Secrets the
/// operator would not expect a chat message to be able to extract, plus Aether1's own
/// database -- the companion reading its own memory file byte by byte is not a feature.
///
/// Matched against a lowercased, forward-slashed rendering of the path (see
/// paths::comparable), so these entries hold on Windows too.
const DENIED_FRAGMENTS: &[&str] = &[
    "/.ssh/",
    "/.gnupg/",
    "/.aws/",
    "/.azure/",
    "/.config/gh/",
    "/.docker/config.json",
    "/.netrc",
    "/etc/shadow",
    "/etc/gshadow",
    "/etc/sudoers",
    "/aether1_memory.db",
    // Windows: the DPAPI master keys that protect saved credentials, and the user's
    // registry hive, which holds a great deal more than it looks like it does.
    "/appdata/roaming/microsoft/protect/",
    "/appdata/roaming/microsoft/credentials/",
    "/appdata/local/microsoft/credentials/",
    "/ntuser.dat",
];

/// File names that are refused outright, anywhere.
const DENIED_NAMES: &[&str] = &[
    ".env",
    "id_rsa",
    "id_ed25519",
    ".netrc",
    ".pgpass",
    "ntuser.dat",
];

fn denied(path: &Path) -> bool {
    // A trailing slash so "/.ssh/" matches the directory itself as well as its contents.
    let as_text = format!("{}/", paths::comparable(path));
    if DENIED_FRAGMENTS
        .iter()
        .any(|fragment| as_text.contains(fragment))
    {
        return true;
    }
    path.file_name()
        .map(|name| {
            // Case-insensitively: Windows filesystems are, and "ID_RSA" is the same key.
            let name = name.to_string_lossy().to_lowercase();
            DENIED_NAMES.iter().any(|denied| name == *denied)
        })
        .unwrap_or(false)
}

/// Resolves `path` for reading, or explains why it can't be read. The error text reaches
/// the model (and the operator's action log), so it says which rule was hit rather than a
/// bare refusal -- a model told only "denied" tends to try again with a variation.
pub fn resolve_readable(path: &str) -> Result<PathBuf, String> {
    let expanded = paths::expand_home(path);

    // canonicalize resolves symlinks and `..`, and requires the path to exist -- both are
    // what make the containment check below meaningful.
    let resolved = expanded
        .canonicalize()
        .map_err(|e| format!("cannot access {}: {e}", expanded.display()))?;

    if denied(&resolved) {
        return Err(format!(
            "{} is off limits: it matches Aether1's list of paths that are never read (credentials, keys, and Aether1's own database)",
            resolved.display()
        ));
    }

    let roots = allowed_roots();
    if !roots.iter().any(|root| resolved.starts_with(root)) {
        let readable: Vec<String> = roots.iter().map(|r| r.display().to_string()).collect();
        return Err(format!(
            "{} is outside the paths Aether1 may read ({})",
            resolved.display(),
            readable.join(", ")
        ));
    }

    Ok(resolved)
}

/// Resolves `path` for writing, or explains why it can't be written.
///
/// Deliberately much narrower than reading. Reading /etc tells the companion how the
/// machine is configured; writing there changes how it boots. So writes are confined to
/// the operator's home directory, and the file's *parent* is what gets canonicalized --
/// the file itself may not exist yet, and a resolver that required it to exist could not
/// create anything.
pub fn resolve_writable(path: &str) -> Result<PathBuf, String> {
    let expanded = paths::expand_home(path);

    let Some(parent) = expanded.parent() else {
        return Err(format!("{} has no parent directory", expanded.display()));
    };
    let Some(name) = expanded.file_name() else {
        return Err(format!("{} does not name a file", expanded.display()));
    };

    // Canonicalizing the parent is what closes the symlink hole: a directory that links
    // out of home resolves to where it really points before containment is checked.
    let resolved_parent = parent
        .canonicalize()
        .map_err(|e| format!("cannot write into {}: {e}", parent.display()))?;
    let resolved = resolved_parent.join(name);

    if denied(&resolved) {
        return Err(format!(
            "{} is off limits: it matches Aether1's list of paths that are never touched",
            resolved.display()
        ));
    }

    let Some(home) = paths::home_dir() else {
        return Err(
            "no home directory could be found (neither HOME nor USERPROFILE is set)".to_string(),
        );
    };
    if !resolved.starts_with(&home) {
        return Err(format!(
            "{} is outside {}, and Aether1 only writes inside the operator's home directory",
            resolved.display(),
            home.display()
        ));
    }

    // An existing symlink is followed to its target, and the target is judged too --
    // otherwise a link inside home is a hole straight out of it.
    if resolved.is_symlink() {
        let target = resolved
            .canonicalize()
            .map_err(|e| format!("cannot resolve the symlink {}: {e}", resolved.display()))?;
        if denied(&target) || !target.starts_with(&home) {
            return Err(format!(
                "{} is a symlink pointing outside the writable area ({})",
                resolved.display(),
                target.display()
            ));
        }
        return Ok(target);
    }

    if resolved.is_dir() {
        return Err(format!("{} is a directory", resolved.display()));
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Point HOME at a scratch directory so these tests describe the rules rather than
    /// whatever happens to be in the runner's home.
    fn with_home<T>(body: impl FnOnce(&Path) -> T) -> T {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let home = std::env::temp_dir().join(format!("aether1_guard_{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        let previous_home = std::env::var_os("HOME");
        let previous_profile = std::env::var_os("USERPROFILE");
        std::env::set_var("HOME", &home);
        // Cleared as well: on a Windows runner it would otherwise be the answer, and these
        // tests would quietly be describing the developer's real home directory.
        std::env::remove_var("USERPROFILE");

        let result = body(&home);

        match previous_home {
            Some(p) => std::env::set_var("HOME", p),
            None => std::env::remove_var("HOME"),
        }
        match previous_profile {
            Some(p) => std::env::set_var("USERPROFILE", p),
            None => std::env::remove_var("USERPROFILE"),
        }
        result
    }

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
    }

    #[test]
    fn a_file_under_home_is_readable() {
        with_home(|home| {
            let note = home.join("notes.txt");
            write(&note, "hello");
            assert_eq!(
                resolve_readable(note.to_str().unwrap()).unwrap(),
                note.canonicalize().unwrap()
            );
            // and by its ~ spelling
            assert!(resolve_readable("~/notes.txt").is_ok());
        });
    }

    #[test]
    fn etc_is_readable_but_shadow_is_not() {
        assert!(resolve_readable("/etc/hostname").is_ok());
        // /etc/shadow may not exist in every environment; the rule is what's under test,
        // so only assert when there is a real file to refuse.
        if Path::new("/etc/shadow").exists() {
            let err = resolve_readable("/etc/shadow").unwrap_err();
            assert!(err.contains("off limits"), "{err}");
        }
    }

    #[test]
    fn traversal_out_of_home_is_refused_after_resolution() {
        with_home(|home| {
            let escape = format!("{}/../../../root", home.display());
            let err = resolve_readable(&escape).unwrap_err();
            // Either it doesn't exist or it's outside the roots; both are refusals, and
            // neither hands back a path.
            assert!(
                err.contains("outside the paths") || err.contains("cannot access"),
                "{err}"
            );
        });
    }

    #[test]
    fn a_symlink_pointing_out_of_bounds_is_refused() {
        with_home(|home| {
            let link = home.join("looks_innocent");
            let _ = std::fs::remove_file(&link);
            // /etc is an allowed root, so point somewhere that isn't: a denied file inside
            // one. The symlink's own name passes; its target must not.
            let target = home.join(".ssh/id_rsa");
            write(&target, "PRIVATE KEY");
            std::os::unix::fs::symlink(&target, &link).unwrap();

            let err = resolve_readable(link.to_str().unwrap()).unwrap_err();
            assert!(
                err.contains("off limits"),
                "a symlink must be judged by its target: {err}"
            );
        });
    }

    #[test]
    fn secrets_under_home_are_refused() {
        with_home(|home| {
            for path in [".ssh/id_rsa", ".aws/credentials", "project/.env", ".netrc"] {
                let full = home.join(path);
                write(&full, "secret");
                let err = resolve_readable(full.to_str().unwrap())
                    .expect_err(&format!("{path} should be refused"));
                assert!(err.contains("off limits"), "{path}: {err}");
            }
        });
    }

    #[test]
    fn the_memory_database_is_not_readable_as_a_file() {
        with_home(|home| {
            let db = home.join("backend/aether1_memory.db");
            write(&db, "sqlite");
            let err = resolve_readable(db.to_str().unwrap()).unwrap_err();
            assert!(err.contains("off limits"), "{err}");
        });
    }

    #[test]
    fn a_new_file_under_home_is_writable() {
        with_home(|home| {
            let target = home.join("notes/new.md");
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            assert!(resolve_writable(target.to_str().unwrap()).is_ok());
        });
    }

    #[test]
    fn writing_outside_home_is_refused_even_where_reading_is_allowed() {
        with_home(|_home| {
            // /etc is readable; it is not writable.
            let err = resolve_writable("/etc/hostname").unwrap_err();
            assert!(err.contains("only writes inside"), "{err}");
        });
    }

    #[test]
    fn writing_through_a_symlink_out_of_home_is_refused() {
        with_home(|home| {
            let link = home.join("escape.conf");
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink("/etc/hostname", &link).unwrap();
            let err = resolve_writable(link.to_str().unwrap()).unwrap_err();
            assert!(err.contains("pointing outside"), "{err}");
        });
    }

    #[test]
    fn writing_into_a_directory_that_links_out_of_home_is_refused() {
        with_home(|home| {
            let link_dir = home.join("linked_etc");
            let _ = std::fs::remove_file(&link_dir);
            std::os::unix::fs::symlink("/etc", &link_dir).unwrap();
            let err = resolve_writable(link_dir.join("newfile").to_str().unwrap()).unwrap_err();
            assert!(err.contains("only writes inside"), "{err}");
        });
    }

    #[test]
    fn secrets_are_not_writable_either() {
        with_home(|home| {
            std::fs::create_dir_all(home.join(".ssh")).unwrap();
            let err =
                resolve_writable(home.join(".ssh/authorized_keys").to_str().unwrap()).unwrap_err();
            assert!(err.contains("off limits"), "{err}");
        });
    }

    #[test]
    fn a_windows_home_is_found_through_userprofile() {
        // The bug this guards: on Windows HOME is usually unset, so a guard that reads
        // only HOME concludes there is no home directory, refuses every file the operator
        // asks about, and cannot write a note anywhere.
        let home = std::env::temp_dir().join(format!("aether1_winhome_{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        let note = home.join("notes.txt");
        write(&note, "hello");

        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous_home = std::env::var_os("HOME");
        let previous_profile = std::env::var_os("USERPROFILE");
        std::env::remove_var("HOME");
        std::env::set_var("USERPROFILE", &home);

        let readable = resolve_readable(note.to_str().unwrap());
        let writable = resolve_writable(home.join("new.md").to_str().unwrap());

        match previous_home {
            Some(p) => std::env::set_var("HOME", p),
            None => std::env::remove_var("HOME"),
        }
        match previous_profile {
            Some(p) => std::env::set_var("USERPROFILE", p),
            None => std::env::remove_var("USERPROFILE"),
        }

        assert!(
            readable.is_ok(),
            "USERPROFILE should locate home: {readable:?}"
        );
        assert!(writable.is_ok(), "and be writable: {writable:?}");
    }

    #[test]
    fn denied_paths_are_matched_with_windows_separators_and_case() {
        // A Windows path never contains "/.ssh/", and "ID_RSA" is the same private key.
        assert!(denied(Path::new(r"C:\Users\Trident\.ssh\id_rsa")));
        assert!(denied(Path::new(r"C:\Users\Trident\.aws\credentials")));
        assert!(denied(Path::new(r"C:\Users\Trident\NTUSER.DAT")));
        assert!(denied(Path::new(
            r"C:\Users\T\AppData\Roaming\Microsoft\Protect\key"
        )));
        assert!(denied(Path::new("/home/operator/.ssh/ID_RSA")));
        assert!(!denied(Path::new(r"C:\Users\Trident\notes.md")));
    }

    #[test]
    fn a_path_outside_every_root_is_refused() {
        let err = resolve_readable("/bin/sh").unwrap_err();
        assert!(err.contains("outside the paths"), "{err}");
    }
}
