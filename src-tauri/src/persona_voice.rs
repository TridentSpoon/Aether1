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

/// The companion's own name when nobody has given it one.
///
/// Aurora was chosen for what it can be shortened to, and the shortening is not a second
/// decision: an operator who picks a voice has picked the name that sounds like it. So the
/// nickname lives in `VOICE_NAMES` below, keyed by voice, and this constant is only the
/// answer for a voice that is not in the table -- a Piper model, the OS voice, or a cloud
/// voice added to the picker and not here.
pub const DEFAULT_NAME: &str = "Aurora";

/// The name that was the default before Aurora.
///
/// It matters because of how a default is stored, which is to say: usually not at all.
/// `agent_name` has no row until something writes one, and the Settings pane writes every
/// field it renders on Save -- so an install whose operator never touched the name can
/// still have `HALCY` sitting in the database, written by a Save of something else
/// entirely. Reading that as a choice would strand those installs on a name nobody picked,
/// which is exactly what `apply_default_name` is for. hAlcy itself is unaffected: it is
/// still an avatar anyone can choose, and choosing it brings its name with it.
pub const LEGACY_DEFAULT_NAME: &str = "HALCY";

/// Which nickname goes with which voice.
///
/// One per cloud voice in `CLOUD_VOICES`, picked to match what the voice sounds like
/// rather than to be unique -- these are nicknames for one companion, not twelve names,
/// so two voices sharing `Rory` is the point and not a collision. The default voice gets
/// the full `Aurora`.
pub const VOICE_NAMES: &[(&str, &str)] = &[
    ("en-US-AriaNeural", "Aurora"),
    ("en-US-JennyNeural", "Rora"),
    ("en-US-GuyNeural", "Rory"),
    ("en-US-DavisNeural", "Auro"),
    ("en-US-EricNeural", "Auro"),
    ("en-GB-SoniaNeural", "Aura"),
    ("en-GB-LibbyNeural", "Rora"),
    ("en-GB-RyanNeural", "Rory"),
    ("en-GB-ThomasNeural", "Auro"),
    ("en-AU-NatashaNeural", "Aura"),
    ("en-AU-WilliamNeural", "Rory"),
    ("ja-JP-NanamiNeural", "Aura"),
];

/// The nickname for one voice, or `DEFAULT_NAME` when that voice has none.
pub fn default_name_for_voice(voice: &str) -> &'static str {
    VOICE_NAMES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(voice))
        .map(|(_, nickname)| *nickname)
        .unwrap_or(DEFAULT_NAME)
}

/// Whether this name is one nobody chose: an Aurora nickname, or the name that used to be
/// the default.
///
/// Used to decide whether a selected avatar may speak under its own name instead, and
/// whether changing the voice may move the nickname with it. A name from outside this list
/// was typed by the operator and is never touched by either.
pub fn is_unchosen_name(name: &str) -> bool {
    let name = name.trim();
    name.is_empty()
        || name.eq_ignore_ascii_case(LEGACY_DEFAULT_NAME)
        || name.eq_ignore_ascii_case(DEFAULT_NAME)
        || VOICE_NAMES
            .iter()
            .any(|(_, nickname)| name.eq_ignore_ascii_case(nickname))
}

/// Marks the one-time move off the old default name as done.
pub const DEFAULT_NAME_MARKER: &str = "aurora_default_name_applied";

/// Moves an install that is still carrying the old default name onto the Aurora nickname
/// for the voice it speaks in. Once, and only for a name nobody picked.
///
/// Same shape and same reason as `tools::apply_default_on`: a stored value that was never
/// a decision, and no way to tell that from the row alone, so the move happens once and
/// records that it happened. Anything the operator typed -- including `HALCY` while the
/// hAlcy avatar is selected, which is that avatar's own name and not a leftover -- stays.
pub fn apply_default_name(db: &MemoryDb) {
    if db.get_setting_bool(DEFAULT_NAME_MARKER, false) {
        return;
    }
    let stored = db.get_setting_string("agent_name", "");
    let persona = db.get_setting_string("persona_type", "default");
    if stored.trim().eq_ignore_ascii_case(LEGACY_DEFAULT_NAME) && persona != "halcy" {
        let voice = db.get_setting_string("voice_name", crate::llm::DEFAULT_VOICE);
        let nickname = default_name_for_voice(&voice);
        // Best-effort: a settings write that fails must not stop the app starting, and the
        // worst case is that this is tried again on the next open.
        if db.set_setting("agent_name", &Value::from(nickname)).is_ok() {
            eprintln!(
                "[AETHER1] The companion was still answering to {LEGACY_DEFAULT_NAME}, \
                 which was the old default name rather than a choice. It is {nickname} \
                 now -- pick the hAlcy avatar, or type any name you like, in Settings."
            );
        }
    }
    let _ = db.set_setting(DEFAULT_NAME_MARKER, &Value::Bool(true));
}

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

/// The two pickers, for the HUD: the cloud voices, and the optional Piper voices with whether each
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

    /// Every voice the picker offers has a nickname, and every nickname is an Aurora one.
    #[test]
    fn every_cloud_voice_has_an_aurora_nickname() {
        const FAMILY: [&str; 5] = ["Aurora", "Rory", "Auro", "Rora", "Aura"];
        for (voice, _) in CLOUD_VOICES {
            let nickname = VOICE_NAMES
                .iter()
                .find(|(name, _)| name == voice)
                .map(|(_, nickname)| *nickname)
                .unwrap_or_else(|| panic!("{voice} has no nickname in VOICE_NAMES"));
            assert!(
                FAMILY.contains(&nickname),
                "{voice} is called {nickname}, which is not an Aurora nickname"
            );
        }
    }

    /// A voice that is not in the table -- a Piper model, the OS voice -- still gets a name.
    #[test]
    fn an_unknown_voice_falls_back_to_the_full_name() {
        assert_eq!(default_name_for_voice("en-ZZ-NobodyNeural"), DEFAULT_NAME);
        assert_eq!(default_name_for_voice(""), DEFAULT_NAME);
    }

    #[test]
    fn the_old_default_and_the_nicknames_read_as_unchosen() {
        assert!(is_unchosen_name(LEGACY_DEFAULT_NAME));
        assert!(is_unchosen_name("aurora"));
        assert!(is_unchosen_name("Rory"));
        assert!(is_unchosen_name("  "));
        assert!(!is_unchosen_name("Mnemosyne"));
        assert!(!is_unchosen_name("A.R.X.LIMES"));
    }

    fn db() -> MemoryDb {
        let path = std::env::temp_dir().join(format!(
            "aether1_persona_voice_{}_{:?}.db",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&path);
        MemoryDb::open(&path).expect("open test db")
    }

    /// The install this was written for: `HALCY` in the database, put there by a Save of
    /// something else, with nobody having chosen it.
    #[test]
    fn the_one_time_move_renames_an_install_that_never_chose() {
        let db = db();
        db.set_setting("agent_name", &Value::from(LEGACY_DEFAULT_NAME))
            .unwrap();
        db.set_setting("voice_name", &Value::from("en-US-GuyNeural"))
            .unwrap();
        apply_default_name(&db);
        assert_eq!(db.get_setting_string("agent_name", ""), "Rory");

        // And once only: a name typed after the move has to stick.
        db.set_setting("agent_name", &Value::from(LEGACY_DEFAULT_NAME))
            .unwrap();
        apply_default_name(&db);
        assert_eq!(
            db.get_setting_string("agent_name", ""),
            LEGACY_DEFAULT_NAME,
            "the move runs once; HALCY typed afterwards is a choice"
        );
    }

    /// An operator who picked the hAlcy avatar is called HALCY on purpose, and the move
    /// must not read that as a leftover.
    #[test]
    fn the_move_leaves_a_chosen_halcy_alone() {
        let db = db();
        db.set_setting("agent_name", &Value::from(LEGACY_DEFAULT_NAME))
            .unwrap();
        db.set_setting("persona_type", &Value::from("halcy"))
            .unwrap();
        apply_default_name(&db);
        assert_eq!(db.get_setting_string("agent_name", ""), LEGACY_DEFAULT_NAME);
    }

    /// Nor does it write a name onto an install that has none -- an absent row is already
    /// the Aurora default, and filling it in would turn a default into a stored choice.
    #[test]
    fn the_move_writes_nothing_when_no_name_is_stored() {
        let db = db();
        apply_default_name(&db);
        assert_eq!(db.get_setting_string("agent_name", "<none>"), "<none>");
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
        // No per-persona Piper voice is configured by default; system TTS is the normal path.
        assert!(local_voice(&db, "red9000").is_none());

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
        set(&db, "llm", Some("en-AU-NatashaNeural"), None).unwrap();
        assert_eq!(
            cloud_voice(&db, "llm").as_deref(),
            Some("en-AU-NatashaNeural")
        );
        assert!(local_voice(&db, "llm").is_none());
        assert!(set(&db, "llm", None, Some("en_US-amy-medium")).is_err());
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
