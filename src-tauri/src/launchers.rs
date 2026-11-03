//! The launcher entries that start AETHER1, and the one way they go wrong.
//!
//! A `.desktop` file in `~/.local/share/applications` is a *path* written down. Nothing
//! keeps it true. Move the program, rename it, let an AppImage manager take the file into
//! its own directory, remove one install and keep another -- the entry still sits in the
//! menu, still has the right icon and the right name, and launching it produces the
//! desktop's own refusal:
//!
//! > Could not find the program '/home/you/.local/bin/Aurora1-x86_64.AppImage'
//!
//! Which is an accurate message about a file, and tells the operator nothing about what to
//! do. The program it names is usually still on the machine, a directory away.
//!
//! AETHER1 writes some of these entries itself (the three Linux installers do) and does not
//! write others (an AppImage manager writes its own, and so does a desktop that was asked
//! to pin a file to the panel). **This module deliberately does not care which.** The entry
//! names a program with one of AETHER1's names on it, that program is not there, and a copy
//! of AETHER1 that *is* there can be pointed to -- the provenance of the file changes none
//! of those three facts. What it does care about is not touching an entry for somebody
//! else's program, which is what `is_ours` is for.
//!
//! Everything here is a pure function over text except `stale_in`, which reads a directory,
//! and `repoint`, which writes one file. The parsing is the half that can be quietly wrong
//! -- `Exec` is a format with field codes and quoting in it -- so it is the half with the
//! tests.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One launcher entry whose program is not where it says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stale {
    /// The `.desktop` file itself.
    pub path: PathBuf,
    /// What the menu shows, so a report can name it the way the operator sees it.
    pub name: String,
    /// The program it points at, which is not there.
    pub target: PathBuf,
}

/// The directory a desktop entry for one user lives in.
pub fn applications_dir(home: &Path) -> PathBuf {
    home.join(".local").join("share").join("applications")
}

/// The program an `Exec=` line runs: the first token, unquoted, with the field codes
/// (`%U`, `%f`, ...) and any `env VAR=x` prefix taken off.
///
/// Returns `None` for an entry that runs something through a shell (`sh -c ...`) or whose
/// Exec is empty -- not because those cannot be ours, but because the program such a line
/// runs is not a thing that can be read out of it honestly, and rewriting one by guessing
/// is how a working entry gets broken.
pub fn exec_program(exec: &str) -> Option<String> {
    let mut tokens = tokenise(exec).into_iter();
    let mut program = tokens.next()?;
    // `env A=1 B=2 /path/to/thing` -- step over the assignments to the real program.
    if Path::new(&program).file_name().and_then(|n| n.to_str()) == Some("env") {
        for token in tokens.by_ref() {
            if !token.contains('=') {
                program = token;
                break;
            }
        }
    }
    let program = program.trim().to_string();
    if program.is_empty() || program.starts_with('%') {
        return None;
    }
    let base = Path::new(&program).file_name()?.to_str()?;
    if matches!(base, "sh" | "bash" | "zsh" | "flatpak" | "systemd-run") {
        return None;
    }
    Some(program)
}

/// Splits an `Exec` value the way the desktop entry specification does: double quotes group,
/// a backslash inside them escapes, and whitespace separates everything else.
fn tokenise(exec: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for ch in exec.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// The value of a key in the `[Desktop Entry]` group, ignoring every other group -- an
/// entry's actions (`[Desktop Action new-window]`) have their own `Exec` and are not the
/// one the menu runs.
pub fn desktop_value(contents: &str, key: &str) -> Option<String> {
    let mut in_main = false;
    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if !in_main {
            continue;
        }
        if let Some((k, value)) = line.split_once('=') {
            // Locale-qualified keys (`Name[de]`) are not the key.
            if k.trim() == key {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

/// Whether a program this entry names is a copy of AETHER1.
///
/// Both command names count, and so does an AppImage whose file name starts with either --
/// `Aurora1-x86_64.AppImage` is the one the releases publish. The test is on the file name
/// alone, because the whole situation this module exists for is a path that is wrong.
pub fn is_ours(program: &str) -> bool {
    let Some(base) = Path::new(program).file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let lowered = base.to_ascii_lowercase();
    if crate::installs::BINARY_NAMES.contains(&lowered.as_str()) {
        return true;
    }
    lowered.ends_with(".appimage")
        && crate::installs::BINARY_NAMES
            .iter()
            .any(|name| lowered.starts_with(name))
}

/// Whether the program an entry names can actually be run.
///
/// A relative program is looked up on `PATH`, which is what the desktop would do with it;
/// an absolute one is simply a file that is there or is not.
fn program_exists(program: &str) -> bool {
    let path = Path::new(program);
    if path.is_absolute() {
        return path.is_file();
    }
    which::which(program).is_ok()
}

/// Every launcher entry in `dir` that names a copy of AETHER1 which is not there.
///
/// An unreadable directory is not a fault: a machine with no `~/.local/share/applications`
/// has no entries to be wrong, and this is called on Windows and macOS too, where it finds
/// nothing and says so.
pub fn stale_in(dir: &Path) -> Vec<Stale> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut stale = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(program) = desktop_value(&contents, "Exec")
            .as_deref()
            .and_then(exec_program)
        else {
            continue;
        };
        if !is_ours(&program) || program_exists(&program) {
            continue;
        }
        let name = desktop_value(&contents, "Name").unwrap_or_else(|| {
            path.file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "AETHER1".to_string())
        });
        stale.push(Stale {
            path,
            name,
            target: PathBuf::from(program),
        });
    }
    stale.sort_by(|a, b| a.path.cmp(&b.path));
    stale
}

/// The stale entries on this machine, or an empty list where the question does not apply.
pub fn stale() -> Vec<Stale> {
    if !cfg!(target_os = "linux") {
        return Vec::new();
    }
    match crate::paths::home_dir() {
        Some(home) => stale_in(&applications_dir(&home)),
        None => Vec::new(),
    }
}

/// Where the running copy of AETHER1 is, as a path that will still mean something tomorrow.
///
/// **This is the part that is easy to get wrong inside an AppImage**, which is exactly the
/// shape the broken launcher entries come from. An AppImage mounts itself and runs from
/// inside the mount, so `current_exe` is something like
/// `/tmp/.mount_Aurora1AbCdEf/usr/bin/aether1` -- a path that stops existing the moment the
/// program does. Writing that into a launcher entry would replace one entry that starts
/// nothing with another, and the second kind is harder to understand. The runtime sets
/// `APPIMAGE` to the AppImage file itself, which is the answer, and anything still under a
/// temporary mount after that is refused rather than written down.
fn runnable_self(appimage: Option<PathBuf>, exe: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(appimage) = appimage.filter(|path| path.is_absolute()) {
        return Some(appimage);
    }
    exe.filter(|path| {
        !["/tmp", "/run", "/var/tmp", "/dev/shm"]
            .iter()
            .any(|ephemeral| path.starts_with(ephemeral))
    })
}

/// A copy of AETHER1 that is actually on this machine and can be put in an entry.
///
/// The running program first -- it is the one copy whose existence is not in question --
/// and only then what `installs::detect` found, newest-looking first by the order it
/// reports. A source checkout is never offered: its path is a directory, not something a
/// desktop can execute.
pub fn replacement(installed: &[crate::installs::Install]) -> Option<PathBuf> {
    let running = runnable_self(verified_appimage(), std::env::current_exe().ok());
    if let Some(exe) = running {
        if exe.is_file() {
            return Some(exe);
        }
    }
    installed
        .iter()
        .find(|install| {
            !matches!(
                install.kind,
                crate::installs::Kind::SourceCheckout | crate::installs::Kind::WindowsInstaller
            ) && install.path.is_file()
        })
        .map(|install| install.path.clone())
}

/// APPIMAGE is an inherited environment hint, not proof that the named file launched us.
/// Accept it only when its canonical path is the executable's own AppImage container.
fn verified_appimage() -> Option<PathBuf> {
    let appimage = PathBuf::from(std::env::var_os("APPIMAGE")?);
    if !appimage.is_absolute() || !appimage.is_file() {
        return None;
    }
    let appimage_real = std::fs::canonicalize(&appimage).ok()?;
    let exe_real = std::fs::canonicalize(std::env::current_exe().ok()?).ok()?;
    // AppImage mounts its image under /tmp/.mount_*/...; the running executable's
    // canonical path is inside that mount. Verify the mount relationship before trusting
    // the environment value as a durable launcher target.
    let parent = exe_real.ancestors().find(|ancestor| {
        ancestor
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(".mount_"))
    })?;
    appimage_matches_mount(parent, &appimage_real).then_some(appimage_real)
}

fn appimage_matches_mount(mountpoint: &Path, appimage: &Path) -> bool {
    let Ok(mountinfo) = std::fs::read_to_string("/proc/self/mountinfo") else {
        return false;
    };
    mountinfo.lines().any(|line| {
        let Some((before, after)) = line.split_once(" - ") else {
            return false;
        };
        let fields: Vec<_> = before.split_whitespace().collect();
        let source: Vec<_> = after.split_whitespace().collect();
        if fields.len() < 5 || source.len() < 2 || unescape_mount_field(fields[4]) != mountpoint {
            return false;
        }
        let Some(loop_name) = Path::new(source[1]).file_name().and_then(|s| s.to_str()) else {
            return false;
        };
        if !loop_name.starts_with("loop") {
            return false;
        }
        let Ok(backing) =
            std::fs::read_to_string(format!("/sys/block/{loop_name}/loop/backing_file"))
        else {
            return false;
        };
        let backing = PathBuf::from(unescape_mount_text(backing.trim()));
        std::fs::canonicalize(backing).is_ok_and(|path| path == appimage)
    })
}

fn unescape_mount_field(value: &str) -> PathBuf {
    PathBuf::from(unescape_mount_text(value))
}

fn unescape_mount_text(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\'
            && index + 3 < bytes.len()
            && bytes[index + 1..index + 4]
                .iter()
                .all(|byte| (b'0'..=b'7').contains(byte))
        {
            let octal = (bytes[index + 1] - b'0') * 64
                + (bytes[index + 2] - b'0') * 8
                + (bytes[index + 3] - b'0');
            out.push(octal);
            index += 4;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The contents of an entry with its `Exec` line pointed at `target`, and a `TryExec` line
/// updated with it where there is one.
///
/// Only the `[Desktop Entry]` group is touched, the arguments after the program are kept --
/// `%U` carries the file an operator dropped on the icon -- and everything else in the file
/// is left exactly as it was, comments and all. A launcher entry is a file people edit.
pub fn repointed(contents: &str, target: &Path) -> String {
    let quoted = quote(target);
    let mut out = String::with_capacity(contents.len() + quoted.len());
    let mut in_main = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_main = trimmed == "[Desktop Entry]";
        }
        let rewritten = if in_main {
            match trimmed.split_once('=') {
                Some(("Exec", value)) => {
                    let tokens = tokenise(value);
                    let (prefix, suffix) = if tokens
                        .first()
                        .and_then(|token| Path::new(token).file_name())
                        .and_then(|name| name.to_str())
                        == Some("env")
                    {
                        let mut program_index = 1;
                        while program_index < tokens.len() && tokens[program_index].contains('=') {
                            program_index += 1;
                        }
                        if program_index >= tokens.len() {
                            (Vec::new(), Vec::new())
                        } else {
                            (
                                tokens[..program_index].to_vec(),
                                tokens
                                    .into_iter()
                                    .skip(program_index + 1)
                                    .collect::<Vec<_>>(),
                            )
                        }
                    } else {
                        (Vec::new(), tokens.into_iter().skip(1).collect::<Vec<_>>())
                    };
                    let mut exec = prefix
                        .iter()
                        .map(|token| quote_exec_token(token))
                        .collect::<Vec<_>>();
                    exec.push(quoted.clone());
                    exec.extend(suffix.iter().map(|token| quote_exec_token(token)));
                    Some(format!("Exec={}", exec.join(" ")))
                }
                Some(("TryExec", _)) => Some(format!("TryExec={quoted}")),
                _ => None,
            }
        } else {
            None
        };
        out.push_str(rewritten.as_deref().unwrap_or(line));
        out.push('\n');
    }
    out
}

/// A path as an `Exec` token: quoted only when it has to be, since a quoted path in a file
/// people read looks like something was wrong with it.
fn quote(path: &Path) -> String {
    quote_exec_token(&path.display().to_string())
}

fn quote_exec_token(text: &str) -> String {
    if text.chars().any(char::is_whitespace) || text.contains('"') {
        format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        text.to_string()
    }
}

/// Points one entry at a program that exists. Returns what to tell the operator.
pub fn repoint(entry: &Stale, target: &Path) -> Result<String, String> {
    if !target.is_file() {
        return Err(format!(
            "{} is not there either, so pointing the launcher at it would change one broken \
             entry for another",
            target.display()
        ));
    }
    let metadata = std::fs::symlink_metadata(&entry.path)
        .map_err(|e| format!("could not inspect {}: {e}", entry.path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "{} is not a regular launcher file; refusing to follow links while repairing it",
            entry.path.display()
        ));
    }
    let contents = std::fs::read_to_string(&entry.path)
        .map_err(|e| format!("could not read {}: {e}", entry.path.display()))?;
    let updated = repointed(&contents, target);
    let parent = entry.path.parent().unwrap_or_else(|| Path::new("."));
    let name = entry
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("launcher.desktop");
    let temporary = parent.join(format!(".{name}.aether1-{}.tmp", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(metadata.permissions().mode());
    }
    use std::io::Write;
    let write_result = (|| {
        let mut file = options.open(&temporary)?;
        file.write_all(updated.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temporary, &entry.path)
    })();
    if write_result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    write_result.map_err(|e| format!("could not safely replace {}: {e}", entry.path.display()))?;
    Ok(format!(
        "{} now starts {}",
        entry.path.display(),
        target.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_program_is_read_out_of_an_exec_line_with_field_codes_and_quotes_in_it() {
        assert_eq!(
            exec_program("/home/t/.local/bin/aurora1 %U").as_deref(),
            Some("/home/t/.local/bin/aurora1")
        );
        assert_eq!(
            exec_program("\"/home/t/My Apps/Aurora1-x86_64.AppImage\" %F").as_deref(),
            Some("/home/t/My Apps/Aurora1-x86_64.AppImage")
        );
        assert_eq!(
            exec_program("env GDK_BACKEND=x11 /opt/aether1 --window").as_deref(),
            Some("/opt/aether1")
        );
        // A shell line is left alone rather than guessed at.
        assert_eq!(exec_program("sh -c 'aurora1 && echo'"), None);
        assert_eq!(exec_program("  "), None);
    }

    #[test]
    fn only_the_main_group_is_read() {
        let entry = "[Desktop Entry]\nName=Aurora1\nExec=/a/aurora1\n\n\
                     [Desktop Action new]\nExec=/b/other\n";
        assert_eq!(desktop_value(entry, "Exec").as_deref(), Some("/a/aurora1"));
        assert_eq!(desktop_value(entry, "Name").as_deref(), Some("Aurora1"));
        assert_eq!(desktop_value(entry, "Icon"), None);
    }

    #[test]
    fn somebody_elses_launcher_is_not_ours_to_touch() {
        assert!(is_ours("/home/t/.local/bin/aurora1"));
        assert!(is_ours("aether1"));
        assert!(is_ours("/home/t/.local/bin/Aurora1-x86_64.AppImage"));
        assert!(!is_ours("/usr/bin/firefox"));
        // Shares a prefix and is not one of ours.
        assert!(!is_ours("/usr/bin/aurora-store"));
        assert!(!is_ours("/home/t/Downloads/Other.AppImage"));
    }

    #[test]
    fn repointing_keeps_the_arguments_the_comments_and_every_other_group() {
        let entry = "# written by hand\n[Desktop Entry]\nName=Aurora1\n\
                     Exec=/home/t/.local/bin/Aurora1-x86_64.AppImage %U\n\
                     TryExec=/home/t/.local/bin/Aurora1-x86_64.AppImage\nIcon=aether1\n\n\
                     [Desktop Action face]\nExec=/home/t/.local/bin/aurora1 face\n";
        let updated = repointed(
            entry,
            Path::new("/home/t/.local/share/AppImages/Aurora1.AppImage"),
        );
        assert!(updated.contains("Exec=/home/t/.local/share/AppImages/Aurora1.AppImage %U\n"));
        assert!(updated.contains("TryExec=/home/t/.local/share/AppImages/Aurora1.AppImage\n"));
        assert!(updated.contains("# written by hand\n"));
        assert!(updated.contains("Icon=aether1\n"));
        // The action's own Exec is not the one the menu runs, and is not rewritten.
        assert!(updated.contains("Exec=/home/t/.local/bin/aurora1 face\n"));
    }

    #[test]
    fn a_path_with_a_space_in_it_comes_back_quoted() {
        let entry = "[Desktop Entry]\nExec=/a/aurora1 %U\n";
        let updated = repointed(entry, Path::new("/home/t/My Apps/aurora1"));
        assert!(updated.contains("Exec=\"/home/t/My Apps/aurora1\" %U\n"));
        assert_eq!(
            exec_program(desktop_value(&updated, "Exec").unwrap().as_str()).as_deref(),
            Some("/home/t/My Apps/aurora1")
        );
    }

    #[test]
    fn a_directory_of_entries_yields_only_the_ones_that_are_ours_and_broken() {
        let dir = std::env::temp_dir().join(format!(
            "aether1-launchers-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let here = std::env::current_exe().unwrap();

        std::fs::write(
            dir.join("Aurora1.desktop"),
            "[Desktop Entry]\nName=Aurora1\nExec=/nowhere/Aurora1-x86_64.AppImage %U\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("Working.desktop"),
            format!(
                "[Desktop Entry]\nName=Aether1\nExec={} %U\n",
                here.display()
            ),
        )
        .unwrap();
        std::fs::write(
            dir.join("Someone-else.desktop"),
            "[Desktop Entry]\nName=Thing\nExec=/nowhere/thing\n",
        )
        .unwrap();
        std::fs::write(dir.join("notes.txt"), "not an entry").unwrap();

        let found = stale_in(&dir);
        assert_eq!(found.len(), 1, "expected one stale entry, got {found:?}");
        assert_eq!(found[0].name, "Aurora1");
        assert_eq!(
            found[0].target,
            PathBuf::from("/nowhere/Aurora1-x86_64.AppImage")
        );

        // And repointing it fixes exactly that.
        let message = repoint(&found[0], &here).unwrap();
        assert!(message.contains("now starts"));
        assert!(stale_in(&dir).is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn inside_an_appimage_the_entry_gets_the_appimage_not_its_mount() {
        let mount = PathBuf::from("/tmp/.mount_Aurora1AbCdEf/usr/bin/aether1");
        let file = PathBuf::from("/home/t/.local/share/AppImages/Aurora1-x86_64.AppImage");
        assert_eq!(
            runnable_self(Some(file.clone()), Some(mount.clone())),
            Some(file)
        );
        // No APPIMAGE, and the only thing running is inside a mount that will not outlive
        // it: nothing here is worth writing into a launcher, so the installed copies are
        // asked instead.
        assert_eq!(runnable_self(None, Some(mount)), None);
        let installed = PathBuf::from("/home/t/.local/bin/aurora1");
        assert_eq!(
            runnable_self(None, Some(installed.clone())),
            Some(installed)
        );
    }

    #[test]
    fn a_replacement_that_is_not_there_is_refused_rather_than_written() {
        let entry = Stale {
            path: PathBuf::from("/nowhere/Aurora1.desktop"),
            name: "Aurora1".to_string(),
            target: PathBuf::from("/nowhere/aurora1"),
        };
        let error = repoint(&entry, Path::new("/nowhere/else/aurora1")).unwrap_err();
        assert!(error.contains("is not there either"), "{error}");
    }
}
