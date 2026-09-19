# Aether1 Goals

## North star

**A companion that lives on your machine, can actually operate it, and gets to know you
by running on your hardware long enough to learn.**

Put plainly, by the person building it: *a chatting friend chilling on your system that
can help out when running into a problem.* Read the rest of this document against that
sentence. The three things in the next section — it runs on your hardware, it can operate
the machine, it remembers — are not the point in themselves; they are what holds the point
up. A friend who forgets you, cannot do anything, and stops working when the wifi drops is
not much of a friend. **Delight is the purpose. The three pillars are how it is earned**,
which is why they are the structure of this document and delight is not a fourth item on
the list.

Warmth has a reference too, and it is an old one: Cortana at launch, before she was a
search box. The bar is a companion people talked to because they wanted to, not because
they needed something.

A chat window with a nice avatar in front of it would be the easy version of this, and it
is not what is being built. The thing that makes a locally-run
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

This is the capability the project is really about, and the one furthest from finished.
The companion can now *see* your system and run a short list of programs that look at it;
the goal is that it can *work* it:

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

**Consent that never learns is its own kind of failure**, though. An assistant that asks the
same question about the same folder forever is not being careful, it is being broken, and
the operator will switch the whole layer off to make it stop. The answer is not a looser
prompt but a wider *field*: each speciality reads the folders its job needs without asking,
you add folders to that speciality in Settings, and the request stops coming up. What no
setting can widen is the list of files that are never read — passwords, keys, and Aether1's
own database — which sits below the domain check and beats a domain, an approval and an
elevation alike.

### 3. It remembers, and becomes yours

**Durable memory is a folder of plain text files** — a vault, in your home directory,
that you can open in any editor, keep in Obsidian, search with `grep`, version with `git`,
back up by copying, and hand to a different tool entirely. An index file at its root says
what is in it and which notes matter for which kind of job, so the companion primes itself
from the right handful of notes rather than trying to hold everything at once. There is no
ceiling: the vault can outgrow any context window because only the relevant part is ever
loaded.

SQLite keeps what it is genuinely good for and nothing else: chat transcripts, settings,
and the action log — high-volume, queryable, uninteresting to read by hand.

Both of those are now true rather than planned. The vault exists, primes the companion, is
written back to, and — since step 35 — receives every conversation as a dated note under
`daily/`, automatically and without a prompt, because asking you to approve your own
conversation being remembered is a question with only one answer. It can be switched off,
after which conversations stay in the database and nowhere else.

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
   nodes change voice and manner — never what the system can or will do. That is a floor,
   not a ceiling: manner *is* the product here, so a persona with jokes, opinions and a
   sense of humour is doing its job rather than overstepping it. Every persona
   pushes back: a companion that agrees with a bad idea is worse than no companion, and
   that is a property of all of them, not a personality option.
7. **One binary, no install ritual.** The Rust/Tauri core plus a static frontend; the
   setup script and `.tar.gz` bundle do the rest.
8. **Linux and Windows are peers.** Development happens on Linux and the result is
   installed on both, so a capability is unfinished until it works on both. macOS follows
   if the hardware ever does. A phone is a *face* for a desktop instance over your own
   network, not a fourth port of the whole system.

## Where we are today

Built (steps 1–12, 14, 20–35 of [IMPLEMENTATION.md](IMPLEMENTATION.md)):

- Multi-provider engine — Ollama, OpenAI-compatible, Gemini, Anthropic — streaming, with
  provider auto-detection, one-click model pulls, hardware-aware model suggestions and a
  local-only mode that refuses to leave the machine (`llm/providers.rs`, `model_scanner.rs`,
  `setup.rs`).
- Reachable without the HUD: global hotkey, `aether1 prompt|status|say|toggle`, and a
  headless HTTP/WebSocket mode (`cli.rs`, `hotkey.rs`, `server.rs`).
- Tools behind a path guard and a consent path: read-only ones run freely, anything that
  changes the machine is proposed and waits for approval, and everything is logged with
  who allowed it and how to undo it (`tools/`, `llm/db.rs`). Native tool-calling wire
  formats where the provider has them, a prompt-level fallback where it doesn't.
- Specialities rather than costumes: each persona reads the folders its job needs, asks
  once per request for anything else, and can have folders added to its field in Settings
  so it stops asking. The never-read list sits below all of it (`tools/domain.rs`,
  `tools/fs_guard.rs`).
- A command runner that ships usable: a short starter allowlist of programs that can only
  look at the machine, never change it, every run still proposed for approval
  (`tools/mutating.rs`).
- The vault: a folder of markdown notes it primes from, writes back to, searches and
  archives — and a dated note per day holding every conversation, written automatically
  (`vault/`).
- Speech in and out without the network, given a local engine: Piper for synthesis,
  whisper.cpp for recognition, hold-Space to talk (`llm/tts.rs`, `llm/stt.rs`).
- Cross-platform host telemetry, a model-performance scoreboard built from real requests,
  the theme engine, personas with per-persona voices, identity forging, holographic avatars
  and HUD, tray presence, setup/packaging scripts.

Missing, against the three things above:

- **The good voice is still a manual install.** Piper's binary and its voice files are
  fetched by hand, from a project whose name collides with an unrelated gaming-mouse app.
  Step 36 made every failure along that path say what is wrong instead of reporting success
  and going quiet, but the path itself is still four manual steps.
- **The hotkey doesn't reach the microphone.** Push-to-talk works while the HUD has focus;
  holding a key to talk from another application needs an OS-level press-and-hold that the
  global-shortcut plugin doesn't express yet.
- **It proposes actions, it doesn't run errands.** Pillar 2's *change* and *do* verbs go
  through the consent path one action at a time. Nothing yet turns "clear out the build
  caches" into a plan with several steps in it.
- **Thin context.** Telemetry reaches the prompt; logs, crashes, and the working
  environment do not (step 13).
- **One model at a time.** Choosing between several local models by what the question needs
  is a stated core requirement and deliberately not started (step 19).

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

**Phase 1b — A voice that works with the network unplugged.** *(shipped, bar the global
press-and-hold)* Local speech synthesis and local speech recognition, and hold-a-key
push-to-talk instead of a microphone deciding for itself when you meant it. This is the
correction that makes principle 2 true.

**Phase 2 — Hands.** *(shipped)* The tool/consent layer, then a first tool set: read files,
inspect processes and services, change Aether1's own settings, and a vetted command runner.
Native tool-calling wire formats for the providers that support them, with a prompt-level
fallback for local models that don't.

**Phase 3 — The vault.** *(shipped)* Durable memory moves out of SQLite and into a folder of
notes with an index at its root, which the companion primes itself from and writes back to
as you work. Retrieval over the notes, session consolidation into notes rather than
transcript, observed-habit capture, decay. No memory browser to build: the browser is your
editor.

**Phase 4 — Situation.** *(half shipped)* AI telemetry — provider, model, tokens, spend —
beside the hardware HUD it already resembles, measured rather than invented. Still to do:
crash and journal capture wired to the tray, so the companion is the first responder with
the failing process already in context.

**Phase 5 — Reach beyond itself.** Hand a task to whichever coding agent or CLI tool is
installed and narrate the result. Possibly an MCP client, to borrow the existing tool
ecosystem instead of growing every integration by hand. Packaging (AUR, `.deb`) so a fresh
machine is one command away.

**Phase 6 — Continuity.** Memory that moves between your machines under your control, and a
persona/palette identity the rest of your system can follow.

## Non-goals

- **Not a coding agent.** Aether1 may hand work to one — with your permission, per
  handoff, and ideally creating and managing that agent for you — but it is not trying to
  be one itself.
- **Not a wrapper around someone else's agent.** Building on a hosted coding agent buys
  enormous capability immediately and costs the thing this project is for: working when
  the network doesn't.
- **Not a cloud service.** No account, no server-side memory, no telemetry leaving the box.
- **Not a desktop environment or distribution.** It runs on your setup; it doesn't replace
  it.
- **Not a replacement for a general-purpose assistant.** Conversation for its own sake is
  in scope: the companion should be good company, tell a joke, and be worth talking to on a
  day when nothing is broken. What belongs somewhere else is the feature with no
  relationship to your machine, your history with it, or the conversation itself.

## Decided, 2026-09-19

Nine questions put to the project's owner, and the answers, recorded here because most of
them are the kind a later reader would otherwise re-litigate.

1. **Delight is the purpose, not a pillar.** Small talk and jokes are in scope.
2. **Linux and Windows are peers**; macOS if the hardware ever appears; a phone later, as a
   face for a desktop instance over the LAN.
3. **Agent handoff is allowed**, per handoff, with permission — and Aether1 may need to
   create and manage those agents itself. Omarchy's *ease* is the bar to clear.
4. **Consent stays per step by default**, but a plan whose steps are all disclosed up front
   may be approved as a whole.
5. **The vault is searched through Obsidian** or a wiki view over the existing layout.
6. **Model routing is both rule-based and learned.** Whatever it suggests must be
   overridable, and the override must be remembered.
7. **A word phrase pairs devices on a network.** Already built — see `serve_auth.rs`.
8. **Every dependency is checked**, and a missing one always points at least to its
   website.
9. **A small, competent local model is the floor**, not a fallback.

## Open questions

- ~~How wide is the command runner by default?~~ **Settled:** a curated starter allowlist
  of programs where no argument can change the machine, seeded once so clearing it stays
  cleared. Every run is still proposed. And "approve always" applies to *tools* and to
  *folders within a speciality's field*, never to running a command — `run_command` cannot
  be pre-approved by any path.
- ~~Retrieval over a folder of notes: index and `read_file`, or a real search index?~~
  **Settled:** Obsidian, or a wiki-style view over the layout that already exists. No
  bespoke search UI to build, and no embeddings unless retrieval is *measured* to be the
  bottleneck — a second model resident in RAM is a real cost to pay on a hunch.
- ~~Where does the vault live, and who owns its layout?~~ **Settled:** a plain folder in
  your home directory, set in Settings, with an index at its root, `daily/` for
  conversations, and wiki-links between notes. Obsidian renders it; nothing requires it.
- ~~Provider-native tool calling, or one normalised shape?~~ **Settled:** native where the
  provider offers it, one prompt-level fallback where it doesn't (step 8). What to do about
  small local models that tool-call badly is still open, and is part of the multi-model
  question below.
- Where may models live? **Settled for now:** this machine, or another on your LAN, and no
  further. Nothing about that is technically forced; it is the boundary the project is
  drawn around.
- MCP client, or a first-party tool set? The first buys an ecosystem; the second keeps the
  consent model entirely ours.
- ~~Windows: a reduced capability set, or a parallel implementation?~~ **Settled:** a
  parallel implementation, because Windows is a peer and not a reduced target. Crash
  capture helps here: it is unbuilt on *every* platform today, so it is a fresh design for
  Linux and Windows together rather than a Linux feature to port afterwards.
- ~~How long is the pairing phrase?~~ **Settled: it stays at 12 words, and the work is
  elsewhere.** 12 BIP-39 words is 128 bits, past the point where more words buy anything,
  so a longer phrase would be ceremony. The phrase was never the weak part — the transport
  around it is. In the order they matter: **self-signed TLS** with the certificate's
  fingerprint shown as a safety number on both screens, because today the derived token
  rides a plain `Authorization` header and anyone on the wifi can lift it and replay it
  forever; **rate limiting** on the auth route, which `serve_auth.rs` already describes
  itself as having and does not; **a token per paired device**, so removing one machine
  does not mean re-pairing all of them; and a PAKE such as SPAKE2 later if it is worth the
  rigour, which would stop the phrase crossing the wire at all. Detail in step 45 of
  [IMPLEMENTATION.md](IMPLEMENTATION.md).
- ~~Where does learned model routing keep its corrections?~~ **Settled: keyed on
  speciality, chosen in the HUD, stored in the settings table.** Each speciality gets the
  model that suits its job — a code-specialised model for L'kemi, something personable for
  Halcy — picked from a dropdown beside the avatar that marks one option *(suggested)*. The
  choice is the operator's, explicitly, rather than something inferred from a thumbs-down,
  because an inferred preference is hard to inspect and this one has to be readable. See
  step 19 of [IMPLEMENTATION.md](IMPLEMENTATION.md).
- Where does the *suggestion* come from? Not from the network, whatever else it costs us:
  principle 2 means a machine with no internet still has to see a sensible default. So the
  suggestion is a small table shipped in the binary, matching model families to specialities
  by name, with the local scoreboard in `model_benchmarks` breaking ties on measured speed.
  A Hugging Face or Ollama lookup would rank by downloads, which measures popularity rather
  than fitness, and would make a core affordance depend on being online.

## Getting there

Step-by-step integration of this plan into the existing codebase: [IMPLEMENTATION.md](IMPLEMENTATION.md).
