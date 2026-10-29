// Self-tuning: the prompts AETHER1 ships are a starting point, and this measures whether a
// different wording does better on this machine's model.
//
// The idea is Hermit's (github.com/Infini-AI-Lab/hermit): leave the model frozen and evolve
// the text wrapped around it -- the instructions, the rules, the skill descriptions -- against
// a set of tasks, keeping a change only when it scores better. Nothing of Hermit's code is
// here and none could be: it is Python training tooling that runs beside a model server,
// whereas this has to run inside the app, on the operator's machine, with no network.
//
// What that buys, concretely. A prompt in this binary was written against the models its
// author had, and the operator is running a different one. The usual way that gap is closed
// is somebody reporting that the companion "won't search" and three sessions reasoning about
// the model -- which has happened here repeatedly, and twice turned out to be an instruction
// in the prompt being obeyed exactly. A loop that can *measure* a wording against cases with
// known answers turns that argument into a number.
//
// Four decisions worth stating, because each has an obvious worse alternative:
//
//   1. **The model never changes.** No fine-tuning, no LoRA, no weights. Only text, which is
//      readable, diffable, revertible, and costs nothing to ship. Everything this module
//      stores can be printed and read by the person whose machine it ran on.
//   2. **A change is kept only if it scores strictly better.** Not "better or equal":
//      equal keeps the incumbent, so a tuned prompt does not drift through a series of
//      sideways rewrites that each looked fine.
//   3. **Only adapters with a checkable score are tunable.** The first one is the Auto
//      router's judge prompt, and that is not an accident of ordering -- its task has a
//      right answer, so a case either passes or does not. An adapter scored by a model's
//      opinion of a reply would be a loop optimising against a second model's taste, and it
//      would read exactly like progress.
//   4. **It is run by hand, never on a timer.** A round is dozens of local generations.
//      Something that quietly spent an evening of somebody's GPU to reword a prompt would
//      be the opposite of what this is for.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{providers, MemoryDb, Provider};

/// Which piece of text an adapter wraps.
///
/// An enum rather than a free string so that a typo at the command line is a refusal with
/// the names in it, and so that adding a tunable prompt means writing down its default and
/// its eval set in the same place as everything else about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Adapter {
    /// The Auto router's judge prompt -- see `llm::auto`. First because its answers are
    /// checkable: a case says what the verdict should be, and the model either gets it or
    /// does not.
    Judge,
}

impl Adapter {
    pub const ALL: &'static [Adapter] = &[Adapter::Judge];

    pub fn key(self) -> &'static str {
        match self {
            Adapter::Judge => "judge",
        }
    }

    pub fn from_key(key: &str) -> Option<Adapter> {
        Adapter::ALL
            .iter()
            .copied()
            .find(|a| a.key() == key.to_ascii_lowercase())
    }

    /// What this adapter is for, in the operator's words.
    pub fn describes(self) -> &'static str {
        match self {
            Adapter::Judge => {
                "the instructions the Auto router gives the small model that sorts each \
                 question"
            }
        }
    }

    /// The text compiled into the binary: the baseline every tuned version is measured
    /// against, and what is used when nothing has beaten it.
    pub fn built_in(self) -> String {
        match self {
            Adapter::Judge => super::auto::built_in_judge_prompt(),
        }
    }

    /// The setting the stored versions live under. One key with a document inside, mirroring
    /// `persona_models` and `domain_extra_roots`, because a settings table with a row per
    /// version is a settings table nobody can read.
    fn setting(self) -> String {
        format!("adapter_{}", self.key())
    }
}

/// One wording of an adapter, and what it scored.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Version {
    pub text: String,
    /// Cases passed out of cases run, at the time it was scored. Stored rather than
    /// recomputed because the set can grow: a version scored against twelve cases and one
    /// scored against twenty are not comparable, and `cases` is what says so.
    pub passed: usize,
    pub cases: usize,
    /// Which model produced the score. A wording tuned for a 3B is not thereby better on a
    /// 14B, and a stored score that does not say what ran it is a number with no meaning.
    pub model: String,
    /// When, as an ISO date. For reading, not for ordering -- the order is the vector's.
    pub made: String,
    /// How this wording came about, for somebody reading the history back.
    pub note: String,
}

/// Every wording tried for one adapter, and which one is in force.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct History {
    #[serde(default)]
    pub versions: Vec<Version>,
    /// The index into `versions` that is in force, or None for the built-in text. None is
    /// the state of every install until a round beats the baseline, so it is the default
    /// rather than a special case.
    #[serde(default)]
    pub active: Option<usize>,
}

impl History {
    /// The text to actually use. The built-in whenever nothing has displaced it, including
    /// when a stored index has gone out of range -- a corrupted setting falls back to the
    /// shipped prompt rather than to nothing.
    pub fn text_for(&self, adapter: Adapter) -> String {
        self.active
            .and_then(|index| self.versions.get(index))
            .map(|version| version.text.clone())
            .unwrap_or_else(|| adapter.built_in())
    }

    pub fn active_version(&self) -> Option<&Version> {
        self.active.and_then(|index| self.versions.get(index))
    }
}

/// Reads one adapter's history. A missing or unparseable setting is an empty history, which
/// means the built-in text -- never a failure, because this is read on the path to a reply.
pub fn history(db: &MemoryDb, adapter: Adapter) -> History {
    let stored = db.get_setting_string(&adapter.setting(), "");
    if stored.trim().is_empty() {
        return History::default();
    }
    serde_json::from_str(&stored).unwrap_or_default()
}

/// The text in force for one adapter.
pub fn text(db: &MemoryDb, adapter: Adapter) -> String {
    history(db, adapter).text_for(adapter)
}

fn save(db: &MemoryDb, adapter: Adapter, history: &History) -> Result<(), String> {
    let encoded =
        serde_json::to_string(history).map_err(|e| format!("could not encode the history: {e}"))?;
    db.set_setting(&adapter.setting(), &json!(encoded))
        .map_err(|e| format!("could not save the history: {e}"))
}

/// Puts the built-in text back in force, keeping the history so a reverted version can be
/// read and compared rather than disappearing.
pub fn revert(db: &MemoryDb, adapter: Adapter) -> Result<(), String> {
    let mut history = history(db, adapter);
    history.active = None;
    save(db, adapter, &history)
}

// ------------------------------------------------------------------- the eval set

/// One case: something somebody might type, and what the right answer is.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Case {
    pub message: String,
    /// The expected answer, field by field, as the adapter's own schema shapes it. A map
    /// rather than a typed verdict so that a second adapter with a different answer shape
    /// needs no change here.
    pub expect: serde_json::Map<String, Value>,
}

/// How one case went.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub message: String,
    /// Which fields were right, and which were not, by name.
    pub wrong: Vec<String>,
    /// Filled when the model did not produce a usable answer at all, which is a different
    /// failure from a wrong one and has to be reported as one: a wording that makes the
    /// model answer in prose is broken, not merely inaccurate.
    pub failed: Option<String>,
}

impl Outcome {
    pub fn passed(&self) -> bool {
        self.failed.is_none() && self.wrong.is_empty()
    }
}

/// Compares one answer against what a case expected.
///
/// Field by field, and only the fields the case names: a case that cares about
/// `needs_fresh_facts` and says nothing about `is_code` is not thereby asserting that
/// `is_code` is false. Scoring an unstated field would make every case an assertion about
/// everything, and a set of those is unwritable by hand.
pub fn judge_case(case: &Case, answer: &Value) -> Outcome {
    let mut wrong = Vec::new();
    for (field, expected) in &case.expect {
        match answer.get(field) {
            Some(actual) if actual == expected => {}
            Some(_) => wrong.push(field.clone()),
            // An absent field is wrong rather than skipped -- the schema requires every
            // field, so a missing one is the model having got round the schema.
            None => wrong.push(field.clone()),
        }
    }
    Outcome {
        message: case.message.clone(),
        wrong,
        failed: None,
    }
}

/// The cases shipped with the binary for one adapter.
///
/// Deliberately small and deliberately boring. Every case is one where a reasonable person
/// would agree about the answer, because a set with arguable cases in it tunes the prompt
/// towards whoever wrote the set. The set is a floor, not a benchmark: `cases` below adds
/// the operator's own on top, and theirs are the ones that describe how they actually use it.
pub fn built_in_cases(adapter: Adapter) -> Vec<Case> {
    let case = |message: &str, difficulty: &str, fresh: bool, code: bool| Case {
        message: message.to_string(),
        expect: serde_json::Map::from_iter([
            ("difficulty".to_string(), json!(difficulty)),
            ("needs_fresh_facts".to_string(), json!(fresh)),
            ("is_code".to_string(), json!(code)),
        ]),
    };
    match adapter {
        Adapter::Judge => vec![
            case("hello", "simple", false, false),
            case("what's your name?", "simple", false, false),
            case(
                "how many days are in February 2024?",
                "simple",
                false,
                false,
            ),
            case("thanks, that worked", "simple", false, false),
            case("what time is it?", "simple", true, false),
            case(
                "what's the latest version of ollama?",
                "simple",
                true,
                false,
            ),
            case(
                "how much disk space is left on this machine?",
                "simple",
                true,
                false,
            ),
            case("who won the last Formula 1 race?", "simple", true, false),
            case(
                "write a Rust function that parses a duration like 1h30m",
                "hard",
                false,
                true,
            ),
            case(
                "why does my borrow checker complain about this closure?",
                "hard",
                false,
                true,
            ),
            case(
                "what does this systemd unit file actually do, line by line?",
                "hard",
                false,
                true,
            ),
            case(
                "draft a two-page argument for why local models matter, with counterarguments",
                "hard",
                false,
                false,
            ),
            case(
                "plan a week of meals for two people on a budget, with a shopping list",
                "hard",
                false,
                false,
            ),
            case(
                "read the changelog for the latest Tauri release and tell me what breaks",
                "hard",
                true,
                true,
            ),
        ],
    }
}

/// Where an operator's own cases live: one JSON file per adapter, beside the database.
///
/// A file rather than a settings key, and rather than a pane: this is a thing somebody
/// edits in a text editor while they think of cases, and a thing they would want to keep
/// in version control. The format is the same `Case` the built-in set uses, as a JSON array.
pub fn cases_path(db: &MemoryDb, adapter: Adapter) -> Option<std::path::PathBuf> {
    db.dir()
        .map(|dir| dir.join("evals").join(format!("{}.json", adapter.key())))
}

/// Every case for one adapter: the shipped ones, then the operator's.
///
/// Returns the cases and whatever went wrong reading the operator's file. A malformed file
/// is reported and skipped rather than failing the run -- a typo in a case somebody added
/// should not stop them scoring the prompt, but it must not be silent either, or the set
/// they think they are measuring against is not the one that ran.
pub fn cases(db: &MemoryDb, adapter: Adapter) -> (Vec<Case>, Option<String>) {
    let mut all = built_in_cases(adapter);
    let Some(path) = cases_path(db, adapter) else {
        return (all, None);
    };
    if !path.exists() {
        return (all, None);
    }
    match std::fs::read_to_string(&path)
        .map_err(|e| format!("could not read {}: {e}", path.display()))
        .and_then(|raw| {
            serde_json::from_str::<Vec<Case>>(&raw)
                .map_err(|e| format!("{} is not a list of cases: {e}", path.display()))
        }) {
        Ok(extra) => {
            all.extend(extra);
            (all, None)
        }
        Err(why) => (all, Some(why)),
    }
}

// ------------------------------------------------------------------- scoring a wording

/// Where the scoring sends its calls, and what it reports back as it goes.
///
/// The trait is what keeps this module testable. Everything above is pure, and the only
/// thing below that needs a model server is this one method -- so the scoring loop, the
/// keep-if-better rule and the report are all exercised in tests against a stand-in that
/// answers from a table.
pub trait Judge {
    /// Asks one case under one wording. `Err` is "no usable answer", which is a distinct
    /// outcome rather than an error to propagate -- one case the model fluffed should not
    /// abandon a round.
    fn ask(&self, prompt: &str, message: &str) -> Result<Value, String>;
    /// Which model is answering, for the stored score.
    fn model(&self) -> String;
}

/// A judge that actually calls the configured local server. The same path a routed turn
/// takes, deliberately: a wording scored through a different code path from the one that
/// will use it is a wording scored against something else.
pub struct LocalJudge {
    pub provider: Provider,
    pub endpoint: String,
    pub model: String,
    pub schema: Value,
}

impl Judge for LocalJudge {
    fn ask(&self, prompt: &str, message: &str) -> Result<Value, String> {
        providers::judge(
            self.provider,
            &self.endpoint,
            &self.model,
            providers::Ask {
                system_prompt: prompt,
                question: message,
                schema: &self.schema,
                budget: providers::VERDICT_TOKENS,
                // Nothing is in front of a reply, and the first case of a round pays the
                // model load for all the ones after it.
                pace: providers::Pace::Unhurried,
            },
        )
    }
    fn model(&self) -> String {
        self.model.clone()
    }
}

/// Which model scores the cases: the same one the router would judge with, so a wording is
/// tuned for the model that will actually be using it.
pub fn judge_model_for(db: &MemoryDb, available: &super::auto::Candidates) -> Option<String> {
    // No turn model and nothing claimed to be loaded: a tuning round is not in front of a
    // reply, so the one thing that matters here is *which* model, not what it costs to
    // reach it. `run_round` pays a load once and scores dozens of cases on it after that.
    available.judge_choice(db, "", &[]).map(|(model, _)| model)
}

/// Which model does the rewriting: the largest installed, by the size in its tag.
///
/// The opposite tie-break from the judge's, and deliberately. Sorting a question is a
/// classification a 3B does well and is paid for before every reply; rewriting a page of
/// instructions is a writing job done once, while somebody waits on purpose, and it is the
/// one place in this module where a slow answer is the right trade. An unlabelled name is
/// the last resort rather than the first, for the same reason it is in
/// `Candidates::judge_choice`.
pub fn rewriter_model_for(available: &super::auto::Candidates) -> Option<String> {
    let labelled = available
        .local
        .iter()
        .filter(|c| super::routing::parameter_billions(&c.model) != u32::MAX)
        .max_by_key(|c| super::routing::parameter_billions(&c.model));
    labelled
        .or_else(|| available.local.first())
        .map(|c| c.model.clone())
}

/// How many tokens a rewritten set of instructions may take.
///
/// Generous, because the thing being produced is the whole of a prompt and a truncated one
/// scores badly for a reason that has nothing to do with its wording -- which would then be
/// recorded as evidence against it.
const REWRITE_TOKENS: u32 = 1400;

/// Asks a local model to reword a prompt, with the answer constrained to one string field.
///
/// The constraint is doing real work here, and not the obvious work. A model asked for a
/// replacement prompt in prose returns it wrapped in a fence, or behind "Here are the
/// revised instructions:", or with a note at the end about what it changed -- and every one
/// of those then *scores*, so the damage arrives as a number rather than as an obvious mess.
/// A schema with one string in it makes the server hand back the instructions and nothing
/// else. `clean_rewrite` still runs afterwards, for the server that has no grammar support
/// and ignored the field.
pub fn reword_with(
    available: &super::auto::Candidates,
    model: &str,
    prompt: &str,
) -> Result<String, String> {
    let answer = providers::judge(
        available.local_provider,
        &available.endpoint,
        model,
        providers::Ask {
            system_prompt: "You rewrite instructions. You reply with the rewritten \
                            instructions and nothing else: no preamble, no explanation, no \
                            markdown.",
            question: prompt,
            schema: &json!({
                "type": "object",
                "properties": {"instructions": {"type": "string"}},
                "required": ["instructions"],
                "additionalProperties": false,
            }),
            budget: REWRITE_TOKENS,
            pace: providers::Pace::Unhurried,
        },
    )?;
    answer
        .get("instructions")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| "the rewrite came back without any instructions in it".to_string())
}

/// What one wording scored, case by case.
#[derive(Clone, Debug)]
pub struct Score {
    pub outcomes: Vec<Outcome>,
}

impl Score {
    pub fn passed(&self) -> usize {
        self.outcomes.iter().filter(|o| o.passed()).count()
    }
    pub fn cases(&self) -> usize {
        self.outcomes.len()
    }
    /// The cases that went wrong, for showing the next wording what it has to fix.
    pub fn failures(&self) -> Vec<&Outcome> {
        self.outcomes.iter().filter(|o| !o.passed()).collect()
    }
}

/// Runs every case under one wording. `watch` is called after each case so a round at a
/// terminal prints as it goes rather than going quiet for a minute a wording.
pub fn score(
    judge: &dyn Judge,
    prompt: &str,
    cases: &[Case],
    watch: &mut dyn FnMut(&Outcome),
) -> Score {
    let mut outcomes = Vec::with_capacity(cases.len());
    for case in cases {
        let outcome = match judge.ask(prompt, &case.message) {
            Ok(answer) => judge_case(case, &answer),
            Err(why) => Outcome {
                message: case.message.clone(),
                wrong: Vec::new(),
                failed: Some(why),
            },
        };
        watch(&outcome);
        outcomes.push(outcome);
    }
    Score { outcomes }
}

/// Whether a candidate displaces what is in force.
///
/// Strictly better, and only against the same number of cases. Equal keeps the incumbent, so
/// a prompt cannot drift through a series of sideways rewrites that each looked fine; and a
/// candidate scored against a set that has since grown is not compared at all, because the
/// larger set is the harder one and the comparison would flatter whichever ran first.
pub fn displaces(candidate: &Score, incumbent_passed: usize, incumbent_cases: usize) -> bool {
    candidate.cases() == incumbent_cases && candidate.passed() > incumbent_passed
}

// ------------------------------------------------------------------- proposing a wording

/// What the rewriter is told. It is handed the current wording and the cases it got wrong,
/// and asked for a replacement -- not a critique, and not an explanation, because a model
/// asked to improve a prompt and explain itself returns the explanation with the prompt
/// embedded in it and the next round scores the explanation.
pub fn rewrite_prompt(adapter: Adapter, current: &str, score: &Score) -> String {
    let mut wrong = String::new();
    for outcome in score.failures().iter().take(8) {
        if let Some(why) = &outcome.failed {
            wrong.push_str(&format!(
                "- {:?} -- produced no usable answer at all ({why})\n",
                outcome.message
            ));
        } else {
            wrong.push_str(&format!(
                "- {:?} -- got {} wrong\n",
                outcome.message,
                outcome.wrong.join(", ")
            ));
        }
    }
    if wrong.is_empty() {
        wrong.push_str("- nothing; every case passed\n");
    }
    format!(
        "You are rewriting a set of instructions given to a small language model. The \
         instructions are for {describes}.\n\n\
         Here are the instructions as they stand:\n\n---\n{current}\n---\n\n\
         Scored against {cases} test cases with known right answers, these instructions got \
         {passed} right. These are the cases they got wrong:\n\n{wrong}\n\
         Write a replacement for the instructions above. Keep every field name exactly as it \
         is -- they are fixed by a schema and renaming one breaks everything. Keep it short: \
         a small model follows three clear sentences better than a page. Change the wording, \
         the order, or the examples to fix the failures above without breaking the cases that \
         already pass.\n\n\
         Reply with the replacement instructions and nothing else. No preamble, no \
         explanation, no quotes around them, no markdown fence.",
        describes = adapter.describes(),
        cases = score.cases(),
        passed = score.passed(),
    )
}

/// Cleans up what a rewriter returned.
///
/// Two things every model does anyway, whatever the prompt said: wrapping the answer in a
/// markdown fence, and opening with "Here are the revised instructions:". Both are stripped
/// here rather than prompted away, because a prompt that merely asks for neither still gets
/// them often enough to poison a round -- and the poisoned version then *scores*, so the
/// damage is a number rather than an obvious mess.
pub fn clean_rewrite(raw: &str) -> String {
    let mut text = raw.trim();
    if let Some(rest) = text.strip_prefix("```") {
        // Drop the fence's language tag along with the fence.
        let rest = rest.split_once('\n').map(|(_, body)| body).unwrap_or(rest);
        text = rest
            .rsplit_once("```")
            .map(|(body, _)| body)
            .unwrap_or(rest)
            .trim();
    }
    // A single opening line that announces the answer rather than being part of it. Judged
    // by shape -- one short line, ending in a colon -- rather than by matching phrases,
    // which is a list that is always missing the one this model used.
    if let Some((first, rest)) = text.split_once('\n') {
        let first = first.trim();
        if first.ends_with(':') && first.len() < 90 && !rest.trim().is_empty() {
            text = rest.trim();
        }
    }
    text.to_string()
}

/// One round: score what is in force, ask for a rewrite, score that, keep it if it is
/// better.
///
/// The report is returned rather than printed so the caller decides where it goes -- a
/// terminal, a pane, or a test.
#[derive(Clone, Debug)]
pub struct Round {
    pub baseline: Score,
    /// None when no rewrite could be had, which ends the round without a candidate rather
    /// than with a bad one.
    pub candidate: Option<(String, Score)>,
    pub kept: bool,
    /// Why the candidate was not kept, when it was not. Worth a sentence: "it scored the
    /// same" and "the rewriter did not answer" are different situations and the operator's
    /// next move differs.
    pub verdict: String,
}

/// Runs one round against a live judge and a rewriter, and stores the result.
///
/// `rewriter` is separate from `judge` because they are different jobs on different models:
/// sorting a question is what the smallest installed model is for, and rewriting a set of
/// instructions is what the largest is for. Passing one closure keeps this module out of
/// the business of choosing either.
pub fn run_round(
    db: &MemoryDb,
    adapter: Adapter,
    judge: &dyn Judge,
    rewriter: &dyn Fn(&str) -> Result<String, String>,
    cases: &[Case],
    watch: &mut dyn FnMut(&str),
) -> Result<Round, String> {
    if cases.is_empty() {
        return Err("there are no cases to score against".to_string());
    }
    let mut history = history(db, adapter);
    let current = history.text_for(adapter);

    watch("scoring the wording in force");
    let baseline = score(judge, &current, cases, &mut |outcome| {
        watch(&one_line(outcome));
    });
    watch(&format!(
        "in force: {}/{} cases",
        baseline.passed(),
        baseline.cases()
    ));

    if baseline.passed() == baseline.cases() {
        return Ok(Round {
            baseline,
            candidate: None,
            kept: false,
            verdict: "every case already passes, so there is nothing to tune. Add cases of \
                      your own -- a set that is all passes measures nothing."
                .to_string(),
        });
    }

    watch("asking for a rewrite");
    let proposed = match rewriter(&rewrite_prompt(adapter, &current, &baseline)) {
        Ok(raw) => clean_rewrite(&raw),
        Err(why) => {
            return Ok(Round {
                baseline,
                candidate: None,
                kept: false,
                verdict: format!("no rewrite could be had, so nothing changed: {why}"),
            })
        }
    };
    if proposed.trim().is_empty() || proposed == current {
        return Ok(Round {
            baseline,
            candidate: None,
            kept: false,
            verdict: "the rewrite came back empty or unchanged, so nothing changed.".to_string(),
        });
    }

    watch("scoring the rewrite");
    let candidate = score(judge, &proposed, cases, &mut |outcome| {
        watch(&one_line(outcome));
    });
    watch(&format!(
        "rewrite: {}/{} cases",
        candidate.passed(),
        candidate.cases()
    ));

    let kept = displaces(&candidate, baseline.passed(), baseline.cases());
    let verdict = if kept {
        format!(
            "kept: {}/{} beats {}/{}.",
            candidate.passed(),
            candidate.cases(),
            baseline.passed(),
            baseline.cases()
        )
    } else {
        format!(
            "discarded: {}/{} does not beat {}/{}, and equal keeps what is in force.",
            candidate.passed(),
            candidate.cases(),
            baseline.passed(),
            baseline.cases()
        )
    };

    // Recorded either way. A wording that was tried and lost is worth keeping: it stops the
    // next round proposing the same thing, and it is the evidence that the one in force was
    // not just the first thing anybody wrote.
    history.versions.push(Version {
        text: proposed.clone(),
        passed: candidate.passed(),
        cases: candidate.cases(),
        model: judge.model(),
        made: today(),
        note: if kept {
            "kept -- scored better than the wording in force".to_string()
        } else {
            "tried and discarded -- did not score better".to_string()
        },
    });
    if kept {
        history.active = Some(history.versions.len() - 1);
    }
    save(db, adapter, &history)?;

    Ok(Round {
        baseline,
        candidate: Some((proposed, candidate)),
        kept,
        verdict,
    })
}

fn one_line(outcome: &Outcome) -> String {
    let message: String = outcome.message.chars().take(56).collect();
    if let Some(why) = &outcome.failed {
        format!("  ! {message} -- no usable answer ({why})")
    } else if outcome.wrong.is_empty() {
        format!("  . {message}")
    } else {
        format!("  x {message} -- wrong: {}", outcome.wrong.join(", "))
    }
}

/// Today, as `YYYY-MM-DD`, for a stored version's date.
fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs / 86_400;
    // Civil-from-days, so a date needs no dependency. The epoch shift puts the leap day at
    // the end of the 400-year era, which is the whole trick.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db_for(name: &str) -> MemoryDb {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "aether1_tuning_{name}_{}_{n}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        MemoryDb::open(path).expect("temp db should open")
    }

    fn case(message: &str, difficulty: &str) -> Case {
        Case {
            message: message.to_string(),
            expect: serde_json::Map::from_iter([("difficulty".to_string(), json!(difficulty))]),
        }
    }

    /// A word that appears in the shipped judge prompt, so a `TableJudge` keyed on it
    /// treats the wording in force as the good one. Asserted below, because a marker that
    /// has drifted out of the prompt makes several tests pass for the wrong reason.
    const IN_SHIPPED: &str = "sort";

    /// A judge that answers from a table, so every branch of the loop is testable without a
    /// model server. The marker is how a test makes one wording better than another: a
    /// prompt containing it gets `good_at` right, and anything else gets nothing right.
    struct TableJudge {
        good_marker: &'static str,
        /// Messages the *good* wording gets right. Everything else it gets wrong, and the
        /// bad wording gets everything wrong.
        good_at: Vec<&'static str>,
    }

    impl Judge for TableJudge {
        fn ask(&self, prompt: &str, message: &str) -> Result<Value, String> {
            let good = prompt.contains(self.good_marker) && self.good_at.contains(&message);
            Ok(json!({"difficulty": if good { "hard" } else { "simple" }}))
        }
        fn model(&self) -> String {
            "table:1b".to_string()
        }
    }

    #[test]
    fn nothing_stored_means_the_shipped_wording() {
        let db = db_for("default");
        assert_eq!(
            text(&db, Adapter::Judge),
            Adapter::Judge.built_in(),
            "an install that has never tuned uses exactly what shipped"
        );
    }

    #[test]
    fn a_corrupt_setting_falls_back_to_the_shipped_wording() {
        let db = db_for("corrupt");
        db.set_setting("adapter_judge", &json!("{not json at all"))
            .unwrap();
        assert_eq!(text(&db, Adapter::Judge), Adapter::Judge.built_in());

        // And so does an index pointing at a version that is not there -- this is read on
        // the path to a reply, so there is no safe way for it to fail.
        let history = History {
            versions: vec![],
            active: Some(3),
        };
        db.set_setting(
            "adapter_judge",
            &json!(serde_json::to_string(&history).unwrap()),
        )
        .unwrap();
        assert_eq!(text(&db, Adapter::Judge), Adapter::Judge.built_in());
    }

    #[test]
    fn only_the_fields_a_case_names_are_scored() {
        let case = case("hello", "simple");
        let outcome = judge_case(
            &case,
            &json!({"difficulty": "simple", "needs_fresh_facts": true, "is_code": true}),
        );
        assert!(
            outcome.passed(),
            "a case silent about a field asserts nothing about it"
        );

        let outcome = judge_case(&case, &json!({"difficulty": "hard"}));
        assert_eq!(outcome.wrong, vec!["difficulty".to_string()]);
    }

    #[test]
    fn a_field_the_model_left_out_is_wrong_rather_than_skipped() {
        let outcome = judge_case(&case("hello", "simple"), &json!({"is_code": false}));
        assert_eq!(
            outcome.wrong,
            vec!["difficulty".to_string()],
            "the schema requires it, so an absent field is the model having got round it"
        );
    }

    #[test]
    fn equal_does_not_displace() {
        let cases = vec![case("a", "simple"), case("b", "hard")];
        let judge = TableJudge {
            good_marker: "BETTER",
            good_at: vec!["b"],
        };
        let candidate = score(&judge, "a wording", &cases, &mut |_| {});
        assert_eq!(candidate.passed(), 1);
        assert!(
            !displaces(&candidate, 1, 2),
            "equal keeps the incumbent, so a prompt cannot drift sideways"
        );
        assert!(displaces(&candidate, 0, 2));
    }

    #[test]
    fn a_score_against_a_different_number_of_cases_is_not_compared() {
        let cases = vec![case("a", "simple")];
        let judge = TableJudge {
            good_marker: "BETTER",
            good_at: vec![],
        };
        let candidate = score(&judge, "x", &cases, &mut |_| {});
        assert!(
            !displaces(&candidate, 0, 14),
            "a smaller set is the easier one; comparing across sets flatters whichever ran first"
        );
    }

    #[test]
    fn a_better_wording_is_kept_and_becomes_what_is_in_force() {
        let db = db_for("kept");
        let cases = vec![case("a", "hard"), case("b", "hard")];
        let judge = TableJudge {
            good_marker: "BETTER",
            good_at: vec!["a", "b"],
        };
        let round = run_round(
            &db,
            Adapter::Judge,
            &judge,
            &|_| Ok("a BETTER wording".to_string()),
            &cases,
            &mut |_| {},
        )
        .expect("the round runs");
        assert_eq!(round.baseline.passed(), 0);
        assert!(round.kept, "{}", round.verdict);
        assert_eq!(
            text(&db, Adapter::Judge),
            "a BETTER wording",
            "and it is what the next reply will actually use"
        );
    }

    #[test]
    fn the_marker_the_other_tests_lean_on_is_really_in_the_shipped_wording() {
        assert!(
            Adapter::Judge.built_in().contains(IN_SHIPPED),
            "the tests below set the incumbent up to score; if this word has gone from the \
             prompt they would instead be testing a zero against a zero"
        );
    }

    #[test]
    fn a_worse_wording_is_recorded_and_not_used() {
        let db = db_for("discarded");
        // Both expect "hard", so the stand-in's default answer fails them: "b" is a case
        // the incumbent gets wrong, which is what makes this 1 out of 2 rather than a pass.
        let cases = vec![case("a", "hard"), case("b", "hard")];
        // The wording in force already gets "a" right; the rewrite carries no marker, so it
        // gets nothing right.
        let judge = TableJudge {
            good_marker: IN_SHIPPED,
            good_at: vec!["a"],
        };
        let round = run_round(
            &db,
            Adapter::Judge,
            &judge,
            &|_| Ok("a wording with no such word in it".to_string()),
            &cases,
            &mut |_| {},
        )
        .expect("the round runs");
        assert_eq!(
            round.baseline.passed(),
            1,
            "the incumbent really does score, so this is a comparison and not two zeroes"
        );
        assert!(!round.kept, "{}", round.verdict);
        assert!(round.verdict.contains("does not beat"));
        assert_eq!(
            text(&db, Adapter::Judge),
            Adapter::Judge.built_in(),
            "what is in force is untouched by a losing round"
        );
        let history = history(&db, Adapter::Judge);
        assert_eq!(
            history.versions.len(),
            1,
            "the loser is kept as evidence, so the next round does not propose it again"
        );
        assert_eq!(history.active, None);
    }

    #[test]
    fn a_rewriter_that_does_not_answer_ends_the_round_without_a_candidate() {
        let db = db_for("no_rewrite");
        let cases = vec![case("a", "hard")];
        let judge = TableJudge {
            good_marker: "BETTER",
            good_at: vec![],
        };
        let round = run_round(
            &db,
            Adapter::Judge,
            &judge,
            &|_| Err("the server was not there".to_string()),
            &cases,
            &mut |_| {},
        )
        .expect("the round still reports");
        assert!(!round.kept);
        assert!(round.candidate.is_none());
        assert!(round.verdict.contains("the server was not there"));
        assert!(
            history(&db, Adapter::Judge).versions.is_empty(),
            "nothing is recorded, because nothing was tried"
        );
    }

    #[test]
    fn a_full_score_says_there_is_nothing_to_tune_rather_than_tuning() {
        let db = db_for("full");
        let cases = vec![case("a", "hard")];
        let judge = TableJudge {
            good_marker: IN_SHIPPED,
            good_at: vec!["a"],
        };
        // A Cell because the rewriter is an `Fn`: it is called once per round at most, and
        // making it `FnMut` to suit a test would widen the interface for nothing.
        let asked = std::cell::Cell::new(false);
        let round = run_round(
            &db,
            Adapter::Judge,
            &judge,
            &|_| {
                asked.set(true);
                Ok("anything".to_string())
            },
            &cases,
            &mut |_| {},
        )
        .expect("the round reports");
        assert!(
            !asked.get(),
            "no generation is spent on a set that proves nothing"
        );
        assert!(round.verdict.contains("Add cases of your own"));
    }

    #[test]
    fn a_case_the_model_fluffed_is_a_different_failure_from_a_wrong_answer() {
        struct Silent;
        impl Judge for Silent {
            fn ask(&self, _: &str, _: &str) -> Result<Value, String> {
                Err("the judgment was not JSON".to_string())
            }
            fn model(&self) -> String {
                "silent".to_string()
            }
        }
        let score = score(&Silent, "x", &[case("a", "hard")], &mut |_| {});
        assert_eq!(score.passed(), 0);
        let outcome = &score.outcomes[0];
        assert!(outcome.failed.is_some());
        assert!(
            outcome.wrong.is_empty(),
            "a wording that produces prose is broken, not inaccurate, and the report says so"
        );
    }

    #[test]
    fn a_fenced_or_announced_rewrite_is_unwrapped() {
        assert_eq!(
            clean_rewrite("```\nSort the question.\nDo not answer it.\n```"),
            "Sort the question.\nDo not answer it."
        );
        assert_eq!(
            clean_rewrite("```text\nSort the question.\n```"),
            "Sort the question."
        );
        assert_eq!(
            clean_rewrite("Here are the revised instructions:\nSort the question."),
            "Sort the question."
        );
        // A colon inside the instructions themselves is not an announcement, and the whole
        // thing must survive: the fields are named with colons after them.
        assert_eq!(
            clean_rewrite("difficulty: simple or hard"),
            "difficulty: simple or hard"
        );
    }

    #[test]
    fn reverting_keeps_the_history_and_puts_the_shipped_wording_back() {
        let db = db_for("revert");
        let cases = vec![case("a", "hard")];
        let judge = TableJudge {
            good_marker: "BETTER",
            good_at: vec!["a"],
        };
        run_round(
            &db,
            Adapter::Judge,
            &judge,
            &|_| Ok("a BETTER wording".to_string()),
            &cases,
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(text(&db, Adapter::Judge), "a BETTER wording");

        revert(&db, Adapter::Judge).unwrap();
        assert_eq!(text(&db, Adapter::Judge), Adapter::Judge.built_in());
        assert_eq!(
            history(&db, Adapter::Judge).versions.len(),
            1,
            "a reverted wording can still be read and compared"
        );
    }

    #[test]
    fn the_shipped_cases_are_a_mix_worth_scoring_against() {
        let cases = built_in_cases(Adapter::Judge);
        assert!(cases.len() >= 12);
        let hard = cases
            .iter()
            .filter(|c| c.expect["difficulty"] == json!("hard"))
            .count();
        let fresh = cases
            .iter()
            .filter(|c| c.expect["needs_fresh_facts"] == json!(true))
            .count();
        // A set that is all one answer is scored perfectly by a wording that always says
        // that answer, which is the failure mode of every hand-written eval set.
        assert!(
            hard >= 4 && hard <= cases.len() - 4,
            "{hard} hard of {}",
            cases.len()
        );
        assert!(fresh >= 4, "{fresh} needing a lookup");
    }

    #[test]
    fn an_operators_own_cases_are_added_and_a_broken_file_is_reported() {
        let db = db_for("own_cases");
        let shipped = built_in_cases(Adapter::Judge).len();
        let path = cases_path(&db, Adapter::Judge).expect("the database has a directory");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();

        std::fs::write(
            &path,
            serde_json::to_string(&vec![case("something of mine", "hard")]).unwrap(),
        )
        .unwrap();
        let (all, problem) = cases(&db, Adapter::Judge);
        assert_eq!(all.len(), shipped + 1);
        assert!(problem.is_none());

        std::fs::write(&path, "{ not a list }").unwrap();
        let (all, problem) = cases(&db, Adapter::Judge);
        assert_eq!(
            all.len(),
            shipped,
            "a broken file is skipped rather than failing the run"
        );
        assert!(
            problem.is_some_and(|why| why.contains("not a list of cases")),
            "but it is never silent -- otherwise the set that ran is not the one they think"
        );
    }

    #[test]
    fn the_rewrite_prompt_shows_the_failures_and_forbids_renaming_a_field() {
        let cases = vec![case("a", "hard"), case("b", "hard")];
        let judge = TableJudge {
            good_marker: "BETTER",
            good_at: vec!["a"],
        };
        // Scored under a wording that carries the marker, so "a" passes and "b" does not --
        // a prompt shown no failures is not the thing worth asserting about.
        let score = score(&judge, "a BETTER wording", &cases, &mut |_| {});
        let prompt = rewrite_prompt(Adapter::Judge, "the current wording", &score);
        assert!(prompt.contains("the current wording"));
        assert!(
            prompt.contains("\"b\""),
            "the case it got wrong is named: {prompt}"
        );
        assert!(prompt.contains("Keep every field name exactly as it is"));
        assert!(prompt.contains("1 right"), "{prompt}");
    }

    #[test]
    fn an_adapter_is_named_or_refused() {
        assert_eq!(Adapter::from_key("judge"), Some(Adapter::Judge));
        assert_eq!(Adapter::from_key("JUDGE"), Some(Adapter::Judge));
        assert_eq!(Adapter::from_key("jugde"), None);
    }

    #[test]
    fn the_date_is_a_real_date() {
        let today = today();
        assert_eq!(today.len(), 10, "{today}");
        let parts: Vec<&str> = today.split('-').collect();
        assert_eq!(parts.len(), 3);
        let year: u32 = parts[0].parse().expect("a year");
        let month: u32 = parts[1].parse().expect("a month");
        let day: u32 = parts[2].parse().expect("a day");
        assert!((2026..2200).contains(&year), "{today}");
        assert!((1..=12).contains(&month), "{today}");
        assert!((1..=31).contains(&day), "{today}");
    }
}
