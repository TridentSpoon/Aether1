// Graft integration for code graph analysis.
//
// Graft (https://github.com/nanonets/graft) is a code analysis tool that builds
// a graph of dependencies and relationships in a codebase. This module provides
// integration to auto-detect projects, build code graphs, and inject relevant
// code context into the LLM's system prompt.
//
// It is installed with `npm install -g @nanonets/graft`. The graph stays in sync without
// this module guessing when the code moved: `graft ask` re-indexes the files that changed
// before it answers, which is about a second for one file, so there is no rebuild schedule
// here. `graft build` is the first build, behind the Settings button, which is the long one.

use crate::llm::MemoryDb;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Path setting for the currently selected project
const SELECTED_PROJECT_SETTING: &str = "graft_selected_project";

/// Whether `graft ask` may re-index changed files before answering. On by default: the
/// refresh costs about a second, and a stale graph points the model at line numbers that
/// have moved.
const AUTO_REFRESH_SETTING: &str = "graft_auto_refresh";

/// Where Graft keeps the graph, and the index it writes at the top of it.
const GRAFT_DIR: &str = "graft";
const GRAFT_INDEX: &str = "INDEX.md";

/// How long a question may wait on the graph before the turn goes ahead without it. An
/// answer from a warm graph is under a second; this is the budget for the incremental
/// re-index that a question after a large edit pays for.
const ASK_TIMEOUT: Duration = Duration::from_secs(20);

/// How long `graft build` may run before it is killed. A first build parses every file, so
/// this is generous; it exists only so a wedged build cannot hold the Settings button down
/// for the rest of the session.
const BUILD_TIMEOUT: Duration = Duration::from_secs(900);

/// The question handed to `graft ask` is cut to this many characters. Ranking is lexical,
/// so a whole conversation turn's worth of text dilutes it rather than sharpening it.
const QUERY_CHARS: usize = 240;

/// Stores information about a detected project
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub path: PathBuf,
    pub name: String,
    pub graft_status: GraftStatus,
}

/// Status of a project's Graft graph
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum GraftStatus {
    /// Graph has not been built yet
    NotBuilt,
    /// Graph exists and is ready to use
    Ready,
    /// Error loading or building the graph
    Error,
}

impl std::fmt::Display for GraftStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GraftStatus::NotBuilt => write!(f, "Not built"),
            GraftStatus::Ready => write!(f, "Ready"),
            GraftStatus::Error => write!(f, "Error"),
        }
    }
}

/// Auto-detect repositories in common project folder locations
pub fn detect_projects() -> Vec<Project> {
    let mut projects = Vec::new();

    // Common locations to search for projects
    let search_dirs = [
        crate::paths::home_dir(),
        crate::paths::home_dir().map(|h| h.join("Projects")),
        crate::paths::home_dir().map(|h| h.join("projects")),
        crate::paths::home_dir().map(|h| h.join("workspace")),
        crate::paths::home_dir().map(|h| h.join("code")),
        crate::paths::home_dir().map(|h| h.join("src")),
        crate::paths::home_dir().map(|h| h.join("dev")),
    ];

    for dir in search_dirs.into_iter().flatten() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // Check if it's a git repository
                    if path.join(".git").exists() {
                        if let Some(name) = path.file_name() {
                            let name_string = name.to_string_lossy().to_string();
                            let graft_status = check_graft_status(&path);
                            projects.push(Project {
                                path,
                                name: name_string,
                                graft_status,
                            });
                        }
                    }
                }
            }
        }
    }

    // Remove duplicates (same path)
    projects.sort_by(|a, b| a.path.cmp(&b.path));
    projects.dedup_by(|a, b| a.path == b.path);

    projects
}

/// Check if a project has a built Graft graph
fn check_graft_status(project_path: &Path) -> GraftStatus {
    let graft_dir = project_path.join(GRAFT_DIR);
    if graft_dir.exists() {
        if graft_dir.join(GRAFT_INDEX).is_file() {
            GraftStatus::Ready
        } else {
            GraftStatus::Error
        }
    } else {
        GraftStatus::NotBuilt
    }
}

/// Where the Graft binary is, if it can be found.
///
/// `graft` is an npm global, and a desktop launcher does not start the app from the
/// operator's shell, so the directory npm installs into is routinely missing from PATH even
/// though `graft` runs fine in a terminal -- nvm most of all, since what adds its bin
/// directory is a shell init file a launcher never sources. PATH is still asked first,
/// through the same lookup as every other binary Aether1 shells out to; npm's own prefixes
/// are the fallback, and `GRAFT_BIN` overrides both for an install somewhere unusual.
fn graft_program() -> PathBuf {
    if let Some(from_env) = std::env::var_os("GRAFT_BIN") {
        let path = PathBuf::from(from_env);
        if path.is_file() {
            return path;
        }
    }
    if let Some(found) = crate::paths::find_installed_binary(&["graft"]) {
        return found;
    }
    if let Some(found) = npm_global_graft() {
        return found;
    }
    // Named rather than given up on: the error from a failed spawn says how to install it,
    // which is more use to whoever reads the log than a path that was guessed at.
    PathBuf::from("graft")
}

/// Look for `graft` where `npm install -g` leaves it.
fn npm_global_graft() -> Option<PathBuf> {
    let home = crate::paths::home_dir();
    let names: &[&str] = if cfg!(target_os = "windows") {
        &["graft.cmd", "graft.exe"]
    } else {
        &["graft"]
    };

    let mut dirs: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "windows") {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            dirs.push(PathBuf::from(appdata).join("npm"));
        }
    } else {
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
    }
    if let Some(home) = home.as_ref() {
        dirs.push(home.join(".npm-global").join("bin"));
        dirs.push(home.join(".npm-packages").join("bin"));
        // nvm keeps one bin directory per installed Node version. All of them are looked at
        // and the last match wins, so an operator with several versions gets the newest
        // install rather than whichever the filesystem happened to list first.
        if let Ok(entries) = std::fs::read_dir(home.join(".nvm").join("versions").join("node")) {
            let mut versions: Vec<PathBuf> = entries
                .flatten()
                .map(|entry| entry.path().join("bin"))
                .collect();
            versions.sort();
            dirs.extend(versions);
        }
    }

    dirs.into_iter()
        .flat_map(|dir| names.iter().map(|name| dir.join(name)).collect::<Vec<_>>())
        .rfind(|candidate| candidate.is_file())
}

/// Run a Graft command in a project folder, killing it if it outstays `timeout`.
///
/// Polled rather than waited on, the same way `code_workspace::run` does it: a subprocess
/// that never returns would otherwise hold a Tauri command open until the operator gives up
/// on the window. No stdin, and no console of its own on Windows -- nothing here is
/// interactive, and a prompt nobody can see would hang.
fn run_graft(
    project_path: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let mut command = Command::new(graft_program());
    command
        .args(args)
        .current_dir(project_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::paths::suppress_console_window(&mut command);

    let mut child = command.spawn().map_err(|e| {
        format!(
            "Graft could not be started: {e}. Install it with `npm install -g @nanonets/graft`, \
             or set GRAFT_BIN to the binary."
        )
    })?;

    let started = Instant::now();
    let timed_out = loop {
        match child.try_wait() {
            Ok(Some(_)) => break false,
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    break true;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(format!("Graft could not be waited for: {e}")),
        }
    };

    let output = child
        .wait_with_output()
        .map_err(|e| format!("Graft produced no readable output: {e}"))?;

    if timed_out {
        return Err(format!(
            "`graft {}` was killed after {} seconds without finishing.",
            args.join(" "),
            timeout.as_secs()
        ));
    }
    Ok(output)
}

/// Get the version of Graft installed on this system
pub fn get_graft_version() -> Result<String, String> {
    // Run from the current directory: a version check belongs to no project, and
    // `--version` reads nothing from the folder it starts in.
    let here = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let output = run_graft(&here, &["--version"], Duration::from_secs(30))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Graft was found but would not report its version: {}",
            stderr.trim()
        ));
    }

    let version = String::from_utf8_lossy(&output.stdout);
    Ok(version.trim().to_string())
}

/// Build the Graft graph for a project
pub fn build_graph(project_path: &Path) -> Result<(), String> {
    let output = run_graft(project_path, &["build"], BUILD_TIMEOUT)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Graft build failed: {}", stderr.trim()));
    }

    if !project_path.join(GRAFT_DIR).join(GRAFT_INDEX).is_file() {
        return Err("Graft build finished but graft/INDEX.md is missing".to_string());
    }

    Ok(())
}

/// Get the currently selected project
pub fn get_selected_project(db: &MemoryDb) -> Option<PathBuf> {
    let configured = db.get_setting_string(SELECTED_PROJECT_SETTING, "");
    if configured.trim().is_empty() {
        None
    } else {
        Some(PathBuf::from(configured.trim()))
    }
}

/// Set the currently selected project
pub fn set_selected_project(db: &MemoryDb, path: &Path) -> Result<(), String> {
    let path_str = path.to_string_lossy().to_string();
    db.set_setting(
        SELECTED_PROJECT_SETTING,
        &serde_json::Value::String(path_str),
    )
    .map_err(|e| format!("Failed to save selected project: {e}"))?;
    Ok(())
}

/// Cut a conversation turn down to something a lexical ranker can use.
fn ask_query(query: &str) -> String {
    let collapsed = query.split_whitespace().collect::<Vec<_>>().join(" ");
    match collapsed.char_indices().nth(QUERY_CHARS) {
        Some((byte, _)) => collapsed[..byte].to_string(),
        None => collapsed,
    }
}

/// Ask Graft for ranked code context and include its source excerpts.
fn query_graph(project_path: &Path, query: &str, refresh: bool) -> Vec<String> {
    let query = ask_query(query);
    if query.is_empty() {
        return Vec::new();
    }
    let mut args = vec!["ask", query.as_str(), "--source", "--json"];
    if !refresh {
        args.push("--no-refresh");
    }

    let output = match run_graft(project_path, &args, ASK_TIMEOUT) {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            eprintln!(
                "[AETHER1] Graft query failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
            return Vec::new();
        }
        Err(error) => {
            // A question the graph cannot answer is not a failed turn: the model is
            // perfectly able to read the code itself, so this is noted and dropped.
            eprintln!("[AETHER1] Could not run Graft query: {error}");
            return Vec::new();
        }
    };

    parse_query(&output.stdout)
}

/// Pull the ranked nodes out of what `graft ask --json` printed.
fn parse_query(stdout: &[u8]) -> Vec<String> {
    let response: serde_json::Value = match serde_json::from_slice(stdout) {
        Ok(response) => response,
        Err(error) => {
            eprintln!("[AETHER1] Could not parse Graft query result: {error}");
            return Vec::new();
        }
    };

    response["hits"]
        .as_array()
        .into_iter()
        .flatten()
        .take(5)
        .filter_map(|hit| {
            let title = hit["title"].as_str()?;
            let pointer = hit["pointer"].as_str().unwrap_or_default();
            // `--source` inlines the code at each hit. Where it is absent -- a node Graft
            // has no span for -- the signature it always carries is the next best thing,
            // and is better than a bare title.
            let code = match hit["code"].as_str().unwrap_or_default() {
                "" => hit["snippet"].as_str().unwrap_or_default(),
                code => code,
            };
            if code.is_empty() {
                Some(format!("{title} ({pointer})"))
            } else {
                Some(format!("{title} ({pointer})\n{code}"))
            }
        })
        .collect()
}

/// The Graft graph contribution to the system prompt, injected when analyzing code.
///
/// `graft ask` re-indexes the files that changed before it answers, so this needs no
/// rebuild schedule of its own: the graph is as current as the last question.
pub fn prime(db: &MemoryDb, query: &str) -> String {
    let selected = match get_selected_project(db) {
        Some(path) => path,
        None => return String::new(),
    };
    if check_graft_status(&selected) != GraftStatus::Ready {
        return String::new();
    }

    let refresh = db.get_setting_bool(AUTO_REFRESH_SETTING, true);
    let relevant_nodes = query_graph(&selected, query, refresh);
    if relevant_nodes.is_empty() {
        return String::new();
    }

    let nodes_text = relevant_nodes
        .iter()
        .enumerate()
        .map(|(i, node)| format!("  {}. {}", i + 1, node))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "\n[CODE ANALYSIS: {}]\n\
         The following code nodes from the project graph are relevant to this question:\n\
         {}\n\
         Use these as entry points for understanding the codebase structure.\n",
        selected.display(),
        nodes_text
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graft_status_not_built() {
        let path = PathBuf::from("/tmp/test_project");
        let status = check_graft_status(&path);
        assert_eq!(status, GraftStatus::NotBuilt);
    }

    #[test]
    fn a_long_question_is_cut_and_collapsed_before_ranking() {
        let query = format!("  how   does\nvoice work {}", "x".repeat(400));
        let asked = ask_query(&query);
        assert_eq!(asked.chars().count(), QUERY_CHARS);
        assert!(asked.starts_with("how does voice work x"));
        assert!(!asked.contains('\n'));
    }

    #[test]
    fn an_empty_question_asks_nothing() {
        assert!(ask_query("   \n  ").is_empty());
    }

    // Copied from `graft ask --source --json` 0.21.1 run on this repository.
    const REAL_ASK_OUTPUT: &str = r#"{
  "query": "voice overlap",
  "mode": "lexical",
  "hits": [
    {
      "kind": "symbol",
      "title": "colsOverlap \u00b7 function",
      "pointer": "frontend/js/layout.js:L677-L679",
      "snippet": "function colsOverlap(a, b)",
      "code": "function colsOverlap(a, b) {\n  return a.start < b.end;\n}",
      "score": 1.28
    },
    {
      "kind": "symbol",
      "title": "VoiceAudioEngine \u00b7 class",
      "pointer": "frontend/js/voice.js:L20-L632",
      "snippet": "class VoiceAudioEngine",
      "score": 0.86
    }
  ]
}
"#;

    #[test]
    fn a_hit_with_no_inlined_source_falls_back_to_its_signature() {
        let nodes = parse_query(REAL_ASK_OUTPUT.as_bytes());
        assert_eq!(nodes.len(), 2);
        assert!(nodes[0].contains("frontend/js/layout.js:L677-L679"));
        assert!(nodes[0].contains("return a.start < b.end;"));
        // No `code` on this one, so the signature stands in rather than nothing at all.
        assert!(nodes[1].ends_with("class VoiceAudioEngine"));
    }

    #[test]
    fn an_answer_that_is_not_json_is_no_nodes_rather_than_a_failed_turn() {
        assert!(parse_query(b"graft: no graph here\n").is_empty());
        assert!(parse_query(b"{ not json at all").is_empty());
    }
}
