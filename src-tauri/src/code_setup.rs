//! What has to happen before this machine can write code with nobody's subscription.
//!
//! `setup.rs` answers "where is this machine on the road to having a brain?" and
//! `voice_setup.rs` answers the same question for speech. This is the third of them, and
//! it is deliberately built the same way: probe the machine, name the single next thing to
//! do, and let the HUD re-ask after every action instead of remembering which page a
//! wizard is on. The reasoning for that shape is written up in `setup.rs` and not repeated.
//!
//! What is different is what it is for. The brain wizard exists so the HUD can think. This
//! one exists for the week the cloud subscription lapses -- the operator still has work to
//! do, the machine in front of them can still do it, and the only thing standing in the way
//! is knowing which of several hundred models fits in their RAM and what to type to point a
//! coding agent at it. That is three facts and one incantation, and getting any of them
//! wrong looks identical to the tool being broken.
//!
//! **Aether1 does not become the coding agent.** It sets one up and gets out of the way.
//! Driving an edit-and-run-tests loop over somebody's repository is what `opencode` and
//! `aider` already do well, and a worse copy of them living inside a companion HUD would
//! be a year of work to arrive behind where those two are today. What Aether1 has that
//! they do not is the machine: it already knows how much memory is here, which server is
//! answering and on which port, and which models are downloaded. So it does the part that
//! is actually hard for somebody doing this the first time -- choose, fetch, and hand over
//! a command with the real endpoint and the real model name already in it.
//!
//! The catalogue is sized against **total RAM**, not video memory, because nothing in
//! Aether1 reads VRAM today and inventing a number would be worse than using the one the
//! brain catalogue already trusts. The whole list is always returned with `fits` marking
//! each entry, so somebody who knows their graphics card better than a heuristic does can
//! pick straight past the recommendation.

use serde::Serialize;

use crate::model_scanner::ScanResult;
use crate::setup::{Os, Step};

/// How far along this machine is toward writing code offline. Ordered: each stage is the
/// one before it, solved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    /// No local model server is answering. Coding needs the same server the HUD's brain
    /// needs, so there is no separate install to do here -- the brain wizard is the step.
    NoServer,
    /// A server is answering but none of the models on it are built for code. A general
    /// chat model will write code and will be noticeably worse at it, which reads as "the
    /// agent is stupid" rather than as "the wrong model is loaded".
    NoCodingModel,
    /// A coding model is downloaded and no coding agent is installed to drive it. The
    /// model alone is a chat box; the agent is what reads files and writes them back.
    NoAgent,
    /// A coding model and at least one agent are both here. Nothing left but the command.
    Ready,
}

impl Stage {
    /// The one-line answer to "where am I?", second person, no jargon.
    pub fn headline(self) -> &'static str {
        match self {
            Stage::NoServer => {
                "No AI is running on this computer yet, so there is nothing to write code with."
            }
            Stage::NoCodingModel => {
                "An AI is running, but none of its models were built for writing code."
            }
            Stage::NoAgent => {
                "A coding model is ready. Now you need something that can read and edit your files."
            }
            Stage::Ready => "Ready. Everything is here to write code offline.",
        }
    }

    pub fn needs_attention(self) -> bool {
        !matches!(self, Stage::Ready)
    }
}

/// A model worth writing code with, and what a machine needs to run it.
#[derive(Debug, Clone, Serialize)]
pub struct CodingModel {
    /// What to pass to `ollama pull`, and what goes in the agent's model setting.
    pub name: String,
    /// The name a person would recognise.
    pub label: String,
    /// One sentence: what it is like to work with.
    pub blurb: String,
    /// Roughly how much disk the download takes.
    pub download: String,
    /// Memory this wants, in gigabytes. On the wire because the hub prints it on a
    /// model's detail panel -- a download size is not a memory need, so it cannot be
    /// re-derived from `download` -- and because the sort by size reads it.
    pub needs_gb: f64,
    /// Whether this machine has the memory to run it comfortably.
    pub fits: bool,
    /// Whether it fits on the graphics card, which is the difference between an answer that
    /// arrives while you are still reading the question and one you wait for. `false` on a
    /// machine with no dedicated card, where every model runs on the processor.
    pub fits_on_gpu: bool,
    /// The one pre-selected for this machine.
    pub recommended: bool,
    /// Whether the server already has it, so the HUD offers Use rather than Download.
    pub installed: bool,
    /// Whether this is also big enough to be the model AETHER1 itself runs on.
    ///
    /// Not a second opinion about code. Offline, `Provider::supports_native_tools` is false
    /// for both local providers, so every tool call, every repair and every agent hand-off
    /// goes through the text protocol in the system prompt -- and a model too small to hold
    /// that protocol does not refuse, it answers in prose that parses as nothing. The line
    /// is drawn at 7B because that is where following a format through a long conversation
    /// starts working; below it the protocol breaks in a way that reads as AETHER1 being
    /// broken.
    pub runs_aether1: bool,
}

/// Every model the coding wizard offers, ordered by the memory a machine needs for it.
///
/// The two rules from `setup::CATALOGUE` hold here too, and `catalogue_is_ordered_by_memory`
/// enforces the first: `needs_gb` never decreases down the list, because the picker takes
/// the *last* entry that fits, which makes the last entry of each memory group that group's
/// recommendation. The video-memory column is held to the same rule by the same test, for
/// the same reason -- `models_for` takes the last entry that fits on the card too.
///
/// Every entry is a model trained for code rather than a general model that can also write
/// some. That is the whole point of a separate list: on a 16 GB machine the best chat model
/// and the best coding model are different downloads, and pointing an edit-and-test loop at
/// the chat one produces confident nonsense with the right indentation.
///
/// The memory figures are generous for the reason `setup.rs` gives -- a model that loads in
/// its theoretical minimum and then swaps for forty seconds per edit is, to the person who
/// followed this wizard, a broken program.
/// The video-memory figures are the weights plus a working margin for the context: what it
/// takes for the whole model to sit on the card. A model over the line still runs -- the
/// parts that do not fit are worked out by the processor instead -- it is just slower, which
/// is why `fits_on_gpu` is a separate answer from `fits` rather than a smaller list.
const CATALOGUE: &[(&str, &str, &str, &str, f64, f64, bool)] = &[
    (
        "qwen2.5-coder:1.5b",
        "Qwen 2.5 Coder (tiny)",
        "Finishes lines and writes small functions. Too small to be trusted with a change \
         across several files, but it runs on almost anything.",
        "about 1 GB",
        4.0,
        2.0,
        false,
    ),
    (
        "qwen2.5-coder:3b",
        "Qwen 2.5 Coder (small)",
        "The smallest one here worth letting edit a file on its own. Good for a single \
         function at a time.",
        "about 1.9 GB",
        5.0,
        3.0,
        false,
    ),
    (
        "qwen2.5-coder:7b",
        "Qwen 2.5 Coder (medium)",
        "The first size that holds a whole file in its head and follows a style you describe \
         to it. A sensible floor for real work.",
        "about 4.7 GB",
        10.0,
        6.0,
        true,
    ),
    (
        "qwen2.5-coder:14b",
        "Qwen 2.5 Coder (large)",
        "Clearly better at languages with strict compilers -- Rust and TypeScript both -- and \
         the best pick for an ordinary 32 GB machine.",
        "about 9 GB",
        18.0,
        11.0,
        true,
    ),
    (
        "devstral",
        "Devstral (24B)",
        "Mistral's model, trained for the read-edit-run-the-tests loop rather than for chat. \
         Every parameter works on every word, so it is accurate and it is slow.",
        "about 14 GB",
        26.0,
        16.0,
        true,
    ),
    (
        "qwen3-coder:30b-a3b-q4_K_M",
        "Qwen 3 Coder (30B, Q4_K_M)",
        "Only a fraction of it works on any one word, so it stays usable even when it does \
         not all fit in memory -- which is why it is the recommendation whenever it fits at \
         all. Reads a very long file without losing the thread.",
        "about 19 GB",
        30.0,
        22.0,
        true,
    ),
    (
        "qwen3-coder-next:q4_K_M",
        "Qwen 3 Coder Next (80B, Q4_K_M)",
        "The strongest thing here that still runs on one machine, and it wants a lot of \
         memory to do it. Same trick as the 30B: big on disk, small per word.",
        "about 52 GB",
        56.0,
        50.0,
        true,
    ),
];

/// The models worth offering on a machine with `ram_total_gb` of memory and, when it has a
/// dedicated graphics card, `vram_gb` of video memory, marking which the server already has.
///
/// Like the brain catalogue, the whole list comes back whatever the machine is. Hiding the
/// big one from somebody who knows their hardware better than this heuristic does is worse
/// than letting them pick it and find out.
///
/// **The card decides the recommendation when there is one.** A local model is not really
/// running on the computer, it is running on whichever memory holds its weights, and the
/// difference between the two is not a few percent: a model that sits entirely on the card
/// answers while you are still reading the question, and the same model a gigabyte over the
/// line has part of itself worked out by the processor at a fraction of the speed. Sizing an
/// edit-and-test loop off system memory alone is how somebody with a 4 GB laptop card and
/// plenty of RAM gets told to download a model that will make them give up on the feature.
/// So the card's own ladder picks, and system memory is the fallback for a machine with no
/// dedicated card -- where the models genuinely do run on the processor out of system RAM.
pub fn models_for(
    ram_total_gb: f64,
    vram_gb: Option<f64>,
    installed: &[String],
) -> Vec<CodingModel> {
    // The same seventy percent the brain catalogue spends, and for the same reason: the OS,
    // the webview and the HUD are already resident, and the figure handed in is total
    // rather than free.
    let usable = ram_total_gb * 0.7;
    // A card is not running a desktop, a webview and a browser out of the same pool, so it
    // keeps far more of what it has. What it does lose is the compositor's framebuffers and
    // whatever the HUD's own three.js scene is holding, which is what the tenth is for.
    let usable_vram = vram_gb.map(|gb| gb * 0.9);

    let mut choices: Vec<CodingModel> = CATALOGUE
        .iter()
        .map(
            |(name, label, blurb, download, needs_gb, needs_vram_gb, runs_aether1)| CodingModel {
                installed: has_model(installed, name),
                name: name.to_string(),
                label: label.to_string(),
                blurb: blurb.to_string(),
                download: download.to_string(),
                needs_gb: *needs_gb,
                fits: *needs_gb <= usable,
                fits_on_gpu: usable_vram.is_some_and(|room| *needs_vram_gb <= room),
                recommended: false,
                runs_aether1: *runs_aether1,
            },
        )
        .collect();

    // The largest that fits on the card. A card too small for even the first entry is not a
    // card worth sizing against -- an old laptop chip with 1 GB would otherwise veto a
    // machine with 64 GB of system memory -- so that case falls through to the memory rule
    // as if there were no card at all.
    let on_card = choices.iter().rposition(|choice| choice.fits_on_gpu);

    // The largest that fits in memory, or the smallest on the list if nothing does. A
    // machine under the floor still gets a recommendation, because "your computer is
    // unsuitable" is not a next step.
    let best = on_card.unwrap_or_else(|| {
        choices
            .iter()
            .rposition(|choice| choice.needs_gb <= usable)
            .unwrap_or(0)
    });
    choices[best].recommended = true;
    choices
}

/// The sentence under the model list saying what the recommendation was measured against.
///
/// Worth saying out loud because the answer is surprising on exactly the machines where it
/// matters: somebody with 64 GB of system memory and a small card is being offered a small
/// model, and without this line that reads as the wizard failing to notice the 64 GB.
pub fn sized_against(ram_total_gb: f64, gpu: Option<&crate::gpu::Gpu>) -> String {
    match gpu {
        Some(card) if card.vram_gb.is_some() => format!(
            "Sized for {}, so the whole model sits on the card. Anything past that still \
             runs, with the part that does not fit worked out by the processor instead.",
            card.summary()
        ),
        _ => format!(
            "Sized for {ram_total_gb:.0} GB of system memory. No dedicated graphics card was \
             found, so these run on the processor."
        ),
    }
}

/// Whether a server's model list contains `name`.
///
/// Two things that look like the same comparison and are not:
///
/// - A catalogue entry written without a tag was pulled as `devstral` and is reported back
///   as `devstral:latest`. An equality test misses it and tells somebody to download what
///   they already have.
/// - A catalogue entry written *with* a tag names one size. `qwen2.5-coder:7b` on the
///   server does not mean `qwen2.5-coder:14b` is here -- they are four gigabytes of
///   download apart. Matching on the family, which is the obvious way to fix the first
///   case, marks every size of a family installed as soon as any one of them is, and the
///   panel then offers to use a model that is not there.
///
/// So the tag decides which comparison applies, and
/// `a_bigger_size_of_the_same_family_is_not_installed` fails loudly if that is ever
/// collapsed back into one.
fn has_model(installed: &[String], name: &str) -> bool {
    installed
        .iter()
        .any(|have| have == name || (!name.contains(':') && have.split(':').next() == Some(name)))
}

/// Whether any model on this server was built for code.
///
/// Matched on the name because that is all a server reports. A false negative costs an
/// unnecessary download and a false positive costs somebody a day wondering why the agent
/// keeps inventing functions, so the list is the families this catalogue knows plus the two
/// words every coding model in the wild has in its name.
fn is_coding_model(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "coder",
        "-code",
        "codestral",
        "devstral",
        "codellama",
        "codegemma",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

/// A program that can read the operator's files, change them, and run their tests.
#[derive(Debug, Clone, Serialize)]
pub struct AgentChoice {
    /// Stable key for the HUD; also the command's own name.
    pub key: String,
    pub label: String,
    /// One sentence: what using it is like, and what it is best at.
    pub blurb: String,
    /// Whether the command is already on this machine.
    pub installed: bool,
    /// The one pre-selected, when neither is installed.
    pub recommended: bool,
    /// How to get it.
    pub install: Vec<Step>,
    /// How to point it at *this* machine's server and model. Built with the real endpoint
    /// and the real model name in them, which is the whole reason this is worth Aether1
    /// doing rather than a link to somebody's documentation.
    pub connect: Vec<Step>,
}

/// How big a context window to ask the server for, given the machine's memory.
///
/// This matters more than it looks. Ollama's default context is small, and an agent handed
/// a small window does not fail -- it silently forgets the top of the file it was editing
/// and writes something that contradicts it. That is the quiet failure this module exists
/// to prevent, and it is worth a line of configuration in every connect step below.
pub fn context_tokens(ram_total_gb: f64) -> u32 {
    if ram_total_gb >= 48.0 {
        65536
    } else if ram_total_gb >= 24.0 {
        32768
    } else if ram_total_gb >= 12.0 {
        16384
    } else {
        8192
    }
}

/// The size, in billions, below which a model cannot hold AETHER1's text tool protocol
/// through a conversation. See `CodingModel::runs_aether1` for why the line is here.
const TOOL_PROTOCOL_FLOOR_TENTHS: u32 = 70;

/// Whether a model AETHER1 is configured to run on is too small to troubleshoot or to drive
/// an agent hand-off.
///
/// Judged from the size in the tag, through `routing::parameter_billions`, so there is one
/// definition of "how big is this model" rather than a second one here that disagrees with
/// the routing table about `:32b-instruct-q4`.
///
/// **An unlabelled name is never called too small.** `parameter_billions` returns `u32::MAX`
/// for a name that claims no size, and a model that did not say is not thereby small --
/// telling somebody their model cannot do this, on a guess, is worse than saying nothing.
fn too_small_for_tools(model: &str) -> bool {
    let tenths = crate::llm::routing::parameter_billions(model);
    tenths != u32::MAX && tenths < TOOL_PROTOCOL_FLOOR_TENTHS
}

/// Which agent commands are on this machine.
///
/// Probed in one place and handed to `advise` rather than looked up inside it, for the
/// reason `setup::advise` takes its three facts as arguments: a decision that reads the
/// machine itself can only be tested on a machine in that state, and "no agent installed"
/// is precisely the state a developer's own machine is never in.
#[derive(Debug, Clone, Copy, Default)]
pub struct AgentsFound {
    pub opencode: bool,
    pub aider: bool,
}

impl AgentsFound {
    /// Looks for both commands on the PATH.
    pub fn probe() -> AgentsFound {
        AgentsFound {
            opencode: which::which("opencode").is_ok(),
            aider: which::which("aider").is_ok(),
        }
    }
}

/// How to give the server a bigger context window, per operating system.
///
/// aider can ask for one per model and does, in its own step. opencode goes through the
/// OpenAI-shaped endpoint, which has no way to say it, so the only place left is the
/// environment the server itself runs in -- and that is set differently depending on how
/// Ollama was started, which is what this is for.
///
/// It is a machine-wide setting, so the detail says so: everything else talking to this
/// server gets the bigger window too, and pays for it in memory.
fn context_step(os: Os, num_ctx: u32) -> Step {
    let shared = "This is set on the server rather than by the program talking to it, so \
                  everything using this AI gets the bigger window -- including the HUD, \
                  which will use a little more memory for it. Restart Ollama afterwards.";
    match os {
        Os::Linux => Step::run(
            "Give the model room to read",
            shared,
            &format!(
                "sudo mkdir -p /etc/systemd/system/ollama.service.d && printf '[Service]\\nEnvironment=\"OLLAMA_CONTEXT_LENGTH={num_ctx}\"\\n' | sudo tee /etc/systemd/system/ollama.service.d/context.conf && sudo systemctl daemon-reload && sudo systemctl restart ollama"
            ),
        ),
        Os::Mac => Step::run(
            "Give the model room to read",
            shared,
            &format!("launchctl setenv OLLAMA_CONTEXT_LENGTH {num_ctx}"),
        ),
        Os::Windows => Step::run(
            "Give the model room to read",
            shared,
            &format!("setx OLLAMA_CONTEXT_LENGTH {num_ctx}"),
        ),
    }
}

/// The two agents worth offering, in the order they are worth trying.
///
/// `opencode` first because it is the closer match to what somebody moving off a cloud
/// coding assistant is used to -- a terminal session that holds the whole project and
/// decides for itself which files to open. `aider` second because it is the better answer
/// for anybody who wants every edit to arrive as its own commit, which is its own kind of
/// safety net when the model driving it is a 7B.
fn agents(
    os: Os,
    found: AgentsFound,
    endpoint: &str,
    model: &str,
    num_ctx: u32,
) -> Vec<AgentChoice> {
    // The two agents want the same server at two different addresses, and the scanner
    // reports whichever one answered -- `/v1` when it found the OpenAI shape, the bare
    // address when it found the native one. Handing either straight to both is how one of
    // them ends up pointed at `.../v1/v1` or at an API it does not speak, so the endpoint
    // is normalised once here and each agent is given the half it wants.
    let native = endpoint
        .trim_end_matches('/')
        .trim_end_matches("/v1")
        .trim_end_matches('/')
        .to_string();
    let openai = format!("{native}/v1");

    // Written on one line, through printf rather than a heredoc, because these are made to
    // be pasted: a heredoc breaks the moment anything indents it, and both the panel and
    // the terminal report indent a command block to show it is one.
    let opencode_config = format!(
        "{{\"$schema\": \"https://opencode.ai/config.json\", \"provider\": \
         {{\"aether1-local\": {{\"npm\": \"@ai-sdk/openai-compatible\", \
         \"name\": \"On this computer\", \"options\": {{\"baseURL\": \"{openai}\"}}, \
         \"models\": {{\"{model}\": {{\"name\": \"{model}\"}}}}}}}}}}"
    );

    let opencode = AgentChoice {
        key: "opencode".to_string(),
        label: "opencode".to_string(),
        blurb: "A whole coding session in a terminal window. You describe the change, it \
                decides which files to open and edits them. Closest to what a paid coding \
                assistant feels like."
            .to_string(),
        installed: found.opencode,
        recommended: !found.aider,
        install: vec![match os {
            Os::Windows | Os::Mac | Os::Linux => Step::run(
                "Install opencode",
                "Paste this into a terminal. It needs Node.js, which most machines with a \
                 code editor on them already have -- if this says `npm: not found`, install \
                 Node.js first and come back.",
                "npm install -g opencode-ai",
            ),
        }],
        connect: vec![
            Step::run(
                "Point it at this computer",
                "This writes opencode's settings file with the address and the model this \
                 machine is actually running.",
                &format!(
                    "mkdir -p ~/.config/opencode && printf '%s' '{opencode_config}' > ~/.config/opencode/opencode.json"
                ),
            ),
            context_step(os, num_ctx),
            Step::run(
                "Start it in your project",
                "Change to the folder your code is in first. Then describe what you want \
                 changed, in a sentence.",
                "opencode",
            ),
        ],
    };

    let aider = AgentChoice {
        key: "aider".to_string(),
        label: "aider".to_string(),
        blurb: "Turns every change it makes into its own git commit, so anything it gets \
                wrong is one `git revert` away. You name the files it may touch."
            .to_string(),
        installed: found.aider,
        recommended: false,
        install: vec![match os {
            Os::Linux => Step::run(
                "Install aider",
                "Paste this into a terminal. It installs aider into its own place rather than \
                 into the system Python, which is what Arch and most current distributions \
                 require. If `uv` is not installed, use `pipx install aider-chat` instead.",
                "uv tool install --force aider-chat",
            ),
            Os::Mac | Os::Windows => Step::run(
                "Install aider",
                "Paste this into a terminal. It installs aider into its own place rather than \
                 alongside your system Python, which avoids the most common way this goes \
                 wrong. If `uv` is not installed, use `pipx install aider-chat` instead.",
                "uv tool install --force aider-chat",
            ),
        }],
        connect: vec![
            Step::run(
                "Give the model room to read",
                "The server's default context is small enough that a long file quietly loses \
                 its top half, and the model then writes something that contradicts the part \
                 it can no longer see. This asks for a bigger one, for this model only, so \
                 nothing else on the machine pays for it. Run it in your project folder.",
                &format!(
                    "printf '%s\\n' '- name: ollama_chat/{model}' '  extra_params:' '    num_ctx: {num_ctx}' > .aider.model.settings.yml"
                ),
            ),
            Step::run(
                "Start it in your project",
                "The first line tells aider where this machine's server is; the second starts \
                 it. `ollama_chat/` rather than `ollama/` matters -- the other one talks to \
                 the wrong half of the server and the model replies as if it were finishing \
                 your sentence.",
                &format!("export OLLAMA_API_BASE={native}\naider --model ollama_chat/{model}"),
            ),
        ],
    };

    vec![opencode, aider]
}

/// The house rules, written for a model rather than for a person.
///
/// This is the answer to the question the whole feature grew out of: a local model will not
/// write like whoever -- or whatever -- wrote the rest of the repository, and no amount of
/// choosing between models fixes that, because it is not a capability problem. It is that
/// the conventions are not written down anywhere the model can read. Both agents above look
/// for an `AGENTS.md` at the root of the project and put it in front of the model on every
/// turn, so writing them down once is worth more than two model sizes.
///
/// Deliberately not written into anybody's project folder. Aether1 writes inside its own
/// data directory and nowhere else, and a wizard that quietly drops files into whatever
/// folder was open is a different kind of program from this one. It is printed, and
/// `aether1 code conventions > AGENTS.md` is one command.
pub fn conventions() -> String {
    "\
# House rules

Follow these when changing this repository. They are not style preferences; each one is
here because ignoring it produced a bug or an unreadable file before.

## Comments

- Comment the **why**, never the what. `// increment i` is noise; `// the last entry that
  fits, because a machine under the floor still needs a next step` is the reason somebody
  can change this safely later.
- Every module starts with a doc comment saying what question the module answers and what
  failure it exists to prevent. If you cannot write that sentence, the module is doing two
  things.
- Write `--` rather than an em dash, everywhere, including in strings shown to the user.

## Shape

- Keep decisions in pure functions that take the facts as arguments. A function that probes
  the machine *and* decides what to do with it cannot be tested without the machine.
- Two states that look alike but mean different things -- no answer versus an empty answer,
  no choice made versus a choice of nothing -- stay separate, and get a test that fails
  loudly when somebody collapses them.
- Never add a compatibility shim, a deprecation path or a migration for users who do not
  exist. Say plainly in the change description what breaks, then make the change.

## Text the user reads

- Second person, full sentences, no jargon. The reader does not know what a context window
  is and does not need to.
- Machinery speaks as the program, never in the character's voice. Attributing a technical
  notice to the persona erodes trust in the persona.
- Say what happened and what to do about it. A message that only reports a failure is half
  a message.

## Tests

- Tests go in a `#[cfg(test)] mod tests` at the bottom of the file they test.
- Test the decision, not the plumbing: which case the input lands in, and that the ordering
  a lookup depends on is actually held.

## Frontend

- Build DOM nodes rather than interpolating data into markup. Anything on the page came
  from a server that names its own models.
- Settings fields keep the ids they have; the load and save paths address them by id and
  know nothing about how they are grouped.
"
    .to_string()
}

/// Everything the HUD needs to draw the coding wizard, from one probe.
#[derive(Debug, Clone, Serialize)]
pub struct CodingAdvice {
    pub os: Os,
    pub stage: Stage,
    pub headline: String,
    /// What to do now, in order.
    pub steps: Vec<Step>,
    /// Models to offer, whether or not any is downloaded yet.
    pub models: Vec<CodingModel>,
    /// The agents to drive them with, with this machine's address already in the commands.
    pub agents: Vec<AgentChoice>,
    /// The server that answered, so the HUD can download onto it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// The coding model these commands were built around: the best one already downloaded,
    /// or the recommendation when none is.
    pub model: String,
    /// Whether that model is downloaded, or is still a suggestion.
    pub model_installed: bool,
    /// The context window the connect steps ask for.
    pub context_tokens: u32,
    /// What the recommendation was measured against, in a sentence, so the reason a large
    /// machine is being offered a small model is on the panel rather than inferred.
    pub sized_against: String,
    /// The model AETHER1 itself is configured to run on, when that is a model on this
    /// machine. `None` for a cloud provider, where none of this applies, and for a fresh
    /// install that has not chosen one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aether1_model: Option<String>,
    /// Whether that model is too small to troubleshoot or to drive an agent hand-off. The
    /// reason this panel says anything about AETHER1's own model at all: the wizard's
    /// smallest tier is a 1B, and a 1B cannot hold the tool protocol.
    pub aether1_model_too_small: bool,
    /// Whether the model these commands are built around would also be a better model for
    /// AETHER1 itself than the one it is on now -- so one download covers both jobs.
    pub covers_aether1_too: bool,
    /// Whether local-only mode is on, which stops both the download and the agent install.
    pub local_only: bool,
    pub needs_attention: bool,
}

/// Reads the machine and says what to do next to write code on it.
///
/// A pure function of the scan, the memory figure and the local-only flag, for the reason
/// `setup::advise` is: every case below can then be tested with no server anywhere near the
/// test.
pub fn advise(
    scan: &ScanResult,
    ram_total_gb: f64,
    gpus: &[crate::gpu::Gpu],
    local_only: bool,
    found: AgentsFound,
    aether1_model: Option<&str>,
    preferred_model: Option<&str>,
) -> CodingAdvice {
    let os = Os::current();

    // The same preference `setup::advise` makes: a server with models beats one without,
    // even though both are up.
    let running = scan
        .local_servers
        .iter()
        .find(|server| !server.models.is_empty())
        .or_else(|| scan.local_servers.first());

    let installed: Vec<String> = running.map(|s| s.models.clone()).unwrap_or_default();
    let endpoint = running.map(|s| s.endpoint.clone());

    // The card, not the machine, when there is one. See `models_for`.
    let card = crate::gpu::dedicated(gpus);
    let models = models_for(ram_total_gb, card.and_then(|gpu| gpu.vram_gb), &installed);

    // The model every command below is built around. A coding model that is already here
    // beats one that would have to be downloaded, however much better the download is --
    // the operator asked to write code, not to wait for 19 GB. Among those already here,
    // the last in catalogue order, which is the largest that this machine was offered.
    let downloaded = models.iter().rfind(|m| m.installed);
    let recommended = models
        .iter()
        .find(|m| m.recommended)
        .expect("models_for always marks one");
    let preferred = preferred_model
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .and_then(|name| {
            models
                .iter()
                .find(|model| model.installed && model.name == name)
        });
    let chosen = preferred.or(downloaded).unwrap_or(recommended);
    let model = chosen.name.clone();
    let model_installed = chosen.installed;

    let has_coding_model = installed.iter().any(|name| is_coding_model(name));

    // What AETHER1 itself is running on, and whether that is enough for the jobs that are
    // not chatting. Offline these all go through the text tool protocol, so the model the
    // brain wizard points a small machine at -- a 1B -- can hold a conversation and cannot
    // hold the protocol.
    let aether1_model = aether1_model
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string);
    let aether1_model_too_small = aether1_model.as_deref().is_some_and(too_small_for_tools);
    // Worth saying only when it is actually an improvement: the model in hand is up to the
    // job and the one AETHER1 is on is not, or is not set at all.
    let covers_aether1_too =
        chosen.runs_aether1 && (aether1_model_too_small || aether1_model.is_none());

    let num_ctx = context_tokens(ram_total_gb);
    let sized_against = sized_against(ram_total_gb, card);

    // Built before the stage is decided rather than after, so "is an agent installed?" is
    // answered by the same list the HUD draws. Deciding it separately is how a panel ends
    // up saying "install one of these" above an entry already marked as installed.
    let agents = agents(
        os,
        found,
        endpoint.as_deref().unwrap_or("http://localhost:11434"),
        &model,
        num_ctx,
    );
    let has_agent = agents.iter().any(|agent| agent.installed);

    let stage = if running.is_none() {
        Stage::NoServer
    } else if !has_coding_model {
        Stage::NoCodingModel
    } else if !has_agent {
        Stage::NoAgent
    } else {
        Stage::Ready
    };

    let mut steps = Vec::new();

    // Said first and once, because both remaining stages end in something that needs the
    // network and finding that out at the download is worse than being told here.
    if local_only && stage != Stage::Ready {
        steps.push(Step::say(
            "Local-only mode is on",
            "Downloading a model and installing a coding program both need the internet, and \
             local-only mode refuses both. Turn it off under The Brain while you set this up, \
             then turn it back on -- everything below runs on this computer once it is here.",
        ));
    }

    match stage {
        Stage::NoServer => steps.push(Step::say(
            "Set up the brain first",
            "Writing code uses the same AI server the rest of Aether1 uses, so there is \
             nothing separate to install here. Open Settings, then The Brain, and press \
             \"Set it up for me\". Come back here once it has found a server.",
        )),
        Stage::NoCodingModel => steps.push(Step::say(
            "Download a model built for code",
            "The models you have will write code and will be worse at it than one trained \
             for it -- the same way a good writer is not a good accountant. Pick one from \
             the list below; it lands on this computer and stays there.",
        )),
        Stage::NoAgent => steps.push(Step::say(
            "Install something that can edit your files",
            "The model can write code but cannot open anything. A coding agent is the part \
             that reads your project, makes the change and runs your tests. Pick one below \
             -- the commands under it already have this computer's address in them.",
        )),
        Stage::Ready => steps.push(Step::say(
            "Start it in the folder your code is in",
            "The commands under your agent are ready to paste. Before the first session, \
             copy the house rules further down into a file called AGENTS.md at the top of \
             your project -- both agents read it every turn, and it does more for keeping \
             the model in your style than a bigger model would.",
        )),
    }

    // Said after the stage's own step, because it is a second thing to do rather than the
    // next one, and only when it changes something.
    if covers_aether1_too {
        let detail = match aether1_model.as_deref() {
            Some(current) => format!(
                "AETHER1 is running on {current}, which is fine for talking and too small for \
                 the rest. Offline it has no built-in way to call a tool, so troubleshooting, \
                 repairs and handing work to an agent all go through a written format in the \
                 prompt, and a model that size loses it. {model} holds it. Once it is \
                 downloaded you can point AETHER1 at it under Settings, then The Brain, and \
                 one download covers both jobs."
            ),
            None => format!(
                "AETHER1 has no model of its own chosen yet. {model} is big enough to be that \
                 too -- offline, troubleshooting and handing work to an agent go through a \
                 written format in the prompt that a small model loses, and this one holds \
                 it. Pick it under Settings, then The Brain, once it is downloaded."
            ),
        };
        steps.push(Step::say(
            "This one can run AETHER1 itself as well",
            &detail,
        ));
    }

    CodingAdvice {
        os,
        stage,
        headline: stage.headline().to_string(),
        steps,
        models,
        agents,
        endpoint,
        model,
        model_installed,
        context_tokens: num_ctx,
        sized_against,
        aether1_model,
        aether1_model_too_small,
        covers_aether1_too,
        local_only,
        needs_attention: stage.needs_attention(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_scanner::{
        CloudKeys, LmStudioStatus, LocalApi, LocalServer, OllamaStatus, ScanResult,
    };

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

    fn scan_with(models: &[&str]) -> ScanResult {
        let mut scan = empty_scan();
        scan.local_servers = vec![server(11434, models)];
        scan.has_local_provider = true;
        scan
    }

    const NO_AGENTS: AgentsFound = AgentsFound {
        opencode: false,
        aider: false,
    };

    /// Rule 1 of the catalogue, and the one `models_for` silently depends on: it takes the
    /// *last* entry that fits, so a list that is not sorted by memory recommends the wrong
    /// model on every machine rather than failing anywhere visible. Both columns, because
    /// there are now two ladders and `models_for` takes the last entry that fits on each.
    #[test]
    fn catalogue_is_ordered_by_memory() {
        let mut previous = 0.0;
        let mut previous_vram = 0.0;
        for (name, _, _, _, needs_gb, needs_vram_gb, _) in CATALOGUE {
            assert!(
                *needs_gb >= previous,
                "{name} needs less memory than the entry before it; the catalogue must not \
                 decrease, because models_for picks the last entry that fits"
            );
            assert!(
                *needs_vram_gb >= previous_vram,
                "{name} needs less video memory than the entry before it; the same rule \
                 applies, because models_for picks the last entry that fits on the card"
            );
            assert!(
                needs_vram_gb < needs_gb,
                "{name} wants more video memory than system memory, which would mean a card \
                 big enough to run it on a machine too small to load it"
            );
            previous = *needs_gb;
            previous_vram = *needs_vram_gb;
        }
    }

    /// Every entry has to be recognisable as a coding model by the same test that decides
    /// whether the machine already has one. Without this, downloading exactly what the
    /// wizard recommended can leave it still saying "download a model built for code".
    #[test]
    fn every_catalogue_entry_reads_as_a_coding_model() {
        for (name, _, _, _, _, _, _) in CATALOGUE {
            assert!(
                is_coding_model(name),
                "{name} is not matched by is_coding_model"
            );
        }
    }

    #[test]
    fn a_general_model_is_not_mistaken_for_a_coding_one() {
        assert!(!is_coding_model("llama3.2:3b"));
        assert!(!is_coding_model("gemma3:4b"));
        assert!(is_coding_model("qwen2.5-coder:7b"));
        assert!(is_coding_model("devstral:latest"));
    }

    /// Ollama answers `devstral:latest` for a tag pulled as `devstral`. Comparing the
    /// strings tells somebody to download what they already have.
    #[test]
    fn an_implicit_latest_tag_counts_as_installed() {
        let installed = vec!["devstral:latest".to_string()];
        let models = models_for(64.0, None, &installed);
        let devstral = models.iter().find(|m| m.name == "devstral").unwrap();
        assert!(devstral.installed);
    }

    /// The flag and the rule have to agree. The table says which entries can be AETHER1's
    /// own model; `too_small_for_tools` decides the same thing for a model somebody has
    /// already configured. If those two ever disagree, the panel marks a model as covering
    /// both jobs and then, once it is selected, calls it too small for one of them.
    #[test]
    fn the_table_and_the_size_rule_agree_on_every_entry() {
        for (name, _, _, _, _, _, runs_aether1) in CATALOGUE {
            assert_eq!(
                *runs_aether1,
                !too_small_for_tools(name),
                "{name} is marked runs_aether1 = {runs_aether1}, which the size rule contradicts"
            );
        }
    }

    /// A bigger model is never worse at holding a format than a smaller one, so the flag
    /// only ever turns on going down the list. A gap would mean a machine gets offered a
    /// larger model that claims to do less.
    #[test]
    fn the_catalogue_never_stops_running_aether1_once_it_starts() {
        let mut started = false;
        for (name, _, _, _, _, _, runs_aether1) in CATALOGUE {
            if *runs_aether1 {
                started = true;
            } else {
                assert!(
                    !started,
                    "{name} is smaller-capable than an entry before it"
                );
            }
        }
    }

    /// `parameter_billions` returns u32::MAX for a name that claims no size, and a model
    /// that did not say is not thereby small. Telling somebody their model cannot do this,
    /// on a guess, is worse than saying nothing.
    #[test]
    fn an_unlabelled_model_is_never_called_too_small() {
        assert!(too_small_for_tools("llama3.2:1b"));
        assert!(too_small_for_tools("qwen2.5-coder:3b"));
        assert!(!too_small_for_tools("qwen2.5-coder:7b"));
        assert!(!too_small_for_tools("some-local-build:latest"));
        assert!(!too_small_for_tools("devstral"));
    }

    /// The whole reason the panel says anything about AETHER1's own model: the brain wizard
    /// points a small machine at a 1B, and a 1B holds a conversation but not the tool
    /// protocol the local providers fall back to.
    #[test]
    fn a_1b_brain_is_told_the_coding_model_covers_both() {
        let advice = advise(
            &scan_with(&["qwen2.5-coder:7b"]),
            32.0,
            &[],
            false,
            NO_AGENTS,
            Some("llama3.2:1b"),
            None,
        );
        assert!(advice.aether1_model_too_small);
        assert!(advice.covers_aether1_too);
        assert!(advice
            .steps
            .iter()
            .any(|step| step.title.contains("run AETHER1 itself")));
    }

    /// Nothing to improve, so nothing said. A panel that offers the same advice whatever
    /// the machine is doing is a panel nobody reads twice.
    #[test]
    fn a_brain_already_big_enough_is_left_alone() {
        let advice = advise(
            &scan_with(&["qwen2.5-coder:7b"]),
            32.0,
            &[],
            false,
            NO_AGENTS,
            Some("qwen2.5-coder:14b"),
            None,
        );
        assert!(!advice.aether1_model_too_small);
        assert!(!advice.covers_aether1_too);
    }

    /// A machine whose only coding model is below the floor cannot cover both, and saying
    /// it does would be the panel promising something the download will not deliver.
    #[test]
    fn a_model_below_the_floor_does_not_claim_to_cover_both() {
        let advice = advise(
            &scan_with(&[]),
            6.0,
            &[],
            false,
            NO_AGENTS,
            Some("llama3.2:1b"),
            None,
        );
        let pick = advice.models.iter().find(|m| m.recommended).unwrap();
        assert!(!pick.runs_aether1);
        assert!(advice.aether1_model_too_small);
        assert!(!advice.covers_aether1_too);
    }

    /// The other half of the tag rule: one size being here says nothing about another.
    #[test]
    fn a_bigger_size_of_the_same_family_is_not_installed() {
        let installed = vec!["qwen2.5-coder:7b".to_string()];
        let models = models_for(64.0, None, &installed);
        let seven = models
            .iter()
            .find(|m| m.name == "qwen2.5-coder:7b")
            .unwrap();
        let fourteen = models
            .iter()
            .find(|m| m.name == "qwen2.5-coder:14b")
            .unwrap();
        assert!(seven.installed);
        assert!(!fourteen.installed, "a 7b on the server is not a 14b");
    }

    #[test]
    fn a_small_machine_is_recommended_a_small_model() {
        let models = models_for(8.0, None, &[]);
        let pick = models.iter().find(|m| m.recommended).unwrap();
        assert_eq!(pick.name, "qwen2.5-coder:3b");
    }

    #[test]
    fn a_workstation_is_recommended_the_largest_that_fits() {
        let models = models_for(64.0, None, &[]);
        let pick = models.iter().find(|m| m.recommended).unwrap();
        assert_eq!(pick.name, "qwen3-coder:30b-a3b-q4_K_M");
    }

    /// A machine under the floor still gets one. "Your computer is unsuitable" is not a
    /// next step, and the smallest model is genuinely worth a try.
    #[test]
    fn a_machine_below_the_floor_still_gets_a_recommendation() {
        let models = models_for(2.0, None, &[]);
        assert_eq!(models.iter().filter(|m| m.recommended).count(), 1);
        assert!(!models[0].fits);
        assert!(models[0].recommended);
    }

    /// The bigger ones are still listed. Somebody who knows their graphics card better than
    /// a RAM heuristic does gets to pick past it.
    #[test]
    fn models_that_do_not_fit_are_still_offered() {
        let models = models_for(8.0, None, &[]);
        assert_eq!(models.len(), CATALOGUE.len());
        assert!(models.iter().any(|m| !m.fits));
    }

    fn card(name: &str, vram_gb: f64) -> crate::gpu::Gpu {
        crate::gpu::Gpu {
            name: name.to_string(),
            vram_gb: Some(vram_gb),
            integrated: false,
        }
    }

    /// The case that started this: a big machine with a mid-sized card. Sized off system
    /// memory alone it is told to download 19 GB of Qwen 3 Coder, most of which will not fit
    /// on the card and will be worked out by the processor one word at a time.
    #[test]
    fn a_card_smaller_than_the_machine_decides_the_recommendation() {
        let by_ram = models_for(64.0, None, &[]);
        assert_eq!(
            by_ram.iter().find(|m| m.recommended).unwrap().name,
            "qwen3-coder:30b-a3b-q4_K_M"
        );

        let by_card = models_for(64.0, Some(16.0), &[]);
        let pick = by_card.iter().find(|m| m.recommended).unwrap();
        assert_eq!(pick.name, "qwen2.5-coder:14b");
        assert!(pick.fits_on_gpu);
        // Still listed, and still marked as something this machine can run -- just not
        // something the card can hold.
        let bigger = by_card
            .iter()
            .find(|m| m.name == "qwen3-coder:30b-a3b-q4_K_M")
            .unwrap();
        assert!(bigger.fits);
        assert!(!bigger.fits_on_gpu);
    }

    /// A laptop card: 4 GB against 16 GB of system memory. The memory rule offers the 7B,
    /// which on that machine means half the model on the processor.
    #[test]
    fn a_small_laptop_card_pulls_the_recommendation_down() {
        let models = models_for(16.0, Some(4.0), &[]);
        assert_eq!(
            models.iter().find(|m| m.recommended).unwrap().name,
            "qwen2.5-coder:3b"
        );
    }

    /// Onboard graphics are not a budget of their own -- their memory is the system memory
    /// already counted -- so `gpu::dedicated` hands `None` here and the memory rule stands.
    #[test]
    fn onboard_graphics_leave_the_memory_rule_alone() {
        let onboard = [crate::gpu::Gpu {
            name: "AMD Radeon Graphics".to_string(),
            vram_gb: Some(0.5),
            integrated: true,
        }];
        assert!(crate::gpu::dedicated(&onboard).is_none());
        let models = models_for(32.0, None, &[]);
        assert_eq!(
            models.iter().find(|m| m.recommended).unwrap().name,
            "qwen2.5-coder:14b"
        );
    }

    /// A card too small for anything on the list must not veto the machine it is in. An old
    /// 1 GB display adapter in a 64 GB workstation would otherwise recommend the 1.5B.
    #[test]
    fn a_card_too_small_for_the_list_falls_back_to_memory() {
        let models = models_for(64.0, Some(1.0), &[]);
        assert!(models.iter().all(|m| !m.fits_on_gpu));
        assert_eq!(
            models.iter().find(|m| m.recommended).unwrap().name,
            "qwen3-coder:30b-a3b-q4_K_M"
        );
    }

    /// The panel has to say which of the two rules it used, because on the machines where
    /// they disagree the answer looks like a bug otherwise.
    #[test]
    fn the_panel_says_what_it_sized_against() {
        let with_card = sized_against(64.0, Some(&card("Radeon RX 6800 XT", 16.0)));
        assert!(with_card.contains("Radeon RX 6800 XT"));
        assert!(with_card.contains("16 GB"));

        let without = sized_against(32.0, None);
        assert!(without.contains("32 GB of system memory"));
        assert!(without.contains("dedicated graphics card was found"));
    }

    /// End to end: the advice a desktop with a 6800 XT gets is the card's answer, not the
    /// machine's.
    #[test]
    fn the_advice_is_built_around_the_model_the_card_can_hold() {
        let advice = advise(
            &scan_with(&[]),
            64.0,
            &[card("Radeon RX 6800 XT", 16.0)],
            false,
            NO_AGENTS,
            None,
            None,
        );
        assert_eq!(advice.model, "qwen2.5-coder:14b");
        assert!(advice.sized_against.contains("Radeon RX 6800 XT"));
    }

    #[test]
    fn nothing_running_points_back_at_the_brain() {
        let advice = advise(&empty_scan(), 32.0, &[], false, NO_AGENTS, None, None);
        assert_eq!(advice.stage, Stage::NoServer);
        assert!(advice.needs_attention);
        assert!(advice.endpoint.is_none());
    }

    #[test]
    fn a_server_of_chat_models_is_told_to_get_a_coding_one() {
        let advice = advise(
            &scan_with(&["llama3.2:3b"]),
            32.0,
            &[],
            false,
            NO_AGENTS,
            None,
            None,
        );
        assert_eq!(advice.stage, Stage::NoCodingModel);
        assert!(!advice.model_installed);
        assert_eq!(advice.endpoint.as_deref(), Some("http://localhost:11434"));
    }

    #[test]
    fn a_coding_model_with_no_agent_is_told_to_install_one() {
        let advice = advise(
            &scan_with(&["qwen2.5-coder:7b"]),
            32.0,
            &[],
            false,
            NO_AGENTS,
            None,
            None,
        );
        assert_eq!(advice.stage, Stage::NoAgent);
        assert!(advice.model_installed);
        assert_eq!(advice.model, "qwen2.5-coder:7b");
    }

    #[test]
    fn a_coding_model_and_an_agent_is_ready() {
        let found = AgentsFound {
            opencode: true,
            aider: false,
        };
        let advice = advise(
            &scan_with(&["qwen2.5-coder:7b"]),
            32.0,
            &[],
            false,
            found,
            None,
            None,
        );
        assert_eq!(advice.stage, Stage::Ready);
        assert!(!advice.needs_attention);
    }

    /// The commands are worth doing in Rust only if they carry this machine's own address
    /// and model. A template with `<your model here>` in it is a link to documentation.
    #[test]
    fn the_connect_commands_name_this_machine_and_this_model() {
        let advice = advise(
            &scan_with(&["qwen2.5-coder:14b"]),
            32.0,
            &[],
            false,
            NO_AGENTS,
            None,
            None,
        );
        let aider = advice.agents.iter().find(|a| a.key == "aider").unwrap();
        let commands: String = aider
            .connect
            .iter()
            .filter_map(|step| step.command.clone())
            .collect();
        assert!(commands.contains("ollama_chat/qwen2.5-coder:14b"));
        assert!(commands.contains("http://localhost:11434"));
        assert!(commands.contains(&advice.context_tokens.to_string()));

        let opencode = advice.agents.iter().find(|a| a.key == "opencode").unwrap();
        let config: String = opencode
            .connect
            .iter()
            .filter_map(|step| step.command.clone())
            .collect();
        // opencode wants the OpenAI-shaped half of the server, not the native one.
        assert!(config.contains("http://localhost:11434/v1"));
    }

    /// The bug this test exists for was found by running it, not by reading it. The
    /// scanner reports whichever address answered -- `.../v1` when it recognised the
    /// OpenAI shape -- and handing that straight to both agents pointed opencode at
    /// `/v1/v1` and aider at an API it does not speak.
    #[test]
    fn an_openai_shaped_endpoint_is_given_to_each_agent_in_its_own_shape() {
        let mut scan = empty_scan();
        scan.local_servers = vec![LocalServer {
            endpoint: "http://127.0.0.1:11434/v1".to_string(),
            port: 11434,
            api: LocalApi::OpenAi,
            provider_key: LocalApi::OpenAi.provider_key(),
            models: vec!["qwen2.5-coder:7b".to_string()],
            label: "OpenAI-compatible server".to_string(),
        }];
        let advice = advise(&scan, 32.0, &[], false, NO_AGENTS, None, None);

        let commands = |key: &str| -> String {
            advice
                .agents
                .iter()
                .find(|a| a.key == key)
                .unwrap()
                .connect
                .iter()
                .filter_map(|step| step.command.clone())
                .collect()
        };

        let opencode = commands("opencode");
        assert!(opencode.contains("http://127.0.0.1:11434/v1"));
        assert!(!opencode.contains("/v1/v1"));

        let aider = commands("aider");
        assert!(aider.contains("OLLAMA_API_BASE=http://127.0.0.1:11434\n"));
        assert!(!aider.contains("OLLAMA_API_BASE=http://127.0.0.1:11434/v1"));
    }

    /// Every command here is made to be pasted, and both the panel and the terminal report
    /// indent a command block to show that is what it is. A heredoc does not survive that
    /// -- its closing word has to start the line -- so a command that writes a file writes
    /// it some other way, and this fails if one ever comes back.
    #[test]
    fn no_command_needs_to_start_at_the_left_margin() {
        let advice = advise(
            &scan_with(&["qwen2.5-coder:7b"]),
            32.0,
            &[],
            false,
            NO_AGENTS,
            None,
            None,
        );
        let every = advice
            .agents
            .iter()
            .flat_map(|agent| agent.install.iter().chain(agent.connect.iter()))
            .chain(advice.steps.iter())
            .filter_map(|step| step.command.as_deref());
        for command in every {
            assert!(
                !command.contains("<<"),
                "a heredoc breaks as soon as the command block is indented: {command}"
            );
        }
    }

    /// A server on a port that is not Ollama's still gets commands that reach it.
    #[test]
    fn a_server_on_another_port_is_carried_into_the_commands() {
        let mut scan = empty_scan();
        scan.local_servers = vec![server(1234, &["qwen2.5-coder:7b"])];
        let advice = advise(&scan, 32.0, &[], false, NO_AGENTS, None, None);
        let opencode = advice.agents.iter().find(|a| a.key == "opencode").unwrap();
        let config: String = opencode
            .connect
            .iter()
            .filter_map(|step| step.command.clone())
            .collect();
        assert!(config.contains("http://localhost:1234/v1"));
    }

    /// The one already downloaded wins over a better one that is not, whatever the machine
    /// could run -- the operator asked to write code, not to wait for 19 GB.
    #[test]
    fn a_downloaded_model_beats_a_better_undownloaded_one() {
        let advice = advise(
            &scan_with(&["qwen2.5-coder:7b"]),
            64.0,
            &[],
            false,
            NO_AGENTS,
            None,
            None,
        );
        assert_eq!(advice.model, "qwen2.5-coder:7b");
        assert!(advice.model_installed);
        // The recommendation for the machine is still the bigger one; they are different
        // questions and the panel shows both.
        let recommended = advice.models.iter().find(|m| m.recommended).unwrap();
        assert_eq!(recommended.name, "qwen3-coder:30b-a3b-q4_K_M");
    }

    #[test]
    fn local_only_is_said_before_anything_that_needs_the_network() {
        let advice = advise(
            &scan_with(&["llama3.2:3b"]),
            32.0,
            &[],
            true,
            NO_AGENTS,
            None,
            None,
        );
        assert!(advice.local_only);
        assert!(advice.steps[0].title.contains("Local-only"));
    }

    /// Nothing is owed on a machine that is ready, so the local-only warning -- which is
    /// about installing things -- has nothing to warn about.
    #[test]
    fn local_only_says_nothing_once_everything_is_here() {
        let found = AgentsFound {
            opencode: true,
            aider: true,
        };
        let advice = advise(
            &scan_with(&["qwen3-coder:30b-a3b-q4_K_M"]),
            64.0,
            &[],
            true,
            found,
            None,
            None,
        );
        assert_eq!(advice.stage, Stage::Ready);
        assert!(!advice.steps[0].title.contains("Local-only"));
    }

    #[test]
    fn the_context_window_grows_with_the_machine() {
        assert_eq!(context_tokens(8.0), 8192);
        assert_eq!(context_tokens(16.0), 16384);
        assert_eq!(context_tokens(32.0), 32768);
        assert_eq!(context_tokens(64.0), 65536);
    }

    /// The house rules are the answer to "it does not write like the rest of the repo", so
    /// they have to be a file an agent will actually read, not prose about conventions.
    #[test]
    fn the_conventions_are_a_markdown_document() {
        let text = conventions();
        assert!(text.starts_with("# House rules"));
        assert!(text.contains("## Comments"));
        assert!(text.contains("## Tests"));
    }
}
