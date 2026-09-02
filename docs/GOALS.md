# Aether1 Goals

## North star

**A companion that lives on your machine, can actually operate it, and gets to know you
by running on your hardware long enough to learn.**

Not a chat window with a nice avatar in front of it. The thing that makes a locally-run
companion worth building is precisely what a hosted assistant cannot do: it is *on the
box*. It can see what the machine is doing, it can act on it, and everything it learns
about you stays on the same disk. Those three facts compound — an assistant that can both
observe and act, and that remembers the last thousand times it did so, becomes personal in
a way no amount of prompt engineering reproduces.

### Where this sits

Two projects are worth naming, because both got something right that this one is aiming at.

The [Omarchy AI manual](https://omarchy.org/manual/ai/) is the reference for *how deeply
integrated* an AI layer should be: agents present at a keystroke, local models a menu entry
away, the system's own configuration something the AI can change, crashes routed to it
automatically. That level of integration is the bar. It is not the blueprint — Omarchy
wires up other people's coding agents on a distribution it controls.

[Jared Rhodes' fullstack-agent](https://github.com/jaredrhod/fullstack-agent) ("Jarvis") is
the reference for *shape*: memory, voice, face, hands as four separable parts you can take
one at a time. Two of its decisions are better than what this project had written down, and
are now adopted below — **memory as a folder of plain text files** rather than a database,
and **hold-a-key push-to-talk** rather than always-listening speech recognition. Its
memory design in particular is the one to beat: notes an agent primes itself from, an index
telling it which notes matter for which job, and no ceiling because nothing has to fit in
one context window.

Where Aether1 differs is the runtime. Jarvis is a skin over Claude Code: its hands *are*
Claude Code's, which is why it can do so much on day one and why it needs a cloud
subscription to do any of it. Aether1 is one native binary that carries its own engine,
its own tool layer, and its own consent model, so that a machine running nothing but
Ollama is still fully functional. That is a slower road to the same capabilities, taken
deliberately: the whole argument for a companion that lives on your hardware collapses if
it stops working when the network does.

## The three things that matter

### 1. It runs on your hardware

Local models are the default path, not a degraded mode. A machine with Ollama or LM Studio
and no internet connection should be fully functional — conversation, telemetry, voice,
memory, actions. Cloud API keys are an option for when you want a bigger model, never a
dependency.

This is not only a privacy stance, though it is that too. It is what makes the rest
possible: an assistant you own outright can be handed the keys to your system and your
history, because nothing leaves the machine when you do.

### 2. It can actually interact with your PC

This is the capability the project is really about, and the one Aether1 does not have yet.
Today the companion can *describe* your system; the goal is that it can *work* it:

- **See** — files, processes, logs, journal entries, hardware state, what crashed and why.
- **Change** — settings and dotfiles, package installs, services started and stopped,
  scripts run, windows and workspaces arranged.
- **Do** — the multi-step errands you would otherwise do by hand: "clear out the build
  caches", "figure out what's eating my battery", "set this machine up like the laptop".

The design constraint is consent, not capability. Every action the model can take is a
declared tool that says whether it mutates anything. Read-only tools run freely; anything
that changes the machine is proposed as a plan you approve before it executes, and every
executed action is logged where you can see and undo it. An agent with root and no brakes
is a liability; an agent that shows you its plan first is a colleague.

### 3. It remembers, and becomes yours

Aether1 already writes chat history and explicit `remember that …` facts to a local SQLite
store (`src-tauri/src/llm/db.rs`). That is a start, but a database is the wrong home for
the part of memory that matters.

**Durable memory is a folder of plain text files** — a vault, in your home directory,
that you can open in any editor, keep in Obsidian, search with `grep`, version with `git`,
back up by copying, and hand to a different tool entirely. An index file at its root says
what is in it and which notes matter for which kind of job, so the companion primes itself
from the right handful of notes rather than trying to hold everything at once. There is no
ceiling: the vault can outgrow any context window because only the relevant part is ever
loaded.

SQLite keeps what it is genuinely good for and nothing else: chat transcripts, settings,
and the action log — high-volume, queryable, uninteresting to read by hand.

**The notes link to each other, which means the mind map already exists.** Wiki-style
links between notes are what Obsidian, Logseq and every other tool of that family render as
a graph — so the picture of what the companion knows, and how it connects, is something an
existing app draws for free. Building that view natively would be months of work to arrive
somewhere worse: worse at layout, worse at search, worse at editing, and unable to open
anyone else's notes. The right move is to write notes that a mind-map app is good at
reading, and let it be the mind map.

What accumulates in the vault:

- **Facts you state** — preferences, names, project details, the way you like things done.
- **Facts it observes** — the tools you actually use, the commands you re-run, the hours
  you keep, what your machine's normal looks like so abnormal is recognisable.
- **What it did for you** — every approved action and its outcome, so "do that thing you
  did last week" resolves to something real.
- **Your enquiries over time** — the topics you keep returning to, which shape what it
  offers unprompted.

For that to stay useful rather than becoming a pile, it needs retrieval (find the right
notes for this question), consolidation (a session becomes a note, not another thousand
lines of transcript), and decay (what stops being true stops being asserted). The files
being plain text is what makes all three inspectable: when the companion says something
odd about you, you can find the line that caused it and delete it.

Personalisation then falls out of memory plus persona: the same companion, tuned by what
it knows, wearing whichever of the personas you chose.

## Principles

1. **Resident, not launched.** Default state is running — tray, hotkey, daemon. Anything
   that requires opening an app first has failed the test.
2. **Local first, cloud by choice.** Full function with zero network. Cloud is an upgrade.
3. **Situated in the host.** Telemetry, crashes, logs and your configuration are native
   context, not something the model has to be told about.
4. **Acting requires consent.** Mutating actions go plan → approve → execute → log, and are
   reversible where possible.
5. **Memory is yours.** A folder of plain text on your disk — readable, editable and
   deletable with the tools you already use, not through an interface we have to build
   first. It never leaves the machine unless you move it yourself.
6. **Persona is presentation, capability is shared.** Halcy, R.E.D. 9000, Nexus, the ARX
   nodes change voice and manner — never what the system can or will do. Every persona
   pushes back: a companion that agrees with a bad idea is worse than no companion, and
   that is a property of all of them, not a personality option.
7. **One binary, no install ritual.** The Rust/Tauri core plus a static frontend; the
   setup script and `.tar.gz` bundle do the rest.

## Where we are today

Built (steps 1–5 of [IMPLEMENTATION.md](IMPLEMENTATION.md)):

- Multi-provider engine — Ollama, OpenAI-compatible, Gemini, Anthropic — streaming, with
  provider auto-detection and one-click model pulls (`llm/providers.rs`,
  `model_scanner.rs`).
- Reachable without the HUD: global hotkey, `aether1 prompt|status|say|toggle`, and a
  headless HTTP/WebSocket mode (`cli.rs`, `hotkey.rs`, `server.rs`).
- Tools behind a path guard and a consent path: read-only ones run freely, anything that
  changes the machine is proposed and waits for approval, and everything is logged with
  who allowed it and how to undo it (`tools/`, `llm/db.rs`).
- Speech in and out without the network, given a local engine: Piper for synthesis,
  whisper.cpp for recognition, hold-Space to talk (`llm/tts.rs`, `llm/stt.rs`).
- Cross-platform host telemetry, personas with per-persona voices, identity forging,
  holographic avatars and HUD, tray presence, setup/packaging scripts.

Missing, against the three things above:

- **Memory is a database, not a vault.** Key-value facts and raw transcript: no notes, no
  index, no retrieval, no consolidation, no observed facts, and nothing you can open in an
  editor.
- **The hotkey doesn't reach the microphone.** Push-to-talk works while the HUD has focus;
  holding a key to talk from another application needs an OS-level press-and-hold that the
  global-shortcut plugin doesn't express yet.
- **Thin context.** Telemetry reaches the prompt; logs, crashes, and the working
  environment do not.

## Target architecture

Four layers, in dependency order:

1. **Presence** — daemon, tray, global hotkey, `aether1` CLI, headless prompt mode.
2. **Context** — telemetry, logs and crashes, environment, and retrieved memory, assembled
   into structured prompt context rather than prose.
3. **Capability** — the tool layer. Each tool declares its arguments and whether it
   mutates; mutating calls route through the consent path; all calls are logged to the
   memory repository as things that happened.
4. **Presentation** — persona, voice, avatar, HUD. Swappable without touching 1–3.

Memory sits across 2 and 3 rather than beside them: it is both an input to context and an
output of capability.

## Roadmap

**Phase 1 — Reachability and flow.** *(shipped)* Global hotkey to summon/dismiss; an
`aether1` CLI with `prompt`, `status`, `say` and `toggle`; streaming responses end to end
so speech starts before generation finishes.

**Phase 1b — A voice that works with the network unplugged.** Local speech synthesis and
local speech recognition, and hold-a-key push-to-talk instead of a microphone deciding for
itself when you meant it. This is the correction that makes principle 2 true.

**Phase 2 — Hands.** The tool/consent layer, then a first tool set: read files, inspect
processes and services, change Aether1's own settings, and a vetted command runner. Native
tool-calling wire formats for the providers that support them, with a prompt-level fallback
for local models that don't.

**Phase 3 — The vault.** Durable memory moves out of SQLite and into a folder of notes with
an index at its root, which the companion primes itself from and writes back to as you
work. Retrieval over the notes, session consolidation into notes rather than transcript,
observed-habit capture, decay. No memory browser to build: the browser is your editor.

**Phase 4 — Situation.** Crash and journal capture wired to the tray, so the companion is
the first responder with the failing process already in context. AI telemetry — provider,
model, tokens, spend — beside the hardware HUD it already resembles.

**Phase 5 — Reach beyond itself.** Hand a task to whichever coding agent or CLI tool is
installed and narrate the result. Possibly an MCP client, to borrow the existing tool
ecosystem instead of growing every integration by hand. Packaging (AUR, `.deb`) so a fresh
machine is one command away.

**Phase 6 — Continuity.** Memory that moves between your machines under your control, and a
persona/palette identity the rest of your system can follow.

## Non-goals

- **Not a coding agent.** Aether1 may hand work to one; it is not trying to be one.
- **Not a wrapper around someone else's agent.** Building on a hosted coding agent buys
  enormous capability immediately and costs the thing this project is for: working when
  the network doesn't.
- **Not a cloud service.** No account, no server-side memory, no telemetry leaving the box.
- **Not a desktop environment or distribution.** It runs on your setup; it doesn't replace
  it.
- **Not a general chat client.** If a feature has no relationship to your machine or your
  history with it, it probably belongs somewhere else.

## Open questions

- How wide is the command runner by default — read-only until explicitly widened, or a
  curated allowlist out of the box? Where does "approve once" end and "approve always"
  begin?
- Retrieval over a folder of notes: does the index plus the existing `read_file` /
  `list_dir` tools get far enough on its own, or does it need a real search index — and if
  so, grep-shaped or embeddings? Embeddings mean a second model resident in RAM alongside
  the chat model.
- Where does the vault live, and who owns its layout? An Obsidian-compatible folder is the
  obvious default, but the notes have to stay useful to someone who has never opened
  Obsidian.
- Provider-native tool calling per provider, or one normalised shape that loses fidelity on
  some? And what do we do for small local models that tool-call badly?
- MCP client, or a first-party tool set? The first buys an ecosystem; the second keeps the
  consent model entirely ours.
- Windows: crash capture and command execution are the most Linux-shaped parts of the
  design. Does Windows get a reduced capability set, or a parallel implementation?

## Getting there

Step-by-step integration of this plan into the existing codebase: [IMPLEMENTATION.md](IMPLEMENTATION.md).
