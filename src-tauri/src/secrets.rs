//! Where a provider key lives.
//!
//! Every cloud provider needs a key, and until now each one sat in the settings table in
//! plain text -- a row in a SQLite file anything able to read the home directory can open,
//! next to the font size. `github_auth` had already decided that question for its own
//! token: the system credential store when there is one, something weaker only when there
//! is not, and [`Storage`] saying out loud which of the two it got rather than letting the
//! weaker one pass for the stronger. This is the same answer for the model keys, with one
//! difference -- the fallback here is the settings row the key was already in, because
//! that is where every install that predates this module has its key and nobody should
//! have to re-enter one to upgrade.
//!
//! Two things follow from a key living outside the settings table:
//!
//!   * `get_settings` can no longer hand it back to the page. It sends [`SAVED`] instead,
//!     which is the form saying "there is one" without saying what it is, and
//!     [`is_sentinel`] is how `save_settings` recognises that sentinel coming home again
//!     and leaves the stored key alone. Without it, every Save from a page that was
//!     never told the key would wipe the key.
//!   * The keychain is one store for the whole machine, not one per database. The account
//!     name is the setting name -- `llm_api_key_openai` and so on -- so a key saved for
//!     one provider is still a key for that provider after a reinstall.

use crate::llm::MemoryDb;

/// The name AETHER1 stores credentials under in the system keychain. The same service
/// `github_auth` uses; the account distinguishes them.
const SERVICE: &str = "AETHER1";

/// The service name under the other branding, read when `SERVICE` has nothing.
///
/// **A keychain entry is not ours to lose.** The service name is half of the key an entry is
/// filed under, so renaming it does not move anything: every provider key the operator
/// pasted in simply stops being found, with no error and no way for them to tell that the
/// value is still sitting there under the old name. So both names are read, newest first,
/// and `store` writes to whichever name this build prefers -- which means the release that
/// flips `SERVICE` to the Aurora1 spelling still finds every key, and writes it across the
/// first time it is saved again.
const ALT_SERVICE: &str = "AURORA1";

/// What the page is given in place of a key it may not see. A password box full of dots
/// either way, so an operator sees exactly what they saw before: a key is set.
///
/// Deliberately not a run of bullets. A bullet is a character somebody could conceivably
/// paste, and a key that happened to equal the sentinel would be silently ignored.
pub const SAVED: &str = "\u{2022}\u{2022}\u{2022} saved \u{2022}\u{2022}\u{2022}";

/// Whether a value coming back from the page is the sentinel rather than a key.
pub fn is_sentinel(value: &str) -> bool {
    value.trim() == SAVED
}

/// Which store a secret is actually in, for showing back to the operator. The point of
/// naming it is that `Settings` is weaker than `Keychain` and should not be able to pass
/// for it: a homelab box with no Secret Service running gets the settings row, and the
/// pane says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Storage {
    Keychain,
    Settings,
    None,
}

/// Switched off for the whole process, by [`use_settings_table_only`] or by setting
/// `AETHER1_NO_KEYCHAIN` in the environment.
///
/// The environment variable is the escape hatch for a machine whose credential store asks
/// for a password every time it is read -- a key that cannot be read without a prompt is
/// worse than a key in the database, and nothing here can tell the difference from the
/// outside. The flag is what the tests use, so that a test suite run on somebody's own
/// desktop does not leave rows in their login keychain.
static KEYCHAIN_OFF: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Keeps secrets in the settings table for the rest of this process. One way: nothing
/// turns it back on, so it cannot matter which order tests run in.
///
/// Test-only. An operator wanting the same thing sets `AETHER1_NO_KEYCHAIN`, which this
/// does not need a function for; a build that compiled this in would be offering a way to
/// quietly weaken where a key is kept.
#[cfg(test)]
pub fn use_settings_table_only() {
    KEYCHAIN_OFF.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn keychain_off() -> bool {
    KEYCHAIN_OFF.load(std::sync::atomic::Ordering::Relaxed)
        || std::env::var_os("AETHER1_NO_KEYCHAIN").is_some()
}

fn entry(name: &str) -> Option<keyring::Entry> {
    if keychain_off() {
        return None;
    }
    keyring::Entry::new(SERVICE, name).ok()
}

/// The same entry under the other branding. Reads only -- nothing writes to the name this
/// build is not using.
fn alt_entry(name: &str) -> Option<keyring::Entry> {
    if keychain_off() {
        return None;
    }
    keyring::Entry::new(ALT_SERVICE, name).ok()
}

/// A non-empty secret from either service name, this build's preference first.
fn from_keychain(name: &str) -> Option<String> {
    for entry in [entry(name), alt_entry(name)].into_iter().flatten() {
        if let Ok(secret) = entry.get_password() {
            if !secret.trim().is_empty() {
                return Some(secret);
            }
        }
    }
    None
}

/// Reads one secret: keychain first, settings row second.
///
/// The settings row is a fallback rather than dead weight. An install that has never run
/// this version has its key there, and a machine with no credential store keeps it there
/// for good.
pub fn load(db: &MemoryDb, name: &str) -> String {
    if let Some(secret) = from_keychain(name) {
        return secret;
    }
    db.get_setting_string(name, "")
}

/// Writes one secret, and says where it landed.
///
/// A successful keychain write deletes the settings row, for the reason `store_token`
/// deletes its file: a copy left behind outlives the stronger one and stays readable by
/// anything that can open the database.
pub fn store(db: &MemoryDb, name: &str, secret: &str) -> Storage {
    if secret.trim().is_empty() {
        clear(db, name);
        return Storage::None;
    }
    if let Some(entry) = entry(name) {
        if entry.set_password(secret).is_ok() {
            let _ = db.delete_setting(name);
            return Storage::Keychain;
        }
    }
    let _ = db.set_setting(name, &serde_json::Value::String(secret.to_string()));
    Storage::Settings
}

/// Forgets one secret, in both stores. Both, rather than whichever answered, because the
/// point of a Forget button is that nothing is left.
pub fn clear(db: &MemoryDb, name: &str) {
    // Both service names as well as both stores, for the same reason: a Forget button that
    // leaves the value readable under the other branding has not forgotten it.
    for entry in [entry(name), alt_entry(name)].into_iter().flatten() {
        let _ = entry.delete_credential();
    }
    let _ = db.delete_setting(name);
}

/// Where this secret is kept, or that there is none.
pub fn storage_of(db: &MemoryDb, name: &str) -> Storage {
    if from_keychain(name).is_some() {
        return Storage::Keychain;
    }
    if db.get_setting_string(name, "").trim().is_empty() {
        Storage::None
    } else {
        Storage::Settings
    }
}

/// Whether this machine has a credential store that will actually take a secret.
///
/// Asked by writing a throwaway value and taking it away again, because
/// `keyring::Entry::new` succeeds on a box with no Secret Service running at all and only
/// the write finds out. Answered once per process: on such a box every attempt is a D-Bus
/// round trip that fails, and the pane asks this each time it is drawn.
pub fn keychain_available() -> bool {
    if keychain_off() {
        return false;
    }
    static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        let Some(entry) = entry("aether1-keychain-probe") else {
            return false;
        };
        if entry.set_password("probe").is_err() {
            return false;
        }
        let _ = entry.delete_credential();
        true
    })
}

pub fn has(db: &MemoryDb, name: &str) -> bool {
    storage_of(db, name) != Storage::None
}

/// Moves whatever keys are in the settings table into the keychain, once, at startup.
///
/// Without this, a key only leaves the plain-text row on the day its owner next opens
/// Settings and presses Save -- which for somebody who configured OpenAI a year ago and
/// has not thought about it since is never. Writing through [`store`] means a machine with
/// no credential store is left exactly as it was rather than churned.
pub fn migrate(db: &MemoryDb) {
    let Ok(settings) = db.get_all_settings() else {
        return;
    };
    let Some(map) = settings.as_object() else {
        return;
    };
    let names: Vec<String> = map
        .keys()
        .filter(|name| {
            name.as_str() == "llm_api_key"
                || name.as_str() == crate::llm::GATEWAY_KEY_SETTING
                || name.starts_with("llm_api_key_")
        })
        .cloned()
        .collect();
    for name in names {
        let secret = db.get_setting_string(&name, "");
        if secret.trim().is_empty() {
            // An empty row is not a key, and writing it back through `store` would only
            // delete it. Left alone: the defaults in `get_settings` list `llm_api_key`,
            // and an install that has saved once has an empty row for every box it has.
            continue;
        }
        store(db, &name, &secret);
    }
}
