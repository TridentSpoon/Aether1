# Aether1 Goals

## North star

Aether1 should be to a desktop what the AI chapter of the
[Omarchy manual](https://omarchy.org/manual/ai/) describes for Omarchy: **the AI layer
of the machine, not an app you open.**

Omarchy's premise is that a workstation ships with AI already wired in — the agents are
installed and launchable from a keystroke, local models are one menu entry away, the
system's own configuration is something the agent knows how to change, and when the box
does something interesting (a segfault, a hitting of usage limits) the AI is already
holding the context. Nothing about that is a chat window you navigate to.

Aether1 aims at the same relationship with the host, from the other direction. Omarchy
starts from a distribution and adds agents; Aether1 starts from a resident companion —
avatar, voice, memory, telemetry, tray presence — and grows into the same set of system
duties. The goal is a companion that is *present* (always running, always summonable),
*situated* (it can see the machine's real state), and *capable* (it can act on the
machine, not just talk about it), on Linux first and portable after.

## What we are taking from Omarchy's model

| Omarchy behaviour | What it means for Aether1 |
| --- | --- |
| Agents pre-installed with launcher stubs; one is the *default agent* | Providers are detected and configured for the user, not typed in by hand. `model_scanner` already does this for Ollama/LM Studio/cloud keys; the same treatment should extend to installed agent CLIs. |
| `Super+Shift+Ctrl+A` and shell aliases summon the agent | Aether1 must be summonable by a global hotkey and a CLI, not only by clicking the tray. Presence is worth nothing if reaching it is slow. |
| `omarchy agent prompt "task"` for unattended runs | A headless entry point: `aether1 prompt "..."` answering on stdout, usable from scripts and other tools. |
| Local models via LM Studio / Ollama, installable from a menu | Already the shape of `model_scanner.rs` (probe, list models, one-click pull). Keep it and make it the recommended path, not the fallback. |
| Usage panel: plan, session/weekly limits, tokens by day and model | Aether1 has live *hardware* telemetry; it should have live *AI* telemetry beside it — spend, token counts, rate-limit headroom, which provider served which turn. |
| Crash capture routed to the agent | The strongest example of situated AI: the system notices, the agent already has the context, the user just answers a notification. Aether1's tray and telemetry loop are the right place for this. |
| A skill that lets the agent tailor the system, with plan mode first | Aether1 needs a way for the companion to *change things* — settings, personas, host config — under an explicit preview-then-confirm contract. |
| Theme sync across agents | Aether1's palettes and personas should be one identity the whole system follows, not a per-app setting. |

## Principles

1. **Resident, not launched.** The default state is running: tray icon, hotkey, daemon.
   Anything that requires opening an app first has failed the test.
2. **Local first, cloud by choice.** A machine with Ollama and nothing else must be fully
   functional, including offline standby TTS. Cloud keys are an upgrade path, never a
   requirement.
3. **Situated in the host.** Telemetry, crashes, logs, and the user's own configuration
   are the companion's native context. An answer that ignores what the machine is
   currently doing is a worse answer.
4. **Acting requires consent.** Every system-touching action is proposed as a plan the
   user approves before execution, and every executed action is logged and reversible
   where possible. This is the price of letting an agent near the host.
5. **Persona is presentation, capability is shared.** Halcy, R.E.D. 9000, Nexus, the ARX
   nodes — they change voice and manner, never what the system can or will do.
6. **One binary, no install ritual.** The Rust/Tauri core plus a static frontend, with
   the setup script and `.tar.gz` bundle doing the rest.

## Where we are today

Already built:

- Multi-provider engine (`src-tauri/src/llm/providers.rs`) — Ollama, OpenAI-compatible,
  Gemini, Anthropic.
- Provider auto-detection and one-click model pulls (`src-tauri/src/model_scanner.rs`).
- Cross-platform host telemetry over a WebSocket (`src-tauri/src/llm/telemetry.rs`,
  `server.rs`).
- Personas with per-persona voices and identity forging from a free-text purpose
  (`llm/persona.rs`, `llm/genesis.rs`).
- Persistent local memory and settings in SQLite (`llm/db.rs`).
- Neural TTS plus browser speech recognition (`llm/tts.rs`, `frontend/js/voice.js`).
- Holographic avatars and HUD (`frontend/js/hologram/`).
- Tray presence with status colours, plus setup/start/package scripts.

Missing, relative to the north star:

- No global hotkey and no CLI entry point — the companion can only be reached by hand.
- The companion cannot *do* anything to the host: no tool calls, no file or command
  execution, no settings changes on its own.
- No awareness of the coding agents already installed on the machine.
- No AI-side telemetry (spend, tokens, limits) beside the hardware telemetry.
- No crash or log capture feeding the companion context.
- Nothing streams: replies arrive whole, which reads as latency in a voice interface.
- No packaged distribution channel (AUR/`.deb`/Omarchy menu entry).

## Target architecture

Four layers, in dependency order:

1. **Presence** — daemon, tray, global hotkey, `aether1` CLI, headless `prompt` mode.
2. **Context** — host telemetry, crash/journal capture, session memory, and the user's
   configuration, assembled into the prompt as structured context rather than prose.
3. **Capability** — a tool/action layer the model can call: read a file, run a vetted
   command, change an Aether1 setting, adjust host config. Every tool declares whether it
   mutates, and mutating calls go through plan → approve → execute → log.
4. **Presentation** — persona, voice, avatar, HUD. Swappable without touching 1–3.

## Roadmap

**Phase 1 — Reachability.** Global hotkey to summon/dismiss the HUD; an `aether1` CLI
with `prompt`, `status`, and `say`; streaming responses end to end (provider → WebSocket →
TTS) so speech starts before generation finishes.

**Phase 2 — Capability.** A tool-call layer with the plan/approve/execute/log contract;
a first tool set covering Aether1's own settings, read-only host inspection, and a vetted
command runner. Native tool-calling wire formats for the providers that support it.

**Phase 3 — Situation.** Crash and journal capture wired to the tray, offering the
companion as the first responder with the failing process already in context. AI telemetry
panel — provider, model, tokens, spend, rate-limit headroom — rendered next to the hardware
HUD it already resembles.

**Phase 4 — Ecosystem.** Detect installed agent CLIs (Claude Code, Codex, OpenCode, and
friends) and let Aether1 hand a task off to one, then narrate the result. Ship an
Aether1 skill so those agents can configure Aether1 the way Omarchy's skill configures
Omarchy. Publish packaging (AUR, `.deb`, Omarchy-menu-friendly install) so a fresh machine
is one command away.

**Phase 5 — Continuity.** Memory that survives and syncs across machines; persona and
palette as a system-wide identity other tools follow.

## Non-goals

- Not a coding agent. Aether1 orchestrates and narrates; the coding agents already do
  coding better than a companion shell around them would.
- Not a cloud service. No account, no telemetry leaving the machine, no server-side
  storage of memory.
- Not a desktop environment, window manager, or distribution. Aether1 runs *on* your
  setup; it does not replace it.
- Not a general chat client. If a feature has no relationship to the host machine, it
  probably belongs somewhere else.

## Open questions

- How far does the vetted command runner go by default — read-only until explicitly
  widened, or a curated allowlist out of the box?
- Do we implement provider-native tool calling per provider, or normalise on one
  OpenAI-compatible shape and lose fidelity on the others?
- Does Aether1 host an MCP client (borrowing the whole existing tool ecosystem) instead of
  growing its own tool set?
- What is the Windows story for phases 2–3, where crash capture and command execution are
  the most Linux-shaped parts of the design?
