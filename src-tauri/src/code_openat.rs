//! Opening a file in the project folder without re-walking its path.
//!
//! `code_workspace::resolve` decides whether a path is inside the project, and until now the
//! write that followed handed the *same string* back to the operating system, which walked
//! it again from `/`. Two walks with a gap between them is a race: a hostile process on this
//! machine -- or a `run` command, or a `build.rs`, or anything else with write access to a
//! directory along the way -- can replace a component with a symlink in that gap, and the
//! second walk goes somewhere the first one approved of and the second one does not. It is a
//! narrow window and a real one, and it is the kind of bug you cannot test your way out of;
//! you have to stop leaving the window open.
//!
//! So this module opens files the way the check meant: **one walk, downwards from the project
//! root, holding each directory open as a file descriptor and never following a symlink.**
//!
//! * The root is opened once. It is the operator's own canonicalized setting, and it is the
//!   trust anchor -- nothing above it is this program's business.
//! * Each component below it is opened with `openat` from the directory *handle* rather than
//!   by name from `/`, so renaming or swapping a directory after it was opened cannot change
//!   which directory the next step happens in. There is no name left to re-resolve.
//! * Every one of those opens carries `O_NOFOLLOW`, so a symlink appearing anywhere along the
//!   way is an error naming the component rather than a redirection nobody sees.
//! * `..` and `.` are refused as components. They cannot appear in a path that came from
//!   `resolve` -- it canonicalizes -- but a guard that depends on its caller having been
//!   careful is not a guard.
//!
//! The result is that containment stops being a judgement made about a string and becomes a
//! property of how the file was opened. There is no second walk to lose the argument.
//!
//! **Windows is not covered.** There is no `openat` there and the equivalent
//! (`NtCreateFile` with a root directory handle, `FILE_FLAG_OPEN_REPARSE_POINT`) is a
//! different piece of work. On Windows these functions fall back to `std::fs` and the old
//! window stays open; `confines()` says so, for the same reason `code_sandbox` says so about
//! `run`, which is that a boundary nobody states is worse than one that is missing.

use std::path::{Component, Path};

/// Whether opening is actually pinned to the directories that were checked on this platform.
/// The Settings page and `docs/SECURITY_MODEL.md` both answer this question from here rather
/// than from a guess about the operating system.
pub fn confines() -> bool {
    cfg!(unix)
}

/// The components of `target` below `root`, refusing anything that could climb.
fn descent(root: &Path, target: &Path) -> Result<Vec<String>, String> {
    let relative = target.strip_prefix(root).map_err(|_| {
        format!(
            "{} is not inside {}, so it cannot be opened from there",
            target.display(),
            root.display()
        )
    })?;
    let mut steps = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(name) => steps.push(
                name.to_str()
                    .ok_or_else(|| format!("{} has a name this cannot read", target.display()))?
                    .to_string(),
            ),
            // `.` is harmless and `..` is the whole problem, but neither belongs in a path
            // that has been canonicalized, so both mean something upstream went wrong.
            other => {
                return Err(format!(
                    "{} contains {other:?}, which is not a plain name",
                    target.display()
                ))
            }
        }
    }
    if steps.is_empty() {
        return Err(format!(
            "{} is the project folder itself, not a file in it",
            target.display()
        ));
    }
    Ok(steps)
}

#[cfg(unix)]
mod imp {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::path::Path;

    use super::descent;

    fn c_name(name: &str) -> Result<CString, String> {
        CString::new(name).map_err(|_| format!("{name:?} is not a name a file can have"))
    }

    fn errno() -> i32 {
        std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
    }

    /// Whether `step` in the directory `fd` is a symbolic link, asked without following it.
    ///
    /// The errno alone cannot answer this. `O_NOFOLLOW` on a link gives `ELOOP`, but
    /// `O_NOFOLLOW | O_DIRECTORY` on a link to a directory gives `ENOTDIR` on Linux, because
    /// the directory check happens first. Reporting "not a directory" for the one thing this
    /// walk exists to catch would hide it in the place it matters most, so the question is
    /// put to the filesystem rather than inferred.
    fn is_a_link(fd: &OwnedFd, step: &str) -> bool {
        let Ok(name) = c_name(step) else {
            return false;
        };
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: `fd` is an open directory, `name` one NUL-terminated component, and `stat`
        // is only read after the call reports success.
        let ok = unsafe {
            libc::fstatat(
                fd.as_raw_fd(),
                name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } == 0;
        ok && (unsafe { stat.assume_init() }.st_mode & libc::S_IFMT) == libc::S_IFLNK
    }

    /// Why a step failed, in the operator's terms. A link is the case that matters: it is
    /// what this guard exists to catch, so it is named as one rather than as whichever errno
    /// the kernel reached for.
    fn step_failed(fd: &OwnedFd, what: &str, step: &str) -> String {
        let code = errno();
        if code == libc::ELOOP || is_a_link(fd, step) {
            format!(
                "{step:?} in {what} is a symbolic link, and this does not follow links when it \
                 opens a file in the project folder. If it points somewhere inside the project, \
                 name that place instead."
            )
        } else {
            format!(
                "cannot open {step:?} in {what}: {}",
                std::io::Error::from_raw_os_error(code)
            )
        }
    }

    fn open_dir(fd: &OwnedFd, step: &str, what: &str) -> Result<OwnedFd, String> {
        let name = c_name(step)?;
        // SAFETY: `fd` is an open directory this function owns, and `name` is a NUL-terminated
        // single component. The returned descriptor is handed straight to OwnedFd, which closes
        // it.
        let opened = unsafe {
            libc::openat(
                fd.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if opened < 0 {
            return Err(step_failed(fd, what, step));
        }
        Ok(unsafe { OwnedFd::from_raw_fd(opened) })
    }

    fn make_dir(fd: &OwnedFd, step: &str) -> Result<(), String> {
        let name = c_name(step)?;
        // SAFETY: as above; `fd` is an open directory and `name` one component.
        let made = unsafe { libc::mkdirat(fd.as_raw_fd(), name.as_ptr(), 0o755) };
        if made < 0 && errno() != libc::EEXIST {
            return Err(format!(
                "cannot make {step:?}: {}",
                std::io::Error::from_raw_os_error(errno())
            ));
        }
        Ok(())
    }

    /// Walks from `root` to the directory holding `target`, and returns it with the final
    /// name. `create` makes missing directories on the way, which is what `create_file`
    /// needs and `edit_file` must not have.
    fn walk(root: &Path, target: &Path, create: bool) -> Result<(OwnedFd, String), String> {
        let mut steps = descent(root, target)?;
        let name = steps.pop().expect("descent returns at least one step");
        // The root is opened by name, once. It is the operator's canonicalized setting, so
        // whatever it is reached through is theirs and not a component this walk crossed.
        let mut fd = OwnedFd::from(
            std::fs::File::open(root)
                .map_err(|e| format!("cannot open the project folder {}: {e}", root.display()))?,
        );
        let mut walked = root.display().to_string();
        for step in steps {
            if create {
                make_dir(&fd, &step)?;
            }
            fd = open_dir(&fd, &step, &walked)?;
            walked = format!("{walked}/{step}");
        }
        Ok((fd, name))
    }

    fn open_at(
        fd: &OwnedFd,
        name: &str,
        flags: libc::c_int,
        mode: libc::mode_t,
        what: &str,
    ) -> Result<std::fs::File, String> {
        let c = c_name(name)?;
        // SAFETY: `fd` is an open directory, `c` one NUL-terminated component, and the
        // descriptor is adopted by File, which closes it.
        let opened = unsafe {
            libc::openat(
                fd.as_raw_fd(),
                c.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                libc::c_uint::from(mode),
            )
        };
        if opened < 0 {
            return Err(step_failed(fd, what, name));
        }
        Ok(std::fs::File::from(unsafe { OwnedFd::from_raw_fd(opened) }))
    }

    pub fn read(root: &Path, target: &Path) -> Result<String, String> {
        use std::io::Read;
        let (fd, name) = walk(root, target, false)?;
        let mut file = open_at(&fd, &name, libc::O_RDONLY, 0, &target.display().to_string())?;
        let mut text = String::new();
        file.read_to_string(&mut text)
            .map_err(|e| format!("cannot read {}: {e}", target.display()))?;
        Ok(text)
    }

    pub fn write(
        root: &Path,
        target: &Path,
        bytes: &[u8],
        create_dirs: bool,
    ) -> Result<(), String> {
        use std::io::Write;
        let (fd, name) = walk(root, target, create_dirs)?;
        let mut file = open_at(
            &fd,
            &name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC,
            0o644,
            &target.display().to_string(),
        )?;
        file.write_all(bytes)
            .map_err(|e| format!("cannot write {}: {e}", target.display()))?;
        Ok(())
    }
}

#[cfg(not(unix))]
mod imp {
    use std::path::Path;

    use super::descent;

    /// Windows has no `openat`. The path is still checked -- `descent` refuses anything that
    /// is not a plain descent from the root -- and then handed to `std::fs`, which walks it
    /// again. The window this module exists to close stays open here, which `confines()`
    /// reports rather than hides.
    pub fn read(root: &Path, target: &Path) -> Result<String, String> {
        descent(root, target)?;
        std::fs::read_to_string(target)
            .map_err(|e| format!("cannot read {}: {e}", target.display()))
    }

    pub fn write(
        root: &Path,
        target: &Path,
        bytes: &[u8],
        create_dirs: bool,
    ) -> Result<(), String> {
        descent(root, target)?;
        if create_dirs {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
            }
        }
        std::fs::write(target, bytes).map_err(|e| format!("cannot write {}: {e}", target.display()))
    }
}

/// Reads a file, walking to it from `root` without following a link on the way.
pub fn read(root: &Path, target: &Path) -> Result<String, String> {
    imp::read(root, target)
}

/// Writes a file, walking to it from `root` without following a link on the way.
///
/// `create_dirs` makes the directories below the root that the path names. It is the one
/// thing `create_file` needs that `edit_file` must not have: a missing directory means an
/// `edit_file` whose path is wrong, and making it would turn a typo into a new tree.
pub fn write(root: &Path, target: &Path, bytes: &[u8], create_dirs: bool) -> Result<(), String> {
    imp::write(root, target, bytes, create_dirs)
}

/// The project-relative descent a path names, for a caller that wants to check without
/// opening anything. It exists for the tests, which is why it is behind `cfg(test)`: a guard
/// worth having is worth checking on its own, and nothing in the program needs it.
#[cfg(test)]
pub fn steps_within(root: &Path, target: &Path) -> Result<Vec<String>, String> {
    descent(root, target)
}
