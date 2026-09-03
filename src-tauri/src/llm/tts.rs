// Speech synthesis, local first.
//
// Two engines behind one function. Piper runs entirely on this machine: a small binary and
// an ONNX voice, no network, nothing leaving the disk. msedge-tts is a client for the
// "MSEdge Read Aloud" WebSocket API -- better voices, but it means the text of everything
// the companion says travels to Microsoft, including anything it read off your disk.
//
// That is why the default is Auto and Auto means "local if it is installed". A companion
// that goes mute when the network drops is not a local companion, and one that quietly
// narrates your files to a third party is not one either. The cloud engine stays because
// its voices are genuinely nicer and that is a trade some operators will want to make --
// deliberately, having been told.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;

use msedge_tts::tts::client::connect;
use msedge_tts::tts::SpeechConfig;

pub const DEFAULT_VOICE: &str = "en-US-AriaNeural";
/// Binaries that are Piper, in the order they are tried. The project has renamed its CLI
/// over time and distributions disagree, so all three are worth looking for.
const PIPER_BINARIES: &[&str] = &["piper", "piper-tts", "piper_tts"];
/// Where a Piper voice is looked for when no path is configured.
const PIPER_VOICE_DIRS: &[&str] = &[
    "~/.local/share/piper/voices",
    "~/.local/share/piper",
    "/usr/share/piper/voices",
    "/usr/local/share/piper-voices",
];
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

/// Which engine to use. `Auto` prefers the local one and falls back to the cloud, which is
/// what makes "install Piper" the whole of the setup story.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Auto,
    Local,
    Cloud,
}

impl Engine {
    pub fn from_key(key: &str) -> Engine {
        match key {
            "local" | "piper" => Engine::Local,
            "cloud" | "msedge" | "edge" => Engine::Cloud,
            _ => Engine::Auto,
        }
    }
}

/// The Piper binary, if one is installed.
pub fn piper_binary() -> Option<PathBuf> {
    PIPER_BINARIES
        .iter()
        .find_map(|name| which::which(name).ok())
}

/// The voice model to speak with: the configured path if there is one, otherwise the first
/// `.onnx` found in the usual places. Returning None means Piper is installed but has no
/// voice, which is a different problem from Piper not being installed and is reported as
/// such.
pub fn piper_voice(configured: Option<&str>) -> Option<PathBuf> {
    if let Some(path) = configured.filter(|p| !p.trim().is_empty()) {
        let path = crate::paths::expand_home(path);
        return path.exists().then_some(path);
    }
    for dir in PIPER_VOICE_DIRS {
        let dir = crate::paths::expand_home(dir);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut voices: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "onnx"))
            .collect();
        voices.sort();
        if let Some(first) = voices.into_iter().next() {
            return Some(first);
        }
    }
    None
}

/// What the local engine can currently do, for the settings panel and for `Auto` to decide
/// with. Distinguishes "no binary" from "no voice" because the fix differs.
pub fn local_status(configured_voice: Option<&str>) -> Result<(PathBuf, PathBuf), String> {
    let binary = piper_binary().ok_or_else(|| {
        format!(
            "no local speech engine found (looked for {})",
            PIPER_BINARIES.join(", ")
        )
    })?;
    let voice = piper_voice(configured_voice).ok_or_else(|| {
        format!(
            "{} is installed but no .onnx voice was found (looked in {})",
            binary.display(),
            PIPER_VOICE_DIRS.join(", ")
        )
    })?;
    Ok((binary, voice))
}

/// Runs Piper over `text`, writing a wav.
fn synthesize_local(
    binary: &Path,
    voice: &Path,
    text: &str,
    output_path: &Path,
) -> Result<(), String> {
    let mut child = Command::new(binary)
        .arg("--model")
        .arg(voice)
        .arg("--output_file")
        .arg(output_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", binary.display()))?;

    child
        .stdin
        .as_mut()
        .ok_or("no stdin on the speech process")?
        .write_all(text.as_bytes())
        .map_err(|e| format!("could not send text to {}: {e}", binary.display()))?;

    let output = child
        .wait_with_output()
        .map_err(|e| format!("could not wait for {}: {e}", binary.display()))?;
    if !output.status.success() {
        return Err(format!(
            "{} failed: {}",
            binary.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    if !output_path.exists() {
        return Err(format!("{} produced no audio", binary.display()));
    }
    Ok(())
}

fn synthesize_cloud(voice_name: &str, text: &str, output_path: &Path) -> Result<(), String> {
    let config = SpeechConfig {
        voice_name: voice_name.to_string(),
        audio_format: "audio-24khz-48kbitrate-mono-mp3".to_string(),
        pitch: DEFAULT_PITCH,
        rate: DEFAULT_RATE,
        volume: 0,
    };

    let mut client = connect().map_err(|e| format!("could not connect to TTS service: {e}"))?;
    let audio = client
        .synthesize(text, &config)
        .map_err(|e| format!("speech synthesis failed: {e}"))?;

    std::fs::write(output_path, &audio.audio_bytes)
        .map_err(|e| format!("could not write {}: {e}", output_path.display()))
}

/// Synthesizes `text` to an audio file under `cache_dir`, returning its path. Reuses a
/// cached file for identical input instead of re-synthesizing. Blocking -- it either runs
/// a process or opens a WebSocket; always call it off the UI thread.
///
/// `engine` chooses; `voice` is the cloud voice name, `local_voice` the path to an ONNX
/// model. The two engines produce different audio for the same text, so the engine is part
/// of the cache key -- otherwise switching engines would keep replaying the old voice.
pub fn generate_speech_with(
    cache_dir: &Path,
    text: &str,
    engine: Engine,
    voice: Option<&str>,
    local_voice: Option<&str>,
) -> Result<PathBuf, String> {
    let clean_text = sanitize_text(text);
    if clean_text.trim().is_empty() {
        return Err("nothing to synthesize after stripping markdown".to_string());
    }
    std::fs::create_dir_all(cache_dir)
        .map_err(|e| format!("could not create {}: {e}", cache_dir.display()))?;

    let voice_name = voice.filter(|v| !v.is_empty()).unwrap_or(DEFAULT_VOICE);
    let local = local_status(local_voice);

    // Auto is the whole point: local when it is there, cloud when it isn't, without the
    // operator having to know which.
    let use_local = match engine {
        Engine::Local => true,
        Engine::Cloud => false,
        Engine::Auto => local.is_ok(),
    };

    if use_local {
        let (binary, voice_model) = local?;
        let key = cache_key(
            &clean_text,
            &format!("piper:{}", voice_model.display()),
            0,
            0,
        );
        let output_path = cache_dir.join(format!("{key}.wav"));
        if let Ok(metadata) = std::fs::metadata(&output_path) {
            if metadata.len() > 0 {
                return Ok(output_path);
            }
        }
        synthesize_local(&binary, &voice_model, &clean_text, &output_path)?;
        return Ok(output_path);
    }

    let key = cache_key(&clean_text, voice_name, DEFAULT_RATE, DEFAULT_PITCH);
    let output_path = cache_dir.join(format!("{key}.mp3"));
    if let Ok(metadata) = std::fs::metadata(&output_path) {
        if metadata.len() > 0 {
            return Ok(output_path);
        }
    }
    synthesize_cloud(voice_name, &clean_text, &output_path)?;
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
    fn engine_keys_map_to_engines_and_default_to_auto() {
        assert_eq!(Engine::from_key("local"), Engine::Local);
        assert_eq!(Engine::from_key("piper"), Engine::Local);
        assert_eq!(Engine::from_key("cloud"), Engine::Cloud);
        assert_eq!(Engine::from_key(""), Engine::Auto);
        assert_eq!(Engine::from_key("something else"), Engine::Auto);
    }

    #[test]
    fn asking_for_local_when_none_is_installed_says_what_is_missing() {
        // Auto would fall back to the cloud here; Local must not pretend, because a
        // silent fallback is exactly how "it works offline" stops being true without
        // anyone noticing.
        let dir = std::env::temp_dir().join(format!("aether1_tts_local_{}", std::process::id()));
        if piper_binary().is_none() {
            let err = generate_speech_with(&dir, "hello", Engine::Local, None, None).unwrap_err();
            assert!(err.contains("no local speech engine"), "{err}");
        }
    }

    #[test]
    fn a_configured_voice_that_does_not_exist_is_not_used() {
        assert!(piper_voice(Some("/nonexistent/voice.onnx")).is_none());
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
        let path = generate_speech_with(&dir, "Testing one two three.", Engine::Cloud, None, None)
            .expect("synthesis against the live service should succeed");
        let metadata = std::fs::metadata(&path).expect("output file should exist");
        assert!(metadata.len() > 0, "synthesized mp3 should not be empty");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
