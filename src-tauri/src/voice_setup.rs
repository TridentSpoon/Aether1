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

/// Where to get Piper, per operating system. Piper has no installer: it is a zip with a
/// binary in it, plus a voice file, and both have to be put somewhere the machine looks.
/// That is a lot to ask, which is exactly why the OS voice exists as a fallback and why
/// these steps are an offer rather than a requirement.
fn piper_steps(os: Os) -> Vec<Step> {
    let voices = Step::open(
        "Download a voice",
        "Voices are separate files. Pick one, download both the .onnx file and the \
         .onnx.json next to it, and put them in the voices folder from the step above. \
         en_GB-alba-medium or en_US-amy-medium are good, ordinary-sounding places to start.",
        "https://huggingface.co/rhasspy/piper-voices/tree/main/en",
    );

    match os {
        Os::Windows => vec![
            Step::open(
                "Download Piper",
                "Open this page and download the file ending in windows_amd64.zip. Unzip it \
                 anywhere you like -- your Documents folder is fine.",
                "https://github.com/rhasspy/piper/releases/latest",
            ),
            Step::say(
                "Put it where Aether1 looks",
                "Move the unzipped piper folder so that piper.exe sits inside it, then add \
                 that folder to your PATH -- or simply copy piper.exe next to Aether1's own \
                 program. Aether1 looks for a command called piper, piper-tts or piper_tts.",
            ),
            voices,
        ],
        Os::Mac => vec![
            Step::run(
                "Install Piper",
                "Homebrew is the shortest route on a Mac. Paste this into Terminal.",
                "brew install piper-tts",
            ),
            voices,
        ],
        Os::Linux => vec![
            Step::run(
                "Install Piper",
                "Most distributions package it. Try your package manager first; if it is not \
                 there, the releases page has a tarball that works anywhere.",
                "sudo pacman -S piper-tts || sudo apt install piper-tts",
            ),
            Step::say(
                "Where the voices go",
                "Aether1 looks in ~/.local/share/piper/voices, ~/.local/share/piper, \
                 /usr/share/piper/voices and /usr/local/share/piper-voices. Create the first \
                 one if it does not exist -- that is the one that needs no permissions.",
            ),
            voices,
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

    match os {
        Os::Windows => vec![
            Step::run(
                "Install the listener",
                "The simplest route on Windows is the Python one. Paste this into Command \
                 Prompt. It brings its own language files, so there is no second step.",
                "pip install faster-whisper",
            ),
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
                "Paste this into Terminal. Homebrew builds it for your Mac's own chip.",
                "brew install whisper-cpp",
            ),
            model,
        ],
        Os::Linux => vec![
            Step::run(
                "Install the listener",
                "Either of these works -- whisper.cpp is faster, faster-whisper is one \
                 command and brings its own language files.",
                "pip install faster-whisper",
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

    let speaking = match (&piper, &os_voice) {
        (Ok((_, voice)), _) => Half {
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
        (Err(why), Ok(())) => Half {
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
        (Err(piper_why), Err(os_why)) => Half {
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
