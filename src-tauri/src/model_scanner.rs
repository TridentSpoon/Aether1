// Rust port of backend/model_scanner.py: cloud API key detection, local Ollama/LM Studio
// probing, and one-click small model pulls. Structs replace the loose response dicts
// (CloudKeys, OllamaStatus, LmStudioStatus, ScanResult); PullStatus is an enum matched
// with `match` instead of a free-form "status" string the caller has to compare by hand.

use serde::{Deserialize, Serialize};
use std::process::Command;
use std::time::Duration;

const OLLAMA_URL: &str = "http://localhost:11434";
const LMSTUDIO_URL: &str = "http://localhost:1234";
const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct CloudKeys {
    pub detected_env_keys: Vec<String>,
    pub detected_key: String,
    pub detected_provider: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OllamaStatus {
    pub available: bool,
    pub cli_installed: bool,
    pub endpoint: String,
    pub models: Vec<String>,
    pub recommended_model: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LmStudioStatus {
    pub available: bool,
    pub endpoint: String,
    pub models: Vec<String>,
    pub recommended_model: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanResult {
    pub cloud_keys: CloudKeys,
    pub ollama: OllamaStatus,
    pub lmstudio: LmStudioStatus,
    pub has_local_provider: bool,
    pub has_cloud_key: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PullStatus {
    Success,
    Started,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct PullResult {
    pub status: PullStatus,
    pub message: String,
}

/// Detects Gemini, OpenAI, Groq, and Anthropic API keys in the environment. Anthropic is
/// scanned (it shows up in detected_env_keys if set) but -- matching the Python -- never
/// becomes the auto-selected detected_key/detected_provider; that's an existing quirk of
/// the original, not something introduced here.
pub fn detect_cloud_keys() -> CloudKeys {
    let env = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    let named = [
        ("GEMINI_API_KEY", env("GEMINI_API_KEY")),
        ("GOOGLE_API_KEY", env("GOOGLE_API_KEY")),
        ("OPENAI_API_KEY", env("OPENAI_API_KEY")),
        ("GROQ_API_KEY", env("GROQ_API_KEY")),
        ("ANTHROPIC_API_KEY", env("ANTHROPIC_API_KEY")),
    ];

    let detected_env_keys: Vec<String> = named
        .iter()
        .filter(|(_, v)| v.is_some())
        .map(|(name, _)| name.to_string())
        .collect();

    let has = |name: &str| detected_env_keys.iter().any(|k| k == name);
    let value_of = |name: &str| {
        named
            .iter()
            .find(|(n, _)| *n == name)
            .and_then(|(_, v)| v.clone())
    };

    let detected_key = value_of("GEMINI_API_KEY")
        .or_else(|| value_of("GOOGLE_API_KEY"))
        .or_else(|| value_of("GROQ_API_KEY"))
        .or_else(|| value_of("OPENAI_API_KEY"))
        .unwrap_or_default();

    let detected_provider = if has("GEMINI_API_KEY") || has("GOOGLE_API_KEY") {
        "gemini"
    } else if has("GROQ_API_KEY") {
        "groq"
    } else if has("OPENAI_API_KEY") {
        "openai"
    } else {
        ""
    }
    .to_string();

    CloudKeys {
        detected_env_keys,
        detected_key,
        detected_provider,
    }
}

#[derive(Deserialize)]
struct OllamaModelEntry {
    name: String,
}

#[derive(Deserialize, Default)]
struct OllamaTagsResponse {
    #[serde(default)]
    models: Vec<OllamaModelEntry>,
}

/// Probes the Ollama server on localhost:11434. `available` is only true on a real 200
/// response (ureq itself already turns any 4xx/5xx into an Err), matching the Python's
/// explicit `resp.status_code == 200` check -- a service that's up but erroring doesn't
/// count as available.
pub fn scan_ollama() -> OllamaStatus {
    let cli_installed = which::which("ollama").is_ok();

    let models = ureq::get(format!("{OLLAMA_URL}/api/tags"))
        .config()
        .timeout_global(Some(PROBE_TIMEOUT))
        .build()
        .call()
        .ok()
        .and_then(|resp| resp.into_body().read_json::<OllamaTagsResponse>().ok())
        .map(|data| data.models.into_iter().map(|m| m.name).collect::<Vec<_>>());

    let available = models.is_some();
    let models = models.unwrap_or_default();
    let recommended_model = models
        .first()
        .cloned()
        .unwrap_or_else(|| "llama3.2:1b".to_string());

    OllamaStatus {
        available,
        cli_installed,
        endpoint: OLLAMA_URL.to_string(),
        models,
        recommended_model,
    }
}

#[derive(Deserialize)]
struct LmStudioModelEntry {
    id: String,
}

#[derive(Deserialize, Default)]
struct LmStudioModelsResponse {
    #[serde(default)]
    data: Vec<LmStudioModelEntry>,
}

/// Probes the LM Studio server on localhost:1234. Same "only a real 200 counts" rule as
/// scan_ollama.
pub fn scan_lmstudio() -> LmStudioStatus {
    let models = ureq::get(format!("{LMSTUDIO_URL}/v1/models"))
        .config()
        .timeout_global(Some(PROBE_TIMEOUT))
        .build()
        .call()
        .ok()
        .and_then(|resp| resp.into_body().read_json::<LmStudioModelsResponse>().ok())
        .map(|data| data.data.into_iter().map(|m| m.id).collect::<Vec<_>>());

    let available = models.is_some();
    let models = models.unwrap_or_default();
    let recommended_model = models
        .first()
        .cloned()
        .unwrap_or_else(|| "local-model".to_string());

    LmStudioStatus {
        available,
        endpoint: format!("{LMSTUDIO_URL}/v1"),
        models,
        recommended_model,
    }
}

/// Cloud keys + both local providers in one call, same shape as the Python's scan_all.
pub fn scan_all() -> ScanResult {
    let cloud_keys = detect_cloud_keys();
    let ollama = scan_ollama();
    let lmstudio = scan_lmstudio();
    let has_local_provider = ollama.available || lmstudio.available;
    let has_cloud_key = !cloud_keys.detected_key.is_empty();

    ScanResult {
        cloud_keys,
        ollama,
        lmstudio,
        has_local_provider,
        has_cloud_key,
    }
}

#[derive(Serialize)]
struct OllamaPullRequest<'a> {
    name: &'a str,
    stream: bool,
}

/// Requests Ollama to pull a model: HTTP API first (fast/synchronous if the model's
/// already cached, otherwise this will time out before a multi-GB download finishes --
/// same 5s timeout as the Python, deliberately), then falls back to a detached CLI pull
/// that keeps running in the background after this function returns.
pub fn pull_model(model_name: &str) -> PullResult {
    let http_ok = ureq::post(format!("{OLLAMA_URL}/api/pull"))
        .config()
        .timeout_global(Some(Duration::from_secs(5)))
        .build()
        .send_json(&OllamaPullRequest {
            name: model_name,
            stream: false,
        })
        .is_ok();

    if http_ok {
        return PullResult {
            status: PullStatus::Success,
            message: format!("Successfully pulled {model_name}"),
        };
    }

    if which::which("ollama").is_ok() {
        return match Command::new("ollama").args(["pull", model_name]).spawn() {
            Ok(_child) => PullResult {
                status: PullStatus::Started,
                message: format!("Started pulling {model_name} in background via Ollama CLI."),
            },
            Err(e) => PullResult {
                status: PullStatus::Error,
                message: format!("Failed to start pull: {e}"),
            },
        };
    }

    PullResult {
        status: PullStatus::Error,
        message: "Ollama service is not running. Please start Ollama (`ollama serve`) first."
            .to_string(),
    }
}

// std::env::set_var mutates process-global state. This guards every test in the crate
// that either sets these specific vars (model_scanner::tests, below) or depends on
// reading a clean/known environment (llm::tests, since load_config's cloud-key fallback
// reads them too) -- `cargo test`'s default parallel execution would otherwise let those
// race each other. Lives outside `mod tests` (but still test-only) so both modules' test
// code can reach it.
#[cfg(test)]
pub(crate) static CLOUD_ENV_TEST_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_VARS: [&str; 5] = [
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "OPENAI_API_KEY",
        "GROQ_API_KEY",
        "ANTHROPIC_API_KEY",
    ];

    fn with_cloud_env<F: FnOnce()>(set: &[(&str, &str)], f: F) {
        let _guard = CLOUD_ENV_TEST_GUARD.lock().unwrap();
        let saved: Vec<(&str, Option<String>)> = KEY_VARS
            .iter()
            .map(|k| (*k, std::env::var(k).ok()))
            .collect();
        for k in KEY_VARS {
            std::env::remove_var(k);
        }
        for (k, v) in set {
            std::env::set_var(k, v);
        }

        f();

        for (k, v) in saved {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }

    #[test]
    fn no_keys_detected_when_env_clear() {
        with_cloud_env(&[], || {
            let keys = detect_cloud_keys();
            assert!(keys.detected_env_keys.is_empty());
            assert_eq!(keys.detected_key, "");
            assert_eq!(keys.detected_provider, "");
        });
    }

    #[test]
    fn gemini_takes_priority_over_openai() {
        with_cloud_env(
            &[
                ("OPENAI_API_KEY", "sk-openai-1234567890"),
                ("GEMINI_API_KEY", "gm-1234567890"),
            ],
            || {
                let keys = detect_cloud_keys();
                assert_eq!(keys.detected_key, "gm-1234567890");
                assert_eq!(keys.detected_provider, "gemini");
                assert_eq!(keys.detected_env_keys.len(), 2);
            },
        );
    }

    #[test]
    fn anthropic_key_is_visible_but_never_auto_selected() {
        with_cloud_env(&[("ANTHROPIC_API_KEY", "sk-ant-1234567890")], || {
            let keys = detect_cloud_keys();
            assert_eq!(
                keys.detected_env_keys,
                vec!["ANTHROPIC_API_KEY".to_string()]
            );
            assert_eq!(
                keys.detected_key, "",
                "Anthropic should never be auto-selected, matching the Python"
            );
            assert_eq!(keys.detected_provider, "");
        });
    }

    #[test]
    fn groq_used_when_only_groq_and_openai_set() {
        with_cloud_env(
            &[
                ("GROQ_API_KEY", "gsk-1234567890"),
                ("OPENAI_API_KEY", "sk-1234567890"),
            ],
            || {
                let keys = detect_cloud_keys();
                assert_eq!(keys.detected_key, "gsk-1234567890");
                assert_eq!(keys.detected_provider, "groq");
            },
        );
    }

    /// Real end-to-end tests against a locally running Ollama server -- not a mock. Each
    /// skips itself (rather than failing the suite) if Ollama isn't reachable, since other
    /// dev machines / CI won't have it running.
    fn ollama_reachable() -> bool {
        ureq::get(format!("{OLLAMA_URL}/api/tags"))
            .config()
            .timeout_global(Some(Duration::from_millis(500)))
            .build()
            .call()
            .is_ok()
    }

    #[test]
    fn scan_ollama_against_live_server_if_available() {
        if !ollama_reachable() {
            eprintln!("skipping scan_ollama_against_live_server_if_available: no Ollama on :11434");
            return;
        }
        let status = scan_ollama();
        assert!(status.available);
        assert!(
            status.cli_installed,
            "the `ollama` binary should be on PATH if its server is running"
        );
        println!("live Ollama models: {:?}", status.models);
    }

    #[test]
    fn scan_all_reports_local_provider_when_ollama_live() {
        if !ollama_reachable() {
            eprintln!(
                "skipping scan_all_reports_local_provider_when_ollama_live: no Ollama on :11434"
            );
            return;
        }
        let result = scan_all();
        assert!(result.has_local_provider);
        assert!(result.ollama.available);
    }

    #[test]
    fn pull_already_present_model_succeeds_fast() {
        if !ollama_reachable() {
            eprintln!("skipping pull_already_present_model_succeeds_fast: no Ollama on :11434");
            return;
        }
        let status = scan_ollama();
        let Some(existing_model) = status.models.first() else {
            eprintln!("skipping pull_already_present_model_succeeds_fast: no models pulled yet");
            return;
        };
        let result = pull_model(existing_model);
        assert_eq!(
            result.status,
            PullStatus::Success,
            "message was: {}",
            result.message
        );
    }

    #[test]
    fn scan_lmstudio_when_not_running_reports_unavailable() {
        // LM Studio isn't part of this dev environment, so this exercises the "service is
        // down" path for real rather than skipping -- still a genuine assertion, just
        // about absence instead of presence.
        let status = scan_lmstudio();
        assert!(!status.available);
        assert!(status.models.is_empty());
        assert_eq!(status.recommended_model, "local-model");
    }
}
