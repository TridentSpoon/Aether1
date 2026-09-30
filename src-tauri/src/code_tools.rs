//! The capabilities AETHER CODE may use, each gated by a named permission.
//!
//! Unlike the companion's broader registry (`tools/`), this list is specific to repository
//! work: inspect files and GitHub, fetch documentation, edit the nominated workspace, and
//! run commands under the workspace/sandbox rules. A reader can understand what the model
//! may do from this catalog and `code_perms` without following a second consent system.
//!
//! What is shared with the companion is the part that must never be answered twice:
//! `fs_guard` decides which paths exist as far as any model is concerned, and `read_file`
//! and `list_dir` are the companion's own implementations rather than second copies of
//! them. One list of denied paths, one truncation rule, one place to fix.
//!
//! GitHub operations remain read-only here. Local edits and commands go through
//! `code_workspace`, which checks the project boundary, edit/run grants, and sandbox before
//! anything is changed or spawned. The terminal is not used by these tools.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;
use ureq::ResponseExt;

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
        line: "gh {\"args\": [\"pr\", \"view\", \"137\", \"--repo\", \"OWNER/REPO\"]} -- the GitHub CLI, read-only subcommands only (view, list, diff, checks, status, search). For a remote repo, include --repo OWNER/REPO so the request cannot accidentally target a different checkout. Anything that changes a repository is refused; put that in a ```bash block instead.",
    },
    Capability {
        name: "fetch_url",
        grant: Grant::Internet,
        line: "fetch_url {\"url\": \"https://docs.rs/ureq/latest/ureq/\"} -- fetch a public page as text.",
    },
    Capability {
        name: "edit_file",
        grant: Grant::Edit,
        line: "edit_file {\"path\": \"src/main.rs\", \"find\": \"let x = 1;\", \"replace\": \"let x = 2;\"} -- replace exact text in a file in the project folder. The text must appear exactly once; include surrounding lines if it does not.",
    },
    Capability {
        name: "create_file",
        grant: Grant::Edit,
        line: "create_file {\"path\": \"src/new.rs\", \"content\": \"...\"} -- write a whole file in the project folder, creating it or replacing it.",
    },
    Capability {
        name: "run",
        grant: Grant::Run,
        line: "run {\"argv\": [\"cargo\", \"test\"], \"cwd\": \"\", \"timeout_secs\": 120} -- run a build or test command in the project folder. A non-zero exit is a result to read, not an error. The command runs in a sandbox: the project folder is the only writable place, the home directory is not there, and the network is off unless the operator turned it on. When Run is off, only command classes they explicitly remembered with Allow similar can run; use a single argv, not a shell. For a new command, give them a bash suggestion so they can approve it once or remember its command class.",
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
        .filter(|cap| {
            if cap.name == "run" {
                return run_tool_available(db);
            }
            code_perms::granted(db, cap.grant)
        })
        .map(|cap| cap.line)
        .collect()
}

/// Whether anything at all is available. An empty catalog means the tool instructions are
/// left out of the prompt entirely -- telling a model about a protocol it has no tools for
/// is a way to get it calling tools that do not exist.
pub fn any_granted(db: &MemoryDb) -> bool {
    CAPABILITIES.iter().any(|cap| {
        if cap.name == "run" {
            run_tool_available(db)
        } else {
            code_perms::granted(db, cap.grant)
        }
    })
}

fn run_tool_available(db: &MemoryDb) -> bool {
    code_perms::granted(db, Grant::Run)
        || crate::code_workspace::root(db)
            .ok()
            .is_some_and(|root| !crate::code_policy::similar_commands(&root).is_empty())
}

/// Runs one call from the model.
///
/// Most paths through this function are reads. The three that are not -- `edit_file`,
/// `create_file` and `run` -- are gated by grants that default off and are confined to the
/// operator's nominated project folder by `code_workspace`, which is where their whole
/// argument lives. The `Err` case is what the model is told, so a refusal here is written
/// as an instruction rather than as a complaint.
pub fn call(db: &MemoryDb, name: &str, args: &Value) -> Result<String, String> {
    let Some(capability) = CAPABILITIES.iter().find(|cap| cap.name == name) else {
        let known: Vec<&str> = CAPABILITIES.iter().map(|cap| cap.name).collect();
        return Err(format!(
            "there is no tool called {name:?}. The ones that exist are: {}",
            known.join(", ")
        ));
    };
    if name == "run" && !code_perms::granted(db, Grant::Run) {
        let root = match crate::code_workspace::root(db) {
            Ok(root) => root,
            Err(_) => {
                return Err(code_perms::require(db, Grant::Run)
                    .expect_err("Run was checked off immediately above")
                    .0)
            }
        };
        let argv: Vec<String> = args
            .get("argv")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        if args.get("shell").is_some() || !crate::code_policy::allows_similar_argv(&root, &argv) {
            return Err("Run is off. This project only remembers specific build/test command classes. Use a single argv covered by its Allow similar rule, or show the operator a bash command so they can approve it once or remember it.".to_string());
        }
    } else {
        code_perms::require(db, capability.grant).map_err(|Refusal(why)| why)?;
    }

    // Before the first thing that changes anything, a way back. See code_checkpoint.rs for
    // why this replaced the table of refused `git` subcommands rather than joining it.
    if matches!(name, "edit_file" | "create_file" | "run") {
        checkpoint_if_due(db);
    }

    match name {
        "read_file" => run_builtin(db, &crate::tools::builtin::ReadFile, args),
        "list_dir" => run_builtin(db, &crate::tools::builtin::ListDir, args),
        "machine" => Ok(machine_summary()),
        "gh" => gh(db, args),
        "fetch_url" => fetch_url(db, args),
        "search_web" => search_web(db, args),
        "edit_file" => crate::code_workspace::edit_file(db, args),
        "create_file" => crate::code_workspace::create_file(db, args),
        "run" => crate::code_workspace::run(db, args),
        // Unreachable while CAPABILITIES and this match agree; a refusal rather than a
        // panic, because the cost of disagreeing is one confused turn and not a crash.
        other => Err(format!("{other} is declared but not implemented")),
    }
}

/// How long one checkpoint covers. A burst of edits and test runs is one piece of work and
/// wants one way back, not forty; an hour later it is a different afternoon and wants its
/// own. Long enough that `git add -A` on a large repository is never in the way, short
/// enough that what a revert loses is a session and not a day.
const CHECKPOINT_EVERY: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// Takes a checkpoint when the newest one is old enough to be a different piece of work.
///
/// Best effort on purpose: a workspace that is not a git repository, or a git that will not
/// run, must not stop the model from working. What it costs is the ability to undo, and
/// `aether1 code checkpoints` says plainly when there is nothing to undo to.
fn checkpoint_if_due(db: &MemoryDb) {
    let Ok(root) = crate::code_workspace::root(db) else {
        return;
    };
    let due = match crate::code_checkpoint::latest(&root) {
        Ok(Some(newest)) => match newest
            .reference
            .rsplit('/')
            .next()
            .and_then(|s| s.parse::<u64>().ok())
        {
            Some(secs) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                now.saturating_sub(secs) >= CHECKPOINT_EVERY.as_secs()
            }
            None => true,
        },
        Ok(None) => true,
        Err(_) => false,
    };
    if due {
        let _ = crate::code_checkpoint::take(&root, "before AETHER CODE changed anything");
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

    run_gh(db, &argv)
}

/// Runs one GitHub write after the operator approved this exact command in the UI.
/// Permanent "similar command" grants are intentionally not offered for remote writes.
pub fn run_approved_github_write(db: &MemoryDb, argv: &[String]) -> Result<String, String> {
    if !code_perms::granted(db, Grant::Github) {
        return Err("GitHub access is off in Settings.".to_string());
    }
    if crate::local_only::enabled(db) {
        return Err("Local-only mode is on, so AETHER1 will not contact GitHub. Turn it off before approving this action.".to_string());
    }
    validate_approved_github_write(argv)?;
    run_gh(db, argv)
}

/// Runs a suggested gh invocation after the operator's exact-command approval. Only the
/// established read-only classifier or the narrowly supported PR write validator can pass.
pub fn run_approved_github_command(db: &MemoryDb, argv: &[String]) -> Result<String, String> {
    if validate_approved_github_write(argv).is_ok() {
        return run_approved_github_write(db, argv);
    }
    code_perms::check_gh(db, argv).map_err(|Refusal(why)| why)?;
    if crate::local_only::enabled(db) {
        return Err("Local-only mode is on, so AETHER1 will not contact GitHub.".to_string());
    }
    run_gh(db, argv)
}

fn validate_approved_github_write(argv: &[String]) -> Result<(), String> {
    let Some([pr, verb]) = argv.get(..2) else {
        return Err("Only `gh pr create` and `gh pr merge` can be approved here.".to_string());
    };
    if pr != "pr" || !matches!(verb.as_str(), "create" | "merge") {
        return Err("Only `gh pr create` and `gh pr merge` can be approved here.".to_string());
    }
    let args = &argv[2..];
    let allowed = if verb == "create" {
        &[
            "--title", "-t", "--body", "-b", "--base", "-B", "--head", "--draft", "--fill",
            "--repo",
        ][..]
    } else {
        &[
            "--merge",
            "--squash",
            "--rebase",
            "--delete-branch",
            "--auto",
            "--repo",
        ][..]
    };
    let mut positional = 0;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if verb == "merge" && positional == 0 && !arg.starts_with('-') {
            positional += 1;
            index += 1;
            continue;
        }
        if allowed.contains(&arg.as_str()) {
            if matches!(
                arg.as_str(),
                "--title" | "-t" | "--body" | "-b" | "--base" | "-B" | "--head" | "--repo"
            ) {
                index += 1;
                if index >= args.len() || args[index].starts_with('-') {
                    return Err(format!("{arg} needs a value."));
                }
            }
        } else {
            return Err(format!(
                "`gh pr {verb}` option {arg:?} is not enabled for one-time approval."
            ));
        }
        index += 1;
    }
    if verb == "merge" && positional != 1 {
        return Err("`gh pr merge` needs a pull-request number or URL.".to_string());
    }
    if verb == "create" && positional != 0 {
        return Err(
            "`gh pr create` does not accept positional arguments in this approval flow."
                .to_string(),
        );
    }
    if verb == "create"
        && !args
            .iter()
            .any(|arg| matches!(arg.as_str(), "--title" | "-t" | "--fill"))
    {
        return Err(
            "`gh pr create` needs a title or `--fill` so it can run without an interactive prompt."
                .to_string(),
        );
    }
    Ok(())
}

fn run_gh(db: &MemoryDb, argv: &[String]) -> Result<String, String> {
    let binary = which::which("gh").map_err(|_| {
        "the GitHub CLI is not installed on this machine. `gh auth status` would be the test; \
         the operator installs it from https://cli.github.com."
            .to_string()
    })?;

    let mut command = Command::new(&binary);
    // gh otherwise inherits AETHER1's install directory as its repository context. When
    // the operator has nominated a workspace, make the local checkout the default target;
    // remote-only tasks can still name an explicit OWNER/REPO in the invocation.
    let has_workspace = !db
        .get_setting_string(crate::code_workspace::ROOT_SETTING, "")
        .trim()
        .is_empty()
        || !db
            .get_setting_string("graft_selected_project", "")
            .trim()
            .is_empty();
    if has_workspace {
        command.current_dir(crate::code_workspace::root(db)?);
    }
    let mut child = command
        .args(argv)
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
    let query = args.get("query").and_then(Value::as_str).ok_or_else(|| {
        "search_web needs a query: {\"query\": \"your search terms\"}".to_string()
    })?;

    if query.trim().is_empty() {
        return Err("search query cannot be empty".to_string());
    }

    // DuckDuckGo's public API endpoint
    let api_url = format!(
        "https://api.duckduckgo.com/?q={}&format=json&no_html=1",
        urlencoding::encode(query)
    );

    code_perms::check_url(db, &api_url).map_err(|Refusal(why)| why)?;

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

    #[test]
    fn github_write_approval_is_limited_to_supported_pr_actions() {
        assert!(validate_approved_github_write(&[
            "pr".into(),
            "create".into(),
            "--title".into(),
            "Fix".into()
        ])
        .is_ok());
        assert!(
            validate_approved_github_write(&["pr".into(), "create".into(), "--fill".into()])
                .is_ok()
        );
        assert!(validate_approved_github_write(&[
            "pr".into(),
            "merge".into(),
            "42".into(),
            "--squash".into()
        ])
        .is_ok());
        assert!(validate_approved_github_write(&[
            "pr".into(),
            "merge".into(),
            "42".into(),
            "43".into()
        ])
        .is_err());
        assert!(validate_approved_github_write(&["repo".into(), "delete".into()]).is_err());
        assert!(validate_approved_github_write(&[
            "pr".into(),
            "create".into(),
            "--title".into(),
            "Fix".into(),
            "--web".into()
        ])
        .is_err());
    }

    /// The loop this whole change exists for, driven through the same `call` a model's
    /// tool block goes through: create a file, run something that proves it is there,
    /// edit it, run again and see the change. No model involved -- this asserts the
    /// machinery under one, which is the part that can be tested without a graphics card.
    #[test]
    fn it_can_create_edit_and_run_in_the_project_folder() {
        let home = std::env::temp_dir().join(format!("aether1_code_loop_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let project = home.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let db = MemoryDb::open(home.join("db.sqlite")).unwrap();
        db.set_setting(
            crate::code_workspace::ROOT_SETTING,
            &json!(project.to_string_lossy().to_string()),
        )
        .unwrap();
        db.set_setting(crate::code_workspace::ALLOWLIST_SETTING, &json!(["cat"]))
            .unwrap();
        // This test is about the loop -- create, run, edit, run again -- and not about the
        // boundary, so it runs either way: confined where the machine can, and with the
        // operator's unconfined switch where it cannot. A machine with no sandbox is the
        // ordinary case on Windows and on a CI runner.
        if !crate::code_sandbox::detect().confines() {
            db.set_setting(crate::code_sandbox::UNCONFINED_SETTING, &json!(true))
                .unwrap();
        }

        // Both grants off to begin with, which is how a fresh install ships.
        let refused = call(
            &db,
            "edit_file",
            &json!({"path": "x", "find": "a", "replace": "b"}),
        )
        .unwrap_err();
        assert!(refused.contains("permission off"), "{refused}");

        code_perms::set(&db, Grant::Edit, true).unwrap();
        code_perms::set(&db, Grant::Run, true).unwrap();

        let _guard = crate::tools::fs_guard::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var("HOME").ok();
        std::env::set_var("HOME", &home);

        let made = call(
            &db,
            "create_file",
            &json!({"path": "note.txt", "content": "before\n"}),
        )
        .unwrap();
        assert!(made.contains("created"), "{made}");

        let ran = call(&db, "run", &json!({"argv": ["cat", "note.txt"]})).unwrap();
        assert!(ran.contains("before"), "{ran}");

        let edited = call(
            &db,
            "edit_file",
            &json!({"path": "note.txt", "find": "before", "replace": "after"}),
        )
        .unwrap();
        assert!(edited.contains("note.txt updated"), "{edited}");

        let again = call(&db, "run", &json!({"argv": ["cat", "note.txt"]})).unwrap();
        assert!(
            again.contains("after"),
            "the edit is what the command sees: {again}"
        );

        // And the boundary holds from in here too.
        let out = call(
            &db,
            "create_file",
            &json!({"path": "../escape.txt", "content": "no"}),
        )
        .unwrap_err();
        assert!(out.contains("outside the project folder"), "{out}");

        match previous {
            Some(p) => std::env::set_var("HOME", p),
            None => std::env::remove_var("HOME"),
        }
    }

    /// The catalog is the model's whole picture of what it can do, so a switched-off
    /// permission has to disappear from it rather than sit there being refused.
    #[test]
    fn switching_a_permission_off_removes_it_from_the_catalog() {
        let db = db();
        // The two changing grants start off, so the default catalog is the reads only.
        assert_eq!(
            catalog(&db).len(),
            CAPABILITIES
                .iter()
                .filter(|cap| cap.grant.default_on())
                .count()
        );
        code_perms::set(&db, Grant::Edit, true).unwrap();
        code_perms::set(&db, Grant::Run, true).unwrap();
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
