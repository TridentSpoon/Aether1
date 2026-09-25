//! The five things AETHER CODE can do, and the one file they are all written in.
//!
//! The companion has a registry (`tools/`), a consent queue, personas with fields, and an
//! action log. None of that is here, on purpose. The coding panel's whole safety argument
//! is that its capabilities can be read in one sitting: five entries, every one of them a
//! read, each gated by a named permission in `code_perms`. A reader who wants to know what
//! this panel may do to their machine should not have to assemble the answer from a
//! registry, a persona table and a domain policy.
//!
//! What is shared with the companion is the part that must never be answered twice:
//! `fs_guard` decides which paths exist as far as any model is concerned, and `read_file`
//! and `list_dir` are the companion's own implementations rather than second copies of
//! them. One list of denied paths, one truncation rule, one place to fix.
//!
//! **Nothing in this file writes, and nothing in this file can be made to.** There is no
//! write tool to gate; `gh` is checked against a whitelist of subcommands that only look;
//! `fetch_url` is a GET. A request to change something comes back as a refusal telling the
//! model to put the command in a fenced block, where `code_chat::commands_in` turns it into
//! a button that types into the operator's terminal and waits for their Return.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;
use ureq::ResponseExt;
use urlencoding;

use crate::code_perms::{self, Grant, Refusal};
use crate::llm::MemoryDb;
use crate::tools::{Tool, ToolContext};

/// How long `gh` gets before it is killed. Long enough for a slow network, short enough
/// that a command waiting on a terminal prompt nobody can answer does not hang the panel.
const GH_TIMEOUT: Duration = Duration::from_secs(30);

/// Caps on what comes back. A tool result that fills the context window has cost the
/// answer it was fetched for.
const MAX_OUTPUT_BYTES: usize = 16 * 1024;
const MAX_PAGE_BYTES: usize = 32 * 1024;

/// One capability, as the model is told about it.
struct Capability {
    name: &'static str,
    grant: Grant,
    /// The line in the prompt catalog: what it does and what to pass it.
    line: &'static str,
}

const CAPABILITIES: &[Capability] = &[
    Capability {
        name: "read_file",
        grant: Grant::System,
        line: "read_file {\"path\": \"/home/you/project/src/main.rs\"} -- read a text file on this machine.",
    },
    Capability {
        name: "list_dir",
        grant: Grant::System,
        line: "list_dir {\"path\": \"/home/you/project/src\"} -- list what is in a folder.",
    },
    Capability {
        name: "machine",
        grant: Grant::System,
        line: "machine {} -- what this machine is: OS, CPU, memory, graphics.",
    },
    Capability {
        name: "gh",
        grant: Grant::Github,
        line: "gh {\"args\": [\"pr\", \"view\", \"137\"]} -- the GitHub CLI, read-only subcommands only (view, list, diff, checks, status, search). Anything that changes a repository is refused; put that in a ```bash block instead.",
    },
    Capability {
        name: "fetch_url",
        grant: Grant::Internet,
        line: "fetch_url {\"url\": \"https://docs.rs/ureq/latest/ureq/\"} -- fetch a public page as text.",
    },
    Capability {
        name: "search_web",
        grant: Grant::Internet,
        line: "search_web {\"query\": \"rust concurrency 2024\"} -- search the web for recent information. Returns top results with title, URL, and snippet.",
    },
];

/// The tools that are available right now, for the system prompt.
///
/// Built per turn from the live settings rather than once at startup: a capability the
/// operator switched off five minutes ago should not still be advertised to the model,
/// which would otherwise spend a round calling it and reading the refusal.
pub fn catalog(db: &MemoryDb) -> Vec<&'static str> {
    CAPABILITIES
        .iter()
        .filter(|cap| code_perms::granted(db, cap.grant))
        .map(|cap| cap.line)
        .collect()
}

/// Whether anything at all is available. An empty catalog means the tool instructions are
/// left out of the prompt entirely -- telling a model about a protocol it has no tools for
/// is a way to get it calling tools that do not exist.
pub fn any_granted(db: &MemoryDb) -> bool {
    CAPABILITIES
        .iter()
        .any(|cap| code_perms::granted(db, cap.grant))
}

/// Runs one call from the model.
///
/// Every path through this function is a read. The `Err` case is what the model is told,
/// so a refusal here is written as an instruction rather than as a complaint.
pub fn call(db: &MemoryDb, name: &str, args: &Value) -> Result<String, String> {
    let Some(capability) = CAPABILITIES.iter().find(|cap| cap.name == name) else {
        let known: Vec<&str> = CAPABILITIES.iter().map(|cap| cap.name).collect();
        return Err(format!(
            "there is no tool called {name:?}. The ones that exist are: {}",
            known.join(", ")
        ));
    };
    code_perms::require(db, capability.grant).map_err(|Refusal(why)| why)?;

    match name {
        "read_file" => run_builtin(db, &crate::tools::builtin::ReadFile, args),
        "list_dir" => run_builtin(db, &crate::tools::builtin::ListDir, args),
        "machine" => Ok(machine_summary()),
        "gh" => gh(db, args),
        "fetch_url" => fetch_url(db, args),
        "search_web" => search_web(db, args),
        // Unreachable while CAPABILITIES and this match agree; a refusal rather than a
        // panic, because the cost of disagreeing is one confused turn and not a crash.
        other => Err(format!("{other} is declared but not implemented")),
    }
}

/// Hands a call to one of the companion's own read-only tools.
///
/// `Tool::call` rather than `tools::run`: the consent queue and the persona domain
/// belong to the companion's turn, and this panel has neither. What `Tool::call` still
/// carries is the part that matters -- `fs_guard` is checked inside the tool, so the
/// denied paths are denied here too.
fn run_builtin(db: &MemoryDb, tool: &dyn Tool, args: &Value) -> Result<String, String> {
    debug_assert!(
        !tool.mutating(),
        "only read-only tools belong in this panel"
    );
    let ctx = ToolContext::new(db);
    tool.call(args, &ctx).map(|outcome| outcome.result)
}

/// What this machine is, in the shape a coding model needs it: enough to answer "will this
/// build here" and "can I run that model", and nothing about who the operator is.
fn machine_summary() -> String {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    system.refresh_cpu_usage();

    let gb = |bytes: u64| bytes as f64 / 1024.0 / 1024.0 / 1024.0;
    let cpu = system
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let graphics = match crate::gpu::cached() {
        [] => "not detected".to_string(),
        gpus => gpus
            .iter()
            .map(crate::gpu::Gpu::summary)
            .collect::<Vec<String>>()
            .join("; "),
    };

    format!(
        "OS: {} ({})\nCPU: {cpu} ({} cores)\nMemory: {:.1} GB total, {:.1} GB available\nGraphics: {graphics}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        system.cpus().len(),
        gb(system.total_memory()),
        gb(system.available_memory()),
    )
}

/// Runs `gh`, once the classifier agrees it only looks.
///
/// There is no shell anywhere in this function. The arguments are passed to the program as
/// an argv, so a pipe, a redirect, a backtick or a `;` in one of them is a literal string
/// that `gh` will reject -- which is the same reason `run_command` takes an array.
fn gh(db: &MemoryDb, args: &Value) -> Result<String, String> {
    let argv: Vec<String> = args
        .get("args")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|v| v.as_str().unwrap_or_default().to_string())
                .collect()
        })
        .unwrap_or_default();
    if argv.is_empty() {
        return Err("gh needs arguments, as an array: {\"args\": [\"pr\", \"list\"]}".to_string());
    }

    code_perms::check_gh(db, &argv).map_err(|Refusal(why)| why)?;

    let binary = which::which("gh").map_err(|_| {
        "the GitHub CLI is not installed on this machine. `gh auth status` would be the test; \
         the operator installs it from https://cli.github.com."
            .to_string()
    })?;

    let mut child = Command::new(&binary)
        .args(&argv)
        // No stdin at all: a gh subcommand that decides to ask a question gets EOF and
        // gives up, rather than waiting out the timeout on a prompt nobody can see.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // gh pages its output through a pager when it thinks it has a terminal, and a
        // pager with no terminal is a process that never exits.
        .env("GH_PAGER", "cat")
        .env("PAGER", "cat")
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1")
        .spawn()
        .map_err(|e| format!("cannot start gh: {e}"))?;

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() > GH_TIMEOUT => {
                let _ = child.kill();
                return Err(format!(
                    "gh was still running after {}s and was stopped",
                    GH_TIMEOUT.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(format!("cannot wait for gh: {e}")),
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("cannot collect gh's output: {e}"))?;
    let stdout = truncate(&String::from_utf8_lossy(&output.stdout), MAX_OUTPUT_BYTES);
    let stderr = truncate(&String::from_utf8_lossy(&output.stderr), MAX_OUTPUT_BYTES);

    if output.status.success() {
        Ok(if stdout.trim().is_empty() {
            format!("gh {} returned nothing.", argv.join(" "))
        } else {
            stdout.trim_end().to_string()
        })
    } else {
        Err(format!(
            "gh {} failed: {}",
            argv.join(" "),
            if stderr.trim().is_empty() {
                stdout.trim_end().to_string()
            } else {
                stderr.trim_end().to_string()
            }
        ))
    }
}

/// Fetches a public page as text.
///
/// Deliberately thin: a GET, a size cap, and HTML reduced to something a model can read.
/// No cookies, no headers from the conversation, no POST, and no redirect to a private
/// address -- the final URL is checked again after any redirect, because a public host
/// that redirects to 127.0.0.1 is the oldest trick against a fetcher like this one.
fn fetch_url(db: &MemoryDb, args: &Value) -> Result<String, String> {
    let url = args
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| "fetch_url needs a url".to_string())?;

    code_perms::check_url(db, url).map_err(|Refusal(why)| why)?;

    let response = ureq::get(url.trim())
        .header("User-Agent", "AETHER1")
        .call()
        .map_err(|e| format!("cannot fetch {url}: {e}"))?;

    let landed = response.get_uri().to_string();
    if landed != url.trim() {
        code_perms::check_url(db, &landed).map_err(|Refusal(why)| {
            format!("{url} redirected somewhere that is not the internet: {why}")
        })?;
    }

    let body = response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("cannot read {url}: {e}"))?;

    Ok(format!(
        "{url}:\n{}",
        truncate(&readable(&body), MAX_PAGE_BYTES)
    ))
}

/// Searches the web using DuckDuckGo's free API for recent information.
///
/// Takes a required `query` parameter and optional `page` (1-indexed pagination).
/// Returns up to 10 results with title, URL, and snippet. Each result is formatted
/// as a numbered list item that the model can read and ask to fetch with fetch_url.
fn search_web(db: &MemoryDb, args: &Value) -> Result<String, String> {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .ok_or_else(|| "search_web needs a query: {\"query\": \"your search terms\"}".to_string())?;

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
                let url = result
                    .get("FirstURL")
                    .and_then(Value::as_str)
                    .unwrap_or("");

                if !url.is_empty() {
                    results.push_str(&format!("{}. {}\n   {}\n", idx + 1, title, url));
                }
            }
        }
    } else {
        results.push_str("No results found.");
    }

    Ok(truncate(&results, MAX_PAGE_BYTES))
}

/// HTML with the markup taken out, or the body unchanged when it was not HTML.
///
/// Not a parser and not trying to be. A model reading documentation needs the sentences;
/// the tags, the scripts and the stylesheets are tokens spent on nothing. Script and style
/// bodies are dropped wholesale rather than stripped of their tags, because their contents
/// are not prose.
fn readable(body: &str) -> String {
    if !body.trim_start().starts_with('<') && !body.contains("</") {
        return body.to_string();
    }

    let mut text = String::with_capacity(body.len() / 2);
    let mut rest = body;
    let mut in_tag = false;
    let mut skipping: Option<&str> = None;

    while let Some(index) = rest.find(['<', '>']) {
        let (chunk, tail) = rest.split_at(index);
        if !in_tag && skipping.is_none() {
            text.push_str(chunk);
        }
        let byte = tail.as_bytes()[0];
        rest = &tail[1..];
        if byte == b'<' {
            in_tag = true;
            let lowered = rest
                .get(..8)
                .map(str::to_ascii_lowercase)
                .unwrap_or_default();
            if let Some(closing) = skipping {
                if lowered.starts_with(&format!("/{closing}")) {
                    skipping = None;
                }
            } else if lowered.starts_with("script") {
                skipping = Some("script");
            } else if lowered.starts_with("style") {
                skipping = Some("style");
            }
        } else {
            in_tag = false;
            if skipping.is_none() {
                text.push(' ');
            }
        }
    }
    if !in_tag && skipping.is_none() {
        text.push_str(rest);
    }

    // Whitespace collapses to one blank line at most: an HTML page stripped of tags is
    // mostly indentation, and a hundred empty lines read as a page with nothing on it.
    let mut out = String::with_capacity(text.len());
    let mut blank = 0;
    for line in text.lines() {
        let line = line.split_whitespace().collect::<Vec<&str>>().join(" ");
        if line.is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(&line);
        out.push('\n');
    }
    out.trim().to_string()
}

/// Cuts to a byte budget on a character boundary, saying so.
fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[truncated at {max} bytes]", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn db() -> MemoryDb {
        let path = std::env::temp_dir().join(format!(
            "aether1_code_tools_{}_{:?}.db",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&path);
        MemoryDb::open(&path).expect("open test db")
    }

    /// The catalog is the model's whole picture of what it can do, so a switched-off
    /// permission has to disappear from it rather than sit there being refused.
    #[test]
    fn switching_a_permission_off_removes_it_from_the_catalog() {
        let db = db();
        assert_eq!(catalog(&db).len(), CAPABILITIES.len());

        code_perms::set(&db, Grant::Github, false).unwrap();
        let lines = catalog(&db);
        assert!(!lines.iter().any(|line| line.starts_with("gh ")));
        assert!(lines.iter().any(|line| line.starts_with("read_file ")));

        for grant in code_perms::ALL {
            code_perms::set(&db, *grant, false).unwrap();
        }
        assert!(catalog(&db).is_empty());
        assert!(!any_granted(&db));
    }

    #[test]
    fn an_invented_tool_is_refused_with_the_real_list() {
        let db = db();
        let Err(message) = call(&db, "write_file", &json!({"path": "/tmp/x"})) else {
            panic!("there is no write tool in this panel and there must never be one");
        };
        assert!(message.contains("read_file"));
        assert!(message.contains("gh"));
    }

    /// Every capability is gated, and the gate is checked before the tool runs rather than
    /// inside it -- otherwise a tool added later without its own check would be open.
    #[test]
    fn every_capability_refuses_when_its_permission_is_off() {
        let db = db();
        for grant in code_perms::ALL {
            code_perms::set(&db, *grant, false).unwrap();
        }
        for capability in CAPABILITIES {
            let Err(message) = call(
                &db,
                capability.name,
                &json!({"path": "/etc/hostname", "url": "https://example.com", "args": ["pr", "list"]}),
            ) else {
                panic!("{} ran with its permission off", capability.name);
            };
            assert!(
                message.contains("aether1 code perms"),
                "{} refused without saying which switch: {message}",
                capability.name
            );
        }
    }

    /// The path guard is the companion's, and it has to still apply here. A private key is
    /// off limits to the coding panel for exactly the same reason it is off limits to the
    /// avatar.
    #[test]
    fn the_path_guard_still_applies() {
        let db = db();
        let Err(message) = call(&db, "read_file", &json!({"path": "/etc/shadow"})) else {
            panic!("/etc/shadow must not be readable");
        };
        assert!(message.contains("off limits") || message.contains("cannot access"));
    }

    #[test]
    fn a_write_through_gh_is_refused_before_anything_is_spawned() {
        let db = db();
        let Err(message) = call(&db, "gh", &json!({"args": ["pr", "merge", "137"]})) else {
            panic!("gh pr merge must never run from here");
        };
        assert!(message.contains("```bash"), "{message}");
    }

    #[test]
    fn gh_needs_its_arguments_as_an_array() {
        let db = db();
        assert!(call(&db, "gh", &json!({"args": []})).is_err());
        assert!(call(&db, "gh", &json!({"command": "pr list"})).is_err());
    }

    #[test]
    fn a_loopback_url_is_refused_before_any_request_is_made() {
        let db = db();
        assert!(call(
            &db,
            "fetch_url",
            &json!({"url": "http://127.0.0.1:11434/api/tags"})
        )
        .is_err());
    }

    #[test]
    fn the_machine_summary_says_what_this_is() {
        let db = db();
        let report = call(&db, "machine", &json!({})).expect("machine always answers");
        assert!(report.contains("OS:"));
        assert!(report.contains("Memory:"));
        assert!(report.contains("Graphics:"));
    }

    #[test]
    fn html_becomes_something_worth_reading() {
        let page = "<html><head><style>body{color:red}</style>\
                    <script>alert('hi')</script></head>\
                    <body><h1>Title</h1>\n\n\n<p>A sentence.</p></body></html>";
        let text = readable(page);
        assert!(text.contains("Title"));
        assert!(text.contains("A sentence."));
        assert!(!text.contains("alert"), "scripts are not prose: {text}");
        assert!(
            !text.contains("color:red"),
            "stylesheets are not prose: {text}"
        );
        assert!(!text.contains('<'));
    }

    #[test]
    fn plain_text_is_left_alone() {
        assert_eq!(readable("fn main() {}\n"), "fn main() {}\n");
    }

    #[test]
    fn truncation_lands_on_a_character_boundary() {
        let text = "é".repeat(100);
        let cut = truncate(&text, 15);
        assert!(cut.contains("[truncated"));
        assert!(cut.len() < text.len());
    }
}
