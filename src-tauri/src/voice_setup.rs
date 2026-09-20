//! What has to happen before the companion can talk, and before it can listen.
//!
//! `setup.rs` answers the same question for the brain, and this is deliberately its twin:
//! probe the machine, name the single next thing to do, and let the HUD re-ask after every
//! action rather than remembering which page a wizard is on. The reasoning behind that
//! shape is written up there and not repeated here.
//!
//! What is different is the failure mode it exists to fix. A missing brain announces
//! itself -- nothing answers. Voice fails *quietly*: `synthesizeSpeechUrl` caught its own
//! exception, wrote a line to a console nobody has open, and returned null, so the whole
//! report available to the person sitting there was that the companion did not say
//! anything. Everything below exists to turn that into a sentence they can act on.
//!
//! The other half of that is `test_speech`: speaking is not a thing you can settle by
//! reading a status field, because the OS voice can be "installed" and still produce no
//! audio. So the wizard actually says something out loud and reports which engine managed
//! it -- a test with a real result, not a green tick derived from a `which` lookup.

use serde::Serialize;

use crate::llm;
use crate::setup::{Os, Step};

/// How well this machine can speak. Ordered: each stage is the one before it, improved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Speaking {
    /// Speech is produced and never heard. Linux only, and the one stage where every
    /// status field in this module can read "installed" while the machine stays silent --
    /// see `media_playback_step`. Ranked below Silent because it is worse to diagnose:
    /// Silent at least says something is missing.
    Unheard,
    /// Nothing on this machine can produce speech. Only reachable on Linux without
    /// espeak-ng, or in local-only mode with no Piper: Windows and macOS both ship a
    /// voice that cannot be uninstalled.
    Silent,
    /// The operating system's own voice works. It is understandable and it is robotic,
    /// and it is the reason "voice doesn't work" is usually really "voice sounds bad".
    BasicVoice,
    /// Piper is installed with a voice model: offline, and the best of the three.
    GoodVoice,
}

impl Speaking {
    pub fn headline(self) -> &'static str {
        match self {
            Speaking::Unheard => {
                "It can speak, but nothing comes out of the speakers -- a piece of the \
                 audio plumbing is missing."
            }
            Speaking::Silent => "Nothing on this computer can speak yet.",
            Speaking::BasicVoice => "It can speak, using the basic voice built into this computer.",
            Speaking::GoodVoice => "It can speak, using the good offline voice.",
        }
    }

    /// True while there is something worth doing. The basic voice counts: it works, so
    /// this is a suggestion rather than a fault, and the HUD says so in those words.
    pub fn needs_attention(self) -> bool {
        !matches!(self, Speaking::GoodVoice)
    }
}

/// How well this machine can listen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Listening {
    /// No speech recognition is installed. The microphone button will record and then
    /// have nothing to hand the recording to.
    Deaf,
    /// whisper.cpp is installed but has no model file, which is a different problem from
    /// not having whisper at all and has a different fix.
    ModelMissing,
    /// Something can turn a recording into text.
    Ready,
}

impl Listening {
    pub fn headline(self) -> &'static str {
        match self {
            Listening::Deaf => "It cannot hear you yet -- speech recognition isn't installed.",
            Listening::ModelMissing => {
                "Speech recognition is installed but has no language file to work from."
            }
            Listening::Ready => "It can hear you. Press the microphone and talk.",
        }
    }

    pub fn needs_attention(self) -> bool {
        !matches!(self, Listening::Ready)
    }
}

/// One half of the voice: what works now, what it is called, and what to do about it.
#[derive(Debug, Clone, Serialize)]
pub struct Half<S> {
    pub stage: S,
    pub headline: String,
    /// The plain-words expansion: what this means in practice, today, for this person.
    pub detail: String,
    /// What is doing the work right now, named the way a person would recognise it.
    /// Empty when nothing is.
    pub engine: String,
    /// What to do to improve it, or to fix it. Empty when there is nothing worth doing.
    pub steps: Vec<Step>,
    /// Whether this half works at all, for the one-glance tick or cross.
    pub working: bool,
}

/// Everything the HUD needs to draw the voice wizard, from one probe.
#[derive(Debug, Clone, Serialize)]
pub struct VoiceAdvice {
    pub os: Os,
    pub headline: String,
    pub speaking: Half<Speaking>,
    pub listening: Half<Listening>,
    /// Whether replies are spoken at all. Everything below it is moot while this is off,
    /// so it is reported first and the HUD says so before listing anything to install.
    pub auto_speak: bool,
    /// The `tts_engine` setting, as chosen. Not what is actually speaking -- that is
    /// `speaking.engine`, and the two differing is worth seeing.
    pub chosen_engine: String,
    pub local_only: bool,
    pub needs_attention: bool,
}

/// The GStreamer elements the webview needs before any synthesized audio can be heard, and
/// the plugin each one lives in.
///
/// `wavparse` is the one that matters: Piper and espeak-ng both produce WAV, so without it
/// every local engine is inaudible. `avdec_mp3` covers the online voice, which returns MP3.
#[cfg(target_os = "linux")]
const REQUIRED_GST_ELEMENTS: &[(&str, &str)] = &[
    (
        "wavparse",
        "the offline voices (Piper and espeak-ng both produce WAV)",
    ),
    ("avdec_mp3", "the online voice (it returns MP3)"),
];

/// Whether the webview can actually play what the speech engines produce, and what to
/// install when it cannot.
///
/// **This is the failure that looks like nothing at all.** Tauri's Linux webview is
/// WebKitGTK, and WebKitGTK decodes `<audio>` through GStreamer. Distributions package the
/// plugins that do the decoding as *optional* for WebKitGTK -- on Arch, `gst-plugins-good`
/// and `gst-libav` are optdepends of `webkit2gtk-4.1`, so installing the webview does not
/// install them. When they are absent the `<audio>` element reports no error and plays
/// silence, so Piper synthesizes correctly, this module's every status field says
/// "installed", the voice test reports "spoke", and the operator hears nothing. Nothing
/// downstream of synthesis is visible to the rest of this app, which is exactly why it has
/// to be asked about here rather than inferred from a failure that never arrives.
///
/// Asked of `gst-inspect-1.0`, which ships in `gstreamer` itself -- a hard dependency of
/// WebKitGTK, so it is present wherever the webview is. A missing `gst-inspect-1.0` means
/// this machine cannot be asked, which returns None: an unproven warning about audio
/// plumbing sends people to reinstall things that were never the problem, and being told
/// the wrong thing is worse than being told nothing.
#[cfg(target_os = "linux")]
pub fn media_playback_step() -> Option<Step> {
    let probe = |element: &str| -> Option<bool> {
        let mut cmd = std::process::Command::new("gst-inspect-1.0");
        cmd.arg(element);
        crate::paths::suppress_console_window(&mut cmd);
        cmd.output().ok().map(|out| out.status.success())
    };

    // If the first probe cannot run at all, gst-inspect-1.0 is missing and nothing here
    // can be established either way.
    probe(REQUIRED_GST_ELEMENTS[0].0)?;

    let missing: Vec<&(&str, &str)> = REQUIRED_GST_ELEMENTS
        .iter()
        .filter(|(element, _)| probe(element) == Some(false))
        .collect();
    if missing.is_empty() {
        return None;
    }

    let what = missing
        .iter()
        .map(|(element, needed_for)| format!("{element}, which {needed_for}"))
        .collect::<Vec<_>>()
        .join("; and ");

    Some(Step::run(
        "Install the audio decoders",
        &format!(
            "Speech is being produced correctly and this computer cannot play it. The \
             window Aether1 draws itself in plays sound through GStreamer, and the \
             decoders are missing: {what}. Most distributions treat these as optional \
             for the webview, so installing Aether1 did not bring them in. The line below \
             is the Arch one; on Debian, Ubuntu and Mint use: sudo apt install \
             gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-libav. On \
             Fedora: sudo dnf install gstreamer1-plugins-good gstreamer1-plugins-bad-free. \
             Restart Aether1 afterwards.",
        ),
        "sudo pacman -S gst-plugins-good gst-plugins-bad gst-libav",
    ))
}

/// Non-Linux platforms play audio through the OS's own media stack, which is not something
/// that can be missing: nothing to check, so nothing to report.
#[cfg(not(target_os = "linux"))]
pub fn media_playback_step() -> Option<Step> {
    None
}

/// Where to get Piper, per operating system. Piper is two separate things: a program,
/// which has to come from a package manager because it runs, and a voice, which is a pair
/// of data files Aether1 now fetches itself. Only the first half is still a chore, which
/// is why the OS voice exists as a fallback and why these steps are an offer rather than
/// a requirement.
fn piper_steps(os: Os) -> Vec<Step> {
    // The voice is the one half of this Aether1 can do for somebody, and "Pick a better
    // voice" is sitting directly beneath these steps in the same panel. A voice is two
    // files and it does not work with only one of them, which was far and away the most
    // common way the by-hand route went wrong -- so the button is the step now, and the
    // manual route is kept underneath for anyone who wants a voice outside the short list.
    let voices = Step::say(
        "Pick a voice below -- Aether1 fetches it",
        "Under these steps there is a short list of voices with a Download button beside \
         each. Aether1 downloads both of the files a voice is made of, into the folder it \
         already looks in, and shows you a bar while it does. Nothing is installed and \
         nothing is run: a voice file is sound, not a program.",
    );

    let voices_by_hand = Step::open(
        "...or fetch a voice by hand",
        "Only needed for a voice that is not on the list -- another language, say. Open a \
         voice folder and download the .onnx file AND the small .onnx.json sitting next to \
         it, then put both in ~/.local/share/piper/voices. Taking only the big .onnx is the \
         usual reason a voice that looks installed never speaks.",
        "https://huggingface.co/rhasspy/piper-voices/tree/main/en",
    );

    match os {
        Os::Windows => vec![
            Step::run(
                "Install Piper",
                "Piper is maintained as a Python package now, and this is the shortest route \
                 on Windows. Open Command Prompt and paste the line below. If Windows says \
                 there is no python, install it from the Microsoft Store first, then try \
                 again. It goes into a small Python environment belonging to Aether1, which \
                 Aether1 looks in by itself -- that keeps piper.exe out of a Scripts folder \
                 that may or may not be on your PATH, and leaves your own Python alone.",
                &llm::managed_env_command("piper-tts"),
            ),
            Step::open(
                "...or take the old ready-made zip",
                "Only if the line above will not run. This is the last ready-made Windows \
                 build the original project made before it was archived -- it still works, \
                 but it is frozen. Download the file ending in windows_amd64.zip and unzip \
                 it anywhere; your Documents folder is fine.",
                "https://github.com/rhasspy/piper/releases/latest",
            ),
            Step::say(
                "Put it where Aether1 looks",
                "Only for the zip route. Move the unzipped piper folder so that piper.exe \
                 sits inside it, then add that folder to your PATH -- or simply copy \
                 piper.exe next to Aether1's own program. Aether1 looks for a command called \
                 piper, piper-tts or piper_tts.",
            ),
            voices,
            voices_by_hand,
        ],
        Os::Mac => vec![
            Step::run(
                "Install Piper",
                "Homebrew is the shortest route on a Mac. Paste this into Terminal.",
                "brew install piper-tts",
            ),
            voices,
            voices_by_hand,
        ],
        Os::Linux => vec![
            Step::say(
                "Careful: 'piper' is two different programs",
                "Do NOT install the package called plainly 'piper'. On Arch, CachyOS and \
                 several other distributions that name belongs to an app for configuring \
                 gaming mice, which has nothing to do with speech. The one you want is \
                 Piper TTS, and it is usually packaged as piper-tts.",
            ),
            Step::run(
                "Install Piper TTS",
                "On Arch and CachyOS it lives in the AUR, so it needs an AUR helper rather \
                 than pacman -- the line below is the Arch one. On Debian, Ubuntu and Mint, \
                 use: sudo apt install piper-tts. If neither finds it, use the tarball step \
                 below instead, which needs no package manager at all.",
                "yay -S piper-tts-bin",
            ),
            Step::run(
                "...or install it into Aether1's own Python, which works on any distribution",
                "Piper is also a Python package, which is the route that does not depend on \
                 your distribution packaging it. Not `pip install --user`, though: Arch, \
                 Debian, Ubuntu and Fedora all refuse to let pip write to the system \
                 Python at all now (the `externally-managed-environment` error), so this \
                 puts it in a small environment belonging to Aether1 instead. Aether1 \
                 looks in there for the piper program by itself.",
                &llm::managed_env_command("piper-tts"),
            ),
            Step::open(
                "...or take the old ready-made tarball",
                "Last resort, and frozen: this is the final ready-made Linux build the \
                 original project made before it was archived. Download the file ending in \
                 linux_x86_64.tar.gz, unpack it, and copy the piper program inside to \
                 ~/.local/bin (create that folder if it is not there). Aether1 looks there \
                 as well as on your PATH.",
                "https://github.com/rhasspy/piper/releases/latest",
            ),
            Step::say(
                "Where the voices go",
                "Aether1 looks in ~/.local/share/piper/voices, ~/.local/share/piper, \
                 /usr/share/piper/voices and /usr/local/share/piper-voices. Create the first \
                 one if it does not exist -- that is the one that needs no permissions.",
            ),
            voices,
            voices_by_hand,
        ],
    }
}

/// Where to get speech recognition, per operating system.
fn whisper_steps(os: Os) -> Vec<Step> {
    let model = Step::open(
        "Download a language file",
        "This is the part that does the understanding. ggml-base.en.bin is about 140 MB and \
         is accurate enough for talking to a companion; ggml-small.en.bin is better and \
         about three times the size. Put it in a folder called models next to the whisper \
         program, or point Settings at it directly.",
        "https://huggingface.co/ggerganov/whisper.cpp/tree/main",
    );

    // The one command that works on every current distribution, and the reason it is not
    // the obvious `pip install faster-whisper`: see `paths::managed_python_env`. It is
    // written out here rather than described so it can be pasted and be done with.
    let faster_whisper = Step::run(
        "Install the listener into Aether1's own Python",
        "faster-whisper brings its own language files, so this is the only step. It goes \
         into a small Python environment belonging to Aether1 rather than the system one, \
         because Arch, Debian, Ubuntu and Fedora all refuse a plain `pip install` into \
         the system Python now -- that is the `externally-managed-environment` error, and \
         a virtual environment is the answer the error itself recommends. Nothing here \
         needs administrator rights, nothing is added to your PATH, and nothing touches \
         the Python your distribution manages. Aether1 looks in this environment by \
         itself; there is nothing to point it at afterwards.",
        &llm::managed_env_command("faster-whisper"),
    );

    match os {
        Os::Windows => vec![
            faster_whisper,
            Step::say(
                "Or use whisper.cpp instead",
                "If you would rather not install Python, whisper.cpp is a single .exe from \
                 its releases page. Aether1 looks for a command called whisper-cli, \
                 whisper-cpp or whisper.",
            ),
            model,
        ],
        Os::Mac => vec![
            Step::run(
                "Install whisper.cpp",
                "Paste this into Terminal. Homebrew builds it for your Mac's own chip, and \
                 it needs no Python at all.",
                "brew install whisper-cpp",
            ),
            faster_whisper,
            model,
        ],
        Os::Linux => vec![
            Step::say(
                "The quick version",
                "There are two listeners and either one is enough. whisper.cpp is a native \
                 program with no Python anywhere in it, and it is the one Aether1 prefers \
                 when both are present -- but it wants a language file downloaded \
                 separately. faster-whisper is a single command and fetches its own. If \
                 you have no preference, take the faster-whisper step and ignore the rest.",
            ),
            faster_whisper,
            Step::say(
                "Or install whisper.cpp instead",
                "Look for whisper.cpp in your package manager -- on Arch it is in the AUR \
                 (yay -S whisper.cpp), on Homebrew it is whisper-cpp. Aether1 looks for a \
                 command called whisper-cli, whisper-cpp, whisper or main. Then take the \
                 language-file step below, which faster-whisper does not need.",
            ),
            model,
        ],
    }
}

/// Looks at the machine and says where the voice stands, both halves of it.
///
/// `configured_*` come from settings rather than being read here, so this stays a pure
/// function of what it is handed -- the same reason `setup::advise` takes a scan result
/// rather than performing one.
pub fn advise(
    auto_speak: bool,
    chosen_engine: &str,
    local_voice: &str,
    stt_model: &str,
    local_only: bool,
) -> VoiceAdvice {
    let os = Os::current();

    // Speech out. Piper is the good answer, the OS voice is the working answer, and the
    // order matters: an operator with Piper installed should not be shown instructions
    // for installing Piper.
    let piper = llm::tts_local_status(Some(local_voice));
    let os_voice = llm::tts_os_status();

    // Asked before any of the three stages below, because it outranks all of them: a
    // machine that cannot play audio at all is not improved by installing a better voice,
    // and the three stages below would each report success while the operator hears
    // nothing. This is the only check here that looks past synthesis at whether the sound
    // actually arrives.
    let playback = media_playback_step();

    let speaking = match (&playback, &piper, &os_voice) {
        (Some(fix), piper_status, _) => Half {
            stage: Speaking::Unheard,
            headline: Speaking::Unheard.headline().to_string(),
            detail: format!(
                "{} Everything up to the speakers is working{}.",
                fix.detail,
                match piper_status {
                    Ok((_, voice)) => format!(
                        " -- Piper is installed, with the voice at {}",
                        voice.display()
                    ),
                    Err(_) => String::new(),
                }
            ),
            engine: String::new(),
            steps: vec![fix.clone()],
            working: false,
        },
        (None, Ok((_, voice)), _) => Half {
            stage: Speaking::GoodVoice,
            headline: Speaking::GoodVoice.headline().to_string(),
            detail: format!(
                "Piper is installed and using the voice at {}. Nothing you say or hear \
                 leaves this computer.",
                voice.display()
            ),
            engine: llm::TTS_LOCAL_NAME.to_string(),
            steps: Vec::new(),
            working: true,
        },
        (None, Err(why), Ok(())) => Half {
            stage: Speaking::BasicVoice,
            headline: Speaking::BasicVoice.headline().to_string(),
            detail: format!(
                "{} will do the talking, which always works and sounds like a computer \
                 reading aloud. Installing Piper is optional and makes it sound much \
                 better. ({why})",
                llm::tts_os_engine_name()
            ),
            engine: llm::tts_os_engine_name().to_string(),
            steps: piper_steps(os),
            working: true,
        },
        (None, Err(piper_why), Err(os_why)) => Half {
            stage: Speaking::Silent,
            headline: Speaking::Silent.headline().to_string(),
            detail: format!("{os_why} ({piper_why})"),
            engine: String::new(),
            // The OS voice is one package away and Piper is a manual install, so the
            // cheap fix goes first. On Linux this is the only stage where it is missing.
            steps: {
                let mut steps = vec![Step::run(
                    "Install the basic voice",
                    "espeak-ng is small, offline, and the thing Aether1 falls back to when \
                     nothing better is installed. This is the one-line fix.",
                    "sudo pacman -S espeak-ng || sudo apt install espeak-ng",
                )];
                steps.extend(piper_steps(os));
                steps
            },
            working: false,
        },
    };

    // Speech in. There is no OS fallback here: dictation either has an engine or it does
    // not, which is why "it cannot hear you" is a fault rather than a suggestion.
    let listening = match llm::stt_local_status(Some(stt_model)) {
        Ok((binary, model)) => Half {
            stage: Listening::Ready,
            headline: Listening::Ready.headline().to_string(),
            detail: format!(
                "Using {} with {}. Recordings are transcribed on this computer and deleted \
                 straight afterwards.",
                binary.display(),
                model.display()
            ),
            engine: binary
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| binary.display().to_string()),
            steps: Vec::new(),
            working: true,
        },
        Err(why) if why.contains("no .bin model") => Half {
            stage: Listening::ModelMissing,
            headline: Listening::ModelMissing.headline().to_string(),
            detail: why,
            engine: String::new(),
            steps: whisper_steps(os)
                .into_iter()
                // The program is already there; only the language file is missing, and
                // showing install steps for something installed is how a wizard loses
                // somebody's trust.
                .filter(|step| step.title.contains("language file"))
                .collect(),
            working: false,
        },
        Err(why) => Half {
            stage: Listening::Deaf,
            headline: Listening::Deaf.headline().to_string(),
            detail: why,
            engine: String::new(),
            steps: whisper_steps(os),
            working: false,
        },
    };

    // The headline is the thing read first, so it reports the fault that makes the rest
    // moot before it reports anything smaller. Replies not being spoken at all outranks
    // the voice being the robotic one.
    let headline = if !auto_speak {
        "Speaking out loud is switched off.".to_string()
    } else if !speaking.working {
        speaking.headline.clone()
    } else if !listening.working {
        listening.headline.clone()
    } else if speaking.stage == Speaking::BasicVoice {
        speaking.headline.clone()
    } else {
        "Voice is fully set up -- it can talk, and it can hear you.".to_string()
    };

    VoiceAdvice {
        os,
        headline,
        needs_attention: !auto_speak
            || speaking.stage.needs_attention()
            || listening.stage.needs_attention(),
        speaking,
        listening,
        auto_speak,
        chosen_engine: chosen_engine.to_string(),
        local_only,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sentence that matters most: a machine whose only voice is the OS one is
    /// *working*, and must never be told it is broken. That is the difference between
    /// "install this optional thing to sound better" and "your computer cannot speak",
    /// and getting it wrong sends somebody off installing Piper for no reason.
    #[test]
    fn the_basic_voice_counts_as_working() {
        let half = Half {
            stage: Speaking::BasicVoice,
            headline: Speaking::BasicVoice.headline().to_string(),
            detail: String::new(),
            engine: String::new(),
            steps: Vec::new(),
            working: true,
        };
        assert!(half.working);
        assert!(
            half.stage.needs_attention(),
            "there is still something worth offering"
        );
        assert!(
            !half.headline.contains("Nothing"),
            "the basic voice must not be described as nothing"
        );
    }

    #[test]
    fn only_a_finished_voice_stops_asking_for_attention() {
        assert!(Speaking::Silent.needs_attention());
        assert!(Speaking::BasicVoice.needs_attention());
        assert!(!Speaking::GoodVoice.needs_attention());
        assert!(Listening::Deaf.needs_attention());
        assert!(Listening::ModelMissing.needs_attention());
        assert!(!Listening::Ready.needs_attention());
    }

    /// Speaking being switched off makes every other finding irrelevant -- somebody who
    /// installs Piper because the wizard suggested it, and still hears nothing, has been
    /// sent on an errand by their own companion.
    #[test]
    fn speaking_switched_off_is_the_first_thing_reported() {
        let advice = advise(false, "auto", "", "", false);
        assert!(advice.headline.contains("switched off"));
        assert!(advice.needs_attention);
    }

    /// Every step is followable on its own: one thing to do, and never both a command to
    /// paste and a page to open, for the same reason setup.rs pins this.
    #[test]
    fn no_voice_step_asks_for_two_things_at_once() {
        for os in [Os::Windows, Os::Mac, Os::Linux] {
            for step in piper_steps(os).iter().chain(whisper_steps(os).iter()) {
                assert!(
                    step.command.is_none() || step.url.is_none(),
                    "{:?} step {:?} carries both a command and a url",
                    os,
                    step.title
                );
                assert!(
                    !step.detail.is_empty(),
                    "{:?} has no explanation",
                    step.title
                );
            }
        }
    }

    /// **No step may tell somebody to pip-install into the system Python.** This is the
    /// regression this test exists for, not a style rule: on Arch, Debian 12+, Ubuntu
    /// 23.04+, Fedora and Homebrew, `pip install <anything>` into the system interpreter
    /// stops dead with `error: externally-managed-environment` (PEP 668). A wizard step
    /// that cannot succeed is worse than no step -- it reads as the app being broken, and
    /// the only "fix" it leaves within reach is `--break-system-packages`, which is how
    /// somebody's distribution gets quietly damaged by a speech setting.
    ///
    /// A command is allowed to contain `pip install` as long as the pip it names is one
    /// inside Aether1's own environment, which is what `managed_env_command` produces.
    #[test]
    fn no_step_tells_anybody_to_pip_install_into_the_system_python() {
        let managed = crate::paths::managed_python_env()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "pyenv".to_string());
        for os in [Os::Windows, Os::Mac, Os::Linux] {
            for step in piper_steps(os).iter().chain(whisper_steps(os).iter()) {
                let Some(command) = &step.command else {
                    continue;
                };
                assert!(
                    !command.contains("--break-system-packages"),
                    "{:?} step {:?} offers --break-system-packages",
                    os,
                    step.title
                );
                if command.contains("pip install") || command.contains("pip3 install") {
                    assert!(
                        command.contains(&managed) || command.contains("venv"),
                        "{:?} step {:?} pip-installs into the system Python: {command}",
                        os,
                        step.title
                    );
                }
            }
        }
    }

    /// The command handed to the operator has to create the same environment the code
    /// later looks in, or they follow the instructions successfully and Aether1 still
    /// reports the engine missing. Tying both ends to `managed_python_env` here is what
    /// stops the two drifting apart.
    #[test]
    fn the_install_command_points_at_the_environment_aether1_reads() {
        let env = crate::paths::managed_python_env().expect("a home directory in the test env");
        let command = llm::managed_env_command("faster-whisper");
        assert!(
            command.contains(&env.display().to_string()),
            "the install command does not name {}: {command}",
            env.display()
        );
        assert!(
            command.contains("venv"),
            "the install command does not create an environment: {command}"
        );
        assert!(
            command.contains("faster-whisper"),
            "the install command does not install what was asked for: {command}"
        );
    }

    /// Off Linux there is no GStreamer to be missing, and a wizard that invents an audio
    /// fault on Windows or macOS sends people to install packages that do not exist for
    /// their platform.
    #[test]
    #[cfg(not(target_os = "linux"))]
    fn only_linux_is_asked_about_audio_decoders() {
        assert!(media_playback_step().is_none());
    }

    /// On Linux the answer is a fact about this machine and may legitimately be either,
    /// but a reported fault must always arrive with the command that fixes it -- "your
    /// audio plumbing is broken" with nothing to do about it is the sentence this whole
    /// module exists to avoid.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_reported_audio_fault_always_names_its_fix() {
        if let Some(step) = media_playback_step() {
            let command = step.command.expect("an audio fault with no command to run");
            assert!(
                command.contains("gst"),
                "the fix does not name the GStreamer plugins: {command}"
            );
            assert!(
                !step.detail.is_empty(),
                "the fault is reported with no explanation"
            );
        }
    }

    /// Every operating system gets a real route to both halves. An empty list is a dead
    /// end, and a dead end in a wizard is worse than no wizard.
    #[test]
    fn every_os_has_somewhere_to_go() {
        for os in [Os::Windows, Os::Mac, Os::Linux] {
            assert!(!piper_steps(os).is_empty(), "{os:?} has no piper route");
            assert!(!whisper_steps(os).is_empty(), "{os:?} has no whisper route");
        }
    }

    /// The missing-model case must offer the model and not the install, because the
    /// program is already there. Filtering by title is only safe while a step with that
    /// wording exists on every platform.
    #[test]
    fn every_os_offers_a_language_file_step() {
        for os in [Os::Windows, Os::Mac, Os::Linux] {
            assert!(
                whisper_steps(os)
                    .iter()
                    .any(|step| step.title.contains("language file")),
                "{os:?} has no language-file step for the model-missing case"
            );
        }
    }
}
