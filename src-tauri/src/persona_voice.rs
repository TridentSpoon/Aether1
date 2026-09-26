// Per-avatar voice overrides.
//
// Every identity in `llm::genesis` ships with two voices of its own -- a cloud voice and a
// Piper model -- and `commands::synthesize_speech` resolves them from whichever persona is
// selected at the moment of speech. Those are the author's choices, not the operator's, and
// until now the only way to disagree with one was to edit the table and rebuild.
//
// This module is the operator's answer: a map of persona key -> the voice they would rather
// that avatar spoke in, stored in one settings row. It is deliberately *sparse*. An avatar
// nobody has touched has no entry at all, so the identity table stays the source of the
// default and a later change to it reaches every avatar that was never overridden. Clearing
// an override removes the entry rather than writing the default into it, which is what makes
// "reset to default" mean the same thing next year as it does today.
//
// The two halves are stored separately because they answer different questions: a machine
// running local-only never speaks the cloud voice, and an operator who has downloaded three
// Piper voices may want one of them here while leaving the cloud voice alone.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::llm::{MemoryDb, Persona};

/// The settings row holding the whole map. One row rather than a key per avatar: the map is
/// read on every sentence spoken, and one read beats twenty-two.
pub const SETTING: &str = "persona_voice_overrides";

/// The cloud voices Aether1 offers by name.
///
/// Microsoft has hundreds; these are the ones the identity table already speaks in, plus the
/// Tokyo voice the Voice & Sound picker has always offered. Listing them here rather than in
/// the HTML is what lets the avatar's own picker and the general one agree, and what lets a
/// name arriving from the HUD be checked against something before it is stored.
pub const CLOUD_VOICES: &[(&str, &str)] = &[
    ("en-US-AriaNeural", "American, female, warm"),
    ("en-US-JennyNeural", "American, female, bright"),
    ("en-US-GuyNeural", "American, male, level"),
    ("en-US-DavisNeural", "American, male, firm"),
    ("en-US-EricNeural", "American, male, dry"),
    ("en-GB-SoniaNeural", "British, female, composed"),
    ("en-GB-LibbyNeural", "British, female, precise"),
    ("en-GB-RyanNeural", "British, male, easy"),
    ("en-GB-ThomasNeural", "British, male, formal"),
    ("en-AU-NatashaNeural", "Australian, female"),
    ("en-AU-WilliamNeural", "Australian, male"),
    ("ja-JP-NanamiNeural", "Japanese, female"),
];

/// One avatar's overrides. Both halves optional, and an entry with neither is not stored.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_voice: Option<String>,
}

impl Choice {
    pub fn is_empty(&self) -> bool {
        self.voice.is_none() && self.local_voice.is_none()
    }
}

/// Every override, by persona key. A row that will not parse is treated as no overrides at
/// all: speech is not worth failing over a settings row somebody hand-edited, and the next
/// save rewrites it.
pub fn all(db: &MemoryDb) -> BTreeMap<String, Choice> {
    let raw = db.get_setting_string(SETTING, "");
    if raw.trim().is_empty() {
        return BTreeMap::new();
    }
    serde_json::from_str(&raw).unwrap_or_default()
}

/// What this avatar has been given, if anything.
pub fn choice(db: &MemoryDb, persona_key: &str) -> Choice {
    all(db).remove(persona_key).unwrap_or_default()
}

/// The cloud voice this persona speaks in: the operator's choice, else the identity table's.
/// `None` means it has none of its own either way, and falls back to the Voice & Sound one.
pub fn cloud_voice(db: &MemoryDb, persona_key: &str) -> Option<String> {
    choice(db, persona_key)
        .voice
        .or_else(|| crate::llm::persona_voice(persona_key).map(str::to_string))
}

/// The Piper voice this persona speaks in, by catalogue name, on the same terms.
pub fn local_voice(db: &MemoryDb, persona_key: &str) -> Option<String> {
    choice(db, persona_key)
        .local_voice
        .or_else(|| crate::llm::persona_local_voice(persona_key).map(str::to_string))
}

fn known_persona(persona_key: &str) -> bool {
    Persona::all().iter().any(|p| p.key() == persona_key)
}

fn known_cloud_voice(name: &str) -> bool {
    CLOUD_VOICES.iter().any(|(voice, _)| *voice == name)
}

fn known_local_voice(name: &str) -> bool {
    crate::voice_download::catalogue()
        .iter()
        .any(|voice| voice.name == name)
}

/// Records one avatar's choice. `None` for a half means "leave that half at the default",
/// which is how the reset button clears one without touching the other.
///
/// Both names are checked against the lists above before they are stored. The Piper name
/// ends up in a file path and the cloud name in a request to Microsoft, so neither is a
/// place to accept whatever arrived.
pub fn set(
    db: &MemoryDb,
    persona_key: &str,
    voice: Option<&str>,
    local_voice: Option<&str>,
) -> Result<(), String> {
    if !known_persona(persona_key) {
        return Err(format!(
            "there is no avatar identity called {persona_key:?}"
        ));
    }
    if let Some(name) = voice.filter(|n| !n.is_empty()) {
        if !known_cloud_voice(name) {
            return Err(format!("{name} is not one of the cloud voices"));
        }
    }
    if let Some(name) = local_voice.filter(|n| !n.is_empty()) {
        if !known_local_voice(name) {
            return Err(format!("{name} is not in the Piper voice catalogue"));
        }
    }

    let mut map = all(db);
    let choice = Choice {
        voice: voice.filter(|n| !n.is_empty()).map(str::to_string),
        local_voice: local_voice.filter(|n| !n.is_empty()).map(str::to_string),
    };
    if choice.is_empty() {
        map.remove(persona_key);
    } else {
        map.insert(persona_key.to_string(), choice);
    }
    write(db, &map)
}

/// Puts one avatar back to the identity table's own voices.
pub fn clear(db: &MemoryDb, persona_key: &str) -> Result<(), String> {
    let mut map = all(db);
    map.remove(persona_key);
    write(db, &map)
}

fn write(db: &MemoryDb, map: &BTreeMap<String, Choice>) -> Result<(), String> {
    let value = serde_json::to_value(map).map_err(|why| why.to_string())?;
    db.set_setting(SETTING, &Value::String(value.to_string()))
        .map_err(|why| format!("could not save the voice choice: {why}"))
}

/// The two pickers, for the HUD: the cloud voices, and the Piper voices with whether each
/// one is actually on this machine. A Piper voice that has not been downloaded is still
/// listed -- it is a real choice, it just needs fetching first, and hiding it would make the
/// avatar's picker disagree with the download list two panes away.
pub fn pickers() -> Value {
    serde_json::json!({
        "cloud": CLOUD_VOICES
            .iter()
            .map(|(name, label)| serde_json::json!({ "name": name, "label": label }))
            .collect::<Vec<_>>(),
        "local": crate::voice_download::catalogue(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn db() -> MemoryDb {
        let path = std::env::temp_dir().join(format!(
            "aether1_persona_voice_{}_{:?}.db",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&path);
        MemoryDb::open(&path).expect("open test db")
    }

    #[test]
    fn nothing_is_stored_until_somebody_chooses() {
        let db = db();
        assert!(all(&db).is_empty());
        // ...and the default still comes through, from the identity table.
        assert_eq!(
            cloud_voice(&db, "red9000").as_deref(),
            Some("en-US-GuyNeural")
        );
    }

    #[test]
    fn an_override_wins_and_a_reset_gives_the_default_back() {
        let db = db();
        set(&db, "red9000", Some("en-GB-SoniaNeural"), None).unwrap();
        assert_eq!(
            cloud_voice(&db, "red9000").as_deref(),
            Some("en-GB-SoniaNeural")
        );
        // The Piper half was left alone, so it is still the identity table's.
        assert_eq!(
            local_voice(&db, "red9000").as_deref(),
            Some("en_US-ryan-medium")
        );

        clear(&db, "red9000").unwrap();
        assert_eq!(
            cloud_voice(&db, "red9000").as_deref(),
            Some("en-US-GuyNeural")
        );
        assert!(all(&db).is_empty());
    }

    #[test]
    fn a_persona_with_no_voice_of_its_own_can_still_be_given_one() {
        // Model's Own has no row in the identity table: it speaks in whatever Voice & Sound
        // says. Being able to hand it a voice is most of the point of this pane.
        let db = db();
        assert!(cloud_voice(&db, "llm").is_none());
        set(
            &db,
            "llm",
            Some("en-AU-NatashaNeural"),
            Some("en_US-amy-medium"),
        )
        .unwrap();
        assert_eq!(
            cloud_voice(&db, "llm").as_deref(),
            Some("en-AU-NatashaNeural")
        );
        assert_eq!(local_voice(&db, "llm").as_deref(), Some("en_US-amy-medium"));
    }

    #[test]
    fn names_that_are_not_in_the_lists_are_refused() {
        let db = db();
        assert!(set(&db, "not-a-persona", None, None).is_err());
        assert!(set(&db, "red9000", Some("en-US-Nobody"), None).is_err());
        assert!(set(&db, "red9000", None, Some("../../etc/passwd")).is_err());
        assert!(all(&db).is_empty());
    }

    #[test]
    fn every_voice_the_identity_table_uses_is_offered_by_the_picker() {
        // Otherwise an avatar's own default would be missing from its own dropdown, and
        // picking anything else would be a one-way door.
        for persona in Persona::all() {
            if let Some(voice) = crate::llm::persona_voice(persona.key()) {
                assert!(
                    known_cloud_voice(voice),
                    "{} speaks in {voice}, which the picker does not offer",
                    persona.key()
                );
            }
        }
    }

    #[test]
    fn a_settings_row_that_will_not_parse_reads_as_no_overrides() {
        let db = db();
        db.set_setting(SETTING, &Value::String("{not json".into()))
            .unwrap();
        assert!(all(&db).is_empty());
        assert_eq!(
            cloud_voice(&db, "nexus").as_deref(),
            Some("en-GB-SoniaNeural")
        );
    }
}
