//! Talking to the coding model, and handing what it says to the operator's terminal.
//!
//! Step 49 set a coding agent up and stopped there, on the grounds that driving an
//! edit-and-test loop over a repository is what `opencode` and `aider` already do. That is
//! still true, and this does not undo it. What it adds is the thing that was missing in
//! between: somewhere to *ask* the model that was just downloaded, without leaving the HUD
//! and without a project checked out -- how do I write this, why does this not compile,
//! what is the flag for that -- and a way to get the command it answers with into the
//! terminal without retyping it.
//!
//! **Two rules this module exists to hold.**
//!
//! *The companion's conversation and this one are separate.* Different model, different
//! system prompt, different history (`SESSION_ID` below), no persona, no flow, no tools. A
//! coding question asked of the avatar gets the avatar, which is the right answer for the
//! avatar and the wrong one for a compiler error; and a coding model asked to be a
//! companion is worse at both.
//!
//! *Nothing here runs anything.* `commands_in` reads the shell commands out of a reply so
//! the HUD can offer each one to the terminal as a button, and a press of that button types
//! the command in -- exactly what the keyboard does, byte for byte, with no newline. The
//! operator's own Return key is what runs it. That is not timidity: `terminal.rs` exists
//! because handing a model a shell is the single change that turns a prompt injection into
//! an unrecoverable afternoon, and a model that can put text in front of you is a different
//! thing from one that can execute it. `scripts/check_terminal_isolation.sh` still passes
//! unchanged -- nothing in this file, in `commands.rs`, in `server.rs` or in `tools/` names
//! the terminal module, and the button lives in the native window, which is the only place
//! a terminal exists at all.

use serde::Serialize;

use crate::llm::providers::{self, ChatContext, Sink};
use crate::llm::{MemoryDb, Message};
use crate::model_scanner::LocalApi;

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
pub fn system_prompt(os: &str, model: &str) -> String {
    format!(
        "You are the coding assistant inside AETHER1, a companion HUD running on {os}. You \
         are {model}, running locally on this machine. The person you are talking to is the \
         operator of that machine, working on their own code.\n\n\
         Answer the question asked. Prefer a short answer with the code in it to a long one \
         about the code. If you do not know, say so -- a guess that looks like an answer \
         costs more than an admission.\n\n\
         When the next step is something to run, put it in a fenced block tagged `bash`, \
         one command per line, with no prompt marker and no line continuations. Each line \
         becomes a button that types that command into the operator's terminal, so a line \
         that is really two commands, or half of one, is a button that does the wrong \
         thing. Explanations go outside the block, never as a trailing comment inside it.\n\n\
         You cannot run anything, read the operator's files, or see their terminal. Do not \
         claim to have done either; ask for what you need pasted in.\n\n\
         The house rules of the codebase you are helping with:\n\n{conventions}",
        conventions = crate::code_setup::conventions()
    )
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
    let history = db
        .get_messages(SESSION_ID, HISTORY_TURNS)
        .unwrap_or_default();
    let system = system_prompt(os, model);

    let ctx = ChatContext {
        system_prompt: &system,
        history: &history,
        prompt,
        agent_name: "AETHER CODE",
        // No tools, in either shape. The operator's terminal is the only thing that acts on
        // this machine, and it acts when they press Return.
        tools: &[],
        exchanges: &[],
    };

    let completion = match api {
        LocalApi::Native => providers::stream_ollama(endpoint, model, &ctx, sink),
        LocalApi::OpenAi => providers::stream_openai_compatible(
            crate::llm::Provider::LmStudio,
            endpoint,
            "",
            model,
            &ctx,
            sink,
        ),
    }?;

    let text = completion.text;

    // Written down only once it arrived. A turn that failed halfway is not history the next
    // question should be answered against.
    let _ = db.add_message(SESSION_ID, "user", prompt);
    let _ = db.add_message(SESSION_ID, "agent", &text);

    let commands = commands_in(&text);
    Ok(CodeReply { text, commands })
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

    /// The house rules are what makes a local model write like this repository, and the
    /// whole point of the panel is that they arrive without anybody remembering to paste
    /// them. A prompt that lost them would look identical and produce different code.
    #[test]
    fn the_house_rules_are_in_the_system_prompt() {
        let prompt = system_prompt("Linux", "qwen2.5-coder:7b");
        assert!(prompt.contains("qwen2.5-coder:7b"));
        assert!(prompt.contains(crate::code_setup::conventions().trim()));
    }

    /// The model is told how to format because the parser reads what it writes. If this
    /// instruction is dropped the buttons quietly stop appearing.
    #[test]
    fn the_model_is_told_one_command_per_line() {
        assert!(system_prompt("Linux", "m").contains("one command per line"));
    }

    /// This conversation must not land in whichever session the companion is using, and
    /// must not be one the avatar reads back later as something it said.
    #[test]
    fn the_coding_conversation_has_a_session_of_its_own() {
        assert_eq!(SESSION_ID, "aether-code");
        assert_ne!(SESSION_ID, "default");
    }
}
