//! What a project has been trusted with, and the four answers an operator can give.
//!
//! The sandbox and the proxy settled *how* a boundary is enforced. This settles *what* the
//! boundary is for one project, and it is the difference between a tool and an agent: an
//! agent that has to be asked about every command is a chatbot with a confirmation dialog,
//! and one that is handed the whole machine is a thing nobody should run. The answer is to
//! make the room large and its walls real, then let the operator say once how large.
//!
//! **Four levels**, because a slider between "safe" and "useful" is a question nobody can
//! answer, while four described configurations is a choice:
//!
//! | | The project folder | Other folders | Network | Commands |
//! |---|---|---|---|---|
//! | Assistant | read | read, where named | through the proxy | in the sandbox |
//! | Developer | read and write | read, where named | through the proxy | in the sandbox |
//! | Agent | read and write | as named, read or write | through the proxy | in the sandbox |
//! | Unrestricted | read and write | everything you can reach | anything | as you |
//!
//! Developer is the one to live in: it is what a coding agent needs and nothing more, and it
//! can work for an hour without asking anything. Assistant is for reading -- "summarise my
//! notes" -- and cannot change the project. Agent adds the folders the operator names, with
//! read and write decided per folder, which is what makes "organise my downloads" a thing
//! that can be asked for without handing over everything else. Unrestricted is the escape
//! hatch, it turns the sandbox off, and it says so in those words.
//!
//! **A level is not a new mechanism.** It is a name for a set of switches that already
//! exist -- `code_perm_edit`, `code_perm_run`, `code_run_network`, `code_run_unconfined` --
//! plus the folder list here. `Level::apply` writes those switches and nothing else.
//!
//! The project's file is the level's one home, and the switches are what it puts in place.
//! It has to be that way round: Developer and Agent set the same four switches and differ
//! only in what they do with the folders named here, so the settings table cannot say which
//! of the two an operator chose. What the switches *can* answer is whether they still match
//! the level that was recorded -- an operator who moves one by hand is told their project is
//! no longer at the level its file claims, rather than being silently rounded to whichever
//! name happens to fit.
//!
//! **The file is the project's.** `.aether/policy.json` sits in the workspace, next to the
//! code it describes, so trusting a project is a fact about that project and travels with
//! it. It is the same file the proxy reads its domains from -- one file per project, not one
//! per subsystem.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::llm::MemoryDb;

/// The per-project directory, relative to the workspace root -- the one a project that has
/// neither gets. The Aurora1 rename moves this to `.aurora`, and when it does, every project
/// folder on the machine still has the old one.
pub const DIR: &str = ".aether";

/// The name the same directory has under the other branding. Projects keep whichever they
/// already have: a dotdir in somebody's workspace is their file, often committed and often
/// in their `.gitignore`, so a rename of ours is not a reason to leave a second one beside
/// it or to stop reading the first.
pub const ALT_DIR: &str = ".aurora";

/// The per-project file, relative to the workspace root, as it is *named* in prose and in
/// the settings pane. `file_in` is what reads and writes go through -- this is the default
/// spelling for a project that has no policy file yet.
pub const FILE: &str = ".aether/policy.json";

/// Which of the two directory names this project is using: whichever it already has, and
/// `DIR` when it has neither.
///
/// Preferring the one that exists is the whole of the compatibility story. It means the
/// release that renames `DIR` finds every existing project's `.aether/policy.json` and keeps
/// writing to it, and a project set up after the rename is read correctly by a copy of
/// AETHER1 built before it -- which matters because an operator runs more than one copy
/// while an update is going through.
pub fn dir_in(root: &Path) -> &'static str {
    for name in [DIR, ALT_DIR] {
        if root.join(name).is_dir() {
            return name;
        }
    }
    DIR
}

/// The policy file for `root`, under whichever directory name that project uses.
pub fn file_in(root: &Path) -> PathBuf {
    root.join(dir_in(root)).join("policy.json")
}

/// The level a project is trusted at when its file does not say. Developer, because an
/// operator who nominated a project folder and switched on a coding agent asked for one
/// that can write in it -- and because the two grants it needs are still off by default at
/// the settings level, so nothing here turns anything on behind their back.
pub const DEFAULT_LEVEL: Level = Level::Developer;

/// One folder the sandbox can see beyond the workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub path: PathBuf,
    /// Whether it is writable. Read and write are separate answers on purpose: "read my
    /// vault and summarise it" and "reorganise my vault" are different requests, and a
    /// permission model that cannot tell them apart makes the safe one impossible.
    pub write: bool,
}

/// How much of the machine this project is trusted with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Reads, and cannot change the project.
    Assistant,
    /// Reads and writes the project. The default, and the one to work in.
    Developer,
    /// The project, plus the folders the operator named, each read or write.
    Agent,
    /// No sandbox. Commands run as the operator, with everything their account can reach.
    Unrestricted,
}

/// Every level, in the order they are shown -- least to most.
pub const ALL: &[Level] = &[
    Level::Assistant,
    Level::Developer,
    Level::Agent,
    Level::Unrestricted,
];

impl Level {
    /// The word the operator types, and what is written in the file.
    pub fn key(self) -> &'static str {
        match self {
            Level::Assistant => "assistant",
            Level::Developer => "developer",
            Level::Agent => "agent",
            Level::Unrestricted => "unrestricted",
        }
    }

    pub fn from_key(key: &str) -> Option<Level> {
        ALL.iter()
            .copied()
            .find(|level| level.key() == key.trim().to_ascii_lowercase())
    }

    /// The name shown to the operator. One source, so the CLI and the Settings page cannot
    /// disagree about what a level is called.
    pub fn title(self) -> &'static str {
        match self {
            Level::Assistant => "Assistant",
            Level::Developer => "Developer",
            Level::Agent => "Agent",
            Level::Unrestricted => "Unrestricted",
        }
    }

    /// One line, addressed to the operator.
    pub fn description(self) -> &'static str {
        match self {
            Level::Assistant => {
                "Read the project and run commands in the sandbox, but change nothing"
            }
            Level::Developer => "Read and write the project, and run commands in the sandbox",
            Level::Agent => {
                "The project, plus the folders you name -- each of them read-only or writable"
            }
            Level::Unrestricted => {
                "No sandbox: commands run as you, with everything your account can reach"
            }
        }
    }

    /// Whether the project folder itself may be written at this level.
    pub fn writes_the_project(self) -> bool {
        !matches!(self, Level::Assistant)
    }

    /// Whether the folders named in the policy file are mounted at this level.
    ///
    /// Assistant and Developer get read-only access to a named folder; only Agent honours a
    /// `write: true` on one. So "read my Documents" works in every level that can read, and
    /// "reorganise my Downloads" is a level the operator chose deliberately.
    pub fn honours_writable_mounts(self) -> bool {
        matches!(self, Level::Agent | Level::Unrestricted)
    }

    /// The switches this level means. `Level::current` reads exactly these back.
    fn switches(self) -> [(&'static str, bool); 4] {
        [
            (
                crate::code_perms::Grant::Edit.setting(),
                self.writes_the_project(),
            ),
            (crate::code_perms::Grant::Run.setting(), true),
            (crate::code_sandbox::NETWORK_SETTING, true),
            (
                crate::code_sandbox::UNCONFINED_SETTING,
                matches!(self, Level::Unrestricted),
            ),
        ]
    }

    /// Writes this level's switches. The level itself is recorded in the project's file by
    /// `set_level`, which calls this -- a level in a file that the switches contradict would
    /// be a lie in exactly the place an operator goes to check.
    pub fn apply(self, db: &MemoryDb) -> Result<(), String> {
        for (key, on) in self.switches() {
            db.set_setting(key, &serde_json::json!(on))
                .map_err(|e: rusqlite::Error| e.to_string())?;
        }
        Ok(())
    }

    /// Whether the switches this level means are the ones in force.
    pub fn applied(self, db: &MemoryDb) -> bool {
        self.switches()
            .iter()
            .all(|(key, on)| db.get_setting_bool(key, false) == *on)
    }
}

// ------------------------------------------------------------------ the file

/// Where what the operator has approved is kept: the settings database, which is under
/// the home directory the sandbox hides, and so is somewhere a command run in the project
/// cannot write.
static APPROVALS: std::sync::OnceLock<MemoryDb> = std::sync::OnceLock::new();

/// The setting holding the approved policies: a map from a project's canonical path to the
/// document that was approved for it and a hash of the file as it stood then.
pub const APPROVED_SETTING: &str = "code_policy_approved";

/// Set once the projects that already had a policy file have been taken as approved.
const ADOPTED_SETTING: &str = "code_policy_adopted_existing";

/// Hands this module the settings database. Called once, where `net::install` is.
///
/// Without it there is nowhere to keep an approval, and the file is read as it stands --
/// which is how a unit test, with a database of its own per test and no process-wide one,
/// still sees what it wrote. Every real entry point comes through `build_llm_engine`.
///
/// The first call on an existing install takes the current project's file as approved, once:
/// it was being obeyed in full the day before, and an update that silently dropped every
/// operator back to the defaults would be read as the update breaking their setup.
pub fn install(db: &MemoryDb) {
    if APPROVALS.set(db.clone()).is_err() {
        return;
    }
    if db.get_setting_bool(ADOPTED_SETTING, false) {
        return;
    }
    let root = db.get_setting_string("code_workspace_root", "");
    if !root.trim().is_empty() {
        let root = crate::paths::expand_home(root.trim());
        if let Some((text, document)) = file_document(&root) {
            let _ = record_approval(db, &root, &text, &document);
        }
    }
    let _ = db.set_setting(ADOPTED_SETTING, &serde_json::json!(true));
}

/// The project's path as approvals are filed under it.
fn approval_key(root: &Path) -> String {
    root.canonicalize()
        .unwrap_or_else(|_| root.to_path_buf())
        .to_string_lossy()
        .to_string()
}

fn sha256_hex(text: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The file as it is on disk: its text and the object in it. None when there is no file or
/// it is not a JSON object.
fn file_document(root: &Path) -> Option<(String, Value)> {
    let text = std::fs::read_to_string(file_in(root)).ok()?;
    let document: Value = serde_json::from_str(&text).ok()?;
    document.is_object().then_some((text, document))
}

fn approvals(db: &MemoryDb) -> serde_json::Map<String, Value> {
    db.get_setting(APPROVED_SETTING)
        .ok()
        .flatten()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

fn record_approval(db: &MemoryDb, root: &Path, text: &str, document: &Value) -> Result<(), String> {
    let mut all = approvals(db);
    all.insert(
        approval_key(root),
        serde_json::json!({ "sha256": sha256_hex(text), "document": document }),
    );
    db.set_setting(APPROVED_SETTING, &Value::Object(all))
        .map_err(|e| format!("the approved policy could not be recorded: {e}"))
}

/// The policy in force for this project.
///
/// **The file is a proposal; what the operator approved is the policy.** The file sits in
/// the workspace, which is the one place a sandboxed command can write and the one place a
/// cloned repository puts things. Obeying it as written meant a build script could add
/// `"level": "unrestricted"` or a writable mount for the next command to enjoy, and a
/// repository could arrive with its own allow list. So the document every reader gets is
/// the one recorded when the operator last answered for this project -- by choosing a
/// level, naming a folder, allowing a domain, or accepting a change to the file -- and a
/// file that has drifted from it is reported by `pending_change` rather than obeyed.
///
/// A project nobody has answered for has the empty policy, which is the baseline: the
/// default level, the starter domains, no extra folders.
pub(crate) fn document(root: &Path) -> Value {
    document_in(APPROVALS.get(), root)
}

fn document_in(db: Option<&MemoryDb>, root: &Path) -> Value {
    let Some(db) = db else {
        return file_document(root)
            .map(|(_, document)| document)
            .unwrap_or_else(|| serde_json::json!({}));
    };
    approvals(db)
        .get(&approval_key(root))
        .and_then(|record| record.get("document"))
        .filter(|document| document.is_object())
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}))
}

/// Writes the policy to the project's file and records it as what the operator approved.
/// Every caller is the operator answering something, which is what makes that true.
pub(crate) fn write_document(root: &Path, policy: &Value) -> Result<(), String> {
    write_document_in(APPROVALS.get(), root, policy)
}

fn write_document_in(db: Option<&MemoryDb>, root: &Path, policy: &Value) -> Result<(), String> {
    let path = file_in(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("{} could not be made: {e}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(policy).unwrap_or_default() + "\n";
    std::fs::write(&path, &text)
        .map_err(|e| format!("{} could not be written: {e}", path.display()))?;
    match db {
        Some(db) => record_approval(db, root, &text, policy),
        None => Ok(()),
    }
}

/// What the project's file says that the operator has not approved, in a sentence, or None
/// when the file and the approved policy agree.
pub fn pending_change(root: &Path) -> Option<String> {
    pending_change_in(APPROVALS.get()?, root)
}

fn pending_change_in(db: &MemoryDb, root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(file_in(root)).ok()?;
    let record = approvals(db).get(&approval_key(root)).cloned();
    let approved_hash = record
        .as_ref()
        .and_then(|record| record.get("sha256"))
        .and_then(Value::as_str);
    if approved_hash == Some(sha256_hex(&text).as_str()) {
        return None;
    }
    let name = format!("{}/policy.json", dir_in(root));
    let Some((_, proposed)) = file_document(root) else {
        return Some(format!(
            "{name} has been changed and is no longer readable. The policy you approved is \
             still the one in force."
        ));
    };
    if proposed.as_object().is_some_and(|object| object.is_empty()) && record.is_none() {
        return None;
    }
    let mut asks = Vec::new();
    if let Some(level) = proposed.get("level").and_then(Value::as_str) {
        asks.push(format!("level {level}"));
    }
    // Named rather than counted: what the operator is being asked to accept is these
    // folders and these domains, and "2 folders" is not something anyone can say yes to.
    let listed = |items: Vec<String>| {
        let shown: Vec<String> = items.iter().take(6).cloned().collect();
        let more = items.len().saturating_sub(shown.len());
        if more > 0 {
            format!("{} and {more} more", shown.join(", "))
        } else {
            shown.join(", ")
        }
    };
    if let Some(list) = proposed.get("mounts").and_then(Value::as_array) {
        let folders: Vec<String> = list
            .iter()
            .filter_map(|entry| {
                let path = entry.get("path").and_then(Value::as_str)?;
                let write = entry.get("write").and_then(Value::as_bool).unwrap_or(false);
                Some(format!(
                    "{path} ({})",
                    if write { "read and write" } else { "read" }
                ))
            })
            .collect();
        if !folders.is_empty() {
            asks.push(format!("the folders {}", listed(folders)));
        }
    }
    for (field, what) in [
        ("allow", "network access to"),
        ("deny", "no network access to"),
        ("similar_commands", "running without asking"),
    ] {
        let items: Vec<String> = proposed
            .get(field)
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        if !items.is_empty() {
            asks.push(format!("{what} {}", listed(items)));
        }
    }
    Some(format!(
        "{name} {} and asks for: {}. None of it is in force until you accept it.",
        if record.is_some() {
            "has changed since you approved it"
        } else {
            "came with this project"
        },
        if asks.is_empty() {
            "nothing".to_string()
        } else {
            asks.join("; ")
        }
    ))
}

/// Takes the file as it stands as the policy. The operator's answer to `pending_change`.
///
/// What is accepted is still read through the same checks as anything else here: a folder
/// that may never be shared, a command that cannot be a rule and a domain that is not one
/// are dropped by `mounts`, `similar_commands` and the proxy whether or not they are in the
/// approved document.
pub fn accept_pending(root: &Path) -> Result<(), String> {
    let db = APPROVALS
        .get()
        .ok_or_else(|| "there is nowhere to record an approval".to_string())?;
    accept_pending_in(db, root)
}

fn accept_pending_in(db: &MemoryDb, root: &Path) -> Result<(), String> {
    let (text, document) = file_document(root)
        .ok_or_else(|| format!("{} is not a readable policy", file_in(root).display()))?;
    record_approval(db, root, &text, &document)
}

/// Puts the file back to the policy the operator approved, discarding what it proposed.
pub fn discard_pending(root: &Path) -> Result<(), String> {
    write_document(root, &document(root))
}

/// Why a folder may never be shared with the sandbox, or None. One rule for a folder being
/// added and for one already written down: a list that is only checked on the way in is not
/// checked for anything that got into the file another way.
fn unshareable(resolved: &Path) -> Option<&'static str> {
    if resolved.parent().is_none() {
        return Some("the filesystem root is not a folder to share");
    }
    if let Some(home) = crate::paths::home_dir() {
        let home = home.canonicalize().unwrap_or(home);
        // The home directory, and anything it is inside of: sharing /home shares it too.
        if home.starts_with(resolved) {
            return Some(
                "the whole home directory is what the sandbox is for. Name a folder inside it.",
            );
        }
    }
    None
}

/// The level this project is trusted at.
pub fn level(root: &Path) -> Level {
    document(root)
        .get("level")
        .and_then(Value::as_str)
        .and_then(Level::from_key)
        .unwrap_or(DEFAULT_LEVEL)
}

/// Whether this project has ever been answered for, which is what decides between showing
/// the operator a question and getting on with the work.
pub fn is_trusted(root: &Path) -> bool {
    document(root).get("level").is_some()
}

/// Records the level in the project's file and applies its switches.
pub fn set_level(root: &Path, db: &MemoryDb, level: Level) -> Result<(), String> {
    let mut policy = document(root);
    policy
        .as_object_mut()
        .ok_or_else(|| format!("{} is not a JSON object", file_in(root).display()))?
        .insert("level".to_string(), Value::String(level.key().to_string()));
    write_document(root, &policy)?;
    level.apply(db)
}

/// The level this project records, and whether the switches still match it.
///
/// `false` is the answer to "I turned the network off by hand" -- the level is still what the
/// file says, and the operator is told that something below it has moved rather than having
/// their choice quietly reinterpreted.
pub fn effective(root: &Path, db: &MemoryDb) -> (Level, bool) {
    let level = level(root);
    (level, level.applied(db))
}

/// A deliberately narrow command class for the "allow similar" choice. The first two
/// words identify an ordinary build/test task such as `cargo test` or `npm test`; shell
/// syntax, quoting, paths, and remote repository operations are never remembered this way.
pub fn similar_command_prefix(command: &str) -> Option<String> {
    let words: Vec<&str> = command.split_whitespace().collect();
    if words.len() < 2
        || words.iter().any(|word| {
            word.is_empty()
                || !word
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._:-/".contains(&byte))
        })
    {
        return None;
    }
    if words[0].contains('/') || words[0].contains('\\') {
        return None;
    }
    if matches!((words[0], words[1]), ("git", "push") | ("gh", "pr")) {
        return None;
    }
    Some(format!("{} {}", words[0], words[1]))
}

/// The project-local command classes the operator explicitly allowed.
pub fn similar_commands(root: &Path) -> Vec<String> {
    document(root)
        .get("similar_commands")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(Value::as_str)
                .filter(|entry| similar_command_prefix(entry).as_deref() == Some(*entry))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Whether an argv request is covered by one of this project's command classes.
pub fn allows_similar_argv(root: &Path, argv: &[String]) -> bool {
    if argv.len() < 2
        || argv[0].contains('/')
        || argv[0].contains('\\')
        || argv.iter().any(|part| {
            part.is_empty()
                || part
                    .bytes()
                    .any(|byte| !(byte.is_ascii_alphanumeric() || b"._:-/".contains(&byte)))
        })
    {
        return false;
    }
    let prefix = format!("{} {}", argv[0], argv[1]);
    similar_commands(root)
        .iter()
        .any(|allowed| allowed == &prefix)
}

/// Remembers only an ordinary two-word command class and scopes it to this project file.
pub fn remember_similar_command(root: &Path, command: &str) -> Result<String, String> {
    let prefix = similar_command_prefix(command).ok_or_else(|| {
        "This command cannot be remembered as a similar-command rule. Choose Allow once, or use a simple build/test command such as `cargo test`.".to_string()
    })?;
    let mut policy = document(root);
    let list = policy
        .as_object_mut()
        .ok_or_else(|| format!("{} is not a JSON object", file_in(root).display()))?
        .entry("similar_commands")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .ok_or_else(|| "the `similar_commands` entry must be a list".to_string())?;
    if !list
        .iter()
        .any(|entry| entry.as_str() == Some(prefix.as_str()))
    {
        list.push(Value::String(prefix.clone()));
        write_document(root, &policy)?;
    }
    Ok(prefix)
}

/// The folders this project names beyond the workspace, as the current level allows them.
///
/// A folder that does not exist is left out rather than reported: a policy file that travels
/// with a repository will name folders that exist on one machine and not another, and a
/// command that cannot start because of it would be the worst of both.
pub fn mounts(root: &Path) -> Vec<Mount> {
    let level = level(root);
    let document = document(root);
    let Some(list) = document.get("mounts").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|entry| {
            let path = entry.get("path").and_then(Value::as_str)?;
            let path = crate::paths::expand_home(path);
            if !path.exists() {
                return None;
            }
            // The same refusals `add_mount` makes, asked again of what is written down.
            let resolved = path.canonicalize().ok()?;
            if !resolved.is_dir() || unshareable(&resolved).is_some() {
                return None;
            }
            let asked_for_write = entry.get("write").and_then(Value::as_bool).unwrap_or(false);
            Some(Mount {
                path,
                write: asked_for_write && level.honours_writable_mounts(),
            })
        })
        .collect()
}

/// Adds a folder to the project's policy.
pub fn add_mount(root: &Path, path: &str, write: bool) -> Result<PathBuf, String> {
    let resolved = crate::paths::expand_home(path);
    let resolved = resolved
        .canonicalize()
        .map_err(|e| format!("{} cannot be read: {e}", resolved.display()))?;
    if !resolved.is_dir() {
        return Err(format!("{} is not a folder", resolved.display()));
    }
    if let Some(reason) = unshareable(&resolved) {
        return Err(reason.to_string());
    }

    let mut policy = document(root);
    let list = policy
        .as_object_mut()
        .ok_or_else(|| format!("{} is not a JSON object", file_in(root).display()))?
        .entry("mounts")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .ok_or_else(|| "the `mounts` entry must be a list".to_string())?;
    let as_text = resolved.to_string_lossy().to_string();
    list.retain(|entry| entry.get("path").and_then(Value::as_str) != Some(as_text.as_str()));
    list.push(serde_json::json!({ "path": as_text, "write": write }));
    write_document(root, &policy)?;
    Ok(resolved)
}

/// Removes a folder from the project's policy. True when there was one to remove.
pub fn remove_mount(root: &Path, path: &str) -> Result<bool, String> {
    let resolved = crate::paths::expand_home(path);
    let resolved = resolved.canonicalize().unwrap_or(resolved);
    let as_text = resolved.to_string_lossy().to_string();
    let mut policy = document(root);
    let Some(list) = policy
        .as_object_mut()
        .and_then(|object| object.get_mut("mounts"))
        .and_then(Value::as_array_mut)
    else {
        return Ok(false);
    };
    let before = list.len();
    list.retain(|entry| entry.get("path").and_then(Value::as_str) != Some(as_text.as_str()));
    let removed = list.len() != before;
    if removed {
        write_document(root, &policy)?;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approvals_fixture(name: &str) -> (MemoryDb, PathBuf) {
        let base = std::env::temp_dir().join(format!(
            "aether1_policy_approval_{name}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("project");
        std::fs::create_dir_all(root.join(DIR)).unwrap();
        let db = MemoryDb::open(base.join("settings.db")).unwrap();
        (db, root)
    }

    #[test]
    fn a_file_nobody_approved_is_a_proposal_and_not_the_policy() {
        // What a cloned repository arrives with, or what a build script leaves behind.
        let (db, root) = approvals_fixture("unapproved");
        std::fs::write(
            file_in(&root),
            r#"{"level": "unrestricted", "allow": ["collector.example"],
                "mounts": [{"path": "/etc", "write": true}]}"#,
        )
        .unwrap();

        let in_force = document_in(Some(&db), &root);
        assert_eq!(
            in_force,
            serde_json::json!({}),
            "the baseline, not the file"
        );
        let said = pending_change_in(&db, &root).expect("and the operator is told");
        assert!(said.contains("came with this project"), "{said}");
        assert!(said.contains("level unrestricted"), "{said}");
        assert!(said.contains("/etc (read and write)"), "{said}");
        assert!(said.contains("collector.example"), "{said}");
    }

    #[test]
    fn what_the_operator_wrote_is_in_force_and_a_later_edit_to_the_file_is_not() {
        let (db, root) = approvals_fixture("drift");
        let approved = serde_json::json!({"level": "developer", "allow": ["crates.io"]});
        write_document_in(Some(&db), &root, &approved).unwrap();
        assert_eq!(document_in(Some(&db), &root), approved);
        assert_eq!(pending_change_in(&db, &root), None);

        // A command inside the sandbox rewrites the file.
        std::fs::write(
            file_in(&root),
            r#"{"level": "unrestricted", "allow": ["crates.io", "collector.example"]}"#,
        )
        .unwrap();
        assert_eq!(
            document_in(Some(&db), &root),
            approved,
            "still what was approved"
        );
        let said = pending_change_in(&db, &root).unwrap();
        assert!(said.contains("has changed since you approved it"), "{said}");

        // Accepting is the operator saying yes to the file as it stands.
        accept_pending_in(&db, &root).unwrap();
        assert_eq!(
            document_in(Some(&db), &root)["level"],
            serde_json::json!("unrestricted")
        );
        assert_eq!(pending_change_in(&db, &root), None);
    }

    #[test]
    fn approvals_are_per_project() {
        let (db, root) = approvals_fixture("per_project");
        write_document_in(Some(&db), &root, &serde_json::json!({"level": "agent"})).unwrap();
        let other = root.parent().unwrap().join("other");
        std::fs::create_dir_all(other.join(DIR)).unwrap();
        std::fs::copy(file_in(&root), file_in(&other)).unwrap();
        assert_eq!(document_in(Some(&db), &other), serde_json::json!({}));
    }

    #[test]
    fn a_folder_that_may_never_be_shared_is_dropped_wherever_it_was_written() {
        let (_db, root) = approvals_fixture("unshareable");
        let home = crate::paths::home_dir().unwrap();
        let shared = root.parent().unwrap().join("shared");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(
            file_in(&root),
            serde_json::json!({"level": "agent", "mounts": [
                {"path": "/", "write": true},
                {"path": home.to_string_lossy(), "write": true},
                {"path": home.parent().unwrap().to_string_lossy(), "write": false},
                {"path": shared.to_string_lossy(), "write": true},
            ]})
            .to_string(),
        )
        .unwrap();
        let named: Vec<PathBuf> = mounts(&root).into_iter().map(|m| m.path).collect();
        assert_eq!(named, vec![shared.canonicalize().unwrap()]);
    }

    fn project(name: &str) -> (MemoryDb, PathBuf) {
        let home = std::env::temp_dir().join(format!(
            "aether1_policy_{name}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&home);
        let root = home.join("project");
        std::fs::create_dir_all(&root).unwrap();
        let db = MemoryDb::open(home.join("aether1_memory.db")).unwrap();
        (db, root)
    }

    /// A project keeps whichever of the two directory names it already has.
    ///
    /// **This is what stops the rename losing a project's answers.** A policy file is in
    /// somebody's workspace, usually committed and usually in their `.gitignore`; renaming
    /// the directory in our source does not move theirs, it only stops finding it -- and a
    /// project that cannot be found reads as one nobody has been asked about, so the trust
    /// question comes round again and the domains they allowed are gone. Looking for what
    /// is there first means the release that flips `DIR` keeps writing to the file every
    /// existing project already has.
    #[test]
    fn a_project_keeps_the_policy_directory_it_already_has() {
        let (_db, root) = project("dirs");
        // Neither there: the name this build writes.
        assert_eq!(dir_in(&root), DIR);
        assert_eq!(file_in(&root), root.join(DIR).join("policy.json"));

        // Only the other one there: that is the project's directory, whichever way round
        // the two constants happen to be pointing in this build.
        std::fs::create_dir_all(root.join(ALT_DIR)).unwrap();
        assert_eq!(dir_in(&root), ALT_DIR);
        assert_eq!(file_in(&root), root.join(ALT_DIR).join("policy.json"));

        // Both there -- a project somebody has had open under both brandings. The one this
        // build prefers wins, so nothing flips back and forth between two files.
        std::fs::create_dir_all(root.join(DIR)).unwrap();
        assert_eq!(dir_in(&root), DIR);
    }

    /// And a policy written while only the other directory exists is read back out of it,
    /// rather than a second file appearing beside it.
    #[test]
    fn a_policy_is_written_and_read_through_the_directory_in_use() {
        let (db, root) = project("dirs_roundtrip");
        std::fs::create_dir_all(root.join(ALT_DIR)).unwrap();
        set_level(&root, &db, Level::Assistant).unwrap();
        assert!(root.join(ALT_DIR).join("policy.json").is_file());
        assert!(
            !root.join(DIR).exists(),
            "nothing should write a second policy directory beside the one in use"
        );
        assert_eq!(level(&root), Level::Assistant);
    }

    /// A level is a name for switches, so setting one and reading it back has to agree --
    /// including through the settings, which is where everything else in the program looks.
    #[test]
    fn a_level_is_the_switches_it_names() {
        let (db, root) = project("levels");
        assert!(
            !is_trusted(&root),
            "a fresh project has not been answered for"
        );
        assert_eq!(level(&root), Level::Developer, "and reads as the default");

        for wanted in ALL {
            set_level(&root, &db, *wanted).unwrap();
            assert_eq!(level(&root), *wanted, "the file records it");
            assert!(
                wanted.applied(&db),
                "and the switches it means are in force"
            );
            assert_eq!(effective(&root, &db), (*wanted, true));
            assert!(is_trusted(&root));
        }

        // Assistant is the one that cannot change the project; Unrestricted is the one that
        // turns the box off. Those are the two properties everything else rests on.
        set_level(&root, &db, Level::Assistant).unwrap();
        assert!(!db.get_setting_bool(crate::code_perms::Grant::Edit.setting(), false));
        assert!(!db.get_setting_bool(crate::code_sandbox::UNCONFINED_SETTING, false));
        set_level(&root, &db, Level::Unrestricted).unwrap();
        assert!(db.get_setting_bool(crate::code_sandbox::UNCONFINED_SETTING, false));
    }

    /// A switch moved by hand does not change the level; it is reported as no longer
    /// matching it, which is the honest version of the same fact.
    #[test]
    fn a_switch_moved_by_hand_is_reported_rather_than_reinterpreted() {
        let (db, root) = project("custom");
        set_level(&root, &db, Level::Developer).unwrap();
        db.set_setting(
            crate::code_sandbox::NETWORK_SETTING,
            &serde_json::json!(false),
        )
        .unwrap();
        assert_eq!(effective(&root, &db), (Level::Developer, false));
    }

    /// Read and write are separate answers, and a writable folder is a level the operator
    /// chose rather than a flag in a file.
    #[test]
    fn a_named_folder_is_read_only_until_the_level_says_otherwise() {
        let (db, root) = project("mounts");
        let shared = root.parent().unwrap().join("Documents");
        std::fs::create_dir_all(&shared).unwrap();

        set_level(&root, &db, Level::Developer).unwrap();
        add_mount(&root, &shared.to_string_lossy(), true).unwrap();
        let at_developer = mounts(&root);
        assert_eq!(at_developer.len(), 1);
        assert!(
            !at_developer[0].write,
            "Developer reads a named folder and does not write it"
        );

        set_level(&root, &db, Level::Agent).unwrap();
        assert!(
            mounts(&root)[0].write,
            "Agent is the level that honours a writable folder"
        );

        assert!(remove_mount(&root, &shared.to_string_lossy()).unwrap());
        assert!(mounts(&root).is_empty());
        assert!(!remove_mount(&root, &shared.to_string_lossy()).unwrap());
    }

    /// The two folders that are never shared, because sharing either one is the same as not
    /// having a sandbox.
    #[test]
    fn the_home_directory_and_the_root_are_refused() {
        let (_db, root) = project("refused");
        if let Some(home) = crate::paths::home_dir() {
            let err = add_mount(&root, &home.to_string_lossy(), false).unwrap_err();
            assert!(err.contains("whole home directory"), "{err}");
        }
        let err = add_mount(&root, "/", false).unwrap_err();
        assert!(err.contains("root"), "{err}");
    }

    /// A folder named on another machine is skipped rather than breaking the command, since
    /// the file travels with the repository.
    #[test]
    fn a_folder_that_is_not_here_is_left_out() {
        let (db, root) = project("elsewhere");
        set_level(&root, &db, Level::Agent).unwrap();
        let mut policy = document(&root);
        policy.as_object_mut().unwrap().insert(
            "mounts".to_string(),
            serde_json::json!([{ "path": "/nowhere/at/all", "write": true }]),
        );
        write_document(&root, &policy).unwrap();
        assert!(mounts(&root).is_empty());
    }

    #[test]
    fn similar_command_rules_are_narrow_and_project_scoped() {
        let (_db, root) = project("similar");
        let rule = remember_similar_command(&root, "cargo test --workspace").unwrap();
        assert_eq!(rule, "cargo test");
        assert!(allows_similar_argv(
            &root,
            &["cargo".into(), "test".into(), "-p".into(), "aether1".into()]
        ));
        assert!(!allows_similar_argv(
            &root,
            &["cargo".into(), "install".into(), "malware".into()]
        ));
        assert!(similar_command_prefix("cargo test && git push").is_none());
        assert!(similar_command_prefix("git push origin main").is_none());
        let other = root.parent().unwrap().join("other");
        std::fs::create_dir_all(&other).unwrap();
        assert!(!allows_similar_argv(
            &other,
            &["cargo".into(), "test".into()]
        ));
    }
}
