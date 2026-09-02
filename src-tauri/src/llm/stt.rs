// Speech recognition, locally.
//
// Audio is captured in the page (getUserMedia -> Web Audio -> a 16 kHz mono WAV assembled
// in JavaScript) and posted here as bytes. That split is deliberate: it keeps microphone
// access in the one place both the Tauri webview and a browser already have it, avoids a
// native audio dependency in the Rust build, and means the recording never leaves this
// machine -- unlike the Web Speech API it replaces, which in most browsers is a cloud
// service wearing a local-looking API.
//
// Transcription shells out to whisper.cpp, which wants exactly what the page produces:
// 16 kHz mono 16-bit WAV.

use std::path::{Path, PathBuf};
use std::process::Command;

/// whisper.cpp's CLI has been renamed more than once; distributions ship at least these.
const WHISPER_BINARIES: &[&str] = &["whisper-cli", "whisper-cpp", "whisper", "main"];
/// Where a model is looked for when none is configured.
const MODEL_DIRS: &[&str] = &[
    "~/.local/share/whisper",
    "~/.cache/whisper",
    "/usr/share/whisper",
    "/usr/local/share/whisper",
];

fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => match std::env::var_os("HOME") {
            Some(home) => PathBuf::from(home).join(rest),
            None => PathBuf::from(path),
        },
        None => PathBuf::from(path),
    }
}

pub fn whisper_binary() -> Option<PathBuf> {
    WHISPER_BINARIES
        .iter()
        .find_map(|name| which::which(name).ok())
}

/// The model file: the configured path, or the first `.bin` in the usual places.
pub fn whisper_model(configured: Option<&str>) -> Option<PathBuf> {
    if let Some(path) = configured.filter(|p| !p.trim().is_empty()) {
        let path = expand_home(path);
        return path.exists().then_some(path);
    }
    for dir in MODEL_DIRS {
        let Ok(entries) = std::fs::read_dir(expand_home(dir)) else {
            continue;
        };
        let mut models: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "bin"))
            .collect();
        models.sort();
        if let Some(first) = models.into_iter().next() {
            return Some(first);
        }
    }
    None
}

/// What local recognition can currently do. "No binary" and "no model" are separated
/// because they are different problems with different fixes.
pub fn local_status(configured_model: Option<&str>) -> Result<(PathBuf, PathBuf), String> {
    let binary = whisper_binary().ok_or_else(|| {
        format!(
            "no local speech recognition found (looked for {})",
            WHISPER_BINARIES.join(", ")
        )
    })?;
    let model = whisper_model(configured_model).ok_or_else(|| {
        format!(
            "{} is installed but no .bin model was found (looked in {})",
            binary.display(),
            MODEL_DIRS.join(", ")
        )
    })?;
    Ok((binary, model))
}

/// whisper.cpp prints timestamped lines like `[00:00:00.000 --> 00:00:02.000]   text`.
/// Strip the timestamps and join what's left; `--no-timestamps` isn't universal across
/// the binary's various names and versions, so parse rather than assume a flag exists.
fn clean_transcript(raw: &str) -> String {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| match (line.starts_with('['), line.find("]  ")) {
            (true, Some(end)) => line[end + 3..].trim(),
            _ => line,
        })
        .filter(|line| !line.is_empty() && !line.starts_with('['))
        // whisper emits these for silence; they are not something to send as a message.
        .filter(|line| !matches!(*line, "(silence)" | "[BLANK_AUDIO]" | "(blank audio)"))
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

/// Transcribes a 16 kHz mono WAV file. Blocking: runs a process that takes as long as the
/// audio takes.
pub fn transcribe(
    wav_path: &Path,
    configured_model: Option<&str>,
    language: Option<&str>,
) -> Result<String, String> {
    let (binary, model) = local_status(configured_model)?;

    let mut command = Command::new(&binary);
    command
        .arg("--model")
        .arg(&model)
        .arg("--file")
        .arg(wav_path)
        .arg("--language")
        .arg(language.unwrap_or("en"));

    let output = command
        .output()
        .map_err(|e| format!("could not run {}: {e}", binary.display()))?;

    if !output.status.success() {
        return Err(format!(
            "{} failed: {}",
            binary.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let transcript = clean_transcript(&String::from_utf8_lossy(&output.stdout));
    if transcript.is_empty() {
        return Err("nothing was said, or nothing could be made out".to_string());
    }
    Ok(transcript)
}

/// Writes uploaded audio to a scratch file for the transcriber to read. Returns the path;
/// the caller removes it when done.
pub fn stage_audio(cache_dir: &Path, bytes: &[u8]) -> Result<PathBuf, String> {
    if bytes.len() < 44 {
        return Err("that is not long enough to be audio".to_string());
    }
    if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("expected a WAV recording".to_string());
    }
    std::fs::create_dir_all(cache_dir)
        .map_err(|e| format!("could not create {}: {e}", cache_dir.display()))?;

    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = cache_dir.join(format!("capture_{millis}.wav"));
    std::fs::write(&path, bytes).map_err(|e| format!("could not write {}: {e}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_stripped_and_lines_joined() {
        let raw = "[00:00:00.000 --> 00:00:02.000]   Open the pod bay doors\n\
                   [00:00:02.000 --> 00:00:04.000]   please\n";
        assert_eq!(clean_transcript(raw), "Open the pod bay doors please");
    }

    #[test]
    fn silence_markers_are_not_transcripts() {
        assert_eq!(clean_transcript("[BLANK_AUDIO]\n"), "");
        assert_eq!(clean_transcript("  \n(silence)\n"), "");
    }

    #[test]
    fn plain_output_passes_through() {
        assert_eq!(clean_transcript(" hello there \n"), "hello there");
    }

    #[test]
    fn only_a_wav_is_accepted() {
        let dir = std::env::temp_dir().join(format!("aether1_stt_{}", std::process::id()));
        assert!(stage_audio(&dir, b"not audio").is_err());
        assert!(stage_audio(&dir, &[0u8; 8]).is_err());

        let mut wav = b"RIFF\0\0\0\0WAVE".to_vec();
        wav.extend_from_slice(&[0u8; 64]);
        let path = stage_audio(&dir, &wav).unwrap();
        assert!(path.exists());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_missing_engine_says_which_thing_is_missing() {
        // Whichever of the two is absent here, the message names it rather than failing
        // with a bare "unavailable".
        if let Err(message) = local_status(Some("/nonexistent/model.bin")) {
            assert!(
                message.contains("no local speech recognition") || message.contains("no .bin model"),
                "{message}"
            );
        }
    }
}
