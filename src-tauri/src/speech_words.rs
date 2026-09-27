// How a word is said, when the synthesizer gets it wrong.
//
// Trident, 2026-09-27: "Will it be possible to have the equivalent of a spell check in a
// pronunciation check list? There are a few words that I find hard to discern when not using
// my local pronunciations."
//
// A list of "say this as that", applied to the text on its way to the synthesizer and to
// nothing else. What is on screen is untouched -- the operator wrote `Aether1` and reads
// `Aether1`; only the voice hears `eether one`.
//
// **Why a respelling and not a phoneme.** The obvious answer is IPA or eSpeak phonemes, and
// it does not survive contact with the three engines: Piper takes plain text and phonemizes
// it itself with no escape hatch, the cloud engine wants SSML `<phoneme>`, and the OS engine
// wants neither. A respelling is the one instruction all three understand, because it is
// just words. It also happens to be the thing the operator can actually author: "say
// L'KEMI as elle kemmy" needs no notation, and the test button says immediately whether it
// worked. The cost is that it cannot express stress or a vowel English spelling has no
// letters for; when that day comes, an optional phoneme field can sit beside this one for
// the engines that take it, without changing what is stored here.
//
// Matching is by whole word and ignores case, because that is what "a word is said wrong"
// means. The replacement is inserted exactly as written, never recapitalised: an engine
// handed `NGINX` may spell it out letter by letter, so `engine ex` must stay lower case even
// where it replaced a shout.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::llm::MemoryDb;

/// The settings row holding the whole list. One row rather than a row per word: it is read
/// once per sentence spoken, and one read beats fifty.
pub const SETTING: &str = "speech_pronunciations";

/// Ceilings, so a hand-edited row or a stuck key cannot turn every sentence into a linear
/// scan of nonsense. Generous enough that nobody sensible will meet them.
pub const MAX_ENTRIES: usize = 200;
pub const MAX_LEN: usize = 120;

/// One correction: say `from` as `to`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Say {
    pub from: String,
    pub to: String,
}

/// Every correction, in the order the operator put them in. A row that will not parse reads
/// as no corrections at all -- speech is not worth failing over a settings row somebody
/// hand-edited, and the next save rewrites it.
pub fn all(db: &MemoryDb) -> Vec<Say> {
    let raw = db.get_setting_string(SETTING, "");
    if raw.trim().is_empty() {
        return Vec::new();
    }
    serde_json::from_str(&raw).unwrap_or_default()
}

/// Checks a list and stores it, returning it as stored. The returned list is what to put
/// back on screen: it is trimmed, it has dropped the empty rows an editing UI always leaves
/// behind, and it has collapsed duplicates -- so a pane that redraws from this cannot
/// disagree with what speech will actually do.
pub fn set(db: &MemoryDb, words: Vec<Say>) -> Result<Vec<Say>, String> {
    let mut kept: Vec<Say> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();

    for word in words {
        let from = word.from.trim().to_string();
        let to = word.to.trim().to_string();
        // A row with neither half is the empty one at the bottom of every editor. Dropping
        // it silently is right; refusing the whole save because of it is not.
        if from.is_empty() && to.is_empty() {
            continue;
        }
        if from.is_empty() {
            return Err("there is a pronunciation with nothing to match on".to_string());
        }
        if to.is_empty() {
            return Err(format!("nothing was given for how to say \"{from}\""));
        }
        if from.chars().count() > MAX_LEN || to.chars().count() > MAX_LEN {
            return Err(format!(
                "\"{from}\" is longer than {MAX_LEN} characters, which is longer than a word"
            ));
        }
        // Matching ignores case, so two rows differing only in case are one rule with two
        // answers. The later one wins, in place, rather than being appended -- an operator
        // correcting a row expects the list to stay the length they can see.
        let key = from.to_lowercase();
        match seen.get(&key) {
            Some(&at) => kept[at] = Say { from, to },
            None => {
                seen.insert(key, kept.len());
                kept.push(Say { from, to });
            }
        }
        if kept.len() > MAX_ENTRIES {
            return Err(format!("that is more than {MAX_ENTRIES} pronunciations"));
        }
    }

    let value = serde_json::to_string(&kept).map_err(|why| why.to_string())?;
    db.set_setting(SETTING, &Value::String(value))
        .map_err(|why| format!("could not save the pronunciations: {why}"))?;
    Ok(kept)
}

/// The list, compiled into something that can be run over a sentence.
///
/// Built once per clip rather than held in a static: the list changes from Settings while
/// the app is running, and a cached regex would keep saying the old thing until a restart --
/// which for a feature whose entire loop is "type it, press test, listen" is the one
/// behaviour that makes it feel broken. One small regex compile against a synthesis that
/// takes hundreds of milliseconds is not a cost worth that.
pub struct Speller {
    matcher: Option<regex::Regex>,
    answers: HashMap<String, String>,
}

impl Speller {
    /// An empty speller, for the paths with no database to ask (the CLI's one-shot `say`
    /// before a db is open, and tests).
    pub fn none() -> Speller {
        Speller {
            matcher: None,
            answers: HashMap::new(),
        }
    }

    pub fn build(words: &[Say]) -> Speller {
        if words.is_empty() {
            return Speller::none();
        }

        // Longest first, so a rule for a phrase beats a rule for one of its words rather
        // than losing to whichever the alternation happened to try first.
        let mut ordered: Vec<&Say> = words.iter().collect();
        ordered.sort_by_key(|w| std::cmp::Reverse(w.from.chars().count()));

        let mut answers = HashMap::new();
        let mut branches = Vec::new();
        for word in ordered {
            answers.insert(word.from.to_lowercase(), word.to.clone());
            branches.push(bounded(&word.from));
        }

        // `(?i)` for the case-insensitivity the answers map already assumes.
        let matcher = regex::Regex::new(&format!("(?i){}", branches.join("|"))).ok();
        Speller { matcher, answers }
    }

    /// The sentence as it should be said.
    pub fn apply(&self, text: &str) -> String {
        let Some(matcher) = &self.matcher else {
            return text.to_string();
        };
        // A closure rather than a replacement template: `$1` in somebody's respelling is
        // four characters they typed, not a capture group.
        matcher
            .replace_all(text, |caught: &regex::Captures| {
                let hit = caught.get(0).map(|m| m.as_str()).unwrap_or_default();
                self.answers
                    .get(&hit.to_lowercase())
                    .cloned()
                    .unwrap_or_else(|| hit.to_string())
            })
            .into_owned()
    }

    pub fn is_empty(&self) -> bool {
        self.matcher.is_none()
    }
}

/// This machine's pronunciations, ready to run.
pub fn speller(db: &MemoryDb) -> Speller {
    Speller::build(&all(db))
}

/// One alternation branch: the word, escaped, with a word boundary on each side that has a
/// word character to sit against.
///
/// The condition is the point. `\b` between two non-word characters never matches, so
/// wrapping `C++` in them unconditionally makes it unmatchable -- and the words an operator
/// most wants to correct are exactly the ones with punctuation in them: `C++`, `.NET`,
/// `L'KEMI`, `nginx-ingress`.
fn bounded(from: &str) -> String {
    let quoted = regex::escape(from);
    let word_char = |c: char| c.is_alphanumeric() || c == '_';
    let open = from.chars().next().is_some_and(word_char);
    let close = from.chars().last().is_some_and(word_char);
    format!(
        "{}{quoted}{}",
        if open { r"\b" } else { "" },
        if close { r"\b" } else { "" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> MemoryDb {
        let path = std::env::temp_dir().join(format!(
            "aether1_speech_words_{}_{:?}.db",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&path);
        MemoryDb::open(&path).expect("open test db")
    }

    fn say(from: &str, to: &str) -> Say {
        Say {
            from: from.to_string(),
            to: to.to_string(),
        }
    }

    #[test]
    fn nothing_is_changed_until_somebody_adds_a_word() {
        let db = db();
        assert!(all(&db).is_empty());
        let speller = speller(&db);
        assert!(speller.is_empty());
        assert_eq!(
            speller.apply("Aether1 is listening."),
            "Aether1 is listening."
        );
    }

    #[test]
    fn a_word_is_said_the_way_it_was_asked_for() {
        let speller = Speller::build(&[say("Aether1", "eether one")]);
        assert_eq!(
            speller.apply("Aether1 is listening."),
            "eether one is listening."
        );
    }

    #[test]
    fn matching_ignores_case_and_the_answer_is_used_exactly_as_written() {
        // An engine handed NGINX may spell it out; recapitalising the answer to match the
        // shout would undo the whole point of the correction.
        let speller = Speller::build(&[say("nginx", "engine ex")]);
        assert_eq!(
            speller.apply("NGINX and Nginx and nginx"),
            "engine ex and engine ex and engine ex"
        );
    }

    #[test]
    fn only_whole_words_are_touched() {
        let speller = Speller::build(&[say("read", "red")]);
        // "already" and "reader" contain it, and neither is the word.
        assert_eq!(
            speller.apply("I already read what the reader read."),
            "I already red what the reader red."
        );
    }

    #[test]
    fn a_word_with_punctuation_in_it_still_matches() {
        // \b between two non-word characters never matches, so these are the cases that a
        // naive \b...\b silently fails to correct -- and they are the ones most worth
        // correcting.
        let speller = Speller::build(&[
            say("C++", "see plus plus"),
            say(".NET", "dot net"),
            say("L'KEMI", "elle kemmy"),
        ]);
        assert_eq!(
            speller.apply("C++ and .NET, and L'KEMI is on it."),
            "see plus plus and dot net, and elle kemmy is on it."
        );
    }

    #[test]
    fn a_phrase_beats_a_word_inside_it() {
        let speller = Speller::build(&[say("code", "coad"), say("AETHER CODE", "eether code")]);
        assert_eq!(
            speller.apply("AETHER CODE wrote the code."),
            "eether code wrote the coad."
        );
    }

    #[test]
    fn a_dollar_sign_in_the_answer_is_four_characters_and_not_a_capture_group() {
        let speller = Speller::build(&[say("cost", "$1 and $2")]);
        assert_eq!(speller.apply("the cost"), "the $1 and $2");
    }

    #[test]
    fn saving_trims_drops_the_empty_row_and_collapses_a_duplicate() {
        let db = db();
        let stored = set(
            &db,
            vec![
                say("  Aether1 ", " eether one "),
                say("", ""),
                say("AETHER1", "eether wun"),
            ],
        )
        .unwrap();
        // One rule, in the place the first one held, with the later answer.
        assert_eq!(stored, vec![say("AETHER1", "eether wun")]);
        assert_eq!(all(&db), stored);
    }

    #[test]
    fn a_half_filled_row_is_refused_and_nothing_is_stored() {
        let db = db();
        assert!(set(&db, vec![say("Aether1", "")]).is_err());
        assert!(set(&db, vec![say("", "eether one")]).is_err());
        assert!(all(&db).is_empty());
    }

    #[test]
    fn the_ceilings_hold() {
        let db = db();
        let long = "a".repeat(MAX_LEN + 1);
        assert!(set(&db, vec![say(&long, "x")]).is_err());
        let many: Vec<Say> = (0..MAX_ENTRIES + 1)
            .map(|n| say(&format!("w{n}"), "x"))
            .collect();
        assert!(set(&db, many).is_err());
        assert!(all(&db).is_empty());
    }

    #[test]
    fn a_settings_row_that_will_not_parse_reads_as_no_corrections() {
        let db = db();
        db.set_setting(SETTING, &Value::String("{not json".into()))
            .unwrap();
        assert!(all(&db).is_empty());
        assert_eq!(speller(&db).apply("Aether1"), "Aether1");
    }

    #[test]
    fn a_word_that_cannot_be_made_into_a_pattern_does_not_take_speech_down_with_it() {
        // regex::escape means there is no such word today, which is exactly why this is
        // asserted: the day somebody removes the escaping, a pronunciation list becomes a
        // way to stop the companion speaking at all.
        let speller = Speller::build(&[say("(unclosed", "un closed")]);
        assert_eq!(speller.apply("(unclosed is fine"), "un closed is fine");
    }
}
