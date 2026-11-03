// Procedural memory: a job somebody taught AETHER1 once, kept where it can be found again.
//
// A skill is a folder with a `SKILL.md` in it -- YAML frontmatter naming the skill, then
// prose saying how the job is done. The format is the one agentskills.io publishes, so a
// skill written for another agent is readable here without a converter, and so is anything
// from a public skill library.
//
// The question this module answers is **how a skill reaches the model**, and the answer is
// deliberately not the obvious one.
//
// The obvious design puts an index of every installed skill into the system prompt, so the
// model always knows what it has. Every agent that does this then has to ration what the
// index costs: the one this format comes from caps each description at 60 characters and
// silently cuts the rest, because the index is loaded on every turn whether a skill is
// wanted or not.
//
// AETHER1 cannot afford that design and does not need it. `LOCAL_CTX_CEILING` is 16K, and
// the comment there says what it is sized for: a system prompt, a tool round, and the
// history a turn sends. There is no room for a list that grows every time somebody installs
// something, and the machine that would suffer first is the 4GB laptop -- the one the whole
// local-model arithmetic exists to serve.
//
// So discovery is a tool call. `find_skill` costs nothing on the turns that do not use it,
// which is nearly all of them, and in exchange the number of installed skills stops
// mattering: a hundred skills cost what two do, because there is no index to grow. That is
// not a workaround for the context budget. It is a better capability that the budget forced.
//
// Two consequences worth naming, because they are the reason to prefer this over the index:
//
//   * **A skill is prose, not authority.** Whatever a `SKILL.md` says, the model still
//     reaches the machine through the same tools and the same `fs_guard` as before, so
//     installing a skill from a stranger cannot widen what AETHER1 may touch. Agents that
//     ship skills naming shell utilities, and run them with the agent's own authority,
//     cannot say that.
//   * **Reading one needs no tool of its own.** `find_skill` returns the path, and
//     `read_file` already reaches it -- the same rule `notes` keeps for the vault. What
//     `find_skill` does carry is each match's body, truncated to a shared budget: the model
//     asked a question whose answer *is* the prose, and sending it back on the next turn
//     instead would cost a round trip on exactly the small local models this is sized for.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::{Outcome, Tool, ToolContext};
use crate::paths;

/// Where installed skills live, under the app's own data directory.
///
/// Machine-level rather than per-project: a skill is something the operator taught AETHER1,
/// not a property of one repository. Per-project skills belong next to the per-project
/// policy file and are a separate change -- see the note at the end of this module.
pub const DIR_NAME: &str = "skills";

/// The file that makes a folder a skill.
pub const MANIFEST: &str = "SKILL.md";

/// What one `find_skill` call may send back, across every match together.
///
/// A budget shared between matches rather than one per match, because the cost that matters
/// is what lands in the context window, and three skills are no cheaper than one long one.
const MAX_REPLY_BYTES: usize = 6 * 1024;

/// The most matches described in one reply, however much budget is left.
///
/// Past a handful the model is choosing between things it cannot all have read, and a
/// ranked list that long is a sign the query was too vague to answer with prose.
const MAX_MATCHES: usize = 4;

/// Where skills are installed, if the machine has a data directory at all.
pub fn root() -> Option<PathBuf> {
    paths::app_data_dir().map(|dir| dir.join(DIR_NAME))
}

/// One installed skill, as far as searching is concerned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    /// The name from the frontmatter, or the folder's name when the frontmatter has none.
    pub name: String,
    /// The one-line description from the frontmatter. Empty when the skill omits it, which
    /// is worth surfacing rather than hiding: a skill nothing can match is a broken skill.
    pub description: String,
    /// The prose under the frontmatter.
    pub body: String,
    /// The `SKILL.md` itself, so a reply can tell the model what to `read_file`.
    pub manifest: PathBuf,
}

/// Splits a `SKILL.md` into its frontmatter and its body.
///
/// Hand-rolled rather than pulling in a YAML parser, and the shape of a skill's frontmatter
/// is why that is honest rather than lazy: the keys this reads are flat `key: value` lines.
/// A dependency would buy nothing here and the project pins its own supply chain
/// deliberately -- `Cargo.toml` has exactly one HTTP client for the same reason.
///
/// Returns `(frontmatter, body)`. A file with no frontmatter is all body, which is the
/// forgiving reading: prose with no header is still a usable description of a job.
fn split_frontmatter(text: &str) -> (&str, &str) {
    // A leading BOM would stop the delimiter matching and the failure would look like a
    // skill with no frontmatter at all -- confusing, and editors on Windows write them.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rest = match text.strip_prefix("---\n") {
        Some(rest) => rest,
        // Written on Windows, where the delimiter line ends CRLF.
        None => match text.strip_prefix("---\r\n") {
            Some(rest) => rest,
            None => return ("", text),
        },
    };
    for delimiter in ["\n---\n", "\r\n---\r\n", "\n---\r\n", "\r\n---\n"] {
        if let Some(end) = rest.find(delimiter) {
            return (&rest[..end], &rest[end + delimiter.len()..]);
        }
    }
    // An opening delimiter and no closing one. Treating the whole file as frontmatter would
    // lose the prose, which is the part worth having, so treat it as none.
    ("", text)
}

/// Reads one flat `key: value` out of frontmatter.
///
/// Only top-level keys, and only the first occurrence. Nested keys are skipped by the
/// indentation test rather than parsed: a `metadata:` block in a skill written for another
/// agent must not have its children mistaken for top-level fields.
fn frontmatter_value(frontmatter: &str, key: &str) -> Option<String> {
    for line in frontmatter.lines() {
        if line.starts_with(char::is_whitespace) {
            continue;
        }
        let (name, value) = line.split_once(':')?;
        if name.trim() != key {
            continue;
        }
        let value = value.trim();
        // Quoted because the value contains a colon, which the format requires. Strip the
        // pair rather than leaving quotes in prose the operator will read.
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        return Some(value.to_string());
    }
    None
}

/// Reads a skill from its folder. `None` when there is no `SKILL.md` to read.
fn read_skill(folder: &Path) -> Option<Skill> {
    let manifest = folder.join(MANIFEST);
    let text = std::fs::read_to_string(&manifest).ok()?;
    let (frontmatter, body) = split_frontmatter(&text);
    // A folder name is a worse name than a declared one, but it is a real name, and a skill
    // whose frontmatter is malformed is still prose somebody wrote about doing a job.
    let name = frontmatter_value(frontmatter, "name")
        .filter(|n| !n.is_empty())
        .or_else(|| folder.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_default();
    Some(Skill {
        name,
        description: frontmatter_value(frontmatter, "description").unwrap_or_default(),
        body: body.trim().to_string(),
        manifest,
    })
}

/// Every skill installed on this machine, sorted by name.
///
/// A machine with no data directory has no skills, which is the same answer as an empty
/// directory and wants no distinct handling: either way there is no procedure to follow.
pub fn installed() -> Vec<Skill> {
    root().map(|root| installed_in(&root)).unwrap_or_default()
}

/// Every skill under one directory, sorted by name.
///
/// Takes the root rather than finding it, so that what the directory walk does can be
/// tested without an environment to set up -- the house rule about decisions living in
/// functions that take their facts as arguments. `installed` is the one caller that knows
/// where skills live.
///
/// A folder without a `SKILL.md` is skipped rather than reported: the skills directory is
/// somewhere an operator puts things, and a stray folder there is not an error worth
/// interrupting a conversation with.
pub fn installed_in(root: &Path) -> Vec<Skill> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut skills: Vec<Skill> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| read_skill(&entry.path()))
        .collect();
    // Stable order so the same query twice gives the same answer: a model that gets a
    // different ranking for an unchanged question has no way to tell what changed.
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// The words a query and a skill are compared on.
///
/// Lowercased, split on anything that is not alphanumeric, and anything shorter than three
/// characters dropped. The short words are the ones that match everything -- "a", "in",
/// "the", and "my" -- and a scorer that counts them ranks by sentence length instead of by
/// subject.
fn terms(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.chars().count() >= 3)
        .map(str::to_string)
        .collect()
}

/// How well a skill answers a query. Zero means it does not.
///
/// Deliberately crude: a name match counts for more than a description match, and a
/// description match for more than a body match, because the name is what the skill claims
/// to be and the body is where a word can appear in passing. There is no corpus here to
/// weight terms against -- an operator has tens of skills, not thousands -- and a scorer
/// nobody can predict is worse for this than one that is obviously about word overlap.
fn score(skill: &Skill, query: &[String]) -> u32 {
    let name = skill.name.to_lowercase();
    let description = skill.description.to_lowercase();
    let body = skill.body.to_lowercase();
    let mut total = 0;
    for term in query {
        if name.contains(term.as_str()) {
            total += 8;
        }
        if description.contains(term.as_str()) {
            total += 4;
        }
        if body.contains(term.as_str()) {
            total += 1;
        }
    }
    total
}

/// The skills that answer a query, best first.
pub fn search(query: &str) -> Vec<Skill> {
    rank(installed(), query)
}

/// Ranks skills against a query, best first, dropping the ones that do not answer it.
///
/// Separate from `search` so the ranking can be tested on skills built in the test rather
/// than on whatever happens to be installed on the machine running the suite.
pub fn rank(skills: Vec<Skill>, query: &str) -> Vec<Skill> {
    let query = terms(query);
    if query.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(u32, Skill)> = skills
        .into_iter()
        .map(|skill| (score(&skill, &query), skill))
        .filter(|(score, _)| *score > 0)
        .collect();
    // Score first, then name, so ties do not reorder between identical queries.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    scored.into_iter().map(|(_, skill)| skill).collect()
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
    format!(
        "{}\n[truncated -- read the whole skill with read_file]",
        &text[..end]
    )
}

/// Renders matches for the model, inside the shared budget.
fn describe(matches: &[Skill]) -> String {
    let mut out = String::new();
    let mut spent = 0usize;
    for skill in matches.iter().take(MAX_MATCHES) {
        let header = format!(
            "## {}\n{}\n{}\n\n",
            skill.name,
            if skill.description.is_empty() {
                "(this skill declares no description)"
            } else {
                &skill.description
            },
            skill.manifest.display()
        );
        // Whatever is left of the budget, shared out between the matches still to come, so
        // a long first skill cannot spend everything and leave the rest as bare names.
        let remaining = MAX_REPLY_BYTES.saturating_sub(spent + header.len());
        let share = remaining / matches.len().min(MAX_MATCHES);
        let body = truncate(&skill.body, share);
        spent += header.len() + body.len();
        out.push_str(&header);
        out.push_str(&body);
        out.push_str("\n\n");
    }
    if matches.len() > MAX_MATCHES {
        out.push_str(&format!(
            "({} more skills matched -- narrow the task if none of these is right)\n",
            matches.len() - MAX_MATCHES
        ));
    }
    out
}

// ---------------------------------------------------------------- find_skill

pub struct FindSkill;

impl Tool for FindSkill {
    fn name(&self) -> &'static str {
        "find_skill"
    }

    fn description(&self) -> &'static str {
        "Look for an installed skill that says how to do a job -- a procedure the operator has taught AETHER1, in their own words. Call this before working out a multi-step job from first principles: if a skill covers it, its steps are the operator's own and are what they expect to happen. Returns the matching skills with their instructions, and the path to read in full. Nothing is listed anywhere else, so this is the only way to find out what is installed."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "What you are trying to do, in a few words -- for example \"add a note to my Obsidian vault\" or \"cut a release\"."
                }
            },
            "required": ["task"]
        })
    }

    fn mutating(&self) -> bool {
        false
    }

    fn call(&self, args: &Value, _ctx: &ToolContext) -> Result<Outcome, String> {
        let task = args
            .get("task")
            .and_then(Value::as_str)
            .ok_or("missing required string argument \"task\"")?;

        let matches = search(task);
        if matches.is_empty() {
            // Two different nothings, kept apart: no skills installed is a fact about the
            // machine and the model should stop asking, while no match is a fact about the
            // query and a different wording might work.
            let installed = installed().len();
            return Ok(Outcome::text(if installed == 0 {
                "No skills are installed on this machine, so there is no procedure to follow -- work the job out as usual.".to_string()
            } else {
                format!(
                    "None of the {installed} installed skills match that. Work the job out as usual, or try different words."
                )
            }));
        }
        Ok(Outcome::text(describe(&matches)))
    }

    fn preview(&self, args: &Value) -> String {
        match args.get("task").and_then(Value::as_str) {
            Some(task) => format!("look for an installed skill about {task:?}"),
            None => "look for an installed skill".to_string(),
        }
    }
}

// Per-project skills, the ones that would let a repository carry its own procedures and so
// un-compile `code_setup::conventions`, belong beside the per-project policy file in
// `code_policy::DIR`. They are deliberately not here: the search and the format are the
// part worth landing first, and reading a second root is a change to what a *project* is
// trusted to tell the model, which is `code_policy`'s question rather than this module's.

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(name: &str, description: &str, body: &str) -> Skill {
        Skill {
            name: name.to_string(),
            description: description.to_string(),
            body: body.to_string(),
            manifest: PathBuf::from("/tmp").join(name).join(MANIFEST),
        }
    }

    #[test]
    fn frontmatter_and_body_come_apart() {
        let (front, body) = split_frontmatter("---\nname: obsidian\n---\n# Vault\n\nDo the thing.");
        assert_eq!(front, "name: obsidian");
        assert_eq!(body, "# Vault\n\nDo the thing.");
    }

    /// Editors on Windows write both, and a skill that came from one must not read as a
    /// skill with no frontmatter at all.
    #[test]
    fn crlf_and_a_byte_order_mark_are_still_frontmatter() {
        let (front, body) =
            split_frontmatter("\u{feff}---\r\nname: cut-a-release\r\n---\r\nSteps.");
        assert_eq!(front, "name: cut-a-release");
        assert_eq!(body, "Steps.");
    }

    #[test]
    fn a_file_with_no_frontmatter_is_all_body() {
        let (front, body) = split_frontmatter("# Just prose\n");
        assert_eq!(front, "");
        assert_eq!(body, "# Just prose\n");
    }

    /// An opening delimiter with no closing one: the prose is the part worth keeping, so
    /// the whole file is body rather than the whole file being frontmatter.
    #[test]
    fn an_unterminated_header_does_not_swallow_the_skill() {
        let (front, body) = split_frontmatter("---\nname: broken\nstill going\n");
        assert_eq!(front, "");
        assert_eq!(body, "---\nname: broken\nstill going\n");
    }

    /// The format requires quoting a description that contains a colon. The quotes are
    /// punctuation of the format, not part of the sentence the operator reads.
    #[test]
    fn a_quoted_value_loses_its_quotes() {
        let front = "description: \"Search arXiv: by author or ID.\"";
        assert_eq!(
            frontmatter_value(front, "description").unwrap(),
            "Search arXiv: by author or ID."
        );
    }

    /// A `metadata:` block is in the published format, and its children are indented. A
    /// reader that took them for top-level keys would read another agent's tags as ours.
    #[test]
    fn nested_keys_are_not_read_as_top_level_ones() {
        let front = "name: real\nmetadata:\n  name: nested\n  description: nested\n";
        assert_eq!(frontmatter_value(front, "name").unwrap(), "real");
        assert_eq!(frontmatter_value(front, "description"), None);
    }

    #[test]
    fn short_words_are_not_search_terms() {
        assert_eq!(terms("add a note to my vault"), ["add", "note", "vault"]);
    }

    /// Otherwise every skill matches every query and the ranking is by length.
    #[test]
    fn a_query_of_only_short_words_matches_nothing() {
        let skills = vec![skill("obsidian", "Notes.", "Vault.")];
        assert!(rank(skills, "a to my").is_empty());
    }

    #[test]
    fn the_name_outranks_the_description_and_the_body() {
        let query = terms("release");
        let named = skill("cut-a-release", "Unrelated.", "Unrelated.");
        let described = skill("unrelated", "Cut a release.", "Unrelated.");
        let mentioned = skill("unrelated", "Unrelated.", "Mentions a release in passing.");
        assert!(score(&named, &query) > score(&described, &query));
        assert!(score(&described, &query) > score(&mentioned, &query));
    }

    #[test]
    fn a_skill_that_does_not_match_scores_zero() {
        assert_eq!(
            score(&skill("obsidian", "Notes.", "Vault."), &terms("docker")),
            0
        );
    }

    /// The whole reason discovery is a tool call: what comes back is bounded, so the number
    /// of installed skills cannot grow what a turn costs.
    #[test]
    fn a_reply_stays_inside_its_budget() {
        let long = "x".repeat(40 * 1024);
        let matches: Vec<Skill> = (0..6)
            .map(|i| skill(&format!("skill-{i}"), "A description.", &long))
            .collect();
        let rendered = describe(&matches);
        assert!(
            rendered.len() <= MAX_REPLY_BYTES + 512,
            "rendered {} bytes",
            rendered.len()
        );
    }

    /// A long first skill must not spend the whole budget and leave the rest as names.
    #[test]
    fn every_match_gets_some_of_the_budget() {
        let matches = vec![
            skill("first", "A description.", &"x".repeat(40 * 1024)),
            skill("second", "A description.", "The second skill's own steps."),
        ];
        let rendered = describe(&matches);
        assert!(rendered.contains("The second skill's own steps."));
    }

    #[test]
    fn more_matches_than_fit_are_counted_rather_than_dropped_silently() {
        let matches: Vec<Skill> = (0..MAX_MATCHES + 2)
            .map(|i| skill(&format!("skill-{i}"), "A description.", "Steps."))
            .collect();
        assert!(describe(&matches).contains("2 more skills matched"));
    }

    /// A directory to install skills into, named after the test using it so that tests
    /// running in parallel cannot see each other's.
    struct Library(PathBuf);

    impl Library {
        fn new(test: &str) -> Library {
            let dir =
                std::env::temp_dir().join(format!("aether1-skills-{}-{test}", std::process::id()));
            std::fs::remove_dir_all(&dir).ok();
            std::fs::create_dir_all(&dir).unwrap();
            Library(dir)
        }

        fn install(&self, folder: &str, manifest: &str) -> &Library {
            let path = self.0.join(folder);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join(MANIFEST), manifest).unwrap();
            self
        }

        fn mkdir(&self, folder: &str) -> &Library {
            std::fs::create_dir_all(self.0.join(folder)).unwrap();
            self
        }
    }

    impl Drop for Library {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    /// The directory walk itself, which is the part no amount of parsing tests covers: a
    /// real folder on a real disk, read the way the running app reads it.
    #[test]
    fn a_skill_on_disk_is_found_by_what_it_is_for() {
        let library = Library::new("found");
        library.install(
            "cut-a-release",
            "---\nname: cut-a-release\ndescription: Tag a release and check the assets.\n---\nTag as v0-yymmdd-PR.\n",
        );
        let found = rank(installed_in(&library.0), "cut a release");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "cut-a-release");
        assert_eq!(found[0].description, "Tag a release and check the assets.");
        assert_eq!(found[0].body, "Tag as v0-yymmdd-PR.");
        assert!(found[0].manifest.ends_with("cut-a-release/SKILL.md"));
    }

    /// A skill whose frontmatter is malformed is still prose about doing a job, so it keeps
    /// its folder's name rather than disappearing from the library.
    #[test]
    fn a_skill_with_no_declared_name_falls_back_to_its_folder() {
        let library = Library::new("nameless");
        library.install("nameless", "# No header here\n");
        let installed = installed_in(&library.0);
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].name, "nameless");
        assert_eq!(installed[0].body, "# No header here");
    }

    /// The skills directory is somewhere the operator puts things, so a stray folder is
    /// skipped rather than reported as a broken skill.
    #[test]
    fn a_folder_without_a_manifest_is_not_a_skill() {
        let library = Library::new("stray");
        library
            .install("real", "---\nname: real\n---\nSteps.\n")
            .mkdir("notes");
        let installed = installed_in(&library.0);
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].name, "real");
    }

    #[test]
    fn a_directory_that_does_not_exist_has_no_skills() {
        assert!(installed_in(Path::new("/definitely/not/here")).is_empty());
    }

    /// Same query twice, same order: a model given a different ranking for an unchanged
    /// question has no way to tell what changed.
    #[test]
    fn skills_come_back_in_a_stable_order() {
        let library = Library::new("order");
        for name in ["zebra", "alpha", "middle"] {
            library.install(
                name,
                &format!("---\nname: {name}\ndescription: A release task.\n---\nSteps.\n"),
            );
        }
        let names: Vec<String> = installed_in(&library.0)
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, ["alpha", "middle", "zebra"]);
        let ranked: Vec<String> = rank(installed_in(&library.0), "release")
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(ranked, ["alpha", "middle", "zebra"]);
    }

    /// Nothing about finding a skill changes the machine, and the operator agreeing to
    /// "stop asking about this" is coherent for a read of a folder they filled themselves.
    #[test]
    fn finding_a_skill_is_read_only_and_pre_approvable() {
        assert!(!FindSkill.mutating());
        assert!(FindSkill.always_allowable());
    }

    #[test]
    fn the_preview_names_the_task() {
        assert_eq!(
            FindSkill.preview(&json!({"task": "cut a release"})),
            "look for an installed skill about \"cut a release\""
        );
    }
}
