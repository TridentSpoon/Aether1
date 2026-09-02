// The read-only tools. Every one of these answers a question about the machine; none of
// them changes anything, which is why they can run without asking. Anything that mutates
// waits for the consent path.

use serde_json::{json, Value};
use sysinfo::{ProcessesToUpdate, System};

use super::fs_guard;
use super::{Outcome, Tool, ToolContext};
use crate::llm::Telemetry;

/// Reading a whole large file into a chat context is worse than useless -- it evicts the
/// conversation to say nothing. Tools truncate and say so.
const MAX_FILE_BYTES: usize = 64 * 1024;
const MAX_DIR_ENTRIES: usize = 200;
const MAX_PROCESSES: usize = 15;
const MAX_MEMORY_HITS: usize = 20;

fn string_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing required string argument {key:?}"))
}

// ---------------------------------------------------------------- read_file

pub struct ReadFile;

impl Tool for ReadFile {
    fn name(&self) -> &'static str {
        "read_file"
    }

    fn description(&self) -> &'static str {
        "Read a text file from the operator's machine. Only paths under their home directory, /etc, /proc and /var/log are readable, and credentials, private keys and Aether1's own database are never readable. Large files are truncated."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute path, or one starting with ~/ for the operator's home directory."
                }
            },
            "required": ["path"]
        })
    }

    fn mutating(&self) -> bool {
        false
    }

    fn call(&self, args: &Value, _ctx: &ToolContext) -> Result<Outcome, String> {
        let path = fs_guard::resolve_readable(string_arg(args, "path")?)?;
        if path.is_dir() {
            return Err(format!(
                "{} is a directory -- use list_dir for that",
                path.display()
            ));
        }

        let bytes =
            std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let truncated = bytes.len() > MAX_FILE_BYTES;
        let slice = &bytes[..bytes.len().min(MAX_FILE_BYTES)];

        // Binary content in a prompt is noise the model can't use and can't be trusted to
        // ignore, so say what it is instead of pasting it.
        let text = match std::str::from_utf8(slice) {
            Ok(text) => text,
            Err(_) => {
                return Ok(Outcome::text(format!(
                    "{} is not a text file ({} bytes of binary data)",
                    path.display(),
                    bytes.len()
                )))
            }
        };

        Ok(Outcome::text(if truncated {
            format!(
                "{} (first {MAX_FILE_BYTES} bytes of {}):\n{text}\n[truncated]",
                path.display(),
                bytes.len()
            )
        } else {
            format!("{}:\n{text}", path.display())
        }))
    }
}

// ----------------------------------------------------------------- list_dir

pub struct ListDir;

impl Tool for ListDir {
    fn name(&self) -> &'static str {
        "list_dir"
    }

    fn description(&self) -> &'static str {
        "List the contents of a directory on the operator's machine, with sizes. Subject to the same path limits as read_file."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute path, or one starting with ~/ for the operator's home directory."
                }
            },
            "required": ["path"]
        })
    }

    fn mutating(&self) -> bool {
        false
    }

    fn call(&self, args: &Value, _ctx: &ToolContext) -> Result<Outcome, String> {
        let path = fs_guard::resolve_readable(string_arg(args, "path")?)?;
        let entries =
            std::fs::read_dir(&path).map_err(|e| format!("cannot list {}: {e}", path.display()))?;

        let mut lines = Vec::new();
        let mut total = 0usize;
        for entry in entries.flatten() {
            total += 1;
            if lines.len() >= MAX_DIR_ENTRIES {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let meta = entry.metadata().ok();
            let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            lines.push(if is_dir {
                format!("{name}/")
            } else {
                format!("{name} ({size} bytes)")
            });
        }
        lines.sort();

        let mut report = format!(
            "{} ({total} entries):\n{}",
            path.display(),
            lines.join("\n")
        );
        if total > lines.len() {
            report.push_str(&format!("\n[{} more not shown]", total - lines.len()));
        }
        Ok(Outcome::text(report))
    }
}

// ----------------------------------------------------------- list_processes

pub struct ListProcesses;

impl Tool for ListProcesses {
    fn name(&self) -> &'static str {
        "list_processes"
    }

    fn description(&self) -> &'static str {
        "List the processes currently using the most CPU, with their PIDs and memory use. Use this when the operator asks what is making the machine slow, hot, or busy."
    }

    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {}, "required": [] })
    }

    fn mutating(&self) -> bool {
        false
    }

    fn call(&self, _args: &Value, _ctx: &ToolContext) -> Result<Outcome, String> {
        let mut system = System::new();
        // CPU percentages are a delta between two samples: a single refresh reports 0% for
        // everything, which would answer "what's using the CPU" with "nothing".
        system.refresh_processes(ProcessesToUpdate::All, true);
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        system.refresh_processes(ProcessesToUpdate::All, true);

        let mut processes: Vec<_> = system
            .processes()
            .values()
            .map(|p| {
                (
                    p.name().to_string_lossy().to_string(),
                    p.pid().as_u32(),
                    p.cpu_usage(),
                    p.memory() / (1024 * 1024),
                )
            })
            .collect();
        processes.sort_by(|a, b| b.2.total_cmp(&a.2));
        processes.truncate(MAX_PROCESSES);

        let lines: Vec<String> = processes
            .iter()
            .map(|(name, pid, cpu, mem_mb)| {
                format!("{name} (pid {pid}): {cpu:.1}% CPU, {mem_mb} MB")
            })
            .collect();

        Ok(Outcome::text(format!(
            "Top {} processes by CPU:\n{}",
            lines.len(),
            lines.join("\n")
        )))
    }
}

// --------------------------------------------------------- telemetry_detail

pub struct TelemetryDetail;

impl Tool for TelemetryDetail {
    fn name(&self) -> &'static str {
        "telemetry_detail"
    }

    fn description(&self) -> &'static str {
        "Take a fresh, full reading of the host: CPU, RAM, storage, network throughput and uptime. The prompt already carries a summary; call this when the operator asks for detail or for a current reading."
    }

    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {}, "required": [] })
    }

    fn mutating(&self) -> bool {
        false
    }

    fn call(&self, _args: &Value, _ctx: &ToolContext) -> Result<Outcome, String> {
        Ok(Outcome::text(Telemetry::snapshot().diagnostic_report()))
    }
}

// ------------------------------------------------------------ search_memory

pub struct SearchMemory;

impl Tool for SearchMemory {
    fn name(&self) -> &'static str {
        "search_memory"
    }

    fn description(&self) -> &'static str {
        "Search everything the operator has asked Aether1 to remember. Use this before saying you don't know something about them -- the answer is often already stored."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Words to look for. Matches against both the name and the content of each stored memory."
                }
            },
            "required": ["query"]
        })
    }

    fn mutating(&self) -> bool {
        false
    }

    fn call(&self, args: &Value, ctx: &ToolContext) -> Result<Outcome, String> {
        let query = string_arg(args, "query")?.to_lowercase();
        let memories = ctx.db.get_all_memories().map_err(|e| e.to_string())?;

        // Substring matching for now. Step 10 replaces this with real retrieval; the
        // tool's shape doesn't change when it does.
        let hits: Vec<String> = memories
            .iter()
            .filter(|m| {
                m.key.to_lowercase().contains(&query) || m.value.to_lowercase().contains(&query)
            })
            .take(MAX_MEMORY_HITS)
            .map(|m| format!("- {}: {}", m.key, m.value))
            .collect();

        Ok(Outcome::text(if hits.is_empty() {
            format!(
                "Nothing stored matching {query:?} ({} memories searched).",
                memories.len()
            )
        } else {
            format!(
                "{} match(es) for {query:?}:\n{}",
                hits.len(),
                hits.join("\n")
            )
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::MemoryDb;

    fn temp_db(name: &str) -> MemoryDb {
        let path =
            std::env::temp_dir().join(format!("aether1_tools_{name}_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        MemoryDb::open(path).unwrap()
    }

    #[test]
    fn read_file_refuses_a_path_outside_the_allowed_roots() {
        let db = temp_db("read_file");
        let ctx = ToolContext { db: &db };
        let err = ReadFile
            .call(&json!({"path": "/bin/sh"}), &ctx)
            .unwrap_err();
        assert!(err.contains("outside the paths"), "{err}");
    }

    #[test]
    fn read_file_reads_an_allowed_file() {
        let db = temp_db("read_hostname");
        let ctx = ToolContext { db: &db };
        let outcome = ReadFile
            .call(&json!({"path": "/etc/hostname"}), &ctx)
            .unwrap();
        assert!(outcome.result.contains("/etc/hostname"));
        assert!(outcome.undo.is_none(), "a read is not reversible work");
    }

    #[test]
    fn read_file_needs_its_argument() {
        let db = temp_db("read_noargs");
        let ctx = ToolContext { db: &db };
        let err = ReadFile.call(&json!({}), &ctx).unwrap_err();
        assert!(err.contains("path"), "{err}");
    }

    #[test]
    fn list_dir_lists_and_reports_the_total() {
        let db = temp_db("list_dir");
        let ctx = ToolContext { db: &db };
        let outcome = ListDir.call(&json!({"path": "/etc"}), &ctx).unwrap();
        assert!(outcome.result.contains("/etc ("));
        assert!(outcome.result.contains("entries"));
    }

    #[test]
    fn search_memory_finds_what_was_stored_and_says_so_when_it_doesnt() {
        let db = temp_db("search_memory");
        db.set_memory("favorite_editor", "helix", "general")
            .unwrap();
        let ctx = ToolContext { db: &db };

        let hit = SearchMemory
            .call(&json!({"query": "editor"}), &ctx)
            .unwrap();
        assert!(hit.result.contains("helix"), "{}", hit.result);

        let miss = SearchMemory
            .call(&json!({"query": "sourdough"}), &ctx)
            .unwrap();
        assert!(miss.result.contains("Nothing stored"), "{}", miss.result);
    }

    #[test]
    fn the_no_argument_tools_answer_without_arguments() {
        let db = temp_db("noargs");
        let ctx = ToolContext { db: &db };
        assert!(TelemetryDetail
            .call(&json!({}), &ctx)
            .unwrap()
            .result
            .contains("SYSTEM DIAGNOSTIC REPORT"));
        assert!(ListProcesses
            .call(&json!({}), &ctx)
            .unwrap()
            .result
            .contains("CPU"));
    }

    #[test]
    fn every_builtin_is_read_only() {
        // The consent path does not exist yet, so nothing registered may mutate.
        for tool in super::super::registry().schemas() {
            assert!(!tool.mutating, "{} must not mutate at this step", tool.name);
        }
    }
}
