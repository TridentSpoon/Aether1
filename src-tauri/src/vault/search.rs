// Retrieval: finding the note that answers the question, in a vault too big to read.
//
// Priming carries the index and the always-loaded notes, and for a small vault that is the
// whole retrieval mechanism -- the index says what exists and read_file does the reaching.
// It stops being enough at the point the index itself is too long to read every turn, which
// is exactly when a vault has become worth having.
//
// So: search. Deliberately not an FTS table and not embeddings. A vault is a few hundred
// markdown files totalling a couple of megabytes; scanning it costs milliseconds, needs no
// index to keep in sync, no migration, and no second copy of the operator's memory in a
// format they cannot open. The plan said to add a real index only if the simple thing
// measurably fails, and the simple thing has not been measured failing yet.
//
// What matters more than the mechanism is the ranking. The failure this is written against
// is "asking about one topic pulls the ten most recent notes": recency is a tiebreaker
// here and never a reason to rank one note above another that matched better. Where a word
// appears is what counts -- a note *named* for the topic beats one that mentions it in a
// heading, which beats one that mentions it in passing.

use std::path::Path;
use std::time::SystemTime;

use crate::llm::MemoryDb;

/// Stop before reading an entire disk. A vault this size is not a vault any more, and the
/// honest answer is to say the search was partial rather than to sit there.
const MAX_NOTES_SCANNED: usize = 2000;

/// Per note. A markdown file larger than this is not a note, and scanning all of it would
/// let one runaway file dominate the time the whole search takes.
const MAX_NOTE_BYTES: u64 = 512 * 1024;

/// How many notes come back. The model reads the ones it wants with read_file, so this is
/// a shortlist rather than an answer -- long enough to contain the right note, short enough
/// that it doesn't crowd out the conversation.
const MAX_HITS: usize = 8;

const SNIPPET_CHARS: usize = 180;

/// Where a word was found, in the order that matters.
const PATH_WEIGHT: u32 = 8;
const HEADING_WEIGHT: u32 = 4;
const BODY_WEIGHT: u32 = 1;

/// Body matches stop counting after this. Without a cap, a note that says "theme" forty
/// times outranks the note actually *about* the theme, which is the wrong answer arrived at
/// by enthusiasm.
const MAX_BODY_MATCHES: u32 = 3;

/// Added for each search word beyond the first that a note matches. A note matching both
/// "theme" and "saturation" is far more likely to be the one wanted than a note matching
/// either word twice as often, and without this nothing in the scoring would say so.
const BREADTH_BONUS: u32 = 5;

/// Words shorter than this are skipped: "a" and "is" match everything and rank nothing.
const MIN_TERM_LEN: usize = 2;

/// Function words, dropped from a query.
///
/// Not an attempt at linguistics -- it exists because of BREADTH_BONUS. A note that happens
/// to contain "the" would otherwise collect the same bonus as one that matched a second
/// real word, and since almost every note contains "the", the bonus would go to whichever
/// notes are longest rather than to whichever are most relevant. Only words that carry no
/// subject at all are listed; anything a note could plausibly be *about* is left in.
const STOPWORDS: &[&str] = &[
    "the", "and", "for", "that", "this", "with", "was", "are", "you", "your", "its", "but", "not",
    "from", "what", "when", "where", "which", "have", "has", "had", "about", "into", "can", "will",
    "would", "should", "there", "their", "they", "them", "then", "than", "how", "why", "who",
    "all", "any", "some", "more", "most", "been", "were", "does", "did", "just", "our", "out",
    "off", "per", "via", "yet", "too", "is", "it", "in", "on", "at", "to", "of", "as", "be", "by",
    "or", "if", "do", "we", "my", "me", "no", "so", "up", "an", "am", "he", "us",
];

/// Enough words to be specific, few enough that a pasted paragraph doesn't become the query.
const MAX_TERMS: usize = 8;

/// One note that matched, and why.
#[derive(Debug, Clone)]
pub struct Hit {
    /// Path relative to the vault root, as the operator would see it in their editor.
    pub note: String,
    pub score: u32,
    /// The heading the match fell under, when it fell under one.
    pub heading: Option<String>,
    pub snippet: String,
    /// Tiebreaker only. See the note at the top of this file.
    modified: SystemTime,
}

/// The search words, lowercased and deduplicated.
///
/// Split on anything that isn't alphanumeric so that `projects/theme-engine.md` and
/// "theme engine" produce the same two words -- the operator's question and their filenames
/// are written by different conventions and neither should have to match the other's.
fn terms(query: &str) -> Vec<String> {
    let words: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= MIN_TERM_LEN)
        .map(str::to_lowercase)
        .collect();

    // A query made entirely of function words keeps them: searching for nothing would be a
    // worse answer than searching for "how" badly.
    let keep_all = words.iter().all(|w| STOPWORDS.contains(&w.as_str()));

    let mut out: Vec<String> = Vec::new();
    for word in words {
        if !keep_all && STOPWORDS.contains(&word.as_str()) {
            continue;
        }
        if !out.contains(&word) {
            out.push(word);
        }
        if out.len() == MAX_TERMS {
            break;
        }
    }
    out
}

/// What one term was worth in one note.
#[derive(Default)]
struct TermScore {
    in_path: bool,
    in_heading: bool,
    body_matches: u32,
}

impl TermScore {
    fn matched(&self) -> bool {
        self.in_path || self.in_heading || self.body_matches > 0
    }

    fn points(&self) -> u32 {
        u32::from(self.in_path) * PATH_WEIGHT
            + u32::from(self.in_heading) * HEADING_WEIGHT
            + self.body_matches.min(MAX_BODY_MATCHES) * BODY_WEIGHT
    }
}

fn is_heading(line: &str) -> bool {
    line.trim_start().starts_with('#')
}

fn clean_heading(line: &str) -> String {
    line.trim().trim_start_matches('#').trim().to_string()
}

/// Scores one note, or returns None when nothing in it matched at all.
///
/// Public to the module so the ranking can be tested against text rather than against a
/// directory of temporary files -- the ranking is the part worth pinning down.
pub(super) fn score(
    note: &str,
    contents: &str,
    terms: &[String],
) -> Option<(u32, Option<String>, String)> {
    if terms.is_empty() {
        return None;
    }
    let lower_path = note.to_lowercase();
    let mut scores: Vec<TermScore> = terms
        .iter()
        .map(|term| TermScore {
            in_path: lower_path.contains(term),
            ..TermScore::default()
        })
        .collect();

    let mut heading: Option<String> = None;
    let mut current_heading: Option<String> = None;
    let mut snippet: Option<String> = None;

    for line in contents.lines() {
        let lower = line.to_lowercase();
        let heading_line = is_heading(line);
        if heading_line {
            current_heading = Some(clean_heading(line));
        }

        let mut matched_here = false;
        for (term, score) in terms.iter().zip(scores.iter_mut()) {
            if !lower.contains(term) {
                continue;
            }
            matched_here = true;
            if heading_line {
                score.in_heading = true;
            } else {
                score.body_matches += 1;
            }
        }

        if !matched_here {
            continue;
        }
        // The first match is the one quoted back: it is where the reader's eye would land
        // if they opened the note themselves.
        if heading.is_none() {
            heading = current_heading.clone();
        }
        if snippet.is_none() && !heading_line && !line.trim().is_empty() {
            snippet = Some(line.trim().to_string());
        }
    }

    let matched = scores.iter().filter(|s| s.matched()).count() as u32;
    if matched == 0 {
        return None;
    }
    let total: u32 = scores.iter().map(TermScore::points).sum::<u32>()
        + BREADTH_BONUS * matched.saturating_sub(1);

    // A note whose only match is its own filename has no line to quote, so fall back to
    // whatever it opens with -- still more use than an empty snippet.
    let snippet = snippet
        .or_else(|| {
            contents
                .lines()
                .find(|l| !l.trim().is_empty() && !is_heading(l))
                .map(|l| l.trim().to_string())
        })
        .unwrap_or_default();

    Some((
        total,
        heading,
        super::truncate_chars(&snippet, SNIPPET_CHARS),
    ))
}

/// How many notes were looked at, and which of them matched.
pub struct Results {
    pub hits: Vec<Hit>,
    pub scanned: usize,
    /// True when the scan stopped at the cap rather than at the end of the vault.
    pub partial: bool,
}

/// Searches every note in the vault.
pub fn search(db: &MemoryDb, query: &str) -> Results {
    let root = super::vault_path(db);
    let terms = terms(query);

    let mut names = Vec::new();
    super::collect_notes(&root, &root, &mut names, 0);
    names.sort();
    let partial = names.len() > MAX_NOTES_SCANNED;
    names.truncate(MAX_NOTES_SCANNED);

    let mut hits = Vec::new();
    for name in &names {
        let path = root.join(name);
        if !readable_size(&path) {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some((score, heading, snippet)) = score(name, &contents, &terms) else {
            continue;
        };
        hits.push(Hit {
            note: name.clone(),
            score,
            heading,
            snippet,
            modified: modified_at(&path),
        });
    }

    // Score first, then recency, then name. The last is only there so that two notes that
    // are equal in every way come back in the same order twice running.
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then(b.modified.cmp(&a.modified))
            .then(a.note.cmp(&b.note))
    });
    hits.truncate(MAX_HITS);

    // What comes back is what the model was offered, so it is what the HUD reports -- as
    // candidates, not as notes that were read. The model may well ignore every one of them.
    for hit in &hits {
        super::consulted::record(&hit.note, super::consulted::How::Found);
    }

    Results {
        hits,
        scanned: names.len(),
        partial,
    }
}

fn readable_size(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.len() <= MAX_NOTE_BYTES)
}

fn modified_at(path: &Path) -> SystemTime {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// The search written out for the model: what matched, where, and how to read it.
pub fn render(db: &MemoryDb, query: &str, results: &Results) -> String {
    let root = super::vault_path(db);
    if results.hits.is_empty() {
        return format!(
            "No note matches {query:?} ({} notes searched in {}). The vault may simply not \
             have this yet -- say so rather than guessing.",
            results.scanned,
            root.display()
        );
    }

    let body: Vec<String> = results
        .hits
        .iter()
        .map(|hit| {
            let under = match &hit.heading {
                Some(heading) if !heading.is_empty() => format!(" — under \"{heading}\""),
                _ => String::new(),
            };
            format!("- `{}`{under}\n  {}", hit.note, hit.snippet)
        })
        .collect();

    format!(
        "{} of {} notes match {query:?}{}:\n\n{}\n\nRead any of these in full with \
         read_file, prefixing the path with `{}/`. The snippets are the first matching line, \
         not the whole note.",
        results.hits.len(),
        results.scanned,
        if results.partial {
            " (the vault is large enough that the search stopped early)"
        } else {
            ""
        },
        body.join("\n"),
        root.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points(note: &str, contents: &str, query: &str) -> u32 {
        score(note, contents, &terms(query))
            .unwrap_or_else(|| panic!("{note:?} should match {query:?}"))
            .0
    }

    /// The failure this whole ranking exists to prevent: the note *about* a topic must beat
    /// the note that merely talks about it a lot, however recent that one is.
    #[test]
    fn a_note_named_for_the_topic_beats_one_that_only_mentions_it() {
        let named = points(
            "projects/theme-engine.md",
            "# Theme engine\n\nHow colours are derived.\n",
            "theme",
        );
        let mentions = points(
            "daily/2026-04-01.md",
            "# Monday\n\nTalked about the theme. The theme is bright. Theme again. Theme.\n",
            "theme",
        );
        assert!(
            named > mentions,
            "named {named} should outrank mentions {mentions}"
        );
    }

    /// And the heading is worth more than the body, for the same reason one step down.
    #[test]
    fn a_heading_match_beats_a_body_match() {
        let heading = points(
            "a.md",
            "# Saturation\n\nsomething else entirely\n",
            "saturation",
        );
        let body = points("b.md", "# Notes\n\nthe saturation slider\n", "saturation");
        assert!(heading > body, "heading {heading} vs body {body}");
    }

    /// Matching both words beats matching one of them repeatedly. Without the breadth bonus
    /// nothing in the scoring would say so, and "theme saturation" would find the noisiest
    /// note about themes rather than the one about the slider.
    #[test]
    fn matching_two_words_beats_matching_one_word_often() {
        let both = points("a.md", "# Notes\n\ntheme\nsaturation\n", "theme saturation");
        let one = points(
            "b.md",
            "# Notes\n\ntheme\ntheme\ntheme\ntheme\ntheme\ntheme\n",
            "theme saturation",
        );
        assert!(both > one, "both {both} vs one {one}");
    }

    /// A note cannot climb the ranking by repetition alone.
    #[test]
    fn body_matches_stop_counting_after_a_few() {
        let three = points("a.md", "x\nx\nx\n".replace('x', "theme").as_str(), "theme");
        let twenty = points("a.md", &"theme\n".repeat(20), "theme");
        assert_eq!(three, twenty);
    }

    #[test]
    fn a_note_matching_nothing_does_not_come_back() {
        assert!(score("a.md", "# Notes\n\nnothing relevant\n", &terms("sourdough")).is_none());
    }

    /// Function words are dropped, or the breadth bonus would be handed to whichever notes
    /// happen to be longest.
    #[test]
    fn function_words_are_not_search_terms() {
        assert_eq!(terms("what is the theme"), vec!["theme"]);
        assert_eq!(terms("a"), Vec::<String>::new());
    }

    /// Unless that is all there is -- searching for nothing is a worse answer.
    #[test]
    fn a_query_of_nothing_but_function_words_still_searches() {
        assert_eq!(terms("how and why"), vec!["how", "and", "why"]);
    }

    /// Filenames and questions are written by different conventions, and neither should have
    /// to match the other's punctuation.
    #[test]
    fn a_hyphenated_filename_matches_a_spaced_query() {
        assert!(score("projects/theme-engine.md", "# x\n", &terms("theme engine")).is_some());
        assert_eq!(terms("theme-engine"), vec!["theme", "engine"]);
    }

    /// The snippet is the line a reader's eye would land on, and the heading is where they
    /// would find it.
    #[test]
    fn the_snippet_quotes_the_first_matching_line_under_its_heading() {
        let (_, heading, snippet) = score(
            "a.md",
            "# Notes\n\n## Editor\n\nThey use helix, not vim.\n",
            &terms("helix"),
        )
        .unwrap();
        assert_eq!(heading.as_deref(), Some("Editor"));
        assert_eq!(snippet, "They use helix, not vim.");
    }

    /// A note matched only by its filename has no line to quote, so it borrows its own
    /// opening rather than coming back blank.
    #[test]
    fn a_filename_only_match_still_carries_a_snippet() {
        let (_, _, snippet) = score(
            "projects/helix.md",
            "# Helix\n\nThe editor they moved to in 2025.\n",
            &terms("helix"),
        )
        .unwrap();
        assert!(!snippet.is_empty(), "snippet should not be empty");
    }

    /// A snippet is cut by characters, not bytes: an em dash at the wrong offset is not a
    /// reason to panic.
    #[test]
    fn a_long_snippet_is_cut_without_splitting_a_character() {
        let line = "—".repeat(400);
        let (_, _, snippet) =
            score("a.md", &format!("# x\n\n{line} theme\n"), &terms("theme")).unwrap();
        assert!(snippet.chars().count() <= SNIPPET_CHARS + 1);
    }
}
