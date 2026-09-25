// The read-only tools. Every one of these answers a question about the machine; none of
// them changes anything, which is why they can run without asking. Anything that mutates
// waits for the consent path.

use serde_json::{json, Value};
use sysinfo::{ProcessesToUpdate, System};
use ureq::ResponseExt;

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

    fn call(&self, args: &Value, ctx: &ToolContext) -> Result<Outcome, String> {
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

        // A note fetched by name is the strongest form of "this is where the answer came
        // from", so it is worth reporting to the HUD. Only vault files: the rest of the
        // filesystem is not the operator's memory and does not belong in that footer.
        if let Some(note) = crate::vault::note_in_vault(ctx.db, &path) {
            crate::vault::consulted::record(&note, crate::vault::consulted::How::Read);
        }

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
        "Search the memory vault -- every note Aether1 keeps about this operator -- by keyword. Ranks by where a word appears: a note named for the topic first, then one with it in a heading, then one that mentions it in passing. Use this before saying you don't know something about them, and whenever the index has no obvious answer. Returns a shortlist of notes to read with read_file, not the notes themselves."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Words to look for. Matched against note names, headings and body text; a note matching more of them ranks higher, so include the topic rather than a whole sentence."
                }
            },
            "required": ["query"]
        })
    }

    fn mutating(&self) -> bool {
        false
    }

    fn call(&self, args: &Value, ctx: &ToolContext) -> Result<Outcome, String> {
        let query = string_arg(args, "query")?;

        /* The vault is the memory now, so it is what this searches. The key-value table is
        still read when there is no vault at all -- which only happens before the first run
        creates one, since `ensure` copies those rows into imported-memories.md and after
        that the vault search covers them. Reading both would mean answering the same fact
        twice under two names. */
        if crate::vault::vault_path(ctx.db).exists() {
            let results = crate::vault::search::search(ctx.db, query);
            return Ok(Outcome::text(crate::vault::search::render(
                ctx.db, query, &results,
            )));
        }

        let lowered = query.to_lowercase();
        let memories = ctx.db.get_all_memories().map_err(|e| e.to_string())?;
        let hits: Vec<String> = memories
            .iter()
            .filter(|m| {
                m.key.to_lowercase().contains(&lowered) || m.value.to_lowercase().contains(&lowered)
            })
            .take(MAX_MEMORY_HITS)
            .map(|m| format!("- {}: {}", m.key, m.value))
            .collect();

        Ok(Outcome::text(if hits.is_empty() {
            format!(
                "Nothing stored matching {query:?} ({} memories searched; there is no vault \
                 yet).",
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

// ------------------------------------------------------------ search_web

pub struct SearchWeb;

impl Tool for SearchWeb {
    fn name(&self) -> &'static str {
        "search_web"
    }

    fn description(&self) -> &'static str {
        "Search the web for recent information using DuckDuckGo. Returns top results with title and URL so you can fetch and read them with fetch_url."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search terms."
                }
            },
            "required": ["query"]
        })
    }

    fn mutating(&self) -> bool {
        false
    }

    fn call(&self, args: &Value, _ctx: &ToolContext) -> Result<Outcome, String> {
        let query = string_arg(args, "query")?;

        if query.trim().is_empty() {
            return Err("search query cannot be empty".to_string());
        }

        // DuckDuckGo's public API endpoint
        let api_url = format!(
            "https://api.duckduckgo.com/?q={}&format=json&no_html=1",
            urlencoding::encode(query)
        );

        let response = ureq::get(&api_url)
            .header("User-Agent", "AETHER1")
            .call()
            .map_err(|e| format!("cannot search the web: {e}"))?;

        let body = response
            .into_body()
            .read_to_string()
            .map_err(|e| format!("cannot read search results: {e}"))?;

        let json: Value = serde_json::from_str(&body)
            .map_err(|e| format!("search API returned invalid JSON: {e}"))?;

        let mut results = String::new();
        results.push_str(&format!("Search results for: {}\n\n", query));

        // Add abstract/featured result if available
        if let Some(abstract_text) = json.get("AbstractText").and_then(Value::as_str) {
            if !abstract_text.trim().is_empty() {
                if let Some(abstract_url) = json.get("AbstractURL").and_then(Value::as_str) {
                    results.push_str(&format!(
                        "Featured: {}\n{}\n\n",
                        abstract_text.trim(),
                        abstract_url
                    ));
                }
            }
        }

        // Add main results
        if let Some(search_results) = json.get("Results").and_then(Value::as_array) {
            if search_results.is_empty() {
                results.push_str("No results found.");
            } else {
                results.push_str("Results:\n");
                for (idx, result) in search_results.iter().take(10).enumerate() {
                    let title = result
                        .get("Text")
                        .and_then(Value::as_str)
                        .unwrap_or("Untitled");
                    let url = result.get("FirstURL").and_then(Value::as_str).unwrap_or("");

                    if !url.is_empty() {
                        results.push_str(&format!("{}. {}\n   {}\n", idx + 1, title, url));
                    }
                }
            }
        } else {
            results.push_str("No results found.");
        }

        const MAX_PAGE_BYTES: usize = 32 * 1024;
        Ok(Outcome::text(super::truncate(&results, MAX_PAGE_BYTES)))
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
        let ctx = ToolContext::new(&db);
        let err = ReadFile
            .call(&json!({"path": "/bin/sh"}), &ctx)
            .unwrap_err();
        assert!(err.contains("outside the paths"), "{err}");
    }

    #[test]
    fn read_file_reads_an_allowed_file() {
        let db = temp_db("read_hostname");
        let ctx = ToolContext::new(&db);
        let outcome = ReadFile
            .call(&json!({"path": "/etc/hostname"}), &ctx)
            .unwrap();
        assert!(outcome.result.contains("/etc/hostname"));
        assert!(outcome.undo.is_none(), "a read is not reversible work");
    }

    #[test]
    fn read_file_needs_its_argument() {
        let db = temp_db("read_noargs");
        let ctx = ToolContext::new(&db);
        let err = ReadFile.call(&json!({}), &ctx).unwrap_err();
        assert!(err.contains("path"), "{err}");
    }

    #[test]
    fn list_dir_lists_and_reports_the_total() {
        let db = temp_db("list_dir");
        let ctx = ToolContext::new(&db);
        let outcome = ListDir.call(&json!({"path": "/etc"}), &ctx).unwrap();
        assert!(outcome.result.contains("/etc ("));
        assert!(outcome.result.contains("entries"));
    }

    /// Sets up a db whose vault is a fresh temporary folder. Pinned explicitly: without it
    /// the vault resolves under the real home directory, and a test that reads whatever
    /// notes happen to be on the machine running it is a test that passes or fails by
    /// accident.
    fn db_with_vault(name: &str) -> (MemoryDb, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("aether1_builtin_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = MemoryDb::open(dir.join("memory.db")).unwrap();
        db.set_setting(
            "vault_path",
            &json!(dir.join("vault").to_string_lossy().to_string()),
        )
        .unwrap();
        let root = crate::vault::ensure(&db).unwrap();
        (db, root)
    }

    /// The HUD's footer, at the two call sites in this file: a note the model went and
    /// fetched is reported as read, and one search merely offered is reported as found.
    /// The distinction is the point -- a shortlist is not a citation.
    #[test]
    fn reading_a_vault_note_reports_it_and_searching_reports_the_shortlist() {
        let (db, root) = db_with_vault("consulted");
        std::fs::write(
            root.join("projects/editors.md"),
            "# Editors\n\nThey moved from vim to helix in 2025.\n",
        )
        .unwrap();
        let ctx = ToolContext::new(&db);

        // read_file only reaches what fs_guard allows, and the vault's real home is the
        // operator's home directory -- so the fixture's has to be one too.
        let notes = crate::tools::fs_guard::with_home(root.parent().unwrap(), || {
            crate::vault::consulted::begin();
            SearchMemory
                .call(&json!({"query": "editors"}), &ctx)
                .unwrap();
            ReadFile
                .call(
                    &json!({"path": root.join("projects/editors.md").to_string_lossy()}),
                    &ctx,
                )
                .unwrap();
            // Outside the vault, and so none of the operator's memory: not in the footer.
            ReadFile
                .call(&json!({"path": "/etc/hostname"}), &ctx)
                .unwrap();
            crate::vault::consulted::taken()
        });
        assert_eq!(
            notes.len(),
            1,
            "only the vault note belongs in the footer: {notes:?}"
        );
        assert_eq!(notes[0].note, "projects/editors.md");
        assert_eq!(
            notes[0].how,
            crate::vault::consulted::How::Read,
            "a note that was found and then read is reported as read"
        );
    }

    /// search_memory searches the vault now, not the old key-value table.
    #[test]
    fn search_memory_finds_the_note_and_says_so_when_there_is_none() {
        let (db, root) = db_with_vault("search_memory");
        std::fs::write(
            root.join("projects/editors.md"),
            "# Editors\n\nThey moved from vim to helix in 2025.\n",
        )
        .unwrap();
        let ctx = ToolContext::new(&db);

        let hit = SearchMemory
            .call(&json!({"query": "editors"}), &ctx)
            .unwrap();
        assert!(hit.result.contains("projects/editors.md"), "{}", hit.result);
        assert!(hit.result.contains("helix"), "{}", hit.result);

        let miss = SearchMemory
            .call(&json!({"query": "sourdough"}), &ctx)
            .unwrap();
        assert!(miss.result.contains("No note matches"), "{}", miss.result);
    }

    /// Before the first run creates a vault there is nowhere to search, and the old
    /// key-value rows are the only memory there is. After it, `ensure` has copied them into
    /// a note and the vault search covers them -- so this path is a fallback, not a second
    /// source competing with the first.
    #[test]
    fn search_memory_falls_back_to_the_old_rows_when_there_is_no_vault_yet() {
        let dir = std::env::temp_dir().join(format!("aether1_novault_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = MemoryDb::open(dir.join("memory.db")).unwrap();
        // Pointed at a folder that does not exist, which is what "before the first run"
        // looks like from here.
        db.set_setting(
            "vault_path",
            &json!(dir.join("absent").to_string_lossy().to_string()),
        )
        .unwrap();
        db.set_memory("favorite_editor", "helix", "general")
            .unwrap();
        let ctx = ToolContext::new(&db);

        let hit = SearchMemory
            .call(&json!({"query": "editor"}), &ctx)
            .unwrap();
        assert!(hit.result.contains("helix"), "{}", hit.result);
    }

    #[test]
    fn the_no_argument_tools_answer_without_arguments() {
        let db = temp_db("noargs");
        let ctx = ToolContext::new(&db);
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
    fn the_read_only_tools_are_the_ones_that_only_look() {
        // The boundary this file exists to hold: everything here answers a question about
        // the machine and changes nothing, which is why these run without asking.
        let looking = [
            "read_file",
            "list_dir",
            "list_processes",
            "telemetry_detail",
            "search_memory",
            "search_web",
        ];
        for schema in super::super::registry().schemas() {
            if looking.contains(&schema.name) {
                assert!(
                    !schema.mutating,
                    "{} only looks and must not mutate",
                    schema.name
                );
            }
        }
    }
}
