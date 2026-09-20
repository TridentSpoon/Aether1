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
use std::time::{Duration, Instant};

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
