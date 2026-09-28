//! The one folder AETHER CODE may change, and the commands it may run inside it.
//!
//! Until now the coding panel could look and nothing else: five read-only tools, and any
//! request to change something came back as a refusal telling the model to put the command
//! in a fenced block, where the operator's own Return key would run it. That is a defensible
//! place to stand, and it is not what a coding agent is. `opencode`, `aider`, Claude Code and
//! the rest are all the same loop -- read a file, change it, run the tests, read what broke,
//! go again -- and a panel that cannot take the second step of that loop can only ever
//! narrate it.
//!
//! So this module moves the boundary, once, and states exactly where it now sits.
//!
//!   1. **One folder.** Everything here resolves against a workspace root the operator
//!      nominated: `code_workspace_root`, or the project already selected for Graft. There
//!      is no default, and no fallback to the home directory or the current directory. With
//!      no root set, every tool in this file refuses and says how to set one.
//!   2. **Inside it, and provably so.** A path is canonicalized before it is judged, its
//!      parent when the file does not exist yet, so a symlink pointing out of the project
//!      is resolved to where it really goes and then refused. `fs_guard` is checked as well
//!      as this, not instead of it: the denied names and the home-directory rule still hold,
//!      so a workspace root cannot be used to reach a credential.
//!   3. **No shell, ever.** `run` takes an argv and spawns the program directly. There is no
//!      `sh -c` anywhere in this file, so a pipe, a redirect, a `;` or a backtick in an
//!      argument is a literal string the program will reject. This is also why the terminal
//!      isolation rule is untouched: nothing here types into the operator's terminal, and
//!      nothing here reads from it. `scripts/check_terminal_isolation.sh` passes unchanged.
//!   4. **A named program, and for git a named subcommand.** The allowlist holds program
//!      names the operator can see and edit. `git` is on it because committing is half of
//!      what this loop is for, with the subcommands that leave the repository or destroy
//!      uncommitted work refused by name -- `push`, `reset`, `clean`, `rebase` and the rest.
//!      Fail closed: a program that is not on the list is refused, whatever it does.
//!   5. **Both grants default off.** `edit` and `run` are the two permissions in this
//!      program that can change the operator's own work, so unlike the three read grants
//!      they start switched off and are turned on deliberately, per machine.
//!
//! What is deliberately *not* here: no delete, no rename, no moving files about. An agent
//! that can only add and amend text inside one folder leaves a repository something `git
//! diff` can explain, and every loss it can cause is one the version control the operator
//! already has can undo.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::llm::MemoryDb;

/// The operator's nominated project folder. Falls back to Graft's selected project, which
/// is the same question asked once already.
pub const ROOT_SETTING: &str = "code_workspace_root";
const GRAFT_PROJECT_SETTING: &str = "graft_selected_project";

/// The programs `run` may spawn, and the flag saying the starter list has been offered.
pub const ALLOWLIST_SETTING: &str = "code_run_allowlist";
const SEEDED_SETTING: &str = "code_run_allowlist_seeded";

/// How long one command gets, and the most the model may ask for. A test suite is minutes;
/// a command still running after ten is waiting for something nobody is going to type.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_TIMEOUT: Duration = Duration::from_secs(600);

/// What comes back from one command. Output past this is cut from the middle, keeping the
/// beginning (what it was doing) and the end (how it failed), because those are the two
/// parts a compiler error lives in.
const MAX_OUTPUT_BYTES: usize = 16 * 1024;

/// The most a single edit may rewrite. Not a safety rule -- a guard against a model that
/// has decided to paste an entire file back as one replacement, which is how a careful
/// edit becomes an accidental rewrite.
const MAX_REPLACEMENT_BYTES: usize = 64 * 1024;

/// What a fresh install may run, before the operator adds anything.
///
/// The rule for this list is not `run_command`'s rule one level up -- that one says no
/// argument to any listed program may change the machine, which excludes every build tool
/// there is. Here the containing rule is the workspace: these run with the project folder
/// as their working directory, and what they change is the operator's checkout, which is
/// the thing they asked for. So the test for a row is narrower than it looks: **would a
/// developer run this in the project without thinking twice?**
///
/// `git` is the one entry that needs its own table, below: `git commit` is exactly what
/// this is for and `git push` is the operator's to press.
pub const STARTER_ALLOWLIST: &[&str] = &[
    "cargo", "npm", "npx", "pnpm", "yarn", "node", "python3", "pytest", "make", "go", "rustc",
    "gradle", "mvn", "dotnet", "eslint", "prettier", "ruff", "black", "git",
];

/// The `git` subcommands `run` refuses, and why each one is on the list.
///
/// Everything else `git` does happens inside the repository and is recoverable from it.
/// These are the ones that are not:
///
/// * `push`, `remote`, `submodule` reach outside the folder this module is confined to.
/// * `reset`, `restore`, `checkout`, `clean`, `stash` destroy uncommitted work -- the
///   operator's, quite possibly, not the model's.
/// * `rebase`, `filter-branch`, `filter-repo`, `gc`, `prune`, `reflog` rewrite or expire
///   the history that makes everything else here reversible.
/// * `config` changes how git behaves for every later command, including the ones here.
const REFUSED_GIT: &[&str] = &[
    "push",
    "remote",
    "submodule",
    "reset",
    "restore",
    "checkout",
    "clean",
    "stash",
    "rebase",
    "filter-branch",
    "filter-repo",
    "gc",
    "prune",
    "reflog",
    "config",
];

// ---------------------------------------------------------------------------- the root

/// The workspace root, or a refusal saying how to set one.
pub fn root(db: &MemoryDb) -> Result<PathBuf, String> {
    let configured = db.get_setting_string(ROOT_SETTING, "");
    let configured = if configured.trim().is_empty() {
        db.get_setting_string(GRAFT_PROJECT_SETTING, "")
    } else {
        configured
    };
    let configured = configured.trim();
    if configured.is_empty() {
        return Err(
            "no project folder is set, so there is nothing this may change. The \
                    operator sets one in Settings -> AETHER CODE, or with `aether1 code \
                    workspace /path/to/project`. Tell them that is what you need."
                .to_string(),
        );
    }

    let expanded = crate::paths::expand_home(configured);
    let resolved = expanded.canonicalize().map_err(|e| {
        format!(
            "the project folder {} cannot be opened: {e}",
            expanded.display()
        )
    })?;
    if !resolved.is_dir() {
        return Err(format!(
            "the project folder {} is not a directory",
            resolved.display()
        ));
    }
    refuse_unbounded_root(&resolved)?;
    Ok(resolved)
}

/// Roots that would make "confined to the project" meaningless.
///
/// The home directory is the one that matters in practice: a companion launched from `~`
/// and pointed at it would have a workspace covering every project, every download and
/// every dotfile, while still reporting itself as confined. The filesystem root and any
/// ancestor of home are the same mistake written larger.
fn refuse_unbounded_root(resolved: &Path) -> Result<(), String> {
    if resolved.parent().is_none() {
        return Err("the filesystem root is not a project folder".to_string());
    }
    if let Some(home) = crate::paths::home_dir() {
        if let Ok(home) = home.canonicalize() {
            if resolved == home || home.starts_with(resolved) {
                return Err(format!(
                    "{} contains the whole home directory, which is not a project folder. \
                     Point the workspace at one project.",
                    resolved.display()
                ));
            }
        }
    }
    Ok(())
}

/// Resolves a path the model gave against the workspace, or says why it is out of bounds.
///
/// Relative paths are taken as relative to the root, which is what a model writing
/// `src/main.rs` means. Absolute paths are allowed and then checked like any other -- a
/// model that has read a file with `read_file` will quote the absolute path back.
pub fn resolve(db: &MemoryDb, path: &str) -> Result<PathBuf, String> {
    let root = root(db)?;
    let raw = path.trim();
    if raw.is_empty() {
        return Err("path is required".to_string());
    }

    let expanded = crate::paths::expand_home(raw);
    let joined = if expanded.is_absolute() {
        expanded
    } else {
        root.join(expanded)
    };

    // The parent is canonicalized rather than the file, so a file that does not exist yet
    // can still be judged -- and so a symlinked directory is resolved to where it really
    // points before containment is decided.
    let Some(parent) = joined.parent() else {
        return Err(format!("{} has no parent directory", joined.display()));
    };
    let Some(name) = joined.file_name() else {
        return Err(format!("{} does not name a file", joined.display()));
    };
    let parent = parent
        .canonicalize()
        .map_err(|e| format!("cannot reach {}: {e}", parent.display()))?;
    let resolved = parent.join(name);

    if !resolved.starts_with(&root) {
        return Err(format!(
            "{} is outside the project folder ({}), and this may only change files inside it",
            resolved.display(),
            root.display()
        ));
    }

    // An existing file is resolved too: a symlink inside the project pointing out of it is
    // a hole straight through rule 2.
    if resolved.is_symlink() {
        let target = resolved
            .canonicalize()
            .map_err(|e| format!("cannot follow {}: {e}", resolved.display()))?;
        if !target.starts_with(&root) {
            return Err(format!(
                "{} is a link to {}, which is outside the project folder",
                resolved.display(),
                target.display()
            ));
        }
    }

    // And the companion's own guard on top: the denied names and the home-directory rule
    // are answered in one place for the whole program, and a workspace does not overrule
    // them.
    crate::tools::fs_guard::resolve_writable(&resolved.to_string_lossy())?;
    Ok(resolved)
}

// ---------------------------------------------------------------------------- editing

/// `edit_file {"path": ..., "find": ..., "replace": ...}` -- one exact, unique replacement.
///
/// Exact text rather than a line range or a patch format, and unique rather than
/// first-match, for the same reason: both of the other shapes fail *silently* when the
/// model's idea of the file is stale. A line number that has moved edits the wrong line and
/// returns success. `find` text that appears twice is genuinely ambiguous, so it is refused
/// with the count and the model is told to include more surrounding context -- which is the
/// one thing that reliably makes a small model's next attempt correct.
pub fn edit_file(db: &MemoryDb, args: &Value) -> Result<String, String> {
    let path = string_arg(args, "path")?;
    let find = string_arg(args, "find")?;
    let replace = args
        .get("replace")
        .and_then(Value::as_str)
        .ok_or_else(|| "replace is required (use \"\" to delete the text)".to_string())?;

    if find.is_empty() {
        return Err("find cannot be empty -- to create a file use create_file".to_string());
    }
    if replace.len() > MAX_REPLACEMENT_BYTES {
        return Err(format!(
            "that replacement is {} bytes, and one edit may rewrite at most {MAX_REPLACEMENT_BYTES}. \
             Make several smaller edits.",
            replace.len()
        ));
    }

    let resolved = resolve(db, &path)?;
    let before = std::fs::read_to_string(&resolved)
        .map_err(|e| format!("cannot read {}: {e}", resolved.display()))?;

    let count = before.matches(find.as_str()).count();
    match count {
        0 => {
            return Err(format!(
                "that text does not appear in {}. Read the file again -- it is not what you \
                 think it is. Whitespace and indentation count.",
                resolved.display()
            ))
        }
        1 => {}
        many => {
            return Err(format!(
                "that text appears {many} times in {}, so which one to change is ambiguous. \
                 Include more of the surrounding lines in `find` so it matches exactly once.",
                resolved.display()
            ))
        }
    }

    let after = before.replacen(find.as_str(), replace, 1);
    if after == before {
        return Err("that edit would change nothing".to_string());
    }
    std::fs::write(&resolved, &after)
        .map_err(|e| format!("cannot write {}: {e}", resolved.display()))?;

    let _ = db.log_action(
        "code.edit_file",
        &serde_json::json!({ "path": resolved.to_string_lossy(), "bytes_before": before.len() }),
        true,
        crate::llm::ActionStatus::Executed,
        None,
    );

    Ok(format!(
        "{} updated: {} line(s) replaced by {} line(s).\n\n{}",
        display_in_root(db, &resolved),
        find.lines().count().max(1),
        replace
            .lines()
            .count()
            .max(if replace.is_empty() { 0 } else { 1 }),
        sketch(&after, replace_start(&before, &find))
    ))
}

/// `create_file {"path": ..., "content": ...}` -- a new file, or the whole of an existing one.
///
/// Overwriting is allowed and is named in the result, rather than being a separate tool: a
/// model that means to create a file that already exists has nearly always misread the
/// project, and the operator reading "overwrote" learns that in one word.
pub fn create_file(db: &MemoryDb, args: &Value) -> Result<String, String> {
    let path = string_arg(args, "path")?;
    let content = args
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| "content is required".to_string())?;
    if content.len() > MAX_REPLACEMENT_BYTES {
        return Err(format!(
            "that file is {} bytes, and one write may be at most {MAX_REPLACEMENT_BYTES}",
            content.len()
        ));
    }

    let resolved = resolve(db, &path)?;
    let existed = resolved.exists();
    if let Some(parent) = resolved.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    std::fs::write(&resolved, content)
        .map_err(|e| format!("cannot write {}: {e}", resolved.display()))?;

    let _ = db.log_action(
        "code.create_file",
        &serde_json::json!({ "path": resolved.to_string_lossy(), "overwrote": existed }),
        true,
        crate::llm::ActionStatus::Executed,
        None,
    );

    Ok(format!(
        "{} {}: {} bytes, {} line(s).",
        display_in_root(db, &resolved),
        if existed { "overwritten" } else { "created" },
        content.len(),
        content.lines().count()
    ))
}

// ---------------------------------------------------------------------------- running

/// The programs `run` may spawn right now.
pub fn allowlist(db: &MemoryDb) -> Vec<String> {
    db.get_setting(ALLOWLIST_SETTING)
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
        .unwrap_or_default()
}

/// Puts the starter list in place on a fresh install, once and never again -- the same rule
/// `run_command`'s own seeding follows. An operator who empties the list is saying no, and a
/// default that comes back is not a default.
pub fn seed_starter_allowlist(db: &MemoryDb) {
    if db.get_setting_bool(SEEDED_SETTING, false) {
        return;
    }
    let starter: Vec<String> = STARTER_ALLOWLIST.iter().map(|s| s.to_string()).collect();
    let _ = db.set_setting(ALLOWLIST_SETTING, &serde_json::json!(starter));
    let _ = db.set_setting(SEEDED_SETTING, &serde_json::json!(true));
}

/// `run {"argv": ["cargo", "test"], "cwd": "crates/thing", "timeout_secs": 300}`
///
/// The return value is what a developer would have seen: the exit status, then stdout and
/// stderr as the program wrote them. A failing command is *not* an `Err` -- a test suite
/// that fails is the loop working, and an error here means the command could not be run at
/// all. Getting that distinction backwards makes a model apologise for a red test instead of
/// reading it.
pub fn run(db: &MemoryDb, args: &Value) -> Result<String, String> {
    // The boundary, before anything is decided. What is allowed to run depends entirely on
    // whether the kernel is holding the walls up, so this is the first question asked.
    let sandbox = crate::code_sandbox::detect();
    if !sandbox.confines() && !crate::code_sandbox::unconfined_allowed(db) {
        return Err(crate::code_sandbox::unconfined_refusal(&sandbox));
    }
    let confined = sandbox.confines();

    // `shell` is one string handed to `sh -lc`, and it exists only inside the sandbox. A
    // shell is the natural way to say `cargo test && cargo clippy`, and refusing one while
    // allowing `bash` in an argv would be a distinction with nothing behind it. Outside the
    // sandbox there is no boundary for it to be inside, so there it is refused.
    let argv: Vec<String> = match args.get("shell").and_then(Value::as_str) {
        Some(line) if !line.trim().is_empty() => {
            if !confined {
                return Err(
                    "shell is only available inside the sandbox, and this machine has none. \
                     Run a single program with argv instead, or see `aether1 code \
                     run-unconfined` for what running without a sandbox means."
                        .to_string(),
                );
            }
            vec!["/bin/sh".to_string(), "-lc".to_string(), line.to_string()]
        }
        _ => args
            .get("argv")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(|v| v.as_str().unwrap_or_default().to_string())
                    .collect()
            })
            .unwrap_or_default(),
    };
    let Some((program, rest)) = argv.split_first() else {
        return Err(
            "run needs an argv array: {\"argv\": [\"cargo\", \"test\"]}, or a shell line: \
             {\"shell\": \"cargo test && cargo clippy\"}."
                .to_string(),
        );
    };
    if program.trim().is_empty() {
        return Err("the first entry of argv must be a program name".to_string());
    }

    // The allowlist is policy, and it only has a job where there is nothing else. Inside
    // the sandbox it is skipped entirely, and that is not a loosening so much as an
    // admission: a list that permits `python3`, `node` and `make` permits arbitrary code
    // already, and one that permits a shell permits everything the shell can reach. What
    // stops a command mattering is the box around it, so the box is what is checked. Where
    // there is no box -- an operator who has switched `run-unconfined` on -- the list is
    // the only thing standing anywhere, and it is enforced exactly as it always was.
    if !confined {
        check_allowed(db, program, rest)?;
    }

    let root = root(db)?;
    let cwd = match args.get("cwd").and_then(Value::as_str) {
        None | Some("") => root.clone(),
        Some(sub) => {
            let dir = resolve(db, sub)?;
            if !dir.is_dir() {
                return Err(format!("{} is not a directory", dir.display()));
            }
            dir
        }
    };

    let timeout = match args.get("timeout_secs").and_then(Value::as_u64) {
        None => DEFAULT_TIMEOUT,
        Some(secs) => {
            let asked = Duration::from_secs(secs);
            if asked.is_zero() || asked > MAX_TIMEOUT {
                return Err(format!(
                    "timeout_secs must be between 1 and {}",
                    MAX_TIMEOUT.as_secs()
                ));
            }
            asked
        }
    };

    let id = db
        .log_action(
            "code.run",
            &serde_json::json!({ "argv": argv, "cwd": cwd.to_string_lossy() }),
            true,
            crate::llm::ActionStatus::Executed,
            None,
        )
        .unwrap_or(0);

    let started = Instant::now();
    // Spawned directly, never through a shell: `argv` is passed as arguments, so nothing in
    // it can be interpreted as syntax. Wrapped in a sandbox where the machine has one, and
    // refused above where it does not, so the containment this tool claims is enforced by
    // the kernel rather than by the allowlist -- see `code_sandbox`.
    let mut child = crate::code_sandbox::command(
        &sandbox,
        program,
        rest,
        &cwd,
        &root,
        crate::code_sandbox::network_for(&sandbox, db),
    )
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .map_err(|e| format!("{program} could not be started in {}: {e}", cwd.display()))?;

    // Polled rather than waited on, so a command that never returns is killed rather than
    // holding the panel open until the operator gives up on it.
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(format!("{program} could not be waited for: {e}")),
        }
    };

    let output = child
        .wait_with_output()
        .map_err(|e| format!("{program} produced no readable output: {e}"))?;
    let mut report = String::new();
    match status {
        None => report.push_str(&format!(
            "`{}` was killed after {} seconds without finishing.\n",
            argv.join(" "),
            timeout.as_secs()
        )),
        Some(status) => report.push_str(&format!(
            "`{}` exited {} after {:.1}s.\n",
            argv.join(" "),
            match status.code() {
                Some(0) => "successfully (0)".to_string(),
                Some(code) => format!("with status {code}"),
                None => "on a signal".to_string(),
            },
            started.elapsed().as_secs_f32()
        )),
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stdout.trim().is_empty() {
        report.push_str(&format!("\nstdout:\n{}\n", middle_out(&stdout)));
    }
    if !stderr.trim().is_empty() {
        report.push_str(&format!("\nstderr:\n{}\n", middle_out(&stderr)));
    }
    if stdout.trim().is_empty() && stderr.trim().is_empty() {
        report.push_str("\nIt printed nothing.\n");
    }

    let _ = id;
    Ok(report)
}

/// Whether this argv may run, as a refusal the model can act on.
pub fn check_allowed(db: &MemoryDb, program: &str, rest: &[String]) -> Result<(), String> {
    // The program is matched by name only. A path is refused rather than resolved, because
    // `/tmp/cargo` and `cargo` are not the same program and an allowlist that cannot tell
    // them apart is not one.
    if program.contains('/') || program.contains('\\') {
        return Err(format!(
            "name the program, not a path to it: {program:?}. The allowlist matches names."
        ));
    }

    let allowed = allowlist(db);
    if !allowed.iter().any(|entry| entry == program) {
        return Err(format!(
            "{program} is not on the list of programs this may run. That list is the \
             operator's: they add one with `aether1 code run-allow {program}`, or in \
             Settings -> AETHER CODE. What is on it now: {}",
            if allowed.is_empty() {
                "nothing".to_string()
            } else {
                allowed.join(", ")
            }
        ));
    }

    if program == "git" {
        // The first argument that is not a global flag is the subcommand. `git -C /elsewhere`
        // is refused outright -- it is the one flag that moves git out of the workspace.
        for arg in rest {
            if arg == "-C" || arg.starts_with("--git-dir") || arg.starts_with("--work-tree") {
                return Err(format!(
                    "{arg} points git at another directory, and this runs in the project \
                     folder only. Use cwd if you mean a subdirectory of it."
                ));
            }
            if arg.starts_with('-') {
                continue;
            }
            if REFUSED_GIT.iter().any(|refused| refused == arg) {
                return Err(format!(
                    "git {arg} is refused: it either reaches outside the project folder or \
                     destroys work that is not committed. Do the part that is yours, and \
                     write the `git {arg}` line in a ```bash block for the operator to run."
                ));
            }
            break;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------- helpers

fn string_arg(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("{key} is required"))
}

/// The path as the operator reads it: relative to the project, since that is how they and
/// every other tool in the project talk about it.
fn display_in_root(db: &MemoryDb, path: &Path) -> String {
    root(db)
        .ok()
        .and_then(|root| {
            path.strip_prefix(root)
                .ok()
                .map(|p| p.display().to_string())
        })
        .unwrap_or_else(|| path.display().to_string())
}

/// Where the replaced text started, as a line number.
fn replace_start(before: &str, find: &str) -> usize {
    before
        .find(find)
        .map(|byte| before[..byte].lines().count().max(1))
        .unwrap_or(1)
}

/// A few lines either side of the change, so the model can see what it actually did
/// without being handed the file back.
fn sketch(after: &str, around_line: usize) -> String {
    let lines: Vec<&str> = after.lines().collect();
    let start = around_line.saturating_sub(3).max(1);
    let end = (around_line + 6).min(lines.len());
    let mut out = String::new();
    for (i, line) in lines
        .iter()
        .enumerate()
        .take(end)
        .skip(start.saturating_sub(1))
    {
        out.push_str(&format!("{:>5} | {line}\n", i + 1));
    }
    out
}

/// Keeps the head and the tail of long output and says how much went missing. A compiler
/// writes what it was doing first and why it stopped last; the middle is the part that can
/// be spared.
fn middle_out(text: &str) -> String {
    if text.len() <= MAX_OUTPUT_BYTES {
        return text.trim_end().to_string();
    }
    let keep = MAX_OUTPUT_BYTES / 2;
    let head_end = floor_char_boundary(text, keep);
    let tail_start = ceil_char_boundary(text, text.len() - keep);
    format!(
        "{}\n\n[... {} bytes of the middle left out ...]\n\n{}",
        &text[..head_end],
        tail_start - head_end,
        text[tail_start..].trim_end()
    )
}

fn floor_char_boundary(text: &str, mut at: usize) -> usize {
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

fn ceil_char_boundary(text: &str, mut at: usize) -> usize {
    while at < text.len() && !text.is_char_boundary(at) {
        at += 1;
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway home directory, named for the test that asked for it so two running at
    /// once cannot land in each other's.
    fn temp_home(name: &str) -> PathBuf {
        let home =
            std::env::temp_dir().join(format!("aether1_workspace_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        home.canonicalize().unwrap()
    }

    /// A database and a project folder inside that home, wired to each other.
    fn workspace(name: &str) -> (MemoryDb, PathBuf, PathBuf) {
        let home = temp_home(name);
        let project = home.join("project");
        std::fs::create_dir_all(project.join("src")).unwrap();
        let db = MemoryDb::open(home.join("db.sqlite")).unwrap();
        db.set_setting(
            ROOT_SETTING,
            &serde_json::json!(project.to_string_lossy().to_string()),
        )
        .unwrap();
        (db, project.canonicalize().unwrap(), home)
    }

    /// `fs_guard` only writes inside the operator's home, so the temp home has to be the
    /// home for the duration. The lock is the one fs_guard's own tests take, because two
    /// tests swapping HOME at once is a race.
    fn with_home<T>(home: &Path, f: impl FnOnce() -> T) -> T {
        let _guard = crate::tools::fs_guard::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var("HOME").ok();
        std::env::set_var("HOME", home);
        let out = f();
        match previous {
            Some(p) => std::env::set_var("HOME", p),
            None => std::env::remove_var("HOME"),
        }
        out
    }

    #[test]
    fn with_no_root_set_everything_refuses_and_says_how() {
        let home = temp_home("noroot");
        let db = MemoryDb::open(home.join("db.sqlite")).unwrap();
        let err = root(&db).unwrap_err();
        assert!(err.contains("no project folder is set"), "{err}");
        assert!(err.contains("aether1 code workspace"), "{err}");
    }

    #[test]
    fn the_home_directory_is_not_a_project_folder() {
        let home = temp_home("ishome");
        let db = MemoryDb::open(home.join("db.sqlite")).unwrap();
        db.set_setting(
            ROOT_SETTING,
            &serde_json::json!(home.to_string_lossy().to_string()),
        )
        .unwrap();
        with_home(&home, || {
            let err = root(&db).unwrap_err();
            assert!(err.contains("whole home directory"), "{err}");
        });
    }

    #[test]
    fn a_path_outside_the_project_is_refused() {
        let (db, _project, home) = workspace("outside");
        with_home(&home, || {
            let err = resolve(&db, "../elsewhere.txt").unwrap_err();
            assert!(err.contains("outside the project folder"), "{err}");
        });
    }

    #[test]
    fn a_link_pointing_out_of_the_project_is_followed_and_refused() {
        let (db, project, home) = workspace("link");
        let outside = home.join("secret.txt");
        std::fs::write(&outside, "not yours").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, project.join("link.txt")).unwrap();
        #[cfg(unix)]
        with_home(&home, || {
            let err = resolve(&db, "link.txt").unwrap_err();
            assert!(err.contains("outside the project folder"), "{err}");
        });
    }

    #[test]
    fn an_edit_replaces_exactly_once() {
        let (db, project, home) = workspace("edit");
        std::fs::write(
            project.join("src/main.rs"),
            "fn main() {\n    let x = 1;\n}\n",
        )
        .unwrap();
        with_home(&home, || {
            let out = edit_file(
                &db,
                &serde_json::json!({
                    "path": "src/main.rs",
                    "find": "let x = 1;",
                    "replace": "let x = 2;",
                }),
            )
            .unwrap();
            assert!(out.starts_with("src/main.rs updated"), "{out}");
            let after = std::fs::read_to_string(project.join("src/main.rs")).unwrap();
            assert_eq!(after, "fn main() {\n    let x = 2;\n}\n");
        });
    }

    #[test]
    fn an_ambiguous_edit_is_refused_with_the_count() {
        let (db, project, home) = workspace("ambiguous");
        std::fs::write(project.join("src/main.rs"), "let x = 1;\nlet x = 1;\n").unwrap();
        with_home(&home, || {
            let err = edit_file(
                &db,
                &serde_json::json!({"path": "src/main.rs", "find": "let x = 1;", "replace": "let x = 2;"}),
            )
            .unwrap_err();
            assert!(err.contains("appears 2 times"), "{err}");
            assert!(err.contains("more of the surrounding lines"), "{err}");
            // And nothing was written.
            let after = std::fs::read_to_string(project.join("src/main.rs")).unwrap();
            assert_eq!(after, "let x = 1;\nlet x = 1;\n");
        });
    }

    #[test]
    fn text_that_is_not_there_says_so_rather_than_guessing() {
        let (db, project, home) = workspace("missing");
        std::fs::write(project.join("src/main.rs"), "fn main() {}\n").unwrap();
        with_home(&home, || {
            let err = edit_file(
                &db,
                &serde_json::json!({"path": "src/main.rs", "find": "fn other()", "replace": "x"}),
            )
            .unwrap_err();
            assert!(err.contains("does not appear"), "{err}");
            assert!(err.contains("Whitespace and indentation count"), "{err}");
        });
    }

    #[test]
    fn creating_says_whether_it_overwrote() {
        let (db, _project, home) = workspace("create");
        with_home(&home, || {
            let first = create_file(
                &db,
                &serde_json::json!({"path": "src/new.rs", "content": "//\n"}),
            )
            .unwrap();
            assert!(first.contains("created"), "{first}");
            let again = create_file(
                &db,
                &serde_json::json!({"path": "src/new.rs", "content": "//!\n"}),
            )
            .unwrap();
            assert!(again.contains("overwritten"), "{again}");
        });
    }

    #[test]
    fn a_program_that_is_not_on_the_list_is_refused_with_the_list() {
        let (db, _project, _home) = workspace("notlisted");
        seed_starter_allowlist(&db);
        let err = check_allowed(&db, "rm", &["-rf".to_string()]).unwrap_err();
        assert!(err.contains("not on the list"), "{err}");
        assert!(
            err.contains("cargo"),
            "the refusal names what is allowed: {err}"
        );
    }

    #[test]
    fn a_path_to_a_program_is_not_the_program() {
        let (db, _project, _home) = workspace("progpath");
        seed_starter_allowlist(&db);
        let err = check_allowed(&db, "/tmp/cargo", &[]).unwrap_err();
        assert!(err.contains("name the program"), "{err}");
    }

    #[test]
    fn git_commits_but_does_not_push_or_reset() {
        let (db, _project, _home) = workspace("git");
        seed_starter_allowlist(&db);
        assert!(check_allowed(&db, "git", &["commit".into(), "-m".into(), "x".into()]).is_ok());
        assert!(check_allowed(&db, "git", &["diff".into()]).is_ok());
        for refused in ["push", "reset", "checkout", "clean", "rebase", "config"] {
            let err = check_allowed(&db, "git", &[refused.to_string()]).unwrap_err();
            assert!(err.contains(&format!("git {refused} is refused")), "{err}");
        }
    }

    #[test]
    fn a_global_flag_does_not_hide_the_subcommand() {
        let (db, _project, _home) = workspace("gitflag");
        seed_starter_allowlist(&db);
        let err = check_allowed(&db, "git", &["--no-pager".into(), "push".into()]).unwrap_err();
        assert!(err.contains("git push is refused"), "{err}");
        let err = check_allowed(&db, "git", &["-C".into(), "/etc".into()]).unwrap_err();
        assert!(err.contains("another directory"), "{err}");
    }

    #[test]
    fn the_starter_list_is_offered_once() {
        let (db, _project, _home) = workspace("seed");
        seed_starter_allowlist(&db);
        db.set_setting(ALLOWLIST_SETTING, &serde_json::json!(Vec::<String>::new()))
            .unwrap();
        seed_starter_allowlist(&db);
        assert!(
            allowlist(&db).is_empty(),
            "an emptied list is an answer, not a box to refill"
        );
    }

    #[test]
    fn a_real_command_runs_in_the_project_and_reports_how_it_went() {
        let (db, project, home) = workspace("run");
        std::fs::write(project.join("hello.txt"), "hi\n").unwrap();
        db.set_setting(ALLOWLIST_SETTING, &serde_json::json!(["ls", "false"]))
            .unwrap();
        // This test is about the reporting, not the boundary, so it runs either way: the
        // sandbox where there is one, and unconfined where there is not.
        allow_unconfined_if_needed(&db);
        with_home(&home, || {
            let out = run(&db, &serde_json::json!({"argv": ["ls"]})).unwrap();
            assert!(out.contains("exited successfully (0)"), "{out}");
            assert!(out.contains("hello.txt"), "it ran in the project: {out}");

            // A command that fails is a result, not an error -- the loop has to read it.
            let failed = run(&db, &serde_json::json!({"argv": ["false"]})).unwrap();
            assert!(failed.contains("exited with status 1"), "{failed}");
        });
    }

    // ------------------------------------------------------ the sandbox, adversarially
    //
    // These are the tests the security review asked for, and they are written the way it
    // framed the question: not "does the allowlist contain python3", but *can a
    // model-controlled argv cause a read, a write or a network call outside the workspace*.
    // Every one of them passes an allowlisted interpreter a hostile one-liner and then
    // looks at what actually happened on disk.
    //
    // They need a sandbox to mean anything, so on a machine without one they assert the
    // other half of the promise instead -- that `run` refuses rather than pretending.

    /// Lets a test that is not about the boundary run on a machine with no sandbox.
    fn allow_unconfined_if_needed(db: &MemoryDb) {
        if !crate::code_sandbox::detect().confines() {
            db.set_setting(
                crate::code_sandbox::UNCONFINED_SETTING,
                &serde_json::json!(true),
            )
            .unwrap();
        }
    }

    /// A workspace with `run` on, the given programs allowed, and the sandbox this machine
    /// actually has. `None` when there is no sandbox, having first checked that `run`
    /// refuses and explains itself.
    fn adversarial(name: &str, programs: &[&str]) -> Option<(MemoryDb, PathBuf, PathBuf)> {
        let (db, project, home) = workspace(name);
        crate::code_perms::set(&db, crate::code_perms::Grant::Run, true).unwrap();
        db.set_setting(ALLOWLIST_SETTING, &serde_json::json!(programs))
            .unwrap();
        if crate::code_sandbox::detect().confines() {
            return Some((db, project, home));
        }
        // Any program will do to see the refusal, and the caller may have passed none --
        // the shell test deliberately allows nothing, to prove the list is not the gate.
        let probe = programs.first().copied().unwrap_or("python3");
        let err = with_home(&home, || {
            run(&db, &serde_json::json!({"argv": [probe]})).unwrap_err()
        });
        assert!(
            err.contains("cannot confine it"),
            "with no sandbox, run must refuse and say why: {err}"
        );
        assert!(
            err.contains("code run-unconfined"),
            "and must say how to override it: {err}"
        );
        None
    }

    /// The exact example from the review: `python3 -c` reading a credential out of the
    /// operator's home directory. It must come back empty-handed.
    #[test]
    fn python_cannot_read_the_operators_home() {
        let Some((db, project, home)) = adversarial("py_read", &["python3"]) else {
            return;
        };
        let secret = home.join(".ssh");
        std::fs::create_dir_all(&secret).unwrap();
        std::fs::write(secret.join("id_ed25519"), "PRIVATE-KEY-MATERIAL").unwrap();

        let out = with_home(&home, || {
            run(
                &db,
                &serde_json::json!({"argv": [
                    "python3", "-c",
                    "import os,pathlib\n\
                     p=pathlib.Path(os.path.expanduser('~/.ssh/id_ed25519'))\n\
                     print('CONTENTS:'+p.read_text() if p.exists() else 'ABSENT')",
                ]}),
            )
            .unwrap()
        });
        assert!(
            !out.contains("PRIVATE-KEY-MATERIAL"),
            "the key must not be readable from inside the sandbox: {out}"
        );
        assert!(out.contains("ABSENT"), "{out}");
        let _ = project;
    }

    /// And writing outside it. The file must not appear, and the workspace write beside it
    /// must -- a sandbox that blocks everything would pass the first half by being useless.
    #[test]
    fn python_cannot_write_outside_the_workspace() {
        let Some((db, project, home)) = adversarial("py_write", &["python3"]) else {
            return;
        };
        let outside = home.join("outside.txt");
        let out = with_home(&home, || {
            run(
                &db,
                &serde_json::json!({"argv": [
                    "python3", "-c",
                    format!(
                        "open('{}','w').write('escaped')\nprint('WROTE OUTSIDE')",
                        outside.display()
                    ),
                ]}),
            )
            .unwrap()
        });
        // The write is *contained*, not refused: inside the box the path exists on a
        // tmpfs and `open` succeeds, so the command reports success. What matters is that
        // nothing reached the operator's disk, which is what this asserts. Nothing here
        // asserts on stdout, because the report echoes the argv and the argv is the
        // attack -- a test looking for its own marker in that echo always finds it.
        assert!(
            !outside.exists(),
            "a write outside the workspace landed on disk: {out}"
        );

        let inside = with_home(&home, || {
            run(
                &db,
                &serde_json::json!({"argv": ["python3", "-c", "open('made.txt','w').write('ok')"]}),
            )
            .unwrap()
        });
        assert!(
            project.join("made.txt").is_file(),
            "but the workspace itself is writable: {inside}"
        );
    }

    /// The network, which is off unless the operator turns it on. `node -e` stands in for
    /// every allowlisted interpreter here; they all have a socket call.
    #[test]
    fn a_command_cannot_reach_the_network_by_default() {
        let Some((db, _project, home)) = adversarial("net", &["python3"]) else {
            return;
        };
        // A machine that will not let bubblewrap unshare a network namespace -- a
        // container, a CI runner -- keeps its filesystem confinement and loses this half.
        // `Sandbox::description` says so out loud, which is the behaviour being relied on
        // here; there is nothing to assert about a namespace that cannot exist.
        if !crate::code_sandbox::detect().can_cut_network() {
            return;
        }
        assert!(!crate::code_sandbox::network_allowed(&db));
        let out = with_home(&home, || {
            run(
                &db,
                &serde_json::json!({"argv": [
                    "python3", "-c",
                    "import socket\n\
                     try:\n\
                     \x20 socket.create_connection(('1.1.1.1',443),timeout=5)\n\
                     \x20 print('NET'+'-OPEN')\n\
                     except OSError as e:\n\
                     \x20 print('NET'+'-BLOCKED')",
                ], "timeout_secs": 30}),
            )
            .unwrap()
        });
        // Assembled at runtime, so these markers can only come from the command's own
        // output and never from the argv the report echoes back.
        assert!(!out.contains("NET-OPEN"), "{out}");
        assert!(out.contains("NET-BLOCKED"), "{out}");
    }

    /// `make` runs whatever the Makefile says, and the Makefile is a file the model can
    /// write. So the recipe is the attack, and it is confined the same as everything else.
    #[test]
    fn a_makefile_recipe_is_confined_too() {
        let Some((db, project, home)) = adversarial("make", &["make"]) else {
            return;
        };
        if crate::paths::find_installed_binary(&["make"]).is_none() {
            return;
        }
        let outside = home.join("from-make.txt");
        std::fs::write(
            project.join("Makefile"),
            format!("all:\n\techo escaped > {}\n", outside.display()),
        )
        .unwrap();
        let out = with_home(&home, || {
            run(&db, &serde_json::json!({"argv": ["make"]})).unwrap()
        });
        assert!(
            !outside.exists(),
            "a make recipe wrote outside the workspace: {out}"
        );
    }

    /// The point of the whole exercise: inside the box the model gets a shell and the
    /// allowlist is not consulted, because a list that permits `python3` and `make`
    /// permits arbitrary code already. What stops a command mattering is the box.
    #[test]
    fn inside_the_sandbox_there_is_a_shell_and_no_allowlist() {
        // Deliberately an empty allowlist: if it were still the gate, nothing would run.
        let Some((db, project, home)) = adversarial("shell", &[]) else {
            return;
        };
        let out = with_home(&home, || {
            run(
                &db,
                &serde_json::json!({"shell": "echo one > a.txt && echo two >> a.txt && wc -l < a.txt"}),
            )
            .unwrap()
        });
        assert!(out.contains("exited successfully (0)"), "{out}");
        assert_eq!(
            std::fs::read_to_string(project.join("a.txt")).unwrap(),
            "one\ntwo\n",
            "a pipeline, a redirect and an && all worked: {out}"
        );

        // And it is still a box: the shell is confined exactly as an argv is.
        let outside = home.join("from-the-shell.txt");
        let escaped = with_home(&home, || {
            run(
                &db,
                &serde_json::json!({"shell": format!("echo no > {}", outside.display())}),
            )
            .unwrap()
        });
        assert!(
            !outside.exists(),
            "the shell is inside the sandbox too: {escaped}"
        );
    }

    /// Without a sandbox there is nothing for a shell to be inside, so there is no shell --
    /// whatever the operator has switched on. `run-unconfined` buys back the old capability,
    /// one program at a time from the operator's own list, not a new one.
    #[test]
    fn without_a_sandbox_there_is_no_shell_even_unconfined() {
        if crate::code_sandbox::detect().confines() {
            return;
        }
        let (db, _project, home) = workspace("noshell");
        db.set_setting(
            crate::code_sandbox::UNCONFINED_SETTING,
            &serde_json::json!(true),
        )
        .unwrap();
        let err = with_home(&home, || {
            run(&db, &serde_json::json!({"shell": "echo hello"})).unwrap_err()
        });
        assert!(err.contains("only available inside the sandbox"), "{err}");
    }

    /// Aether1 holds API keys in its own environment. A build script is not entitled to
    /// them, and the sandbox rebuilds the environment from a short list rather than
    /// inheriting one.
    #[test]
    fn the_environment_does_not_leak_into_a_command() {
        let Some((db, _project, home)) = adversarial("env", &["python3"]) else {
            return;
        };
        let out = with_home(&home, || {
            std::env::set_var("AETHER1_LLM_KEY_FOR_TEST", "sk-secret");
            let out = run(
                &db,
                &serde_json::json!({"argv": [
                    "python3", "-c",
                    "import os; print('KEY:'+os.environ.get('AETHER1_LLM_KEY_FOR_TEST','ABSENT'))",
                ]}),
            )
            .unwrap();
            std::env::remove_var("AETHER1_LLM_KEY_FOR_TEST");
            out
        });
        assert!(out.contains("KEY:ABSENT"), "{out}");
    }

    #[test]
    fn long_output_keeps_both_ends() {
        let long = "START".to_string() + &"a".repeat(MAX_OUTPUT_BYTES * 3) + "THE-END";
        let cut = middle_out(&long);
        assert!(cut.starts_with("START"), "the beginning survives");
        assert!(cut.ends_with("THE-END"), "and so does the end");
        assert!(
            cut.contains("bytes of the middle left out"),
            "and it says so"
        );
        assert!(cut.len() < long.len() / 2);
    }
}
