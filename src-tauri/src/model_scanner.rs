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
    /// Everything found by port, whatever it is -- the list the Settings dropdown is
    /// built from. The two fields above are the same information for the two ports that
    /// predate this, kept because the older UI and the Python port both read them.
    pub local_servers: Vec<LocalServer>,
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
    let local_servers = scan_local_servers();
    let has_local_provider = ollama.available || lmstudio.available || !local_servers.is_empty();
    let has_cloud_key = !cloud_keys.detected_key.is_empty();

    ScanResult {
        cloud_keys,
        ollama,
        lmstudio,
        local_servers,
        has_local_provider,
        has_cloud_key,
    }
}

/// The loopback ports a local model server is likely to be listening on.
///
/// Deliberately a list of *ports*, not of products. Every one of these is somebody's
/// default, but which program answers is neither knowable from a probe nor interesting:
/// what matters is that something on this machine speaks a known API and can name its
/// models. New tools appear constantly and most adopt one of these ports; adding a
/// number here is the whole cost of supporting one.
const LOCAL_PORTS: &[u16] = &[
    11434, // the common native-API default
    1234, 8080, 1337, 8000, 5001, 5000, 4891, 8081,
];

/// How a local server expects to be talked to. Both are already implemented -- this is
/// only which of the two existing paths a discovered server should be driven through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LocalApi {
    /// The OpenAI-compatible shape: `/v1/models`, `/v1/chat/completions`.
    OpenAi,
    /// The native shape: `/api/tags`, `/api/generate`.
    Native,
}

impl LocalApi {
    /// The `llm_provider` setting that drives a server of this kind.
    pub fn provider_key(self) -> &'static str {
        match self {
            LocalApi::OpenAi => "lmstudio",
            LocalApi::Native => "ollama",
        }
    }
}

/// A local model server that answered.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalServer {
    /// Exactly what belongs in the ENDPOINT setting -- including the `/v1` suffix where
    /// the API wants one, so the operator never has to know that detail.
    pub endpoint: String,
    pub port: u16,
    pub api: LocalApi,
    pub provider_key: &'static str,
    /// What it says it can run. May be empty: a server with no model loaded is still
    /// found, and saying so is more useful than pretending it isn't there.
    pub models: Vec<String>,
    /// One line for a dropdown, naming the port rather than guessing at a product.
    pub label: String,
}

fn describe_local(port: u16, models: &[String]) -> String {
    match models.len() {
        0 => format!("Local server on port {port} (no model loaded)"),
        1 => format!("Local server on port {port} -- {}", models[0]),
        n => format!("Local server on port {port} -- {n} models"),
    }
}

/// Asks one port whether it is a model server, OpenAI-compatible shape first.
///
/// The native API is tried second and only as a fallback, because a server that speaks
/// both is then driven through the OpenAI-compatible path -- the one every other tool
/// here also uses.
fn probe_local_port(port: u16) -> Option<LocalServer> {
    let base = format!("http://127.0.0.1:{port}");

    let openai: Option<Vec<String>> = ureq::get(format!("{base}/v1/models"))
        .config()
        .timeout_global(Some(PROBE_TIMEOUT))
        .build()
        .call()
        .ok()
        .and_then(|resp| resp.into_body().read_json::<LmStudioModelsResponse>().ok())
        .map(|data| data.data.into_iter().map(|m| m.id).collect());

    if let Some(models) = openai {
        return Some(LocalServer {
            endpoint: format!("{base}/v1"),
            port,
            api: LocalApi::OpenAi,
            provider_key: LocalApi::OpenAi.provider_key(),
            label: describe_local(port, &models),
            models,
        });
    }

    let native: Option<Vec<String>> = ureq::get(format!("{base}/api/tags"))
        .config()
        .timeout_global(Some(PROBE_TIMEOUT))
        .build()
        .call()
        .ok()
        .and_then(|resp| resp.into_body().read_json::<OllamaTagsResponse>().ok())
        .map(|data| data.models.into_iter().map(|m| m.name).collect());

    native.map(|models| LocalServer {
        endpoint: base,
        port,
        api: LocalApi::Native,
        provider_key: LocalApi::Native.provider_key(),
        label: describe_local(port, &models),
        models,
    })
}

/// Every local model server that answers, in port order.
///
/// Probed in parallel: a closed port on loopback refuses immediately, but a port held by
/// something that accepts the connection and then says nothing costs the full timeout,
/// and nine of those in a row is a scan the operator is sitting and waiting through.
pub fn scan_local_servers() -> Vec<LocalServer> {
    let mut found: Vec<LocalServer> = std::thread::scope(|scope| {
        let handles: Vec<_> = LOCAL_PORTS
            .iter()
            .map(|&port| scope.spawn(move || probe_local_port(port)))
            .collect();
        handles
            .into_iter()
            .filter_map(|h| h.join().ok().flatten())
            .collect()
    });
    found.sort_by_key(|server| server.port);
    found
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

    // A pull that answers inside five seconds did not download several gigabytes -- it is
    // Ollama saying it already has this model. That is the only case this branch can
    // honestly call finished, and it is worth having: re-picking a model you already
    // downloaded should be instant, not another wait.
    if http_ok {
        return PullResult {
            status: PullStatus::Success,
            message: format!("{model_name} is already on this machine and ready to use."),
        };
    }

    if which::which("ollama").is_ok() {
        return match Command::new("ollama").args(["pull", model_name]).spawn() {
            // Started, not finished: nothing here waits for it, and a several-gigabyte
            // download does not fit in any timeout worth holding a thread for. Whoever asked
            // for this finds out it landed by asking the server what models it has -- which
            // is what the setup wizard does, every few seconds, until this one appears.
            Ok(_child) => PullResult {
                status: PullStatus::Started,
                message: format!(
                    "Downloading {model_name}. This takes a few minutes and continues in the \
                     background."
                ),
            },
            Err(e) => PullResult {
                status: PullStatus::Error,
                message: format!("Failed to start pull: {e}"),
            },
        };
    }

    // Nothing answered on the port and there is no CLI to fall back on, which means Ollama
    // is not installed rather than merely stopped. Saying "start it" to someone who has not
    // got it sends them looking for a service that was never there.
    PullResult {
        status: PullStatus::Error,
        message: "Ollama was not found on this machine -- there is nothing listening on \
                  port 11434 and no `ollama` command on PATH. Install it first (on Arch and \
                  CachyOS: `sudo pacman -S ollama`, then `sudo systemctl enable --now \
                  ollama`; elsewhere see ollama.com/download), then run this again."
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
mod local_server_tests {
    use super::*;

    /// A port nobody is listening on is not a server, and refusing it must not take the
    /// full timeout -- the whole scan is something the operator waits through.
    #[test]
    fn a_closed_port_finds_nothing_and_returns_promptly() {
        let started = std::time::Instant::now();
        // 9 is discard; nothing binds it here, and it is not in LOCAL_PORTS either.
        assert_eq!(probe_local_port(9), None);
        assert!(
            started.elapsed() < PROBE_TIMEOUT * 2,
            "a refused connection should not wait out the timeout"
        );
    }

    /// The label is what the operator reads in the dropdown, so it says what was found
    /// and where -- never which product it is guessed to be.
    #[test]
    fn the_label_names_the_port_and_never_a_product() {
        let none = describe_local(8080, &[]);
        assert!(none.contains("port 8080"), "{none}");
        assert!(none.contains("no model loaded"), "{none}");

        let one = describe_local(1234, &["qwen2.5-7b".to_string()]);
        assert!(one.contains("qwen2.5-7b"), "{one}");

        let many = describe_local(11434, &["a".to_string(), "b".to_string(), "c".to_string()]);
        assert!(many.contains("3 models"), "{many}");

        for label in [none, one, many] {
            let lowered = label.to_lowercase();
            for product in ["ollama", "lm studio", "lmstudio", "llama.cpp", "jan"] {
                assert!(!lowered.contains(product), "{label} names a product");
            }
        }
    }

    /// The endpoint a discovered server reports is the one that belongs in the setting,
    /// /v1 suffix and all -- picking a server has to be the whole configuration step.
    #[test]
    fn each_api_shape_maps_to_the_provider_that_drives_it() {
        assert_eq!(LocalApi::OpenAi.provider_key(), "lmstudio");
        assert_eq!(LocalApi::Native.provider_key(), "ollama");
    }

    /// A stub HTTP server that answers one path with one JSON body and nothing else, so a
    /// probe can be pointed at something real. Returns the port it bound.
    fn stub_server(path: &'static str, body: &'static str) -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            // Two connections: the probe tries the OpenAI shape first and the native shape
            // second, so a native-only stub has to survive the first request missing.
            for stream in listener.incoming().take(2) {
                let Ok(mut stream) = stream else { continue };
                let mut buf = [0u8; 1024];
                let read = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..read]).to_string();
                let response = if request.contains(path) {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                } else {
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_string()
                };
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        port
    }

    /// The happy path: a server that answers is found, and the endpoint reported is the one
    /// that belongs in the setting -- `/v1` and all. Without this, every other test here
    /// would still pass if the probe found nothing, ever.
    #[test]
    fn a_server_answering_the_openai_shape_is_found_with_its_models() {
        let port = stub_server("/v1/models", r#"{"data":[{"id":"qwen2.5-7b"}]}"#);
        let found = probe_local_port(port).expect("a server that answers must be found");
        assert_eq!(found.api, LocalApi::OpenAi);
        assert_eq!(found.provider_key, "lmstudio");
        assert_eq!(found.endpoint, format!("http://127.0.0.1:{port}/v1"));
        assert_eq!(found.models, vec!["qwen2.5-7b".to_string()]);
    }

    /// And the native shape, which is what a default Ollama install answers with. The
    /// endpoint carries no `/v1` here, which is exactly the detail the operator should
    /// never have to know.
    #[test]
    fn a_server_answering_the_native_shape_is_found_and_needs_no_v1() {
        let port = stub_server("/api/tags", r#"{"models":[{"name":"llama3.2:1b"}]}"#);
        let found = probe_local_port(port).expect("a native server must be found");
        assert_eq!(found.api, LocalApi::Native);
        assert_eq!(found.provider_key, "ollama");
        assert_eq!(found.endpoint, format!("http://127.0.0.1:{port}"));
        assert_eq!(found.models, vec!["llama3.2:1b".to_string()]);
    }

    /// Whatever is running locally, the scan must not hang: every port is probed at once.
    #[test]
    fn scanning_every_port_costs_about_one_probe() {
        let started = std::time::Instant::now();
        let _ = scan_local_servers();
        assert!(
            started.elapsed() < PROBE_TIMEOUT * 3,
            "the scan took {:?}, which suggests the ports are being probed one at a time",
            started.elapsed()
        );
    }
}

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
        // Deliberately not asserting the CLI is installed: something answering on this
        // port is not evidence of a particular program. It could be a container, a
        // forwarded port, or another tool that speaks the same API -- all of which the
        // scan is meant to accept.
        println!(
            "live server on :11434 -- models {:?}, cli_installed {}",
            status.models, status.cli_installed
        );
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
    fn scan_lmstudio_reports_what_is_actually_there() {
        let status = scan_lmstudio();
        if status.available {
            // Somebody is running a server on :1234 -- which, for anyone working on this
            // project, is the normal case rather than a broken environment. Asserting
            // absence here would fail on exactly the machines this feature is for.
            assert!(status.endpoint.ends_with("/v1"), "{}", status.endpoint);
            assert!(!status.recommended_model.is_empty());
            return;
        }
        assert!(status.models.is_empty());
        assert_eq!(status.recommended_model, "local-model");
    }
}
