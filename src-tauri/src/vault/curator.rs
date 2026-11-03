// Looking over the vault and saying what wants attention.
//
// The vault is a folder of notes and nothing prunes it, so over a year it accumulates the
// things any filing system accumulates: notes nothing points at, notes that were started
// and never written, a year of dailies. None of that is visible from the Memories list,
// which shows what is there rather than what is wrong with it.
//
// What this module is *for* follows from one line in `vault/mod.rs`: "The index is the
// retrieval mechanism. INDEX.md says what is here and which notes matter for which kind of
// question; the model reads it every turn and reaches for the rest with read_file."
//
// That makes reachability the thing worth checking. A note the index does not name and no
// other note links to is not merely untidy -- it is **memory the companion can no longer
// find**. It will sit in the folder being perfectly good information that never reaches a
// conversation again. Nothing else in the app reports that, and age does not reveal it:
// a note written months ago that the index names is working exactly as intended.
//
// **Age is deliberately not treated as staleness.** Hermes's curator archives skills by
// inactivity, which is fair for a procedure nobody runs. These are facts about the
// operator. A note recording an allergy, a machine's graphics card or a standing
// preference has not gone stale by sitting still -- it has settled, and archiving it for
// being old would hide exactly the kind of thing the vault exists to keep. The one place
// where age does mean what it appears to mean is `daily/`, where a note from months ago is
// journal rather than knowledge, so that is the only class this proposes archiving.
//
// **An unresolved link is not a fault here**, and the `Link` doc in `reader.rs` says why:
// "a link to a note that has not been written yet is a normal state of a vault". So those
// are reported as notes worth writing, strongest first, rather than as breakage. A vault
// that mentions `[[allergies]]` four times and has no such note is telling the operator
// something, and it is not that something is broken.
//
// Nothing here changes anything. It reports, and the companion already has `archive_note`
// to act with -- which is mutating, so it is proposed and the operator approves it, the
// rule `notes` keeps for everything written about a person. The curator supplies the one
// thing that was missing, which is knowing what to propose. No model is involved, so this
// costs no tokens, cannot evict a resident model, and gives the same answer twice.

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use serde_json::{json, Value};

use crate::llm::MemoryDb;
use crate::tools::{Outcome, Tool, ToolContext};

/// How old a daily note has to be before it is worth archiving.
///
/// Three months. Short enough that the folder does not become a year of scrollback, long
/// enough that "what was I doing last quarter" is still answerable from the vault proper
/// rather than from `archive/`. Dailies only -- see the note at the top about why age is
/// not applied to anything else.
const STALE_DAILY_DAYS: u64 = 90;

const SECONDS_PER_DAY: u64 = 60 * 60 * 24;

/// How many of each kind of finding to name.
///
/// A review is read by somebody deciding what to do next, and a list of forty notes is one
/// nobody acts on. The counts are reported in full; it is the names that are capped.
const MAX_NAMED: usize = 8;

/// What a note looks like to the review: enough to judge it, and nothing that needs a disk.
///
/// Taking the facts as arguments rather than reading them here is the house rule about
/// decisions living in functions that can be tested without the machine -- and it is what
/// lets the review be checked against a vault built in a test.
pub struct NoteFacts {
    /// Path relative to the vault root, as `collect_notes` reports it.
    pub name: String,
    pub body: String,
    /// Seconds since the epoch.
    pub modified: u64,
}

/// A note nothing can reach, and so memory the companion has lost.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unreachable {
    pub note: String,
}

/// A note the vault keeps referring to and nobody has written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Wanted {
    /// The link target, as written.
    pub target: String,
    /// How many notes mention it. Notes rather than mentions: a note that says
    /// `[[allergies]]` six times wants it once, the same as any other note does.
    pub mentions: usize,
}

/// A daily note old enough to be journal rather than knowledge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StaleDaily {
    pub note: String,
    pub days: u64,
}

/// What a look over the vault found. Empty fields are the good case.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct Review {
    pub notes: usize,
    pub unreachable: Vec<Unreachable>,
    pub wanted: Vec<Wanted>,
    pub empty: Vec<String>,
    pub stale_dailies: Vec<StaleDaily>,
}

impl Review {
    /// Whether anything at all wants attention. A vault in good order is a real answer and
    /// is worth being able to give plainly.
    pub fn is_clean(&self) -> bool {
        self.unreachable.is_empty()
            && self.wanted.is_empty()
            && self.empty.is_empty()
            && self.stale_dailies.is_empty()
    }
}

/// Whether a note has anything in it besides headings, blank lines and list bullets.
///
/// A started-and-abandoned note is `# Allergies` and nothing else. Judging by byte length
/// would catch it too, and would also catch a one-line note that says the whole thing it
/// needs to say -- "Allergic to penicillin." is complete.
fn is_empty(body: &str) -> bool {
    !body.lines().any(|line| {
        let line = line.trim();
        !line.is_empty()
            && !line.starts_with('#')
            && line.trim_start_matches(['-', '*', '+']).trim() != ""
    })
}

/// Everything in `daily/`, judged by the folder the vault itself puts them in.
fn is_daily(name: &str) -> bool {
    name.starts_with("daily/") || name.starts_with("daily\\")
}

/// Looks over a vault and says what wants attention.
///
/// `now` is seconds since the epoch, passed in rather than read so that the stale-daily
/// boundary can be tested without waiting three months.
pub fn review(notes: &[NoteFacts], now: u64) -> Review {
    let names: Vec<String> = notes.iter().map(|n| n.name.clone()).collect();

    // Who points at whom. Counted per *linking note* so that repeating a link in one note
    // does not make its target look more wanted, or its own note look reachable.
    let mut reached: HashSet<String> = HashSet::new();
    let mut wanted: HashMap<String, usize> = HashMap::new();
    for note in notes {
        let mut seen_here: HashSet<String> = HashSet::new();
        for link in super::reader::resolve_links(&super::reader::links_in(&note.body), &names) {
            match link.note {
                // A note linking to itself does not make itself reachable; that is how an
                // orphan with a self-reference would hide from this.
                Some(target) if target != note.name => {
                    reached.insert(target);
                }
                Some(_) => {}
                None => {
                    if seen_here.insert(link.target.to_lowercase()) {
                        *wanted.entry(link.target.clone()).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    let mut review = Review {
        notes: notes.len(),
        ..Default::default()
    };

    for note in notes {
        // The notes loaded into every conversation are reachable by definition, and
        // `archive_destination` already refuses to move them. Flagging them would be
        // advice the rest of the app is built to decline.
        if super::is_core_note(&note.name) {
            continue;
        }
        // Already archived: being unreferenced is what the archive is for.
        if note.name.starts_with("archive/") || note.name.starts_with("archive\\") {
            continue;
        }

        if is_empty(&note.body) {
            review.empty.push(note.name.clone());
        } else if !reached.contains(&note.name) && !is_daily(&note.name) {
            // Empty *and* unreachable is reported once, as empty: writing it is the thing
            // to do, and linking an empty note to the index fixes nothing.
            //
            // Dailies are exempt from reachability, and running this against a real vault
            // is what showed why: nothing ever links to a journal entry, so every daily
            // came back as lost memory. On a vault with a year of them that buries the one
            // finding that matters. A daily is reached by its date and was never something
            // the index was meant to name; the stale-daily check below is the whole of
            // what that folder needs.
            review.unreachable.push(Unreachable {
                note: note.name.clone(),
            });
        }

        if is_daily(&note.name) {
            let age = now.saturating_sub(note.modified) / SECONDS_PER_DAY;
            if age >= STALE_DAILY_DAYS {
                review.stale_dailies.push(StaleDaily {
                    note: note.name.clone(),
                    days: age,
                });
            }
        }
    }

    review.wanted = wanted
        .into_iter()
        .map(|(target, mentions)| Wanted { target, mentions })
        .collect();

    // Stable orders throughout: the same vault reviewed twice gives the same answer, and a
    // model handed a different order for an unchanged vault cannot tell what moved.
    review.unreachable.sort_by(|a, b| a.note.cmp(&b.note));
    review.empty.sort();
    review.wanted.sort_by(|a, b| {
        b.mentions
            .cmp(&a.mentions)
            .then_with(|| a.target.cmp(&b.target))
    });
    review
        .stale_dailies
        .sort_by(|a, b| b.days.cmp(&a.days).then_with(|| a.note.cmp(&b.note)));
    review
}

/// Reads the vault and reviews it.
pub fn review_vault(db: &MemoryDb) -> Review {
    let root = super::vault_path(db);
    let mut names = Vec::new();
    super::collect_notes(&root, &root, &mut names, 0);
    let notes: Vec<NoteFacts> = names
        .into_iter()
        .filter_map(|name| {
            let path = root.join(&name);
            let body = std::fs::read_to_string(&path).ok()?;
            let modified = std::fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            Some(NoteFacts {
                name,
                body,
                modified,
            })
        })
        .collect();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    review(&notes, now)
}

/// One list for the model, named and counted.
fn listed(label: &str, items: &[String], out: &mut String) {
    if items.is_empty() {
        return;
    }
    out.push_str(&format!("{label} ({}): ", items.len()));
    out.push_str(
        &items
            .iter()
            .take(MAX_NAMED)
            .cloned()
            .collect::<Vec<_>>()
            .join(", "),
    );
    if items.len() > MAX_NAMED {
        out.push_str(&format!(", and {} more", items.len() - MAX_NAMED));
    }
    out.push('\n');
}

/// Renders a review for the model, as instructions rather than as a report.
///
/// Each section says what to do with what it found, because a model handed a list of note
/// names without a next step tends to read them all back to the operator.
pub fn describe(review: &Review) -> String {
    if review.is_clean() {
        return format!(
            "The vault is in good order: {} notes, all reachable, nothing empty, no old dailies. Say so and do not propose changes.",
            review.notes
        );
    }

    let mut out = format!("Looked over {} notes.\n\n", review.notes);

    if !review.unreachable.is_empty() {
        let names: Vec<String> = review.unreachable.iter().map(|u| u.note.clone()).collect();
        listed(
            "Nothing links to these, so you will not find them again",
            &names,
            &mut out,
        );
        out.push_str(
            "Add a line for each to INDEX.md, or link it from a note that belongs with it.\n\n",
        );
    }

    if !review.empty.is_empty() {
        listed("Started and never written", &review.empty, &mut out);
        out.push_str("Either write them or archive them.\n\n");
    }

    if !review.wanted.is_empty() {
        let names: Vec<String> = review
            .wanted
            .iter()
            .map(|w| format!("{} ({} notes)", w.target, w.mentions))
            .collect();
        listed("Referred to but never written", &names, &mut out);
        out.push_str(
            "These are not broken links. The vault keeps reaching for them, so they are worth writing when you next learn something that belongs there.\n\n",
        );
    }

    if !review.stale_dailies.is_empty() {
        let names: Vec<String> = review
            .stale_dailies
            .iter()
            .map(|s| format!("{} ({} days)", s.note, s.days))
            .collect();
        listed("Dailies older than three months", &names, &mut out);
        out.push_str(
            "Propose archive_note for these if the operator wants the folder tidied. Never archive anything else for being old: a settled fact is not a stale one.\n",
        );
    }

    out
}

// ------------------------------------------------------------ review_memory

pub struct ReviewMemory;

impl Tool for ReviewMemory {
    fn name(&self) -> &'static str {
        "review_memory"
    }

    fn description(&self) -> &'static str {
        "Look over the memory vault and report what wants attention: notes nothing links to and so cannot be found again, notes started and never written, subjects the vault keeps referring to with no note yet, and dailies old enough to archive. Reads only -- it changes nothing. Use it when the operator asks about the state of their memory or asks you to tidy it, then propose the individual changes."
    }

    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {}, "required": [] })
    }

    fn mutating(&self) -> bool {
        false
    }

    fn call(&self, _args: &Value, ctx: &ToolContext) -> Result<Outcome, String> {
        Ok(Outcome::text(describe(&review_vault(ctx.db))))
    }

    fn preview(&self, _args: &Value) -> String {
        "look over the memory vault and report what wants attention".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(name: &str, body: &str) -> NoteFacts {
        NoteFacts {
            name: name.to_string(),
            body: body.to_string(),
            modified: 0,
        }
    }

    fn aged(name: &str, body: &str, days_old: u64) -> NoteFacts {
        NoteFacts {
            name: name.to_string(),
            body: body.to_string(),
            // Saturating, so a test can say "very old" without having to know what the
            // fixed `now` is or tripping an overflow in the helper rather than the code.
            modified: now().saturating_sub(days_old * SECONDS_PER_DAY),
        }
    }

    fn now() -> u64 {
        1_000 * SECONDS_PER_DAY
    }

    /// The finding the whole module exists for: information the companion can no longer
    /// reach, which no amount of looking at the Memories list reveals.
    #[test]
    fn a_note_nothing_links_to_is_reported_as_lost() {
        let notes = vec![
            note("INDEX.md", "# Index\n\nSee [[profile]].\n"),
            note("profile.md", "Likes tea.\n"),
            note("allergies.md", "Allergic to penicillin.\n"),
        ];
        let review = review(&notes, now());
        assert_eq!(
            review.unreachable,
            [Unreachable {
                note: "allergies.md".to_string()
            }]
        );
    }

    #[test]
    fn a_note_the_index_names_is_not_lost() {
        let notes = vec![
            note("INDEX.md", "# Index\n\n- [[allergies]]\n"),
            note("allergies.md", "Allergic to penicillin.\n"),
        ];
        assert!(review(&notes, now()).unreachable.is_empty());
    }

    /// Otherwise an orphan hides from this by mentioning itself.
    #[test]
    fn linking_to_itself_does_not_make_a_note_reachable() {
        let notes = vec![
            note("INDEX.md", "# Index\n"),
            note("orphan.md", "See [[orphan]] for more.\n"),
        ];
        assert_eq!(review(&notes, now()).unreachable.len(), 1);
    }

    /// The three always-loaded notes reach every conversation by definition, and
    /// `archive_destination` already refuses to move them.
    #[test]
    fn the_notes_loaded_every_turn_are_never_flagged() {
        let notes = vec![
            note("INDEX.md", "# Index\n"),
            note("profile.md", "Likes tea.\n"),
            note("machine.md", "Radeon 6800 XT.\n"),
        ];
        let review = review(&notes, now());
        assert!(review.unreachable.is_empty(), "{:?}", review.unreachable);
    }

    /// Being unreferenced is what the archive is for.
    #[test]
    fn an_archived_note_is_not_expected_to_be_reachable() {
        let notes = vec![
            note("INDEX.md", "# Index\n"),
            note("archive/old.md", "Used to be true.\n"),
        ];
        assert!(review(&notes, now()).unreachable.is_empty());
    }

    #[test]
    fn a_note_with_only_a_heading_was_started_and_never_written() {
        let notes = vec![
            note("INDEX.md", "# Index\n\n- [[stub]]\n"),
            note("stub.md", "# Allergies\n\n- \n"),
        ];
        assert_eq!(review(&notes, now()).empty, ["stub.md"]);
    }

    /// A short note can be a complete note. Length is not the test.
    #[test]
    fn one_line_that_says_the_whole_thing_is_not_empty() {
        let notes = vec![
            note("INDEX.md", "# Index\n\n- [[allergies]]\n"),
            note("allergies.md", "Allergic to penicillin.\n"),
        ];
        let review = review(&notes, now());
        assert!(review.empty.is_empty());
    }

    /// Reported once, as the thing worth doing: linking an empty note fixes nothing.
    #[test]
    fn an_empty_orphan_is_reported_as_empty_and_not_twice() {
        let notes = vec![note("INDEX.md", "# Index\n"), note("stub.md", "# Stub\n")];
        let review = review(&notes, now());
        assert_eq!(review.empty, ["stub.md"]);
        assert!(review.unreachable.is_empty());
    }

    /// The vault's own doc calls an unresolved link a normal state, so these are reported
    /// as subjects worth writing and ranked by how much the vault reaches for them.
    #[test]
    fn a_subject_the_vault_keeps_reaching_for_is_ranked_by_how_many_notes_want_it() {
        let notes = vec![
            note("INDEX.md", "# Index\n\nSee [[a]] and [[b]].\n"),
            note("a.md", "Mentions [[allergies]].\n"),
            note("b.md", "Also [[allergies]] and [[hobbies]].\n"),
        ];
        let wanted = review(&notes, now()).wanted;
        assert_eq!(wanted[0].target, "allergies");
        assert_eq!(wanted[0].mentions, 2);
        assert_eq!(wanted[1].target, "hobbies");
        assert_eq!(wanted[1].mentions, 1);
    }

    /// A note that wants the same thing six times wants it once.
    #[test]
    fn repeating_a_link_in_one_note_does_not_inflate_how_wanted_it_is() {
        let notes = vec![note(
            "INDEX.md",
            "[[allergies]] [[allergies]] [[Allergies]]\n",
        )];
        let wanted = review(&notes, now()).wanted;
        assert_eq!(wanted.len(), 1);
        assert_eq!(wanted[0].mentions, 1);
    }

    #[test]
    fn a_daily_older_than_three_months_is_offered_for_archiving() {
        let notes = vec![
            note("INDEX.md", "# Index\n\n- [[daily/2026-01-01]]\n"),
            aged("daily/2026-01-01.md", "Did some things.\n", 120),
            aged("daily/2026-10-01.md", "Did some things.\n", 10),
        ];
        let stale = review(&notes, now()).stale_dailies;
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].note, "daily/2026-01-01.md");
        assert_eq!(stale[0].days, 120);
    }

    /// The boundary itself, so the threshold cannot drift by a day unnoticed.
    #[test]
    fn the_stale_boundary_is_ninety_days() {
        let just_under = vec![aged("daily/a.md", "Things.\n", STALE_DAILY_DAYS - 1)];
        let just_on = vec![aged("daily/a.md", "Things.\n", STALE_DAILY_DAYS)];
        assert!(review(&just_under, now()).stale_dailies.is_empty());
        assert_eq!(review(&just_on, now()).stale_dailies.len(), 1);
    }

    /// The point of the module doc: a settled fact is not a stale one. An old note outside
    /// `daily/` must never be offered for archiving.
    #[test]
    fn an_old_note_that_is_not_a_daily_is_left_alone() {
        let notes = vec![
            note("INDEX.md", "# Index\n\n- [[allergies]]\n"),
            aged("allergies.md", "Allergic to penicillin.\n", 3_000),
        ];
        let review = review(&notes, now());
        assert!(review.stale_dailies.is_empty());
        assert!(review.unreachable.is_empty());
        assert!(review.is_clean(), "{review:?}");
    }

    /// Found by running this against a real vault, not by reading the code: nothing ever
    /// links to a journal entry, so before this every daily came back as lost memory and
    /// buried the one orphan that mattered.
    #[test]
    fn a_daily_is_not_reported_as_lost_for_having_nothing_link_to_it() {
        let notes = vec![
            note("INDEX.md", "# Index\n\n- [[profile]]\n"),
            note("profile.md", "Likes tea.\n"),
            note("daily/2026-10-30.md", "Did things today.\n"),
            note("projects/forgotten.md", "Real content nothing points at.\n"),
        ];
        let review = review(&notes, now());
        assert_eq!(
            review.unreachable,
            [Unreachable {
                note: "projects/forgotten.md".to_string()
            }],
            "only the real orphan should be reported"
        );
    }

    #[test]
    fn a_vault_in_good_order_says_so_rather_than_printing_nothing() {
        let notes = vec![
            note("INDEX.md", "# Index\n\n- [[profile]]\n"),
            note("profile.md", "Likes tea.\n"),
        ];
        let review = review(&notes, now());
        assert!(review.is_clean());
        let rendered = describe(&review);
        assert!(rendered.contains("good order"));
        assert!(rendered.contains("do not propose changes"));
    }

    /// Long lists are capped by name and counted in full, so the count stays honest.
    #[test]
    fn a_long_list_is_counted_in_full_and_named_in_part() {
        let mut notes = vec![note("INDEX.md", "# Index\n")];
        for i in 0..20 {
            notes.push(note(&format!("orphan-{i:02}.md"), "Real content here.\n"));
        }
        let review = review(&notes, now());
        assert_eq!(review.unreachable.len(), 20);
        let rendered = describe(&review);
        assert!(rendered.contains("(20)"));
        assert!(rendered.contains("and 12 more"));
    }

    /// Only `daily/` is offered for archiving, and the rendering has to say so: it is the
    /// instruction that stops a model tidying away a fact for being old.
    #[test]
    fn the_rendering_tells_the_model_not_to_archive_anything_else_for_being_old() {
        let notes = vec![aged("daily/a.md", "Things.\n", 200)];
        let rendered = describe(&review(&notes, now()));
        assert!(rendered.contains("Never archive anything else for being old"));
    }

    /// Unresolved links must not be described as breakage, whatever else changes here.
    #[test]
    fn a_wanted_subject_is_not_called_a_broken_link() {
        let notes = vec![note("INDEX.md", "Mentions [[allergies]].\n")];
        let rendered = describe(&review(&notes, now()));
        assert!(rendered.contains("not broken links"));
    }

    #[test]
    fn the_same_vault_reviewed_twice_gives_the_same_answer() {
        let notes = vec![
            note("INDEX.md", "# Index\n\n[[a]] [[b]] [[missing]]\n"),
            note("a.md", "Real content. [[also-missing]]\n"),
            note("b.md", "# Stub\n"),
            note("orphan.md", "Real content.\n"),
            aged("daily/old.md", "Things.\n", 400),
        ];
        assert_eq!(review(&notes, now()), review(&notes, now()));
    }

    #[test]
    fn reviewing_memory_is_read_only_and_pre_approvable() {
        assert!(!ReviewMemory.mutating());
        assert!(ReviewMemory.always_allowable());
    }

    #[test]
    fn an_empty_vault_is_not_an_error() {
        let review = review(&[], now());
        assert!(review.is_clean());
        assert_eq!(review.notes, 0);
    }
}
