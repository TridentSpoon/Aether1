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

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use bip39::Mnemonic;
use serde::{Deserialize, Serialize};
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

fn devices_path() -> PathBuf {
    crate::project_root()
        .join("backend")
        .join("serve_devices.json")
}

/// How many bytes of randomness a device's own token is made of. 256 bits from the operating
/// system's generator, unrelated to the phrase: a device token is not derived from anything,
/// so learning one says nothing about the phrase or about any other device's.
const TOKEN_BYTES: usize = 32;

fn mint_token() -> Result<String, String> {
    let mut bytes = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes)
        .map_err(|e| format!("could not read randomness to make a device token: {e}"))?;
    Ok(to_hex(&bytes))
}

/// A short handle for one device, shown when listing and typed when revoking. Derived from
/// the token's hash rather than being a counter, so it is stable, unguessable from the
/// outside, and says nothing about how many devices there are.
fn device_id(token_hash: &str) -> String {
    token_hash.chars().take(8).collect()
}

fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default()
}

/// One paired device. The token itself is never here -- only its hash, the same way the
/// phrase was never on disk either.
#[derive(Clone, Serialize, Deserialize)]
struct Device {
    id: String,
    label: String,
    token_hash: String,
    paired_at: u64,
}

/// What a listing shows. Deliberately not the hash: nothing outside this module has any use
/// for it, and a list is something an operator might paste into a message asking for help.
pub struct DeviceSummary {
    pub id: String,
    pub label: String,
    pub paired_at: u64,
}

/// Holds the phrase's hash -- never the phrase, never a token in the clear -- plus one entry
/// per device that has paired.
///
/// The phrase and a device's token do different jobs now. The phrase is what lets a *new*
/// device in, checked once at `/api/pair` and never accepted as a credential afterwards; the
/// token a device gets back is what it presents from then on. That split is what makes
/// revoking one machine possible: before, every device carried the same phrase-derived
/// token, so taking access from one meant taking it from all of them and pairing everything
/// again.
pub struct ServeAuth {
    /// The stored phrase hash, behind a lock because it can change under a running server:
    /// `aether1 pair`, and the pane's New phrase / Use phrase buttons, all write this file
    /// from a *different process* to the one serving. Held as a value read from disk rather
    /// than a constant so a phrase changed while `--serve --lan` is up takes effect on the
    /// next pairing attempt instead of the next restart -- the same reason the device list
    /// is stamped and re-read, and the same bug in the other half of the credential.
    phrase_hash: Mutex<String>,
    /// What the hash file looked like when `phrase_hash` was read from it.
    hash_stamp: Mutex<Option<Stamp>>,
    hash_file: PathBuf,
    /// Where the one-time pairing code lives, when there is one.
    code_file: PathBuf,
    devices: Mutex<Vec<Device>>,
    /// What the device file looked like when the list in memory was read from it. `aether1
    /// revoke` runs in a separate process from `aether1 --serve`, so without this the server
    /// would go on accepting a revoked device until it was restarted -- which is precisely
    /// the moment revoking is for. Caught running the two side by side, not by a test.
    stamp: Mutex<Option<Stamp>>,
    devices_file: PathBuf,
}

/// Enough of a file's identity to notice it changed: when it was last written and how long
/// it is. Cheap to take -- one `stat` -- which matters because it is taken per request.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    modified: Option<SystemTime>,
    length: u64,
}

fn stamp(path: &Path) -> Option<Stamp> {
    let metadata = std::fs::metadata(path).ok()?;
    Some(Stamp {
        modified: metadata.modified().ok(),
        length: metadata.len(),
    })
}

impl ServeAuth {
    /// Whether this token belongs to a device that is still paired.
    pub fn accepts(&self, presented_token: &str) -> bool {
        let presented = hash_token(presented_token);
        let devices = self.devices();
        // Every device is checked even after a match, so the time taken says nothing about
        // which entry matched or how far down the list it was.
        devices.iter().fold(false, |found, device| {
            constant_time_eq(presented.as_bytes(), device.token_hash.as_bytes()) | found
        })
    }

    /// Whether this is the live one-time pairing code.
    ///
    /// Read from disk on every call rather than held: the code is minted by the window
    /// process while this one serves, and one `stat` and a short read on a path that is
    /// rate limited to five attempts a minute is not worth caching around.
    pub fn code_matches(&self, presented: &str) -> bool {
        let Some(stored) = read_pairing_code(&self.code_file) else {
            return false;
        };
        let presented = hash_token(&normalise_code(presented));
        constant_time_eq(presented.as_bytes(), stored.hash.as_bytes())
    }

    /// Spends the code, so it lets exactly one device in. A code that could be typed twice
    /// is a password with a short life, which is not what it was offered as.
    pub fn spend_code(&self) {
        let _ = std::fs::remove_file(&self.code_file);
    }

    /// Whether this is the token the pairing phrase derives to -- the one thing the phrase is
    /// still good for, and not something `accepts` will take.
    pub fn phrase_matches(&self, presented_token: &str) -> bool {
        // Cheap enough to do per call -- one `stat` -- and this path is rate limited to five
        // attempts a minute per address anyway.
        self.reload_phrase_if_changed();
        let stored = self
            .phrase_hash
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        constant_time_eq(hash_token(presented_token).as_bytes(), stored.as_bytes())
    }

    /// Re-reads the phrase hash when the file has changed underneath us, so a phrase minted
    /// or adopted in the window process is the one a device has to type from then on.
    ///
    /// An unreadable or empty file leaves the remembered hash in place: the alternative is a
    /// server that accepts nothing, or worse an empty hash that some future comparison
    /// treats as a match, on the strength of one failed read.
    fn reload_phrase_if_changed(&self) {
        let current = stamp(&self.hash_file);
        let mut remembered = self
            .hash_stamp
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *remembered == current {
            return;
        }
        if let Some(found) = read_hash(&self.hash_file) {
            *self
                .phrase_hash
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = found;
        }
        *remembered = current;
    }

    fn devices(&self) -> Vec<Device> {
        self.reload_if_changed();
        self.lock().clone()
    }

    /// Re-reads the device file when it has changed underneath us, so a revocation made from
    /// another process takes hold on the next request rather than the next restart.
    fn reload_if_changed(&self) {
        let current = stamp(&self.devices_file);
        let mut remembered = self
            .stamp
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *remembered == current {
            return;
        }
        *self.lock() = load_devices(&self.devices_file);
        *remembered = current;
    }

    /// Records the file as we just left it, so writing does not look like somebody else's
    /// change and send us back to disk for what is already in memory.
    fn restamp(&self) {
        let current = stamp(&self.devices_file);
        *self
            .stamp
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = current;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Device>> {
        self.devices
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Mints a token for a newly paired device and remembers it. The token is returned once,
    /// here, and never again -- only its hash is kept.
    pub fn add_device(&self, label: &str) -> Result<String, String> {
        let token = mint_token()?;
        let token_hash = hash_token(&token);
        let device = Device {
            id: device_id(&token_hash),
            label: tidy_label(label),
            token_hash,
            paired_at: now_seconds(),
        };
        {
            let mut devices = self.lock();
            devices.push(device);
            save_devices(&self.devices_file, &devices)?;
        }
        self.restamp();
        Ok(token)
    }

    pub fn list_devices(&self) -> Vec<DeviceSummary> {
        self.devices()
            .into_iter()
            .map(|device| DeviceSummary {
                id: device.id,
                label: device.label,
                paired_at: device.paired_at,
            })
            .collect()
    }

    /// Takes one device's access away without touching any other. Returns what was revoked,
    /// so the caller can say which machine it just cut off rather than only that it did.
    pub fn revoke_device(&self, id: &str) -> Result<Option<String>, String> {
        // Against the list as it stands on disk, not a copy that may have gone stale while
        // another process was revoking too -- otherwise saving would put the other's entry back.
        self.reload_if_changed();
        let mut devices = self.lock();
        let Some(position) = devices.iter().position(|device| device.id == id) else {
            return Ok(None);
        };
        let removed = devices.remove(position);
        save_devices(&self.devices_file, &devices)?;
        drop(devices);
        self.restamp();
        Ok(Some(removed.label))
    }

    /// Drops every paired device, which is what rotating the phrase has always promised:
    /// "any phrase paired before this no longer works". Per-device tokens would quietly
    /// break that promise if the devices those phrases produced outlived the rotation.
    pub fn revoke_all_devices(&self) -> Result<usize, String> {
        // Against the list as it stands on disk, not a copy that may have gone stale while
        // another process was revoking too -- otherwise saving would put the other's entry back.
        self.reload_if_changed();
        let mut devices = self.lock();
        let count = devices.len();
        devices.clear();
        save_devices(&self.devices_file, &devices)?;
        drop(devices);
        self.restamp();
        Ok(count)
    }
}

/// Keeps a device label short, printable and on one line. It arrives from the network -- a
/// browser's User-Agent, or whatever a client chose to send -- so it is not to be trusted to
/// be any of those things.
fn tidy_label(label: &str) -> String {
    let cleaned: String = label
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.is_empty() {
        return "an unnamed device".to_string();
    }
    cleaned.chars().take(60).collect()
}

fn save_devices(path: &Path, devices: &[Device]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(devices)
        .map_err(|e| format!("could not write out the device list: {e}"))?;
    std::fs::write(path, json).map_err(|e| format!("could not save {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// The stored phrase hash as written, or nothing when the file is missing, unreadable or
/// blank. Shared by every path that asks what phrase this machine currently answers to.
fn read_hash(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|found| found.trim().to_string())
        .filter(|found| !found.is_empty())
}

fn load_devices(path: &Path) -> Vec<Device> {
    // A missing file is an install with nothing paired yet. A corrupt one is treated the
    // same way: refusing to start because a list of devices will not parse would lock the
    // operator out of the only machine that can fix it, and the cost of being wrong is that
    // devices pair again.
    std::fs::read_to_string(path)
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
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
    save_hash_at(path, &hash)?;
    Ok((phrase, hash))
}

/// Writes the stored token hash, locked down as far as the filesystem allows. Shared by the
/// phrase this machine generates for itself and the one it is given from another machine --
/// both end as the same single hash on disk, which is the whole point: nothing downstream
/// can tell, or needs to, where the phrase came from.
fn save_hash_at(path: &Path, hash: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    std::fs::write(path, hash).map_err(|e| {
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
    Ok(())
}

fn load_or_create_at(path: &Path, devices_file: &Path) -> Result<(ServeAuth, Setup), String> {
    let existing = read_hash(path);

    let (phrase_hash, setup) = match existing {
        Some(hash) => (hash, Setup::Existing),
        None => {
            let (phrase, hash) = generate_and_save_at(path)?;
            (hash, Setup::New { phrase })
        }
    };

    let mut devices = load_devices(devices_file);
    // An install that paired before devices had tokens of their own has exactly one
    // credential in the wild: the phrase-derived token. It is recorded here as a device so
    // those machines keep working across the upgrade -- and, being a device now, it can be
    // revoked like any other once they have paired again.
    if devices.is_empty() && matches!(setup, Setup::Existing) && !devices_file.exists() {
        devices.push(Device {
            id: device_id(&phrase_hash),
            label: "paired before devices had tokens of their own".to_string(),
            token_hash: phrase_hash.clone(),
            paired_at: now_seconds(),
        });
        save_devices(devices_file, &devices)?;
    }

    Ok((
        ServeAuth {
            phrase_hash: Mutex::new(phrase_hash),
            hash_stamp: Mutex::new(stamp(path)),
            hash_file: path.to_path_buf(),
            // Beside the device list rather than fetched from `pair_code_path`, so a
            // ServeAuth built on temporary paths keeps its code there too.
            code_file: devices_file
                .parent()
                .map(|dir| dir.join("serve_pair_code.json"))
                .unwrap_or_else(pair_code_path),
            devices: Mutex::new(devices),
            stamp: Mutex::new(stamp(devices_file)),
            devices_file: devices_file.to_path_buf(),
        },
        setup,
    ))
}

/// Loads the stored pairing token and the devices paired with it, generating a fresh phrase
/// on first run.
pub fn load_or_create() -> Result<(ServeAuth, Setup), String> {
    load_or_create_at(&hash_path(), &devices_path())
}

/// Generates a brand new phrase and overwrites the stored token, invalidating whatever
/// devices were paired with the old one. Used by `aether1 pair` when the operator wants to
/// revoke access rather than just add a device (adding a device re-uses the existing phrase
/// -- there's nothing to rotate for that).
pub fn rotate() -> Result<(String, usize), String> {
    let (phrase, _hash) = generate_and_save_at(&hash_path())?;
    // Every device paired under the old phrase goes with it, which is what "the old phrase no
    // longer works" has always meant. Revoking one device is `revoke` -- this is the
    // everything-at-once door, and it should stay the blunt one.
    let devices = load_devices(&devices_path());
    let count = devices.len();
    save_devices(&devices_path(), &[])?;
    Ok((phrase, count))
}

/// Takes a phrase made somewhere else and makes it this machine's phrase too.
///
/// The mirror of `rotate`: same one hash on disk at the end, and the same consequence for
/// devices paired under whatever phrase was there before. It exists because a household with
/// two AETHER1s otherwise has two phrases to keep, and the pane could only ever mint a new
/// one -- there was no way to say "use the words I already have".
///
/// The phrase is validated as a BIP-39 mnemonic before anything is written, so a typo cannot
/// leave the machine with a token nobody can reproduce. Adopting the phrase that is already
/// stored here is a no-op rather than a mass unpairing: the hash would be identical, so every
/// paired device's token still works and taking them away would be a lie about what changed.
pub fn adopt(phrase: &str) -> Result<usize, String> {
    adopt_at(&hash_path(), &devices_path(), phrase)
}

fn adopt_at(path: &Path, devices_file: &Path, phrase: &str) -> Result<usize, String> {
    let hash = hash_token(&derive_token(&derive_token_from_phrase_inner(phrase)?));

    let unchanged = read_hash(path).is_some_and(|found| found == hash);
    if unchanged {
        return Ok(0);
    }

    save_hash_at(path, &hash)?;
    let count = load_devices(devices_file).len();
    save_devices(devices_file, &[])?;
    Ok(count)
}

/// How long a pairing code is good for. Long enough to walk to another room and get a
/// browser open, short enough that a code left on a screen is not a standing invitation.
pub const PAIRING_CODE_TTL: u64 = 600;

/// How many characters a pairing code has. Eight from a 32-symbol alphabet is 40 bits --
/// against five attempts a minute and a ten-minute life, guessing one is not a strategy,
/// and it is short enough to read off a screen and type on a phone without a mistake.
const CODE_LENGTH: usize = 8;

/// No I, L, O or U: the first three are the characters people confuse with 1 and 0 when
/// copying by eye, and the fourth is left out so a code cannot spell an unfortunate word.
const CODE_ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

fn pair_code_path() -> PathBuf {
    crate::project_root()
        .join("backend")
        .join("serve_pair_code.json")
}

/// A code as typed, reduced to what it means. Case, the dash the screen shows it with, and
/// any spaces are presentation; and since I, L and O are not in the alphabet, someone who
/// types them plainly meant the 1 or the 0 they look like.
fn normalise_code(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| match c.to_ascii_uppercase() {
            'I' | 'L' => '1',
            'O' => '0',
            other => other,
        })
        .collect()
}

/// A pairing code on disk: its hash and when it stops working.
///
/// On disk rather than in the server's memory because the two are different processes --
/// the window mints the code and the `--serve --lan` child is the one that has to honour
/// it, exactly the split that made the phrase hash need re-reading.
#[derive(Serialize, Deserialize)]
struct PairingCode {
    hash: String,
    expires_at: u64,
}

fn read_pairing_code(path: &Path) -> Option<PairingCode> {
    let code: PairingCode = std::fs::read_to_string(path)
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())?;
    // An expired code is the same as no code. Returned as None rather than deleted, because
    // the process asking is often the server, and a read should not write.
    (code.expires_at > now_seconds()).then_some(code)
}

/// Makes a code that will let exactly one device pair, and says when it stops working.
///
/// A second call replaces the first: there is one code at a time, so a code read out loud
/// and then re-made cannot still be sitting there working.
pub fn mint_pairing_code() -> Result<(String, u64), String> {
    mint_pairing_code_at(&pair_code_path())
}

fn mint_pairing_code_at(path: &Path) -> Result<(String, u64), String> {
    let mut bytes = [0u8; CODE_LENGTH];
    getrandom::fill(&mut bytes)
        .map_err(|e| format!("could not read randomness to make a pairing code: {e}"))?;
    // Rejection-free because the alphabet is exactly 32 symbols and 256 is a multiple of it,
    // so every byte maps to a symbol with no bias.
    let code: String = bytes
        .iter()
        .map(|b| CODE_ALPHABET[(*b as usize) % CODE_ALPHABET.len()] as char)
        .collect();
    let expires_at = now_seconds() + PAIRING_CODE_TTL;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    let record = PairingCode {
        hash: hash_token(&code),
        expires_at,
    };
    let json = serde_json::to_string(&record)
        .map_err(|e| format!("could not write out the pairing code: {e}"))?;
    std::fs::write(path, json)
        .map_err(|e| format!("could not save the pairing code to {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok((code, expires_at))
}

/// When the live pairing code stops working, or nothing when there is none. What the pane
/// counts down from, and the only thing about a code that can be asked twice -- the code
/// itself is shown by the press that made it, like the phrase.
pub fn pairing_code_expiry() -> Option<u64> {
    read_pairing_code(&pair_code_path()).map(|code| code.expires_at)
}

/// Throws the live code away. Called when a device has used it and when the operator leaves
/// the sequence, so a code outlives neither the pairing it was for nor the screen it was on.
pub fn clear_pairing_code() {
    let _ = std::fs::remove_file(pair_code_path());
}

/// Loads the device list on its own, for `aether1 devices` and `aether1 revoke`, which have
/// no server running and no phrase to check.
pub fn open_devices() -> Result<ServeAuth, String> {
    let (auth, _setup) = load_or_create_at(&hash_path(), &devices_path())?;
    Ok(auth)
}

/// Whether a pairing phrase has ever been set up here, asked without setting one up.
pub fn pairing_is_set_up() -> bool {
    hash_path().exists()
}

/// The paired devices, read straight off disk.
///
/// Deliberately not `open_devices`: that one generates a phrase when none exists yet, which
/// is right for `aether1 devices` (the operator asked about pairing) and wrong for the
/// Profile pane, which lists devices merely because Settings was opened. Minting a
/// credential should stay something somebody asked for.
pub fn paired_devices() -> Vec<DeviceSummary> {
    load_devices(&devices_path())
        .into_iter()
        .map(|device| DeviceSummary {
            id: device.id,
            label: device.label,
            paired_at: device.paired_at,
        })
        .collect()
}

/// `aether1 revoke <id>`, done from the HUD. Works on the file rather than on a running
/// server's copy of the list, exactly as the CLI does -- a `--serve --lan` process notices
/// the file changed on its next request and stops honouring the token (see `Stamp`).
pub fn revoke_paired_device(id: &str) -> Result<Option<String>, String> {
    let path = devices_path();
    let mut devices = load_devices(&path);
    let Some(position) = devices.iter().position(|device| device.id == id) else {
        return Ok(None);
    };
    let removed = devices.remove(position);
    save_devices(&path, &devices)?;
    Ok(Some(removed.label))
}

/// `aether1 revoke all`, done from the HUD. The phrase is left alone, so each machine can
/// pair again with what the operator already has written down.
pub fn revoke_all_paired_devices() -> Result<usize, String> {
    let path = devices_path();
    let count = load_devices(&path).len();
    save_devices(&path, &[])?;
    Ok(count)
}

/// Turns a phrase someone typed in (e.g. into the HUD's pairing prompt) into the same token
/// `ServeAuth::accepts` would check -- this is what `/api/pair` calls to answer "does this
/// phrase work," without ever exposing the stored hash itself to the network.
pub fn derive_token_from_phrase(phrase: &str) -> Result<String, String> {
    derive_token_from_phrase_inner(phrase).map(|m| derive_token(&m))
}

/// The parse on its own, so `adopt` can hash the same token this derives without going
/// through a string and back.
fn derive_token_from_phrase_inner(phrase: &str) -> Result<Mnemonic, String> {
    Mnemonic::parse_normalized(phrase.trim())
        .map_err(|_| "that doesn't look like a valid pairing phrase".to_string())
}

/// How many wrong guesses an address gets before it is made to wait. Five is generous for
/// someone mistyping twelve words and hopeless for anyone working through a dictionary.
const MAX_FAILURES: u32 = 5;

/// Failures older than this stop counting, so a wrong phrase typed on Monday and another on
/// Friday never add up to a lockout.
const FAILURE_WINDOW: Duration = Duration::from_secs(60);

/// How long an address that exhausted its attempts is refused, correct phrase or not.
const LOCKOUT: Duration = Duration::from_secs(60);

/// A ceiling on how many addresses are tracked at once, because the key is chosen by whoever
/// is connecting: without it, a long enough run of attempts from changing addresses would
/// grow this map until the process ran out of memory, which is its own denial of service.
const MAX_TRACKED: usize = 1024;

struct Attempts {
    failures: u32,
    window_started: Instant,
    locked_until: Option<Instant>,
}

impl Attempts {
    /// True once nothing about this record can affect a decision any more, which is what
    /// makes it safe to drop when the table needs room.
    fn is_spent(&self, now: Instant) -> bool {
        self.locked_until.is_none_or(|until| now >= until)
            && now.duration_since(self.window_started) >= FAILURE_WINDOW
    }
}

/// Counts failed pairing attempts per address and refuses an address that has had too many.
///
/// The phrase is 128 bits, so this is not what stands between an attacker and the token --
/// nothing could work through that space regardless. What it stops is the cheaper attack the
/// entropy does not address: a client hammering the endpoint indefinitely, which costs this
/// machine real work per request and leaves no trace anyone would notice. It also makes the
/// claim in `WORD_COUNT`'s comment above true, which it was not before.
///
/// Keyed on the peer address rather than on anything the client sends, since a header can be
/// set to whatever an attacker likes. Behind a proxy every client would share one address and
/// so one budget, which is a reason not to put this behind a proxy rather than a reason to
/// trust `X-Forwarded-For`.
pub struct AttemptLimiter {
    attempts: Mutex<HashMap<IpAddr, Attempts>>,
}

impl Default for AttemptLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl AttemptLimiter {
    pub fn new() -> Self {
        Self {
            attempts: Mutex::new(HashMap::new()),
        }
    }

    /// How long this address must wait, or None if it may try now.
    pub fn retry_after(&self, ip: IpAddr) -> Option<Duration> {
        self.retry_after_at(ip, Instant::now())
    }

    pub fn record_failure(&self, ip: IpAddr) {
        self.record_failure_at(ip, Instant::now());
    }

    /// Clears an address's history. Someone who gets it right on the fourth go should not be
    /// one typo away from a lockout for the rest of the minute.
    pub fn record_success(&self, ip: IpAddr) {
        self.lock().remove(&ip);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<IpAddr, Attempts>> {
        // A poisoned lock here means another thread panicked mid-update. The data is a few
        // counters, none of it is sensitive, and refusing to serve because of it would turn
        // one panic into a dead auth endpoint -- so the guard is taken either way.
        self.attempts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn retry_after_at(&self, ip: IpAddr, now: Instant) -> Option<Duration> {
        let attempts = self.lock();
        if let Some(record) = attempts.get(&ip) {
            let until = record.locked_until?;
            return (until > now).then(|| until.duration_since(now));
        }
        // An address nobody has seen before is normally free to try. The exception is a
        // full table where every entry is a live lockout, which takes thousands of failures
        // in one minute to reach and means an attack is underway: rather than let the next
        // new address in unmetered, everyone waits until the first of those lockouts ends.
        // Refusing an unknown client is a real cost, but it is bounded, and the alternative
        // is a bypass that costs an attacker nothing more than another address.
        saturating_wait(&attempts, now)
    }

    fn record_failure_at(&self, ip: IpAddr, now: Instant) {
        let mut attempts = self.lock();
        prune(&mut attempts, now, ip);
        if !attempts.contains_key(&ip) && attempts.len() >= MAX_TRACKED {
            // No room was freed, so there is nothing to count against. `retry_after_at`
            // refuses this address anyway for as long as that stays true.
            return;
        }

        let record = attempts.entry(ip).or_insert(Attempts {
            failures: 0,
            window_started: now,
            locked_until: None,
        });

        // A lockout that has run out, or a window that has closed, starts the count again --
        // otherwise one lockout would make every later mistake an instant second one.
        let lock_expired = record.locked_until.is_some_and(|until| now >= until);
        if lock_expired || now.duration_since(record.window_started) >= FAILURE_WINDOW {
            record.failures = 0;
            record.window_started = now;
            record.locked_until = None;
        }

        record.failures += 1;
        if record.failures >= MAX_FAILURES {
            record.locked_until = Some(now + LOCKOUT);
        }
    }
}

/// Makes room in the table before a new address is added: spent records first, and only if
/// that is not enough, whichever *unlocked* address has been quiet longest.
///
/// A live lockout is never what gets dropped. Evicting one would hand an attacker the whole
/// mechanism for free -- fail five times, then fill the table from other addresses until the
/// record of those five failures is pushed out, and start again.
fn prune(attempts: &mut HashMap<IpAddr, Attempts>, now: Instant, incoming: IpAddr) {
    if attempts.len() < MAX_TRACKED || attempts.contains_key(&incoming) {
        return;
    }
    attempts.retain(|_, record| !record.is_spent(now));
    if attempts.len() < MAX_TRACKED {
        return;
    }
    let evictable = attempts
        .iter()
        .filter(|(_, record)| !is_locked(record, now))
        .min_by_key(|(_, record)| record.window_started)
        .map(|(ip, _)| *ip);
    if let Some(ip) = evictable {
        attempts.remove(&ip);
    }
}

fn is_locked(record: &Attempts, now: Instant) -> bool {
    record.locked_until.is_some_and(|until| until > now)
}

/// How long an unknown address must wait when the table is full and every entry in it is a
/// live lockout -- None whenever there is still room to track someone new.
fn saturating_wait(attempts: &HashMap<IpAddr, Attempts>, now: Instant) -> Option<Duration> {
    if attempts.len() < MAX_TRACKED {
        return None;
    }
    let earliest = attempts
        .values()
        .map(|record| record.locked_until.filter(|until| *until > now))
        .min()??;
    Some(earliest.duration_since(now))
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

    /// The phrase file and the device list, both fresh.
    fn temp_paths(name: &str) -> (PathBuf, PathBuf) {
        let hash = temp_path(name);
        let devices = hash.with_extension("devices.json");
        let _ = std::fs::remove_file(&hash);
        let _ = std::fs::remove_file(&devices);
        (hash, devices)
    }

    /// Pairs a device the way `/api/pair` does and hands back its token.
    fn pair(auth: &ServeAuth, phrase: &str, label: &str) -> String {
        let token = derive_token_from_phrase(phrase).unwrap();
        assert!(auth.phrase_matches(&token), "the phrase should be accepted");
        auth.add_device(label).unwrap()
    }

    /// The bug that made "connecting via LAN doesn't work, regardless of the phrase" true:
    /// the pane writes the hash from the window's process while `--serve --lan` runs in a
    /// child of it, so a phrase minted or adopted after the server started was checked
    /// against the hash the server read at boot. Every device then got "wrong pairing
    /// phrase" for the phrase the pane had just shown, and the *old* phrase still worked.
    #[test]
    fn a_phrase_changed_while_the_server_runs_is_the_one_it_checks() {
        let (hash_file, devices_file) = temp_paths("phrase_reload");

        // A server comes up and reads the phrase that exists now.
        let (auth, setup) = load_or_create_at(&hash_file, &devices_file).unwrap();
        let first = match setup {
            Setup::New { phrase } => phrase,
            Setup::Existing => panic!("a fresh path should be new"),
        };
        assert!(auth.phrase_matches(&derive_token_from_phrase(&first).unwrap()));

        // Another process rotates it -- `aether1 pair`, or the pane's New phrase button.
        // The file's length never changes (a hash is a fixed width), so only the modified
        // time distinguishes it; sleep past the filesystem's resolution rather than trust it.
        std::thread::sleep(Duration::from_millis(1100));
        let (second, _hash) = generate_and_save_at(&hash_file).unwrap();

        assert!(
            auth.phrase_matches(&derive_token_from_phrase(&second).unwrap()),
            "the running server should take the phrase that was just made"
        );
        assert!(
            !auth.phrase_matches(&derive_token_from_phrase(&first).unwrap()),
            "and should stop taking the one it replaced"
        );

        let _ = std::fs::remove_file(&hash_file);
        let _ = std::fs::remove_file(&devices_file);
    }

    /// A hash file that has gone missing must not turn into a server that accepts anything,
    /// nor one that has forgotten the phrase it was serving a moment ago.
    #[test]
    fn a_vanished_hash_file_leaves_the_phrase_it_was_serving_in_place() {
        let (hash_file, devices_file) = temp_paths("phrase_vanished");
        let (auth, setup) = load_or_create_at(&hash_file, &devices_file).unwrap();
        let phrase = match setup {
            Setup::New { phrase } => phrase,
            Setup::Existing => panic!("a fresh path should be new"),
        };

        let _ = std::fs::remove_file(&hash_file);
        assert!(
            auth.phrase_matches(&derive_token_from_phrase(&phrase).unwrap()),
            "the phrase already loaded should keep working"
        );
        assert!(!auth.phrase_matches("not the token at all"));

        let _ = std::fs::remove_file(&devices_file);
    }

    /// A code is good once. The whole reason it can be shown on a screen and read across a
    /// room is that using it takes it out of circulation.
    #[test]
    fn a_pairing_code_lets_one_device_in_and_then_stops() {
        let (hash_file, devices_file) = temp_paths("code_once");
        let (auth, _setup) = load_or_create_at(&hash_file, &devices_file).unwrap();
        let code_file = devices_file.parent().unwrap().join("serve_pair_code.json");

        let (code, expires_at) = mint_pairing_code_at(&code_file).unwrap();
        assert_eq!(code.len(), CODE_LENGTH);
        assert!(
            expires_at > now_seconds(),
            "a fresh code should be in the future"
        );
        assert!(auth.code_matches(&code));
        // How it is shown and how it is typed are not how it is stored.
        assert!(auth.code_matches(&format!("{}-{}", &code[..4], &code[4..]).to_lowercase()));

        auth.spend_code();
        assert!(
            !auth.code_matches(&code),
            "a spent code must not work twice"
        );

        let _ = std::fs::remove_file(&hash_file);
        let _ = std::fs::remove_file(&devices_file);
        let _ = std::fs::remove_file(&code_file);
    }

    /// A code left on a screen stops being an invitation by itself, with nothing having to
    /// remember to take it away.
    #[test]
    fn a_pairing_code_stops_working_when_it_runs_out() {
        let (hash_file, devices_file) = temp_paths("code_expiry");
        let (auth, _setup) = load_or_create_at(&hash_file, &devices_file).unwrap();
        let code_file = devices_file.parent().unwrap().join("serve_pair_code.json");

        let (code, _) = mint_pairing_code_at(&code_file).unwrap();
        assert!(auth.code_matches(&code));

        // Written back with an expiry in the past, which is what waiting ten minutes does.
        let expired = serde_json::to_string(&PairingCode {
            hash: hash_token(&code),
            expires_at: now_seconds() - 1,
        })
        .unwrap();
        std::fs::write(&code_file, expired).unwrap();
        assert!(
            !auth.code_matches(&code),
            "an expired code must not let anyone in"
        );
        assert!(!auth.code_matches("anything else at all"));

        let _ = std::fs::remove_file(&hash_file);
        let _ = std::fs::remove_file(&devices_file);
        let _ = std::fs::remove_file(&code_file);
    }

    /// The characters left out of the alphabet are the ones people mistake for others, so
    /// what they plainly meant is what is read -- but nothing beyond that is forgiven.
    #[test]
    fn a_code_is_read_as_typed_not_as_it_looks() {
        assert_eq!(normalise_code("abcd-efgh"), "ABCDEFGH");
        assert_eq!(normalise_code(" IL O 1234 "), "110".to_string() + "1234");
        assert_ne!(normalise_code("ABCDEFGH"), normalise_code("ABCDEFGJ"));
        for symbol in CODE_ALPHABET {
            let c = *symbol as char;
            assert!(
                !"ILOU".contains(c),
                "{c} is too easy to misread to be in a code"
            );
        }
    }

    #[test]
    fn a_phrase_from_another_machine_is_adopted_and_unpairs_what_was_there() {
        let (hash_file, devices_file) = temp_paths("adopt");

        // The other machine's phrase, made the way that machine would have made it.
        let (elsewhere, other_file) = temp_paths("adopt_elsewhere");
        let (their_phrase, their_hash) = generate_and_save_at(&elsewhere).unwrap();
        let _ = std::fs::remove_file(&other_file);

        // This machine starts with a phrase of its own and a device paired to it.
        let (auth, setup) = load_or_create_at(&hash_file, &devices_file).unwrap();
        let mine = match setup {
            Setup::New { phrase } => phrase,
            Setup::Existing => panic!("a fresh path should be new"),
        };
        pair(&auth, &mine, "laptop");

        let unpaired = adopt_at(&hash_file, &devices_file, &their_phrase).unwrap();
        assert_eq!(
            unpaired, 1,
            "the device on the old phrase should be cut off"
        );
        assert_eq!(
            std::fs::read_to_string(&hash_file).unwrap(),
            their_hash,
            "the stored hash should be the one the other machine's phrase derives"
        );

        // And the adopted phrase is now the one that opens the door here.
        let (auth, _) = load_or_create_at(&hash_file, &devices_file).unwrap();
        assert!(auth.phrase_matches(&derive_token_from_phrase(&their_phrase).unwrap()));
        assert!(!auth.phrase_matches(&derive_token_from_phrase(&mine).unwrap()));
        assert!(auth.list_devices().is_empty());

        let _ = std::fs::remove_file(&hash_file);
        let _ = std::fs::remove_file(&devices_file);
        let _ = std::fs::remove_file(&elsewhere);
    }

    #[test]
    fn adopting_the_phrase_already_stored_here_leaves_paired_devices_alone() {
        let (hash_file, devices_file) = temp_paths("adopt_same");

        let (auth, setup) = load_or_create_at(&hash_file, &devices_file).unwrap();
        let mine = match setup {
            Setup::New { phrase } => phrase,
            Setup::Existing => panic!("a fresh path should be new"),
        };
        let token = pair(&auth, &mine, "phone");

        // Typing in the words this machine already answers to changes nothing, so it must not
        // be reported or acted on as a rotation.
        assert_eq!(adopt_at(&hash_file, &devices_file, &mine).unwrap(), 0);
        let (auth, _) = load_or_create_at(&hash_file, &devices_file).unwrap();
        assert!(
            auth.accepts(&token),
            "the paired device should still be let in"
        );

        let _ = std::fs::remove_file(&hash_file);
        let _ = std::fs::remove_file(&devices_file);
    }

    #[test]
    fn a_phrase_that_is_not_a_mnemonic_is_refused_before_anything_is_written() {
        let (hash_file, devices_file) = temp_paths("adopt_bad");

        let (auth, setup) = load_or_create_at(&hash_file, &devices_file).unwrap();
        let mine = match setup {
            Setup::New { phrase } => phrase,
            Setup::Existing => panic!("a fresh path should be new"),
        };
        let token = pair(&auth, &mine, "tablet");
        let before = std::fs::read_to_string(&hash_file).unwrap();

        assert!(adopt_at(&hash_file, &devices_file, "not twelve real words at all").is_err());
        assert_eq!(std::fs::read_to_string(&hash_file).unwrap(), before);
        let (auth, _) = load_or_create_at(&hash_file, &devices_file).unwrap();
        assert!(auth.accepts(&token), "a refused phrase must cut nobody off");

        let _ = std::fs::remove_file(&hash_file);
        let _ = std::fs::remove_file(&devices_file);
    }

    #[test]
    fn a_fresh_path_generates_a_phrase_and_saves_only_its_hash() {
        let (hash_file, devices_file) = temp_paths("fresh");

        let (auth, setup) =
            load_or_create_at(&hash_file, &devices_file).expect("should generate a fresh token");
        let phrase = match setup {
            Setup::New { phrase } => phrase,
            Setup::Existing => panic!("a path with nothing on it should be treated as new"),
        };
        assert_eq!(phrase.split_whitespace().count(), WORD_COUNT);

        let on_disk = std::fs::read_to_string(&hash_file).unwrap();
        assert!(
            !on_disk.contains(' '),
            "the phrase must never be written to disk, only its derived hash"
        );

        let token = pair(&auth, &phrase, "a laptop");
        assert!(auth.accepts(&token));
    }

    #[test]
    fn a_saved_token_is_loaded_rather_than_regenerated() {
        let (hash_file, devices_file) = temp_paths("existing");

        let (first, first_setup) = load_or_create_at(&hash_file, &devices_file).unwrap();
        let Setup::New { phrase } = first_setup else {
            panic!("a path with nothing on it should be treated as new")
        };
        let device_token = pair(&first, &phrase, "a laptop");

        let (auth, second_setup) = load_or_create_at(&hash_file, &devices_file).unwrap();
        assert!(matches!(second_setup, Setup::Existing));

        // A device paired before the restart must still be paired after it -- reloading is
        // not a reason to make someone dig the phrase out again.
        assert!(auth.accepts(&device_token));
        assert_eq!(auth.list_devices().len(), 1);
    }

    #[test]
    fn the_wrong_phrase_is_rejected() {
        let (hash_file, devices_file) = temp_paths("wrong-phrase");

        let (auth, setup) = load_or_create_at(&hash_file, &devices_file).unwrap();
        let Setup::New { phrase } = setup else {
            panic!("expected a freshly generated phrase")
        };
        assert!(auth.phrase_matches(&derive_token_from_phrase(&phrase).unwrap()));

        let wrong_mnemonic = Mnemonic::generate(WORD_COUNT).unwrap();
        let wrong_token = derive_token(&wrong_mnemonic);
        assert!(!auth.phrase_matches(&wrong_token));
        assert!(!auth.accepts(&wrong_token));
    }

    #[test]
    fn the_phrase_opens_the_door_but_is_not_a_key() {
        let (hash_file, devices_file) = temp_paths("phrase-not-a-key");

        let (auth, setup) = load_or_create_at(&hash_file, &devices_file).unwrap();
        let Setup::New { phrase } = setup else {
            panic!("expected a freshly generated phrase")
        };
        let phrase_token = derive_token_from_phrase(&phrase).unwrap();

        // The phrase is what lets a new device in. It is not itself a credential: if it
        // were, every device would be carrying the same one again and revoking a single
        // machine would be impossible.
        assert!(auth.phrase_matches(&phrase_token));
        assert!(
            !auth.accepts(&phrase_token),
            "the phrase-derived token must not work as a device token"
        );
    }

    #[test]
    fn each_device_gets_its_own_token() {
        let (hash_file, devices_file) = temp_paths("per-device");

        let (auth, setup) = load_or_create_at(&hash_file, &devices_file).unwrap();
        let Setup::New { phrase } = setup else {
            panic!("expected a freshly generated phrase")
        };

        let laptop = pair(&auth, &phrase, "the laptop");
        let phone = pair(&auth, &phrase, "the phone");

        assert_ne!(laptop, phone, "two devices must not share a token");
        assert!(auth.accepts(&laptop));
        assert!(auth.accepts(&phone));
        assert_eq!(auth.list_devices().len(), 2);
    }

    #[test]
    fn revoking_one_device_leaves_the_others_alone() {
        let (hash_file, devices_file) = temp_paths("revoke-one");

        let (auth, setup) = load_or_create_at(&hash_file, &devices_file).unwrap();
        let Setup::New { phrase } = setup else {
            panic!("expected a freshly generated phrase")
        };
        let laptop = pair(&auth, &phrase, "the laptop");
        let phone = pair(&auth, &phrase, "the phone");

        let lost = auth
            .list_devices()
            .into_iter()
            .find(|device| device.label == "the phone")
            .expect("the phone should be listed");
        assert_eq!(
            auth.revoke_device(&lost.id).unwrap().as_deref(),
            Some("the phone")
        );

        assert!(
            !auth.accepts(&phone),
            "the revoked device must be locked out"
        );
        assert!(
            auth.accepts(&laptop),
            "this is the whole point: the other machines keep working"
        );

        // And it survives a restart, rather than coming back when the list is reloaded.
        let (reloaded, _) = load_or_create_at(&hash_file, &devices_file).unwrap();
        assert!(!reloaded.accepts(&phone));
        assert!(reloaded.accepts(&laptop));
    }

    #[test]
    fn revoking_an_id_that_is_not_there_says_so() {
        let (hash_file, devices_file) = temp_paths("revoke-missing");
        let (auth, _) = load_or_create_at(&hash_file, &devices_file).unwrap();
        assert_eq!(auth.revoke_device("nosuchid").unwrap(), None);
    }

    #[test]
    fn revoking_everything_unpairs_every_device() {
        let (hash_file, devices_file) = temp_paths("revoke-all");

        let (auth, setup) = load_or_create_at(&hash_file, &devices_file).unwrap();
        let Setup::New { phrase } = setup else {
            panic!("expected a freshly generated phrase")
        };
        let laptop = pair(&auth, &phrase, "the laptop");
        let phone = pair(&auth, &phrase, "the phone");

        assert_eq!(auth.revoke_all_devices().unwrap(), 2);
        assert!(!auth.accepts(&laptop));
        assert!(!auth.accepts(&phone));
        assert!(auth.list_devices().is_empty());
    }

    #[test]
    fn a_revocation_from_another_process_takes_hold_without_a_restart() {
        let (hash_file, devices_file) = temp_paths("cross-process-revoke");

        // The running server.
        let (server, setup) = load_or_create_at(&hash_file, &devices_file).unwrap();
        let Setup::New { phrase } = setup else {
            panic!("a fresh path should be new");
        };
        let laptop = pair(&server, &phrase, "the laptop");
        let phone = pair(&server, &phrase, "the phone");
        assert!(server.accepts(&phone));

        // `aether1 revoke` is a separate process, so it reads the same files fresh, revokes,
        // and exits. The server is still running and still holds its own copy of the list.
        let (cli, _) = load_or_create_at(&hash_file, &devices_file).unwrap();
        let id = cli
            .list_devices()
            .into_iter()
            .find(|device| device.label == "the phone")
            .expect("the phone should be listed")
            .id;
        assert_eq!(
            cli.revoke_device(&id).unwrap().as_deref(),
            Some("the phone")
        );

        assert!(
            !server.accepts(&phone),
            "a revoked device must be shut out on its next request, not at the next restart"
        );
        assert!(
            server.accepts(&laptop),
            "and the machines that were not revoked must not be disturbed by the reload"
        );
    }

    #[test]
    fn a_device_paired_before_this_existed_keeps_working() {
        let (hash_file, devices_file) = temp_paths("migration");

        // An install from before device tokens: a phrase on disk, no device list, and one
        // credential in the wild -- the token the phrase derives to.
        let (phrase, _hash) = generate_and_save_at(&hash_file).unwrap();
        let old_token = derive_token_from_phrase(&phrase).unwrap();

        let (auth, setup) = load_or_create_at(&hash_file, &devices_file).unwrap();
        assert!(matches!(setup, Setup::Existing));
        assert!(
            auth.accepts(&old_token),
            "upgrading must not silently lock out the machines already paired"
        );

        // And it is a device like any other, so it can be revoked once they have paired
        // again rather than being a permanent exception.
        let legacy = auth.list_devices().pop().expect("should be listed");
        auth.revoke_device(&legacy.id).unwrap();
        assert!(!auth.accepts(&old_token));
    }

    #[test]
    fn a_label_from_the_network_is_not_taken_at_its_word() {
        assert_eq!(tidy_label("  a   laptop \n"), "a laptop");
        assert_eq!(tidy_label("two\nlines"), "two lines");
        assert_eq!(tidy_label("   "), "an unnamed device");
        assert_eq!(tidy_label(&"x".repeat(500)).chars().count(), 60);
    }

    #[test]
    fn rotating_invalidates_the_previous_phrase() {
        let (hash_file, devices_file) = temp_paths("rotate");

        let (auth, setup) = load_or_create_at(&hash_file, &devices_file).unwrap();
        let Setup::New { phrase: old_phrase } = setup else {
            panic!("expected a freshly generated phrase")
        };
        let old_device = pair(&auth, &old_phrase, "a laptop");

        let (new_phrase, _new_hash) = generate_and_save_at(&hash_file).unwrap();
        assert_ne!(old_phrase, new_phrase);
        auth.revoke_all_devices().unwrap();

        let (reloaded, _) = load_or_create_at(&hash_file, &devices_file).unwrap();
        assert!(
            !reloaded.accepts(&old_device),
            "a device paired under the old phrase must not outlive it"
        );
        assert!(reloaded.phrase_matches(&derive_token_from_phrase(&new_phrase).unwrap()));
    }

    #[test]
    fn an_invalid_phrase_is_reported_rather_than_panicking() {
        assert!(derive_token_from_phrase("not a real bip39 phrase at all").is_err());
    }

    fn ip(last: u8) -> IpAddr {
        IpAddr::from([192, 168, 1, last])
    }

    #[test]
    fn an_address_is_locked_out_once_it_runs_out_of_attempts() {
        let limiter = AttemptLimiter::new();
        let now = Instant::now();
        let client = ip(10);

        for _ in 1..MAX_FAILURES {
            limiter.record_failure_at(client, now);
            assert!(
                limiter.retry_after_at(client, now).is_none(),
                "a typo before the last one must not lock anyone out"
            );
        }

        limiter.record_failure_at(client, now);
        let wait = limiter
            .retry_after_at(client, now)
            .expect("the last attempt should have locked this address out");
        assert!(wait <= LOCKOUT && wait > Duration::ZERO);
    }

    #[test]
    fn the_lockout_ends_by_itself() {
        let limiter = AttemptLimiter::new();
        let now = Instant::now();
        let client = ip(11);

        for _ in 0..MAX_FAILURES {
            limiter.record_failure_at(client, now);
        }
        assert!(limiter.retry_after_at(client, now).is_some());
        assert!(limiter.retry_after_at(client, now + LOCKOUT).is_none());
    }

    #[test]
    fn a_lockout_that_expired_starts_the_count_over() {
        let limiter = AttemptLimiter::new();
        let now = Instant::now();
        let client = ip(12);

        for _ in 0..MAX_FAILURES {
            limiter.record_failure_at(client, now);
        }
        // One wrong guess after the wait is over must not walk straight back into a lockout.
        let later = now + LOCKOUT;
        limiter.record_failure_at(client, later);
        assert!(limiter.retry_after_at(client, later).is_none());
    }

    #[test]
    fn failures_spread_past_the_window_never_add_up() {
        let limiter = AttemptLimiter::new();
        let mut now = Instant::now();
        let client = ip(13);

        for _ in 0..MAX_FAILURES * 3 {
            limiter.record_failure_at(client, now);
            assert!(
                limiter.retry_after_at(client, now).is_none(),
                "attempts a full window apart are not an attack"
            );
            now += FAILURE_WINDOW;
        }
    }

    #[test]
    fn getting_it_right_clears_what_came_before() {
        let limiter = AttemptLimiter::new();
        let now = Instant::now();
        let client = ip(14);

        for _ in 1..MAX_FAILURES {
            limiter.record_failure_at(client, now);
        }
        limiter.record_success(client);

        // Having been forgiven, this address gets its whole budget back rather than being
        // one typo from a lockout for the rest of the minute.
        for _ in 1..MAX_FAILURES {
            limiter.record_failure_at(client, now);
            assert!(limiter.retry_after_at(client, now).is_none());
        }
    }

    #[test]
    fn one_address_locking_itself_out_does_not_lock_out_another() {
        let limiter = AttemptLimiter::new();
        let now = Instant::now();

        for _ in 0..MAX_FAILURES {
            limiter.record_failure_at(ip(15), now);
        }
        assert!(limiter.retry_after_at(ip(15), now).is_some());
        assert!(
            limiter.retry_after_at(ip(16), now).is_none(),
            "a neighbour mistyping the phrase must not shut this machine out"
        );
    }

    #[test]
    fn the_table_stays_bounded_however_many_addresses_turn_up() {
        let limiter = AttemptLimiter::new();
        let now = Instant::now();

        for n in 0..(MAX_TRACKED as u32 * 2) {
            limiter.record_failure_at(IpAddr::from(n.to_be_bytes()), now);
        }
        assert!(limiter.lock().len() <= MAX_TRACKED);
    }

    #[test]
    fn a_locked_out_address_survives_the_table_filling_up() {
        let limiter = AttemptLimiter::new();
        let now = Instant::now();
        let client = ip(17);

        for _ in 0..MAX_FAILURES {
            limiter.record_failure_at(client, now);
        }
        // Pruning evicts the quietest record, and this one's window started first, so the
        // guard is that a live lockout is never what gets dropped to make room.
        for n in 0..(MAX_TRACKED as u32) {
            limiter.record_failure_at(IpAddr::from(n.to_be_bytes()), now + Duration::from_secs(1));
        }
        assert!(
            limiter.retry_after_at(client, now).is_some(),
            "a lockout must not be evictable by flooding the table"
        );
    }

    #[test]
    fn an_unknown_address_is_free_to_try_while_the_table_has_room() {
        let limiter = AttemptLimiter::new();
        let now = Instant::now();

        // One address locked out is nothing like a saturated table, and must not make the
        // rest of the house wait.
        for _ in 0..MAX_FAILURES {
            limiter.record_failure_at(ip(18), now);
        }
        assert!(limiter.retry_after_at(ip(19), now).is_none());
    }

    #[test]
    fn a_table_full_of_live_lockouts_makes_everyone_wait() {
        let limiter = AttemptLimiter::new();
        let now = Instant::now();

        for n in 0..(MAX_TRACKED as u32) {
            let attacker = IpAddr::from(n.to_be_bytes());
            for _ in 0..MAX_FAILURES {
                limiter.record_failure_at(attacker, now);
            }
        }

        let wait = limiter
            .retry_after_at(ip(20), now)
            .expect("a table of nothing but live lockouts should refuse a new address");
        assert!(wait <= LOCKOUT);
        // And it lets go on its own once those lockouts run out.
        assert!(limiter.retry_after_at(ip(20), now + LOCKOUT).is_none());
    }

    #[test]
    fn constant_time_eq_matches_naive_equality() {
        assert!(constant_time_eq(b"same", b"same"));
        assert!(!constant_time_eq(b"same", b"diff"));
        assert!(!constant_time_eq(b"short", b"longer-string"));
    }
}
