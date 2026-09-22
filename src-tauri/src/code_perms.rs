//! What AETHER CODE is allowed to do, and the one thing it is never allowed to do.
//!
//! #137 gave the coding model a panel and a way to put a command in front of the operator.
//! It had no capabilities of its own: no files, no `gh`, no network. That was the right
//! place to stop for one step and the wrong place to stay -- a coding assistant that cannot
//! see the file you are asking about, or the pull request you are asking about, spends its
//! answers asking you to paste things in.
//!
//! This module is the permission model that lets it look, stated as four rules rather than
//! as whatever the code happens to do:
//!
//!   1. **Three grants, each named, each a switch.** The system (files and what this
//!      machine is), the GitHub CLI, and the internet. Each is read separately and can be
//!      turned off separately, from `aether1 code perms` or the HUD.
//!   2. **Reads run. Writes never do.** Every capability here is read-only *by
//!      construction*, not by intention: there is no tool that writes a file, and `gh` is
//!      checked against a table of subcommands that only look. A request to change
//!      something is refused with the command written out, so it can go to the terminal as
//!      a button.
//!   3. **The terminal is the only thing that changes this machine, and the operator's
//!      Return key is the only thing that runs the terminal.** That boundary is older than
//!      this module (`scripts/check_terminal_isolation.sh`) and this module does not touch
//!      it. Nothing here can type, press, or reach a shell: `gh` is spawned directly with
//!      an argv, so there is no shell to hand a pipe or a redirect to.
//!   4. **Fail closed.** A subcommand this file has not heard of is a refusal, not a guess.
//!      `gh` grows new verbs faster than this table does, and the cost of being wrong in
//!      the permissive direction is somebody's repository.
//!
//! The grants default to on, because the operator asked for these three things to be
//! allowed and a permission you have to go and switch on after asking for it is a worse
//! answer than the one they asked for. What does *not* default to on -- what does not exist
//! at all -- is any way to write.

use crate::llm::MemoryDb;

/// One capability, as the operator talks about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grant {
    /// Reading this machine: files it may read, folders, what the hardware is.
    System,
    /// The GitHub CLI, read-only subcommands only.
    Github,
    /// Fetching a public page, for documentation the model does not carry.
    Internet,
}

/// Every grant, in the order they are shown.
pub const ALL: &[Grant] = &[Grant::System, Grant::Github, Grant::Internet];

impl Grant {
    /// The word the operator types, and the stem of the settings key.
    pub fn key(self) -> &'static str {
        match self {
            Grant::System => "system",
            Grant::Github => "github",
            Grant::Internet => "internet",
        }
    }

    /// The settings row. Prefixed so these can never be confused with the companion's own
    /// permissions -- `tools_enabled` governs what the *avatar* may do, and the two
    /// question are answered separately on purpose.
    pub fn setting(self) -> &'static str {
        match self {
            Grant::System => "code_perm_system",
            Grant::Github => "code_perm_github",
            Grant::Internet => "code_perm_internet",
        }
    }

    /// One line, addressed to the operator, for a settings row or `aether1 code perms`.
    pub fn description(self) -> &'static str {
        match self {
            Grant::System => "Read files and folders on this machine, and what the hardware is",
            Grant::Github => "Run read-only GitHub CLI commands (gh pr view, gh run list, ...)",
            Grant::Internet => "Fetch a public page when it needs documentation",
        }
    }

    /// Parses what the operator typed. Exact, lowercase, no abbreviations: a permission
    /// that can be granted by a near-miss is not a permission.
    pub fn from_key(key: &str) -> Option<Grant> {
        ALL.iter().copied().find(|g| g.key() == key)
    }
}

/// Whether this capability is on right now.
///
/// Read fresh at every call rather than once per turn: the operator can switch one off
/// mid-conversation, and a grant cached at the top of a turn would stay open for the rest
/// of it.
pub fn granted(db: &MemoryDb, grant: Grant) -> bool {
    db.get_setting_bool(grant.setting(), true)
}

/// Turns one on or off.
pub fn set(db: &MemoryDb, grant: Grant, on: bool) -> Result<(), String> {
    db.set_setting(grant.setting(), &serde_json::json!(on))
        .map_err(|e| format!("cannot save {}: {e}", grant.setting()))
}

/// Why a request was refused, written to be read by the model.
///
/// Every refusal says what to do instead, and for a write that instruction is always the
/// same one: put it in a fenced block so it becomes a button. A refusal that only says no
/// gets argued with; one that says no and points at the door gets followed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal(pub String);

impl Refusal {
    fn off(grant: Grant) -> Refusal {
        Refusal(format!(
            "the operator has turned the `{}` permission off, so this is not available. \
             Tell them what you wanted it for; they can turn it back on with \
             `aether1 code perms {} on`.",
            grant.key(),
            grant.key()
        ))
    }

    /// The write refusal. This wording is the whole policy in two sentences, and it is the
    /// only thing the model is told about how to make something happen.
    fn write(what: &str) -> Refusal {
        Refusal(format!(
            "refused: {what} would change something, and nothing here is allowed to change \
             anything. Writes happen in the operator's terminal and nowhere else. Put the \
             command in a fenced ```bash block instead -- it becomes a button that types it \
             into their terminal, and they press Return."
        ))
    }
}

/// Checks a grant, for a tool that needs nothing else checked.
pub fn require(db: &MemoryDb, grant: Grant) -> Result<(), Refusal> {
    if granted(db, grant) {
        Ok(())
    } else {
        Err(Refusal::off(grant))
    }
}

// ------------------------------------------------------------------ the gh classifier

/// The `gh` subcommands that only look, as `(command, verb)`.
///
/// Read as a whitelist and nothing else: a pair that is not in this table is refused,
/// whatever it does. The rule for adding a row is the same one `run_command`'s allowlist
/// uses one level up -- **no argument to this pair can change anything** -- and it is why
/// some obvious-looking reads are missing:
///
/// * `repo clone`, `run download`, `release download` write files, so they are writes here
///   even though they only read from GitHub.
/// * `browse` opens a browser. Harmless and still an action on the operator's desktop,
///   which is not this panel's to take.
/// * `auth token` prints a credential into a conversation that is stored in the database.
///   The one read in this whole file that is refused for what it returns rather than for
///   what it does.
/// * `pr checkout` moves the operator's working tree under them.
const READ_ONLY_GH: &[(&str, &str)] = &[
    ("auth", "status"),
    ("cache", "list"),
    ("gist", "list"),
    ("gist", "view"),
    ("issue", "list"),
    ("issue", "status"),
    ("issue", "view"),
    ("label", "list"),
    ("org", "list"),
    ("pr", "checks"),
    ("pr", "diff"),
    ("pr", "list"),
    ("pr", "status"),
    ("pr", "view"),
    ("project", "list"),
    ("project", "view"),
    ("release", "list"),
    ("release", "view"),
    ("repo", "list"),
    ("repo", "view"),
    ("ruleset", "list"),
    ("ruleset", "view"),
    ("run", "list"),
    ("run", "view"),
    ("search", "code"),
    ("search", "commits"),
    ("search", "issues"),
    ("search", "prs"),
    ("search", "repos"),
    ("variable", "list"),
    ("workflow", "list"),
    ("workflow", "view"),
];

/// `gh` subcommands that take no verb and only report.
const READ_ONLY_GH_BARE: &[&str] = &["status", "version"];

/// Checks a `gh` invocation, given its arguments exactly as they will be passed to the
/// program -- no shell, so this is the whole of what will run.
pub fn check_gh(db: &MemoryDb, args: &[String]) -> Result<(), Refusal> {
    require(db, Grant::Github)?;

    // A help or version flag anywhere makes the whole invocation a page of text, whatever
    // else is on the line: `gh pr merge --help` explains merging rather than merging.
    if args
        .iter()
        .any(|a| a == "--help" || a == "-h" || a == "--version")
    {
        return Ok(());
    }

    let mut rest = args.iter().map(String::as_str);
    let Some(command) = rest.next() else {
        return Err(Refusal(
            "gh needs a subcommand -- `gh pr list`, `gh run view 123`, and so on.".to_string(),
        ));
    };

    // The subcommand comes first, and the verb comes straight after it. That is how gh is
    // written everywhere and it is the only shape this classifier will read, because a
    // flag in front of the verb may or may not carry a value of its own -- `gh pr --repo x
    // list` and `gh pr --draft list` put a different token in the same position, and a
    // classifier that guesses wrong reads `list` where the operator wrote something else.
    if command.starts_with('-') {
        return Err(Refusal(format!(
            "put the subcommand first: `gh pr list --repo x`, not `gh {command} ...`."
        )));
    }

    if READ_ONLY_GH_BARE.contains(&command) {
        return Ok(());
    }

    // `gh api` is the one that cannot be judged by its verb, because the verb is a flag.
    if command == "api" {
        return check_gh_api(rest.collect::<Vec<&str>>().as_slice());
    }

    let Some(verb) = rest.next().filter(|v| !v.starts_with('-')) else {
        return Err(Refusal(format!(
            "`gh {command}` needs the thing you want to look at, directly after it -- \
             `gh {command} list`, `gh {command} view <id>`."
        )));
    };

    if READ_ONLY_GH.contains(&(command, verb)) {
        return Ok(());
    }

    Err(Refusal::write(&format!("`gh {command} {verb}`")))
}

/// `gh api`, where the method is an argument.
///
/// Anything that is not a plain GET is a write, and the flags that *imply* a POST count as
/// asking for one: `gh api -f name=x /repos/...` sends a body without ever naming a method.
/// This is the case where reading the verb would have been exactly wrong.
fn check_gh_api(args: &[&str]) -> Result<(), Refusal> {
    let mut index = 0;
    while index < args.len() {
        let arg = args[index];
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) => (flag, Some(value)),
            None => (arg, None),
        };

        match flag {
            "-X" | "--method" => {
                let method = inline
                    .or_else(|| args.get(index + 1).copied())
                    .unwrap_or_default();
                if !method.eq_ignore_ascii_case("GET") && !method.eq_ignore_ascii_case("HEAD") {
                    return Err(Refusal::write(&format!("`gh api --method {method}`")));
                }
                if inline.is_none() {
                    index += 1;
                }
            }
            // Every one of these puts a body on the request, which makes it a write
            // whatever the method says.
            "-f" | "--raw-field" | "-F" | "--field" | "--input" => {
                return Err(Refusal::write(&format!("`gh api {flag}`")));
            }
            _ => {}
        }
        index += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------- the internet check

/// Checks a URL the model wants to fetch.
///
/// Three things are checked and they are all about *where*, never about what comes back:
///
/// * The grant, like everything else here.
/// * Local-only mode, which is the operator's stated "nothing leaves this machine" and
///   outranks a permission they left on by default (`local_only.rs`).
/// * That the address is actually on the internet. A grant to read the internet is not a
///   grant to read `http://127.0.0.1:11434` or the printer down the hall, and a model that
///   can fetch a loopback URL can reach every unauthenticated service on this machine.
pub fn check_url(db: &MemoryDb, url: &str) -> Result<(), Refusal> {
    require(db, Grant::Internet)?;

    if crate::local_only::enabled(db) {
        return Err(Refusal(
            "local-only mode is on, so nothing in AETHER1 talks to the internet. Answer from \
             what you know, or ask the operator to paste the page in."
                .to_string(),
        ));
    }

    let parsed = url::Url::parse(url.trim())
        .map_err(|e| Refusal(format!("{url} is not a URL I can fetch: {e}")))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(Refusal(format!(
            "only http and https can be fetched, not {}.",
            parsed.scheme()
        )));
    }
    if crate::local_only::is_local_endpoint(parsed.as_str()) {
        return Err(Refusal(
            "that address is on this machine or this network, and the internet permission is \
             for public pages. Ask the operator to run it in their terminal instead."
                .to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> MemoryDb {
        let path = std::env::temp_dir().join(format!(
            "aether1_code_perms_{}_{:?}.db",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&path);
        MemoryDb::open(&path).expect("open test db")
    }

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(String::from).collect()
    }

    /// The operator asked for these three to be allowed, so they are allowed without
    /// anybody having to go and find a switch first.
    #[test]
    fn the_three_grants_start_on() {
        let db = db();
        for grant in ALL {
            assert!(granted(&db, *grant), "{} should start on", grant.key());
        }
    }

    #[test]
    fn a_grant_can_be_turned_off_and_back_on() {
        let db = db();
        set(&db, Grant::Github, false).unwrap();
        assert!(!granted(&db, Grant::Github));
        assert!(granted(&db, Grant::System), "one switch, not all three");
        set(&db, Grant::Github, true).unwrap();
        assert!(granted(&db, Grant::Github));
    }

    #[test]
    fn a_refusal_for_an_off_grant_says_how_to_turn_it_on() {
        let db = db();
        set(&db, Grant::System, false).unwrap();
        let Err(Refusal(message)) = require(&db, Grant::System) else {
            panic!("a grant that is off must refuse");
        };
        assert!(message.contains("aether1 code perms system on"));
    }

    #[test]
    fn reading_github_is_allowed() {
        let db = db();
        for line in [
            "pr list",
            "pr view 137",
            "pr diff 137",
            "pr checks 137",
            "issue list --state open",
            "run list --limit 5",
            "run view 12345 --log",
            "repo view TridentSpoon/Aether1",
            "search code --owner TridentSpoon commands_in",
            "auth status",
            "status",
            "pr list --repo TridentSpoon/Aether1",
            "version",
            "--version",
        ] {
            assert!(
                check_gh(&db, &args(line)).is_ok(),
                "gh {line} only looks and should be allowed"
            );
        }
    }

    /// The whole point of the module. Every one of these is a real thing a coding model
    /// offers to do for you, and not one of them may happen without the operator's Return.
    #[test]
    fn writing_to_github_is_refused_and_pointed_at_the_terminal() {
        let db = db();
        for line in [
            "pr merge 137",
            "pr create --fill",
            "pr close 137",
            "pr checkout 137",
            "issue create --title x",
            "issue close 12",
            "repo clone TridentSpoon/Aether1",
            "repo delete TridentSpoon/Aether1",
            "release create v1",
            "run rerun 12345",
            "run download 12345",
            "workflow run build.yml",
            "secret set TOKEN",
            "auth login",
            "auth logout",
            "alias set co 'pr checkout'",
        ] {
            let Err(Refusal(message)) = check_gh(&db, &args(line)) else {
                panic!("gh {line} changes something and must be refused");
            };
            assert!(
                message.contains("```bash"),
                "gh {line} was refused without saying where writes go: {message}"
            );
        }
    }

    /// `gh browse` takes no verb, opens a browser on the operator's desktop, and is
    /// therefore neither a read nor something this panel gets to do.
    #[test]
    fn opening_a_browser_is_refused() {
        let db = db();
        assert!(check_gh(&db, &args("browse")).is_err());
        assert!(check_gh(&db, &args("browse 137")).is_err());
    }

    /// A credential printed into the panel is stored in the same database as the
    /// conversation. Read-only and still refused, which is why the table is a whitelist
    /// rather than a list of dangerous verbs.
    #[test]
    fn printing_the_auth_token_is_refused() {
        let db = db();
        assert!(check_gh(&db, &args("auth token")).is_err());
    }

    /// `gh api` is the hole in every read/write split done by subcommand: the method is a
    /// flag, so `gh api` looks identical whether it is reading a repository or deleting
    /// one.
    #[test]
    fn gh_api_is_judged_by_its_method_and_not_its_name() {
        let db = db();
        assert!(check_gh(&db, &args("api /repos/TridentSpoon/Aether1")).is_ok());
        assert!(check_gh(&db, &args("api -X GET /rate_limit")).is_ok());
        assert!(check_gh(&db, &args("api --method=GET /rate_limit")).is_ok());

        for line in [
            "api -X DELETE /repos/TridentSpoon/Aether1",
            "api --method PATCH /repos/x/y",
            "api --method=PUT /repos/x/y",
            "api -f title=x /repos/x/y/issues",
            "api --field body=x /repos/x/y/issues",
            "api --input body.json /repos/x/y/issues",
        ] {
            assert!(
                check_gh(&db, &args(line)).is_err(),
                "gh {line} writes and must be refused"
            );
        }
    }

    /// Fail closed. `gh` gains subcommands on its own schedule and this table does not.
    #[test]
    fn an_unknown_subcommand_is_refused() {
        let db = db();
        assert!(check_gh(&db, &args("sparkle everything")).is_err());
        assert!(check_gh(&db, &args("pr sparkle")).is_err());
    }

    /// A flag between the command and the verb is common (`gh --repo x pr list`) and must
    /// not make `pr list` unreadable to the classifier.
    /// Flags go after the verb, which is how everybody writes gh. A flag standing where
    /// the verb should be is refused rather than skipped: `--repo x list` and `--draft
    /// list` put different things in the same position, so skipping is a coin toss over
    /// which word gets classified.
    #[test]
    fn the_verb_must_come_straight_after_the_subcommand() {
        let db = db();
        assert!(check_gh(&db, &args("pr list --repo TridentSpoon/Aether1")).is_ok());
        assert!(check_gh(&db, &args("pr --repo TridentSpoon/Aether1 list")).is_err());
        assert!(check_gh(&db, &args("--repo TridentSpoon/Aether1 pr list")).is_err());
    }

    /// Help is text. Asking gh to explain a write is not asking it to perform one.
    #[test]
    fn help_is_allowed_even_for_a_write_subcommand() {
        let db = db();
        assert!(check_gh(&db, &args("pr merge --help")).is_ok());
    }

    #[test]
    fn github_refuses_entirely_when_the_grant_is_off() {
        let db = db();
        set(&db, Grant::Github, false).unwrap();
        assert!(check_gh(&db, &args("pr list")).is_err());
    }

    #[test]
    fn a_public_page_can_be_fetched() {
        let db = db();
        assert!(check_url(&db, "https://doc.rust-lang.org/std/").is_ok());
    }

    /// The internet grant is for documentation. Pointed at loopback it is a way to read
    /// every unauthenticated service on the operator's own machine, including AETHER1's.
    #[test]
    fn fetching_this_machine_is_not_fetching_the_internet() {
        let db = db();
        for url in [
            "http://127.0.0.1:11434/api/tags",
            "http://localhost:1420/",
            "http://192.168.1.50/",
            "http://nas.local/",
        ] {
            assert!(check_url(&db, url).is_err(), "{url} is not the internet");
        }
    }

    #[test]
    fn only_http_urls_are_fetched() {
        let db = db();
        assert!(check_url(&db, "file:///etc/shadow").is_err());
        assert!(check_url(&db, "ftp://example.com/x").is_err());
    }

    /// Local-only mode is a stated position about this machine, and it outranks a
    /// permission that is merely on because it defaults to on.
    #[test]
    fn local_only_mode_beats_the_internet_grant() {
        let db = db();
        db.set_setting(crate::local_only::SETTING, &serde_json::json!(true))
            .unwrap();
        assert!(granted(&db, Grant::Internet));
        assert!(check_url(&db, "https://doc.rust-lang.org/std/").is_err());
    }
}
