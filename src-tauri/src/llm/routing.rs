// Step 19: several local models, and choosing between them.
//
// The intent, in one sentence: the backend holds more than one local model at a time and
// sends a task to whichever suits it -- a code-specialised model where code is the job,
// something chatty where conversation is.
//
// The design turns on one decision, and it is worth stating plainly because the obvious
// alternative is so tempting. **The speciality is the key, not the request.** Nothing here
// classifies what you just typed. The personas already carve the work up by job, so "which
// model for which job" is a question that can be asked once, answered in a dropdown, and
// read back as a sentence an operator can check -- "L'kemi uses Qwen-Coder" -- rather than
// a routing decision made per message that nobody can see or predict.
//
// Three consequences follow, all deliberate:
//
//   1. **The suggestion is a table shipped in the binary.** Not a Hugging Face or Ollama
//      library lookup: those rank by downloads, which measures popularity rather than
//      fitness, and principle 2 says a machine with no network still has to see a sensible
//      default. The table ages and is updated with releases. That is the honest cost.
//   2. **A suggestion is never a choice.** The operator's pick is stored and wins; the
//      table only fills the gap where they have not picked, and marks one entry
//      *(suggested)* in the dropdown.
//   3. **Speed breaks ties, it does not decide.** `model_benchmarks` measures tokens per
//      second, which is speed. Fitness is a different axis, and letting the scoreboard pick
//      would quietly route code at whatever model happens to be smallest.
//
// Local only, and host-only for now. Models on another machine are a separate feature with
// a different threat model, which step 45 has since given the authentication half of; the
// two stay separable so this one ships without waiting.

use std::collections::HashMap;

use super::{MemoryDb, Persona};

/// Setting holding the operator's per-speciality picks, as `{"nexus": "qwen2.5-coder:7b"}`.
/// One key with a map inside, mirroring `domain_extra_roots`, because a settings table with
/// sixteen nearly identical keys in it is a settings table nobody can read.
pub const MODELS_SETTING: &str = "persona_models";

/// One row of the shipped table: a family of models, and the personas it suits.
///
/// Matched on a substring of the model's name rather than an exact id, because the same
/// family arrives under a dozen tags -- `qwen2.5-coder:7b`, `qwen2.5-coder:32b-instruct-q4`,
/// `Qwen2.5-Coder-7B-Instruct-GGUF` -- and a table of exact ids would be stale the day it
/// shipped rather than merely eventually.
struct Family {
    /// Lowercase fragments; a model matches when its name contains all of them.
    needles: &'static [&'static str],
    /// What this family is good at, in the operator's words, for the dropdown's tooltip.
    good_at: &'static str,
    /// The personas it suits, best first.
    suits: &'static [Persona],
}

/// The table. Ordered most-specific first, because `qwen` alone would otherwise swallow
/// `qwen-coder` and route code at a general chat model.
///
/// Kept short on purpose. A row earns its place by being a family someone actually runs
/// locally and by suiting a speciality clearly enough that the suggestion is not a guess
/// wearing a label -- a model that is merely fine at everything belongs in the fallback,
/// not in a row that claims it is the right answer for a job.
const FAMILIES: &[Family] = &[
    Family {
        needles: &["qwen", "coder"],
        good_at: "writing and fixing code",
        suits: &[Persona::Nexus, Persona::ArxLkemi, Persona::ArxLexico],
    },
    Family {
        needles: &["deepseek", "coder"],
        good_at: "writing and fixing code",
        suits: &[Persona::Nexus, Persona::ArxLkemi],
    },
    Family {
        needles: &["codellama"],
        good_at: "writing and fixing code",
        suits: &[Persona::Nexus, Persona::ArxLkemi],
    },
    Family {
        needles: &["starcoder"],
        good_at: "writing and fixing code",
        suits: &[Persona::Nexus],
    },
    Family {
        needles: &["deepseek", "r1"],
        good_at: "working a problem through step by step",
        suits: &[
            Persona::ArxLegionare,
            Persona::Halcy,
            Persona::ArxLucre,
            Persona::Default,
        ],
    },
    Family {
        needles: &["hermes"],
        good_at: "conversation, and following an instruction closely",
        suits: &[Persona::Halcy, Persona::ArxLocas, Persona::ArxLoregenda],
    },
    Family {
        needles: &["llama"],
        good_at: "general conversation",
        suits: &[Persona::Halcy, Persona::ArxLocas, Persona::ArxLogos],
    },
    Family {
        needles: &["mistral"],
        good_at: "general work, quickly",
        suits: &[Persona::ArxLocas, Persona::Red9000, Persona::ArxLyksaum],
    },
    Family {
        needles: &["gemma"],
        good_at: "short answers without padding",
        suits: &[Persona::Red9000, Persona::ArxLyksaum],
    },
    Family {
        needles: &["phi"],
        good_at: "short answers on a small machine",
        suits: &[Persona::Red9000, Persona::ArxLyksaum, Persona::ArxLimes],
    },
];

fn matches(family: &Family, model: &str) -> bool {
    let model = model.to_ascii_lowercase();
    family
        .needles
        .iter()
        .all(|needle| model.contains(&needle.to_ascii_lowercase()))
}

/// What a model is good at, per the shipped table, or None when the table does not claim to
/// know. None is a real answer and is shown as one: a model the table has never heard of is
/// not thereby bad, and saying nothing about it is more honest than inventing a strength.
pub fn good_at(model: &str) -> Option<&'static str> {
    FAMILIES
        .iter()
        .find(|family| matches(family, model))
        .map(|family| family.good_at)
}

/// The model this persona should be offered, out of what is actually installed.
///
/// Walks the table in order and takes the first family that both matches an available model
/// and names this persona. Where a family has several models installed -- a 7B and a 32B of
/// the same coder -- `speeds` breaks the tie, which is the one thing measured speed is
/// genuinely the right answer for.
///
/// Returns None when nothing installed suits this persona in particular. That is not a
/// failure: it means the operator's general model is the right thing to use, and claiming
/// otherwise would be the table pretending to know something it does not.
pub fn suggestion(
    persona: &Persona,
    available: &[String],
    speeds: &HashMap<String, f64>,
) -> Option<String> {
    for family in FAMILIES {
        if !family.suits.contains(persona) {
            continue;
        }
        let mut candidates: Vec<&String> = available
            .iter()
            .filter(|model| matches(family, model))
            .collect();
        if candidates.is_empty() {
            continue;
        }
        // Fastest first. Where nothing has been measured -- a fresh install, which is
        // exactly when a suggestion matters most -- the smaller model wins, because on an
        // unknown machine the smaller one is the one more likely to run at all. Name last,
        // only so a fresh install gets a stable answer rather than whatever order the
        // server happened to list.
        candidates.sort_by(|a, b| {
            let speed = |m: &str| speeds.get(&m.to_ascii_lowercase()).copied().unwrap_or(0.0);
            speed(b)
                .partial_cmp(&speed(a))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| parameter_billions(a).cmp(&parameter_billions(b)))
                .then_with(|| a.cmp(b))
        });
        return candidates.first().map(|model| (*model).clone());
    }
    None
}

/// How many billion parameters a model's name claims, in tenths so `1.5b` sorts properly.
///
/// Read off the tag rather than asked of the server, because the server does not say and
/// the tag almost always does -- `:7b`, `:32b-instruct-q4`, `-7B-Instruct-GGUF`. A name
/// that claims nothing sorts last: an unlabelled model is not thereby small, and guessing
/// it is would be how a 70B ends up suggested on a laptop.
fn parameter_billions(model: &str) -> u32 {
    let lower = model.to_ascii_lowercase();
    // The tag after the colon where there is one; Ollama puts the size there, and the part
    // before it often carries an unrelated version number (`qwen2.5`, `llama3.2`).
    let tail = lower.rsplit_once(':').map(|(_, tag)| tag).unwrap_or(&lower);
    let bytes: Vec<char> = tail.chars().collect();
    let mut found = None;
    for (index, c) in bytes.iter().enumerate() {
        if *c != 'b' {
            continue;
        }
        // A digit must sit before the b, and a letter must not sit after it -- otherwise
        // every "base" and "bit" in a name would read as a size.
        if bytes
            .get(index + 1)
            .is_some_and(|next| next.is_alphanumeric())
        {
            continue;
        }
        let mut start = index;
        while start > 0 && (bytes[start - 1].is_ascii_digit() || bytes[start - 1] == '.') {
            start -= 1;
        }
        if start == index {
            continue;
        }
        if let Ok(value) = bytes[start..index]
            .iter()
            .collect::<String>()
            .parse::<f64>()
        {
            found = Some((value * 10.0).round() as u32);
        }
    }
    found.unwrap_or(u32::MAX)
}

/// Why the model about to be used is the one being used. The operator can see this, so it
/// is written to be read rather than matched on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    /// The operator picked it and it is installed.
    Chosen,
    /// The operator has not picked for this speciality; the shipped table suggested it.
    Suggested,
    /// The operator picked a model that is no longer installed. Carries what they picked,
    /// so the notice can name it rather than saying "your model" -- see `Choice::notice`.
    Missing(String),
    /// Nothing specific to this speciality; the general `llm_model` setting stands.
    General,
}

/// The outcome of asking "what should this persona run on".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    /// None means "leave `llm_model` alone", which is the honest answer when nothing
    /// installed suits this speciality in particular.
    pub model: Option<String>,
    pub reason: Reason,
}

impl Choice {
    /// The line to show the operator, or None when nothing needs saying.
    ///
    /// Only the missing case speaks. Uninstalling a model must not quietly change who you
    /// are talking to -- that is the whole point of saying it -- but a choice being honoured
    /// or a suggestion being taken is the system working, and narrating that every turn is
    /// how a useful notice becomes noise someone turns off.
    pub fn notice(&self, persona: &Persona) -> Option<String> {
        let Reason::Missing(gone) = &self.reason else {
            return None;
        };
        let speciality = persona.speciality();
        Some(match &self.model {
            Some(now) => format!(
                "{gone} is no longer installed, so \"{speciality}\" is running on {now} for \
                 now. Your choice is kept -- reinstall {gone} and it goes back."
            ),
            None => format!(
                "{gone} is no longer installed, so \"{speciality}\" is running on the general \
                 model for now. Your choice is kept -- reinstall {gone} and it goes back."
            ),
        })
    }
}

/// The operator's picks, as stored.
pub fn choices(db: &MemoryDb) -> HashMap<String, String> {
    db.get_setting(MODELS_SETTING)
        .ok()
        .flatten()
        .and_then(|value| serde_json::from_value::<HashMap<String, String>>(value).ok())
        .unwrap_or_default()
}

/// Records one persona's pick, or clears it when `model` is None.
///
/// Clearing is a real operation rather than storing an empty string: "no pick" and "picked
/// nothing" would otherwise be the same stored value and different intentions, and the
/// difference is exactly whether the suggestion is allowed to apply.
pub fn set_choice(db: &MemoryDb, persona: &Persona, model: Option<&str>) -> Result<(), String> {
    let mut all = choices(db);
    match model {
        Some(model) if !model.trim().is_empty() => {
            all.insert(persona.key().to_string(), model.trim().to_string());
        }
        _ => {
            all.remove(persona.key());
        }
    }
    db.set_setting(
        MODELS_SETTING,
        &serde_json::to_value(&all).map_err(|e| format!("could not store the choice: {e}"))?,
    )
    .map_err(|e| format!("could not store the choice: {e}"))
}

/// What this persona should run on, given what is installed.
///
/// `available` empty means nothing could be asked -- the server is down, or the scan has
/// not run. That is deliberately treated as "do not second-guess anything": with no list to
/// check against, a stored choice cannot be known to be missing, and reporting it as such
/// would tell the operator their model was uninstalled every time their server was briefly
/// unreachable.
pub fn resolve(
    db: &MemoryDb,
    persona: &Persona,
    available: &[String],
    speeds: &HashMap<String, f64>,
) -> Choice {
    let chosen = choices(db).get(persona.key()).cloned();

    if available.is_empty() {
        return match chosen {
            Some(model) => Choice {
                model: Some(model),
                reason: Reason::Chosen,
            },
            None => Choice {
                model: None,
                reason: Reason::General,
            },
        };
    }

    let installed = |name: &str| {
        available
            .iter()
            .any(|model| model.eq_ignore_ascii_case(name))
    };

    match chosen {
        Some(model) if installed(&model) => Choice {
            model: Some(model),
            reason: Reason::Chosen,
        },
        Some(gone) => Choice {
            model: suggestion(persona, available, speeds),
            reason: Reason::Missing(gone),
        },
        None => match suggestion(persona, available, speeds) {
            Some(model) => Choice {
                model: Some(model),
                reason: Reason::Suggested,
            },
            None => Choice {
                model: None,
                reason: Reason::General,
            },
        },
    }
}

/// Measured speeds, keyed by lowercase model name, for breaking ties.
pub fn measured_speeds(db: &MemoryDb) -> HashMap<String, f64> {
    db.benchmarks()
        .unwrap_or_default()
        .into_iter()
        .map(|bench| (bench.model.to_ascii_lowercase(), bench.average_tps))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db(name: &str) -> MemoryDb {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "aether1_routing_{name}_{}_{n}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        MemoryDb::open(path).expect("temp db should open")
    }

    fn models(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    fn no_speeds() -> HashMap<String, f64> {
        HashMap::new()
    }

    #[test]
    fn a_coder_model_is_suggested_for_the_coding_speciality() {
        let available = models(&["llama3.2:3b", "qwen2.5-coder:7b", "mistral:7b"]);
        assert_eq!(
            suggestion(&Persona::Nexus, &available, &no_speeds()).as_deref(),
            Some("qwen2.5-coder:7b")
        );
    }

    #[test]
    fn the_same_machine_suggests_something_else_for_conversation() {
        let available = models(&["llama3.2:3b", "qwen2.5-coder:7b", "mistral:7b"]);
        let chatty = suggestion(&Persona::Halcy, &available, &no_speeds());
        assert_eq!(
            chatty.as_deref(),
            Some("llama3.2:3b"),
            "the point of the table is that one machine answers differently per speciality"
        );
    }

    #[test]
    fn a_general_family_does_not_swallow_the_coder_of_the_same_name() {
        // `qwen` alone would match `qwen2.5-coder` too, which is why the table is ordered
        // most-specific first. Pinned here because reordering the table would break it
        // silently otherwise.
        let available = models(&["qwen2.5-coder:7b"]);
        assert_eq!(
            suggestion(&Persona::Nexus, &available, &no_speeds()).as_deref(),
            Some("qwen2.5-coder:7b")
        );
    }

    #[test]
    fn speed_breaks_a_tie_within_a_family_and_nothing_more() {
        let available = models(&["qwen2.5-coder:32b", "qwen2.5-coder:7b"]);
        let mut speeds = HashMap::new();
        speeds.insert("qwen2.5-coder:32b".to_string(), 4.0);
        speeds.insert("qwen2.5-coder:7b".to_string(), 48.0);
        assert_eq!(
            suggestion(&Persona::Nexus, &available, &speeds).as_deref(),
            Some("qwen2.5-coder:7b")
        );

        // But it never reaches across families: a very fast chat model does not become the
        // suggestion for code.
        let mixed = models(&["qwen2.5-coder:32b", "gemma2:2b"]);
        let mut fast_chat = HashMap::new();
        fast_chat.insert("gemma2:2b".to_string(), 120.0);
        assert_eq!(
            suggestion(&Persona::Nexus, &mixed, &fast_chat).as_deref(),
            Some("qwen2.5-coder:32b"),
            "speed is a tiebreak within a family, not a way to route code at whatever is fastest"
        );
    }

    #[test]
    fn with_nothing_measured_the_smaller_model_of_a_family_is_suggested() {
        // Found by running it: sorting ties by name alone put `qwen2.5-coder:32b` ahead of
        // `:7b`, so a fresh install was pointed at the biggest model on the machine.
        let available = models(&["qwen2.5-coder:32b", "qwen2.5-coder:7b"]);
        assert_eq!(
            suggestion(&Persona::Nexus, &available, &no_speeds()).as_deref(),
            Some("qwen2.5-coder:7b")
        );
    }

    #[test]
    fn a_size_is_read_off_the_tag_and_not_off_the_version_number() {
        assert_eq!(parameter_billions("qwen2.5-coder:7b"), 70);
        assert_eq!(parameter_billions("qwen2.5-coder:32b-instruct-q4"), 320);
        assert_eq!(parameter_billions("llama3.2:3b"), 30);
        assert_eq!(parameter_billions("Qwen2.5-Coder-7B-Instruct-GGUF"), 70);
        assert_eq!(parameter_billions("something:1.5b"), 15);
        assert_eq!(
            parameter_billions("qwen2.5-coder:base"),
            u32::MAX,
            "a name claiming no size must sort last rather than reading as tiny"
        );
        assert_eq!(
            parameter_billions("model-8bit:latest"),
            u32::MAX,
            "\"8bit\" is a quantisation, not eight billion parameters"
        );
    }

    #[test]
    fn a_measured_speed_still_beats_the_size_guess() {
        // Size is only the answer while nothing is known. Once this machine has actually
        // run them, measurement wins -- the guess exists to cover the first run, not to
        // override what the machine has since demonstrated.
        let available = models(&["qwen2.5-coder:32b", "qwen2.5-coder:7b"]);
        let mut speeds = HashMap::new();
        speeds.insert("qwen2.5-coder:32b".to_string(), 60.0);
        speeds.insert("qwen2.5-coder:7b".to_string(), 12.0);
        assert_eq!(
            suggestion(&Persona::Nexus, &available, &speeds).as_deref(),
            Some("qwen2.5-coder:32b")
        );
    }

    #[test]
    fn nothing_suitable_installed_is_answered_with_nothing_rather_than_a_guess() {
        let available = models(&["some-model-nobody-has-heard-of:1b"]);
        assert_eq!(suggestion(&Persona::Nexus, &available, &no_speeds()), None);
        assert_eq!(good_at("some-model-nobody-has-heard-of:1b"), None);
    }

    #[test]
    fn an_operators_pick_beats_the_suggestion() {
        let db = temp_db("pick_wins");
        let available = models(&["qwen2.5-coder:7b", "llama3.2:3b"]);
        set_choice(&db, &Persona::Nexus, Some("llama3.2:3b")).unwrap();

        let choice = resolve(&db, &Persona::Nexus, &available, &no_speeds());
        assert_eq!(choice.model.as_deref(), Some("llama3.2:3b"));
        assert_eq!(choice.reason, Reason::Chosen);
        assert!(
            choice.notice(&Persona::Nexus).is_none(),
            "the system working is not something to narrate every turn"
        );
    }

    #[test]
    fn clearing_a_pick_lets_the_suggestion_apply_again() {
        let db = temp_db("clear_pick");
        let available = models(&["qwen2.5-coder:7b", "llama3.2:3b"]);
        set_choice(&db, &Persona::Nexus, Some("llama3.2:3b")).unwrap();
        set_choice(&db, &Persona::Nexus, None).unwrap();

        let choice = resolve(&db, &Persona::Nexus, &available, &no_speeds());
        assert_eq!(choice.model.as_deref(), Some("qwen2.5-coder:7b"));
        assert_eq!(choice.reason, Reason::Suggested);
        assert!(
            !choices(&db).contains_key(Persona::Nexus.key()),
            "cleared means gone from the map, not stored as an empty string"
        );
    }

    #[test]
    fn a_model_that_was_uninstalled_falls_back_and_says_so_without_losing_the_choice() {
        let db = temp_db("uninstalled");
        set_choice(&db, &Persona::Nexus, Some("codellama:13b")).unwrap();
        let available = models(&["qwen2.5-coder:7b"]);

        let choice = resolve(&db, &Persona::Nexus, &available, &no_speeds());
        assert_eq!(choice.model.as_deref(), Some("qwen2.5-coder:7b"));
        assert_eq!(choice.reason, Reason::Missing("codellama:13b".to_string()));

        let notice = choice
            .notice(&Persona::Nexus)
            .expect("uninstalling a model must not silently change who you are talking to");
        assert!(notice.contains("codellama:13b"));
        assert!(notice.contains("qwen2.5-coder:7b"));

        assert_eq!(
            choices(&db).get(Persona::Nexus.key()).map(String::as_str),
            Some("codellama:13b"),
            "the choice is kept, so reinstalling puts it back without retyping"
        );
    }

    #[test]
    fn an_unreachable_server_is_not_read_as_every_model_being_uninstalled() {
        let db = temp_db("server_down");
        set_choice(&db, &Persona::Nexus, Some("codellama:13b")).unwrap();

        let choice = resolve(&db, &Persona::Nexus, &[], &no_speeds());
        assert_eq!(choice.model.as_deref(), Some("codellama:13b"));
        assert_eq!(
            choice.reason,
            Reason::Chosen,
            "with no list to check against, a choice cannot be known to be missing"
        );
        assert!(choice.notice(&Persona::Nexus).is_none());
    }

    #[test]
    fn a_speciality_with_nothing_suited_leaves_the_general_model_alone() {
        let db = temp_db("general");
        let available = models(&["qwen2.5-coder:7b"]);

        // Nothing in the table claims a coder model suits cost accounting.
        let choice = resolve(&db, &Persona::ArxLucre, &available, &no_speeds());
        assert_eq!(choice.model, None);
        assert_eq!(choice.reason, Reason::General);
    }

    #[test]
    fn a_pick_for_one_speciality_is_not_a_pick_for_another() {
        let db = temp_db("scoped");
        set_choice(&db, &Persona::Nexus, Some("codellama:13b")).unwrap();
        assert!(!choices(&db).contains_key(Persona::Halcy.key()));
    }

    #[test]
    fn the_table_never_suggests_a_model_that_is_not_installed() {
        // The one property the whole feature rests on: a dropdown offering something the
        // machine cannot run is worse than an empty dropdown.
        let available = models(&["llama3.2:3b"]);
        for persona in Persona::all() {
            if let Some(model) = suggestion(&persona, &available, &no_speeds()) {
                assert!(
                    available.contains(&model),
                    "{} was suggested {model}, which is not installed",
                    persona.key()
                );
            }
        }
    }

    #[test]
    fn every_family_in_the_table_suits_at_least_one_persona_and_says_what_it_is_good_at() {
        for family in FAMILIES {
            assert!(
                !family.suits.is_empty(),
                "a family that suits nobody is a row that can never fire"
            );
            assert!(!family.good_at.is_empty());
            assert!(
                !family.needles.is_empty(),
                "a family with no needles would match every model"
            );
        }
    }
}
