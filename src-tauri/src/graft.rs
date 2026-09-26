// Graft integration for code graph analysis.
//
// Graft (https://github.com/nanonets/graft) is a code analysis tool that builds
// a graph of dependencies and relationships in a codebase. This module provides
// integration to auto-detect projects, build code graphs, and inject relevant
// code context into the LLM's system prompt.
//
// The graph is automatically kept in sync with the codebase:
// - Checks for code changes on each LLM turn
// - Auto-rebuilds when changes are detected
// - Tracks Graft version to ensure compatibility

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};
use crate::llm::MemoryDb;

/// Path setting for the currently selected project
const SELECTED_PROJECT_SETTING: &str = "graft_selected_project";

/// Last successful build time for the selected project
const LAST_BUILD_TIME_SETTING: &str = "graft_last_build_time";

/// How often to auto-rebuild the graph (in seconds)
/// Default: rebuild every hour or when code changes are detected
const AUTO_REBUILD_INTERVAL_SECS: u64 = 3600;

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

    for dir_opt in search_dirs {
        if let Some(dir) = dir_opt {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        // Check if it's a git repository
                        if path.join(".git").exists() {
                            if let Some(name) = path.file_name() {
                                let graft_status = check_graft_status(&path);
                                projects.push(Project {
                                    path,
                                    name: name.to_string_lossy().to_string(),
                                    graft_status,
                                });
                            }
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
    let graft_dir = project_path.join(".graft");
    if graft_dir.exists() {
        // Check for graph markdown files
        if let Ok(entries) = std::fs::read_dir(&graft_dir) {
            let has_graph = entries
                .flatten()
                .any(|e| e.path().extension().map_or(false, |ext| ext == "md"));
            if has_graph {
                return GraftStatus::Ready;
            }
        }
        GraftStatus::Error
    } else {
        GraftStatus::NotBuilt
    }
}

/// Get the version of Graft installed on this system
pub fn get_graft_version() -> Result<String, String> {
    let output = Command::new("graft")
        .arg("--version")
        .output()
        .map_err(|e| format!("Failed to check graft version: {e}"))?;

    if !output.status.success() {
        return Err("Graft not found or version check failed".to_string());
    }

    let version = String::from_utf8_lossy(&output.stdout);
    Ok(version.trim().to_string())
}

/// Build the Graft graph for a project
pub fn build_graph(project_path: &Path) -> Result<(), String> {
    // Try to run `graft build` in the project directory
    let output = Command::new("graft")
        .arg("build")
        .current_dir(project_path)
        .output()
        .map_err(|e| format!("Failed to run graft command: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Graft build failed: {stderr}"));
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

/// Load the Graft graph markdown files from a project
fn load_graph_files(project_path: &Path) -> Result<Vec<(String, String)>, String> {
    let graft_dir = project_path.join(".graft");
    if !graft_dir.exists() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&graft_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "md") {
                if let Ok(contents) = std::fs::read_to_string(&path) {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    files.push((name, contents));
                }
            }
        }
    }
    Ok(files)
}

/// Search for relevant code nodes based on a query
fn search_graph_nodes(files: &[(String, String)], query: &str) -> Vec<String> {
    let query_lower = query.to_lowercase();
    let mut results = Vec::new();

    for (filename, contents) in files {
        // Simple search: look for lines containing the query
        for line in contents.lines() {
            if line.to_lowercase().contains(&query_lower) {
                // Extract the node name (usually the first part before description)
                let trimmed = line.trim();
                if !trimmed.is_empty() && !results.contains(&trimmed.to_string()) {
                    results.push(trimmed.to_string());
                    if results.len() >= 5 {
                        return results;
                    }
                }
            }
        }
    }

    results
}

/// Get the last build timestamp for the selected project
fn get_last_build_time(db: &MemoryDb) -> u64 {
    let time_str = db.get_setting_string(LAST_BUILD_TIME_SETTING, "0");
    time_str.trim().parse::<u64>().unwrap_or(0)
}

/// Update the last build timestamp
fn set_last_build_time(db: &MemoryDb, timestamp: u64) {
    let _ = db.set_setting(
        LAST_BUILD_TIME_SETTING,
        &serde_json::Value::String(timestamp.to_string()),
    );
}

/// Check if code has changed since the last build
fn has_code_changed(project_path: &Path, last_build_time: u64) -> bool {
    // Check if any source files are newer than the last build
    let source_extensions = ["rs", "py", "js", "ts", "go", "c", "cpp", "java", "rb", "php"];

    if let Ok(entries) = std::fs::read_dir(project_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if let Some(ext_str) = ext.to_str() {
                        if source_extensions.contains(&ext_str) {
                            if let Ok(metadata) = path.metadata() {
                                if let Ok(modified) = metadata.modified() {
                                    if let Ok(duration) = modified.duration_since(UNIX_EPOCH) {
                                        if duration.as_secs() > last_build_time {
                                            return true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    false
}

/// Auto-sync the Graft graph: rebuild if needed and if interval has passed
fn auto_sync_graph(db: &MemoryDb, project_path: &Path) -> Result<bool, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let last_build = get_last_build_time(db);
    let time_since_build = now.saturating_sub(last_build);

    // Check if we should rebuild based on time interval or code changes
    let should_rebuild = time_since_build >= AUTO_REBUILD_INTERVAL_SECS
        || has_code_changed(project_path, last_build);

    if should_rebuild {
        match build_graph(project_path) {
            Ok(()) => {
                set_last_build_time(db, now);
                Ok(true)
            }
            Err(e) => {
                eprintln!("[AETHER1] Failed to auto-rebuild Graft graph: {e}");
                Err(e)
            }
        }
    } else {
        Ok(false)
    }
}

/// The Graft graph contribution to the system prompt, injected when analyzing code.
/// Auto-syncs the graph if needed before injecting relevant nodes.
pub fn prime(db: &MemoryDb, query: &str) -> String {
    let selected = match get_selected_project(db) {
        Some(path) => path,
        None => return String::new(),
    };

    // Auto-sync the graph on each LLM turn, only rebuilding if needed
    let _ = auto_sync_graph(db, &selected);

    let graph_files = match load_graph_files(&selected) {
        Ok(files) if !files.is_empty() => files,
        _ => return String::new(),
    };

    let relevant_nodes = search_graph_nodes(&graph_files, query);
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
}
