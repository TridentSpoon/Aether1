// The Auto router: one cheap judgment before the reply, deciding where the question goes.
//
// What it is for, in one sentence: a question that a small model on this machine can answer
// should not be sent to a company, and a question that needs looking up should not be
// answered from memory -- and deciding which is which is itself a small, structured
// question that a small model is good at.
//
// This is the thing `routing.rs` says not to build, and the difference is worth stating
// plainly rather than leaving as an apparent contradiction. Step 19's objection to a
// per-message classifier was that it makes the choice "invisible, unpredictable and
// impossible to check". Every one of those is answered here by construction, and if any of
// them stops being true this feature has become the thing that was refused:
//
//   * **Visible.** Every routed turn says which model Auto picked and why, in one line, as
//     the program speaking rather than the persona -- the same channel step 19's own
//     notices use.
//   * **Opt-in.** Off unless the operator picks "Auto" in the model picker. An explicit
//     pick in that list turns it off again, so choosing a model is still choosing a model.
//   * **Checkable.** The judgment is three fields from fixed sets of values, constrained by
//     the server's own grammar (`providers::judge`), and what is *done* with those three
//     fields is ordinary Rust in `decide` -- a pure function with tests, not a model's
//     opinion about model names. A 1B model asked to name a model id invents one; asked
//     whether a question is about code, it is useful.
//
// Three further boundaries, each of which exists because the obvious alternative is worse:
//
//   * **The judge is always local.** A cloud judge would bill somebody per message to
//     decide whether to bill them per message, and it would mean a router that cannot run
//     on an aeroplane. No local server means Auto has nothing to judge with, and the
//     operator's stored choice stands.
//   * **It never picks a *local* model.** Step 19 already decides which installed model
//     suits the persona, with a shipped table, measured speeds and the operator's own
//     picks on top. A second mechanism choosing between local models would be two things
//     deciding one question, which is how the last six fault reports in this codebase
//     started. Auto decides local *or* cloud; `routing::resolve` still decides which local.
//   * **A failed judgment changes nothing.** Not a guess, not a default to cloud: the
//     operator's choice, unrouted, plus a line saying the judge could not be reached.

use serde::Deserialize;
use serde_json::{json, Value};

use super::{providers, MemoryDb, Persona, Provider};

/// Whether Auto is in charge of this install's routing. Off by default: a feature that
/// sends some questions to a paid provider is not one to switch on for somebody.
pub const ENABLED_SETTING: &str = "auto_router";

/// Which installed model does the judging. Empty means "the smallest one installed", which
/// is the right default for a judge -- the job is classification, not composition.
pub const JUDGE_SETTING: &str = "auto_router_judge";

/// The value the model picker sends to mean "let Auto decide". Not a `Provider`: Auto is
/// not somewhere a question can be sent, it is the decision about where to send it, and
/// giving it a provider key would put it in every `match` that dispatches a request.
pub const PICKER_VALUE: &str = "\u{0}auto";

/// The other spelling of "Auto is on": `llm_provider` set to the literal `auto`.
///
/// Not how this was designed -- Auto is a decision about where a question goes, not a place
/// it can be sent, and a provider key for it would have to appear in every `match` that
/// dispatches a request. But it is how a picker naturally writes it, because every other
/// entry in that list is a provider and a model, and it is what the chat redesign's popover
/// saves. Accepted here, in the one function that answers "is Auto on", rather than refused
/// at the door: two front ends disagreeing about a setting's spelling is a thing to absorb
/// in one place, and the place is the reader.
pub const PROVIDER_SPELLING: &str = "auto";

/// Where the local server's shape is remembered, for when `llm_provider` is holding `auto`
/// instead. Written by `commands::save_settings` whenever a local provider is stored, so it
/// is already right by the time somebody picks Auto.
pub const LOCAL_SHAPE_SETTING: &str = "llm_provider_local";

/// Whether Auto is in charge of this install's routing.
///
/// Either spelling counts. The switch is the canonical one and is what AETHER1's own picker
/// writes; the provider spelling is what a picker that only knows how to save a provider
/// writes. One reader, so they cannot disagree.
pub fn enabled(db: &MemoryDb) -> bool {
    db.get_setting_bool(ENABLED_SETTING, false)
        || db.get_setting_string("llm_provider", "") == PROVIDER_SPELLING
}

/// What the stored provider means, with `auto` read as "no provider of its own".
///
/// This exists because of one specific way the provider spelling could go wrong, and it is
/// worse than it looks. `Provider::from_key` falls back to `Offline` for anything it does
/// not recognise, so a stored `llm_provider` of `auto` would make the turn's *fallback*
/// provider Offline -- and the fallback is exactly what runs when the judge cannot be
/// reached. Somebody who picked Auto and then stopped their model server would get the
/// offline persona's canned replies, with nothing anywhere saying that the setting naming
/// the router had been read as "answer from a script".
///
/// So: `auto` resolves to the local provider the endpoint is being driven as, which is
/// where a routed turn would have gone anyway, and the one answer that cannot strand
/// anyone.
pub fn stored_provider(db: &MemoryDb) -> Provider {
    let key = db.get_setting_string("llm_provider", "offline");
    if key == PROVIDER_SPELLING {
        return local_provider_for(&db.get_setting_string(LOCAL_SHAPE_SETTING, ""));
    }
    Provider::from_key(&key)
}

/// Which of the two local providers an endpoint is being driven as.
///
/// The stored provider is the only thing that knows: the same address answers both shapes,
/// and reading the port would be the mislabel this codebase has already chased through six
/// pull requests. With Auto stored in the provider slot that record is gone, so
/// `llm_provider_local` keeps it -- and an install that has never had one is Ollama, which
/// is the default endpoint's own shape.
fn local_provider_for(remembered: &str) -> Provider {
    if remembered == "lmstudio" {
        Provider::LmStudio
    } else {
        Provider::Ollama
    }
}

/// One place a question could be sent.
///
/// `tools` is only ever filled in for a local model, from `providers::ollama_facts`, with
/// the same three answers it has there: yes, no, and the server did not say.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub provider: Provider,
    pub model: String,
    pub local: bool,
    pub tools: Option<bool>,
    /// Whether this one is configured: a key exists for a cloud provider, and a local
    /// model is on the machine. Deliberately *not* "and local-only mode allows it" -- the
    /// mode is a separate fact (`Candidates::local_only`) because the picker needs to show
    /// a configured provider it cannot currently use, greyed out, rather than hide it. A
    /// provider that vanishes when a switch is flipped is indistinguishable from one that
    /// was never set up.
    pub ready: bool,
}

/// Everything that could answer, as the picker lists it and as the router chooses from it.
///
/// One gatherer for both, on purpose. The picker showing one set of options while the
/// router chooses from another would be a bug nobody could see from either side.
#[derive(Clone, Debug)]
pub struct Candidates {
    /// Which of the two local providers the endpoint is being driven as.
    pub local_provider: Provider,
    pub endpoint: String,
    pub local: Vec<Candidate>,
    pub online: Vec<Candidate>,
    pub local_only: bool,
}

/// Asks the configured endpoint what it can run, and the settings which companies have keys.
///
/// Deliberately the configured endpoint rather than a port scan: this is the server the
/// companion is pointed at, and offering a model from a different one would be offering a
/// switch that also silently moves the address.
pub fn candidates(db: &MemoryDb) -> Candidates {
    let provider_key = db.get_setting_string("llm_provider", "offline");
    let endpoint = db.get_setting_string("llm_endpoint", "http://localhost:11434");
    let local_only = crate::local_only::enabled(db);

    // Which of the two the endpoint is being driven as. The stored provider is the only
    // thing that knows: the same address answers both shapes, and reading the port would
    // be the mislabel this codebase has already chased through six pull requests.
    let local_provider = if provider_key == PROVIDER_SPELLING {
        local_provider_for(&db.get_setting_string(LOCAL_SHAPE_SETTING, ""))
    } else {
        local_provider_for(&provider_key)
    };

    let local = crate::model_scanner::models_at(&endpoint)
        .unwrap_or_default()
        .into_iter()
        .map(|model| Candidate {
            provider: local_provider,
            tools: providers::ollama_facts(&endpoint, &model).tools,
            model,
            local: true,
            // On this machine, so neither a key nor local-only mode can stand in the way.
            ready: true,
        })
        .collect();

    let online = [
        Provider::Anthropic,
        Provider::OpenAi,
        Provider::Gemini,
        Provider::Groq,
        Provider::Gateway,
    ]
    .into_iter()
    .map(|provider| Candidate {
        provider,
        // What the operator last set for *this* provider, which #245/#255 remember per
        // provider, before the shipped default. Routing a hard question to Anthropic and
        // sending it `claude-opus-5` when they have chosen something else for Anthropic
        // would be the router overriding a choice it has no business touching.
        model: {
            let remembered = db.get_setting_string(&super::model_setting_for(provider), "");
            if remembered.trim().is_empty() {
                super::default_model_for(provider).to_string()
            } else {
                remembered.trim().to_string()
            }
        },
        local: false,
        tools: None,
        // A provider with no key switches and then fails with a 401, which is worse than
        // not being offered.
        ready: !super::api_key_for(db, provider).trim().is_empty(),
    })
    .collect();

    Candidates {
        local_provider,
        endpoint,
        local,
        online,
        local_only,
    }
}

impl Candidates {
    /// The cloud provider a routed turn should use, when it wants one.
    ///
    /// `configured` -- whatever the operator last chose -- wins where it is a ready cloud
    /// provider, because "Auto, and when it goes to the cloud use Claude" is a sentence an
    /// operator can hold in their head. Otherwise the first ready one in the fixed order
    /// above, so the answer does not depend on hash iteration.
    fn cloud(&self, configured: Provider) -> Option<&Candidate> {
        // The mode is checked here rather than baked into `ready`, so that a provider with
        // a key is still reported as configured while being refused -- see `Candidate`.
        if self.local_only {
            return None;
        }
        self.online
            .iter()
            .find(|c| c.ready && c.provider == configured)
            .or_else(|| self.online.iter().find(|c| c.ready))
    }

    /// Whether anything local could answer at all.
    fn has_local(&self) -> bool {
        !self.local.is_empty()
    }

    /// Which installed model a locally-answered turn should fall back to, when the stored
    /// `llm_model` cannot serve as one.
    ///
    /// It usually can, and then this returns None and nothing is overridden: the stored
    /// model is a local model and step 19's `routing::resolve` already decides what to do
    /// with it. The exception is the turn Auto has just moved *off* a cloud provider, where
    /// the stored model id is `claude-opus-5` or similar -- a name no Ollama has heard of,
    /// and the one that would be sent whenever step 19 has no opinion of its own
    /// (`Reason::General`, which is every persona with no matching family installed).
    ///
    /// The answer comes from step 19's own table rather than from a second one here. Where
    /// that has nothing to say, the largest labelled model installed: this is the model that
    /// will answer, not the one that judged, and a size in the tag is the only evidence
    /// available. An unlabelled name is the last resort rather than the first, for the same
    /// reason `parameter_billions` sorts it last -- it is not thereby small, or large.
    fn local_base(
        &self,
        persona: &Persona,
        speeds: &std::collections::HashMap<String, f64>,
    ) -> Option<String> {
        let names: Vec<String> = self.local.iter().map(|c| c.model.clone()).collect();
        if names.is_empty() {
            return None;
        }
        super::routing::suggestion(persona, &names, speeds).or_else(|| {
            names
                .iter()
                .filter(|name| super::routing::parameter_billions(name) != u32::MAX)
                .max_by_key(|name| super::routing::parameter_billions(name))
                .or_else(|| names.first())
                .cloned()
        })
    }

    /// The model to judge with, and what the judgment is allowed to cost.
    ///
    /// The order is the whole fix for the first desktop log, where the judge timed out in
    /// front of every reply. Reading it from the top:
    ///
    /// 1. **The operator's own pick**, where they made one and it is still installed. Their
    ///    choice is not second-guessed, but it is still reported honestly as a load when it
    ///    is not in memory.
    /// 2. **A model the server already has loaded**, preferring the one this turn is going
    ///    to use anyway. This is the ordinary path on a machine that has been chatting for
    ///    a minute, and it costs a prompt of one sentence and nothing else.
    /// 3. **The model the turn will use**, when it is local but not yet loaded. The wait is
    ///    the reply's either way: judging on it moves the load earlier rather than adding
    ///    one, and the reply then finds it warm.
    /// 4. **The smallest installed model**, last. Smallest rather than fastest, and that is
    ///    deliberately not step 19's tie-break: there the question is which model should
    ///    *answer*, where a measured speed is the better guide, and here the job is four
    ///    short fields under a grammar. `parameter_billions` returns `u32::MAX` for a name
    ///    that claims no size, so an unlabelled model is never mistaken for a small one.
    ///
    /// Only 2 is `Warm`. Everything else is a model load, and saying so is what lets
    /// `providers::Pace` give it a bound it can actually meet -- the old code gave every
    /// judgment twenty seconds, which is generous for 2 and hopeless for the rest.
    pub fn judge_choice(
        &self,
        db: &MemoryDb,
        turn_model: &str,
        loaded: &[String],
    ) -> Option<(String, providers::Pace)> {
        let installed = |name: &str| {
            self.local
                .iter()
                .find(|c| c.model.eq_ignore_ascii_case(name.trim()))
                .map(|c| c.model.clone())
        };
        let resident = |name: &str| {
            loaded
                .iter()
                .any(|held| held.eq_ignore_ascii_case(name.trim()))
        };
        let paced = |model: String| {
            let pace = if resident(&model) {
                providers::Pace::Warm
            } else {
                providers::Pace::Loading
            };
            Some((model, pace))
        };

        let stored = db.get_setting_string(JUDGE_SETTING, "");
        if let Some(found) = installed(&stored) {
            return paced(found);
        }
        if let Some(found) = installed(turn_model).filter(|model| resident(model)) {
            return Some((found, providers::Pace::Warm));
        }
        if let Some(found) = self
            .local
            .iter()
            .find(|c| resident(&c.model))
            .map(|c| c.model.clone())
        {
            return Some((found, providers::Pace::Warm));
        }
        if let Some(found) = installed(turn_model) {
            return Some((found, providers::Pace::Loading));
        }
        self.local
            .iter()
            .min_by(|a, b| {
                super::routing::parameter_billions(&a.model)
                    .cmp(&super::routing::parameter_billions(&b.model))
                    .then_with(|| a.model.cmp(&b.model))
            })
            .map(|c| (c.model.clone(), providers::Pace::Loading))
    }
}

/// How hard the question is, as the judge sees it.
///
/// Two values rather than a score out of ten. A number invites a threshold that drifts, and
/// a small model's "7" is not comparable to its "6" -- whereas "could a small model on this
/// machine answer this well" is a question it can be asked directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    /// A small local model would answer this about as well as a large one.
    Simple,
    /// Long reasoning, specialist knowledge, or a careful piece of writing.
    Hard,
}

/// The judge's answer: three facts about the question, and nothing about models.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct Verdict {
    pub difficulty: Difficulty,
    /// Whether answering it correctly needs something the model cannot know: anything after
    /// its training, anything on this machine, anything that changes.
    pub needs_fresh_facts: bool,
    /// Whether it is about code. Not used to pick a local model -- step 19 does that from
    /// the persona -- but a coding question is one of the two cases where the difference
    /// between a 7B and a frontier model is most visible.
    pub is_code: bool,
}

/// The schema the server is told to constrain the answer to.
///
/// Public because the tuning harness scores wordings of the judge prompt through the same
/// schema the router uses -- a wording scored under a different schema is scored against
/// something else. Three fields, all required,
/// `difficulty` restricted to the two words -- so "medium" cannot come back and be read as
/// neither.
pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "difficulty": {"type": "string", "enum": ["simple", "hard"]},
            "needs_fresh_facts": {"type": "boolean"},
            "is_code": {"type": "boolean"},
        },
        "required": ["difficulty", "needs_fresh_facts", "is_code"],
        "additionalProperties": false,
    })
}

/// What the judge is told it is doing, as shipped.
///
/// The baseline rather than necessarily what runs: `llm::tuning` can score a different
/// wording against cases with known answers and put that in force instead, and `route`
/// below reads whichever is. This stays the thing every tuned wording is measured against,
/// and the thing an install that has never tuned uses.
///
/// Written for a small model: short, no persona, no politeness, and each field explained by
/// the question it answers rather than by its name. It says *do not answer the question*
/// twice, because the single most likely failure of a small model here is helpfulness --
/// answering the operator instead of classifying them.
pub fn built_in_judge_prompt() -> String {
    "You sort questions. You do not answer them.\n\n\
     You will be given one message that somebody typed to an AI assistant. Decide three \
     things about it and reply with those three fields only.\n\n\
     difficulty: \"simple\" if a small 7-billion-parameter model running on a home computer \
     would answer it about as well as a large one -- chat, a short explanation, a quick \
     rewrite, a fact it would know. \"hard\" if it needs long careful reasoning, specialist \
     knowledge, a substantial piece of writing, or a non-trivial piece of code.\n\n\
     needs_fresh_facts: true if answering it correctly needs something no model can know \
     from training -- today's news, a price, a version number, a web page, a file on this \
     computer, anything that changes. false if it can be answered from general knowledge or \
     from the conversation.\n\n\
     is_code: true if it is about writing, reading, fixing or explaining code or a \
     configuration file.\n\n\
     Do not answer the message. Sort it."
        .to_string()
}

/// What Auto decided, ready to be applied to the turn and shown to the operator.
#[derive(Clone, Debug, PartialEq)]
pub struct Routed {
    pub provider: Provider,
    /// Empty means "leave the model alone", which is the ordinary case: step 19's routing
    /// resolves which installed model this persona uses, and overwriting it here would be
    /// the second model-picking mechanism this module exists not to be. Filled in only
    /// where the stored model cannot serve as step 19's fallback -- see
    /// `Candidates::local_base`.
    pub model: String,
    /// Whether the tools go on the request this turn.
    pub tools: bool,
    /// The one line the operator sees. Written to be read, not matched on.
    pub why: String,
}

/// What the operator's switches leave reachable this turn.
///
/// The two are a pair because they are read as a pair, and because two booleans of the same
/// type sitting next to each other in an argument list are a transposition waiting to
/// happen -- named at the call site, they cannot be swapped silently.
#[derive(Debug, Clone, Copy)]
pub struct Reach {
    /// Whether there is anything to switch on: the operator's tools setting and a non-empty
    /// catalogue, resolved by the caller. Auto narrows this; it never widens it, so an
    /// install with tools off never acquires them because a router said so.
    pub tools: bool,
    /// Whether the two web tools would actually run: the Search chip and local-only mode.
    /// It changes only what the operator is told, never which tools go on the request.
    pub web: bool,
}

/// The whole of the decision, as a pure function of the verdict and what is available.
///
/// Separated from the call that produces the verdict so that every branch below is testable
/// without a model server anywhere near the test -- the same split `setup::advise` is built
/// on. Returns None when there is nothing to route: one candidate is not a choice.
pub fn decide(
    verdict: Verdict,
    available: &Candidates,
    configured: Provider,
    reach: Reach,
    local_base: Option<&str>,
) -> Option<Routed> {
    /* Tools first, because it is decided the same way wherever the question ends up: a
    question that needs a fact nobody can know from training needs something that can go
    and get it. The other direction matters more than it looks: an ordinary chat turn sent
    without the tool catalogue is a shorter prompt and a faster reply on a 7B, which is most
    of what Auto can do for somebody on a laptop.

    `reach.web` deliberately does *not* appear here, and the distinction is the whole
    reason it is a separate field. "Needs a fact nobody can know from training" is not the
    same question as "needs the internet": how much disk space is left, what is in a file,
    what is running -- all of those need a tool and none of them needs the web, and they are
    exactly the machine-facing questions a local-first companion exists for. The Search chip
    and local-only mode gate the two web tools (`tools::web_tools_enabled`,
    `net::require_online`) and nothing else, so withholding the whole catalogue when they
    are off would break a question that works today. What `reach.web` changes is what
    the operator is *told*: see the `why` below. */
    let tools = reach.tools && verdict.needs_fresh_facts;

    // The cloud is for the two cases where a frontier model is visibly better and a 7B
    // visibly struggles. Everything else stays on this machine, which is the default this
    // whole project is built around.
    let wants_cloud = verdict.difficulty == Difficulty::Hard;
    let cloud = available.cloud(configured);

    if wants_cloud {
        if let Some(cloud) = cloud {
            let subject = if verdict.is_code { "code" } else { "work" };
            return Some(Routed {
                provider: cloud.provider,
                model: cloud.model.to_string(),
                tools,
                why: format!(
                    "Auto sent this to {} -- it reads as {subject} worth a larger model.",
                    cloud.provider
                ),
            });
        }
        if !available.has_local() {
            return None;
        }
        // Nothing to send it to: no key, or local-only mode refusing them all. Said rather
        // than silently answered locally, because "Auto would have used a bigger model" is
        // the difference between a thin answer and a thin answer with a reason.
        return Some(Routed {
            provider: available.local_provider,
            model: local_base.unwrap_or_default().to_string(),
            tools,
            // Which of the two reasons it is, because they need different things done
            // about them -- one is a setting, the other is a key.
            why: if available.local_only {
                "Auto kept this on your machine -- it would have used a larger model, but \
                 local-only mode is on."
                    .to_string()
            } else {
                "Auto kept this on your machine -- it would have used a larger model, but \
                 no online provider has a key."
                    .to_string()
            },
        });
    }

    if !available.has_local() {
        return None;
    }
    Some(Routed {
        provider: available.local_provider,
        model: local_base.unwrap_or_default().to_string(),
        tools,
        why: match (tools, reach.web) {
            (true, true) => "Auto answered this on your machine, with tools -- it needs something \
                 looked up."
                .to_string(),
            /* The lookup was wanted and the web half of it cannot happen. Said outright,
            for the same reason the hard-question branch above says it would have used a
            larger model: the one line Auto prints is the whole of what the operator can
            check it by, and a line promising a lookup while `search_web` is being refused
            at the gate is worse than no line at all. Dropping silently to "Auto answered
            this on your machine" would hide exactly the thing they need to know. */
            (true, false) => "Auto answered this on your machine, with the tools on it -- \
                              it needs something looked up, but web lookups are switched \
                              off, so only what is on this machine is reachable."
                .to_string(),
            _ => "Auto answered this on your machine.".to_string(),
        },
    })
}

/// Asks the judge, then decides. None means the turn goes out exactly as it would have.
///
/// What the two halves of `reach` mean, and why they are not one flag, is on `Reach`.
pub fn route(
    db: &MemoryDb,
    persona: &Persona,
    configured: Provider,
    configured_model: &str,
    endpoint_in_use: &str,
    prompt: &str,
    reach: Reach,
) -> Result<Routed, String> {
    if prompt.trim().is_empty() {
        return Err("nothing to judge".to_string());
    }
    let available = candidates(db);
    // The endpoint the turn is already using, not the one in the settings, for the one case
    // where they differ: a gateway rewrites `endpoint`, and judging at a gateway's address
    // would send the question to the cloud to decide whether to send it to the cloud.
    let endpoint = if available.endpoint.is_empty() {
        endpoint_in_use
    } else {
        &available.endpoint
    };
    let loaded = providers::loaded_models(available.local_provider, endpoint);
    let (judge, pace) = available
        .judge_choice(db, configured_model, &loaded)
        .ok_or_else(|| "no local model to judge with".to_string())?;
    let answer = providers::judge(
        available.local_provider,
        endpoint,
        &judge,
        providers::Ask {
            // Whatever wording is in force: the shipped one, or one that beat it on this
            // machine's model. See `llm::tuning`.
            system_prompt: &super::tuning::text(db, super::tuning::Adapter::Judge),
            question: prompt,
            schema: &schema(),
            budget: providers::VERDICT_TOKENS,
            pace,
        },
    )
    // Named, because the first desktop log of this said only "timeout: global" and the one
    // thing worth knowing was which model it had gone to ask and whether that model was in
    // memory at the time.
    .map_err(|why| match pace {
        providers::Pace::Warm => format!("{judge} is loaded and did not answer: {why}"),
        _ => format!("{judge} had to be loaded first and did not answer in time: {why}"),
    })?;
    let verdict: Verdict = serde_json::from_value(answer.clone())
        .map_err(|e| format!("the judgment was JSON but not a verdict ({e}): {answer}"))?;
    /* Whether a locally-answered turn needs a model named for it, judged by asking the
    question directly: is the stored model one the server actually has? Two ways it is not,
    and the second was found by driving it rather than by reasoning:

      * Auto moved this turn off a cloud provider, so the stored id is `claude-opus-5`.
      * The stored model is *empty*, because the picker that turned Auto on wrote a provider
        and a blank model -- which is what a picker does when every other row in it is a
        provider and a model. Nothing then named a model at all, and the server answered
        "no model by that name" on a machine with three installed.

    Asking "is it installed" covers both, and covers the ones nobody has thought of. */
    let installed = available.local.iter().any(|candidate| {
        candidate
            .model
            .eq_ignore_ascii_case(configured_model.trim())
    });
    let local_base = (!installed)
        .then(|| available.local_base(persona, &super::routing::measured_speeds(db)))
        .flatten();
    decide(
        verdict,
        &available,
        configured,
        reach,
        local_base.as_deref(),
    )
    .ok_or_else(|| "there was only one place to send it".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real file rather than ":memory:" -- every MemoryDb method opens its own
    /// connection, so an in-memory database would lose each write as it closed.
    fn db_for(name: &str) -> MemoryDb {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("aether1_auto_{name}_{}_{n}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        MemoryDb::open(path).expect("temp db should open")
    }

    fn local_model(name: &str) -> Candidate {
        Candidate {
            provider: Provider::Ollama,
            model: name.to_string(),
            local: true,
            tools: Some(true),
            ready: true,
        }
    }

    fn cloud(provider: Provider, ready: bool) -> Candidate {
        Candidate {
            provider,
            model: super::super::default_model_for(provider).to_string(),
            local: false,
            tools: None,
            ready,
        }
    }

    fn both() -> Candidates {
        Candidates {
            local_provider: Provider::Ollama,
            endpoint: "http://localhost:11434".to_string(),
            local: vec![local_model("qwen3:8b")],
            online: vec![
                cloud(Provider::Anthropic, true),
                cloud(Provider::OpenAi, true),
            ],
            local_only: false,
        }
    }

    /// The judge the old `judge_model` would have returned: nothing loaded, and a turn on
    /// a cloud model, so neither of the two new steps applies.
    fn chose(available: &Candidates, db: &MemoryDb) -> Option<String> {
        available
            .judge_choice(db, "claude-opus-5", &[])
            .map(|(model, _)| model)
    }

    fn verdict(difficulty: Difficulty, fresh: bool, code: bool) -> Verdict {
        Verdict {
            difficulty,
            needs_fresh_facts: fresh,
            is_code: code,
        }
    }

    #[test]
    fn a_simple_question_stays_on_the_machine() {
        let routed = decide(
            verdict(Difficulty::Simple, false, false),
            &both(),
            Provider::Anthropic,
            Reach {
                tools: true,
                web: true,
            },
            None,
        )
        .expect("something to route to");
        assert_eq!(routed.provider, Provider::Ollama);
        assert!(!routed.tools, "nothing to look up, so no catalogue is sent");
        // The model is left for step 19 to resolve rather than named here.
        assert!(routed.model.is_empty());
    }

    #[test]
    fn a_hard_question_goes_to_the_configured_cloud_provider() {
        let routed = decide(
            verdict(Difficulty::Hard, false, true),
            &both(),
            Provider::OpenAi,
            Reach {
                tools: true,
                web: true,
            },
            None,
        )
        .expect("something to route to");
        assert_eq!(
            routed.provider,
            Provider::OpenAi,
            "the operator's own cloud provider, not the first in the list"
        );
        assert!(routed.why.contains("code"));
    }

    #[test]
    fn a_configured_provider_with_no_key_does_not_win() {
        let mut available = both();
        available.online = vec![
            cloud(Provider::Anthropic, false),
            cloud(Provider::Groq, true),
        ];
        let routed = decide(
            verdict(Difficulty::Hard, false, false),
            &available,
            Provider::Anthropic,
            Reach {
                tools: true,
                web: true,
            },
            None,
        )
        .expect("something to route to");
        assert_eq!(routed.provider, Provider::Groq);
    }

    #[test]
    fn local_only_mode_keeps_a_hard_question_local_and_says_why() {
        let mut available = both();
        available.local_only = true;
        // Configured, with a key, and still refused -- so the mode is doing the refusing
        // and the notice has to say so rather than blaming a missing key.
        available.online = vec![cloud(Provider::Anthropic, true)];
        let routed = decide(
            verdict(Difficulty::Hard, false, false),
            &available,
            Provider::Anthropic,
            Reach {
                tools: true,
                web: true,
            },
            None,
        )
        .expect("something to route to");
        assert_eq!(routed.provider, Provider::Ollama);
        assert!(
            routed.why.contains("local-only"),
            "the operator is told which of the two reasons it was: {}",
            routed.why
        );
    }

    #[test]
    fn with_no_key_anywhere_the_reason_names_the_key_and_not_the_mode() {
        let mut available = both();
        available.online = vec![cloud(Provider::Anthropic, false)];
        let routed = decide(
            verdict(Difficulty::Hard, false, false),
            &available,
            Provider::Anthropic,
            Reach {
                tools: true,
                web: true,
            },
            None,
        )
        .expect("something to route to");
        assert!(routed.why.contains("key"), "{}", routed.why);
        assert!(!routed.why.contains("local-only"));
    }

    #[test]
    fn a_question_needing_a_lookup_gets_the_tools() {
        let routed = decide(
            verdict(Difficulty::Simple, true, false),
            &both(),
            Provider::Anthropic,
            Reach {
                tools: true,
                web: true,
            },
            None,
        )
        .expect("something to route to");
        assert!(routed.tools);
        assert!(routed.why.contains("looked up"));
    }

    #[test]
    fn with_web_lookups_off_the_notice_says_only_this_machine_is_reachable() {
        /* The tools still go, because `needs_fresh_facts` covers questions about this
        machine -- the disk, the clock, the project folder -- and those tools are not
        gated by the web switch. What changes is what the operator is told, so the one
        `⚙` line cannot promise a lookup that the gate will refuse. */
        let routed = decide(
            verdict(Difficulty::Simple, true, false),
            &both(),
            Provider::Anthropic,
            Reach {
                tools: true,
                web: false,
            },
            None,
        )
        .expect("something to route to");
        assert!(routed.tools, "the local tools are still worth sending");
        assert!(
            routed.why.contains("web lookups are switched off"),
            "{}",
            routed.why
        );
    }

    #[test]
    fn auto_never_switches_tools_on_for_an_install_that_has_them_off() {
        for difficulty in [Difficulty::Simple, Difficulty::Hard] {
            let routed = decide(
                verdict(difficulty, true, false),
                &both(),
                Provider::Anthropic,
                Reach {
                    tools: false,
                    web: true,
                },
                None,
            )
            .expect("something to route to");
            assert!(
                !routed.tools,
                "the operator's own switch is narrowed, never widened"
            );
        }
    }

    #[test]
    fn with_nothing_installed_and_no_key_there_is_nothing_to_decide() {
        let available = Candidates {
            local_provider: Provider::Ollama,
            endpoint: "http://localhost:11434".to_string(),
            local: vec![],
            online: vec![cloud(Provider::Anthropic, false)],
            local_only: false,
        };
        for difficulty in [Difficulty::Simple, Difficulty::Hard] {
            assert!(
                decide(
                    verdict(difficulty, false, false),
                    &available,
                    Provider::Anthropic,
                    Reach {
                        tools: true,
                        web: true
                    },
                    None,
                )
                .is_none(),
                "an unrouted turn is better than a routed one with nowhere to go"
            );
        }
    }

    #[test]
    fn the_judge_is_the_smallest_installed_model() {
        let db = db_for("smallest");
        let available = Candidates {
            local_provider: Provider::Ollama,
            endpoint: String::new(),
            local: vec![
                local_model("qwen3:14b"),
                local_model("llama3.2:3b"),
                local_model("qwen3:8b"),
            ],
            online: vec![],
            local_only: false,
        };
        assert_eq!(chose(&available, &db), Some("llama3.2:3b".to_string()));
    }

    #[test]
    fn a_model_already_in_memory_is_judged_with_rather_than_a_smaller_one_on_disk() {
        /* The fault the first desktop log showed. The smallest installed model is the
        cheapest to *run* and the most expensive to *reach* when it is not loaded: on a
        card with one large model resident it is a read off disk, and possibly an eviction
        of the model the reply is about to use. Twenty seconds was not enough for that, so
        Auto stood down on every message. */
        let db = db_for("resident");
        let available = Candidates {
            local_provider: Provider::Ollama,
            endpoint: String::new(),
            local: vec![local_model("qwen3:14b"), local_model("llama3.2:3b")],
            online: vec![],
            local_only: false,
        };
        let loaded = vec!["qwen3:14b".to_string()];
        assert_eq!(
            available.judge_choice(&db, "claude-opus-5", &loaded),
            Some(("qwen3:14b".to_string(), providers::Pace::Warm))
        );
    }

    #[test]
    fn the_model_this_turn_will_use_is_judged_with_when_nothing_is_loaded() {
        // The load is the reply's either way, so judging on it moves the wait earlier
        // instead of adding one -- which is why this is allowed to take longer.
        let db = db_for("turn_model");
        let available = Candidates {
            local_provider: Provider::Ollama,
            endpoint: String::new(),
            local: vec![local_model("qwen3:14b"), local_model("llama3.2:3b")],
            online: vec![],
            local_only: false,
        };
        assert_eq!(
            available.judge_choice(&db, "qwen3:14b", &[]),
            Some(("qwen3:14b".to_string(), providers::Pace::Loading))
        );
    }

    #[test]
    fn an_operators_own_judge_is_still_reported_as_a_load_when_it_is_not_in_memory() {
        // Their choice is honoured, and honoured honestly: the bound has to match what the
        // call is really about to do, or it fails in front of every reply.
        let db = db_for("own_judge_cold");
        let available = Candidates {
            local_provider: Provider::Ollama,
            endpoint: String::new(),
            local: vec![local_model("qwen3:14b"), local_model("llama3.2:3b")],
            online: vec![],
            local_only: false,
        };
        db.set_setting(JUDGE_SETTING, &json!("llama3.2:3b"))
            .unwrap();
        let loaded = vec!["qwen3:14b".to_string()];
        assert_eq!(
            available.judge_choice(&db, "qwen3:14b", &loaded),
            Some(("llama3.2:3b".to_string(), providers::Pace::Loading))
        );
    }

    #[test]
    fn an_unlabelled_model_is_not_mistaken_for_a_small_one() {
        let db = db_for("unlabelled");
        let available = Candidates {
            local_provider: Provider::Ollama,
            endpoint: String::new(),
            local: vec![
                local_model("something-custom:latest"),
                local_model("qwen3:8b"),
            ],
            online: vec![],
            local_only: false,
        };
        assert_eq!(chose(&available, &db), Some("qwen3:8b".to_string()));
    }

    #[test]
    fn the_operators_own_judge_wins_while_it_is_installed() {
        let db = db_for("own_judge");
        let available = Candidates {
            local_provider: Provider::Ollama,
            endpoint: String::new(),
            local: vec![local_model("qwen3:14b"), local_model("llama3.2:3b")],
            online: vec![],
            local_only: false,
        };
        db.set_setting(JUDGE_SETTING, &json!("qwen3:14b")).unwrap();
        assert_eq!(chose(&available, &db), Some("qwen3:14b".to_string()));

        // Uninstalled, and the answer falls back rather than asking a server for a model it
        // does not have -- which fails per message, in front of every reply.
        db.set_setting(JUDGE_SETTING, &json!("qwen3:32b")).unwrap();
        assert_eq!(chose(&available, &db), Some("llama3.2:3b".to_string()));
    }

    #[test]
    fn the_providers_own_spelling_of_auto_counts_as_on() {
        let db = db_for("spelling");
        assert!(!enabled(&db));
        db.set_setting("llm_provider", &json!(PROVIDER_SPELLING))
            .unwrap();
        assert!(
            enabled(&db),
            "a picker that can only save a provider still turns Auto on"
        );
    }

    /// The failure this is really about: `Provider::from_key` falls back to `Offline` for
    /// anything it does not know, so `auto` in the provider slot would make the *fallback*
    /// provider Offline -- and the fallback is what runs when the judge cannot be reached.
    /// Picking Auto and then stopping the model server would get the offline persona's
    /// canned replies, with nothing saying why.
    #[test]
    fn auto_in_the_provider_slot_never_resolves_to_offline() {
        let db = db_for("not_offline");
        db.set_setting("llm_provider", &json!(PROVIDER_SPELLING))
            .unwrap();
        assert_eq!(stored_provider(&db), Provider::Ollama);
        assert_ne!(stored_provider(&db), Provider::Offline);

        // And it remembers which local shape, which is the thing the provider slot was the
        // only record of.
        db.set_setting(LOCAL_SHAPE_SETTING, &json!("lmstudio"))
            .unwrap();
        assert_eq!(stored_provider(&db), Provider::LmStudio);
        assert_eq!(
            candidates(&db).local_provider,
            Provider::LmStudio,
            "so the local models are listed and judged as the server that is actually there"
        );
    }

    #[test]
    fn a_real_provider_in_the_slot_still_reads_as_itself() {
        let db = db_for("real_provider");
        for key in ["ollama", "lmstudio", "anthropic", "openai", "offline"] {
            db.set_setting("llm_provider", &json!(key)).unwrap();
            assert_eq!(
                stored_provider(&db),
                Provider::from_key(key),
                "{key} is untouched by the auto spelling"
            );
        }
    }

    /// Found by driving it rather than by reading it. A picker whose every other row is a
    /// provider and a model writes a *blank* model when Auto is chosen, so nothing named a
    /// model at all and the server answered "no model by that name" on a machine with three
    /// installed. The condition is therefore "is the stored model installed", which covers
    /// the blank, the cloud id, and whatever else arrives in that slot.
    #[test]
    fn a_local_turn_gets_a_model_named_for_it_whenever_the_stored_one_is_not_installed() {
        let available = both();
        let installed = &available.local[0].model;
        let speeds = std::collections::HashMap::new();
        let base = available
            .local_base(&Persona::Default, &speeds)
            .expect("something installed to fall back to");
        assert!(
            available.local.iter().any(|c| c.model == base),
            "the fallback is a model the server actually has: {base}"
        );

        // And it is only a fallback: a stored model that *is* installed is left alone, so
        // step 19 stays the only thing choosing between installed models.
        assert_eq!(installed, "qwen3:8b");
    }

    #[test]
    fn a_routed_cloud_turn_uses_the_model_set_for_that_provider() {
        let db = db_for("remembered_model");
        let available = candidates(&db);
        let anthropic = available
            .online
            .iter()
            .find(|c| c.provider == Provider::Anthropic)
            .expect("anthropic is offered");
        assert_eq!(
            anthropic.model,
            super::super::default_model_for(Provider::Anthropic),
            "the shipped default, while nothing has been chosen"
        );

        db.set_setting(
            &super::super::model_setting_for(Provider::Anthropic),
            &json!("claude-something-else"),
        )
        .unwrap();
        let available = candidates(&db);
        let anthropic = available
            .online
            .iter()
            .find(|c| c.provider == Provider::Anthropic)
            .expect("anthropic is offered");
        assert_eq!(
            anthropic.model, "claude-something-else",
            "the router does not override a model the operator chose for that provider"
        );
    }

    #[test]
    fn auto_is_off_until_it_is_asked_for() {
        let db = db_for("enabled");
        assert!(!enabled(&db));
        db.set_setting(ENABLED_SETTING, &json!(true)).unwrap();
        assert!(enabled(&db));
    }

    #[test]
    fn the_schema_pins_the_three_fields_and_the_two_words() {
        let schema = schema();
        let required: Vec<&str> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(
            required,
            vec!["difficulty", "needs_fresh_facts", "is_code"],
            "every field is required: a verdict missing one is not a verdict"
        );
        assert_eq!(
            schema["properties"]["difficulty"]["enum"],
            json!(["simple", "hard"])
        );
        assert_eq!(schema["additionalProperties"], json!(false));
    }

    #[test]
    fn a_verdict_parses_from_what_the_schema_describes() {
        let verdict: Verdict = serde_json::from_value(json!({
            "difficulty": "hard",
            "needs_fresh_facts": true,
            "is_code": false,
        }))
        .expect("the schema and the type agree");
        assert_eq!(verdict.difficulty, Difficulty::Hard);
        assert!(verdict.needs_fresh_facts);

        // And a word outside the enum is a parse failure rather than a quiet default --
        // which is what makes the fallback path reachable instead of invisible.
        assert!(serde_json::from_value::<Verdict>(json!({
            "difficulty": "medium",
            "needs_fresh_facts": false,
            "is_code": false,
        }))
        .is_err());
    }

    #[test]
    fn the_judge_is_told_not_to_answer() {
        let prompt = built_in_judge_prompt();
        assert!(prompt.contains("do not answer") || prompt.contains("Do not answer"));
        assert!(
            prompt.contains("difficulty")
                && prompt.contains("needs_fresh_facts")
                && prompt.contains("is_code"),
            "every field the schema requires is explained to the model"
        );
    }
}
