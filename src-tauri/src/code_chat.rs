//! A separate coding conversation backed by the installed coding model.
//!
//! It shares neither persona nor history with the companion. It does have its own bounded
//! repository tools: reads and read-only GitHub access, plus workspace-scoped file edits and
//! sandboxed commands when the corresponding permissions are enabled. `code_tools` and
//! `code_workspace` enforce those capabilities on every call; prompt wording is not the
//! boundary.
//!
//! Replies can also contain shell commands. `commands_in` turns those into buttons that type
//! into the operator's terminal without pressing Return. This terminal hand-off is separate
//! from the model's `run` tool, which executes only inside the configured project boundary.
//! Neither path can press keys or read terminal output. The Agent Browser uses this same
//! coding-model and permission path with backend-defined task profiles.

use serde::Serialize;

use crate::code_tools;
use crate::llm::providers::{self, ChatContext, Sink};
use crate::llm::{MemoryDb, Message};
use crate::model_scanner::LocalApi;
use crate::tools::protocol;

/// The conversation this panel keeps, apart from the companion's own sessions.
///
/// A session id rather than a table of its own: `MemoryDb` already stores messages by
/// session, the history list already derives itself from what has been said, and a second
/// storage shape for the same thing would be two things to migrate later.
pub const SESSION_ID: &str = "aether-code";

/// How many past turns go back to the model.
///
/// Shorter than the companion's, deliberately. A coding model at the sizes this machine can
/// run is holding a file in its head as well as the conversation, and the turn that matters
/// is nearly always the last one; six keeps the thread of a follow-up without spending the
/// context window on a question from twenty minutes ago.
const HISTORY_TURNS: u32 = 6;

/// The most commands offered from one reply.
///
/// A model that answers a question with a forty-line install script is not producing forty
/// things to press; it is producing a script, which belongs in a file. The cap keeps the
/// reply readable and is a deliberate ceiling on how much a single answer can put in front
/// of the operator to run.
const MAX_COMMANDS: usize = 10;

/// How many times one question may go round the look-then-answer loop.
///
/// Three, against the companion's larger budget, because the models this panel talks to are
/// the ones that fit on the operator's own graphics card. A small model that has not reached
/// an answer in three rounds is usually looping on a file it cannot find, and a fourth round
/// spends the context window confirming that.
const MAX_TOOL_ROUNDS: usize = 3;

/// The budget once it can edit and run. Enough for a real loop -- read, change, build, read
/// the failure, change again, build again -- and short enough that a model going in circles
/// stops within one cup of tea rather than overnight.
const ACTING_TOOL_ROUNDS: usize = 25;

/// A language tag on a fenced block that means "this is for a shell".
///
/// `console` and `shell-session` are included because that is what a model trained on
/// documentation writes when it is showing a transcript, and excluded from nothing else:
/// the `$ ` those carry is stripped below.
const SHELL_TAGS: &[&str] = &[
    "bash",
    "sh",
    "shell",
    "zsh",
    "fish",
    "console",
    "shell-session",
    "terminal",
    "",
];

/// A reply from the coding model, and what of it can be run.
#[derive(Debug, Clone, Serialize)]
pub struct CodeReply {
    pub text: String,
    /// The shell commands in it, in the order they were written, each one a single line
    /// that the terminal button types verbatim.
    pub commands: Vec<String>,
}

/// What the coding model is told it is, before anything the operator says.
///
/// The house rules go in wholesale -- they are the same ones `aether1 code conventions`
/// writes to an `AGENTS.md`, and the whole reason that file exists is that a local model
/// writes like the repository only when the repository's habits are in front of it.
///
/// The paragraph about commands is not decoration. `commands_in` turns each line of a shell
/// block into a button, so a model that answers with a wall of prose containing a command
/// mid-sentence produces a reply with nothing to press. Saying how to format it is cheaper
/// and more reliable than a parser clever enough not to need it.
pub fn system_prompt(db: &MemoryDb, os: &str, model: &str) -> String {
    let base = format!(
        "You are the coding assistant inside AETHER1, a companion HUD running on {os}. You \
         are {model}, running locally on this machine. The person you are talking to is the \
         operator of that machine, working on their own code.\n\n\
         Answer the question asked. Prefer a short answer with the code in it to a long one \
         about the code. If you do not know, say so -- a guess that looks like an answer \
         costs more than an admission.\n\n\
         When a project folder is set, check its AGENTS.md before changing code and follow \
         its repository-specific instructions. For GitHub work, use the gh tool's read-only \
         commands and include --repo OWNER/REPO when the target is not the selected local \
         checkout. Distinguish remote PR contents from the local working tree.\n\n\
         When the next step is something to run, put it in a fenced block tagged `bash`, \
         one command per line, with no prompt marker and no line continuations. Each line \
         becomes a button that types that command into the operator's terminal, so a line \
         that is really two commands, or half of one, is a button that does the wrong \
         thing. Explanations go outside the block, never as a trailing comment inside it."
    );

    let capabilities = if code_tools::any_granted(db) {
        tool_instructions(db, &code_tools::catalog(db))
    } else {
        // Every permission off. The old sentence, which was true of the whole panel before
        // there were any tools and is true again whenever the operator says so.
        "\n\nYou cannot run anything, read the operator's files, or see their terminal. Do \
         not claim to have done either; ask for what you need pasted in."
            .to_string()
    };

    format!(
        "{base}{capabilities}\n\nThe house rules of the codebase you are helping with:\n\n{conventions}",
        conventions = crate::code_setup::conventions()
    )
}

/// How the model asks to look at something, and the one paragraph that keeps it honest
/// about what looking is.
///
/// The text protocol rather than a provider's native tool format, for both shapes this
/// panel talks to. The models here are whatever fits on the operator's machine, native
/// tool support among them is uneven, and a fenced block is something every one of them can
/// produce. `code_tools::call` is the only thing that acts on what comes back, so a model
/// that invents a tool gets a refusal rather than an effect.
fn tool_instructions(db: &MemoryDb, catalog: &[&'static str]) -> String {
    let tools: Vec<String> = catalog.iter().map(|line| format!("- {line}")).collect();

    // The closing paragraph is the one part of these instructions that is not the same
    // every time, because what it says has to be *true*: with both changing grants off
    // this panel still cannot alter anything, and telling a model otherwise is how it ends
    // up claiming to have edited a file it never touched.
    let can_edit = crate::code_perms::granted(db, crate::code_perms::Grant::Edit);
    let can_run = crate::code_perms::granted(db, crate::code_perms::Grant::Run);
    let acting = if can_edit || can_run {
        let where_it_works = match crate::code_workspace::root(db) {
            Ok(root) => format!(
                "The project folder is {}. Paths you give are relative to it.",
                root.display()
            ),
            Err(_) => "No project folder is set yet, so file changes and commands will refuse \
                       until the operator sets one. Tell them that is what you need."
                .to_string(),
        };
        let mut available = Vec::new();
        if can_edit {
            available.push("You can edit or create files, but only inside the project folder.");
        } else {
            available.push("You cannot edit files because Edit is disabled.");
        }
        if can_run {
            available
                .push("You can run commands using the listed run tool and its sandbox policy.");
        } else {
            available.push("You cannot run commands because Run is disabled; provide commands as bash suggestions instead.");
        }
        format!(
            "\n         - {}\n\
             - {}\n\
             - {where_it_works}\n\
             - Work the way a developer does: inspect the existing code and diff, make \
             focused edits when Edit is enabled, and run relevant checks only when Run is \
             enabled. Read the result before deciding the change is complete.\n\
             - Anything outside that folder, and anything that leaves this machine -- \
             pushing, installing system packages, changing settings -- remains unavailable \
             to these tools. Put a suggested command in a ```bash block for the operator. \
             Never claim an edit or check that did not happen.",
            available[0], available[1],
        )
    } else {
        "\n         - Everything above only reads. Nothing you can call changes a file, a \
         repository, a setting or a process, and asking for one that does will be \
         refused.\n\
         - So when the next step would change something -- editing, committing, merging, \
         installing, running a build -- write the command in a ```bash block instead. It \
         becomes a button that types the command into the operator's terminal, and they \
         press Return. That is the only way anything on this machine changes."
            .to_string()
    };

    format!(
        "\n\n[WHAT YOU CAN LOOK AT]\n\
         You can look at things on this machine by calling a tool:\n\n{tools}\n\n\
         To call one, emit a fenced block tagged `tool` containing JSON, and stop:\n\
         ```tool\n\
         {{\"tool\": \"read_file\", \"arguments\": {{\"path\": \"/home/you/project/src/main.rs\"}}}}\n\
         ```\n\n\
         Rules:\n\
         - The results come back as [TOOL RESULTS]. Then answer the operator normally.\n\
         - Call a tool only when you need what it returns. Most questions need none.\n\
         - Never invent a tool or an argument that isn't listed above.\n\
         - Never claim you looked at something you did not call a tool for.{acting}",
        tools = tools.join("\n")
    )
}

/// Whether this panel may change anything at all right now.
pub fn can_act(db: &MemoryDb) -> bool {
    crate::code_perms::granted(db, crate::code_perms::Grant::Edit)
        || crate::code_perms::granted(db, crate::code_perms::Grant::Run)
}

/// How many rounds this question gets.
///
/// Three was right while every tool was a read: a small model that has not answered after
/// looking three times is looping on a file it cannot find, and a fourth round spends the
/// context window confirming that. An edit-and-test loop is a different shape -- read,
/// change, build, read the error, change again -- and three rounds cannot reach the end of
/// one. So the budget follows the capability rather than being one number for both.
fn max_rounds(db: &MemoryDb) -> usize {
    if can_act(db) {
        ACTING_TOOL_ROUNDS
    } else {
        MAX_TOOL_ROUNDS
    }
}

/// The shell commands in a reply, in order, ready to be typed one at a time.
///
/// A pure function of the text for the reason every decision in this project is: the case
/// that matters -- a model writing a block one careless way -- is reproducible in a test
/// and not reproducible against a live model.
///
/// **One line, one command, and never a newline.** A multi-line block is several buttons
/// rather than one, because the button types the text and stops: a paste carrying its own
/// newlines would run every line but the last the moment it landed, which is exactly the
/// thing the operator's Return key is supposed to decide.
pub fn commands_in(markdown: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut in_block = false;
    let mut block_is_shell = false;

    for line in markdown.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("```") {
            if in_block {
                in_block = false;
                block_is_shell = false;
            } else {
                in_block = true;
                // Split on whitespace so ```bash title="x" is still a shell block, and
                // lowercase because a model writes ```Bash often enough to matter.
                let tag = rest.split_whitespace().next().unwrap_or("").to_lowercase();
                block_is_shell = SHELL_TAGS.contains(&tag.as_str());
            }
            continue;
        }
        if !in_block || !block_is_shell {
            continue;
        }
        if let Some(command) = command_on(line) {
            // A model asked twice in one reply to `cd` somewhere repeats itself, and two
            // identical buttons are a worse answer than one.
            if !found.iter().any(|have| have == &command) {
                found.push(command);
            }
            if found.len() == MAX_COMMANDS {
                break;
            }
        }
    }

    found
}

/// One line of a shell block, as something to type -- or nothing, when it is not.
///
/// The three cases that are not commands and look like them: a blank line, a comment, and
/// the `$` or `#` a transcript puts in front of the command to show it is a prompt. The
/// last one is the dangerous one to get wrong: typing `$ ls` runs neither `ls` nor an
/// error worth reading.
fn command_on(line: &str) -> Option<String> {
    let mut text = line.trim();
    if text.is_empty() {
        return None;
    }

    // A transcript's prompt marker. `#` is only a prompt when a space follows, otherwise it
    // is a comment -- and `#!` never is either.
    for marker in ["$ ", "> "] {
        if let Some(rest) = text.strip_prefix(marker) {
            text = rest.trim_start();
            break;
        }
    }

    if text.starts_with('#') || text.is_empty() {
        return None;
    }

    // A line that ends in a backslash is half a command. Typing it alone leaves the shell
    // waiting at a continuation prompt, which reads as the terminal having hung.
    if text.ends_with('\\') {
        return None;
    }

    Some(text.to_string())
}

/// Asks the coding model, streaming the reply as it arrives.
///
/// The endpoint, the API shape and the model all come from the caller rather than from a
/// setting, because they are the ones `code_setup::advise` already worked out -- the server
/// that answered and the best coding model on it, which is not the model the companion is
/// configured to use and must not be confused with it.
pub fn ask(
    db: &MemoryDb,
    endpoint: &str,
    api: LocalApi,
    model: &str,
    os: &str,
    prompt: &str,
    sink: Sink,
) -> Result<CodeReply, String> {
    ask_in_session(db, endpoint, api, model, os, prompt, sink, SESSION_ID)
}

/// Asks in an independent history, used for simultaneous Agent Browser analyses.
#[allow(clippy::too_many_arguments)]
pub fn ask_in_session(
    db: &MemoryDb,
    endpoint: &str,
    api: LocalApi,
    model: &str,
    os: &str,
    prompt: &str,
    sink: Sink,
    session_id: &str,
) -> Result<CodeReply, String> {
    let mut history = db
        .get_messages(session_id, HISTORY_TURNS)
        .unwrap_or_default();
    let system = system_prompt(db, os, model);

    // What the operator sees and what is stored: the reply with the tool calls taken out,
    // and a trace line where each one happened.
    let mut visible = String::new();
    let mut current_prompt = prompt.to_string();

    for _round in 0..max_rounds(db) {
        let ctx = ChatContext {
            system_prompt: &system,
            history: &history,
            prompt: &current_prompt,
            agent_name: "AETHER CODE",
            // The text protocol, on both shapes. Nothing is passed as a provider tool
            // definition: see `tool_instructions`.
            tools: &[],
            exchanges: &[],
        };

        let mut filter = ToolFenceFilter::new();
        let completion = {
            let mut round_sink = |delta: &str| {
                let shown = filter.push(delta);
                if !shown.is_empty() {
                    visible.push_str(&shown);
                    sink(&shown);
                }
            };
            match api {
                LocalApi::Native => {
                    providers::stream_ollama(endpoint, model, &ctx, &mut round_sink)
                }
                LocalApi::OpenAi => providers::stream_openai_compatible(
                    crate::llm::Provider::LmStudio,
                    endpoint,
                    "",
                    model,
                    &ctx,
                    &mut round_sink,
                ),
            }
        }?;
        let tail = filter.finish();
        if !tail.is_empty() {
            visible.push_str(&tail);
            sink(&tail);
        }

        let raw = completion.text;
        let calls = protocol::parse_calls(&raw);
        if calls.is_empty() {
            break;
        }

        let mut results = Vec::new();
        for call in &calls {
            // What it looked at, shown where it happened. The operator reading back a
            // reply that quotes their own file should be able to see that it was read
            // rather than guessed.
            let trace = protocol::trace_line(call);
            visible.push_str(&trace);
            sink(&trace);
            results.push((
                call.tool.clone(),
                code_tools::call(db, &call.tool, &call.arguments),
            ));
        }

        // The round goes into the history so the next one can see what it asked for and
        // what came back. These live for this turn only; what is stored at the end is the
        // question and the visible answer.
        history.push(Message {
            sender: "user".to_string(),
            text: current_prompt,
            timestamp: String::new(),
        });
        history.push(Message {
            sender: "assistant".to_string(),
            text: raw,
            timestamp: String::new(),
        });
        current_prompt = protocol::format_results(&results);
    }

    let text = visible.trim().to_string();

    // Written down only once it arrived. A turn that failed halfway is not history the next
    // question should be answered against.
    let _ = db.add_message(session_id, "user", prompt);
    let _ = db.add_message(session_id, "agent", &text);

    let commands = commands_in(&text);
    Ok(CodeReply { text, commands })
}

/// Holds back `tool` blocks while they stream, and lets every other fenced block through.
///
/// The companion's `protocol::FenceFilter` swallows *every* fenced block, which is right
/// for an avatar that answers in prose and wrong here by exactly the thing this panel is
/// for: a bash block is the button, a rust block is the answer, and a filter that ate both
/// would leave the operator watching a reply about code with the code removed. So this one
/// reads the tag on the opening fence and only swallows `tool`.
///
/// Streaming is what makes it fiddly. A fence arrives a character at a time, so text is
/// held back exactly as long as it might still turn out to be the start of one, and the
/// tag cannot be judged until its line ends.
#[derive(Default)]
struct ToolFenceFilter {
    holding: String,
    swallowing: bool,
}

impl ToolFenceFilter {
    fn new() -> ToolFenceFilter {
        ToolFenceFilter::default()
    }

    /// Feeds one delta in and returns the part of it to show now.
    fn push(&mut self, delta: &str) -> String {
        self.holding.push_str(delta);
        let mut visible = String::new();

        loop {
            if self.swallowing {
                match self.holding.find("```") {
                    Some(end) => {
                        self.holding = self.holding[end + 3..].to_string();
                        self.swallowing = false;
                    }
                    None => {
                        let keep = partial_fence_len(&self.holding);
                        self.holding = self.holding[self.holding.len() - keep..].to_string();
                        return visible;
                    }
                }
                continue;
            }

            let Some(start) = self.holding.find("```") else {
                let keep = partial_fence_len(&self.holding);
                let release = self.holding.len() - keep;
                visible.push_str(&self.holding[..release]);
                self.holding = self.holding[release..].to_string();
                return visible;
            };

            // The tag is the rest of that line, so nothing can be decided until the line
            // ends. Until then the fence stays held -- releasing it early would show the
            // operator the ``` of a tool block whose body is about to vanish.
            let after = &self.holding[start + 3..];
            let Some(newline) = after.find('\n') else {
                visible.push_str(&self.holding[..start]);
                self.holding = self.holding[start..].to_string();
                return visible;
            };

            let tag = after[..newline].trim().to_lowercase();
            if tag == "tool" {
                visible.push_str(&self.holding[..start]);
                self.holding = after[newline + 1..].to_string();
                self.swallowing = true;
            } else {
                // Somebody else's block, including the closing fence of one: released as
                // it was written, up to and including the newline that ended the tag line.
                let consumed = start + 3 + newline + 1;
                visible.push_str(&self.holding[..consumed]);
                self.holding = self.holding[consumed..].to_string();
            }
        }
    }

    /// Releases whatever is still held at the end of a round. An unterminated tool block
    /// stays swallowed: half a tool call is not something to show anybody.
    fn finish(&mut self) -> String {
        if self.swallowing {
            self.holding.clear();
            return String::new();
        }
        std::mem::take(&mut self.holding)
    }
}

/// How many trailing characters could be the beginning of a fence marker.
fn partial_fence_len(text: &str) -> usize {
    let bytes = text.as_bytes();
    for len in (1..3).rev() {
        if bytes.len() >= len && bytes[bytes.len() - len..] == b"```"[..len] {
            return len;
        }
    }
    0
}

/// Forgets the coding conversation. The companion's sessions are untouched.
pub fn clear(db: &MemoryDb) -> Result<(), String> {
    db.delete_session(SESSION_ID)
        .map_err(|e: rusqlite::Error| e.to_string())
}

/// What has been said so far, oldest first, for a panel that was just opened.
pub fn transcript(db: &MemoryDb) -> Vec<Message> {
    db.get_messages(SESSION_ID, 50).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_bash_block_becomes_one_button_per_line() {
        let reply = "Install it first:\n\n```bash\ncargo add serde\ncargo build\n```\n";
        assert_eq!(commands_in(reply), vec!["cargo add serde", "cargo build"]);
    }

    /// The whole reason each line is its own button. A block typed in as one string would
    /// carry its own newlines, and a shell runs a line the instant the newline lands --
    /// so every command but the last would run without anybody pressing Return.
    #[test]
    fn no_command_carries_a_newline() {
        let reply = "```sh\ncd /tmp\nrm -rf build\n```";
        for command in commands_in(reply) {
            assert!(
                !command.contains('\n') && !command.contains('\r'),
                "{command:?} would run itself when typed"
            );
        }
    }

    /// A transcript's prompt marker is not part of the command. Typing `$ ls` runs nothing
    /// and reports an error about a program called `$`.
    #[test]
    fn a_transcript_prompt_is_not_typed() {
        let reply = "```console\n$ git status\n$ git add -p\n```";
        assert_eq!(commands_in(reply), vec!["git status", "git add -p"]);
    }

    #[test]
    fn comments_and_blank_lines_are_not_commands() {
        let reply = "```bash\n# build it\n\ncargo build\n```";
        assert_eq!(commands_in(reply), vec!["cargo build"]);
    }

    /// A shebang is a comment to a shell, and a `#` with no space after it is one too.
    #[test]
    fn a_shebang_is_not_offered_as_a_command() {
        let reply = "```bash\n#!/usr/bin/env bash\necho hi\n```";
        assert_eq!(commands_in(reply), vec!["echo hi"]);
    }

    /// Half a command typed alone leaves the shell at a continuation prompt, which reads
    /// as the terminal having hung rather than as the button having been wrong.
    #[test]
    fn a_continued_line_is_not_offered() {
        let reply = "```bash\ncargo build \\\n  --release\n```";
        assert_eq!(commands_in(reply), vec!["--release"]);
    }

    /// Prose about a command is not a command, and a model that mentions `rm -rf` in a
    /// sentence must not produce a button for it.
    #[test]
    fn nothing_outside_a_fenced_block_is_offered() {
        let reply = "You could run `rm -rf /` but please do not.\n\nTry `ls` instead.";
        assert!(commands_in(reply).is_empty());
    }

    /// A rust block is code to read, not a command to run. Offering `fn main() {` to a
    /// shell is the same mistake as the prompt marker, one layer up.
    #[test]
    fn a_code_block_in_another_language_is_not_a_command() {
        let reply = "```rust\nfn main() {\n    println!(\"hi\");\n}\n```";
        assert!(commands_in(reply).is_empty());
    }

    /// An untagged block is the common case from a small model, and treating it as shell
    /// is the judgement call here: the block is shown in full next to the button, and the
    /// button types rather than runs, so a wrong guess costs a line the operator deletes.
    #[test]
    fn an_untagged_block_is_treated_as_shell() {
        assert_eq!(commands_in("```\nls -la\n```"), vec!["ls -la"]);
    }

    #[test]
    fn a_tag_is_read_whatever_its_case_or_trimmings() {
        assert_eq!(commands_in("```Bash\nls\n```"), vec!["ls"]);
        assert_eq!(commands_in("```bash title=\"x\"\nls\n```"), vec!["ls"]);
    }

    #[test]
    fn the_same_command_twice_is_one_button() {
        let reply = "```bash\ncargo test\n```\nthen again\n```bash\ncargo test\n```";
        assert_eq!(commands_in(reply), vec!["cargo test"]);
    }

    /// A forty-line script is a file, not forty buttons.
    #[test]
    fn a_very_long_script_is_capped() {
        let body: String = (0..40).map(|i| format!("echo {i}\n")).collect();
        let reply = format!("```bash\n{body}```");
        assert_eq!(commands_in(&reply).len(), MAX_COMMANDS);
    }

    #[test]
    fn an_unclosed_block_still_yields_what_it_had() {
        assert_eq!(commands_in("```bash\ncargo build"), vec!["cargo build"]);
    }

    fn db() -> MemoryDb {
        let path = std::env::temp_dir().join(format!(
            "aether1_code_chat_{}_{:?}.db",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&path);
        MemoryDb::open(&path).expect("open test db")
    }

    /// The house rules are what makes a local model write like this repository, and the
    /// whole point of the panel is that they arrive without anybody remembering to paste
    /// them. A prompt that lost them would look identical and produce different code.
    #[test]
    fn the_house_rules_are_in_the_system_prompt() {
        let prompt = system_prompt(&db(), "Linux", "qwen2.5-coder:7b");
        assert!(prompt.contains("qwen2.5-coder:7b"));
        assert!(prompt.contains(crate::code_setup::conventions().trim()));
    }

    /// The model is told how to format because the parser reads what it writes. If this
    /// instruction is dropped the buttons quietly stop appearing.
    #[test]
    fn the_model_is_told_one_command_per_line() {
        assert!(system_prompt(&db(), "Linux", "m").contains("one command per line"));
    }

    /// The prompt has to say both halves, because a model told only what it can do will
    /// try to do the rest of it: it may look, and anything that changes something goes to
    /// the operator's terminal as a button.
    #[test]
    fn the_prompt_says_it_may_look_and_may_not_change_anything() {
        let prompt = system_prompt(&db(), "Linux", "m");
        assert!(prompt.contains("[WHAT YOU CAN LOOK AT]"));
        assert!(prompt.contains("read_file"));
        assert!(prompt.contains("gh"));
        assert!(prompt.contains("```bash block instead"));
    }

    /// With every permission off the panel is what #137 shipped, and the prompt has to say
    /// so -- a model told about tools it does not have spends a round finding that out.
    #[test]
    fn with_every_permission_off_the_prompt_promises_nothing() {
        let db = db();
        for grant in crate::code_perms::ALL {
            crate::code_perms::set(&db, *grant, false).unwrap();
        }
        let prompt = system_prompt(&db, "Linux", "m");
        assert!(!prompt.contains("[WHAT YOU CAN LOOK AT]"));
        assert!(prompt.contains("cannot run anything"));
    }

    #[test]
    fn edit_and_run_permissions_are_described_separately() {
        let db = db();
        crate::code_perms::set(&db, crate::code_perms::Grant::Edit, true).unwrap();
        let prompt = system_prompt(&db, "Linux", "coder");
        assert!(prompt.contains("You can edit or create files"));
        assert!(prompt.contains("Run is disabled"));
        assert!(!prompt.contains("You can run commands using the listed run tool"));

        crate::code_perms::set(&db, crate::code_perms::Grant::Edit, false).unwrap();
        crate::code_perms::set(&db, crate::code_perms::Grant::Run, true).unwrap();
        let prompt = system_prompt(&db, "Linux", "coder");
        assert!(prompt.contains("Edit is disabled"));
        assert!(prompt.contains("You can run commands using the listed run tool"));
        assert!(!prompt.contains("You can edit or create files"));
    }

    fn filtered(text: &str) -> String {
        // One character at a time: the worst case for a streaming filter, and close to
        // what a slow local model actually produces.
        let mut filter = ToolFenceFilter::new();
        let mut out = String::new();
        for ch in text.chars() {
            out.push_str(&filter.push(&ch.to_string()));
        }
        out.push_str(&filter.finish());
        out
    }

    /// The reason this filter exists instead of the companion's. A bash block is the
    /// button and a rust block is the answer; swallowing either would leave the operator
    /// reading a reply about code with the code taken out.
    #[test]
    fn code_blocks_survive_the_filter() {
        let reply = "Try this:\n\n```bash\ncargo build\n```\n\nand in Rust:\n\n```rust\nfn main() {}\n```\n";
        assert_eq!(filtered(reply), reply);
    }

    #[test]
    fn a_tool_block_is_swallowed_and_the_prose_around_it_is_not() {
        let reply = "Let me look.\n```tool\n{\"tool\": \"read_file\", \"arguments\": {\"path\": \"/etc/hostname\"}}\n```\nDone.";
        let shown = filtered(reply);
        assert!(!shown.contains("read_file"), "{shown:?}");
        assert!(shown.starts_with("Let me look."));
        assert!(shown.trim_end().ends_with("Done."));
    }

    /// Half a tool call is not something to show anybody, and a small model that stops
    /// mid-JSON is not a rare event.
    #[test]
    fn an_unterminated_tool_block_stays_swallowed() {
        let shown = filtered("Looking.\n```tool\n{\"tool\": \"read_fi");
        assert_eq!(shown.trim_end(), "Looking.");
    }

    /// An untagged block is a bash block as far as `commands_in` is concerned, so it has
    /// to reach the operator intact rather than being mistaken for a tool call.
    #[test]
    fn an_untagged_block_survives() {
        let reply = "```\nls -la\n```\n";
        assert_eq!(filtered(reply), reply);
    }

    /// The tag is read case-insensitively for the same reason `commands_in` reads its own
    /// that way: a model writes ```Tool often enough to matter.
    #[test]
    fn the_tool_tag_is_read_whatever_its_case() {
        assert!(!filtered("```TOOL\n{}\n```").contains('{'));
    }

    /// This conversation must not land in whichever session the companion is using, and
    /// must not be one the avatar reads back later as something it said.
    #[test]
    fn the_coding_conversation_has_a_session_of_its_own() {
        assert_eq!(SESSION_ID, "aether-code");
        assert_ne!(SESSION_ID, "default");
    }
}
