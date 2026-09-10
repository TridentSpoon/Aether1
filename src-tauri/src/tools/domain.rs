// Which reads a persona makes without asking, and how the rest get asked about.
//
// The tool layer already answers one question -- does this change the machine? -- and
// proposes anything that does. This module answers the second: is this read the sort of
// thing this persona is *for*? A System Diagnosis persona reading the system log is doing
// its job. The same call from Creative Work is worth a glance, so it becomes a proposal.
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
use crate::llm::{Domain, Root};

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

/// Whether `path` (as the model wrote it) falls inside the domain's roots.
///
/// Resolved before comparing, symlinks and `..` included: judging a path by its spelling
/// is how `~/project/../.ssh` gets counted as project work. A path that does not resolve
/// is not inside anything, which means it is proposed -- and then refused by fs_guard for
/// the same reason, one step later.
fn within_roots(domain: &Domain, db: &MemoryDb, path: &str) -> bool {
    let Ok(resolved) = crate::paths::expand_home(path).canonicalize() else {
        return false;
    };
    domain
        .roots
        .iter()
        .flat_map(|root| directories(*root, db))
        .any(|dir| contains(&dir, &resolved))
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
    let field = domain.field();
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
    if within_roots(&domain, ctx.db, path) {
        return None;
    }

    Some(format!(
        "{path} is outside {speciality}'s field ({field}). Approving runs this one read. \
         It will ask again next time."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::Persona;

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

    /// The home directory is not a project. If it were, the Coding persona's field would be
    /// nearly the whole disk, which is the thing a field exists to prevent.
    #[test]
    fn home_is_never_the_project_tree() {
        let db = temp_db("project_tree");
        let home = crate::paths::home_dir().unwrap();
        let domain = Persona::Nexus.domain();
        assert!(
            !within_roots(&domain, &db, &home.display().to_string()),
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
        assert!(within_roots(&domain, &db, &vault.display().to_string()));
        if cfg!(unix) && Path::new("/var/log").exists() {
            assert!(!within_roots(&domain, &db, "/var/log"));
            assert!(within_roots(&Persona::Default.domain(), &db, "/var/log"));
        }
    }
}
