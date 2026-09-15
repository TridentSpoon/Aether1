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
use std::sync::{LazyLock, OnceLock};

use msedge_tts::tts::client::connect;
use msedge_tts::tts::SpeechConfig;
use serde::Serialize;

pub const DEFAULT_VOICE: &str = "en-US-AriaNeural";
/// How the two named engines are referred to when Aether1 is explaining itself. Piper and
/// msedge-tts are the names in the code; these are the names a person can act on.
pub const LOCAL_NAME: &str = "Piper (the good offline voice)";
pub const CLOUD_NAME: &str = "Microsoft's online voice";
/// Binaries that are Piper, in the order they are tried. The project has renamed its CLI
/// over time and distributions disagree, so all three are worth looking for.
///
/// Unambiguous names first, and `piper` last, because **there is another program called
/// `piper`**: the GTK application that configures gaming mice, which is what
/// `pacman -S piper` installs on Arch and what a search for "piper linux" finds first. It
/// puts a binary called `piper` on the PATH, so a machine that has it and not Piper TTS
/// used to report the good offline voice as installed and then produce silence. Ordering
/// alone does not fix that -- see `is_piper_tts`, which is what actually decides.
const PIPER_BINARIES: &[&str] = &["piper-tts", "piper_tts", "piper"];
/// Where a Piper voice is looked for when no path is configured.
///
/// The `~/.local/share` entries are on both lists on purpose: installer/aether1.iss puts
/// the bundled voice in `%USERPROFILE%\.local\share\piper\voices` on Windows too, so
/// that one location means the same thing everywhere. The `/usr` entries are the ones
/// that do not exist on Windows, and listing them there is worse than useless -- this
/// list is printed verbatim in the "no voice found" message, so an operator staring at
/// it would be sent looking in folders their machine cannot have.
#[cfg(windows)]
const PIPER_VOICE_DIRS: &[&str] = &["~/.local/share/piper/voices", "~/.local/share/piper"];
#[cfg(not(windows))]
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

/// Which engine to use. `Auto` tries Piper, then the cloud, then the OS's own built-in
/// voice, in that order, and only moves to the next one when the previous is either not
/// installed or actually fails -- see generate_speech_with. That third rung is what makes
/// "install Piper" optional rather than mandatory: Piper needs a manual binary + voice
/// install and the cloud engine needs a reachable network, but the OS engine needs neither
/// (SAPI ships with every copy of Windows; espeak-ng is one setup.sh package away on
/// Linux), so Auto always has *something* to speak with, on a machine that has done
/// nothing but run setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Auto,
    Local,
    Cloud,
    Os,
}

impl Engine {
    /// What this engine means once local-only mode has had its say.
    ///
    /// With the mode on, `Auto` stops being "local if installed, cloud otherwise" and
    /// becomes plain `Local`, and an explicit `Cloud` choice is overruled rather than
    /// honoured. Overruling looks rude, but the alternative is a settings row quietly
    /// beating the switch whose entire job is to say nothing leaves this machine -- and
    /// the operator is told, both in Settings and in the error if Piper is missing.
    pub fn resolve(self, local_only: bool) -> Engine {
        if local_only {
            Engine::Local
        } else {
            self
        }
    }

    pub fn from_key(key: &str) -> Engine {
        match key {
            "local" | "piper" => Engine::Local,
            "cloud" | "msedge" | "edge" => Engine::Cloud,
            "os" | "system" | "sapi" | "espeak" => Engine::Os,
            _ => Engine::Auto,
        }
    }
}

/// Name of the OS engine `Os`/`Auto` would actually use, for the settings panel -- distinct
/// strings because the fix for "not installed" differs (nothing to install on Windows).
pub fn os_engine_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "Windows Speech (SAPI)"
    } else {
        "espeak-ng"
    }
}

/// Whether the OS engine is actually available: always true on Windows (SAPI ships with
/// the OS -- there's nothing to check), conditional on Linux where espeak-ng is a package
/// setup.sh installs but an existing checkout may predate.
pub fn os_status() -> Result<(), String> {
    if cfg!(target_os = "windows") {
        return Ok(());
    }
    if which::which("espeak-ng").is_ok() || which::which("espeak").is_ok() {
        Ok(())
    } else {
        Err(
            "espeak-ng is not installed -- re-run ./setup.sh, or install your \
             distribution's espeak-ng package by hand"
                .to_string(),
        )
    }
}

/// Speaks through whatever the OS provides on its own: SAPI via PowerShell on Windows
/// (`System.Speech` has shipped with every edition since Vista, so this needs no install
/// and cannot be missing the way Piper or espeak-ng can), espeak-ng elsewhere. This is the
/// engine of last resort -- lowest audio quality of the three, but the one Auto can always
/// fall back to, which is the whole point of it existing.
#[cfg(target_os = "windows")]
fn synthesize_os(text: &str, output_path: &Path) -> Result<(), String> {
    // Single-quoted PowerShell string: the only character that needs escaping is the
    // quote itself, doubled. Building the whole SpeechSynthesizer pipeline in one
    // -Command string (rather than a temp .ps1 script) keeps this to one process spawn.
    let escaped_text = text.replace('\'', "''");
    let escaped_path = output_path.to_string_lossy().replace('\'', "''");
    let script = format!(
        "Add-Type -AssemblyName System.Speech; \
         $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
         $s.SetOutputToWaveFile('{escaped_path}'); \
         $s.Speak('{escaped_text}'); \
         $s.Dispose();"
    );
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    crate::paths::suppress_console_window(&mut cmd);
    let output = cmd
        .output()
        .map_err(|e| format!("could not start powershell: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "Windows Speech failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    if !output_path.exists() {
        return Err("Windows Speech produced no audio".to_string());
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn synthesize_os(text: &str, output_path: &Path) -> Result<(), String> {
    let binary = which::which("espeak-ng")
        .or_else(|_| which::which("espeak"))
        .map_err(|_| os_status().unwrap_err())?;

    let mut cmd = Command::new(&binary);
    cmd.arg("-w").arg(output_path);
    let output = speak_via_subprocess(cmd, text)?;
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

/// Whether a binary called some variant of "piper" is the speech synthesiser rather than
/// the gaming-mouse configurator of the same name.
///
/// Asked by running `--help` and looking for the one flag Piper TTS cannot do without: it
/// is handed a `.onnx` model with `-m`/`--model`, and nothing about configuring a mouse has
/// any reason to mention that. `--version` would not do -- both programs have one, and both
/// answer it happily.
///
/// A program that cannot be run at all, or that says nothing recognisable, is treated as
/// not-Piper. That is the safe direction: the cost of a wrong "no" is the OS voice speaking
/// instead of the better one, and the cost of a wrong "yes" is silence with a tick beside
/// it, which is the failure this whole module exists to stop.
fn is_piper_tts(binary: &Path) -> bool {
    let mut cmd = Command::new(binary);
    cmd.arg("--help");
    crate::paths::suppress_console_window(&mut cmd);
    let Ok(output) = cmd.output() else {
        return false;
    };
    // Some builds print usage to stderr, some to stdout, so both are read.
    let mut help = String::from_utf8_lossy(&output.stdout).into_owned();
    help.push_str(&String::from_utf8_lossy(&output.stderr));
    let help = help.to_lowercase();
    help.contains("--model") || help.contains("onnx")
}

/// The Piper binary, if one is installed and is actually Piper.
///
/// Cached: this spawns a process to answer, `voice_status` is re-asked every time the
/// wizard is opened or a setting changes, and the answer cannot change without the machine
/// being reinstalled under it. `OnceLock` rather than a field because the callers are
/// scattered and none of them owns a place to keep it.
pub fn piper_binary() -> Option<PathBuf> {
    static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOUND
        .get_or_init(|| {
            PIPER_BINARIES
                .iter()
                .filter_map(|name| crate::paths::find_installed_binary(&[name]))
                .find(|path| is_piper_tts(path))
        })
        .clone()
}

/// Smallest a real Piper voice can be. The smallest published voices are the `x_low`
/// models at a little over 5 MB; anything under a megabyte is a download that stopped
/// early, an HTML error page saved with the wrong name, or a git-lfs pointer file -- all of
/// which exist on disk, satisfy `exists()`, and make Piper fail at the moment of speaking
/// rather than at the moment of checking.
const MIN_VOICE_BYTES: u64 = 1_000_000;

/// Whether a `.onnx` path is a voice Piper can actually load: big enough to be a model, and
/// accompanied by the `.onnx.json` that describes it.
///
/// **The sidecar is the common failure.** A voice on Hugging Face is two separate files
/// with two separate download buttons, the `.json` is a few kilobytes next to a file of
/// tens of megabytes, and taking only the obvious one is the natural mistake. Piper will
/// not start without it. Checking here means "no voice found" is said while the wizard is
/// open and can explain it, instead of the voice looking installed and the companion going
/// quiet the first time it tries to speak.
fn usable_voice(onnx: &Path) -> bool {
    voice_problem(onnx).is_none()
}

/// Why a `.onnx` cannot be used, in words that name the fix, or None when it can be.
///
/// Separate from `usable_voice` so the wizard can say *which* of the two mistakes was made.
/// "No voice found" is the wrong sentence to show someone staring at a voice file they
/// definitely downloaded, and being told the wrong thing is worse than being told nothing:
/// they go and download it again.
fn voice_problem(onnx: &Path) -> Option<String> {
    let name = onnx
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| onnx.display().to_string());
    let Ok(meta) = std::fs::metadata(onnx) else {
        return Some(format!("{name} is not there"));
    };
    if meta.len() < MIN_VOICE_BYTES {
        return Some(format!(
            "{name} is only {} KB, which is too small to be a voice -- the download probably \
             stopped early. Delete it and fetch it again.",
            meta.len() / 1024
        ));
    }
    // `en_GB-alba-medium.onnx` -> `en_GB-alba-medium.onnx.json`: an appended extension, not
    // a replaced one, which is why this is string work rather than `set_extension`.
    if !PathBuf::from(format!("{}.json", onnx.display())).is_file() {
        return Some(format!(
            "{name} is missing the small {name}.json file that has to sit beside it. \
             Download it from the same page as the voice and put it in the same folder.",
        ));
    }
    None
}

/// The nearest thing to a voice on this machine, and what is wrong with it. Used only to
/// explain a failure -- `piper_voice` remains the thing that decides what gets used.
fn nearest_unusable_voice(configured: Option<&str>) -> Option<String> {
    if let Some(path) = configured.filter(|p| !p.trim().is_empty()) {
        return voice_problem(&crate::paths::expand_home(path));
    }
    PIPER_VOICE_DIRS
        .iter()
        .flat_map(|dir| {
            std::fs::read_dir(crate::paths::expand_home(dir))
                .into_iter()
                .flatten()
        })
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "onnx"))
        .find_map(|p| voice_problem(&p))
}

/// The voice model to speak with: the configured path if there is one, otherwise the first
/// usable `.onnx` found in the usual places. Returning None means Piper is installed but
/// has no voice, which is a different problem from Piper not being installed and is
/// reported as such.
///
/// A configured path is checked the same way an auto-detected one is. Pointing the setting
/// at a half-downloaded file is exactly as easy as leaving one in the voices folder, and
/// trusting the setting because someone typed it would only move the silent failure.
pub fn piper_voice(configured: Option<&str>) -> Option<PathBuf> {
    if let Some(path) = configured.filter(|p| !p.trim().is_empty()) {
        let path = crate::paths::expand_home(path);
        return usable_voice(&path).then_some(path);
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
            .filter(|p| usable_voice(p))
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
        match nearest_unusable_voice(configured_voice) {
            // A voice is there and cannot be used: say what is wrong with that one, not that
            // there is nothing, which is the sentence that sends people to download it twice.
            Some(problem) => format!("{} is installed, but {problem}", binary.display()),
            None => format!(
                "{} is installed but no .onnx voice was found (looked in {})",
                binary.display(),
                PIPER_VOICE_DIRS.join(", ")
            ),
        }
    })?;
    Ok((binary, voice))
}

/// Spawns `cmd` with piped stdin/stderr, sends it `text` on stdin, and returns its output.
/// Shared by Piper and the non-Windows OS engine -- both are "pipe text in, wav out"
/// processes with the same pitfall: writing the *entire* stdin before touching stdout/
/// stderr deadlocks if the child writes enough to its (piped, and so pipe-buffer-limited)
/// stderr while this process is still blocked in that stdin write -- neither side can make
/// progress. A long chat reply is exactly the kind of input that pushes a chatty process
/// (or just its own echoed text) over that buffer. Writing stdin from a second thread lets
/// `wait_with_output()` drain stdout/stderr concurrently with the write, the way it already
/// does with the child's exit, instead of only starting to drain after stdin is done.
fn speak_via_subprocess(mut cmd: Command, text: &str) -> Result<std::process::Output, String> {
    let program = cmd.get_program().to_string_lossy().to_string();
    crate::paths::suppress_console_window(&mut cmd);
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start {program}: {e}"))?;

    let mut stdin = child.stdin.take().ok_or("no stdin on the speech process")?;
    let text = text.to_string();
    let writer = std::thread::spawn(move || stdin.write_all(text.as_bytes()));

    let output = child
        .wait_with_output()
        .map_err(|e| format!("could not wait for {program}: {e}"))?;
    // A write failure here (e.g. a broken pipe because the process exited immediately) is
    // already reflected in output.status/stderr -- that's the diagnostic worth keeping.
    let _ = writer.join();
    Ok(output)
}

/// Runs Piper over `text`, writing a wav.
fn synthesize_local(
    binary: &Path,
    voice: &Path,
    text: &str,
    output_path: &Path,
) -> Result<(), String> {
    let mut cmd = Command::new(binary);
    cmd.arg("--model")
        .arg(voice)
        .arg("--output_file")
        .arg(output_path);
    let output = speak_via_subprocess(cmd, text)?;
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

/// rustls needs exactly one process-wide `CryptoProvider` installed before any TLS
/// connection can be built. With both the `ring` and `aws-lc-rs` backends pulled in
/// transitively (ureq wants one, msedge-tts's platform-verifier wants the other), rustls
/// can no longer pick one automatically from crate features and instead panics --
/// `install_default()` makes the choice explicit. `LazyLock` runs this exactly once no
/// matter how many times `synthesize_cloud` is called; the install itself can still race
/// with some other part of the process installing a provider first, so the result is
/// discarded rather than unwrapped -- either way, by the time this returns, a provider is
/// installed.
static CRYPTO_PROVIDER: LazyLock<()> = LazyLock::new(|| {
    let _ = rustls::crypto::ring::default_provider().install_default();
});

fn synthesize_cloud(voice_name: &str, text: &str, output_path: &Path) -> Result<(), String> {
    LazyLock::force(&CRYPTO_PROVIDER);
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
pub fn generate_speech_reporting(
    cache_dir: &Path,
    text: &str,
    engine: Engine,
    voice: Option<&str>,
    local_voice: Option<&str>,
) -> Result<Speech, Vec<Attempt>> {
    let clean_text = sanitize_text(text);
    if clean_text.trim().is_empty() {
        return Err(vec![Attempt {
            engine: "nothing to say",
            ok: false,
            detail: "there was no speakable text left after the formatting was stripped out"
                .to_string(),
        }]);
    }
    if let Err(e) = std::fs::create_dir_all(cache_dir) {
        return Err(vec![Attempt {
            engine: "the audio folder",
            ok: false,
            detail: format!("could not create {}: {e}", cache_dir.display()),
        }]);
    }

    let voice_name = voice.filter(|v| !v.is_empty()).unwrap_or(DEFAULT_VOICE);

    let try_local = || -> Result<PathBuf, String> {
        let (binary, voice_model) = local_status(local_voice)?;
        let key = cache_key(
            &clean_text,
            &format!("piper:{}", voice_model.display()),
            0,
            0,
        );
        let output_path = cache_dir.join(format!("{key}.wav"));
        if matches!(std::fs::metadata(&output_path), Ok(m) if m.len() > 0) {
            return Ok(output_path);
        }
        synthesize_local(&binary, &voice_model, &clean_text, &output_path)?;
        Ok(output_path)
    };

    let try_cloud = || -> Result<PathBuf, String> {
        let key = cache_key(&clean_text, voice_name, DEFAULT_RATE, DEFAULT_PITCH);
        let output_path = cache_dir.join(format!("{key}.mp3"));
        if matches!(std::fs::metadata(&output_path), Ok(m) if m.len() > 0) {
            return Ok(output_path);
        }
        synthesize_cloud(voice_name, &clean_text, &output_path)?;
        Ok(output_path)
    };

    let try_os = || -> Result<PathBuf, String> {
        os_status()?;
        // Not part of the cache key with anything voice-specific -- there is exactly one
        // OS voice as far as this function is concerned (whatever SAPI/espeak-ng defaults
        // to), so "os-native" alone is enough to keep this out of the other engines' cache
        // entries for the same text.
        let key = cache_key(&clean_text, "os-native", 0, 0);
        let output_path = cache_dir.join(format!("{key}.wav"));
        if matches!(std::fs::metadata(&output_path), Ok(m) if m.len() > 0) {
            return Ok(output_path);
        }
        synthesize_os(&clean_text, &output_path)?;
        Ok(output_path)
    };

    // Every attempt is recorded, successful or not, because "it did not speak" is the one
    // report nobody can act on. What the operator needs to see is which engines were tried
    // and what each one said -- "Piper: not installed. Microsoft online voice: timed out.
    // Windows Speech: spoke." is a diagnosis; silence is not.
    let mut attempts: Vec<Attempt> = Vec::new();
    let mut run =
        |name: &'static str, f: &dyn Fn() -> Result<PathBuf, String>| -> Option<PathBuf> {
            match f() {
                Ok(path) => {
                    attempts.push(Attempt {
                        engine: name,
                        ok: true,
                        detail: "spoke".to_string(),
                    });
                    Some(path)
                }
                Err(why) => {
                    attempts.push(Attempt {
                        engine: name,
                        ok: false,
                        detail: why,
                    });
                    None
                }
            }
        };

    let spoken = match engine {
        Engine::Local => run(LOCAL_NAME, &try_local),
        Engine::Cloud => run(CLOUD_NAME, &try_cloud),
        Engine::Os => run(os_engine_name(), &try_os),
        // Auto is the whole point: try each in turn and fall through on failure, not just
        // on "not installed" -- a Piper binary that crashes or a cloud call that times out
        // gets the same treatment as not having them at all, because either way the
        // operator still needs to hear something. This is guaranteed to end in Ok on
        // Windows (SAPI can't be "not installed"); on Linux it still needs espeak-ng,
        // which is the one thing among all three engines setup.sh actually installs by
        // default, but an existing checkout that predates that change could still lack it.
        Engine::Auto => run(LOCAL_NAME, &try_local)
            .or_else(|| run(CLOUD_NAME, &try_cloud))
            .or_else(|| run(os_engine_name(), &try_os)),
    };

    match spoken {
        Some(path) => {
            let engine = attempts
                .last()
                .map(|a| a.engine)
                .unwrap_or_else(os_engine_name);
            Ok(Speech {
                path,
                engine,
                attempts,
            })
        }
        None => Err(attempts),
    }
}

/// What one engine was asked to do and what came back. `detail` is the engine's own words
/// on failure, so it is the thing worth putting in front of somebody whose companion will
/// not talk.
#[derive(Debug, Clone, Serialize)]
pub struct Attempt {
    pub engine: &'static str,
    pub ok: bool,
    pub detail: String,
}

/// A successful synthesis, and the record of how it got there.
#[derive(Debug, Clone)]
pub struct Speech {
    pub path: PathBuf,
    /// The engine that actually produced the audio, in words a person recognises.
    pub engine: &'static str,
    pub attempts: Vec<Attempt>,
}

/// The everyday entry point: the audio, or the last engine's complaint.
///
/// Callers that want to *show* somebody why nothing was said want
/// `generate_speech_reporting` instead -- this one flattens the whole chain down to one
/// string, which is the right shape for a caller that is going to play the audio and the
/// wrong shape for one that is going to explain the silence.
pub fn generate_speech_with(
    cache_dir: &Path,
    text: &str,
    engine: Engine,
    voice: Option<&str>,
    local_voice: Option<&str>,
) -> Result<PathBuf, String> {
    generate_speech_reporting(cache_dir, text, engine, voice, local_voice)
        .map(|speech| speech.path)
        .map_err(|attempts| {
            attempts
                .iter()
                .map(|a| format!("{}: {}", a.engine, a.detail))
                .collect::<Vec<_>>()
                .join("; ")
        })
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
        for dir in PIPER_VOICE_DIRS {
            let plausible = dir.starts_with('~') || !cfg!(windows);
            assert!(plausible, "{dir} cannot exist on Windows");
        }
    }

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
        assert_eq!(Engine::from_key("os"), Engine::Os);
        assert_eq!(Engine::from_key("system"), Engine::Os);
        assert_eq!(Engine::from_key("sapi"), Engine::Os);
        assert_eq!(Engine::from_key("espeak"), Engine::Os);
        assert_eq!(Engine::from_key(""), Engine::Auto);
        assert_eq!(Engine::from_key("something else"), Engine::Auto);
    }

    #[test]
    fn os_engine_is_never_missing_on_windows_and_says_why_when_it_is_elsewhere() {
        if cfg!(target_os = "windows") {
            // SAPI ships with the OS -- os_status() has nothing to check and cannot fail.
            assert!(os_status().is_ok());
            return;
        }
        if which::which("espeak-ng").is_err() && which::which("espeak").is_err() {
            let err = os_status().unwrap_err();
            assert!(err.contains("espeak-ng"), "{err}");
        }
    }

    #[test]
    fn auto_falls_all_the_way_through_to_the_os_engine_when_nothing_else_is_available() {
        // Can't control whether Piper/espeak-ng are actually installed on the machine
        // running this test, or whether the cloud endpoint is reachable -- but Auto must
        // never itself be the reason all three fail when at least the OS engine can speak,
        // which on this OS it either always can (Windows) or can be asserted about
        // directly via os_status() (see the test above).
        if cfg!(target_os = "windows") || os_status().is_ok() {
            let dir = std::env::temp_dir().join(format!("aether1_tts_auto_{}", std::process::id()));
            let result = generate_speech_with(&dir, "hello", Engine::Auto, None, None);
            assert!(result.is_ok(), "{result:?}");
            let _ = std::fs::remove_dir_all(&dir);
        }
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
    fn local_only_mode_overrules_every_engine_choice() {
        // Including an explicit Cloud: a switch that says nothing leaves this machine
        // cannot be outvoted by the dropdown above it.
        for chosen in [Engine::Auto, Engine::Local, Engine::Cloud] {
            assert_eq!(chosen.resolve(true), Engine::Local, "{chosen:?}");
        }
    }

    #[test]
    fn with_the_mode_off_the_engine_choice_stands() {
        for chosen in [Engine::Auto, Engine::Local, Engine::Cloud] {
            assert_eq!(chosen.resolve(false), chosen, "{chosen:?}");
        }
    }

    #[test]
    fn a_configured_voice_that_does_not_exist_is_not_used() {
        assert!(piper_voice(Some("/nonexistent/voice.onnx")).is_none());
    }

    /// Builds a voice directory: `onnx_bytes` of model, and the sidecar only if asked for.
    fn voice_fixture(name: &str, onnx_bytes: usize, with_sidecar: bool) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aether1_voice_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let onnx = dir.join("en_GB-alba-medium.onnx");
        std::fs::write(&onnx, vec![0u8; onnx_bytes]).unwrap();
        if with_sidecar {
            std::fs::write(dir.join("en_GB-alba-medium.onnx.json"), b"{}").unwrap();
        }
        onnx
    }

    /// The failure this exists to stop: two download buttons on the voice page, and only
    /// the obvious one pressed. Piper cannot load the model without the .json beside it, so
    /// a voice that looks present and is missing its sidecar has to read as "no voice"
    /// while the wizard is still open to say so.
    #[test]
    fn a_voice_without_its_json_sidecar_is_not_a_voice() {
        let onnx = voice_fixture("nosidecar", 2_000_000, false);
        assert!(piper_voice(Some(&onnx.display().to_string())).is_none());
        let _ = std::fs::remove_dir_all(onnx.parent().unwrap());
    }

    /// A download that stopped early leaves a real file of the wrong size. So does an error
    /// page saved under the model's name, and so does a git-lfs pointer.
    #[test]
    fn a_half_downloaded_voice_is_not_a_voice() {
        let onnx = voice_fixture("partial", 4_096, true);
        assert!(piper_voice(Some(&onnx.display().to_string())).is_none());
        let _ = std::fs::remove_dir_all(onnx.parent().unwrap());
    }

    #[test]
    fn a_voice_with_both_files_is_used() {
        let onnx = voice_fixture("complete", 2_000_000, true);
        assert_eq!(
            piper_voice(Some(&onnx.display().to_string())),
            Some(onnx.clone())
        );
        let _ = std::fs::remove_dir_all(onnx.parent().unwrap());
    }

    /// The gaming-mouse `piper` does not answer `--help` with anything about models, and
    /// neither does any other program that happens to be on the PATH under that name. A
    /// stand-in is used here because the real mouse app is not installed on a build
    /// machine; what is being tested is that the question is asked of the binary at all
    /// rather than assumed from its name.
    #[test]
    fn a_program_that_knows_nothing_about_models_is_not_piper() {
        let not_piper = which::which("true").or_else(|_| which::which("cmd"));
        if let Ok(path) = not_piper {
            assert!(!is_piper_tts(&path), "{} passed as Piper", path.display());
        }
    }

    /// The point of the whole check: someone looking at a voice file they definitely
    /// downloaded must not be told there is no voice. They would download it again.
    #[test]
    fn the_sidecar_failure_names_the_sidecar() {
        let onnx = voice_fixture("message", 2_000_000, false);
        let problem = voice_problem(&onnx).unwrap();
        assert!(problem.contains(".onnx.json"), "{problem}");
        let _ = std::fs::remove_dir_all(onnx.parent().unwrap());
    }

    #[test]
    fn the_truncated_failure_says_it_is_too_small() {
        let onnx = voice_fixture("message_small", 4_096, true);
        let problem = voice_problem(&onnx).unwrap();
        assert!(problem.contains("too small"), "{problem}");
        let _ = std::fs::remove_dir_all(onnx.parent().unwrap());
    }

    #[test]
    fn a_binary_that_cannot_be_run_is_not_piper() {
        assert!(!is_piper_tts(Path::new("/nonexistent/piper")));
    }

    /// Ordering matters on a machine that has both: the unambiguous names come first so the
    /// mouse app is never even asked unless nothing else answered.
    #[test]
    fn the_unambiguous_names_are_tried_before_the_ambiguous_one() {
        assert_eq!(
            PIPER_BINARIES.last(),
            Some(&"piper"),
            "bare piper must be the last resort"
        );
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
