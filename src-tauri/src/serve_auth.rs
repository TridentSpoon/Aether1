// The shared secret that gates `--serve --lan`.
//
// Modeled on the "pairing phrase" pattern browser sync services use: a random phrase is
// generated once, shown to the operator exactly once, and typed into each other device that
// should be able to reach this instance -- rather than copying a raw token or scanning a QR
// code, which is what most self-hosted tools reach for instead. Nothing about the phrase
// itself is ever written to disk. What IS written to disk is a SHA-256 hash of a token
// *derived* from the phrase (see `derive_token`), so a leaked config file hands out nothing
// usable: reproducing the working token means re-typing the same words.
//
// Loopback mode (the default -- see server.rs's bind_address) needs none of this, since the
// operating system already refuses every connection that isn't from this machine. This
// module only matters once `--lan` is opted into, which is also why it stays a separate,
// skippable module rather than something wired into every `--serve` run regardless.

use std::path::{Path, PathBuf};

use bip39::Mnemonic;
use sha2::{Digest, Sha256};

/// How many words the pairing phrase has. 12 words is a 128-bit BIP-39 mnemonic -- already
/// far more entropy than a rate-limited LAN auth endpoint could ever be brute-forced through;
/// this isn't protecting a cryptocurrency wallet; it's a shared password substitute for a
/// household, so more words would only make it harder to type in without buying anything.
const WORD_COUNT: usize = 12;

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The token actually carried in the `Authorization` header, derived from the phrase rather
/// than being the phrase itself -- so the wire credential is a fixed-length opaque string,
/// and the phrase is never what gets logged, cached in a browser, or pasted into a bug
/// report by someone debugging a failed request.
fn derive_token(mnemonic: &Mnemonic) -> String {
    // The passphrase argument is BIP-39's optional extra secret for a wallet; there's no
    // second secret here, so it's left empty and all the entropy comes from the words.
    to_hex(&mnemonic.to_seed("")[..32])
}

fn hash_token(token: &str) -> String {
    to_hex(&Sha256::digest(token.as_bytes()))
}

/// Same-length, branchless comparison so a mistyped token takes the same time to reject
/// regardless of where the first wrong byte falls -- the usual defense against timing
/// attacks on a secret comparison.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn hash_path() -> PathBuf {
    crate::project_root()
        .join("backend")
        .join("serve_token.hash")
}

/// Holds only the hash -- never the phrase, never even the derived token in the clear --
/// which is all `accepts` needs to check a presented token.
pub struct ServeAuth {
    token_hash: String,
}

impl ServeAuth {
    pub fn accepts(&self, presented_token: &str) -> bool {
        constant_time_eq(
            hash_token(presented_token).as_bytes(),
            self.token_hash.as_bytes(),
        )
    }
}

/// Whether `load_or_create` found a phrase already set up, or had to generate a fresh one.
pub enum Setup {
    Existing,
    /// The phrase is only ever available here, at the moment it's generated -- there is no
    /// "show me the phrase again" later, the same way Brave/Signal-style pairing schemes
    /// don't let you recover a lost sync phrase from the server.
    New {
        phrase: String,
    },
}

fn generate_and_save_at(path: &Path) -> Result<(String, String), String> {
    let mnemonic = Mnemonic::generate(WORD_COUNT)
        .map_err(|e| format!("could not generate a pairing phrase: {e}"))?;
    let phrase = mnemonic.to_string();
    let hash = hash_token(&derive_token(&mnemonic));

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    std::fs::write(path, &hash).map_err(|e| {
        format!(
            "could not save the pairing token to {}: {e}",
            path.display()
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Best-effort: a filesystem that won't take permission changes (some network
        // mounts) still has a working token on it, just not one this can lock down further.
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok((phrase, hash))
}

fn load_or_create_at(path: &Path) -> Result<(ServeAuth, Setup), String> {
    if let Ok(existing) = std::fs::read_to_string(path) {
        let existing = existing.trim();
        if !existing.is_empty() {
            return Ok((
                ServeAuth {
                    token_hash: existing.to_string(),
                },
                Setup::Existing,
            ));
        }
    }
    let (phrase, hash) = generate_and_save_at(path)?;
    Ok((ServeAuth { token_hash: hash }, Setup::New { phrase }))
}

/// Loads the stored pairing token, generating and saving a fresh one on first run.
pub fn load_or_create() -> Result<(ServeAuth, Setup), String> {
    load_or_create_at(&hash_path())
}

/// Generates a brand new phrase and overwrites the stored token, invalidating whatever
/// devices were paired with the old one. Used by `aether1 pair` when the operator wants to
/// revoke access rather than just add a device (adding a device re-uses the existing phrase
/// -- there's nothing to rotate for that).
pub fn rotate() -> Result<String, String> {
    generate_and_save_at(&hash_path()).map(|(phrase, _hash)| phrase)
}

/// Turns a phrase someone typed in (e.g. into the HUD's pairing prompt) into the same token
/// `ServeAuth::accepts` would check -- this is what `/api/pair` calls to answer "does this
/// phrase work," without ever exposing the stored hash itself to the network.
pub fn derive_token_from_phrase(phrase: &str) -> Result<String, String> {
    Mnemonic::parse_normalized(phrase)
        .map(|m| derive_token(&m))
        .map_err(|_| "that doesn't look like a valid pairing phrase".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "aether1_serve_auth_{name}_{}.hash",
            std::process::id()
        ))
    }

    #[test]
    fn a_fresh_path_generates_a_phrase_and_saves_only_its_hash() {
        let path = temp_path("fresh");
        let _ = std::fs::remove_file(&path);

        let (auth, setup) = load_or_create_at(&path).expect("should generate a fresh token");
        let phrase = match setup {
            Setup::New { phrase } => phrase,
            Setup::Existing => panic!("a path with nothing on it should be treated as new"),
        };
        assert_eq!(phrase.split_whitespace().count(), WORD_COUNT);

        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(
            !on_disk.contains(' '),
            "the phrase must never be written to disk, only its derived hash"
        );

        let token = derive_token_from_phrase(&phrase).unwrap();
        assert!(auth.accepts(&token));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_saved_token_is_loaded_rather_than_regenerated() {
        let path = temp_path("existing");
        let _ = std::fs::remove_file(&path);

        let (_auth, first_setup) = load_or_create_at(&path).unwrap();
        let Setup::New { phrase } = first_setup else {
            panic!("a path with nothing on it should be treated as new")
        };
        let original_token = derive_token_from_phrase(&phrase).unwrap();

        let (auth, second_setup) = load_or_create_at(&path).unwrap();
        assert!(matches!(second_setup, Setup::Existing));

        // The original phrase must still work against the reloaded auth -- reloading must
        // not have generated a different token underneath it.
        assert!(auth.accepts(&original_token));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_wrong_phrase_is_rejected() {
        let path = temp_path("wrong-phrase");
        let _ = std::fs::remove_file(&path);

        let (auth, setup) = load_or_create_at(&path).unwrap();
        let Setup::New { phrase } = setup else {
            panic!("expected a freshly generated phrase")
        };
        let correct_token = derive_token_from_phrase(&phrase).unwrap();
        assert!(auth.accepts(&correct_token));

        let wrong_mnemonic = Mnemonic::generate(WORD_COUNT).unwrap();
        let wrong_token = derive_token(&wrong_mnemonic);
        assert!(!auth.accepts(&wrong_token));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rotating_invalidates_the_previous_phrase() {
        let path = temp_path("rotate");
        let _ = std::fs::remove_file(&path);

        let (_auth, setup) = load_or_create_at(&path).unwrap();
        let Setup::New { phrase: old_phrase } = setup else {
            panic!("expected a freshly generated phrase")
        };
        let old_token = derive_token_from_phrase(&old_phrase).unwrap();

        let (new_phrase, new_hash) = generate_and_save_at(&path).unwrap();
        assert_ne!(old_phrase, new_phrase);
        let reloaded = ServeAuth {
            token_hash: new_hash,
        };
        assert!(!reloaded.accepts(&old_token));
        assert!(reloaded.accepts(&derive_token_from_phrase(&new_phrase).unwrap()));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_invalid_phrase_is_reported_rather_than_panicking() {
        assert!(derive_token_from_phrase("not a real bip39 phrase at all").is_err());
    }

    #[test]
    fn constant_time_eq_matches_naive_equality() {
        assert!(constant_time_eq(b"same", b"same"));
        assert!(!constant_time_eq(b"same", b"diff"));
        assert!(!constant_time_eq(b"short", b"longer-string"));
    }
}
