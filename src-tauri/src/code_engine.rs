//! Where the engine's own state lives, so it survives the box.
//!
//! The sandbox puts a tmpfs over `$HOME` and binds a short list of toolchain caches back
//! over it. That is the right default and it has one consequence nobody wanted: an engine
//! started in the box writes its configuration, its session database and its logs into
//! `$HOME`, and all of it is gone when the command returns. Every run starts from nothing.
//! For `cargo test` that is a feature. For an agent that is supposed to remember what it was
//! doing, it is the whole problem.
//!
//! So the engine gets one directory of its own, bound in writable, and the four `XDG_*`
//! variables point at it. Verified against opencode 1.19.0: with those four set it writes
//! **nothing at all** into `$HOME` -- config, the session database, logs, locks and its
//! downloaded binaries all land under them.
//!
//! **Where it lives, and why not in the project.** The obvious home is
//! `<project>/.aether/engine/`, next to `policy.json`, so the state travels with the
//! repository. Two things rule that out:
//!
//!   1. At `Level::Assistant` the workspace is bind-mounted **read-only** -- the agent reads
//!      and changes nothing. An engine that cannot write its own session database cannot run
//!      at all there, and binding one writable hole into a read-only workspace would turn
//!      the simplest level into the one with an exception in it.
//!   2. A session database is AETHER1's state *about* a project, not a file *of* it. Putting
//!      it in the tree makes it something the operator has to gitignore, review in diffs, and
//!      explain to collaborators.
//!
//! So it lives beside the settings database instead, one directory per project folder, keyed
//! by the canonical path. The folder's own name is kept on the front of the key so an
//! operator looking at the directory can tell which is which; the hash is what makes it
//! unambiguous, because two projects can be called `api`.
//!
//! What this is not: a sandbox hole. The directory is bound in only for a caller that asks
//! for it, the same rule the model socket follows -- `code_workspace::run` does not ask, so
//! no command the operator or the companion starts can see it.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::llm::MemoryDb;

/// Where the directory is mounted inside the box.
///
/// Under the box's `$HOME`, which is the tmpfs the sandbox makes for itself -- bubblewrap can
/// create a mount point there, and it cannot on the read-only host. Named visibly rather than
/// hidden: an operator who gets a shell in the box and wonders where the agent's files went
/// should be able to see the answer in `ls`.
pub const DIR_NAME: &str = "aether1-engine";

/// The four roots inside the directory, which are the four `XDG_*` variables.
///
/// Kept apart rather than pointed at one directory, because the specification separates them
/// and a tool that honours it will put a 400 MB downloaded runtime in the cache root and its
/// session history in the data root. An operator clearing space should be able to delete one
/// and keep the other.
const ROOTS: [(&str, &str); 4] = [
    ("XDG_CONFIG_HOME", "config"),
    ("XDG_DATA_HOME", "data"),
    ("XDG_STATE_HOME", "state"),
    ("XDG_CACHE_HOME", "cache"),
];

/// The directory on this machine that holds one project's engine state, made if it is not
/// there yet.
///
/// `root` is the workspace. It is canonicalised first, so the same folder reached by a
/// symlink or a relative path is the same project rather than a second one with an empty
/// history.
pub fn state_dir(db: &MemoryDb, root: &Path) -> Result<PathBuf, String> {
    let base = db
        .path()
        .parent()
        .ok_or_else(|| "there is nowhere to put the engine's state".to_string())?
        .join("engine");
    let dir = base.join(key_for(root));
    for (_, leaf) in ROOTS {
        std::fs::create_dir_all(dir.join(leaf))
            .map_err(|e| format!("{} could not be made: {e}", dir.join(leaf).display()))?;
    }
    Ok(dir)
}

/// One project folder's directory name: something readable, then something unambiguous.
fn key_for(root: &Path) -> String {
    let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let digest = Sha256::digest(canonical.to_string_lossy().as_bytes());
    let hash: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();

    // The folder's own name, with anything that is not a plain character dropped rather than
    // replaced: this half is a label for a human reading `ls`, and the hash is what has to be
    // unique. An empty or unreadable name simply leaves the hash standing alone.
    let label: String = canonical
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(24)
        .collect();

    if label.is_empty() {
        hash
    } else {
        format!("{label}-{hash}")
    }
}

/// The `XDG_*` variables a command gets when the directory is bound in, pointing at the four
/// roots *as the box sees them*.
///
/// `home` is the box's `$HOME`, which is the operator's own home path -- the tmpfs is mounted
/// over it rather than somewhere else, so the path is the same inside and out. The host-side
/// directory is a different path entirely, and nothing inside the box is ever told it.
pub fn env_in_sandbox(home: &str) -> Vec<(&'static str, String)> {
    ROOTS
        .iter()
        .map(|(key, leaf)| (*key, format!("{home}/{DIR_NAME}/{leaf}")))
        .collect()
}

// ------------------------------------------------------------------ running it

/// The engine AETHER CODE drives, and where to find it.
///
/// One name rather than a list. `code_setup` offers the operator a choice of two agents to
/// run themselves, which is a different question from which one AETHER1 starts inside its own
/// box: that one has to be pinned, because the box's shape, the config written into it and
/// the arguments below are all specific to it.
pub const ENGINE: &str = "opencode";

/// What came back from a run.
#[derive(Debug)]
pub struct Done {
    pub status: i32,
    pub output: String,
}

/// Writes the engine's configuration into its own directory, pointing it at the model socket.
///
/// Rewritten every run rather than once, so an operator who changes model or endpoint gets
/// the new one on the next task instead of on the next install. The address written here is
/// the box's -- `127.0.0.1` on the relay port -- because that is the only address that means
/// anything to a program inside.
fn write_config(state: &Path, base_url: &str, model: &str) -> Result<(), String> {
    // The two shapes again, as `code_setup::agents` normalises them: opencode speaks the
    // OpenAI-compatible half, and handing it a base URL that already ends in `/v1` would
    // point it at `.../v1/v1`.
    let openai = format!(
        "{}/v1",
        base_url
            .trim_end_matches('/')
            .trim_end_matches("/v1")
            .trim_end_matches('/')
    );
    let config = serde_json::json!({
        "$schema": "https://opencode.ai/config.json",
        "provider": {
            "aether1-local": {
                "npm": "@ai-sdk/openai-compatible",
                "name": "On this computer",
                "options": { "baseURL": openai },
                "models": { model: { "name": model } }
            }
        }
    });
    let dir = state.join("config").join(ENGINE);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("{} could not be made: {e}", dir.display()))?;
    let path = dir.join(format!("{ENGINE}.json"));
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&config).unwrap_or_default(),
    )
    .map_err(|e| format!("{} could not be written: {e}", path.display()))
}

/// Runs one task through the engine, inside the sandbox, and hands back what it said.
///
/// Everything this needs is something another module already owns: the box from
/// `code_sandbox`, the model from `code_model`, what the project is trusted with from
/// `code_policy`, the way out from `code_proxy`. What is here is the assembly and the
/// refusals -- and each refusal says what to do next, because "it did not work" about a thing
/// with this many moving parts is no use to anybody.
///
/// `session` continues an earlier conversation when given. The engine keeps those in its own
/// database, which is in the directory this module exists to preserve, so a session id stays
/// meaningful between runs.
pub fn run(
    db: &MemoryDb,
    model: &str,
    task: &str,
    session: Option<&str>,
    timeout: std::time::Duration,
) -> Result<Done, String> {
    let task = task.trim();
    if task.is_empty() {
        return Err("there is no task to give the engine".to_string());
    }
    let root = crate::code_workspace::root(db)?;

    let sandbox = crate::code_sandbox::detect();
    if !sandbox.confines() {
        return Err(crate::code_sandbox::unconfined_refusal(&sandbox));
    }

    let Some(engine) = crate::paths::find_installed_binary(&[ENGINE]) else {
        return Err(format!(
            "{ENGINE} is not installed, so there is no engine to run. `npm install -g \
             {ENGINE}-ai` puts it on this machine, and the coding panel under The Brain has \
             the same step with a button."
        ));
    };

    // The brain first, because without it the rest is pointless: an engine in a box with no
    // model will start, think for a moment and fail with something less clear than this.
    let brain = crate::code_model::start(db)?;
    let state = state_dir(db, &root)?;
    write_config(&state, &brain.endpoint.in_box(), model)?;

    let mut args = vec!["run".to_string()];
    if let Some(session) = session.map(str::trim).filter(|s| !s.is_empty()) {
        args.push("--session".to_string());
        args.push(session.to_string());
    }
    args.push("--model".to_string());
    args.push(format!("aether1-local/{model}"));
    // Everything after this is the operator's text, passed as one argument. `command` spawns
    // without a shell, so nothing in it is ever parsed as syntax.
    args.push(task.to_string());

    let level = crate::code_policy::level(&root);
    let mut child = crate::code_sandbox::command(
        &sandbox,
        &engine.to_string_lossy(),
        &args,
        &root,
        &root,
        &crate::code_sandbox::Bridges {
            net: crate::code_sandbox::network_for(&sandbox, db),
            model: Some(brain.socket.clone()),
        },
        &crate::code_sandbox::Access {
            write_workspace: level.writes_the_project(),
            extra: crate::code_policy::mounts(&root),
            engine_state: Some(state),
        },
    )
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .spawn()
    .map_err(|e| format!("{ENGINE} could not be started: {e}"))?;

    // Polled rather than waited on, the same way `code_workspace::run` does it, so an engine
    // that never returns is killed rather than holding everything open until the operator
    // gives up on it.
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => return Err(format!("{ENGINE} could not be waited on: {e}")),
        }
    };
    let out = child
        .wait_with_output()
        .map_err(|e| format!("{ENGINE}'s output could not be read: {e}"))?;
    let output =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);

    match status {
        Some(status) => Ok(Done {
            status: status.code().unwrap_or(1),
            output,
        }),
        None => Err(format!(
            "{ENGINE} was still going after {} seconds and was stopped. What it had said:\n\n{}",
            timeout.as_secs(),
            output.trim()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> MemoryDb {
        let path = std::env::temp_dir().join(format!(
            "aether1_code_engine_{}_{:?}.db",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&path);
        MemoryDb::open(&path).expect("open test db")
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aether1_engine_{name}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    /// The four roots exist before anything is told to write into them, because a tool
    /// handed `XDG_DATA_HOME` pointing at nothing does not always make it.
    #[test]
    fn the_four_roots_are_made_before_the_engine_looks_for_them() {
        let db = db();
        let root = scratch("roots");
        let dir = state_dir(&db, &root).expect("state dir");
        for leaf in ["config", "data", "state", "cache"] {
            assert!(dir.join(leaf).is_dir(), "{leaf} should exist in {dir:?}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The same project twice is the same directory -- which is the entire point, since a key
    /// that moved would be a history that silently started again.
    #[test]
    fn the_same_project_gets_the_same_directory_every_time() {
        let db = db();
        let root = scratch("stable");
        assert_eq!(
            state_dir(&db, &root).expect("first"),
            state_dir(&db, &root).expect("second")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Reached by a different path, still the same project. Without canonicalising, a
    /// workspace nominated through a symlink would get a second, empty history and the
    /// operator would see an agent that had forgotten everything for no visible reason.
    #[test]
    fn the_same_project_reached_another_way_is_not_a_second_project() {
        let db = db();
        let root = scratch("symlink");
        let direct = state_dir(&db, &root).expect("direct");

        let link = std::env::temp_dir().join(format!(
            "aether1_engine_link_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&link);
        #[cfg(unix)]
        {
            if std::os::unix::fs::symlink(&root, &link).is_ok() {
                assert_eq!(direct, state_dir(&db, &link).expect("through the link"));
                let _ = std::fs::remove_file(&link);
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Two projects with the same folder name are two projects. The readable half of the key
    /// is a label, and a label is not an identity.
    #[test]
    fn two_projects_called_the_same_thing_are_still_two_projects() {
        let db = db();
        let one = scratch("same/api");
        let two = scratch("other/api");
        std::fs::create_dir_all(one.join("api")).expect("one");
        std::fs::create_dir_all(two.join("api")).expect("two");
        assert_ne!(
            state_dir(&db, &one.join("api")).expect("one"),
            state_dir(&db, &two.join("api")).expect("two")
        );
        let _ = std::fs::remove_dir_all(&one);
        let _ = std::fs::remove_dir_all(&two);
    }

    /// A name with a path separator or a space in it must not be able to climb out of the
    /// engine directory or split it in two.
    #[test]
    fn an_awkward_folder_name_cannot_escape_the_directory() {
        for name in ["..", "a/b", "a b", "...", ".", "-"] {
            let key = key_for(Path::new(name));
            assert!(!key.contains('/'), "{name:?} produced {key:?}");
            assert!(!key.contains(".."), "{name:?} produced {key:?}");
            assert!(!key.is_empty(), "{name:?} produced nothing");
        }
    }

    /// The variables name the box's paths, never this machine's. A host path leaking into the
    /// environment would tell a program inside the box where its directory really is, which
    /// is the one thing the bind mount exists to hide.
    #[test]
    fn the_variables_describe_the_box_and_not_this_machine() {
        let env = env_in_sandbox("/home/someone");
        assert_eq!(env.len(), 4);
        for (key, value) in &env {
            assert!(
                value.starts_with(&format!("/home/someone/{DIR_NAME}/")),
                "{key} should be inside the box's own directory: {value}"
            );
        }
        let names: Vec<&str> = env.iter().map(|(k, _)| *k).collect();
        assert_eq!(
            names,
            [
                "XDG_CONFIG_HOME",
                "XDG_DATA_HOME",
                "XDG_STATE_HOME",
                "XDG_CACHE_HOME"
            ]
        );
    }
    /// The claim, through the real `code_sandbox::command` rather than against a description
    /// of it: a command in a box writes into `XDG_DATA_HOME`, a second box is built, and what
    /// the first one wrote is still there. The same two runs without the directory show the
    /// tmpfs doing what it is supposed to do, so the test fails if the bind ever stops
    /// mattering rather than passing for the wrong reason.
    ///
    /// Skips itself where it cannot run honestly: no bubblewrap that can cut the network, no
    /// python3, or a `$HOME` this process does not have.
    #[cfg(unix)]
    #[test]
    fn what_the_engine_writes_in_one_box_is_there_in_the_next() {
        let sandbox = crate::code_sandbox::detect();
        if !sandbox.confines() || std::env::var("HOME").unwrap_or_default().is_empty() {
            return;
        }
        let Some(python) = crate::paths::find_installed_binary(&["python3"]) else {
            return;
        };

        let db = db();
        let root = scratch("survives");
        let state = state_dir(&db, &root).expect("state dir");

        // Writes if it is not there, reports what it found either way -- so one script serves
        // as both halves of the test and the second run cannot accidentally be the first.
        let script = "import os,pathlib\n\
                      p=pathlib.Path(os.environ.get('XDG_DATA_HOME','/nowhere'))/'note'\n\
                      print('SAW'+'-'+(p.read_text() if p.exists() else 'nothing'))\n\
                      p.parent.mkdir(parents=True,exist_ok=True)\n\
                      p.write_text('remembered')\n";

        let run = |engine_state: Option<PathBuf>| -> String {
            let out = crate::code_sandbox::command(
                &sandbox,
                &python.to_string_lossy(),
                &["-c".to_string(), script.to_string()],
                &root,
                &root,
                &crate::code_sandbox::Bridges::default(),
                &crate::code_sandbox::Access {
                    write_workspace: true,
                    extra: Vec::new(),
                    engine_state,
                },
            )
            .output()
            .expect("the sandbox runs");
            String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr)
        };

        let first = run(Some(state.clone()));
        assert!(
            first.contains("SAW-nothing"),
            "the first box should start empty: {first}"
        );
        let second = run(Some(state.clone()));
        assert!(
            second.contains("SAW-remembered"),
            "and the second should find what the first wrote: {second}"
        );

        // The other half of the rule: without the directory the box forgets, which is what
        // every command the operator starts still does.
        let without = run(None);
        assert!(
            without.contains("SAW-nothing"),
            "a box with no engine directory keeps nothing: {without}"
        );

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&state);
    }

    /// The configuration points the engine at the box's own address for the model, never at
    /// this machine's -- a base URL naming the host would be a path out of the box that the
    /// box cannot provide, and the engine would report the model as unreachable.
    #[test]
    fn the_configuration_names_the_address_the_box_can_use() {
        let root = scratch("config");
        write_config(&root, "http://127.0.0.1:8081", "qwen2.5-coder:7b").expect("written");
        let written =
            std::fs::read_to_string(root.join("config").join(ENGINE).join("opencode.json"))
                .expect("config file");
        assert!(
            written.contains("http://127.0.0.1:8081/v1"),
            "the OpenAI-shaped half of the box's address: {written}"
        );
        assert!(written.contains("qwen2.5-coder:7b"), "{written}");

        // An endpoint that already carries the version must not become `/v1/v1`, which is the
        // mistake `code_setup::agents` normalises for the same two agents.
        write_config(&root, "http://127.0.0.1:8081/v1", "m").expect("written");
        let written =
            std::fs::read_to_string(root.join("config").join(ENGINE).join("opencode.json"))
                .expect("config file");
        assert!(!written.contains("/v1/v1"), "{written}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A task of nothing but spaces is not a task. Worth its own line because the engine would
    /// otherwise be started, think about an empty prompt, and bill the operator's machine for
    /// it.
    #[test]
    fn an_empty_task_is_refused_before_anything_starts() {
        let db = db();
        let why = run(&db, "m", "   ", None, std::time::Duration::from_secs(1))
            .expect_err("nothing to do");
        assert!(why.contains("no task"), "{why}");
    }
}
