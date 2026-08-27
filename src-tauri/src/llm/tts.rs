// Rust port of backend/tts_engine.py, using the msedge-tts crate (a client for the same
// "MSEdge Read Aloud" WebSocket API the Python edge-tts library wraps) instead of shelling
// out to Python. Same voices, same rate/pitch defaults, same markdown-stripping and
// hash-based caching behavior -- just no Python process involved.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use msedge_tts::tts::client::connect;
use msedge_tts::tts::SpeechConfig;

pub const DEFAULT_VOICE: &str = "en-US-AriaNeural";
const DEFAULT_RATE: i32 = 5; // matches tts_engine.py's "+5%"
const DEFAULT_PITCH: i32 = 2; // matches tts_engine.py's "+2Hz"

static CODE_BLOCK_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?s)```.*?```").unwrap());
static INLINE_CODE_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"`([^`]+)`").unwrap());
static MD_LINK_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\[([^\]]+)\]\([^)]+\)").unwrap());
static MD_SYMBOLS_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"[#*_~>\u{2501}\u{2500}\u{2550}]").unwrap());
static WHITESPACE_RE: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"\s+").unwrap());

/// Strips markdown syntax and formatting characters so speech sounds natural, matching
/// tts_engine.py's _sanitize_text.
fn sanitize_text(text: &str) -> String {
    let text = CODE_BLOCK_RE.replace_all(text, "code block omitted");
    let text = INLINE_CODE_RE.replace_all(&text, "$1");
    let text = MD_LINK_RE.replace_all(&text, "$1");
    let text = MD_SYMBOLS_RE.replace_all(&text, " ");
    WHITESPACE_RE.replace_all(&text, " ").trim().to_string()
}

fn cache_key(text: &str, voice: &str, rate: i32, pitch: i32) -> String {
    let mut hasher = DefaultHasher::new();
    (text, voice, rate, pitch).hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Synthesizes `text` to an mp3 file under `cache_dir`, returning its path. Reuses a cached
/// file for identical (text, voice, rate, pitch) instead of re-synthesizing. Blocking --
/// opens a real WebSocket to Microsoft's TTS endpoint; always call this off the UI thread.
pub fn generate_speech(
    cache_dir: &Path,
    text: &str,
    voice: Option<&str>,
) -> Result<PathBuf, String> {
    let clean_text = sanitize_text(text);
    if clean_text.trim().is_empty() {
        return Err("nothing to synthesize after stripping markdown".to_string());
    }

    let voice_name = voice.filter(|v| !v.is_empty()).unwrap_or(DEFAULT_VOICE);
    let key = cache_key(&clean_text, voice_name, DEFAULT_RATE, DEFAULT_PITCH);
    std::fs::create_dir_all(cache_dir).map_err(|e| format!("could not create {}: {e}", cache_dir.display()))?;
    let output_path = cache_dir.join(format!("{key}.mp3"));

    if let Ok(metadata) = std::fs::metadata(&output_path) {
        if metadata.len() > 0 {
            return Ok(output_path);
        }
    }

    let config = SpeechConfig {
        voice_name: voice_name.to_string(),
        audio_format: "audio-24khz-48kbitrate-mono-mp3".to_string(),
        pitch: DEFAULT_PITCH,
        rate: DEFAULT_RATE,
        volume: 0,
    };

    let mut client = connect().map_err(|e| format!("could not connect to TTS service: {e}"))?;
    let audio = client
        .synthesize(&clean_text, &config)
        .map_err(|e| format!("speech synthesis failed: {e}"))?;

    std::fs::write(&output_path, &audio.audio_bytes)
        .map_err(|e| format!("could not write {}: {e}", output_path.display()))?;

    Ok(output_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_markdown() {
        let dirty = "**Status**: `nominal`\n```rust\nfn x() {}\n```\nSee [docs](http://example.com) for more # info";
        let clean = sanitize_text(dirty);
        assert!(!clean.contains('`'));
        assert!(!clean.contains('*'));
        assert!(!clean.contains('#'));
        assert!(clean.contains("nominal"));
        assert!(clean.contains("code block omitted"));
        assert!(clean.contains("docs"));
        assert!(!clean.contains("example.com"));
    }

    #[test]
    fn cache_key_is_deterministic_and_input_sensitive() {
        let a = cache_key("hello", "en-US-AriaNeural", 5, 2);
        let b = cache_key("hello", "en-US-AriaNeural", 5, 2);
        let c = cache_key("hello", "en-US-GuyNeural", 5, 2);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    /// Real end-to-end test against Microsoft's live TTS endpoint -- not a mock. Skips
    /// itself if there's no network path to it, rather than failing the suite on offline
    /// dev machines/CI.
    #[test]
    fn generate_speech_against_live_service_if_reachable() {
        // Normally installed once by main() before any thread touches TLS; the test
        // harness never calls main(), so this test needs to do it itself. Ignore the
        // error rather than unwrap -- if another test in this binary raced us to install
        // it first, that's still a successfully-installed provider, not a failure.
        let _ = rustls::crypto::ring::default_provider().install_default();

        let reachable = msedge_tts::voice::get_voices_list().is_ok();
        if !reachable {
            eprintln!("skipping generate_speech_against_live_service_if_reachable: TTS endpoint unreachable");
            return;
        }

        let dir = std::env::temp_dir().join(format!("aether1_tts_test_{}", std::process::id()));
        let path = generate_speech(&dir, "Testing one two three.", None)
            .expect("synthesis against the live service should succeed");
        let metadata = std::fs::metadata(&path).expect("output file should exist");
        assert!(metadata.len() > 0, "synthesized mp3 should not be empty");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
