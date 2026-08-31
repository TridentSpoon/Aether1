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

### On Omarchy

The [Omarchy AI manual](https://omarchy.org/manual/ai/) is the reference point for *how
deeply integrated* an AI layer should be — agents present at a keystroke, local models a
menu entry away, the system's own configuration something the AI can change, crashes
routed to it automatically. That level of integration is the bar.

It is not the blueprint. Omarchy wires up other people's coding agents on a distribution
it controls. Aether1 is one resident companion on whatever machine you already run, and
its job is different: not to write your code, but to be the presence that knows your
system and your habits. We are taking the integration depth and going somewhere else with
it.

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
store (`src-tauri/src/llm/db.rs`). That is the seed of the real thing: a **memory
repository** that accumulates into a model of you and your machine.

What belongs in it, beyond what's there now:

- **Facts you state** — preferences, names, project details, the way you like things done.
- **Facts it observes** — the tools you actually use, the commands you re-run, the hours
  you keep, what your machine's normal looks like so abnormal is recognisable.
- **What it did for you** — every approved action and its outcome, so "do that thing you
  did last week" resolves to something real.
- **Your enquiries over time** — the topics you keep returning to, which shape what it
  offers unprompted.

For that to be useful rather than a growing pile, memory needs retrieval (semantic search
over the store, not just key lookup), consolidation (summarising old sessions into durable
facts instead of unbounded history), and decay (what stops being true stops being
asserted). It also needs to be inspectable and editable — you can read the whole file, see
what it believes about you, and delete any of it.

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
5. **Memory is yours.** On your disk, human-readable, editable, deletable. It never leaves
   the machine unless you move it yourself.
6. **Persona is presentation, capability is shared.** Halcy, R.E.D. 9000, Nexus, the ARX
   nodes change voice and manner — never what the system can or will do.
7. **One binary, no install ritual.** The Rust/Tauri core plus a static frontend; the
   setup script and `.tar.gz` bundle do the rest.

## Where we are today

Built:

- Multi-provider engine — Ollama, OpenAI-compatible, Gemini, Anthropic
  (`src-tauri/src/llm/providers.rs`).
- Provider auto-detection and one-click model pulls (`src-tauri/src/model_scanner.rs`).
- Cross-platform host telemetry over a WebSocket (`llm/telemetry.rs`, `server.rs`).
- Personas with per-persona voices, and identity forging from a free-text purpose
  (`llm/persona.rs`, `llm/genesis.rs`).
- Local SQLite memory and settings, locked to owner-only (`llm/db.rs`).
- Neural TTS plus browser speech recognition (`llm/tts.rs`, `frontend/js/voice.js`).
- Holographic avatars and HUD (`frontend/js/hologram/`), tray presence, setup/packaging
  scripts.

Missing, against the three things above:

- **No hands.** No tool calling of any kind — the companion cannot read a file, run a
  command, or change a setting. This is the single largest gap.
- **No consent machinery.** No plan/approve/execute/log path, which has to exist *before*
  the tools do.
- **Memory is flat.** Key-value plus raw history: no retrieval, no consolidation, no
  observed facts, no record of actions taken, no UI to inspect or edit it.
- **Not reachable.** No global hotkey, no CLI, no headless prompt mode.
- **Nothing streams.** Replies arrive whole, which reads as dead air in a voice interface.
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

**Phase 1 — Reachability and flow.** Global hotkey to summon/dismiss; an `aether1` CLI with
`prompt`, `status`, and `say`; streaming responses end to end (provider → WebSocket → TTS)
so speech starts before generation finishes.

**Phase 2 — Hands.** The tool/consent layer, then a first tool set: read files, inspect
processes and services, change Aether1's own settings, and a vetted command runner. Native
tool-calling wire formats for the providers that support them, with a prompt-level fallback
for local models that don't.

**Phase 3 — Memory that earns its name.** Embeddings and semantic retrieval over the store;
session consolidation into durable facts; an action log; observed-habit capture; and a
memory browser in the HUD where you can read, edit and delete anything it believes.

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
- **Not a cloud service.** No account, no server-side memory, no telemetry leaving the box.
- **Not a desktop environment or distribution.** It runs on your setup; it doesn't replace
  it.
- **Not a general chat client.** If a feature has no relationship to your machine or your
  history with it, it probably belongs somewhere else.

## Open questions

- How wide is the command runner by default — read-only until explicitly widened, or a
  curated allowlist out of the box? Where does "approve once" end and "approve always"
  begin?
- Embeddings locally means another model resident in RAM. Is that acceptable alongside a
  chat model, or does retrieval start as SQLite FTS and grow later?
- Provider-native tool calling per provider, or one normalised shape that loses fidelity on
  some? And what do we do for small local models that tool-call badly?
- MCP client, or a first-party tool set? The first buys an ecosystem; the second keeps the
  consent model entirely ours.
- Windows: crash capture and command execution are the most Linux-shaped parts of the
  design. Does Windows get a reduced capability set, or a parallel implementation?
