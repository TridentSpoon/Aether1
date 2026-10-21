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

/// What an operator with no `HOME` is told, and why.
///
/// Its own function so it can be read in a test without a test having to unset `HOME` on the
/// whole process. That matters more than it looks: `std::env::set_var` is process-global and
/// the suite runs in parallel, so a test that blanks `HOME` for even a moment can be the
/// reason a sandbox test in another module sees a box built without one. One flaky
/// cross-module failure costs more than this function saves.
fn no_home_refusal() -> String {
    format!(
        "{ENGINE} keeps its sessions in a directory mounted inside the sandbox's own home \
         folder, and this process has no HOME -- so there would be nowhere to put them and \
         every task would start again from nothing. Start AETHER1 from a session that has \
         HOME set, and `--session` will mean something between runs."
    )
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
    said: crate::llm::providers::Sink,
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

    // The engine's directory is mounted at `$HOME/aether1-engine` and the four `XDG_*`
    // variables are built from that same `$HOME`, so a process without one has nowhere to
    // put either. `code_sandbox` is right to bind nothing and set nothing in that case --
    // an `XDG_DATA_HOME` pointing into the tmpfs would be worse than none. The consequence
    // is what this refuses: the engine would run with its state in the `$HOME` tmpfs, lose
    // every session the moment the command returned, and say nothing about it. "The agent
    // has forgotten what it was doing", with nothing in the output to explain it, is the
    // worst shape this bug has, so it is answered here rather than left to be discovered.
    //
    // Asked next to the sandbox question because it is the same kind of question -- what
    // this machine can do -- and before the engine is looked for, so an operator missing
    // both hears about the one that is theirs to fix either way.
    if std::env::var("HOME").unwrap_or_default().is_empty() {
        return Err(no_home_refusal());
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
    let child = crate::code_sandbox::command(
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

    collect(child, timeout, said)
}

/// Reads one running engine to its end, handing every line to `said` as it arrives, and
/// stops it if it overruns.
///
/// Split out from [`run`] so the waiting can be tested without an engine installed: the
/// streaming and the watchdog are the parts with the timing in them, and they behave the
/// same whether the child is opencode or `/bin/sh`.
fn collect(
    mut child: std::process::Child,
    timeout: std::time::Duration,
    said: crate::llm::providers::Sink,
) -> Result<Done, String> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("{ENGINE} was started without a way to read its output"))?;
    let child = std::sync::Arc::new(std::sync::Mutex::new(child));
    let overran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    let watchdog = {
        let child = std::sync::Arc::clone(&child);
        let overran = std::sync::Arc::clone(&overran);
        std::thread::Builder::new()
            .name("aether1-engine-timeout".to_string())
            .spawn(move || {
                let started = std::time::Instant::now();
                loop {
                    // Short naps rather than one long one, so a run that finishes early
                    // takes this thread with it instead of holding a handle for the whole
                    // timeout.
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    let Ok(mut child) = child.lock() else { return };
                    match child.try_wait() {
                        Ok(Some(_)) => return,
                        Ok(None) => {}
                        Err(_) => return,
                    }
                    if started.elapsed() >= timeout {
                        overran.store(true, std::sync::atomic::Ordering::SeqCst);
                        let _ = child.kill();
                        return;
                    }
                }
            })
            .ok()
    };

    let mut output = String::new();
    {
        use std::io::BufRead;
        let mut lines = std::io::BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            // Bytes rather than `lines()`, because an engine that writes a progress line
            // without a newline should still reach the operator, and a lossy conversion here
            // is better than dropping the run over one bad byte.
            match lines.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    output.push_str(&line);
                    said(&line);
                }
                Err(_) => break,
            }
        }
    }

    // Whatever it said on the way out, after the stream has ended: the engine's own
    // diagnostics land here, and a refusal with an empty body is no use to anybody.
    let mut status = None;
    let mut stderr = String::new();
    if let Ok(mut child) = child.lock() {
        if let Some(mut pipe) = child.stderr.take() {
            use std::io::Read;
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            stderr = String::from_utf8_lossy(&buf).to_string();
        }
        status = child.wait().ok();
    }
    if let Some(watchdog) = watchdog {
        let _ = watchdog.join();
    }
    if !stderr.is_empty() {
        said(&stderr);
        output.push_str(&stderr);
    }

    if overran.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(format!(
            "{ENGINE} was still going after {} seconds and was stopped. What it had said:\n\n{}",
            timeout.as_secs(),
            output.trim()
        ));
    }
    match status {
        Some(status) => Ok(Done {
            status: status.code().unwrap_or(1),
            output,
        }),
        None => Err(format!(
            "{ENGINE} could not be waited on. What it said:\n\n{output}"
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

    /// A process with no `HOME` is refused rather than run with its memory in a tmpfs, and
    /// the refusal says both what is missing and what it costs.
    ///
    /// Without that check the whole of the engine's state directory came apart silently on
    /// exactly the setups most likely to hit it -- a service unit, a container, a bare
    /// `env -i` -- because `code_sandbox` binds nothing and sets no `XDG_*` when `HOME` is
    /// empty, and an engine told nothing starts again from nothing on every task.
    ///
    /// Asserted against the message rather than by unsetting `HOME` and calling `run`:
    /// `set_var` is process-global, the suite runs in parallel, and a test that blanks `HOME`
    /// for a moment turns into a sandbox test failing in another module. That is not a
    /// trade worth making for one call site.
    #[test]
    fn with_no_home_the_refusal_says_what_is_missing_and_what_it_costs() {
        let why = no_home_refusal();
        assert!(why.contains("HOME"), "name what is missing: {why}");
        assert!(
            why.contains("start again from nothing"),
            "and what it costs, which is the sessions: {why}"
        );
    }

    /// Spawns `/bin/sh -c <script>` the way `run` spawns the engine, so `collect` is tested
    /// against a real child with real pipes rather than a stand-in for one.
    #[cfg(unix)]
    fn sh(script: &str) -> std::process::Child {
        std::process::Command::new("/bin/sh")
            .args(["-c", script])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("sh runs")
    }

    /// The point of the streaming: each line reaches the operator as it is written, not in
    /// one lump at the end. A panel waiting a quarter of an hour for a coding agent cannot
    /// tell a long edit from a hung one, and this is what tells it.
    #[cfg(unix)]
    #[test]
    fn what_the_engine_says_arrives_line_by_line_and_in_full() {
        let mut seen: Vec<String> = Vec::new();
        let done = collect(
            sh("echo first; echo second; echo third"),
            std::time::Duration::from_secs(30),
            &mut |said| seen.push(said.to_string()),
        )
        .expect("it runs");

        assert_eq!(done.status, 0);
        assert_eq!(done.output, "first\nsecond\nthird\n");
        // Three deliveries rather than one, which is the difference between streaming and
        // collecting: the same text arriving at the end would pass an output assertion.
        assert_eq!(
            seen,
            ["first\n", "second\n", "third\n"],
            "each line should have been handed over as it arrived"
        );
    }

    /// An engine that has stopped making progress is stopped, and what it managed to say
    /// first comes back with the refusal -- the useful half of a run that overran.
    #[cfg(unix)]
    #[test]
    fn an_engine_that_overruns_is_stopped_and_still_reports_what_it_said() {
        let why = collect(
            sh("echo starting; sleep 30"),
            std::time::Duration::from_millis(300),
            &mut |_| {},
        )
        .expect_err("it should be stopped");

        assert!(
            why.contains("still going"),
            "the refusal should say what happened: {why}"
        );
        assert!(
            why.contains("starting"),
            "and carry what it had said before it was stopped: {why}"
        );
    }

    /// A non-zero exit is not an error to throw the output away for. An engine that worked
    /// and then failed has said the most useful thing on the screen, so the status is
    /// carried beside the text rather than replacing it.
    #[cfg(unix)]
    #[test]
    fn a_failing_engine_still_hands_back_what_it_said() {
        let done = collect(
            sh("echo tried this; exit 3"),
            std::time::Duration::from_secs(30),
            &mut |_| {},
        )
        .expect("a non-zero exit is still an answer");
        assert_eq!(done.status, 3);
        assert!(done.output.contains("tried this"), "{}", done.output);
    }

    /// Whatever it said on the way out is part of the answer. The engine reports why it
    /// could not do something on stderr, and a panel showing only stdout would show the
    /// operator a blank bubble for the one run they most need to read.
    #[cfg(unix)]
    #[test]
    fn what_the_engine_said_on_the_way_out_is_not_dropped() {
        let mut seen = String::new();
        let done = collect(
            sh("echo out; echo trouble 1>&2"),
            std::time::Duration::from_secs(30),
            &mut |said| seen.push_str(said),
        )
        .expect("it runs");
        assert!(done.output.contains("out"), "{}", done.output);
        assert!(done.output.contains("trouble"), "{}", done.output);
        assert!(
            seen.contains("trouble"),
            "and it reached the panel too: {seen}"
        );
    }

    /// A task of nothing but spaces is not a task. Worth its own line because the engine would
    /// otherwise be started, think about an empty prompt, and bill the operator's machine for
    /// it.
    #[test]
    fn an_empty_task_is_refused_before_anything_starts() {
        let db = db();
        let why = run(
            &db,
            "m",
            "   ",
            None,
            std::time::Duration::from_secs(1),
            &mut |_| {},
        )
        .expect_err("nothing to do");
        assert!(why.contains("no task"), "{why}");
    }
}
