// Graft integration for code graph analysis.
//
// Graft (https://github.com/nanonets/graft) builds a context graph of a repository --
// a wiring graph of symbols, their call edges, and per-file cards -- and answers
// questions against it with exact `file:line` pointers. This module detects projects,
// builds the graph, and injects the nodes relevant to a question into the model's
// system prompt.
//
// It is installed with `npm install -g @nanonets/graft`. Everything here talks to that
// CLI and nothing else: no graph format is parsed by hand, because `graft ask` already
// ranks and returns exactly what the prompt wants, and it refreshes the graph for the
// files that changed before answering -- so the graph stays in sync without this module
// guessing when the code moved.

use crate::llm::MemoryDb;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Path setting for the currently selected project
const SELECTED_PROJECT_SETTING: &str = "graft_selected_project";

/// Whether `graft ask` may re-index changed files before answering. On by default:
/// a refresh of one changed file is about a second, and a stale graph points the
/// model at line numbers that have moved.
const AUTO_REFRESH_SETTING: &str = "graft_auto_refresh";

/// Where Graft keeps the graph: `<repo>/graft`, a regenerable local cache it
/// git-ignores for you, like `node_modules`.
const GRAFT_DIR: &str = "graft";

/// The index Graft writes at the top of that folder. Its presence is what separates
/// a built graph from a folder that happens to be called `graft`.
const GRAFT_INDEX: &str = "INDEX.md";

/// How long a question may wait on the graph before the turn goes ahead without it.
/// An answer from a warm graph is under a second; this is the budget for the
/// incremental re-index that a question after a large edit pays for.
const ASK_TIMEOUT: Duration = Duration::from_secs(20);

/// How long `graft build` may run before it is killed. A first build of a large
/// repository parses every file, so this is generous; it exists only so a wedged
/// build cannot hold the Settings button down forever.
const BUILD_TIMEOUT: Duration = Duration::from_secs(900);

/// How many ranked nodes to put in the prompt.
const ASK_RESULTS: usize = 5;

/// The question handed to `graft ask` is cut to this many characters. Ranking is
/// lexical, so a whole conversation turn's worth of text dilutes it rather than
/// sharpening it.
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

/// Check if a project has a built Graft graph.
///
/// `Error` is reserved for the one case worth distinguishing: a `graft/` folder that
/// exists but has no index in it, which is what a build that died halfway leaves
/// behind. A project nobody has built yet is `NotBuilt`, not an error.
fn check_graft_status(project_path: &Path) -> GraftStatus {
    let graft_dir = project_path.join(GRAFT_DIR);
    if !graft_dir.is_dir() {
        return GraftStatus::NotBuilt;
    }
    if graft_dir.join(GRAFT_INDEX).is_file() {
        GraftStatus::Ready
    } else {
        GraftStatus::Error
    }
}

/// Where the Graft binary is, if it can be found.
///
/// `graft` is an npm global, and a desktop launcher does not start the app from the
/// operator's shell, so the directory npm installs into is routinely missing from PATH
/// even though `graft` runs fine in a terminal -- nvm is the common case, since its bin
/// directory is added by a shell init file that a launcher never sources. PATH is still
/// asked first, through the same lookup as every other binary Aether1 shells out to;
/// npm's own prefixes are the fallback, and `GRAFT_BIN` overrides both for an install
/// somewhere unusual.
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
    // Named rather than given up on: the error from a failed spawn says how to install
    // it, which is more use than a path that was guessed at.
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
        // nvm keeps one bin directory per installed Node version. Every one of them is
        // looked at and the last match wins, so an operator with several versions gets
        // the newest install rather than whichever the filesystem happened to list first.
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

/// Start a Graft command in a project folder, with no terminal of its own and no
/// stdin: nothing here is interactive, and a prompt nobody can see would hang.
fn graft_command(project_path: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(graft_program());
    command
        .args(args)
        .current_dir(project_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::paths::suppress_console_window(&mut command);
    command
}

/// Run a Graft command, killing it if it outstays `timeout`.
///
/// Polled rather than waited on, so a build that never returns is killed instead of
/// holding the Tauri command open until the operator gives up on the window.
fn run_graft(
    project_path: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let mut child = graft_command(project_path, args).spawn().map_err(|e| {
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
    // Run from the current directory: a version check does not belong to any project,
    // and `--version` reads nothing from the folder it starts in.
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

/// One ranked node, as `graft ask --json` reports it.
#[derive(Debug, Deserialize)]
struct AskHit {
    #[serde(default)]
    title: String,
    #[serde(default)]
    pointer: String,
    #[serde(default)]
    snippet: String,
}

#[derive(Debug, Default, Deserialize)]
struct AskResult {
    #[serde(default)]
    hits: Vec<AskHit>,
}

/// Cut a conversation turn down to something a lexical ranker can use.
fn ask_query(query: &str) -> String {
    let collapsed = query.split_whitespace().collect::<Vec<_>>().join(" ");
    match collapsed.char_indices().nth(QUERY_CHARS) {
        Some((byte, _)) => collapsed[..byte].to_string(),
        None => collapsed,
    }
}

/// Ask the graph which nodes bear on a question.
fn ask(project_path: &Path, query: &str, limit: usize, refresh: bool) -> Vec<AskHit> {
    let query = ask_query(query);
    if query.is_empty() {
        return Vec::new();
    }
    let limit = limit.to_string();
    let mut args = vec!["ask", "--json", "-n", limit.as_str()];
    if !refresh {
        args.push("--no-refresh");
    }
    args.push(query.as_str());

    let output = match run_graft(project_path, &args, ASK_TIMEOUT) {
        Ok(output) => output,
        Err(e) => {
            // A question the graph cannot answer is not a failed turn: the model is
            // perfectly able to read the code itself, so this is noted and dropped.
            eprintln!("[AETHER1] Graft could not answer: {e}");
            return Vec::new();
        }
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("[AETHER1] Graft could not answer: {}", stderr.trim());
        return Vec::new();
    }

    parse_ask(&String::from_utf8_lossy(&output.stdout))
}

/// Pull the ranked nodes out of what `graft ask --json` printed.
///
/// It prints a progress line ahead of the JSON when it refreshes the graph first, so the
/// document is taken from the first `{` rather than from the start of stdout.
fn parse_ask(stdout: &str) -> Vec<AskHit> {
    let Some(start) = stdout.find('{') else {
        return Vec::new();
    };
    match serde_json::from_str::<AskResult>(&stdout[start..]) {
        Ok(result) => result.hits,
        Err(e) => {
            eprintln!("[AETHER1] Graft's answer could not be read: {e}");
            Vec::new()
        }
    }
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
    let hits = ask(&selected, query, ASK_RESULTS, refresh);
    if hits.is_empty() {
        return String::new();
    }

    let nodes_text = hits
        .iter()
        .enumerate()
        .map(|(i, hit)| {
            let mut line = format!("  {}. {}", i + 1, hit.title.trim());
            if !hit.pointer.trim().is_empty() {
                line.push_str(&format!(" -- {}", hit.pointer.trim()));
            }
            if !hit.snippet.trim().is_empty() {
                line.push_str(&format!("\n     {}", hit.snippet.trim()));
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "\n[CODE ANALYSIS: {}]\n\
         The following code nodes from the project graph are relevant to this question:\n\
         {}\n\
         Each is given as file:line in that project. Use these as entry points, and read \
         the files themselves before relying on anything not shown here.\n",
        selected.display(),
        nodes_text
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_with_no_graft_folder_has_not_been_built() {
        let dir = std::env::temp_dir().join(format!("graft_not_built_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        assert_eq!(check_graft_status(&dir), GraftStatus::NotBuilt);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_graft_folder_with_an_index_is_ready() {
        let dir = std::env::temp_dir().join(format!("graft_ready_{}", std::process::id()));
        let graph = dir.join(GRAFT_DIR);
        std::fs::create_dir_all(&graph).expect("temp dir");
        std::fs::write(graph.join(GRAFT_INDEX), "# index").expect("index");
        assert_eq!(check_graft_status(&dir), GraftStatus::Ready);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // A build that died halfway leaves the folder without its index. That is worth
    // telling apart from "nobody has built this yet", because the fix is different:
    // one needs a build, the other needs the half-written folder cleared.
    #[test]
    fn a_graft_folder_with_no_index_is_an_error() {
        let dir = std::env::temp_dir().join(format!("graft_half_{}", std::process::id()));
        std::fs::create_dir_all(dir.join(GRAFT_DIR)).expect("temp dir");
        assert_eq!(check_graft_status(&dir), GraftStatus::Error);
        let _ = std::fs::remove_dir_all(&dir);
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

    // Copied from `graft ask --json` 0.21.1 run on this repository, progress line and
    // all: that line is printed to stdout ahead of the document whenever the graph is
    // refreshed first, so parsing from byte zero would fail on exactly the common case.
    const REAL_ASK_OUTPUT: &str = r#"[graft] refreshed the graph (1 file changed) before answering
{
  "query": "voice overlap",
  "mode": "lexical",
  "hits": [
    {
      "kind": "symbol",
      "title": "colsOverlap \u00b7 function",
      "pointer": "frontend/js/layout.js:L677-L679",
      "snippet": "function colsOverlap(a, b)",
      "score": 1.2845510223512564
    },
    {
      "kind": "symbol",
      "title": "VoiceAudioEngine \u00b7 class",
      "pointer": "frontend/js/voice.js:L20-L632",
      "snippet": "class VoiceAudioEngine",
      "score": 0.8694274961723312
    }
  ],
  "coverage": 0.7197058449837305
}
"#;

    #[test]
    fn the_progress_line_does_not_stop_the_answer_being_read() {
        let hits = parse_ask(REAL_ASK_OUTPUT);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].pointer, "frontend/js/layout.js:L677-L679");
        assert_eq!(hits[1].snippet, "class VoiceAudioEngine");
    }

    #[test]
    fn an_answer_that_is_not_json_is_no_nodes_rather_than_a_failed_turn() {
        assert!(parse_ask("graft: no graph here\n").is_empty());
        assert!(parse_ask("{ not json at all").is_empty());
    }
}
