// The one door out of this machine.
//
// Local-only mode (src/local_only.rs) was correct before this module existed. It was
// correct because nineteen separate places each remembered to ask `local_only::enabled`
// before reaching out. That is a property of the people who wrote those nineteen places,
// not a property of the program, and it decays in one direction only: the twentieth
// subsystem is the one that forgets, and when it forgets the failure is silent. Traffic
// leaves a machine whose operator was told it would not, and nothing in a compiler, a
// test run or a code review necessarily notices.
//
// So the check lives here instead of being remembered. Every outbound HTTP request in
// Aether1 is built by `get` or `post` below, both of which refuse before they hand back a
// request builder. There is no second way to build one: `scripts/check_egress_gate.sh`
// fails the build if `ureq`'s request verbs appear in any file but this one. A new
// subsystem that wants the network cannot get a request without passing the gate, because
// asking for the request *is* passing the gate.
//
// Two things this module must not do, both of which would break the product rather than
// protect it:
//
//   1. It must not treat the local network as the internet. Reaching an Ollama server on
//      loopback or on the LAN is the whole point of the platform, and local-only mode has
//      always allowed it (see docs/IMPLEMENTATION.md, "What 'local' means here"). The
//      distinction lives in `local_only::is_local_endpoint`, and `require_online` asks it
//      first: a local URL is allowed without the mode ever being consulted.
//
//   2. It must not cache the operator's answer. The mode can be switched off in Settings
//      mid-session, and a cached "on" would keep refusing after they opened the door --
//      just as a cached "off" would keep the door open after they shut it. The setting is
//      read fresh at every decision, which is what `local_only::enabled` already
//      promises.
//
// The gate needs the settings database to read the mode, and most of the call sites it
// replaced had no database in scope -- `releases.rs`, `github_auth.rs`,
// `model_scanner.rs` and `downloads.rs` are all reached from places that never held one.
// Threading one into each would have been a diff across the Tauri commands, the HTTP
// routes and the CLI, for no gain: there is exactly one settings database per process.
// So the process installs it here once at startup, with `install`, and the gate reads it
// at each decision. `MemoryDb` is a path, not a connection (src/llm/db.rs), so holding
// one costs nothing and every read opens its own connection.
//
// Before `install` has run, the gate refuses anything that is not provably local. That is
// the uncomfortable direction to fail in -- a missed `install` in some future entry point
// breaks that entry point's network features loudly -- and it is the correct one, because
// the other direction is traffic leaving a machine whose policy was never loaded.
// `check_egress_gate.sh` checks that each entry point we have installs it, so the
// uncomfortable case is a CI failure rather than a bug report.

use std::sync::OnceLock;

use crate::llm::MemoryDb;

/// The settings database this process decides egress with. Installed once at startup;
/// see the module comment for why it lives here instead of being threaded through.
static POLICY: OnceLock<MemoryDb> = OnceLock::new();

/// Hands the gate the settings database. Call this once, as early in an entry point as
/// the database exists -- before anything that might reach the network.
///
/// Idempotent and never fails: a second call keeps the first database rather than
/// panicking, because two entry points both being careful is not an error. Returns
/// whether this call was the one that installed it, which the tests use.
pub fn install(db: &MemoryDb) -> bool {
    POLICY.set(db.clone()).is_ok()
}

/// Whether a policy has been installed. Used by diagnostics (`doctor.rs`) so an operator
/// looking at a machine whose network features all refuse can see why.
pub fn policy_installed() -> bool {
    POLICY.get().is_some()
}

/// Whether local-only mode is on, as the gate sees it.
///
/// With no policy installed, the environment override is still authoritative and the
/// answer is otherwise "on", matching the fail-closed behaviour of `require_online`.
pub fn local_only_on() -> bool {
    match POLICY.get() {
        Some(db) => crate::local_only::enabled(db),
        None => true,
    }
}

/// The single decision: may this process talk to `url` right now, and if not, why not.
///
/// `action` completes "so ..." in the refusal the operator reads -- "no model was
/// downloaded", "the page was not fetched" -- so it is written from their side of the
/// screen, naming what did not happen rather than what the code was doing.
///
/// A URL on this machine or on the local network is always allowed: see the module
/// comment. A URL that cannot be parsed is not provably local, so it is refused whenever
/// the mode is on, which is the same way `is_local_endpoint` fails.
pub fn require_online(url: &str, action: &str) -> Result<(), String> {
    if crate::local_only::is_local_endpoint(url) {
        return Ok(());
    }

    // Checked before the database so that a machine nailed shut by the environment stays
    // shut even in an entry point that never installed a policy.
    if crate::local_only::env_forced() {
        return Err(crate::local_only::refusal(action));
    }

    match POLICY.get() {
        Some(db) => {
            if crate::local_only::enabled(db) {
                Err(crate::local_only::refusal(action))
            } else {
                Ok(())
            }
        }
        // Fail closed, and say which of the two it is: an operator who sees this has hit
        // a bug in Aether1, not a setting of theirs, and the message has to send them to
        // the right place.
        None => Err(format!(
            "Aether1 has not loaded its network policy yet, so {action}. This is a bug \
             rather than a setting -- please report it."
        )),
    }
}

/// An outbound GET, if the operator's policy allows one.
///
/// The returned builder is unconfigured, exactly as `ureq::get` would be, so callers keep
/// their own timeouts and headers.
pub fn get(
    url: &str,
    action: &str,
) -> Result<ureq::RequestBuilder<ureq::typestate::WithoutBody>, String> {
    require_online(url, action)?;
    Ok(ureq::get(url))
}

/// An outbound POST, if the operator's policy allows one. See `get`.
pub fn post(
    url: &str,
    action: &str,
) -> Result<ureq::RequestBuilder<ureq::typestate::WithBody>, String> {
    require_online(url, action)?;
    Ok(ureq::post(url))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_mode(db: &MemoryDb, on: bool) {
        db.set_setting(crate::local_only::SETTING, &serde_json::json!(on))
            .unwrap();
    }

    // `POLICY` is process-wide and an `OnceLock` cannot be unset, and the environment
    // override is read from the process environment, so none of the gate's states can be
    // set up independently of the others: tests in one binary run in parallel threads and
    // would see each other's setup. They are therefore checked in one test, in the order
    // a real process moves through them.
    #[test]
    fn the_gate_follows_the_operators_policy() {
        let dir = std::env::temp_dir().join("aether1-net-gate-test");
        std::fs::create_dir_all(&dir).unwrap();
        let db = MemoryDb::open(dir.join("memory.db")).unwrap();
        set_mode(&db, false);

        // Local traffic is allowed in every state, including before a policy loads: the
        // local engine has to keep working, which is the whole product.
        for local in [
            "http://127.0.0.1:11434/api/tags",
            "http://192.168.1.9:11434/api/tags",
            "http://aether-box.local:11434/api/tags",
            "http://localhost:1234/v1/models",
        ] {
            assert!(
                require_online(local, "x").is_ok(),
                "{local} should be allowed"
            );
        }

        // An unparseable URL is not provably local, so it must not take the shortcut
        // above -- it has to reach the mode check and fail closed there.
        for url in ["", "not a url", "localhost:11434", "://x"] {
            assert!(
                !crate::local_only::is_local_endpoint(url),
                "{url:?} must not read as local"
            );
        }

        // No policy yet: anything off this network is refused, and the refusal says it is
        // a bug rather than sending the operator to a setting they never touched.
        let refused = require_online("https://api.openai.com/v1", "nothing was sent").unwrap_err();
        assert!(
            refused.contains("has not loaded its network policy"),
            "{refused}"
        );
        assert!(refused.contains("nothing was sent"), "{refused}");
        assert!(local_only_on(), "with no policy the gate reads as closed");
        assert!(require_online("not a url", "x").is_err());

        // The environment can nail the mode shut before any policy exists, and the local
        // engine still answers. Checked here, with the variable removed again, because
        // the gate reads the environment at each decision.
        unsafe { std::env::set_var(crate::local_only::ENV_VAR, "1") };
        let forced = require_online("https://api.openai.com/v1", "nothing left").unwrap_err();
        assert!(forced.contains("local-only mode is on"), "{forced}");
        assert!(require_online("http://127.0.0.1:11434/api/tags", "x").is_ok());
        unsafe { std::env::remove_var(crate::local_only::ENV_VAR) };

        assert!(
            install(&db),
            "this test must be the one that installs the policy"
        );
        assert!(policy_installed());

        // Mode off: the internet is reachable, through the gate and nowhere else.
        assert!(require_online("https://api.openai.com/v1", "x").is_ok());
        assert!(!local_only_on());
        assert!(get("https://api.openai.com/v1", "x").is_ok());
        assert!(post("https://api.openai.com/v1", "x").is_ok());

        // Mode on: it is not, and the local engine still is.
        set_mode(&db, true);
        assert!(local_only_on());
        let refused =
            require_online("https://api.openai.com/v1", "no request was made").unwrap_err();
        assert!(refused.contains("local-only mode is on"), "{refused}");
        assert!(refused.contains("no request was made"), "{refused}");
        assert!(refused.contains("Settings"), "{refused}");
        assert!(get("https://api.openai.com/v1", "x").is_err());
        assert!(post("https://api.openai.com/v1", "x").is_err());
        assert!(require_online("http://127.0.0.1:11434/api/tags", "x").is_ok());
        assert!(require_online("http://aether-box.local:11434/api/tags", "x").is_ok());
        // Unparseable with the mode on: refused, not waved through.
        assert!(require_online("not a url", "x").is_err());

        // Read fresh, not cached: switching it back off reopens the door within the same
        // process, which is what the Settings panel does mid-session.
        set_mode(&db, false);
        assert!(require_online("https://api.openai.com/v1", "x").is_ok());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
