// Speech recognition, locally.
//
// Audio is captured in the page (getUserMedia -> Web Audio -> a 16 kHz mono WAV assembled
// in JavaScript) and posted here as bytes. That split is deliberate: it keeps microphone
// access in the one place both the Tauri webview and a browser already have it, avoids a
// native audio dependency in the Rust build, and means the recording never leaves this
// machine -- unlike the Web Speech API it replaces, which in most browsers is a cloud
// service wearing a local-looking API.
//
// Two engines, tried in order, both wanting exactly what the page produces: 16 kHz mono
// 16-bit WAV.
//
//   1. whisper.cpp -- a prebuilt native binary with no Python dependency at all. This is
//      what the fully-offline installer bundles (see scripts/package_offline_*), because
//      it needs nothing else on the machine and never touches the network once installed.
//   2. faster-whisper -- a Python package, no native binary to bundle. This is what the
//      small installer relies on instead: smaller artifact, at the cost of needing Python
//      and a network reachable the first time a given model size is used (it caches itself
//      under ~/.cache/huggingface after that). It goes in a virtual environment Aether1
//      owns rather than the system Python, which current distributions refuse to let pip
//      write to at all -- see `python_binary` and `paths::managed_python_env`.
//
// Whichever is actually installed wins; a machine with both installed uses whisper.cpp,
// since a dedicated native binary needs nothing from Python's own environment to keep
// working.

use std::path::{Path, PathBuf};
use std::process::Command;

/// whisper.cpp's CLI has been renamed more than once; distributions ship at least these.
const WHISPER_BINARIES: &[&str] = &["whisper-cli", "whisper-cpp", "whisper", "main"];
/// Where a model is looked for when none is configured.
///
/// Split by platform for the same reason as PIPER_VOICE_DIRS in tts.rs: this list is
/// printed verbatim when no model is found, and `/usr/share` on Windows is an instruction
/// to go and look somewhere that cannot exist. The `~` entries stay on both lists --
/// installer/aether1.iss writes the bundled model to `%USERPROFILE%\.local\share\whisper`.
#[cfg(windows)]
const MODEL_DIRS: &[&str] = &["~/.local/share/whisper", "~/.cache/whisper"];
#[cfg(not(windows))]
const MODEL_DIRS: &[&str] = &[
    "~/.local/share/whisper",
    "~/.cache/whisper",
    "/usr/share/whisper",
    "/usr/local/share/whisper",
];
/// faster-whisper's model size when none is configured -- matches WHISPER_MODEL in
/// scripts/package_offline_linux.sh / package_offline_windows.ps1, so "small" means the
/// same trade-off (accuracy vs. download size and speed) regardless of which engine ends
/// up running.
const DEFAULT_FASTER_WHISPER_MODEL: &str = "small";
/// Feeds `sys.argv[1..]` (model size, wav path, language) to faster-whisper and prints the
/// transcript, plain, one line, no timestamps -- there is no CLI to shell out to the way
/// there is for whisper.cpp, since pip installs a library. Passed to `python -c` rather
/// than a shipped .py file so there is no extra path to resolve on every platform this
/// runs on; the parameters arrive as real argv entries, not string-interpolated into the
/// script, so nothing here can be broken by a WAV path with spaces or quotes in it.
const FASTER_WHISPER_SCRIPT: &str = "\
import sys
from faster_whisper import WhisperModel
model_size, wav_path, language = sys.argv[1], sys.argv[2], sys.argv[3]
model = WhisperModel(model_size, device='cpu', compute_type='int8')
segments, _ = model.transcribe(wav_path, language=None if language == 'auto' else language)
print(' '.join(segment.text.strip() for segment in segments))
";

pub fn whisper_binary() -> Option<PathBuf> {
    crate::paths::find_installed_binary(WHISPER_BINARIES)
}

/// The model file: the configured path, or the first `.bin` in the usual places.
pub fn whisper_model(configured: Option<&str>) -> Option<PathBuf> {
    if let Some(path) = configured.filter(|p| !p.trim().is_empty()) {
        let path = crate::paths::expand_home(path);
        return path.exists().then_some(path);
    }
    for dir in MODEL_DIRS {
        let Ok(entries) = std::fs::read_dir(crate::paths::expand_home(dir)) else {
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

/// Aether1's own environment first, then `python3`, then `python`.
///
/// The environment comes first because it is the only one `faster-whisper` can reliably be
/// *in*. A system Python on Arch, Debian 12+, Ubuntu 23.04+, Fedora or Homebrew refuses
/// `pip install` outright (PEP 668, `error: externally-managed-environment`), so the route
/// the wizard now gives people is a virtual environment under Aether1's own data directory
/// -- see `paths::managed_python_env`. Nothing puts that interpreter on PATH, which is
/// exactly the point, and also the reason it has to be looked for by name here: without
/// this line the operator follows the instructions, the install succeeds, and Aether1 goes
/// on reporting that it cannot hear them.
///
/// Then `python3` before `python`: on Linux, plenty of distributions no longer symlink the
/// bare name at all, while `python3` is the one guarantee across all of them. On Windows,
/// where a bare `python3` is rare, the fallback picks up whatever winget/python.org
/// installed as plain `python`.
fn python_binary() -> Option<PathBuf> {
    crate::paths::managed_python().or_else(|| {
        which::which("python3")
            .or_else(|_| which::which("python"))
            .ok()
    })
}

/// The command that creates Aether1's Python environment and puts `packages` in it, ready
/// to be shown in the wizard and pasted into a terminal.
///
/// One string with `&&` rather than a list, because it is one thing the operator does and
/// the second half is meaningless without the first. `python3 -m venv` needs no network and
/// no privileges, and writes only inside Aether1's own data directory.
pub fn managed_env_command(packages: &str) -> String {
    let env = crate::paths::managed_python_env()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "~/.local/share/aether1/pyenv".to_string());
    if cfg!(target_os = "windows") {
        format!("python -m venv \"{env}\" && \"{env}\\Scripts\\pip\" install {packages}")
    } else {
        format!("python3 -m venv \"{env}\" && \"{env}/bin/pip\" install {packages}")
    }
}

/// Whether faster-whisper is importable through whichever Python this machine has. Pip
/// installs it as a library, not a CLI, so there is no binary on PATH to look for the way
/// there is for whisper.cpp -- asking the interpreter directly is the only real check.
fn faster_whisper_status() -> Result<PathBuf, String> {
    let python = python_binary().ok_or_else(|| {
        format!(
            "no Python found -- install Python (winget on Windows, your package manager on \
             Linux), then run: {}",
            managed_env_command("faster-whisper")
        )
    })?;
    let mut cmd = Command::new(&python);
    cmd.args(["-c", "import faster_whisper"]);
    crate::paths::suppress_console_window(&mut cmd);
    let ok = cmd.output().map(|o| o.status.success()).unwrap_or(false);
    if ok {
        Ok(python)
    } else {
        // Deliberately not `{python} -m pip install faster-whisper`: on every current
        // distribution that is the line that answers with
        // `error: externally-managed-environment` and sends somebody looking for
        // --break-system-packages. The venv command works on all of them and is the one
        // route this module can then actually find the result of.
        Err(format!(
            "faster-whisper is not installed for {} -- run: {}",
            python.display(),
            managed_env_command("faster-whisper")
        ))
    }
}

/// What local recognition can currently do, and with which engine. "No binary" and "no
/// model" are separated for whisper.cpp because they are different problems with
/// different fixes; faster-whisper has no separate model-file step (see
/// FASTER_WHISPER_SCRIPT), so its `model` half of the pair is the size name rather than a
/// path, purely for the settings panel to have something to show.
pub fn local_status(configured_model: Option<&str>) -> Result<(PathBuf, PathBuf), String> {
    if let Some(binary) = whisper_binary() {
        let model = whisper_model(configured_model).ok_or_else(|| {
            format!(
                "{} is installed but no .bin model was found (looked in {})",
                binary.display(),
                MODEL_DIRS.join(", ")
            )
        })?;
        return Ok((binary, model));
    }
    match faster_whisper_status() {
        Ok(python) => Ok((python, PathBuf::from(DEFAULT_FASTER_WHISPER_MODEL))),
        Err(faster_whisper_why) => Err(format!(
            "no local speech recognition found -- neither whisper.cpp (looked for {}) nor \
             faster-whisper ({faster_whisper_why})",
            WHISPER_BINARIES.join(", ")
        )),
    }
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

fn transcribe_with_whisper_cpp(
    binary: &Path,
    model: &Path,
    wav_path: &Path,
    language: Option<&str>,
) -> Result<String, String> {
    let mut command = Command::new(binary);
    command
        .arg("--model")
        .arg(model)
        .arg("--file")
        .arg(wav_path)
        .arg("--language")
        .arg(language.unwrap_or("en"));
    crate::paths::suppress_console_window(&mut command);

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

fn transcribe_with_faster_whisper(
    python: &Path,
    wav_path: &Path,
    language: Option<&str>,
) -> Result<String, String> {
    let mut command = Command::new(python);
    command
        .arg("-c")
        .arg(FASTER_WHISPER_SCRIPT)
        .arg(DEFAULT_FASTER_WHISPER_MODEL)
        .arg(wav_path)
        .arg(language.unwrap_or("en"));
    crate::paths::suppress_console_window(&mut command);

    let output = command
        .output()
        .map_err(|e| format!("could not run {}: {e}", python.display()))?;

    if !output.status.success() {
        return Err(format!(
            "faster-whisper failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let transcript = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if transcript.is_empty() {
        return Err("nothing was said, or nothing could be made out".to_string());
    }
    Ok(transcript)
}

/// Transcribes a 16 kHz mono WAV file with whichever engine is installed (whisper.cpp
/// preferred; faster-whisper otherwise -- see the module doc comment). Blocking: runs a
/// process that takes as long as the audio takes.
pub fn transcribe(
    wav_path: &Path,
    configured_model: Option<&str>,
    language: Option<&str>,
) -> Result<String, String> {
    if let Some(binary) = whisper_binary() {
        let model = whisper_model(configured_model).ok_or_else(|| {
            format!(
                "{} is installed but no .bin model was found (looked in {})",
                binary.display(),
                MODEL_DIRS.join(", ")
            )
        })?;
        return transcribe_with_whisper_cpp(&binary, &model, wav_path, language);
    }
    let python = faster_whisper_status()?;
    transcribe_with_faster_whisper(&python, wav_path, language)
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

    /// The list is printed verbatim when nothing is found, so every entry has to be a
    /// place this platform could actually put the file. A Unix absolute path on Windows
    /// sends the operator hunting through a folder tree that cannot exist, which is a
    /// worse outcome than saying nothing at all.
    #[test]
    fn every_named_folder_is_one_this_platform_can_have() {
        for dir in MODEL_DIRS {
            let plausible = dir.starts_with('~') || !cfg!(windows);
            assert!(plausible, "{dir} cannot exist on Windows");
        }
    }

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
                message.contains("no local speech recognition")
                    || message.contains("no .bin model"),
                "{message}"
            );
        }
    }

    #[test]
    fn faster_whisper_missing_says_whether_its_python_or_the_package() {
        // Whichever half is missing on this machine (no python at all, or python without
        // the package pip-installed), the message names that specific thing rather than a
        // bare "unavailable" -- same philosophy as tts.rs's equivalent espeak-ng test.
        if let Err(message) = faster_whisper_status() {
            assert!(
                message.contains("no python or python3")
                    || message.contains("faster-whisper is not installed"),
                "{message}"
            );
        }
    }

    #[test]
    fn faster_whisper_script_reads_argv_and_prints_one_line() {
        // Not a real transcription (no model download in a test run) -- just confirms the
        // script this module hands to `python -c` is self-contained Python that reads its
        // three parameters from argv rather than expecting them baked into the source.
        assert!(FASTER_WHISPER_SCRIPT.contains("sys.argv[1], sys.argv[2], sys.argv[3]"));
        assert!(FASTER_WHISPER_SCRIPT.contains("from faster_whisper import WhisperModel"));
        assert_eq!(FASTER_WHISPER_SCRIPT.matches("print(").count(), 1);
    }
}
