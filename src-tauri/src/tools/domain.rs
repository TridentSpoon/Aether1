// Which reads a persona makes without asking, and how the rest get asked about.
//
// The tool layer already answers one question -- does this change the machine? -- and
// proposes anything that does. This module answers the second: is this read the sort of
// thing this persona is *for*? A System Diagnosis persona reading the system log is doing
// its job. The same call from Signal & Logic is worth a glance, so it becomes a proposal.
//
// Three properties are worth stating plainly, because each of them is easy to lose:
//
//   1. This narrows, it never widens. fs_guard is checked inside the tool, after this, and
//      its deny list beats a domain, an approval and an elevation alike. Security & White
//      Hat having network configuration in its field does not give it ~/.ssh/id_rsa.
//   2. Elevation lasts exactly one call. There is no mode, no session flag, no timed
//      grant. Approving a proposal runs that proposal; the next call, identical or not,
//      asks again. A grant that stays on is the old behaviour wearing a lock icon.
//   3. An unresolvable root contains nothing rather than everything. If the platform has
//      no answer for "where are the service definitions", the domain simply does not cover
//      that read and the operator is asked -- wrong in the direction that costs a click
//      rather than the one that costs a secret.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{Tool, ToolContext};
use crate::llm::MemoryDb;
use crate::llm::{Domain, Persona, Root};

/// Setting holding the folders the operator has added to a persona's field, as
/// `{"nexus": ["/home/you/Projects"], ...}`.
const EXTRA_ROOTS_SETTING: &str = "domain_extra_roots";

/// How many folders one persona may be given. A cap rather than a judgement: a list long
/// enough to be unreadable is a list nobody is checking, and the point of the field is that
/// the operator can see what it is at a glance.
pub const MAX_EXTRA_ROOTS: usize = 12;

/// Where a root actually is on this machine, right now. Empty when the platform has no
/// answer -- see property 3 above.
pub fn directories(root: Root, db: &MemoryDb) -> Vec<PathBuf> {
    let exists = |candidates: &[&str]| -> Vec<PathBuf> {
        candidates
            .iter()
            .map(PathBuf::from)
            .filter(|p| p.exists())
            .collect()
    };

    // Under %SystemRoot%, keeping only what is actually there. Windows is not always on
    // C:, so the prefix is read from the environment rather than assumed.
    let under_system_root = |tails: &[&[&str]]| -> Vec<PathBuf> {
        let Some(system_root) = crate::paths::system_root() else {
            return Vec::new();
        };
        tails
            .iter()
            .map(|tail| {
                tail.iter()
                    .fold(system_root.clone(), |p, part| p.join(part))
            })
            .filter(|p| p.exists())
            .collect()
    };

    match root {
        Root::SystemLogs => {
            if cfg!(windows) {
                // The event logs, which is what "check the event viewer" means, plus the
                // servicing and setup logs beside them.
                under_system_root(&[&["System32", "winevt", "Logs"], &["Logs"]])
            } else {
                exists(&["/var/log"])
            }
        }
        Root::ServiceState => {
            if cfg!(windows) {
                // Windows has no service-state *file*: services live in the registry, and
                // what they are doing right now comes from list_processes and the System
                // event log, both of which these personas already hold. So this root points
                // at the event log rather than pretending a directory of unit files exists.
                under_system_root(&[&["System32", "winevt", "Logs"]])
            } else {
                exists(&[
                    "/etc/systemd",
                    "/etc/init.d",
                    "/lib/systemd/system",
                    "/usr/lib/systemd/system",
                    "/run/systemd",
                ])
            }
        }
        Root::NetworkConfig => {
            if cfg!(windows) {
                // hosts, services, protocol and networks -- the same four files Unix keeps
                // in /etc, in the one place Windows keeps them.
                under_system_root(&[&["System32", "drivers", "etc"]])
            } else {
                exists(&[
                    "/etc/hosts",
                    "/etc/resolv.conf",
                    "/etc/nsswitch.conf",
                    "/etc/network",
                    "/etc/netplan",
                    "/etc/iptables",
                    "/etc/NetworkManager",
                    "/etc/nftables.conf",
                    "/proc/net",
                ])
            }
        }
        Root::ProjectTree => project_tree().into_iter().collect(),
        Root::Vault => vec![crate::vault::vault_path(db)],
    }
}

/// The working directory, but only when it is plausibly a project *and* somewhere the path
/// guard will actually let a tool read.
///
/// Two rules, and both matter for the same reason.
///
/// The companion is often launched from the operator's home directory, and a ProjectTree
/// that resolved to `~` would quietly make the Coding persona's field nearly the whole disk
/// -- the opposite of what a field is for. So home itself, the filesystem root, and any
/// ancestor of home resolve to no project at all.
///
/// And it must sit *inside* home, because that is the only place `fs_guard` permits reading
/// outside the system roots. Started from `/opt/something`, the directory is a real project
/// and every read of it would still be refused -- so naming it as the persona's field would
/// advertise access that does not exist, and elevation would not conjure it either. Better
/// to say there is no project than to point at one nothing can open.
fn project_tree() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?.canonicalize().ok()?;
    cwd.parent()?;
    let home = crate::paths::home_dir()?.canonicalize().ok()?;
    if cwd == home || !cwd.starts_with(&home) {
        return None;
    }
    Some(cwd)
}

/// The folders the operator has added to this persona's field.
///
/// Stored per persona rather than globally, because a field that widens for everyone at
/// once is not a field. Adding the project directory to the Coding persona says something
/// specific; adding it to all twelve says only that the idea has been abandoned.
pub fn extra_roots(db: &MemoryDb, persona: &Persona) -> Vec<String> {
    db.get_setting(EXTRA_ROOTS_SETTING)
        .ok()
        .flatten()
        .and_then(|v| {
            serde_json::from_value::<std::collections::HashMap<String, Vec<String>>>(v).ok()
        })
        .and_then(|mut map| map.remove(persona.key()))
        .unwrap_or_default()
}

/// Replaces the folders in this persona's field, returning what was stored and what was
/// refused.
///
/// Every path is checked with the same guard the tools themselves use, at the moment it is
/// saved. That ordering is the whole point: a folder the guard would refuse can be typed
/// into the box, and storing it would leave the operator believing they had granted a
/// access that does not exist -- the panel would say the persona reads it, and every read
/// would still be refused. Better to say no while someone is looking at the box.
///
/// This widens what runs *without asking*. It cannot widen what may be read at all:
/// `fs_guard` runs inside each tool, after this, and its deny list beats a domain, an
/// approval and an operator's setting alike.
pub fn set_extra_roots(
    db: &MemoryDb,
    persona: &Persona,
    paths: &[String],
) -> Result<(Vec<String>, Vec<String>), String> {
    let mut accepted: Vec<String> = Vec::new();
    let mut refused: Vec<String> = Vec::new();

    for raw in paths {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        if accepted.len() >= MAX_EXTRA_ROOTS {
            refused.push(format!(
                "{raw} — a persona may be given at most {MAX_EXTRA_ROOTS} folders"
            ));
            continue;
        }
        match super::fs_guard::resolve_readable(raw) {
            Ok(resolved) => {
                // The filesystem root would make the field everything, which is the one
                // outcome a field exists to prevent. Home is allowed: it is a real answer
                // to "where do I keep my things", and the deny list still covers the keys
                // and credentials inside it.
                if resolved.parent().is_none() {
                    refused.push(format!(
                        "{raw} — the whole filesystem is not a field. Name a folder inside it"
                    ));
                    continue;
                }
                let resolved = resolved.display().to_string();
                if !accepted.contains(&resolved) {
                    accepted.push(resolved);
                }
            }
            Err(e) => refused.push(format!("{raw} — {e}")),
        }
    }

    let mut map: std::collections::HashMap<String, Vec<String>> = db
        .get_setting(EXTRA_ROOTS_SETTING)
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    if accepted.is_empty() {
        map.remove(persona.key());
    } else {
        map.insert(persona.key().to_string(), accepted.clone());
    }
    db.set_setting(EXTRA_ROOTS_SETTING, &serde_json::json!(map))
        .map_err(|e| format!("could not save the folder list: {e}"))?;

    Ok((accepted, refused))
}

/// The persona's field in words, including anything the operator added to it.
///
/// Derived at the moment it is shown rather than stored, for the same reason `Domain::field`
/// is: a description kept beside the thing it describes drifts from it, and this one is read
/// by an operator deciding whether to approve something.
pub fn field_with_extras(db: &MemoryDb, persona: &Persona) -> String {
    let base = persona.domain().field();
    let extra = extra_roots(db, persona);
    match extra.len() {
        0 => base,
        1 => format!("{base}, and {}", extra[0]),
        n => format!("{base}, and {n} folders you added"),
    }
}

/// Whether `path` (as the model wrote it) falls inside the domain's roots.
///
/// Resolved before comparing, symlinks and `..` included: judging a path by its spelling
/// is how `~/project/../.ssh` gets counted as project work. A path that does not resolve
/// is not inside anything, which means it is proposed -- and then refused by fs_guard for
/// the same reason, one step later.
fn within_roots(domain: &Domain, db: &MemoryDb, persona: &Persona, path: &str) -> bool {
    let Ok(resolved) = crate::paths::expand_home(path).canonicalize() else {
        return false;
    };
    let declared = domain
        .roots
        .iter()
        .flat_map(|root| directories(*root, db))
        .any(|dir| contains(&dir, &resolved));
    declared
        || extra_roots(db, persona)
            .iter()
            .any(|dir| contains(Path::new(dir), &resolved))
}

/// True when `dir` is `candidate` or an ancestor of it. A resolved root may itself be a
/// file (/etc/hosts is network configuration, and it is not a directory), so equality
/// counts as well as containment.
fn contains(dir: &Path, candidate: &Path) -> bool {
    match dir.canonicalize() {
        Ok(dir) => candidate == dir || candidate.starts_with(&dir),
        Err(_) => false,
    }
}

/// Why this read falls outside the active persona's field, or None when it does not.
///
/// The string is written for the operator rather than the model: it names the persona, the
/// field, and -- crucially -- that approving grants one read and nothing more. An operator
/// who believes they are granting a standing permission approves differently from one who
/// knows they are not.
pub fn elevation_needed(ctx: &ToolContext, tool: &dyn Tool, args: &Value) -> Option<String> {
    // Mutating tools have their own gate and their own reason for existing; a domain has
    // nothing to add to "this would change your machine".
    if tool.mutating() {
        return None;
    }

    let persona = &ctx.persona;
    let domain = persona.domain();
    let field = field_with_extras(ctx.db, persona);
    let speciality = persona.short_name();

    if !domain.allows_tool(tool.name()) {
        return Some(format!(
            "{} is outside {speciality}'s field ({field}). Approving runs this one call. \
             It will ask again next time.",
            tool.name()
        ));
    }

    // Only the tools that take a path are subject to roots; the rest were settled above.
    let path = args.get("path").and_then(Value::as_str)?;
    if within_roots(&domain, ctx.db, persona, path) {
        return None;
    }

    Some(format!(
        "{path} is outside {speciality}'s field ({field}). Approving runs this one read, and \
         it will ask again next time — add the folder to {speciality}'s field in Settings to \
         stop being asked about it."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::Persona;

    /// A real directory under the operator's home, because that is where the path guard
    /// permits reading and therefore the only place `set_extra_roots` will accept. Building
    /// these in /tmp is what the first version of these tests did, and the guard refused
    /// every one of them -- correctly.
    /// Holds the environment still for the duration of a test.
    ///
    /// `HOME` is process-global and `fs_guard`'s tests move it about to describe their own
    /// rules; these tests resolve real paths under the real home, so one running while the
    /// other had `HOME` swapped out made `within_roots` disagree with itself. It was
    /// intermittent, and only on a compiler new enough to schedule the two together --
    /// which is to say it was a coin toss that had been landing the right way. The lock is
    /// `fs_guard`'s own, because a second lock would protect nothing from the first.
    fn hold_the_environment() -> std::sync::MutexGuard<'static, ()> {
        crate::tools::fs_guard::ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn dir_under_home(name: &str) -> PathBuf {
        let dir = crate::paths::home_dir()
            .unwrap()
            .join(format!("aether1_test_{name}_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn temp_db(name: &str) -> MemoryDb {
        let path =
            std::env::temp_dir().join(format!("aether1_domain_{name}_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        MemoryDb::open(path).unwrap()
    }

    #[test]
    fn every_persona_has_a_domain_of_real_tools() {
        let registry = super::super::registry();
        for persona in Persona::all() {
            let domain = persona.domain();
            assert!(
                !domain.tools.is_empty(),
                "{} has no tools at all, which is a domain nobody can use",
                persona.key()
            );
            for tool in domain.tools {
                let found = registry.get(tool).unwrap_or_else(|| {
                    panic!("{} names a tool that does not exist: {tool}", persona.key())
                });
                assert!(
                    !found.mutating(),
                    "{} lists the mutating tool {tool}: a domain decides what runs without \
                     asking, and nothing that changes the machine may be on that list",
                    persona.key()
                );
            }
        }
    }

    #[test]
    fn the_field_reads_as_a_sentence() {
        assert_eq!(
            Persona::Alt.domain().field(),
            "network configuration and service state"
        );
        assert_eq!(Persona::ArxLogos.domain().field(), "your notes");
        assert_eq!(
            Persona::ArxLimes.domain().field(),
            "your notes and the project directory"
        );
    }

    /// The operator adding a folder is what makes the read stop being asked about -- that
    /// is the whole feature, and it is the assertion that would fail if the extra roots
    /// were stored but never consulted.
    #[test]
    fn a_folder_the_operator_adds_becomes_part_of_the_field() {
        let db = temp_db("extra_roots_gate");
        let _environment = hold_the_environment();
        let dir = dir_under_home("extra");
        let inside = dir.join("notes.txt");
        std::fs::write(&inside, "hello").unwrap();

        let persona = Persona::Nexus;
        let domain = persona.domain();
        assert!(
            !within_roots(&domain, &db, &persona, &inside.display().to_string()),
            "nothing is in the field before the operator puts it there"
        );

        let (accepted, refused) =
            set_extra_roots(&db, &persona, &[dir.display().to_string()]).unwrap();
        assert_eq!(accepted.len(), 1, "the folder was accepted: {refused:?}");
        assert!(
            within_roots(&domain, &db, &persona, &inside.display().to_string()),
            "a file inside an added folder is inside the field"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Widening one persona says nothing about the others. A grant that leaked across all
    /// of them would leave the fields intact on screen while meaning nothing.
    #[test]
    fn widening_one_persona_does_not_widen_another() {
        let db = temp_db("extra_roots_scope");
        let _environment = hold_the_environment();
        let dir = dir_under_home("scope");
        let inside = dir.join("f.txt");
        std::fs::write(&inside, "x").unwrap();

        set_extra_roots(&db, &Persona::Nexus, &[dir.display().to_string()]).unwrap();

        assert!(within_roots(
            &Persona::Nexus.domain(),
            &db,
            &Persona::Nexus,
            &inside.display().to_string()
        ));
        assert!(
            !within_roots(
                &Persona::ArxLucre.domain(),
                &db,
                &Persona::ArxLucre,
                &inside.display().to_string()
            ),
            "a folder given to one persona is not given to every persona"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A path the guard would refuse is refused while the operator is looking at it, rather
    /// than stored and silently ineffective. Storing it would put a folder on the panel that
    /// the persona cannot actually read.
    #[test]
    fn a_folder_the_guard_refuses_is_never_stored() {
        let db = temp_db("extra_roots_refused");
        let (accepted, refused) = set_extra_roots(
            &db,
            &Persona::Nexus,
            &["/definitely/not/a/real/path/here".to_string()],
        )
        .unwrap();
        assert!(accepted.is_empty(), "nothing readable was named");
        assert_eq!(refused.len(), 1, "and the operator is told which one");
        assert!(extra_roots(&db, &Persona::Nexus).is_empty());
    }

    /// Granting the filesystem root would make "field" meaningless, so it is the one path
    /// refused by name rather than by the guard.
    #[test]
    fn the_whole_filesystem_is_not_a_field() {
        let db = temp_db("extra_roots_slash");
        let root = if cfg!(windows) { "C:\\" } else { "/" };
        let (accepted, refused) =
            set_extra_roots(&db, &Persona::Nexus, &[root.to_string()]).unwrap();
        assert!(accepted.is_empty(), "the filesystem root is not a folder");
        assert_eq!(refused.len(), 1);
    }

    /// A field with folders added says so. The sentence is what an operator reads when
    /// deciding whether to approve something, and a field description that omits half the
    /// field is the same class of untruth as a made-up number on a panel.
    #[test]
    fn the_described_field_includes_what_was_added() {
        let db = temp_db("extra_roots_field");
        let _environment = hold_the_environment();
        let dir = dir_under_home("field");

        let persona = Persona::Nexus;
        assert_eq!(field_with_extras(&db, &persona), persona.domain().field());

        set_extra_roots(&db, &persona, &[dir.display().to_string()]).unwrap();
        let described = field_with_extras(&db, &persona);
        assert!(
            described.len() > persona.domain().field().len(),
            "the added folder is part of the sentence: {described}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The home directory is not a project. If it were, the Coding persona's field would be
    /// nearly the whole disk, which is the thing a field exists to prevent.
    #[test]
    fn home_is_never_the_project_tree() {
        let db = temp_db("project_tree");
        let home = crate::paths::home_dir().unwrap();
        let domain = Persona::Nexus.domain();
        assert!(
            !within_roots(&domain, &db, &Persona::Nexus, &home.display().to_string()),
            "the operator's home directory must not count as a project"
        );
    }

    /// Every root that means something on this platform resolves to somewhere the path
    /// guard will actually let a tool read. A domain root the guard refuses is decorative:
    /// it reads as access the persona has and does not.
    #[test]
    fn every_resolved_root_is_readable_by_the_path_guard() {
        let db = temp_db("roots_readable");
        for root in [
            Root::SystemLogs,
            Root::ServiceState,
            Root::NetworkConfig,
            Root::ProjectTree,
        ] {
            for dir in directories(root, &db) {
                let as_text = dir.display().to_string();
                assert!(
                    super::super::fs_guard::resolve_readable(&as_text).is_ok(),
                    "{root:?} resolves to {as_text}, which the path guard refuses -- a \
                     domain root the guard denies is access the persona appears to have \
                     and does not"
                );
            }
        }
    }

    #[test]
    fn a_root_that_this_platform_has_no_answer_for_is_empty_not_everything() {
        let db = temp_db("empty_root");
        if !cfg!(windows) {
            // The Windows-only roots have no Unix answer and must stay empty here rather
            // than falling back to something broad.
            assert!(crate::paths::system_root().is_none());
        }
        // Whatever this platform resolves, no root may ever resolve to the filesystem root:
        // "everything" is not a field.
        for root in [
            Root::SystemLogs,
            Root::ServiceState,
            Root::NetworkConfig,
            Root::ProjectTree,
            Root::Vault,
        ] {
            for dir in directories(root, &db) {
                assert!(
                    dir.parent().is_some(),
                    "{root:?} resolved to the filesystem root"
                );
            }
        }
    }

    #[test]
    fn the_vault_is_in_the_minimum_domain_and_a_system_log_is_not() {
        let db = temp_db("vault_in_domain");
        // Pointed at a directory of its own rather than the default under HOME: other test
        // modules move HOME about under a mutex, and this test has nothing to say about
        // where the vault lives by default.
        let dir = std::env::temp_dir().join(format!("aether1_domain_vault_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        db.set_setting("vault_path", &serde_json::json!(dir.display().to_string()))
            .unwrap();
        let vault = crate::vault::ensure(&db).unwrap();
        let domain = Persona::Halcy.domain();
        assert!(within_roots(
            &domain,
            &db,
            &Persona::Halcy,
            &vault.display().to_string()
        ));
        if cfg!(unix) && Path::new("/var/log").exists() {
            assert!(!within_roots(&domain, &db, &Persona::Halcy, "/var/log"));
            assert!(within_roots(
                &Persona::Default.domain(),
                &db,
                &Persona::Default,
                "/var/log"
            ));
        }
    }
}
