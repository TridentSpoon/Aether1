// Every copy of AETHER1 on this machine, and taking away the ones that are no longer the
// one you run.
//
// This exists because AETHER1 has been installable five different ways -- a built binary
// copied to ~/.local/bin by setup.sh, the offline and slim bundles, an AppImage, a .deb or
// AUR package, and a per-user Inno Setup install on Windows -- and none of them knows about
// the others. Install it a second way and the first one stays exactly where it was: an old
// binary still on PATH, a launcher entry still pointing at it, and a version number in the
// tray that depends on which icon you happened to click. The symptom is never "there are two
// installs"; it is a bug that was fixed weeks ago still happening.
//
// Three rules shape everything below, and each is here because the alternative is a thing
// that cannot be undone:
//
// 1. **Nothing is removed without being asked for.** Detection is automatic; removal is not.
//    The HUD asks, or the operator names an id on the command line. There is no silent path.
// 2. **Data is never in scope.** An install is a program: a binary, a launcher, an icon. The
//    vault, the conversation database and the settings live under ~/.local/share/aether1 and
//    are deliberately unreachable from here -- `remove` refuses any path that touches them,
//    and a test holds that refusal in place.
// 3. **What AETHER1 did not put there, AETHER1 does not delete.** A .deb or an AUR package
//    belongs to dpkg or pacman, and deleting its files behind its back leaves the package
//    manager believing the package is still installed, which is worse than the duplicate.
//    Those are handed over as the exact command to run. A source checkout is not an install
//    at all and is only ever reported.
//
// The scan reads the machine through the `Machine` trait rather than calling dpkg, pacman
// and the Windows registry directly, so the tests can hand it a machine that does not exist.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::paths;

/// The settings key that turns the startup notice off. Answering "keep them" is a decision,
/// and a decision the next launch has forgotten is a prompt that never goes away.
pub const NOTICE_SETTING: &str = "installs_notice";

/// What a copy of AETHER1 says when asked its version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Version {
    /// The scheme this app prints: `Ver 0.4.152`, where 152 is the pull request it was
    /// built from (see build.rs). The older spelling, `Aether1 0.3.Rev152`, says the same
    /// thing and is still read: the whole point of this module is finding copies installed
    /// before now, and those copies print what they printed when they were built.
    ///
    /// `pr` is what compares. It is a pull request number on one repository, so it only ever
    /// increases -- across a major or minor bump as much as within one -- while the major
    /// and minor are editorial and are kept only so a version can be shown back accurately.
    Build { major: u32, minor: u32, pr: u32 },
    /// Whatever a package manager reports instead. Kept as text on purpose: a `0.4.0` from
    /// dpkg and a `0.4.152` from the binary are not points on the same number line, and
    /// pretending they are is how the wrong copy gets deleted.
    Opaque(String),
}

impl Version {
    /// A build of this project at `major.minor`, from pull request `pr`. Only the tests
    /// construct one directly -- everything else reads a version off a binary via `parse`.
    #[cfg(test)]
    pub fn build(major: u32, minor: u32, pr: u32) -> Version {
        Version::Build { major, minor, pr }
    }

    /// Reads a version out of a `--version` line or a package manager's answer. Deliberately
    /// forgiving about what surrounds it -- `aether1 --version` prints `Ver 0.4.152
    /// (a1b2c3d)`, a build from before that prints `Aether1 0.3.Rev152`, an older one
    /// printed less, and a package manager prints a bare number.
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let lowered = text.to_ascii_lowercase();
        // The old spelling first, because `Aether1 0.3.Rev152` also contains a bare `0.3`
        // that the three-number branch below would happily read as a version and get wrong.
        if let Some(at) = lowered.find("rev") {
            let digits: String = text[at + 3..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(pr) = digits.parse::<u32>() {
                let (major, minor) = Version::major_minor_before(&text[..at]).unwrap_or((0, 0));
                return Some(Version::Build { major, minor, pr });
            }
        }
        // The current spelling: a token of exactly three numbers, `0.4.152`.
        for token in text.split_whitespace() {
            let token = token.trim_matches(['(', ')', ',']);
            let mut parts = token.split('.');
            if let (Some(a), Some(b), Some(c), None) =
                (parts.next(), parts.next(), parts.next(), parts.next())
            {
                if let (Ok(major), Ok(minor), Ok(pr)) =
                    (a.parse::<u32>(), b.parse::<u32>(), c.parse::<u32>())
                {
                    return Some(Version::Build { major, minor, pr });
                }
            }
        }
        // Not the Rev scheme: keep the first token that has a digit in it, which is the
        // version in every shape a package manager prints.
        text.split_whitespace()
            .find(|token| token.chars().any(|c| c.is_ascii_digit()))
            .map(|token| Version::Opaque(token.trim_matches(['(', ')', ',']).to_string()))
    }

    /// The `major.minor` immediately before a `Rev`, for the old spelling. None when there
    /// is no such number, which is what a build too old to print one gives.
    fn major_minor_before(prefix: &str) -> Option<(u32, u32)> {
        let token = prefix
            .split_whitespace()
            .next_back()?
            .trim_end_matches('.')
            .trim_matches(['(', ')', ',']);
        let mut parts = token.split('.');
        match (parts.next(), parts.next(), parts.next()) {
            (Some(a), Some(b), None) => Some((a.parse().ok()?, b.parse().ok()?)),
            _ => None,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Version::Build { major, minor, pr } => format!("{major}.{minor}.{pr}"),
            Version::Opaque(text) => text.clone(),
        }
    }

    /// `Some(true)` when this version is definitely behind `other`, `Some(false)` when it is
    /// definitely not, and `None` when the two cannot be compared at all -- which is the
    /// answer whenever either side is opaque, and the reason `Standing::Unknown` exists.
    pub fn older_than(&self, other: &Version) -> Option<bool> {
        match (self, other) {
            // On `pr` alone: see the note on the variant. Comparing the major and minor
            // first would say the same thing, since pull request numbers keep climbing
            // across a version bump, but it would imply the bump carries information about
            // which build is newer, and it does not.
            (Version::Build { pr: mine, .. }, Version::Build { pr: theirs, .. }) => {
                Some(mine < theirs)
            }
            _ => None,
        }
    }
}

/// How a copy of AETHER1 got onto the machine. This is what the operator recognises it by,
/// so it is named after the thing they did rather than after the files it left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// setup.sh, the offline bundle or the slim bundle: a binary in ~/.local/bin with a
    /// launcher entry and an icon beside it.
    UserBundle,
    /// A downloaded AppImage, wherever it was left.
    AppImage,
    /// A binary under /usr/local/bin or /usr/bin that no package manager claims.
    SystemBinary,
    /// Installed by dpkg, and dpkg's to remove.
    DebPackage,
    /// Installed by pacman (the AUR package), and pacman's to remove.
    PacmanPackage,
    /// The per-user Inno Setup install on Windows, which ships its own uninstaller.
    WindowsInstaller,
    /// A git checkout that builds and runs AETHER1. Not an install, and never removed.
    SourceCheckout,
}

impl Kind {
    pub fn describe(&self) -> &'static str {
        match self {
            Kind::UserBundle => "installed for this user (setup.sh or a bundle)",
            Kind::AppImage => "AppImage",
            Kind::SystemBinary => "installed system-wide",
            Kind::DebPackage => "installed by dpkg",
            Kind::PacmanPackage => "installed by pacman",
            Kind::WindowsInstaller => "installed by the Windows installer",
            Kind::SourceCheckout => "a source checkout",
        }
    }
}

/// What removing this copy would actually mean.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removal {
    /// Files AETHER1 put there itself and can take back.
    Files(Vec<PathBuf>),
    /// The installer left its own uninstaller, which runs as this user and needs no
    /// elevation (the Windows install is per-user by design -- see installer/aether1.iss).
    Uninstaller { program: PathBuf, args: Vec<String> },
    /// Something else owns these files and removing them needs root. AETHER1 never asks for
    /// root, so the operator gets the exact command instead of a prompt.
    HandOver { command: String, why: &'static str },
    /// Not AETHER1's to delete.
    Keep { why: &'static str },
}

/// One copy of AETHER1 on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Install {
    /// Eight hex characters derived from the path, so `aether1 installs remove <id>` names
    /// one copy unambiguously and the same copy keeps the same id between runs.
    pub id: String,
    pub kind: Kind,
    /// The path that identifies it: the binary, the AppImage, or the install directory.
    pub path: PathBuf,
    pub version: Option<Version>,
    /// True for the copy that is executing right now.
    pub running: bool,
    pub removal: Removal,
}

impl Install {
    fn new(kind: Kind, path: PathBuf, version: Option<Version>, removal: Removal) -> Install {
        Install {
            id: short_id(&path),
            kind,
            path,
            version,
            running: false,
            removal,
        }
    }

    /// One line, the way it is read in a list: what it is, where it is, how old it is.
    pub fn describe(&self) -> String {
        let version = match &self.version {
            Some(version) => version.label(),
            None => "version unknown".to_string(),
        };
        format!(
            "{} -- {} ({version})",
            self.path.display(),
            self.kind.describe()
        )
    }
}

/// Why a copy is, or is not, offered for removal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// The copy that is executing right now. Never removed: it is the one that would be
    /// doing the removing.
    Running,
    /// Older than the copy that is running. This is the thing the feature exists for.
    Stale,
    /// The same build, sitting in a second place. Still worth clearing away -- two copies of
    /// one build is how the wrong one ends up on PATH -- but calling it "older" would be a
    /// lie the operator can check.
    Duplicate,
    /// Newer than the copy that is running. Never offered -- the likeliest reason for a
    /// newer copy to exist is that the operator just installed it and happens to have
    /// launched the old one, and removing it would quietly undo the upgrade.
    Newer,
    /// Another copy whose age cannot be established. Offered, because a second copy is a
    /// problem whether or not its version can be read, but labelled as the guess it is.
    Unknown,
    /// Not AETHER1's to delete.
    Keep(&'static str),
}

impl Standing {
    pub fn offered(&self) -> bool {
        matches!(
            self,
            Standing::Stale | Standing::Duplicate | Standing::Unknown
        )
    }
}

/// Where a copy stands against the one that is running.
pub fn standing(install: &Install, running: Option<&Version>) -> Standing {
    if install.running {
        return Standing::Running;
    }
    if let Removal::Keep { why } = install.removal {
        return Standing::Keep(why);
    }
    match (install.version.as_ref(), running) {
        (Some(mine), Some(theirs)) => match mine.older_than(theirs) {
            Some(true) => Standing::Stale,
            Some(false) if mine == theirs => Standing::Duplicate,
            Some(false) => Standing::Newer,
            None => Standing::Unknown,
        },
        _ => Standing::Unknown,
    }
}

/// What a removal did. Two of the three did not delete anything here, and saying which is
/// the difference between "it is gone" and "it is gone once you run this".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Removed { paths: Vec<PathBuf> },
    Ran { command: String },
    HandedOver { command: String },
}

/// Everything the scan learns from outside the filesystem, behind a trait so the tests can
/// hand it a machine that does not exist rather than needing dpkg, pacman, a Windows
/// registry, or a second copy of AETHER1 actually installed on the runner.
pub trait Machine {
    fn home(&self) -> Option<PathBuf>;
    /// The binary that is executing.
    fn running_exe(&self) -> Option<PathBuf>;
    /// This build's own version, which is the yardstick everything else is measured against.
    fn running_version(&self) -> Option<Version>;
    /// What `<binary> --version` says, or None if it cannot be run or does not answer.
    fn version_of(&self, binary: &Path) -> Option<Version>;
    /// The version dpkg reports for the `aether1` package, if dpkg is here and it knows it.
    fn deb_package(&self) -> Option<String>;
    /// The same question for pacman -- the AUR package from step 17.
    fn pacman_package(&self) -> Option<String>;
    /// `(version, uninstaller path)` from the Windows registry, for the per-user Inno Setup
    /// install. None everywhere else.
    fn windows_install(&self) -> Option<(Option<String>, PathBuf)>;
}

/// The real machine.
pub struct ThisMachine;

impl Machine for ThisMachine {
    fn home(&self) -> Option<PathBuf> {
        paths::home_dir()
    }

    fn running_exe(&self) -> Option<PathBuf> {
        std::env::current_exe().ok()
    }

    fn running_version(&self) -> Option<Version> {
        Version::parse(crate::APP_VERSION)
    }

    fn version_of(&self, binary: &Path) -> Option<Version> {
        // Asking a binary its own version means running it, which is only safe because every
        // path this is called with is one AETHER1 itself writes to. `--version` is handled in
        // cli::parse before anything is built or opened, so this returns immediately.
        let output = Command::new(binary).arg("--version").output().ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Version::parse(stdout.lines().next().unwrap_or_default())
    }

    fn deb_package(&self) -> Option<String> {
        query_package("dpkg-query", &["-W", "-f=${Version}", "aether1"])
    }

    fn pacman_package(&self) -> Option<String> {
        // `-Q` prints "aether1 0.4.0-1"; the version is the second field.
        query_package("pacman", &["-Q", "aether1"])
            .and_then(|line| line.split_whitespace().nth(1).map(|v| v.to_string()))
    }

    #[cfg(target_os = "windows")]
    fn windows_install(&self) -> Option<(Option<String>, PathBuf)> {
        // The AppId in installer/aether1.iss, with the `_is1` suffix Inno Setup appends, under
        // HKCU because the install is per-user (PrivilegesRequired=lowest). Read with reg.exe
        // rather than a registry crate: one query, and nothing else in AETHER1 needs one.
        const KEY: &str = concat!(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\",
            r"{9B7B6C6E-7B3E-4A9B-9C7A-8A1E8B6A6F1E}_is1"
        );
        let output = Command::new("reg").args(["query", KEY]).output().ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout).to_string();
        let value = |name: &str| -> Option<String> {
            text.lines()
                .find(|line| line.trim_start().starts_with(name))
                .and_then(|line| line.split_whitespace().nth(2))
                .map(|v| v.trim_matches('"').to_string())
        };
        let uninstaller = value("UninstallString")?;
        Some((value("DisplayVersion"), PathBuf::from(uninstaller)))
    }

    #[cfg(not(target_os = "windows"))]
    fn windows_install(&self) -> Option<(Option<String>, PathBuf)> {
        None
    }
}

/// Runs a package manager query, treating "not installed" and "not here at all" as the same
/// answer, because they are: either way there is no package to remove.
fn query_package(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Eight hex characters from the path. FNV-1a rather than a real hash: this names a row in a
/// list of at most a handful, and nothing depends on it being hard to collide.
fn short_id(path: &Path) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{:08x}", (hash >> 32) as u32)
}

/// Paths a removal must never touch, whatever an install claims to own: everything the
/// operator would actually lose is under here -- the vault, the conversation database, the
/// settings, the paired devices.
fn data_roots(home: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = vec![crate::project_root()];
    if let Some(home) = home {
        roots.push(home.join(".local").join("share").join("aether1"));
    }
    roots
}

/// True when deleting `path` would take data with it -- either because it is inside a data
/// directory, or because it is a parent of one.
fn touches_data(path: &Path, roots: &[PathBuf]) -> bool {
    roots
        .iter()
        .any(|root| path.starts_with(root) || root.starts_with(path))
}

/// Where AppImages are left. Not a search of the disk: an AppImage anywhere else was put
/// there deliberately, and finding it would mean reading every directory the operator has.
fn appimage_dirs(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join("Applications"),
        home.join("Downloads"),
        home.join(".local").join("bin"),
        home.join("bin"),
        PathBuf::from("/opt"),
    ]
}

fn is_appimage(name: &str) -> bool {
    let lowered = name.to_ascii_lowercase();
    lowered.starts_with("aether1") && lowered.ends_with(".appimage")
}

/// True when this path sits inside a git checkout of AETHER1 -- a directory with both
/// `frontend/` and `src-tauri/` in it, under a `.git`. A binary in `target/release` there is
/// something the operator builds, not something they installed, and deleting it would delete
/// their own build output.
fn checkout_root(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|dir| {
            dir.join("frontend").is_dir()
                && dir.join("src-tauri").is_dir()
                && dir.join(".git").exists()
        })
        .map(|dir| dir.to_path_buf())
}

/// Every copy of AETHER1 this machine can see, the running one included.
pub fn detect(machine: &dyn Machine) -> Vec<Install> {
    let running_exe = machine.running_exe();
    let mut found: Vec<Install> = Vec::new();

    // Package managers first, so a binary at /usr/bin that dpkg or pacman owns is attributed
    // to the package rather than listed a second time as a loose file to delete.
    let mut package_owned = false;
    if let Some(version) = machine.deb_package() {
        package_owned = true;
        found.push(Install::new(
            Kind::DebPackage,
            PathBuf::from("/usr/bin/aether1"),
            Some(Version::Opaque(version)),
            Removal::HandOver {
                command: "sudo apt-get remove aether1".to_string(),
                why: "dpkg owns these files, so removing them any other way leaves it \
                      believing the package is still installed",
            },
        ));
    }
    if let Some(version) = machine.pacman_package() {
        package_owned = true;
        found.push(Install::new(
            Kind::PacmanPackage,
            PathBuf::from("/usr/bin/aether1"),
            Some(Version::Opaque(version)),
            Removal::HandOver {
                command: "sudo pacman -R aether1".to_string(),
                why: "pacman owns these files, so removing them any other way leaves it \
                      believing the package is still installed",
            },
        ));
    }

    if let Some((version, uninstaller)) = machine.windows_install() {
        let dir = uninstaller
            .parent()
            .map(|dir| dir.to_path_buf())
            .unwrap_or_else(|| uninstaller.clone());
        found.push(Install::new(
            Kind::WindowsInstaller,
            dir,
            version.as_deref().and_then(Version::parse),
            Removal::Uninstaller {
                program: uninstaller,
                // Inno's own silent switches: no wizard, no message boxes, and no restart
                // prompt for a per-user install that cannot need one.
                args: vec![
                    "/VERYSILENT".to_string(),
                    "/SUPPRESSMSGBOXES".to_string(),
                    "/NORESTART".to_string(),
                ],
            },
        ));
    }

    if let Some(home) = machine.home() {
        // The user bundle: one binary, one launcher entry, one icon. Piper's and whisper's
        // files under ~/.local/share are deliberately not in this list -- they are shared
        // engines that whichever copy survives still uses.
        let bin = home.join(".local").join("bin").join("aether1");
        if bin.is_file() {
            let removal = match checkout_root(&bin.canonicalize().unwrap_or_else(|_| bin.clone())) {
                Some(_) => Removal::Keep {
                    why: "this is a link into a source checkout, not a copy of its own",
                },
                None => Removal::Files(vec![
                    bin.clone(),
                    home.join(".local")
                        .join("share")
                        .join("applications")
                        .join("Aether1.desktop"),
                    home.join(".local")
                        .join("share")
                        .join("icons")
                        .join("hicolor")
                        .join("256x256")
                        .join("apps")
                        .join("aether1.png"),
                ]),
            };
            let version = machine.version_of(&bin);
            found.push(Install::new(Kind::UserBundle, bin, version, removal));
        }

        for dir in appimage_dirs(&home) {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() || !is_appimage(&entry.file_name().to_string_lossy()) {
                    continue;
                }
                let version = machine.version_of(&path);
                found.push(Install::new(
                    Kind::AppImage,
                    path.clone(),
                    version,
                    Removal::Files(vec![path]),
                ));
            }
        }
    }

    if !package_owned {
        for path in [
            PathBuf::from("/usr/local/bin/aether1"),
            PathBuf::from("/usr/bin/aether1"),
        ] {
            if !path.is_file() {
                continue;
            }
            let version = machine.version_of(&path);
            found.push(Install::new(
                Kind::SystemBinary,
                path.clone(),
                version,
                Removal::HandOver {
                    command: format!("sudo rm {}", path.display()),
                    why: "this is outside your home directory, and AETHER1 never asks for root",
                },
            ));
        }
    }

    // The checkout is reported rather than offered, so a developer machine does not look
    // like it has a stray install on it.
    if let Some(root) = running_exe.as_deref().and_then(checkout_root) {
        found.push(Install::new(
            Kind::SourceCheckout,
            root,
            machine.running_version(),
            Removal::Keep {
                why: "a source checkout is yours, not an install -- remove it with git",
            },
        ));
    }

    // One row per place, and the running copy marked wherever it turned up.
    let running_exe = running_exe.and_then(|exe| exe.canonicalize().ok());
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut unique = Vec::new();
    for mut install in found {
        if seen.contains(&install.path) {
            continue;
        }
        seen.push(install.path.clone());
        if let (Some(running), Ok(mine)) = (running_exe.as_ref(), install.path.canonicalize()) {
            install.running = &mine == running;
        }
        unique.push(install);
    }
    unique
}

/// Takes one copy away. Refuses the running copy, refuses anything marked `Keep`, and refuses
/// any path that touches the data directories whatever the install claims to own.
pub fn remove(install: &Install, machine: &dyn Machine) -> Result<Outcome, String> {
    if install.running {
        return Err(
            "that is the copy you are running right now -- start the one you are keeping and \
             remove this one from there"
                .to_string(),
        );
    }
    match &install.removal {
        Removal::Keep { why } => Err(format!(
            "{} is not AETHER1's to remove: {why}",
            install.path.display()
        )),
        Removal::HandOver { command, .. } => Ok(Outcome::HandedOver {
            command: command.clone(),
        }),
        Removal::Uninstaller { program, args } => {
            let status = Command::new(program)
                .args(args)
                .status()
                .map_err(|e| format!("could not run {}: {e}", program.display()))?;
            if status.success() {
                Ok(Outcome::Ran {
                    command: program.display().to_string(),
                })
            } else {
                Err(format!(
                    "{} exited with {status} -- nothing was removed",
                    program.display()
                ))
            }
        }
        Removal::Files(paths) => {
            let roots = data_roots(machine.home().as_deref());
            // Checked before anything is deleted rather than as each file comes up: a removal
            // that stops halfway has already done the damage.
            for path in paths {
                if touches_data(path, &roots) {
                    return Err(format!(
                        "refusing to remove {}: your vault, conversations and settings are in \
                         there, and an install is only ever a program",
                        path.display()
                    ));
                }
            }
            let mut removed = Vec::new();
            for path in paths {
                let result = if path.is_dir() {
                    std::fs::remove_dir_all(path)
                } else {
                    std::fs::remove_file(path)
                };
                match result {
                    Ok(()) => removed.push(path.clone()),
                    // A launcher entry or an icon that is already gone is not a failure --
                    // half of these installs never wrote one.
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(format!("could not remove {}: {e}", path.display())),
                }
            }
            if removed.is_empty() {
                Err(format!(
                    "nothing left to remove at {}",
                    install.path.display()
                ))
            } else {
                Ok(Outcome::Removed { paths: removed })
            }
        }
    }
}

/// The copies worth saying something about at startup: everything that is not the running
/// one and is not a source checkout. Empty on the machine of someone who installed AETHER1
/// once, which is the common case and the reason nothing is said then.
pub fn others(installs: &[Install], running: Option<&Version>) -> Vec<Install> {
    installs
        .iter()
        .filter(|install| standing(install, running).offered())
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A machine that does not exist: a home directory in a temp folder, and flat answers
    /// for the package managers and the registry. Everything the scan reads from the real
    /// world goes through this, which is the whole reason `Machine` is a trait.
    struct FakeMachine {
        home: PathBuf,
        running_exe: Option<PathBuf>,
        running_version: Option<Version>,
        versions: Vec<(PathBuf, Version)>,
        deb: Option<String>,
        pacman: Option<String>,
    }

    impl FakeMachine {
        fn new(home: PathBuf) -> FakeMachine {
            FakeMachine {
                home,
                running_exe: None,
                running_version: Some(Version::build(0, 4, 120)),
                versions: Vec::new(),
                deb: None,
                pacman: None,
            }
        }
    }

    impl Machine for FakeMachine {
        fn home(&self) -> Option<PathBuf> {
            Some(self.home.clone())
        }
        fn running_exe(&self) -> Option<PathBuf> {
            self.running_exe.clone()
        }
        fn running_version(&self) -> Option<Version> {
            self.running_version.clone()
        }
        fn version_of(&self, binary: &Path) -> Option<Version> {
            self.versions
                .iter()
                .find(|(path, _)| path == binary)
                .map(|(_, version)| version.clone())
        }
        fn deb_package(&self) -> Option<String> {
            self.deb.clone()
        }
        fn pacman_package(&self) -> Option<String> {
            self.pacman.clone()
        }
        fn windows_install(&self) -> Option<(Option<String>, PathBuf)> {
            None
        }
    }

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_home() -> PathBuf {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("aether1-installs-{}-{unique}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp home");
        dir
    }

    fn write(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, contents).expect("write");
    }

    /// The bundle installs as setup.sh leaves it: binary, launcher entry, icon.
    fn install_user_bundle(home: &Path) -> PathBuf {
        let bin = home.join(".local").join("bin").join("aether1");
        write(&bin, "binary");
        write(
            &home
                .join(".local")
                .join("share")
                .join("applications")
                .join("Aether1.desktop"),
            "[Desktop Entry]",
        );
        write(
            &home
                .join(".local")
                .join("share")
                .join("icons")
                .join("hicolor")
                .join("256x256")
                .join("apps")
                .join("aether1.png"),
            "png",
        );
        bin
    }

    /// Both spellings this project has printed, because this module exists to find copies
    /// installed in the past and those answer `--version` the way they always did. The old
    /// one must keep its own major and minor rather than being restamped with today's: a
    /// 0.3 build is not a 0.4 build, and the label is shown to somebody deciding which copy
    /// to delete.
    #[test]
    fn reads_the_version_scheme_this_app_prints() {
        assert_eq!(
            Version::parse("Ver 0.4.152 (a1b2c3d)"),
            Some(Version::build(0, 4, 152))
        );
        assert_eq!(
            Version::parse("Aether1 0.3.Rev152 (a1b2c3d)"),
            Some(Version::build(0, 3, 152))
        );
        // A build too old to print a major.minor at all still yields its pull request.
        assert_eq!(Version::parse("Rev7"), Some(Version::build(0, 0, 7)));
        assert_eq!(
            Version::parse("0.4.0-1"),
            Some(Version::Opaque("0.4.0-1".into()))
        );
        assert_eq!(Version::parse("   "), None);
        assert_eq!(Version::parse("no numbers here"), None);
    }

    /// The two spellings of the same build are the same build, and the label says which
    /// version it is rather than inventing a scheme of its own.
    #[test]
    fn the_old_and_new_spellings_of_one_build_agree() {
        let new = Version::parse("Ver 0.4.152").unwrap();
        let old = Version::parse("Aether1 0.4.Rev152").unwrap();
        assert_eq!(new, old);
        assert_eq!(new.label(), "0.4.152");
        // Across a version bump, the pull request is still what decides.
        assert_eq!(
            Version::build(0, 4, 152).older_than(&Version::build(0, 5, 160)),
            Some(true)
        );
        assert_eq!(
            Version::build(0, 5, 160).older_than(&Version::build(0, 4, 152)),
            Some(false)
        );
    }

    #[test]
    fn two_revisions_compare_and_anything_else_refuses_to() {
        assert_eq!(
            Version::build(0, 4, 10).older_than(&Version::build(0, 4, 20)),
            Some(true)
        );
        assert_eq!(
            Version::build(0, 4, 20).older_than(&Version::build(0, 4, 10)),
            Some(false)
        );
        // A package manager's number is not on the same line as a Rev, and saying so is the
        // point: an uncomparable pair becomes Standing::Unknown, never Stale by accident.
        assert_eq!(
            Version::Opaque("0.4.0".into()).older_than(&Version::build(0, 4, 10)),
            None
        );
    }

    #[test]
    fn finds_the_bundle_and_the_appimage_and_nothing_else() {
        let home = temp_home();
        let bin = install_user_bundle(&home);
        let appimage = home.join("Applications").join("Aether1-x86_64.AppImage");
        write(&appimage, "appimage");
        // Not AETHER1, and not to be swept up with it.
        write(&home.join("Applications").join("Other.AppImage"), "no");

        let mut machine = FakeMachine::new(home.clone());
        machine
            .versions
            .push((bin.clone(), Version::build(0, 4, 99)));
        machine
            .versions
            .push((appimage.clone(), Version::build(0, 4, 118)));

        let found = detect(&machine);
        let paths: Vec<&PathBuf> = found.iter().map(|install| &install.path).collect();
        assert!(paths.contains(&&bin), "expected the bundle, got {paths:?}");
        assert!(
            paths.contains(&&appimage),
            "expected the AppImage, got {paths:?}"
        );
        assert_eq!(
            found.len(),
            2,
            "nothing else should have been picked up: {paths:?}"
        );
        assert!(found.iter().all(|install| !install.running));

        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn a_newer_copy_is_reported_but_never_offered() {
        let home = temp_home();
        let bin = install_user_bundle(&home);
        let mut machine = FakeMachine::new(home.clone());
        machine.running_version = Some(Version::build(0, 4, 100));
        machine
            .versions
            .push((bin.clone(), Version::build(0, 4, 140)));

        let found = detect(&machine);
        let running = machine.running_version();
        assert_eq!(
            standing(&found[0], running.as_ref()),
            Standing::Newer,
            "a copy ahead of the running one must not be offered for removal"
        );
        assert!(others(&found, running.as_ref()).is_empty());

        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn a_copy_of_the_same_age_somewhere_else_is_still_a_duplicate() {
        let home = temp_home();
        let bin = install_user_bundle(&home);
        let mut machine = FakeMachine::new(home.clone());
        machine.running_version = Some(Version::build(0, 4, 120));
        machine.versions.push((bin, Version::build(0, 4, 120)));

        let found = detect(&machine);
        let running = machine.running_version();
        assert_eq!(standing(&found[0], running.as_ref()), Standing::Duplicate);
        assert_eq!(others(&found, running.as_ref()).len(), 1);

        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn removing_a_bundle_takes_the_program_and_leaves_everything_else() {
        let home = temp_home();
        let bin = install_user_bundle(&home);
        // The things that must survive it: the vault, the database, the settings.
        let vault = home
            .join(".local")
            .join("share")
            .join("aether1")
            .join("vault");
        write(&vault.join("note.md"), "# a note");

        let machine = FakeMachine::new(home.clone());
        let found = detect(&machine);
        let outcome = remove(&found[0], &machine).expect("removal");

        assert!(matches!(outcome, Outcome::Removed { .. }));
        assert!(!bin.exists(), "the old binary should be gone");
        assert!(
            !home
                .join(".local")
                .join("share")
                .join("applications")
                .join("Aether1.desktop")
                .exists(),
            "the launcher entry should be gone with it"
        );
        assert!(
            vault.join("note.md").exists(),
            "the vault must be untouched"
        );

        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn a_removal_that_would_take_data_with_it_is_refused_before_it_starts() {
        let home = temp_home();
        let data = home.join(".local").join("share").join("aether1");
        write(&data.join("backend").join("aether1_memory.db"), "sqlite");
        let stray = home.join(".local").join("bin").join("aether1");
        write(&stray, "binary");

        let machine = FakeMachine::new(home.clone());
        let install = Install::new(
            Kind::UserBundle,
            stray.clone(),
            None,
            Removal::Files(vec![stray.clone(), data.clone()]),
        );

        let error = remove(&install, &machine).expect_err("this must be refused");
        assert!(error.contains("refusing to remove"), "got {error}");
        assert!(data.join("backend").join("aether1_memory.db").exists());
        // Refused *before* anything was deleted, not partway through it.
        assert!(stray.exists(), "nothing should have been removed at all");

        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn the_running_copy_is_never_removed() {
        let home = temp_home();
        let bin = install_user_bundle(&home);
        let mut machine = FakeMachine::new(home.clone());
        machine.running_exe = Some(bin.clone());

        let found = detect(&machine);
        assert!(found[0].running, "the running copy should be marked");
        assert_eq!(standing(&found[0], None), Standing::Running);
        let error = remove(&found[0], &machine).expect_err("this must be refused");
        assert!(error.contains("running right now"), "got {error}");
        assert!(bin.exists());

        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn a_package_is_handed_to_its_package_manager_rather_than_deleted() {
        let home = temp_home();
        let mut machine = FakeMachine::new(home.clone());
        machine.pacman = Some("0.4.0-1".to_string());

        let found = detect(&machine);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, Kind::PacmanPackage);
        match remove(&found[0], &machine).expect("handover") {
            Outcome::HandedOver { command } => assert_eq!(command, "sudo pacman -R aether1"),
            other => panic!("expected a handover, got {other:?}"),
        }

        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn a_checkout_is_reported_and_kept() {
        let home = temp_home();
        let checkout = home.join("code").join("Aether1");
        std::fs::create_dir_all(checkout.join("frontend")).expect("frontend");
        std::fs::create_dir_all(checkout.join("src-tauri")).expect("src-tauri");
        std::fs::create_dir_all(checkout.join(".git")).expect("git");
        let built = checkout
            .join("src-tauri")
            .join("target")
            .join("release")
            .join("aether1");
        write(&built, "binary");

        let mut machine = FakeMachine::new(home.clone());
        machine.running_exe = Some(built);

        let found = detect(&machine);
        let checkout_row = found
            .iter()
            .find(|install| install.kind == Kind::SourceCheckout)
            .expect("the checkout should be reported");
        assert!(matches!(checkout_row.removal, Removal::Keep { .. }));
        assert!(remove(checkout_row, &machine).is_err());
        assert!(others(&found, None).is_empty());

        std::fs::remove_dir_all(&home).ok();
    }
}
