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
//! 2. **It only hands over inside one line.** The operator picked a cast, not a character.
//!    The line is either the one the selected avatar belongs to, or -- since the lines are
//!    selectable in their own right -- the one the operator picked whole, in which case it
//!    holds even while the avatar answering changes. A persona with no line and no line
//!    picked (A1, Model's Own, Custom) never flows at all.
//! 3. **It holds unless the match is confident.** A missed hand-off is invisible and costs
//!    nothing; a wrong one interrupts the conversation with a line that turned out to be
//!    unnecessary. So the bar is set where a miss is the cheaper mistake.

use crate::llm::db::MemoryDb;
use crate::llm::genesis;
use crate::llm::persona::Persona;

pub const ENABLED_SETTING: &str = "flow_mode_enabled";

/// The line the operator picked whole, if they picked one rather than a single avatar. An
/// empty value means they are wearing a character, and the line to flow within is then
/// whichever one that character belongs to.
pub const GROUP_SETTING: &str = "flow_group";

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

/// The line the operator picked whole, if it is one the personas actually cover. A name
/// that no longer matches a line -- a renamed cast, a setting written by hand -- reads as
/// no pick at all rather than as a line nothing can ever match.
pub fn line(db: &MemoryDb) -> Option<&'static str> {
    let stored = db.get_setting_string(GROUP_SETTING, "");
    if stored.trim().is_empty() {
        return None;
    }
    Persona::flow_lines().into_iter().find(|g| *g == stored)
}

/// Pick a line whole, or release it (`None`).
///
/// Picking one turns flow mode on in the same breath: a line is a cast rather than a
/// character, and a cast with hand-offs switched off is just whichever member happened to
/// be selected. It also moves the operator to that line's anchor when they were standing
/// outside it, so the first question is answered from inside the cast they just picked;
/// the persona it moved to, if any, comes back so the HUD can wear the matching avatar.
///
/// Releasing a line leaves the persona exactly where the last hand-off left it and leaves
/// flow mode as it was -- dropping back to a character the operator never picked would
/// undo the session rather than end the arrangement.
pub fn set_line(db: &MemoryDb, group: Option<&str>) -> Result<Option<Persona>, String> {
    let Some(group) = group else {
        db.set_setting(GROUP_SETTING, &serde_json::Value::String(String::new()))
            .map_err(|e| format!("could not clear the flow line: {e}"))?;
        return Ok(None);
    };
    let group = Persona::flow_lines()
        .into_iter()
        .find(|g| *g == group)
        .ok_or_else(|| format!("{group:?} is not a line that can be picked whole"))?;

    db.set_setting(GROUP_SETTING, &serde_json::Value::String(group.to_string()))
        .map_err(|e| format!("could not save the flow line: {e}"))?;
    set_enabled(db, true)?;

    let current = Persona::from_key(&db.get_setting_string("persona_type", "default"));
    if current.group() == Some(group) {
        return Ok(None);
    }
    let anchor = Persona::group_anchor(group)
        .ok_or_else(|| format!("{group} has no persona behind it yet"))?;
    wear(db, &anchor)?;
    Ok(Some(anchor))
}

/// Whether this message should move to somebody else, and who. Decides nothing else and
/// writes nothing -- [`apply`] is the half that changes state, kept separate so the rule
/// can be tested without a database behind it.
pub fn consider(db: &MemoryDb, prompt: &str) -> Option<Handover> {
    if !enabled(db) {
        return None;
    }
    let current = Persona::from_key(&db.get_setting_string("persona_type", "default"));
    decide_within(&current, line(db), prompt)
}

/// The rule, with the line stated. `line` is the cast the operator picked whole; when
/// there is none the line is whichever one the persona answering belongs to, which is the
/// original behaviour. A picked line outranks the current persona's own, so a hand-off can
/// also move *into* the cast from a persona standing outside it -- which is the whole point
/// of being able to pick one: A1 asked a defence question hands to the line's defender.
pub fn decide_within(
    current: &Persona,
    line: Option<&'static str>,
    prompt: &str,
) -> Option<Handover> {
    let group = line.or_else(|| current.group())?;
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
    wear(db, &handover.to)?;

    // The voice moves with the identity too, but nothing is written for it here: speech
    // resolves the voice from whichever persona is selected at the moment it speaks (see
    // commands::synthesize_speech), so a hand-off is already a change of voice. Writing it
    // as a setting would also overwrite the operator's own choice permanently, for a
    // switch that may last one question.
    Ok(())
}

/// Put one persona on: the two settings every other part of AETHER1 reads to know who is
/// answering. Shared by a hand-off and by picking a line whole, so both arrive at the same
/// state rather than two nearly-identical writes drifting apart.
fn wear(db: &MemoryDb, persona: &Persona) -> Result<(), String> {
    db.set_setting(
        "persona_type",
        &serde_json::Value::String(persona.key().to_string()),
    )
    .map_err(|e| format!("could not switch persona: {e}"))?;
    db.set_setting(
        "agent_name",
        &serde_json::Value::String(persona.avatar().unwrap_or("AETHER").to_string()),
    )
    .map_err(|e| format!("could not switch the agent name: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No line picked: the operator is wearing a character, and the line to move within is
    /// that character's own. The shorthand the cases below were written against.
    fn decide(current: &Persona, prompt: &str) -> Option<Handover> {
        decide_within(current, None, prompt)
    }

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

    #[test]
    fn a_picked_line_flows_from_a_persona_standing_outside_it() {
        // The point of picking a line rather than a character: A1 belongs to no cast, so
        // on its own it never flows -- but an operator who picked The Umbrals whole has
        // asked for that cast to answer, and a defence question belongs to its defender.
        let handover = decide_within(
            &Persona::Default,
            Some("The Umbrals"),
            "harden the firewall on this box",
        )
        .expect("the picked line supplies what A1 has not got");
        assert_eq!(handover.to, Persona::ArxLegionare);
    }

    #[test]
    fn a_picked_line_still_refuses_a_specialist_from_another_cast() {
        // Picking a line widens who may answer to that whole cast and to nothing else:
        // the coding question goes to the Umbrals' developer, never to The Nexus.
        let handover = decide_within(
            &Persona::ArxLocas,
            Some("The Umbrals"),
            "refactor this python script for me",
        )
        .expect("L'kemi is the Umbrals' developer");
        assert_eq!(handover.to, Persona::ArxLkemi);

        // And a line held while wearing a member of another one keeps its own members:
        // the Singular Ascended Class covers code with The Nexus... which is Trace
        // Protocols, so there is nobody in this cast to move to.
        assert!(decide_within(
            &Persona::Halcy,
            Some("Singular Ascended Class"),
            "refactor this python script for me"
        )
        .is_none());
    }

    #[test]
    fn a_picked_line_outranks_the_line_of_whoever_is_answering() {
        // A hand-off inside a picked line leaves the operator wearing that line's member,
        // so this case is transient -- but it is reachable by picking a line and then an
        // avatar from another one, and the pick is what the operator asked for.
        let handover = decide_within(
            &Persona::Nexus,
            Some("The Umbrals"),
            "write me a short story about a city",
        )
        .expect("the picked line decides, not The Nexus's own");
        assert_eq!(handover.to, Persona::ArxLoregenda);
    }

    #[test]
    fn every_line_that_can_be_picked_has_an_anchor_to_land_on() {
        for group in Persona::flow_lines() {
            let anchor = Persona::group_anchor(group)
                .unwrap_or_else(|| panic!("{group} is offered as a line with nobody in it"));
            assert_eq!(anchor.group(), Some(group));
            assert!(
                Persona::group_members(group).len() > 1,
                "{group} has nobody to hand over to, so it is not a cast"
            );
        }
    }
}
