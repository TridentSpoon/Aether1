//! What AETHER CODE will actually do, and which answer decided it.
//!
//! Five grants in `code_perms`, four switches behind a level in `code_policy`, a domain list
//! and a folder list in the project's own file, local-only mode, and whether this machine
//! can confine a command at all. Every one of them is defensible on its own. Together they
//! can disagree, and when they disagree the operator is the person who finds out last.
//!
//! `aether1 code perms` prints `internet  ON`. With local-only mode switched on, nothing in
//! AETHER1 reaches the internet -- `code_perms::check_url` refuses before the URL is even
//! parsed. The list is not wrong about the switch and it is wrong about the world, which is
//! the only thing the operator was asking. The same gap sits under `run`: the switch can be
//! on, and on a machine with no usable bubblewrap every command is refused anyway.
//!
//! So this module answers the question the switches cannot:
//!
//! > For each thing AETHER CODE might do: will it happen, and who said so?
//!
//! **It decides nothing.** Every answer below is read from the module that already owns it
//! -- `code_perms::granted`, `local_only::enabled`, `code_sandbox::detect`,
//! `code_policy::effective`, `code_proxy::allowed`. A second place that worked out whether a
//! command may run would be a second policy, and two policies that agree today are two
//! policies that disagree after the next change. If this report and the real behaviour ever
//! differ, the bug is in this file, and the fix is to call the decider rather than to
//! reproduce its reasoning.
//!
//! **Why a layer is worth naming.** "Commands are refused" is half an answer; an operator who
//! reads it goes and turns the switch on, and is refused again, because it was the machine
//! that said no. Naming the layer turns the row into something that can be acted on: a
//! switch the operator can flip, a level they chose, a line in a file that travels with the
//! repository, local-only mode they asked for, or a fact about the machine that no setting
//! will move.

use std::path::PathBuf;

use crate::code_policy::Level;
use crate::llm::MemoryDb;

// ------------------------------------------------------------------ what a row says

/// What will happen when AETHER CODE tries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// It goes ahead without anybody being asked.
    Allow,
    /// The operator is asked at the moment it happens. Not a softer `Allow`: an unattended
    /// run stops here, which is the difference between a thing that works overnight and a
    /// thing that waits for somebody.
    Ask,
    /// It is refused. The `rule` says in what words.
    Deny,
}

impl Action {
    pub fn key(self) -> &'static str {
        match self {
            Action::Allow => "allow",
            Action::Ask => "ask",
            Action::Deny => "deny",
        }
    }

    /// The word for a row, chosen so that `Deny` does not read as a failure. Refusing to
    /// write files is this product working, not this product broken.
    pub fn title(self) -> &'static str {
        match self {
            Action::Allow => "Allowed",
            Action::Ask => "Asks first",
            Action::Deny => "Blocked",
        }
    }
}

/// Which answer decided a row.
///
/// Ordered by how hard it is to change, loosest first, because that is the order an operator
/// needs: a switch is one click, a level is a decision about a project, a machine is a
/// different machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Nobody has said anything, so the grant's own default stands.
    Default,
    /// A switch in Settings, set by hand.
    Switch,
    /// The level this project is trusted at, which wrote the switch.
    Level,
    /// The project's own `.aether/policy.json` -- its domains or its folders.
    Project,
    /// No project folder is nominated, so there is nothing for the row to apply to.
    NoWorkspace,
    /// Local-only mode, which outranks a permission left on by default.
    LocalOnly,
    /// `AETHER1_LOCAL_ONLY` in this process's environment, which outranks the setting too
    /// and cannot be turned off from inside the app.
    Environment,
    /// What this machine can actually do. No setting moves it.
    Machine,
    /// Which model server AETHER1 itself is pointed at. Changed in Settings like a switch,
    /// but not one of AETHER CODE's switches -- a row that said `switch` would send an
    /// operator hunting through Permissions for something that lives beside the model.
    Model,
}

impl Source {
    pub fn key(self) -> &'static str {
        match self {
            Source::Default => "default",
            Source::Switch => "switch",
            Source::Level => "level",
            Source::Project => "project",
            Source::NoWorkspace => "no-workspace",
            Source::LocalOnly => "local-only",
            Source::Environment => "environment",
            Source::Machine => "machine",
            Source::Model => "model",
        }
    }

    /// Where the operator goes to change it, or why they cannot.
    pub fn title(self) -> &'static str {
        match self {
            Source::Default => "Not answered yet",
            Source::Switch => "A switch in Settings",
            Source::Level => "This project's level",
            Source::Project => "This project's policy file",
            Source::NoWorkspace => "No project folder nominated",
            Source::LocalOnly => "Local-only mode",
            Source::Environment => "The environment it was started in",
            Source::Machine => "This machine",
            Source::Model => "The model AETHER1 is on",
        }
    }
}

/// One capability, as the operator talks about it, with the answer and who gave it.
#[derive(Debug, Clone)]
pub struct Row {
    /// Stable id for the HUD and for a test. Not shown.
    pub key: &'static str,
    /// The operator-facing name.
    pub title: &'static str,
    pub action: Action,
    pub decided_by: Source,
    /// One sentence saying what will happen. Written to be read on its own, because in a
    /// narrow HUD it is the only part of the row that survives.
    pub rule: String,
    /// Rules narrower than this row: the exceptions, the allowlists, the named folders. A
    /// row that says `Allowed` with four exceptions is a different fact from a row that says
    /// `Allowed`, and collapsing them is how a permission page becomes decoration.
    pub exceptions: Vec<String>,
}

/// The whole answer for one project.
#[derive(Debug, Clone)]
pub struct Report {
    /// The nominated project folder, if there is one.
    pub root: Option<PathBuf>,
    /// The level its file records. `None` when no folder is nominated.
    pub level: Option<Level>,
    /// Whether the switches the level means are still the ones in force. `false` is the
    /// answer to "I turned the network off by hand": the rows below are right, and the
    /// level named above is no longer a description of them.
    pub level_applied: bool,
    pub rows: Vec<Row>,
}

impl Report {
    /// Whether anything here would stop a command. The HUD uses it to decide whether the
    /// section needs attention; `aether1 code effective` uses it for its last line.
    pub fn anything_blocked(&self) -> bool {
        self.rows.iter().any(|r| r.action == Action::Deny)
    }
}

// ------------------------------------------------------------------ reading the answer

/// Whether a setting has been written, as opposed to standing at its default. The
/// difference between "the operator chose this" and "nobody has been asked" is the whole
/// of `Source::Default`, and `get_setting_bool` cannot tell them apart.
fn was_set(db: &MemoryDb, key: &str) -> bool {
    matches!(db.get_setting(key), Ok(Some(_)))
}

/// Which layer wrote one of a level's four switches.
///
/// A level and a hand-set switch write the same row in the same table, so this cannot be
/// read off the value. What it can do is ask whether the level's whole set is in force: if
/// it is, the level is the honest answer for every switch in it, and if it is not, somebody
/// has moved one by hand and `Switch` is the honest answer for all of them.
fn switch_source(db: &MemoryDb, key: &str, level: Option<Level>, applied: bool) -> Source {
    match level {
        Some(_) if applied => Source::Level,
        _ if was_set(db, key) => Source::Switch,
        _ => Source::Default,
    }
}

/// The source of a grant that no level touches -- the three reading grants.
fn grant_source(db: &MemoryDb, grant: crate::code_perms::Grant) -> Source {
    if was_set(db, grant.setting()) {
        Source::Switch
    } else {
        Source::Default
    }
}

/// Builds the report.
pub fn report(db: &MemoryDb) -> Report {
    let root = crate::code_workspace::root(db).ok();
    let (level, level_applied) = match root.as_deref() {
        Some(root) => {
            let (level, applied) = crate::code_policy::effective(root, db);
            (Some(level), applied)
        }
        // Without a folder there is no policy file, so there is no level to be out of step
        // with. `true` here means "nothing to report", not "everything matches".
        None => (None, true),
    };
    let sandbox = crate::code_sandbox::detect();

    let rows = vec![
        read_row(db),
        github_row(db),
        web_row(db, crate::local_only::env_forced()),
        edit_row(db, root.as_deref(), level, level_applied),
        run_row(db, &sandbox, level, level_applied),
        network_row(db, &sandbox, root.as_deref(), level, level_applied),
        brain_row(db, &sandbox),
        folders_row(root.as_deref(), level),
    ];

    Report {
        root,
        level,
        level_applied,
        rows,
    }
}

fn read_row(db: &MemoryDb) -> Row {
    use crate::code_perms::Grant;
    let on = crate::code_perms::granted(db, Grant::System);
    Row {
        key: "read",
        title: "Read this machine",
        action: if on { Action::Allow } else { Action::Deny },
        decided_by: grant_source(db, Grant::System),
        rule: if on {
            "Reads any file and folder your account can read, and what the hardware is.".into()
        } else {
            "Reads nothing on this machine, including the project folder.".into()
        },
        // Named because an operator who reads "reads any file" should know the one place it
        // does not, and because it is the gap that `code_openat` exists to close.
        exceptions: if on {
            vec!["A path is resolved and opened as one step, so a link swapped underneath it cannot redirect the read.".into()]
        } else {
            Vec::new()
        },
    }
}

fn github_row(db: &MemoryDb) -> Row {
    use crate::code_perms::Grant;
    let on = crate::code_perms::granted(db, Grant::Github);
    Row {
        key: "github",
        title: "GitHub",
        action: if on { Action::Allow } else { Action::Deny },
        decided_by: grant_source(db, Grant::Github),
        rule: if on {
            "Runs the read-only `gh` subcommands with your own signed-in account.".into()
        } else {
            "Runs no `gh` command at all.".into()
        },
        exceptions: if on {
            vec![
                "Only a listed (command, verb) pair runs; anything not on the list is refused whatever it does.".into(),
                "`gh api` is judged by its method and by the flags that imply a body, not by its name.".into(),
                "`auth token`, `repo clone`, `pr checkout` and `browse` are refused: they print a credential, write files, or act on your desktop.".into(),
            ]
        } else {
            Vec::new()
        },
    }
}

/// `env_forced` is passed in rather than read here so that the ordering below can be tested
/// without setting a variable in the process -- half this suite calls `local_only::enabled`,
/// and a test that turns local-only on for everybody for a moment is a flake waiting to
/// happen in whichever test runs beside it.
fn web_row(db: &MemoryDb, env_forced: bool) -> Row {
    use crate::code_perms::Grant;
    let on = crate::code_perms::granted(db, Grant::Internet);

    // The order matters and it is the whole point of this row. Local-only mode is checked
    // before the grant because that is the order `code_perms::check_url` checks them in, and
    // a report that put the grant first would print `Allowed` for a machine that refuses
    // every fetch.
    if env_forced {
        return Row {
            key: "web",
            title: "The internet",
            action: Action::Deny,
            decided_by: Source::Environment,
            rule: "AETHER1_LOCAL_ONLY is set in this process's environment, so nothing reaches the internet. The switch in Settings cannot turn this off.".into(),
            exceptions: Vec::new(),
        };
    }
    if crate::local_only::enabled(db) {
        return Row {
            key: "web",
            title: "The internet",
            action: Action::Deny,
            decided_by: Source::LocalOnly,
            // Said in two sentences rather than one with a dash in it, because the second
            // half is the whole message: the operator is looking at a switch that is on,
            // and the row has to tell them it is not the one deciding.
            rule: format!(
                "Local-only mode is on, so nothing reaches the internet.{}",
                if on {
                    " The internet permission is on and does not apply."
                } else {
                    ""
                }
            ),
            exceptions: Vec::new(),
        };
    }

    Row {
        key: "web",
        title: "The internet",
        action: if on { Action::Allow } else { Action::Deny },
        decided_by: grant_source(db, Grant::Internet),
        rule: if on {
            "Fetches a public http or https page when it needs documentation.".into()
        } else {
            "Fetches nothing. It answers from what the model knows, or asks you to paste the page in.".into()
        },
        exceptions: if on {
            vec![
                "Only http and https; any other scheme is refused.".into(),
                "An address on this machine or this network is refused, before and again after a redirect: the permission is for public pages, not for the services on your LAN.".into(),
            ]
        } else {
            Vec::new()
        },
    }
}

fn edit_row(
    db: &MemoryDb,
    root: Option<&std::path::Path>,
    level: Option<Level>,
    level_applied: bool,
) -> Row {
    use crate::code_perms::Grant;
    let on = crate::code_perms::granted(db, Grant::Edit);
    let source = switch_source(db, Grant::Edit.setting(), level, level_applied);

    let Some(root) = root else {
        return Row {
            key: "edit",
            title: "Change files",
            action: Action::Deny,
            decided_by: Source::NoWorkspace,
            rule: "No project folder is nominated, so there is nowhere it may write. Name one with `aether1 code workspace <path>`.".into(),
            exceptions: Vec::new(),
        };
    };

    let writable: Vec<String> = crate::code_policy::mounts(root)
        .into_iter()
        .filter(|m| m.write)
        .map(|m| {
            format!(
                "{} is writable too, named in this project's policy file.",
                m.path.display()
            )
        })
        .collect();

    Row {
        key: "edit",
        title: "Change files",
        action: if on { Action::Allow } else { Action::Deny },
        decided_by: source,
        rule: if on {
            format!(
                "Creates and edits files inside {}, and nowhere else.",
                root.display()
            )
        } else {
            format!(
                "Changes nothing. It can read {} and it writes a change you want into your terminal for you to run.",
                root.display()
            )
        },
        exceptions: if on { writable } else { Vec::new() },
    }
}

fn run_row(
    db: &MemoryDb,
    sandbox: &crate::code_sandbox::Sandbox,
    level: Option<Level>,
    level_applied: bool,
) -> Row {
    use crate::code_perms::Grant;
    let on = crate::code_perms::granted(db, Grant::Run);
    let unconfined = crate::code_sandbox::unconfined_allowed(db);

    // The machine is checked before the switch, for the same reason local-only is checked
    // before the internet grant: on a host with no usable bubblewrap `run` refuses whatever
    // the switch says, and the only useful thing to tell the operator is that it is not
    // theirs to fix from Settings.
    if !sandbox.confines() && !unconfined {
        return Row {
            key: "run",
            title: "Run commands",
            action: Action::Deny,
            decided_by: Source::Machine,
            rule: format!(
                "Runs nothing: {}. Install bubblewrap, or switch the project to Unrestricted and accept that commands then run as you.",
                sandbox.description()
            ),
            exceptions: Vec::new(),
        };
    }

    // Local-only mode on a machine that cannot cut the network off from a command: run
    // refuses, whatever the switches say, because the promise cannot be kept for one (see
    // code_sandbox::local_only_refusal). Reported before the unconfined switch so the row
    // names the thing actually deciding -- a row saying "runs commands as you" under a mode
    // that will refuse every one of them is the contradiction this whole area keeps
    // producing.
    if crate::local_only::enabled(db) && !sandbox.can_cut_network() {
        return Row {
            key: "run",
            title: "Run commands",
            action: Action::Deny,
            decided_by: Source::LocalOnly,
            rule: format!(
                "Runs nothing while local-only mode is on: {}. A command here would have your network whatever any switch says, so it is refused rather than run with a promise that cannot be kept. Install bubblewrap, use a machine that can cut the network, or turn local-only mode off.",
                sandbox.description()
            ),
            exceptions: Vec::new(),
        };
    }

    if unconfined {
        return Row {
            key: "run",
            title: "Run commands",
            action: if on { Action::Allow } else { Action::Deny },
            decided_by: switch_source(
                db,
                crate::code_sandbox::UNCONFINED_SETTING,
                level,
                level_applied,
            ),
            rule: if on {
                "Runs commands as you, unconfined. There is no sandbox: a command can read and write everything your account can reach.".into()
            } else {
                "The sandbox is switched off, and so is permission to run anything, so nothing runs.".into()
            },
            exceptions: if on {
                crate::code_workspace::allowlist(db)
                    .into_iter()
                    .map(|p| format!("`{p}` is on the list of programs you keep."))
                    .collect()
            } else {
                Vec::new()
            },
        };
    }

    Row {
        key: "run",
        title: "Run commands",
        action: if on { Action::Allow } else { Action::Deny },
        decided_by: switch_source(db, Grant::Run.setting(), level, level_applied),
        rule: if on {
            format!(
                "Runs build and test commands, including a shell, inside the sandbox: {}.",
                sandbox.description()
            )
        } else {
            "Runs nothing. A command it wants is written into your terminal for you to run.".into()
        },
        // The allowlist is deliberately absent here. Confined, it is not consulted -- the
        // sandbox is the boundary, not a list of program names -- and printing it would
        // describe a check that does not happen.
        exceptions: if on {
            vec!["Inside the sandbox the list of program names is not consulted: the box is the boundary, and a shell is allowed in it.".into()]
        } else {
            Vec::new()
        },
    }
}

/// Whether a coding agent inside the sandbox could reach AETHER1's own model server.
///
/// Not a switch, which is why this row has no `Switch` or `Level` source: the socket is
/// opened for a caller that asks for it, and the only thing an operator can change is where
/// AETHER1 itself is pointed. So the row reports the two facts that decide it -- the machine
/// can confine a command at all, and the model is one with a local address -- and
/// `code_model` is what answers the second, rather than this file working it out again.
fn brain_row(db: &MemoryDb, sandbox: &crate::code_sandbox::Sandbox) -> Row {
    const KEY: &str = "brain";
    const TITLE: &str = "A coding agent's own model";

    if !sandbox.confines() {
        return Row {
            key: KEY,
            title: TITLE,
            action: Action::Deny,
            decided_by: Source::Machine,
            rule: "This machine cannot confine a command, so there is no sandbox to open the \
                   model server to -- and commands are refused here anyway."
                .into(),
            exceptions: Vec::new(),
        };
    }

    match crate::code_model::configured(db) {
        Ok(endpoint) => Row {
            key: KEY,
            title: TITLE,
            action: Action::Allow,
            decided_by: Source::Model,
            rule: format!(
                "An agent in the sandbox is given one socket to {}:{} -- AETHER1's own model \
                 server, and nothing else on this machine. It works with the network off.",
                endpoint.host, endpoint.port
            ),
            exceptions: vec![
                format!(
                    "Inside the sandbox it is addressed as {}",
                    endpoint.in_box()
                ),
                "Every other address goes through the network policy instead".to_string(),
            ],
        },
        Err(why) => Row {
            key: KEY,
            title: TITLE,
            action: Action::Deny,
            decided_by: Source::Model,
            rule: why,
            exceptions: Vec::new(),
        },
    }
}

fn network_row(
    db: &MemoryDb,
    sandbox: &crate::code_sandbox::Sandbox,
    root: Option<&std::path::Path>,
    level: Option<Level>,
    level_applied: bool,
) -> Row {
    let switch = crate::code_sandbox::network_allowed(db);
    let source = switch_source(
        db,
        crate::code_sandbox::NETWORK_SETTING,
        level,
        level_applied,
    );

    if !switch {
        return Row {
            key: "network",
            title: "Network from a command",
            action: Action::Deny,
            decided_by: source,
            rule: "A command in the sandbox has no route anywhere: a loopback interface and nothing else.".into(),
            exceptions: Vec::new(),
        };
    }

    match crate::code_sandbox::network_for(sandbox, db) {
        crate::code_sandbox::Net::None => Row {
            key: "network",
            title: "Network from a command",
            action: Action::Deny,
            decided_by: source,
            rule: "A command in the sandbox has no route anywhere.".into(),
            exceptions: Vec::new(),
        },
        // Honest about the hole rather than claiming the isolation. This is the one row
        // where the switch says yes, the machine says yes, and the answer is still worse
        // than it looks.
        crate::code_sandbox::Net::Unenforced => Row {
            key: "network",
            title: "Network from a command",
            action: Action::Allow,
            decided_by: Source::Machine,
            rule: "This machine will not let the sandbox cut the network, so a command has the host's network whatever the domain list says. The proxy variables are set and a well-behaved tool honours them; nothing enforces it.".into(),
            exceptions: Vec::new(),
        },
        crate::code_sandbox::Net::Proxied { .. } => {
            let Some(root) = root else {
                return Row {
                    key: "network",
                    title: "Network from a command",
                    action: Action::Ask,
                    decided_by: Source::NoWorkspace,
                    rule: "Every connection goes through the proxy, which asks you per domain. No project folder is nominated, so there is no list to remember an answer in.".into(),
                    exceptions: Vec::new(),
                };
            };
            let allowed: Vec<String> = crate::code_proxy::allowed(root).into_iter().collect();
            Row {
                key: "network",
                title: "Network from a command",
                action: Action::Ask,
                decided_by: Source::Project,
                rule: format!(
                    "Every connection is decided by host at the proxy, on the CONNECT line, with no certificate and nothing decrypted. {} domain{} answered for; anything else asks you.",
                    allowed.len(),
                    if allowed.len() == 1 { " is" } else { "s are" }
                ),
                exceptions: allowed
                    .into_iter()
                    .map(|d| format!("{d} is allowed without asking."))
                    .collect(),
            }
        }
    }
}

fn folders_row(root: Option<&std::path::Path>, level: Option<Level>) -> Row {
    let Some(root) = root else {
        return Row {
            key: "folders",
            title: "Folders outside the project",
            action: Action::Deny,
            decided_by: Source::NoWorkspace,
            rule: "No project folder is nominated, so there is no policy file to name another folder in.".into(),
            exceptions: Vec::new(),
        };
    };

    let mounts = crate::code_policy::mounts(root);
    if mounts.is_empty() {
        return Row {
            key: "folders",
            title: "Folders outside the project",
            action: Action::Deny,
            decided_by: Source::Project,
            rule: "No folder outside the project is named, so the sandbox sees none of them. Your home is a fresh tmpfs inside the box and your credential files are masked.".into(),
            exceptions: Vec::new(),
        };
    }

    // A `write: true` that the level does not honour is the interesting case: the line is in
    // the file, the operator wrote it, and it is not in force. Saying so is better than
    // either obeying it or dropping it silently.
    let asked_for_write_but_read_only =
        level.map(|l| !l.honours_writable_mounts()).unwrap_or(false);

    Row {
        key: "folders",
        title: "Folders outside the project",
        action: Action::Allow,
        decided_by: Source::Project,
        rule: format!(
            "{} folder{} outside the project {} mounted into the sandbox.{}",
            mounts.len(),
            if mounts.len() == 1 { "" } else { "s" },
            if mounts.len() == 1 { "is" } else { "are" },
            if asked_for_write_but_read_only {
                " At this level they are read-only, whatever the file asks for; only Agent honours a writable folder."
            } else {
                ""
            }
        ),
        exceptions: mounts
            .into_iter()
            .map(|m| {
                format!(
                    "{} is mounted {}.",
                    m.path.display(),
                    if m.write {
                        "read and write"
                    } else {
                        "read-only"
                    }
                )
            })
            .collect(),
    }
}

// ------------------------------------------------------------------ the wire shape

/// The report as JSON, for the HTTP route, the Tauri command and the HUD.
///
/// Flat rows with stable keys rather than a nested object per capability: the HUD draws one
/// table, and a shape that mirrors the table is a shape nobody has to transform.
pub fn to_json(report: &Report) -> serde_json::Value {
    serde_json::json!({
        "root": report.root.as_ref().map(|r| r.to_string_lossy().into_owned()),
        "level": report.level.map(|l| l.key()),
        "level_title": report.level.map(|l| l.title()),
        "level_applied": report.level_applied,
        "anything_blocked": report.anything_blocked(),
        "rows": report.rows.iter().map(|row| serde_json::json!({
            "key": row.key,
            "title": row.title,
            "action": row.action.key(),
            "action_title": row.action.title(),
            "decided_by": row.decided_by.key(),
            "decided_by_title": row.decided_by.title(),
            "rule": row.rule,
            "exceptions": row.exceptions,
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_perms::Grant;

    fn db() -> MemoryDb {
        let path = std::env::temp_dir().join(format!(
            "aether1_code_effective_{}_{:?}.db",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&path);
        MemoryDb::open(&path).expect("open test db")
    }

    fn project(db: &MemoryDb) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "aether1_effective_proj_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        db.set_setting(
            crate::code_workspace::ROOT_SETTING,
            &serde_json::json!(root.to_string_lossy()),
        )
        .unwrap();
        root
    }

    fn row<'a>(report: &'a Report, key: &str) -> &'a Row {
        report
            .rows
            .iter()
            .find(|r| r.key == key)
            .unwrap_or_else(|| panic!("no {key} row"))
    }

    /// The defect this module exists for.
    ///
    /// `aether1 code perms` prints `internet  ON` while local-only mode refuses every
    /// fetch, because it reads the switch and local-only mode is a different switch. The
    /// report has to answer about the world, not about the row it happens to be reading.
    #[test]
    fn local_only_outranks_the_internet_permission() {
        let db = db();
        crate::code_perms::set(&db, Grant::Internet, true).unwrap();
        db.set_setting("local_only", &serde_json::json!(true))
            .unwrap();

        let report = report(&db);
        let web = row(&report, "web");
        assert_eq!(
            web.action,
            Action::Deny,
            "the grant is on and nothing gets out"
        );
        assert_eq!(
            web.decided_by,
            Source::LocalOnly,
            "and the operator needs to be sent to local-only mode, not to the switch they \
             already turned on"
        );
    }

    /// And the report must not drift from the decider. `check_url` is what actually refuses
    /// a fetch, so the row's verdict is asserted against it rather than against a repeat of
    /// its reasoning -- which is the one way a report like this goes quietly wrong.
    #[test]
    fn the_internet_row_agrees_with_the_check_that_actually_refuses() {
        let db = db();
        for grant_on in [true, false] {
            for local_only in [true, false] {
                crate::code_perms::set(&db, Grant::Internet, grant_on).unwrap();
                db.set_setting("local_only", &serde_json::json!(local_only))
                    .unwrap();

                let allowed_in_report = row(&report(&db), "web").action == Action::Allow;
                let allowed_in_practice =
                    crate::code_perms::check_url(&db, "https://example.com/doc").is_ok();
                assert_eq!(
                    allowed_in_report, allowed_in_practice,
                    "grant_on={grant_on} local_only={local_only}"
                );
            }
        }
    }

    /// The same shape one level down: the environment outranks the setting, and nothing in
    /// the app can turn it off, so the row has to say so rather than offering a switch an
    /// operator would then find does nothing.
    #[test]
    fn the_environment_outranks_the_setting_and_is_named_as_itself() {
        let db = db();
        crate::code_perms::set(&db, Grant::Internet, true).unwrap();
        // Switched off in the database, forced on by the environment: the two disagree, and
        // the environment is the one that wins.
        db.set_setting("local_only", &serde_json::json!(false))
            .unwrap();

        let forced = web_row(&db, true);
        assert_eq!(forced.action, Action::Deny);
        assert_eq!(forced.decided_by, Source::Environment);
        assert!(
            forced.rule.contains("cannot turn this off"),
            "an operator sent to a switch that does nothing is worse off: {}",
            forced.rule
        );

        // And without it, the database decides again.
        assert_eq!(web_row(&db, false).action, Action::Allow);
    }

    /// The other half of the defect. A machine with no usable sandbox refuses every command
    /// whatever the switch says, and telling the operator to go and turn on a switch that
    /// is already on is the worst answer available.
    #[test]
    fn a_machine_that_cannot_confine_outranks_the_run_switch() {
        let db = db();
        crate::code_perms::set(&db, Grant::Run, true).unwrap();
        let sandbox = crate::code_sandbox::Sandbox::Unavailable("bubblewrap is not installed");

        let run = run_row(&db, &sandbox, None, true);
        assert_eq!(run.action, Action::Deny);
        assert_eq!(run.decided_by, Source::Machine);
        assert!(
            run.rule.contains("bubblewrap"),
            "the refusal has to name what is missing: {}",
            run.rule
        );
    }

    /// The Windows case, and the Linux-without-a-network-namespace case, which are the same
    /// case: the machine cannot cut a command off from the network, so local-only mode
    /// cannot be kept for one and `run` refuses.
    ///
    /// Asserted with unconfined running *on*, because that is the combination that used to
    /// contradict itself: the operator had agreed to run without a sandbox -- a statement
    /// about this machine's files -- and the row read "runs commands as you" while the mode
    /// whose job is to say nothing leaves was switched on beside it.
    #[test]
    fn local_only_outranks_unconfined_on_a_machine_that_cannot_cut_the_network() {
        let db = db();
        crate::code_perms::set(&db, Grant::Run, true).unwrap();
        db.set_setting(
            crate::code_sandbox::UNCONFINED_SETTING,
            &serde_json::json!(true),
        )
        .unwrap();
        db.set_setting(crate::local_only::SETTING, &serde_json::json!(true))
            .unwrap();
        let sandbox =
            crate::code_sandbox::Sandbox::Unavailable("there is no sandbox for this platform yet");

        let run = run_row(&db, &sandbox, None, true);
        assert_eq!(run.action, Action::Deny);
        assert_eq!(
            run.decided_by,
            Source::LocalOnly,
            "the row has to name the thing actually deciding: {}",
            run.rule
        );
        assert!(
            run.rule.contains("local-only"),
            "and say so in words: {}",
            run.rule
        );
    }

    /// The same machine with the mode off still runs, so the refusal above is the mode and
    /// not a machine this code has quietly given up on.
    #[test]
    fn the_same_machine_still_runs_with_local_only_off() {
        let db = db();
        crate::code_perms::set(&db, Grant::Run, true).unwrap();
        db.set_setting(
            crate::code_sandbox::UNCONFINED_SETTING,
            &serde_json::json!(true),
        )
        .unwrap();
        let sandbox =
            crate::code_sandbox::Sandbox::Unavailable("there is no sandbox for this platform yet");

        assert_eq!(run_row(&db, &sandbox, None, true).action, Action::Allow);
    }

    /// Unconfined is the escape hatch, and the row says the thing an operator would want to
    /// be told once rather than discover: there is no sandbox.
    #[test]
    fn unconfined_says_there_is_no_sandbox() {
        let db = db();
        crate::code_perms::set(&db, Grant::Run, true).unwrap();
        db.set_setting(
            crate::code_sandbox::UNCONFINED_SETTING,
            &serde_json::json!(true),
        )
        .unwrap();
        let sandbox = crate::code_sandbox::Sandbox::Unavailable("bubblewrap is not installed");

        let run = run_row(&db, &sandbox, None, true);
        assert_eq!(run.action, Action::Allow);
        assert!(
            run.rule.contains("no sandbox"),
            "unconfined has to say so in those words: {}",
            run.rule
        );
    }

    /// A level and a hand-set switch write the same row in the same table, so the only
    /// honest reading is whether the level's whole set is still in force. Both answers are
    /// useful and they are different: one sends the operator to the level, the other tells
    /// them their level no longer describes their project.
    #[test]
    fn a_switch_moved_by_hand_is_not_reported_as_the_level() {
        let db = db();
        let root = project(&db);
        crate::code_policy::set_level(&root, &db, Level::Developer).unwrap();

        let report_at_level = report(&db);
        assert!(report_at_level.level_applied);
        assert_eq!(row(&report_at_level, "edit").decided_by, Source::Level);

        // One switch, by hand, which is exactly what `code_policy::effective` is there to
        // notice.
        crate::code_perms::set(&db, Grant::Run, false).unwrap();

        let moved = report(&db);
        assert!(
            !moved.level_applied,
            "the level no longer describes the switches"
        );
        assert_eq!(
            row(&moved, "edit").decided_by,
            Source::Switch,
            "with the set broken, every switch in it is a hand-set switch"
        );
        assert_eq!(row(&moved, "run").action, Action::Deny);
    }

    /// Without a folder there is nothing for half the rows to apply to, and the misleading
    /// answer is the plausible one: `edit` would otherwise read the switch, find it on, and
    /// report that files can be changed.
    #[test]
    fn with_no_project_folder_the_rows_that_need_one_say_so() {
        let db = db();
        crate::code_perms::set(&db, Grant::Edit, true).unwrap();

        let report = report(&db);
        assert!(report.root.is_none());
        assert!(report.level.is_none());
        for key in ["edit", "folders"] {
            let row = row(&report, key);
            assert_eq!(row.action, Action::Deny, "{key}");
            assert_eq!(row.decided_by, Source::NoWorkspace, "{key}");
        }
        assert!(
            row(&report, "edit").rule.contains("code workspace"),
            "a refusal says what to do next"
        );
    }

    /// Every refusal tells the operator something. A blank one is the failure mode of a
    /// report built out of a match on an enum, and it is invisible until somebody reads the
    /// page on the one machine that hits that arm.
    #[test]
    fn every_row_says_something_and_every_key_is_its_own() {
        let db = db();
        let report = report(&db);
        let mut seen = std::collections::HashSet::new();
        for row in &report.rows {
            assert!(!row.key.is_empty());
            assert!(seen.insert(row.key), "two rows share the key {}", row.key);
            assert!(!row.title.is_empty(), "{} has no title", row.key);
            assert!(
                row.rule.len() > 20,
                "{} says nothing useful: {:?}",
                row.key,
                row.rule
            );
        }
        assert_eq!(seen.len(), 8, "a row was added or lost without a test");
    }

    /// The HUD and the HTTP route read this shape, and a row drawn without its layer is a
    /// row that has lost the only thing this card adds.
    #[test]
    fn the_json_carries_the_layer_for_every_row() {
        let db = db();
        let json = to_json(&report(&db));
        let rows = json["rows"].as_array().expect("rows");
        assert_eq!(rows.len(), 8);
        for row in rows {
            for field in [
                "key",
                "title",
                "action",
                "action_title",
                "decided_by",
                "decided_by_title",
                "rule",
            ] {
                assert!(
                    row[field].as_str().is_some_and(|s| !s.is_empty()),
                    "{field} missing from {row:?}"
                );
            }
            assert!(row["exceptions"].is_array());
        }
    }
    /// The brain row reports and does not decide: whatever `code_model` says about the
    /// settings is what the row says, and the row never starts a socket to find out. Both
    /// directions, so a row that always said one thing would fail.
    #[test]
    fn the_brain_row_is_whatever_code_model_says_about_the_settings() {
        let db = db();
        let sandbox = crate::code_sandbox::Sandbox::Bubblewrap {
            bwrap: std::path::PathBuf::from("/usr/bin/bwrap"),
            can_unshare_net: true,
        };

        for (provider, endpoint) in [
            ("ollama", "http://localhost:11434"),
            ("lmstudio", "http://127.0.0.1:1234/v1"),
            ("anthropic", "http://localhost:11434"),
            ("ollama", "http://models.example.com:11434"),
            ("offline", "http://localhost:11434"),
        ] {
            db.set_setting("llm_provider", &serde_json::json!(provider))
                .expect("provider");
            db.set_setting("llm_endpoint", &serde_json::json!(endpoint))
                .expect("endpoint");

            let row = brain_row(&db, &sandbox);
            let allowed = crate::code_model::configured(&db).is_ok();
            assert_eq!(
                row.action == Action::Allow,
                allowed,
                "{provider} at {endpoint}: the row must agree with code_model, not repeat \
                 its reasoning"
            );
            // Never one of AETHER CODE's five switches, because it is not one of them.
            assert_eq!(row.decided_by, Source::Model, "{provider} at {endpoint}");
        }
    }

    /// A machine with no sandbox has nothing to open the model to, and that is the machine's
    /// answer rather than the model's -- the same place `run` is already refused.
    #[test]
    fn with_no_sandbox_the_brain_row_blames_the_machine() {
        let db = db();
        db.set_setting("llm_provider", &serde_json::json!("ollama"))
            .expect("provider");
        let row = brain_row(
            &db,
            &crate::code_sandbox::Sandbox::Unavailable("no bubblewrap"),
        );
        assert_eq!(row.action, Action::Deny);
        assert_eq!(row.decided_by, Source::Machine);
    }
}
