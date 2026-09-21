//! Flow mode: the avatar that answers follows the question, inside one group.
//!
//! **Static mode** is the behaviour AETHER1 has always had -- the operator picks an avatar
//! and it answers everything. **Flow mode** keeps the operator's group and lets the
//! specialist inside it take the questions that are its own, announced by whoever is
//! holding the line before the switch happens.
//!
//! Three rules make it bearable rather than twitchy:
//!
//! 1. **The classifier is a table, not a model.** It is the Genesis keyword table that
//!    already turns a stated purpose into an identity -- see [`genesis::match_score`]. That
//!    costs microseconds, adds no round trip, and can be read back and argued with, which a
//!    model deciding silently between turns could not be.
//! 2. **It only hands over inside one group.** The operator picked a cast, not a character.
//!    A persona with no group (A1, Model's Own, Custom) never flows at all.
//! 3. **It holds unless the match is confident.** A missed hand-off is invisible and costs
//!    nothing; a wrong one interrupts the conversation with a line that turned out to be
//!    unnecessary. So the bar is set where a miss is the cheaper mistake.

use crate::llm::db::MemoryDb;
use crate::llm::genesis;
use crate::llm::persona::Persona;

pub const ENABLED_SETTING: &str = "flow_mode_enabled";

/// The points a match needs before it may move the conversation. A single keyword scores
/// one and a multi-word phrase two, so this asks for either two separate signals or one
/// phrase nobody types by accident.
const CONFIDENT: u32 = 2;

/// Under this many words a message is a follow-up rather than a new subject -- "and the
/// other one?", "do it", "thanks" -- and a follow-up belongs to whoever is already
/// answering, whatever word happens to be in it.
const ENOUGH_WORDS: usize = 4;

/// One hand-off, decided but not yet applied.
#[derive(Debug, Clone)]
pub struct Handover {
    pub from: Persona,
    pub to: Persona,
    /// What `from` says on the way out, in their own voice.
    pub line: String,
}

impl Handover {
    /// The two labels the HUD has to draw: the hand-off line is attributed to the node
    /// leaving -- it is the one speaking -- and the reply that follows to the node arriving.
    pub fn speaker_name(&self) -> &'static str {
        self.from.avatar().unwrap_or("AETHER")
    }

    pub fn to_name(&self) -> &'static str {
        self.to.avatar().unwrap_or("AETHER")
    }

    /// The persona key the switch writes. The HUD finds its own avatar id from this, since
    /// the mapping between the two already lives in AVATAR_PRESETS and keeping a second
    /// copy anywhere else is how the two drift apart.
    pub fn to_key(&self) -> &'static str {
        self.to.key()
    }
}

pub fn enabled(db: &MemoryDb) -> bool {
    db.get_setting_bool(ENABLED_SETTING, false)
}

pub fn set_enabled(db: &MemoryDb, on: bool) -> Result<(), String> {
    db.set_setting(ENABLED_SETTING, &serde_json::Value::Bool(on))
        .map_err(|e| format!("could not save the flow mode setting: {e}"))
}

/// Whether this message should move to somebody else, and who. Decides nothing else and
/// writes nothing -- [`apply`] is the half that changes state, kept separate so the rule
/// can be tested without a database behind it.
pub fn consider(db: &MemoryDb, prompt: &str) -> Option<Handover> {
    if !enabled(db) {
        return None;
    }
    let current = Persona::from_key(&db.get_setting_string("persona_type", "default"));
    decide(&current, prompt)
}

/// The rule itself, with the settings read out of the way.
pub fn decide(current: &Persona, prompt: &str) -> Option<Handover> {
    let group = current.group()?;
    if prompt.split_whitespace().count() < ENOUGH_WORDS {
        return None;
    }

    let found = genesis::match_score(prompt)?;
    // A contested match still counts when it is emphatic. What it never does is carry a
    // single stray word: "plan" in a question about something else would otherwise pull
    // the conversation to the generalist every time it was typed.
    let confident = found.score >= CONFIDENT || !found.contested;
    if !confident {
        return None;
    }

    let next = Persona::from_key(found.persona_type);
    // from_key falls back rather than failing, so an unmapped persona_type would arrive
    // here as Default -- which has no group and is therefore refused by the check below
    // rather than silently becoming a hand-off to A1.
    if next == *current || next.group() != Some(group) {
        return None;
    }

    Some(Handover {
        line: current.handoff_line(&next),
        from: current.clone(),
        to: next,
    })
}

/// Make the hand-off real. The persona setting is what every other part of AETHER1 already
/// reads -- the directive, the HUD's avatar, the model routing, the tool field -- so moving
/// it is the whole switch, and nothing else needs to be told separately.
pub fn apply(db: &MemoryDb, handover: &Handover) -> Result<(), String> {
    db.set_setting(
        "persona_type",
        &serde_json::Value::String(handover.to_key().to_string()),
    )
    .map_err(|e| format!("could not switch persona: {e}"))?;
    db.set_setting(
        "agent_name",
        &serde_json::Value::String(handover.to_name().to_string()),
    )
    .map_err(|e| format!("could not switch the agent name: {e}"))?;

    // The voice belongs to the identity, so it moves with it. A hand-off that kept the
    // outgoing voice would have the new node speaking in the old one's.
    if let Some(identity) = genesis::identity_for(handover.to_key()) {
        let _ = db.set_setting("voice_name", &serde_json::Value::String(identity.voice));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pointed_question_moves_to_the_specialist_that_owns_it() {
        let handover = decide(&Persona::ArxLocas, "harden the firewall on this box")
            .expect("two defence keywords is a confident match");
        assert_eq!(handover.to, Persona::ArxLegionare);
        assert!(
            handover.line.contains("A.R.X.LEGIONARE"),
            "the outgoing node names the incoming one: {}",
            handover.line
        );
    }

    #[test]
    fn a_follow_up_stays_with_whoever_is_answering() {
        // Short messages are the ones most likely to carry a stray keyword and least
        // likely to be a new subject.
        assert!(decide(&Persona::ArxLocas, "and the plan?").is_none());
        assert!(decide(&Persona::ArxLegionare, "do it").is_none());
    }

    #[test]
    fn a_single_word_shared_with_another_node_is_not_enough() {
        // "story" is Loregenda's and "debug" is L'kemi's; a message carrying one of each
        // is ambiguous, and an ambiguous message is not a reason to interrupt.
        let contested = decide(&Persona::ArxLocas, "debug the story generator for me");
        assert!(
            contested.is_none(),
            "two nodes scored one each, so neither is confident"
        );
    }

    #[test]
    fn one_node_scoring_alone_is_confident_even_at_a_single_word() {
        // The common case: a plain request with exactly one specialist word in it. Nothing
        // else matched, so there is nothing to be ambiguous about.
        let handover = decide(&Persona::ArxLocas, "write me a short story about a city")
            .expect("only Loregenda scored");
        assert_eq!(handover.to, Persona::ArxLoregenda);
    }

    #[test]
    fn nothing_flows_outside_the_operators_own_group() {
        // The Nexus is Trace Protocols and L'kemi is an Umbral. A coding question asked of
        // The Nexus stays with The Nexus, which is already the group's answer for code.
        assert!(decide(&Persona::Nexus, "refactor this python script for me").is_none());
    }

    #[test]
    fn a_persona_with_no_group_never_flows() {
        // A1 is the starting point and belongs to no cast. Flow mode has nothing to flow
        // between, so it does nothing rather than dragging the operator into a line they
        // did not pick.
        assert!(decide(&Persona::Default, "harden the firewall on this box").is_none());
    }

    #[test]
    fn a_question_the_current_node_already_owns_does_not_hand_over_to_itself() {
        assert!(decide(&Persona::ArxLegionare, "harden the firewall on this box").is_none());
    }
}
