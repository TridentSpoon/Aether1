// The local-only switch.
//
// Aether1 can already run without the internet: the model scanner probes loopback ports,
// Piper speaks, whisper.cpp listens, the vault is a file on disk, and every stylesheet,
// font and script the HUD needs is vendored into the repo. But "runs offline" was an
// emergent property of what happened to be installed rather than something the operator
// could switch on and check. Two paths reached out on their own -- speech fell back to
// Microsoft's Read Aloud service when Piper was missing, and the launch-time update check
// called GitHub -- and a cloud API key sitting in the environment could quietly promote an
// "offline" install to a cloud one.
//
// This module is the switch that makes it a stated mode instead of a hope. When it is on,
// every path in Aether1 that would leave this machine refuses and says so. Nothing here
// blocks the *local network*: reaching an Ollama server on the LAN is the point of the
// platform (see docs/IMPLEMENTATION.md, "What 'local' means here"), and a future version
// that routes between several local models must keep working with this switch on.
//
// It is off by default, because switching it on would break every operator who chose a
// cloud provider deliberately. It is not in `SETTABLE` in tools/mutating.rs, so the
// companion cannot turn it off about itself -- that decision stays with the operator, the
// same way tools_enabled does.

use crate::llm::MemoryDb;

/// The settings key. Read through `enabled` rather than directly, so the environment
/// override below can never be bypassed by reading the row.
pub const SETTING: &str = "local_only";

/// Forces local-only on regardless of the setting. For an operator who wants the mode
/// nailed shut -- a locked-down machine, a shared install, a wrapper script -- rather than
/// merely switched on in a database anyone with the file can edit.
pub const ENV_VAR: &str = "AETHER1_LOCAL_ONLY";

/// What counts as "on" in the environment. Deliberately narrow: an unset variable, an
/// empty one, and an explicit "0"/"false"/"no"/"off" all mean off, and everything else
/// means on. Anyone who sets this variable at all meant something by it.
pub fn truthy(value: Option<&str>) -> bool {
    match value.map(str::trim) {
        None | Some("") => false,
        Some(v) => !matches!(
            v.to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        ),
    }
}

/// Whether the environment is forcing the mode on.
pub fn env_forced() -> bool {
    truthy(std::env::var(ENV_VAR).ok().as_deref())
}

/// Whether anything in this process may talk to the internet.
///
/// Read this fresh at each decision rather than caching it: the operator can flip the
/// setting mid-session from the Settings panel, and a cached "off" would keep a cloud
/// path open after they closed it.
pub fn enabled(db: &MemoryDb) -> bool {
    env_forced() || db.get_setting_bool(SETTING, false)
}

/// Whether `endpoint` names something on this machine or on the local network.
///
/// This is the boundary the project has already written down (docs/IMPLEMENTATION.md,
/// "What 'local' means here"): the local host, and the LAN, and no further. Loopback and
/// the private ranges are in; a public address or a public hostname is out. It exists
/// because `llm_provider` can be "ollama" -- which local-only mode allows, since talking
/// to a model server on the LAN is the whole point -- while `llm_endpoint` points at a
/// rented box on the internet. Checking the provider without checking the address it is
/// pointed at would be a boundary with a hole in the middle of it.
///
/// A hostname that is not an address is judged by name: `localhost` and anything under
/// `.local` (mDNS, which is how most LAN machines are actually addressed) are local, and
/// every other name is treated as remote. That is deliberately strict -- `nas.lan` may
/// well resolve to a private address, but resolving names here would mean a DNS lookup,
/// and asking the internet where a host is, in order to decide whether we may talk to
/// the internet, is not a question worth asking. An operator with such a name can point
/// the setting at its address instead.
pub fn is_local_endpoint(endpoint: &str) -> bool {
    let Ok(parsed) = url::Url::parse(endpoint.trim()) else {
        // Unparseable is not provably local, and this decision fails closed.
        return false;
    };
    match parsed.host() {
        Some(url::Host::Ipv4(ip)) => {
            ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
        }
        Some(url::Host::Ipv6(ip)) => {
            ip.is_loopback()
                || ip.is_unspecified()
                // Unique local (fc00::/7) and link-local (fe80::/10). Both have stable
                // `is_*` helpers only on nightly, so the prefixes are checked directly.
                || (ip.segments()[0] & 0xfe00) == 0xfc00
                || (ip.segments()[0] & 0xffc0) == 0xfe80
        }
        Some(url::Host::Domain(name)) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            name == "localhost" || name.ends_with(".localhost") || name.ends_with(".local")
        }
        None => false,
    }
}

/// Whether writing `value` to the setting `key` would point Aether1 at the internet, and
/// if so, why.
///
/// The companion can change `llm_provider` and `llm_endpoint` about itself (see SETTABLE
/// in tools/mutating.rs) -- which is the right call normally, and exactly the wrong one
/// with local-only mode on: "switch yourself to OpenAI" would otherwise be a one-approval
/// way out of the mode. The switch itself is already unreachable, being absent from
/// SETTABLE; this closes the door next to it.
pub fn setting_reaches_the_internet(key: &str, value: &serde_json::Value) -> Option<String> {
    let text = value.as_str()?;
    match key {
        // The wire keys of the four cloud providers, as `Provider::from_key` reads them.
        "llm_provider" => matches!(text, "openai" | "groq" | "gemini" | "anthropic")
            .then(|| format!("{text} is a cloud provider")),
        "llm_endpoint" => (!is_local_endpoint(text))
            .then(|| format!("{text} is not on this machine or the local network")),
        _ => None,
    }
}

/// The one sentence every refusal says, so the operator meets the same words wherever
/// they hit the boundary. `action` completes "so ..." -- e.g. "the cloud voice was not
/// used".
pub fn refusal(action: &str) -> String {
    format!("local-only mode is on, so {action}. Turn it off in Settings if you want this.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unset_or_empty_variable_is_off() {
        assert!(!truthy(None));
        assert!(!truthy(Some("")));
        assert!(!truthy(Some("   ")));
    }

    #[test]
    fn the_usual_ways_of_writing_no_are_off() {
        for off in ["0", "false", "FALSE", "no", "off", " Off "] {
            assert!(!truthy(Some(off)), "{off:?} should read as off");
        }
    }

    #[test]
    fn anything_else_is_on() {
        for on in ["1", "true", "yes", "on", "please"] {
            assert!(truthy(Some(on)), "{on:?} should read as on");
        }
    }

    #[test]
    fn loopback_and_the_private_ranges_are_local() {
        for endpoint in [
            "http://localhost:11434",
            "http://LOCALHOST:11434",
            "http://127.0.0.1:11434",
            "http://127.2.3.4:1234",
            "http://[::1]:11434",
            "http://10.0.0.5:11434",
            "http://172.16.4.1:11434",
            "http://172.31.255.255:11434",
            "http://192.168.1.50:1234",
            "http://169.254.10.2:11434",
            "http://aether-box.local:11434",
            "https://nas.LOCAL/v1",
            "http://[fd00::1]:11434",
            "http://[fe80::1]:11434",
        ] {
            assert!(is_local_endpoint(endpoint), "{endpoint} should be local");
        }
    }

    #[test]
    fn public_addresses_and_names_are_not_local() {
        for endpoint in [
            "https://api.openai.com/v1",
            "http://8.8.8.8:11434",
            "https://ollama.example.com",
            // Adjacent to the private ranges without being in them -- the usual
            // off-by-one that makes a hand-written check wrong.
            "http://172.15.0.1:11434",
            "http://172.32.0.1:11434",
            "http://11.0.0.1:11434",
            "http://192.169.1.1:11434",
            "http://[2001:4860:4860::8888]:11434",
        ] {
            assert!(!is_local_endpoint(endpoint), "{endpoint} should be remote");
        }
    }

    #[test]
    fn anything_unparseable_fails_closed() {
        for endpoint in ["", "   ", "not a url", "localhost:11434", "://x"] {
            assert!(
                !is_local_endpoint(endpoint),
                "{endpoint:?} is not provably local and must be refused"
            );
        }
    }

    #[test]
    fn a_name_that_merely_contains_local_is_not_local() {
        // ".local" has to end the name, not appear somewhere in it -- otherwise
        // "local.example.com" walks straight through.
        assert!(!is_local_endpoint("http://local.example.com:11434"));
        assert!(!is_local_endpoint("http://mylocal.example.com:11434"));
    }

    #[test]
    fn the_companion_cannot_point_itself_at_the_cloud() {
        use serde_json::json;
        for provider in ["openai", "groq", "gemini", "anthropic"] {
            assert!(
                setting_reaches_the_internet("llm_provider", &json!(provider)).is_some(),
                "{provider} should be refused"
            );
        }
        for provider in ["ollama", "lmstudio", "offline"] {
            assert!(
                setting_reaches_the_internet("llm_provider", &json!(provider)).is_none(),
                "{provider} should be allowed"
            );
        }
        assert!(
            setting_reaches_the_internet("llm_endpoint", &json!("https://ollama.example.com"))
                .is_some()
        );
        assert!(
            setting_reaches_the_internet("llm_endpoint", &json!("http://192.168.1.9:11434"))
                .is_none()
        );
        // Settings that have nothing to do with the network are not this check's business.
        assert!(setting_reaches_the_internet("agent_name", &json!("openai")).is_none());
    }

    #[test]
    fn a_refusal_names_the_mode_and_the_way_out() {
        let message = refusal("the cloud voice was not used");
        assert!(message.contains("local-only mode is on"));
        assert!(message.contains("the cloud voice was not used"));
        assert!(message.contains("Settings"));
    }
}
