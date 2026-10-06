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

/// Path setting for the project chosen before there was more than one. Read only to carry
/// an existing choice into the tracked list; nothing writes it any more.
const SELECTED_PROJECT_SETTING: &str = "graft_selected_project";

/// The projects whose graphs are asked on a code question, as a JSON array of paths. Only
/// consulted when `graft_track_all` is off.
const TRACKED_PROJECTS_SETTING: &str = "graft_tracked_projects";

/// Track every repository that turns up, including ones made later. On by default: the
/// operator asked for every repository rather than a chosen one, and a repository that
/// appears next week is one of those.
const TRACK_ALL_SETTING: &str = "graft_track_all";

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

/// How many projects may be asked about one question. They are asked at the same time, so
/// the wait does not grow with the count, but each one is a process and a graph read -- and
/// past a handful the answer is diluted rather than improved, because the nodes that matter
/// are in one or two repositories and the rest contribute noise.
const MAX_PROJECTS_PER_QUESTION: usize = 8;

/// How many ranked nodes reach the prompt, across every project asked.
const MAX_NODES_PER_QUESTION: usize = 5;

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

/// Whether every repository that turns up is tracked, rather than a chosen few.
pub fn tracking_all(db: &MemoryDb) -> bool {
    db.get_setting_bool(TRACK_ALL_SETTING, true)
}

/// The paths the operator has explicitly tracked.
///
/// A project chosen before this setting existed is carried in, so an upgrade keeps asking
/// the graph it was asking yesterday rather than quietly going quiet.
fn tracked_paths(db: &MemoryDb) -> Vec<PathBuf> {
    let stored = db.get_setting_string(TRACKED_PROJECTS_SETTING, "");
    let mut paths: Vec<PathBuf> = serde_json::from_str::<Vec<String>>(stored.trim())
        .unwrap_or_default()
        .into_iter()
        .map(PathBuf::from)
        .collect();

    if paths.is_empty() {
        let legacy = db.get_setting_string(SELECTED_PROJECT_SETTING, "");
        if !legacy.trim().is_empty() {
            paths.push(PathBuf::from(legacy.trim()));
        }
    }
    paths
}

/// Is this project one whose graph gets asked?
pub fn is_tracked(db: &MemoryDb, project_path: &Path) -> bool {
    if tracking_all(db) {
        return true;
    }
    tracked_paths(db).iter().any(|p| p == project_path)
}

/// Start or stop tracking one project.
///
/// Turning one off while tracking everything is a decision about that project, so it also
/// turns off "track everything" and writes the rest of what was detected into the list --
/// otherwise the switch would appear to do nothing.
pub fn set_tracked(db: &MemoryDb, project_path: &Path, tracked: bool) -> Result<(), String> {
    let mut paths = if tracking_all(db) {
        detect_projects()
            .into_iter()
            .map(|project| project.path)
            .collect::<Vec<_>>()
    } else {
        tracked_paths(db)
    };

    paths.retain(|p| p != project_path);
    if tracked {
        paths.push(project_path.to_path_buf());
    }
    paths.sort();
    paths.dedup();

    store_tracked(db, &paths)?;
    if tracking_all(db) && !tracked {
        set_tracking_all(db, false)?;
    }
    Ok(())
}

/// Track everything, or go back to the list.
pub fn set_tracking_all(db: &MemoryDb, all: bool) -> Result<(), String> {
    db.set_setting(TRACK_ALL_SETTING, &serde_json::Value::Bool(all))
        .map_err(|e| format!("Failed to save the track-everything setting: {e}"))
}

fn store_tracked(db: &MemoryDb, paths: &[PathBuf]) -> Result<(), String> {
    let as_strings: Vec<String> = paths
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    let encoded = serde_json::to_string(&as_strings)
        .map_err(|e| format!("Failed to encode the tracked projects: {e}"))?;
    db.set_setting(
        TRACKED_PROJECTS_SETTING,
        &serde_json::Value::String(encoded),
    )
    .map_err(|e| format!("Failed to save the tracked projects: {e}"))?;
    Ok(())
}

/// The projects to ask about a question: tracked, built, and capped.
///
/// Ordered by path so the set asked is the same from one turn to the next, which matters
/// when the cap bites: a question that silently consulted a different five repositories
/// each time would be impossible to reason about.
fn projects_to_ask(db: &MemoryDb) -> Vec<PathBuf> {
    let candidates: Vec<PathBuf> = if tracking_all(db) {
        detect_projects()
            .into_iter()
            .map(|project| project.path)
            .collect()
    } else {
        tracked_paths(db)
    };

    let mut ready: Vec<PathBuf> = candidates
        .into_iter()
        .filter(|path| check_graft_status(path) == GraftStatus::Ready)
        .collect();
    ready.sort();
    ready.dedup();
    ready.truncate(MAX_PROJECTS_PER_QUESTION);
    ready
}

/// Cut a conversation turn down to something a lexical ranker can use.
fn ask_query(query: &str) -> String {
    let collapsed = query.split_whitespace().collect::<Vec<_>>().join(" ");
    match collapsed.char_indices().nth(QUERY_CHARS) {
        Some((byte, _)) => collapsed[..byte].to_string(),
        None => collapsed,
    }
}

/// One ranked node, with the project it came from and the score that orders it against
/// nodes from every other project.
#[derive(Debug, Clone)]
struct Hit {
    project: String,
    title: String,
    pointer: String,
    code: String,
    score: f64,
}

/// Ask Graft for ranked code context and include its source excerpts.
fn query_graph(project_path: &Path, query: &str, refresh: bool) -> Vec<Hit> {
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
                "[AETHER1] Graft query failed in {}: {}",
                project_path.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
            return Vec::new();
        }
        Err(error) => {
            // A question one graph cannot answer is not a failed turn, and with several
            // projects it is not even a failed question: the others still answer, and the
            // model can read the code itself.
            eprintln!(
                "[AETHER1] Could not run Graft query in {}: {error}",
                project_path.display()
            );
            return Vec::new();
        }
    };

    parse_query(project_label(project_path), &output.stdout)
}

/// What a project is called in the prompt: its folder name, which is what the operator
/// calls it, with the full path kept out of the model's way.
fn project_label(project_path: &Path) -> String {
    project_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| project_path.to_string_lossy().to_string())
}

/// Pull the ranked nodes out of what `graft ask --json` printed.
fn parse_query(project: String, stdout: &[u8]) -> Vec<Hit> {
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
        .take(MAX_NODES_PER_QUESTION)
        .filter_map(|hit| {
            let title = hit["title"].as_str()?;
            // `--source` inlines the code at each hit. Where it is absent -- a node Graft
            // has no span for -- the signature it always carries is the next best thing,
            // and is better than a bare title.
            let code = match hit["code"].as_str().unwrap_or_default() {
                "" => hit["snippet"].as_str().unwrap_or_default(),
                code => code,
            };
            Some(Hit {
                project: project.clone(),
                title: title.to_string(),
                pointer: hit["pointer"].as_str().unwrap_or_default().to_string(),
                code: code.to_string(),
                score: hit["score"].as_f64().unwrap_or(0.0),
            })
        })
        .collect()
}

/// Ask every tracked project at once and keep the best nodes across all of them.
///
/// At the same time rather than one after another: each ask is a process that spends its
/// time waiting on Graft, so in sequence the wait would grow with the number of
/// repositories and a question would get slower every time the operator started a new
/// project. In parallel the wait is the slowest single ask.
fn gather_hits(projects: &[PathBuf], query: &str, refresh: bool) -> Vec<Hit> {
    let mut hits: Vec<Hit> = std::thread::scope(|scope| {
        let handles: Vec<_> = projects
            .iter()
            .map(|project| scope.spawn(move || query_graph(project, query, refresh)))
            .collect();
        handles
            .into_iter()
            .filter_map(|handle| handle.join().ok())
            .flatten()
            .collect()
    });

    rank_hits(&mut hits);
    hits
}

/// Put the best nodes first, whichever project they came from.
///
/// Graft's scores are comparable between repositories because they come from the same
/// lexical ranker over the same kind of graph. Ties break on the project and then the
/// pointer so the order is stable rather than whichever thread happened to finish first --
/// the same question twice should not reorder the prompt.
fn rank_hits(hits: &mut Vec<Hit>) {
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.project.cmp(&b.project))
            .then_with(|| a.pointer.cmp(&b.pointer))
    });
    hits.truncate(MAX_NODES_PER_QUESTION);
}

/// The Graft graph contribution to the system prompt, injected when analyzing code.
///
/// `graft ask` re-indexes the files that changed before it answers, so this needs no
/// rebuild schedule of its own: the graph is as current as the last question.
pub fn prime(db: &MemoryDb, query: &str) -> String {
    let projects = projects_to_ask(db);
    if projects.is_empty() {
        return String::new();
    }

    let refresh = db.get_setting_bool(AUTO_REFRESH_SETTING, true);
    let hits = gather_hits(&projects, query, refresh);
    if hits.is_empty() {
        return String::new();
    }

    let nodes_text = hits
        .iter()
        .enumerate()
        .map(|(i, hit)| {
            // The project is named on every node, because with several repositories asked
            // at once a pointer like `src/main.rs:L20` belongs to no file in particular
            // until you know which repository it is in.
            let mut line = format!("  {}. [{}] {}", i + 1, hit.project, hit.title);
            if !hit.pointer.is_empty() {
                line.push_str(&format!(" ({})", hit.pointer));
            }
            if !hit.code.is_empty() {
                line.push_str(&format!("\n{}", hit.code));
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n");

    let scope = if projects.len() == 1 {
        project_label(&projects[0])
    } else {
        format!(
            "{} projects: {}",
            projects.len(),
            projects
                .iter()
                .map(|path| project_label(path))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

    format!(
        "\n[CODE ANALYSIS: {}]\n\
         The following code nodes from the project graphs are relevant to this question:\n\
         {}\n\
         Each node is labelled with the project it is in, and its pointer is a path inside \
         that project. Use these as entry points for understanding the codebase structure.\n",
        scope, nodes_text
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // A real temp file per test, not ":memory:" -- see the warning on MemoryDb::open.
    fn temp_db(name: &str) -> MemoryDb {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "aether1_graft_{name}_{}_{n}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        MemoryDb::open(path).expect("temp db should open")
    }

    fn temp_project(name: &str, built: bool) -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "aether1_graft_project_{name}_{}_{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp project");
        if built {
            let graph = dir.join(GRAFT_DIR);
            std::fs::create_dir_all(&graph).expect("temp graph");
            std::fs::write(graph.join(GRAFT_INDEX), "# index").expect("index");
        }
        dir
    }

    fn hit(project: &str, pointer: &str, score: f64) -> Hit {
        Hit {
            project: project.to_string(),
            title: format!("{project}::thing"),
            pointer: pointer.to_string(),
            code: String::new(),
            score,
        }
    }

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
        let hits = parse_query("Aether1".to_string(), REAL_ASK_OUTPUT.as_bytes());
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].pointer, "frontend/js/layout.js:L677-L679");
        assert!(hits[0].code.contains("return a.start < b.end;"));
        // No `code` on this one, so the signature stands in rather than nothing at all.
        assert_eq!(hits[1].code, "class VoiceAudioEngine");
        // The project rides along on every hit: a pointer alone names no file once more
        // than one repository has been asked.
        assert!(hits.iter().all(|hit| hit.project == "Aether1"));
        assert_eq!(hits[0].score, 1.28);
    }

    #[test]
    fn an_answer_that_is_not_json_is_no_nodes_rather_than_a_failed_turn() {
        assert!(parse_query("x".to_string(), b"graft: no graph here\n").is_empty());
        assert!(parse_query("x".to_string(), b"{ not json at all").is_empty());
    }

    #[test]
    fn the_best_nodes_win_whichever_project_they_came_from() {
        let mut hits = vec![
            hit("Tuxman", "src/a.rs:L1", 0.4),
            hit("Aether1", "src/b.rs:L2", 1.9),
            hit("Tuxman", "src/c.rs:L3", 0.9),
        ];
        rank_hits(&mut hits);
        assert_eq!(
            hits.iter().map(|h| h.score).collect::<Vec<_>>(),
            vec![1.9, 0.9, 0.4]
        );
        assert_eq!(hits[0].project, "Aether1");
    }

    // Two repositories can score a node identically, and the order the threads finish in
    // is not something to put in a prompt.
    #[test]
    fn an_equal_score_is_broken_the_same_way_every_time() {
        let ordered = |hits: Vec<Hit>| {
            let mut hits = hits;
            rank_hits(&mut hits);
            hits.iter()
                .map(|h| format!("{}:{}", h.project, h.pointer))
                .collect::<Vec<_>>()
        };
        let one = ordered(vec![
            hit("Tuxman", "src/a.rs:L1", 1.0),
            hit("Aether1", "src/z.rs:L9", 1.0),
        ]);
        let other = ordered(vec![
            hit("Aether1", "src/z.rs:L9", 1.0),
            hit("Tuxman", "src/a.rs:L1", 1.0),
        ]);
        assert_eq!(one, other);
        assert_eq!(one[0], "Aether1:src/z.rs:L9");
    }

    #[test]
    fn no_more_nodes_reach_the_prompt_than_the_cap() {
        let mut hits: Vec<Hit> = (0..40)
            .map(|i| hit("Aether1", &format!("src/f{i}.rs:L1"), i as f64))
            .collect();
        rank_hits(&mut hits);
        assert_eq!(hits.len(), MAX_NODES_PER_QUESTION);
    }

    #[test]
    fn tracking_everything_is_the_default() {
        let db = temp_db("default_all");
        assert!(tracking_all(&db));
        assert!(is_tracked(&db, Path::new("/anywhere/at/all")));
    }

    #[test]
    fn a_project_chosen_before_there_was_a_list_is_carried_into_it() {
        let db = temp_db("legacy");
        set_tracking_all(&db, false).expect("stop tracking all");
        db.set_setting(
            SELECTED_PROJECT_SETTING,
            &serde_json::Value::String("/home/op/Projects/Aether1".to_string()),
        )
        .expect("legacy setting");

        assert!(is_tracked(&db, Path::new("/home/op/Projects/Aether1")));
        assert!(!is_tracked(&db, Path::new("/home/op/Projects/Tuxman")));
    }

    #[test]
    fn a_tracked_project_survives_a_round_trip_and_can_be_turned_off() {
        let db = temp_db("round_trip");
        set_tracking_all(&db, false).expect("stop tracking all");
        let one = PathBuf::from("/home/op/Projects/Tuxman");

        set_tracked(&db, &one, true).expect("track");
        assert!(is_tracked(&db, &one));

        set_tracked(&db, &one, false).expect("untrack");
        assert!(!is_tracked(&db, &one));
    }

    // Unticking one project while everything is tracked has to leave the others ticked,
    // or the switch reads as doing nothing.
    #[test]
    fn turning_one_project_off_keeps_the_rest_and_stops_tracking_everything() {
        let db = temp_db("narrow");
        let kept = temp_project("kept", true);
        let dropped = temp_project("dropped", true);

        // Stand in for detection, which reads the operator's real home directory.
        store_tracked(&db, &[kept.clone(), dropped.clone()]).expect("seed");
        set_tracking_all(&db, false).expect("seed");

        set_tracked(&db, &dropped, false).expect("untrack");
        assert!(!tracking_all(&db));
        assert!(is_tracked(&db, &kept));
        assert!(!is_tracked(&db, &dropped));

        let _ = std::fs::remove_dir_all(&kept);
        let _ = std::fs::remove_dir_all(&dropped);
    }

    // A tracked project with no graph cannot answer anything, and asking it would spend a
    // process per turn to be told so.
    #[test]
    fn only_projects_with_a_graph_are_asked() {
        let db = temp_db("ready_only");
        set_tracking_all(&db, false).expect("stop tracking all");
        let built = temp_project("built", true);
        let unbuilt = temp_project("unbuilt", false);
        store_tracked(&db, &[built.clone(), unbuilt.clone()]).expect("seed");

        let asked = projects_to_ask(&db);
        assert_eq!(asked, vec![built.clone()]);

        let _ = std::fs::remove_dir_all(&built);
        let _ = std::fs::remove_dir_all(&unbuilt);
    }

    // The one test here that uses the real Graft CLI, so it is ignored by default and run
    // by hand: `cargo test -- --ignored graft`. Everything above it is about our own
    // bookkeeping; this is about whether two repositories really do get asked at once and
    // come back ranked against each other, which is the whole point of the change and is
    // not something the unit tests can show.
    #[test]
    #[ignore = "needs the graft CLI on PATH"]
    fn two_real_projects_are_asked_at_once_and_ranked_together() {
        let root = std::env::temp_dir().join(format!("aether1_graft_two_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut built = Vec::new();

        for (name, body) in [
            ("alpha", "pub fn alpha_telemetry_sampler() -> u32 { 1 }\n"),
            ("beta", "pub fn beta_telemetry_sampler() -> u32 { 2 }\n"),
        ] {
            let dir = root.join(name);
            std::fs::create_dir_all(dir.join("src")).expect("project tree");
            std::fs::write(dir.join("src").join("lib.rs"), body).expect("source");
            build_graph(&dir).expect("graft build should succeed");
            assert_eq!(check_graft_status(&dir), GraftStatus::Ready);
            built.push(dir);
        }

        let db = temp_db("two_real");
        set_tracking_all(&db, false).expect("stop tracking all");
        store_tracked(&db, &built).expect("track both");
        assert_eq!(projects_to_ask(&db).len(), 2);

        let primed = prime(&db, "telemetry sampler");
        assert!(
            primed.contains("[alpha]") && primed.contains("[beta]"),
            "both projects should contribute nodes, got:\n{primed}"
        );
        assert!(primed.contains("2 projects: alpha, beta"), "got:\n{primed}");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_project_is_named_in_the_prompt_by_its_folder() {
        assert_eq!(
            project_label(Path::new("/home/op/Projects/Aether1")),
            "Aether1"
        );
        assert_eq!(project_label(Path::new("/")), "/");
    }
}
