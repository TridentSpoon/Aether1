//! What a person has to do next to get a model thinking behind the HUD.
//!
//! Aether1 ships with no model. That is the honest default -- it cannot bundle a
//! several-gigabyte download, and it should not quietly reach for a cloud key -- but it
//! leaves a fresh install in "offline standby", answering with canned text out of
//! `Persona::offline_reply`. The HUD looked finished and thought nothing, and the only way
//! out was to already know what Ollama is.
//!
//! This module is the way out. It answers one question -- *where is this machine, and what
//! is the single next thing to do?* -- and it answers it from what is actually on the
//! machine rather than from a wizard's memory of which page it is on. Every field below is
//! derived from a fresh probe, so re-asking after each step is the whole progress
//! mechanism: a stage cannot be skipped past, and cannot be claimed without being true.
//!
//! The presentation lives in the HUD. What lives here is the part worth testing: which
//! stage the machine is in, and which models it has the memory to run.

use serde::{Deserialize, Serialize};

use crate::model_scanner::ScanResult;

/// How far along this machine is. Ordered: each stage is the one before it, solved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    /// Nothing on this machine speaks a model API, and the Ollama command is not installed
    /// either. The next thing to do is install a program.
    NothingInstalled,
    /// The Ollama command exists but nothing answered on its port -- it is installed and not
    /// running. The next thing to do is start it.
    InstalledNotRunning,
    /// A server answered but has no models. The next thing to do is download one.
    RunningNoModel,
    /// A server answered and named at least one model. The next thing to do is select it,
    /// which the HUD can do without any further help from the operator.
    ReadyToSelect,
    /// The settings already name a working provider and model. Nothing to do.
    Configured,
}

impl Stage {
    /// The one-line answer to "where am I?", in the second person, no jargon.
    pub fn headline(self) -> &'static str {
        match self {
            Stage::NothingInstalled => "No AI is installed on this computer yet.",
            Stage::InstalledNotRunning => "Ollama is installed, but it isn't running.",
            Stage::RunningNoModel => "The AI program is running, but has no brain downloaded.",
            Stage::ReadyToSelect => "Everything is ready -- pick a model and start talking.",
            Stage::Configured => "Set up and ready. A model is connected and answering.",
        }
    }

    /// True while there is still something for the operator to do.
    pub fn needs_attention(self) -> bool {
        !matches!(self, Stage::Configured)
    }
}

/// Which family of instructions to show. Decided here rather than in JavaScript so the
/// steps match the machine Aether1 is actually running on, not the machine whose browser
/// happens to be pointed at it.
/// `Deserialize` as well as `Serialize` because a recorded observation carries the OS it was
/// taken on (see doctor.rs): a fault observed on Windows has to be readable on the Linux
/// runner that judges it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Windows,
    Mac,
    Linux,
}

/// The machine asking, for a record that does not say which machine it came from. Only a
/// hand-written or truncated observation reaches this -- every one AETHER1 records names its
/// own OS -- and guessing at the local one is the reading least likely to mislead.
impl Default for Os {
    fn default() -> Os {
        Os::current()
    }
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "windows") {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Mac
        } else {
            Os::Linux
        }
    }
}

/// One thing to do, written so it can be followed without knowing what any of it means.
///
/// `command` is text to paste into a terminal and `url` is a page to open. A step carries
/// at most one of them: a step that offers both is a step that has not decided what it is
/// asking for, and that is exactly where a first-timer stalls.
#[derive(Debug, Clone, Serialize)]
pub struct Step {
    /// The imperative, short enough to be a button's worth of instruction.
    pub title: String,
    /// Why, and what should happen. Full sentences.
    pub detail: String,
    /// A command to copy and paste, when that is what the step is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// A page to open, when that is what the step is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl Step {
    pub(crate) fn say(title: &str, detail: &str) -> Step {
        Step {
            title: title.to_string(),
            detail: detail.to_string(),
            command: None,
            url: None,
        }
    }

    pub(crate) fn run(title: &str, detail: &str, command: &str) -> Step {
        Step {
            command: Some(command.to_string()),
            ..Step::say(title, detail)
        }
    }

    pub(crate) fn open(title: &str, detail: &str, url: &str) -> Step {
        Step {
            url: Some(url.to_string()),
            ..Step::say(title, detail)
        }
    }
}

/// A model this machine has the memory to run, with what it is good for in plain words.
#[derive(Debug, Clone, Serialize)]
pub struct ModelChoice {
    /// What to pass to `ollama pull`, and what goes in the model box.
    pub name: String,
    /// The name a person would recognise.
    pub label: String,
    /// One sentence: what it is like to talk to.
    pub blurb: String,
    /// Roughly how much disk the download takes, for the "will this finish today?" question.
    pub download: String,
    /// Free RAM this wants, in gigabytes. Also what the hub prints as the memory figure
    /// on a model's detail panel, which is why it is on the wire rather than skipped: the
    /// list used to only need it to decide `fits`, and a browser that shows the figure
    /// cannot re-derive it from "about 4.7 GB" (a download size is not a memory need).
    pub needs_gb: f64,
    /// Whether this machine has the memory to run it comfortably.
    ///
    /// The list is long enough now that showing every entry at once is its own kind of
    /// unhelpful, so the HUD folds the ones that do not fit away behind a "show the
    /// bigger ones" line. They are still offered -- someone who knows their hardware
    /// better than a heuristic does gets to pick past it -- just not shouted.
    pub fits: bool,
    /// Video memory this wants for the whole model to sit on the card, in gigabytes.
    pub needs_vram_gb: f64,
    /// Whether a dedicated graphics card on this machine has room for the whole thing.
    ///
    /// Separate from `fits`, and deliberately not part of what gets recommended: a model
    /// over the card's line still runs -- the layers that do not fit are worked out by the
    /// processor -- it is just slower. The recommendation stays a memory decision; this is
    /// the extra fact the hub shows beside it, the same one the coding list already showed.
    ///
    /// Unlike the coding catalogue, the figures here are each model's own and do not climb
    /// monotonically down the list: nothing picks by them, so an entry that is smaller than
    /// the one above it says so rather than being rounded up to keep an ordering nothing
    /// reads.
    pub fits_on_gpu: bool,
    /// The one pre-selected for this machine.
    pub recommended: bool,
}

/// Every model the guided install offers, grouped by the memory a machine needs for it
/// and, inside each group, ending with the one worth recommending there.
///
/// Two rules hold this list together and `catalogue_is_ordered_by_memory` enforces the
/// first of them:
///
/// 1. `needs_gb` never decreases down the list, because `models_for` picks the *last*
///    entry that fits.
/// 2. So the last entry of each memory group is the recommendation for that class of
///    machine. Ordering inside a group is a decision, not a formatting choice.
///
/// The RAM figures are deliberately generous. A model that technically loads in its
/// theoretical minimum and then swaps for forty seconds per sentence is, to the person
/// who followed this wizard, a broken program -- and they will blame Aether1, correctly.
/// Better to recommend something small that answers immediately.
const CATALOGUE: &[(&str, &str, &str, &str, f64, f64)] = &[
    // Runs on almost anything, including an old laptop or a mini PC.
    (
        "qwen2.5:0.5b",
        "Qwen 2.5 (tiny)",
        "The smallest thing here that still holds a conversation. For machines with very little memory.",
        "about 400 MB",
        2.0,
        1.0,
    ),
    (
        "gemma3:1b",
        "Gemma 3 (tiny)",
        "Google's small one. Writes more naturally than its size suggests.",
        "about 815 MB",
        2.0,
        1.5,
    ),
    (
        "qwen2.5:1.5b",
        "Qwen 2.5 (small)",
        "Fast, and noticeably better at following instructions than most models this size.",
        "about 1 GB",
        2.0,
        1.8,
    ),
    (
        "deepseek-r1:1.5b",
        "DeepSeek R1 (small)",
        "Thinks a problem through before answering, so it is slower but better at puzzles and maths.",
        "about 1.1 GB",
        2.0,
        1.8,
    ),
    (
        "llama3.2:1b",
        "Llama 3.2 (small)",
        "Quick and light. Good for chatting and simple questions; it will get hard facts wrong sometimes.",
        "about 1.3 GB",
        2.0,
        1.8,
    ),
    // The ordinary 8 GB desktop or laptop.
    (
        "gemma2:2b",
        "Gemma 2 (medium)",
        "A step up in writing quality for a small step up in size.",
        "about 1.6 GB",
        5.0,
        2.5,
    ),
    (
        "qwen2.5:3b",
        "Qwen 2.5 (medium)",
        "Careful with instructions and good at code for something this small.",
        "about 1.9 GB",
        5.0,
        3.0,
    ),
    (
        "phi4-mini",
        "Phi 4 (mini)",
        "Microsoft's small one. Strong at reasoning and maths for its size.",
        "about 2.5 GB",
        5.0,
        3.5,
    ),
    (
        "gemma3:4b",
        "Gemma 3 (medium)",
        "The best writing of the medium models, and a little slower for it.",
        "about 3.3 GB",
        5.0,
        4.5,
    ),
    (
        "llama3.2:3b",
        "Llama 3.2 (medium)",
        "Handles longer conversations and keeps track of what was said. The best all-rounder at this size.",
        "about 2 GB",
        5.0,
        3.0,
    ),
    // A 16 GB machine.
    (
        "mistral",
        "Mistral (large)",
        "Clearly better answers, and slower. Worth it if this machine has the memory.",
        "about 4.1 GB",
        10.0,
        5.5,
    ),
    (
        "qwen2.5:7b",
        "Qwen 2.5 (large)",
        "Very good at code and at doing exactly what it was asked.",
        "about 4.7 GB",
        10.0,
        6.0,
    ),
    (
        "deepseek-r1:8b",
        "DeepSeek R1 (large)",
        "Works through its reasoning before answering. Slow, and hard to beat on tricky questions.",
        "about 5.2 GB",
        10.0,
        6.5,
    ),
    (
        "llama3.1:8b",
        "Llama 3.1 (large)",
        "The most widely used model on this list, and a dependable all-rounder. The best pick for an ordinary gaming PC.",
        "about 4.9 GB",
        10.0,
        6.5,
    ),
    // A 32 GB workstation.
    (
        "gemma3:12b",
        "Gemma 3 (very large)",
        "Writes about as well as anything you can run at home. Needs a serious machine.",
        "about 8.1 GB",
        20.0,
        10.0,
    ),
    (
        "deepseek-r1:14b",
        "DeepSeek R1 (very large)",
        "The reasoning one, at a size where the reasoning really shows. Takes its time.",
        "about 9 GB",
        20.0,
        11.0,
    ),
    (
        "qwen2.5:14b",
        "Qwen 2.5 (very large)",
        "The best all-round answers you can get without a workstation-class machine.",
        "about 9 GB",
        20.0,
        11.0,
    ),
    // 64 GB and up, or a machine with a lot of video memory.
    (
        "mixtral:8x7b",
        "Mixtral (huge)",
        "Splits the work between several smaller experts, so it answers faster than its size suggests.",
        "about 26 GB",
        48.0,
        30.0,
    ),
    (
        "llama3.3:70b",
        "Llama 3.3 (huge)",
        "As close to a commercial cloud assistant as a home machine gets. Only for very large amounts of memory.",
        "about 43 GB",
        48.0,
        44.0,
    ),
];

/// The models worth offering on a machine with `ram_total_gb` of memory.
///
/// The whole list is always returned -- hiding the big one from someone who knows their
/// machine better than a heuristic does is worse than letting them pick it -- but only
/// models that comfortably fit are marked `recommended`, and the best fitting one is the
/// single pre-selection.
pub fn models_for(ram_total_gb: f64, vram_gb: Option<f64>) -> Vec<ModelChoice> {
    // The OS, the browser and the HUD are already resident, and the figure is total rather
    // than free. Seventy percent is what is realistically spendable on a model -- and the
    // thresholds it is measured against are themselves generous, so this is not the only
    // margin protecting a machine from being promised something that swaps.
    let usable = ram_total_gb * 0.7;
    // The same tenth `code_setup::models_for` holds back, for the same reason: a card is not
    // also running the desktop and the webview, but it is holding the compositor's
    // framebuffers and the HUD's own three.js scene.
    let usable_vram = vram_gb.map(|gb| gb * 0.9);

    let mut choices: Vec<ModelChoice> = CATALOGUE
        .iter()
        .map(
            |(name, label, blurb, download, needs_gb, needs_vram_gb)| ModelChoice {
                name: name.to_string(),
                label: label.to_string(),
                blurb: blurb.to_string(),
                download: download.to_string(),
                needs_gb: *needs_gb,
                fits: *needs_gb <= usable,
                needs_vram_gb: *needs_vram_gb,
                fits_on_gpu: usable_vram.is_some_and(|room| *needs_vram_gb <= room),
                recommended: false,
            },
        )
        .collect();

    // The largest that fits, or the smallest on the list if nothing does -- a machine below
    // the floor still gets a recommendation, because "your computer is unsuitable" is not a
    // next step and the smallest model is genuinely worth a try.
    let best = choices
        .iter()
        .rposition(|choice| choice.needs_gb <= usable)
        .unwrap_or(0);
    choices[best].recommended = true;
    choices
}

/// Everything the HUD needs to draw the setup wizard, from one probe.
#[derive(Debug, Clone, Serialize)]
pub struct Advice {
    pub stage: Stage,
    pub headline: String,
    pub os: Os,
    /// What to do now. Empty once there is nothing to do.
    pub steps: Vec<Step>,
    /// Models to offer, once there is somewhere to put one.
    pub models: Vec<ModelChoice>,
    /// The endpoint a found server is answering on, so the HUD can fill the field in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// The provider key that drives that server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Models that server already has.
    pub installed_models: Vec<String>,
    /// True when a cloud key was found in the environment -- an alternative route, offered
    /// but never taken automatically.
    pub cloud_key_found: bool,
    /// Whether `ollama pull` can be driven from inside the app on this machine. Without the
    /// command there is nothing to drive and the HUD must not show a button that cannot work.
    pub can_install_from_here: bool,
    /// Whether anything is still owed from the operator. Sent rather than left for the HUD
    /// to work out from the stage name, so "is there something to do?" has one answer and
    /// one place that decides it.
    pub needs_attention: bool,
}

/// Reads the machine and says what to do next.
///
/// `configured` is whether the settings already name a non-offline provider *and* a model;
/// it is passed in rather than read here so this stays a pure function of the scan and the
/// two facts, which is what makes the table of cases below testable.
pub fn advise(
    scan: &ScanResult,
    ram_total_gb: f64,
    gpus: &[crate::gpu::Gpu],
    configured: bool,
) -> Advice {
    let os = Os::current();

    // Any server that answered, preferring one that actually has models: a running server
    // with an empty model list is a worse thing to point the HUD at than one with a model,
    // even though both are "up".
    let running = scan
        .local_servers
        .iter()
        .find(|server| !server.models.is_empty())
        .or_else(|| scan.local_servers.first());

    let installed_models: Vec<String> = running.map(|s| s.models.clone()).unwrap_or_default();
    let endpoint = running.map(|s| s.endpoint.clone());
    let provider = running.map(|s| s.provider_key.to_string());

    let stage = if configured && scan.has_local_provider {
        Stage::Configured
    } else if running.is_some() && !installed_models.is_empty() {
        Stage::ReadyToSelect
    } else if running.is_some() {
        Stage::RunningNoModel
    } else if scan.ollama.cli_installed {
        Stage::InstalledNotRunning
    } else {
        Stage::NothingInstalled
    };

    let steps = match stage {
        Stage::NothingInstalled => install_steps(os),
        Stage::InstalledNotRunning => start_steps(os),
        Stage::RunningNoModel => vec![Step::say(
            "Download a brain",
            "Pick one from the list below and press Download. It is a big file, so this takes \
             a few minutes -- you can watch it count up. Nothing is sent anywhere; the model \
             lands on this computer and stays there.",
        )],
        Stage::ReadyToSelect => vec![Step::say(
            "Choose which one answers",
            "Pick a model below and press Finish. You can change it later in Settings, under \
             The Brain.",
        )],
        Stage::Configured => Vec::new(),
    };

    Advice {
        stage,
        needs_attention: stage.needs_attention(),
        headline: stage.headline().to_string(),
        os,
        steps,
        models: models_for(
            ram_total_gb,
            crate::gpu::dedicated(gpus).and_then(|gpu| gpu.vram_gb),
        ),
        endpoint,
        provider,
        installed_models,
        cloud_key_found: scan.has_cloud_key,
        // Ollama's own port is the one `ollama pull` fills, so the in-app download is only
        // offered when that is the server in play. An OpenAI-compatible server on some other
        // port loads its models its own way and this button would lie.
        can_install_from_here: scan.ollama.cli_installed,
    }
}

/// How to install Ollama, per OS.
///
/// Ollama and not something else because it is the only one of these that installs a
/// *service* -- it comes up with the machine and answers on a known port, with no window to
/// leave open and no "start server" button to find again next week. Everything else here
/// is an app you must remember to run.
fn install_steps(os: Os) -> Vec<Step> {
    match os {
        Os::Windows => vec![
            Step::open(
                "Download Ollama",
                "This opens the Ollama website in your browser. Click the big Download button, \
                 then choose Windows. You will get a file called OllamaSetup.exe.",
                "https://ollama.com/download/windows",
            ),
            Step::say(
                "Run the file you downloaded",
                "Double-click OllamaSetup.exe and click through it. There is nothing to choose. \
                 When it finishes, Ollama is running in the background -- you will not see a \
                 window, and that is correct.",
            ),
            Step::say(
                "Come back here and press Check again",
                "Aether1 looks for it on this computer. Once it finds it, this page moves on to \
                 picking a brain by itself.",
            ),
        ],
        Os::Mac => vec![
            Step::open(
                "Download Ollama",
                "This opens the Ollama website. Click Download, then macOS. You will get a file \
                 called Ollama.dmg.",
                "https://ollama.com/download/mac",
            ),
            Step::say(
                "Open it and drag Ollama into Applications",
                "Double-click Ollama.dmg, drag the Ollama icon onto the Applications folder next \
                 to it, then open Ollama from Applications once. macOS will ask whether you are \
                 sure -- say yes. After that it starts on its own every time.",
            ),
            Step::say(
                "Come back here and press Check again",
                "Aether1 looks for it on this computer and moves on by itself once it answers.",
            ),
        ],
        Os::Linux => vec![
            Step::run(
                "Paste this into a terminal",
                "Open a terminal and paste this line, then press Enter. It downloads Ollama and \
                 sets it up to start with the computer. It will ask for your password, because \
                 installing a program does.",
                "curl -fsSL https://ollama.com/install.sh | sh",
            ),
            Step::run(
                "Start it",
                "On most systems the line above already started it. If Aether1 still cannot find \
                 it, paste this one too.",
                "systemctl --user start ollama || sudo systemctl start ollama",
            ),
            Step::say(
                "Come back here and press Check again",
                "Aether1 looks for it on this computer and moves on by itself once it answers.",
            ),
        ],
    }
}

/// Ollama is installed and nothing answered -- how to start it.
fn start_steps(os: Os) -> Vec<Step> {
    match os {
        Os::Windows => vec![Step::say(
            "Start Ollama",
            "Press the Start button, type Ollama, and open it. No window appears -- it runs in \
             the background, which is what it is supposed to do. Then press Check again here.",
        )],
        Os::Mac => vec![Step::say(
            "Start Ollama",
            "Open your Applications folder and double-click Ollama. No window appears; look for \
             its icon in the menu bar at the top of the screen. Then press Check again here.",
        )],
        Os::Linux => vec![
            Step::run(
                "Start the Ollama service",
                "Paste this into a terminal. The second half runs if the first does not apply to \
                 your system, so it is safe to paste the whole line either way.",
                "systemctl --user start ollama || sudo systemctl start ollama",
            ),
            Step::run(
                "Or run it in a terminal window",
                "If your system does not use systemd, this works instead -- but the window has to \
                 stay open for as long as you want Aether1 to think.",
                "ollama serve",
            ),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_scanner::{
        CloudKeys, LmStudioStatus, LocalApi, LocalServer, OllamaStatus, ScanResult,
    };

    /// A scan with nothing on the machine, which each test then bends into its own case.
    fn empty_scan() -> ScanResult {
        ScanResult {
            cloud_keys: CloudKeys {
                detected_env_keys: Vec::new(),
                detected_key: String::new(),
                detected_provider: String::new(),
            },
            ollama: OllamaStatus {
                available: false,
                cli_installed: false,
                endpoint: "http://localhost:11434".to_string(),
                models: Vec::new(),
                recommended_model: "llama3.2:1b".to_string(),
            },
            lmstudio: LmStudioStatus {
                available: false,
                endpoint: "http://localhost:1234/v1".to_string(),
                models: Vec::new(),
                recommended_model: "local-model".to_string(),
            },
            local_servers: Vec::new(),
            has_local_provider: false,
            has_cloud_key: false,
        }
    }

    fn server(port: u16, models: &[&str]) -> LocalServer {
        LocalServer {
            endpoint: format!("http://localhost:{port}"),
            port,
            api: LocalApi::Native,
            provider_key: LocalApi::Native.provider_key(),
            models: models.iter().map(|m| m.to_string()).collect(),
            label: format!("Local server on port {port}"),
        }
    }

    #[test]
    fn a_bare_machine_is_told_to_install_something() {
        let advice = advise(&empty_scan(), 16.0, &[], false);
        assert_eq!(advice.stage, Stage::NothingInstalled);
        assert!(!advice.steps.is_empty());
        assert!(!advice.can_install_from_here);
    }

    /// The distinction the old scanner could not draw, and the one that matters most: having
    /// the program and having it running are different problems with different fixes.
    #[test]
    fn an_installed_but_silent_ollama_is_told_to_start_it() {
        let mut scan = empty_scan();
        scan.ollama.cli_installed = true;
        let advice = advise(&scan, 16.0, &[], false);
        assert_eq!(advice.stage, Stage::InstalledNotRunning);
        assert!(advice.can_install_from_here);
    }

    #[test]
    fn a_running_server_with_no_models_is_told_to_download_one() {
        let mut scan = empty_scan();
        scan.ollama.cli_installed = true;
        scan.local_servers = vec![server(11434, &[])];
        scan.has_local_provider = true;
        let advice = advise(&scan, 16.0, &[], false);
        assert_eq!(advice.stage, Stage::RunningNoModel);
        assert_eq!(advice.endpoint.as_deref(), Some("http://localhost:11434"));
        assert_eq!(advice.provider.as_deref(), Some("ollama"));
    }

    #[test]
    fn a_server_with_a_model_is_ready_to_select() {
        let mut scan = empty_scan();
        scan.local_servers = vec![server(11434, &["llama3.2:1b"])];
        scan.has_local_provider = true;
        let advice = advise(&scan, 16.0, &[], false);
        assert_eq!(advice.stage, Stage::ReadyToSelect);
        assert_eq!(advice.installed_models, vec!["llama3.2:1b"]);
    }

    /// Settings alone are not enough to claim "configured": a saved provider pointing at a
    /// server that is no longer up is exactly the state the wizard exists to catch.
    #[test]
    fn settings_alone_do_not_make_a_machine_configured() {
        let advice = advise(&empty_scan(), 16.0, &[], true);
        assert_eq!(advice.stage, Stage::NothingInstalled);

        let mut scan = empty_scan();
        scan.local_servers = vec![server(11434, &["llama3.2:1b"])];
        scan.has_local_provider = true;
        assert_eq!(advise(&scan, 16.0, &[], true).stage, Stage::Configured);
    }

    /// A server with models is preferred over one without, so the HUD fills its fields in
    /// from the machine that can actually answer.
    #[test]
    fn a_server_with_models_wins_over_an_empty_one() {
        let mut scan = empty_scan();
        scan.local_servers = vec![server(8080, &[]), server(11434, &["qwen2.5:1.5b"])];
        scan.has_local_provider = true;
        let advice = advise(&scan, 16.0, &[], false);
        assert_eq!(advice.endpoint.as_deref(), Some("http://localhost:11434"));
    }

    #[test]
    fn every_stage_but_the_last_has_something_to_do() {
        for stage in [
            Stage::NothingInstalled,
            Stage::InstalledNotRunning,
            Stage::RunningNoModel,
            Stage::ReadyToSelect,
        ] {
            assert!(stage.needs_attention(), "{stage:?} should need attention");
        }
        assert!(!Stage::Configured.needs_attention());
    }

    /// 8 GB is the ordinary desktop, so it is the case worth pinning: it should land on a
    /// 3B model, not be talked down to a 1B one.
    #[test]
    fn an_ordinary_machine_is_recommended_a_middling_model() {
        let models = models_for(8.0, None);
        let pick = models.iter().find(|m| m.recommended).unwrap();
        assert_eq!(pick.name, "llama3.2:3b");
    }

    #[test]
    fn a_small_machine_is_recommended_a_small_model() {
        let models = models_for(4.0, None);
        let pick = models.iter().find(|m| m.recommended).unwrap();
        assert_eq!(pick.name, "llama3.2:1b");
    }

    #[test]
    fn a_big_machine_is_recommended_a_big_model() {
        let models = models_for(64.0, None);
        let pick = models.iter().find(|m| m.recommended).unwrap();
        assert_eq!(pick.name, "qwen2.5:14b");
    }

    /// 16 GB is the ordinary gaming PC, and the class of machine most likely to be running
    /// a HUD with a 3D avatar in it. It should land on an 8B model.
    #[test]
    fn a_gaming_pc_is_recommended_a_large_model() {
        let models = models_for(16.0, None);
        let pick = models.iter().find(|m| m.recommended).unwrap();
        assert_eq!(pick.name, "llama3.1:8b");
    }

    /// A machine below every threshold still gets one pick. "Nothing here will run" is not a
    /// next step, and the smallest model on the list is worth trying anyway.
    #[test]
    fn a_tiny_machine_still_gets_one_recommendation() {
        let models = models_for(1.0, None);
        let picked: Vec<_> = models.iter().filter(|m| m.recommended).collect();
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].name, "qwen2.5:0.5b");
    }

    /// `models_for` picks the *last* entry that fits, which is only the largest fitting one
    /// while the list stays sorted by memory. Adding a model in the wrong place would
    /// silently recommend it to machines that cannot run it, so the order is a test.
    #[test]
    fn catalogue_is_ordered_by_memory() {
        let mut previous = 0.0;
        for (name, _, _, _, needs_gb, _) in CATALOGUE {
            assert!(
                *needs_gb >= previous,
                "{name} needs {needs_gb} GB, less than the entry above it ({previous} GB) -- \
                 the catalogue must not go backwards",
            );
            previous = *needs_gb;
        }
    }

    /// Every model is offered, but the HUD folds away the ones this machine cannot run, so
    /// `fits` has to be honest about which is which.
    #[test]
    fn only_models_the_machine_can_run_are_marked_as_fitting() {
        let models = models_for(8.0, None);
        assert!(models.iter().any(|m| m.fits), "8 GB fits something");
        assert!(
            models.iter().any(|m| !m.fits),
            "8 GB does not fit the big ones"
        );
        // The recommendation is always one of the ones that fit.
        let pick = models.iter().find(|m| m.recommended).unwrap();
        assert!(pick.fits);
        // And nothing below the pick is marked as not fitting: fits is a prefix of the list.
        let last_fitting = models.iter().rposition(|m| m.fits).unwrap();
        assert!(models[..=last_fitting].iter().all(|m| m.fits));
    }

    /// The blurb and the download size are the only things a first-timer reads, and a model
    /// with a name but no explanation is a dead end in the one place that cannot afford one.
    #[test]
    fn every_model_explains_itself() {
        for (name, label, blurb, download, _, _) in CATALOGUE {
            assert!(!label.is_empty(), "{name} has no label");
            assert!(blurb.len() > 30, "{name} has no real blurb");
            assert!(
                download.contains("GB") || download.contains("MB"),
                "{name} has no size"
            );
        }
    }

    /// The video-memory figure is what a model needs to sit entirely on the card, and the
    /// weights are a subset of what the whole thing needs in memory -- so an entry claiming
    /// to want more video memory than memory is a typo, and a silent one: it would simply
    /// never light the "fits on the graphics card" line on a machine where it should.
    ///
    /// Unlike the coding catalogue there is deliberately no ordering rule here. Nothing
    /// picks by this column, so each entry states its own need.
    #[test]
    fn no_model_wants_more_video_memory_than_memory() {
        for (name, _, _, _, needs_gb, needs_vram_gb) in CATALOGUE {
            assert!(
                needs_vram_gb < needs_gb,
                "{name} wants {needs_vram_gb} GB of video memory but only {needs_gb} GB of \
                 memory -- the weights cannot be bigger than the whole model",
            );
        }
    }

    /// A machine with no card and a machine with a card too small are different facts, and
    /// the hub prints a different line for each. Collapsing them would tell someone with a
    /// 16 GB card that nothing fits on it.
    #[test]
    fn the_graphics_card_answer_is_separate_from_the_memory_one() {
        // No card: nothing claims to fit on one, whatever the memory.
        assert!(models_for(64.0, None).iter().all(|m| !m.fits_on_gpu));
        // A big card on a small machine: the card answer says yes where memory says no.
        let models = models_for(4.0, Some(16.0));
        assert!(
            models.iter().any(|m| m.fits_on_gpu && !m.fits),
            "a 16 GB card holds models a 4 GB machine has no memory for",
        );
        // A card too small for anything does not pretend otherwise.
        assert!(models_for(64.0, Some(0.5)).iter().all(|m| !m.fits_on_gpu));
    }

    /// Two entries with the same ollama name would draw two radio buttons that do the same
    /// thing, and one of them would be wrong.
    #[test]
    fn no_model_is_listed_twice() {
        let mut names: Vec<&str> = CATALOGUE.iter().map(|(name, ..)| *name).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "a model is listed twice");
    }

    /// The whole catalogue is always offered, whatever the machine: the list is advice, not
    /// a gate, and someone who knows their hardware may pick past the recommendation.
    #[test]
    fn every_machine_is_offered_the_whole_list() {
        assert_eq!(models_for(2.0, None).len(), CATALOGUE.len());
        assert_eq!(models_for(128.0, None).len(), CATALOGUE.len());
    }

    /// A step that both says "paste this" and "open that" has not decided what it is asking
    /// for, and a first-timer stalls exactly there.
    #[test]
    fn no_step_asks_for_two_things_at_once() {
        for os in [Os::Windows, Os::Mac, Os::Linux] {
            for step in install_steps(os).iter().chain(start_steps(os).iter()) {
                assert!(
                    step.command.is_none() || step.url.is_none(),
                    "{:?} step {:?} carries both a command and a url",
                    os,
                    step.title
                );
                assert!(!step.detail.trim().is_empty());
            }
        }
    }
}
