# Aether1 Implementation Plan

How to get from what's in this repository today to what [GOALS.md](GOALS.md) describes,
one shippable change at a time.

Each step below is sized to be a single PR: it names the files it touches, what changes in
them, and how you know it worked. The order matters — later steps assume the earlier ones
exist. Nothing here is a rewrite; every step extends the modules that are already there.

## Ground rules this codebase imposes

These constrain every step, so they're worth stating once:

1. **Two transports, one implementation.** The Tauri IPC path (`main.rs` commands) and the
   headless axum path (`server.rs` routes) both call the same plain functions in
   `commands.rs`. Any new capability goes in `commands.rs` first, then gets a thin wrapper
   in *both* `main.rs` and `server.rs`. A feature that exists on only one path is a bug.
2. **The frontend branches at every call site.** `frontend/js/app.js` picks between
   `tauriInvoke(...)` and `apiFetch(...)` on `IS_TAURI`. New API surface needs both arms.
3. **Offline must never break.** `Persona::offline_reply` is the floor. Every step must
   leave a machine with no network and no local model still working.
4. **Schema changes live in one place.** `MemoryDb::open` runs a `CREATE TABLE IF NOT
   EXISTS` batch (`llm/db.rs`); new tables go there, and it must stay safe to run against
   an existing database.
5. **Keep the per-module tests.** `persona.rs`, `genesis.rs`, `db.rs`, `tts.rs`,
   `model_scanner.rs` and `llm/mod.rs` each carry a `#[cfg(test)]` block. New modules get
   one too.
6. **Providers are blocking `ureq` calls** inside a synchronous `generate_response`.
   Streaming and tool loops change that shape — step 3 is where that gets confronted.

### What "local" means here

Settled vocabulary, because these three get used interchangeably everywhere else and the
difference decides what the code is allowed to reach:

| Term | Means |
| --- | --- |
| **Local host** | This system. The machine Aether1 is running on, and nothing else. |
| **Local network** | The LAN. Machines reachable without crossing the internet. |
| **Local LLM** | An edge or offline model, running on the local host **or** within the local network. No further. |

"No further" is the operative half. A model behind someone else's API is not a local LLM
however it is billed, and the local-only paths must never reach one. The boundary is the
LAN, and it is deliberately drawn there for now rather than at the host: models on other
machines you own are in scope, models anywhere else are not.

Use these words in code, comments, settings labels and documentation, and say which one you
mean. "Local" on its own is the ambiguity this table exists to remove.

---

## Phase 1 — Reachability and flow

### Step 1: `aether1` CLI and headless prompt — **shipped**

*Why first:* it's the cheapest real capability, and it forces the "one implementation, two
transports" discipline before there's much to keep in sync.

Landed as `src-tauri/src/cli.rs`: `prompt` (with `--session`, or text piped on stdin),
`status` (`--json`), `say` (`--voice`, `--no-play`), plus `--help`/`--version`. `main()`
now parses argv once and either runs a one-shot command, serves, or launches the app.
`build_llm_engine` creates `backend/` before opening the database, so a headless run on a
fresh checkout shares the HUD's memory instead of silently falling back to a temp file.

- **`src-tauri/src/main.rs`** — `main()` already checks for `--serve`. Extend that argument
  parse into a small subcommand match: `prompt <text>` (print the reply to stdout and
  exit), `status` (print `Telemetry::snapshot().diagnostic_report()`), `say <text>`
  (synthesize and play, or write the wav path). Each arm builds the engine via the existing
  `build_llm_engine()` and calls `commands::generate_response` / `commands::static_info` /
  `commands::synthesize_speech`. No new engine code.
- **`scripts/install_desktop_app.sh`** — already installs the binary as
  `~/.local/bin/aether1`, so the CLI needs no extra install step; it just tells you the
  commands exist, and warns when `~/.local/bin` isn't on `PATH`.
- **Verify:** `aether1 prompt "what's my CPU doing"` answers from a cold start with no HUD
  open, and `aether1 status` matches what the HUD shows.

### Step 2: Global hotkey — **shipped**

- **`src-tauri/Cargo.toml`** — add `tauri-plugin-global-shortcut`.
- **`src-tauri/src/main.rs`** — register the plugin in the builder; in `setup()`, bind a
  default chord (suggest `Super+Shift+A`) to toggle the main window's visibility and focus,
  reusing the same show/hide logic the tray's `show` menu item already uses.
- **`src-tauri/src/llm/db.rs` settings** — store the chord as a setting
  (`hotkey_toggle`), so it's editable rather than compiled in.
- **`frontend/js/app.js`** — a field in the settings panel to change it; re-register on
  save.
- **Verify:** the HUD appears and disappears on the chord from any application, and
  survives a restart with a custom chord set.

Landed as `src-tauri/src/hotkey.rs`, with the chord in the `hotkey_toggle` setting
(`Super+Shift+A` by default, empty to disable) and re-registered live when Settings is
saved. A chord that won't parse is a warning, not a startup failure.

The plan missed one thing: **an application cannot grab keys system-wide on Wayland**,
which is most of this project's target audience. So the step also added
`tauri-plugin-single-instance` and the `aether1 show` / `aether1 toggle` subcommands — a
second launch hands its argv to the running instance instead of starting a second one, so
a compositor keybinding bound to `aether1 toggle` does what the global hotkey does on X11.
Startup says so explicitly when it detects a Wayland session. The plugin also stops a
double launch from producing two tray icons.

### Step 3: Streaming end to end — **shipped**

*The largest refactor in Phase 1. Do it before tools — a tool loop on top of a
non-streaming call is much harder to retrofit than the other way round.*

- **`src-tauri/src/llm/providers.rs`** — add a streaming variant per provider. Each
  currently does one `send_json(...).read_json()`; streaming means reading the response body
  line by line (`stream: true` for Ollama, SSE for the OpenAI-compatible/Anthropic/Gemini
  shapes) and invoking a `&mut dyn FnMut(&str)` callback per delta. Keep the existing
  one-shot functions — they become the fallback when a provider or model can't stream.
- **`src-tauri/src/llm/mod.rs`** — `generate_response_streaming(prompt, session_id, sink)`
  alongside the current method. It accumulates the full text for `record_usage` and
  `add_message` exactly as now, so persistence behaviour doesn't change.
- **`src-tauri/src/commands.rs`** — a streaming entry point taking a sink closure.
- **`src-tauri/src/main.rs`** — emit `chat-delta` Tauri events. **`src-tauri/src/server.rs`**
  — an SSE route (`/api/chat/stream`) or reuse of the existing WebSocket.
- **`frontend/js/app.js`** — `appendMessage` grows an "open" message node that deltas append
  into; `handleSendMessage` subscribes instead of awaiting a whole reply.
- **`frontend/js/voice.js`** — speak on sentence boundaries as they complete rather than
  once at the end. This is the point of the whole step: the companion starts talking
  immediately.
- **Verify:** first token visible in well under a second against a local Ollama model, and
  speech begins before generation finishes.

Landed with `generate_response_streamed` as the *only* path through the providers:
`generate_response` was removed rather than kept beside it, so the blocking callers (the
CLI, `/api/chat`) are streaming calls that discard their deltas, and the two can't drift.
Each provider has a `stream_*` beside its `call_*`, sharing one payload builder; a stream
that fails before producing any text silently retries the blocking call, which is what
keeps a provider or model that can't stream working. A stream that fails *after* text has
reached the operator keeps the partial reply and appends a notice — restarting over the
top of what they are already reading would be worse.

Two additions the plan didn't list: `/ws/chat` for the browser path (a WebSocket, because
the socket plumbing already existed for telemetry, and the client sends the prompt on the
same connection), and `/api/tts`, without which the browser path had no way to synthesize
a single sentence — it only ever got whole-reply audio bundled into `/api/chat`. Auto-speak
now synthesizes per sentence on both paths and queues the clips in order, so speech starts
mid-generation; the whole-reply replay button synthesizes lazily on first click instead.

### Step 3a: Local speech synthesis — **shipped**

*Why now:* `llm/tts.rs` opens a WebSocket to a Microsoft endpoint. Every spoken reply
therefore needs the network, which makes the offline claim in GOALS.md false, and sends the
text of everything the companion says to a third party — including, once tools land,
things it read off your disk.

- **`src-tauri/src/llm/tts.rs`** — put the existing msedge-tts path behind a `TtsEngine`
  enum and add a local one. Piper is the obvious first target: small ONNX voices, a
  permissive licence, one binary, good enough quality, and it runs on a CPU. Detect it the
  way `model_scanner.rs` detects Ollama — if it's installed, prefer it.
- **Voice selection** — persona voices are currently Microsoft voice ids
  (`en-US-AriaNeural`). Map each persona to a local voice as well, so switching engines
  doesn't silently change who the companion sounds like.
- **Settings** — engine choice (auto / local / cloud), with auto meaning "local if present".
- **Verify:** with the network down and Ollama running, a spoken reply still comes out of
  the speakers.

Landed in `llm/tts.rs` as an `Engine` (Auto / Local / Cloud) with Piper as the local
engine — binary and `.onnx` voice both auto-detected, configurable when they live
somewhere unusual. Auto means "local if installed", which makes installing Piper the whole
of the setup story. `Local` refuses rather than falling back, because a silent fallback is
exactly how "it works offline" stops being true without anyone noticing. The engine is part
of the cache key, so switching it doesn't keep replaying the old voice, and `/api/audio`
now serves wav as `audio/wav` rather than claiming everything is an mp3.

### Step 3b: Push-to-talk with local recognition — **shipped**

*Why this shape:* `voice.js` uses the browser's Web Speech API, which in most browsers is a
cloud service, and auto-sends whatever it thinks it heard. Hold-a-key is both more
reliable and less alarming than a microphone permanently deciding whether you meant it.

- **`src-tauri/src/llm/stt.rs`** (new) — whisper.cpp via a small local binary, or
  `whisper-rs` if the build stays manageable. Record while a key is held, transcribe on
  release.
- **`src-tauri/src/hotkey.rs`** — a second registered chord for talk. Press-and-hold starts
  capture, release ends it; the same Wayland caveat and the same `aether1 talk` fallback
  for compositor keybindings.
- **Frontend** — the HUD shows listening state while held (the tray already has a Listening
  colour), then the transcript appears in the input and is sent.
- **Keep the browser path** as the fallback for `--serve` in a browser, where no local
  microphone capture is available to the Rust side.
- **Verify:** hold the key, speak, release, and the answer starts coming back — with the
  network down.

Built differently from the plan, and better for it. The plan called for a Rust-side
recorder (`cpal` or similar) plus an OS-level press-and-hold chord. Instead the capture
happens in the page — `getUserMedia` → Web Audio → a 16 kHz mono WAV assembled in
JavaScript — and is posted to `/api/stt` (or `transcribe_rust`) to be transcribed by a
local whisper.cpp. That keeps microphone access in the one place both the webview and a
browser already have it, adds no native audio dependency to the Rust build, and works
identically on both transports. Hold **Space** anywhere outside a text field, or hold the
mic button.

The Settings panel now says plainly whether speech in and out are local, and names what is
missing when they aren't, rather than leaving the operator to discover it by pulling the
cable.

**Still cloud-shaped:** holding a key while the HUD is *unfocused* needs an OS-level
press-and-hold, which the global-shortcut plugin doesn't express. That is a follow-up, and
the browser Web Speech path remains as a fallback where no local model is installed.

---

## Phase 2 — Hands

### Step 4: Tool registry and action log (no tools yet) — **shipped**

- **New `src-tauri/src/tools/mod.rs`** — a `Tool` describing `name`, `description`, a JSON
  Schema for its arguments, a `mutating: bool`, and `fn call(&self, args: &Value) ->
  Result<String, String>`. A `Registry` that holds them, serialises their schemas for a
  prompt or an API tool block, and looks one up by name.
- **`src-tauri/src/llm/db.rs`** — add to the `CREATE TABLE` batch:
  `action_log(id, ts, tool, args_json, result_json, status, approved_by, undo_json)`.
  Add `log_action` / `recent_actions` / `get_action` methods.
- **Settings** — `tools_enabled` (default **false**) so nothing changes until you opt in.
- **Verify:** unit tests only. Registry round-trips a schema; the log writes and reads back.
  Nothing is user-visible yet, and that's fine.

Landed as `src-tauri/src/tools/mod.rs` (the `Tool` trait, `Outcome`, `Registry`, and a
process-wide `registry()` that is deliberately empty) plus the `action_log` table and its
read/write methods in `llm/db.rs`. `tools_enabled` defaults to false.

Two departures from the sketch. Actions are logged *before* they run rather than after, so
a call that panics or hangs still leaves a trace of having been attempted. And rather than
leaving the whole module unreachable, the catalog and the log are exposed on both
transports now (`get_tools_rust` / `get_actions_rust`, `GET /api/tools` / `GET
/api/actions`), so the shape the settings UI and the approval history will read is settled
and exercised before anything depends on it. The catalog answers
`{"enabled": false, "tools": []}` today, which is the honest description of a companion
that cannot yet do anything.

### Step 5: The tool loop, read-only tools only — **shipped**

- **`src-tauri/src/tools/`** — first tools, all `mutating: false`: `read_file`,
  `list_dir`, `list_processes`, `telemetry_detail`, `search_memory`. Each one small, each
  one tested.
- **`src-tauri/src/llm/mod.rs`** — turn the single provider call into a bounded loop (cap
  at ~6 rounds): call the model, parse tool calls out of the reply, execute the
  non-mutating ones, append results to the context, call again. Start with a **prompt-level
  protocol** — the tool list rendered into the system prompt, the model replying with a
  fenced JSON block — because that works on every provider including small local models.
  Native tool calling comes in step 8.
- **Path safety:** `read_file` and `list_dir` resolve and canonicalise against an allowed
  roots list (home directory and below by default, `/etc` read-only, never `~/.ssh` or the
  Aether1 database itself). This is the same class of check `server.rs`'s `get_audio`
  already does with its basename sanitisation — extend that habit rather than inventing a
  new one.
- **Verify:** with `tools_enabled` on, "what's in my downloads folder" is answered from a
  real listing, and every call appears in `action_log`.

Landed as `tools/builtin.rs` (`read_file`, `list_dir`, `list_processes`,
`telemetry_detail`, `search_memory`), `tools/fs_guard.rs`, `tools/protocol.rs`, and
`LlmEngine::tool_loop`. A checkbox in Settings turns it on; off, the turn is byte for byte
what it was before tools existed.

The part the plan under-described is streaming. A tool call arrives token by token like
everything else, so the fence has to be recognized mid-stream and withheld, or the
operator watches their companion type JSON at them. `FenceFilter` does that: prose streams
through, the block is swallowed, and a one-line trace (`⚙ read_file /etc/hostname`) is
shown in its place. What gets stored as the reply is what the operator saw, not the raw
text — the history should read the way the conversation looked.

`tools::run` refuses a mutating tool outright rather than trusting that none is
registered, so the consent path can't be bypassed by a future tool being added carelessly.
A call refused on its name, its arguments, or the path guard never reaches the log as
having happened; a call that runs and fails is logged as failed, with the reason.

### Step 6: The consent path

*This is the step that makes mutating tools safe, so it lands before any of them.*

- **`src-tauri/src/tools/consent.rs`** — a pending-action store: `propose(tool, args)`
  returns an id and a human-readable rendering of what will happen; `approve(id)` executes
  and logs; `reject(id)` logs the refusal. Pending actions expire.
- **`src-tauri/src/commands.rs`** — `pending_actions`, `approve_action`, `reject_action`;
  wrappers in `main.rs` and `server.rs`.
- **`src-tauri/src/llm/mod.rs`** — in the loop, a mutating tool call doesn't execute. It
  proposes, and the model is told the action is awaiting approval.
- **`frontend/js/app.js` + `frontend/css/A1theme.css`** — an approval card in the chat
  stream: the tool, the rendered arguments, Approve / Reject, and a per-tool "always allow"
  that writes to a settings allowlist.
- **Verify:** a proposed action does nothing until approved; rejecting it is recorded;
  "always allow" survives a restart.

### Step 7: Mutating tools and undo — **shipped**

- **`src-tauri/src/tools/`** — `set_aether_setting`, `write_file` (snapshot the previous
  contents into `undo_json` first), `run_command` (allowlist-gated, no shell interpolation,
  argv only, with a timeout), `service_control` (allowlist-gated).
- **`src-tauri/src/tools/mod.rs`** — an optional `undo` on `Tool`, and
  `commands::undo_action(id)` replaying it from `undo_json`.
- **`frontend/js/app.js`** — an action history panel listing recent actions with an Undo
  button where one is available.
- **Verify:** the companion changes its own persona on request via the approval flow; a
  written file can be reverted from the history panel.

Landed as `tools/mutating.rs` — `write_file`, `set_aether_setting`, `run_command` — plus
`fs_guard::resolve_writable`, `Tool::undo`, `consent::undo`, and an Activity panel in the
HUD. Notes on what was decided:

- **The write guard is much narrower than the read guard.** Reading `/etc` tells the
  companion how the machine is configured; writing there changes how it boots. Writes are
  confined to the home directory, and it is the file's *parent* that gets canonicalized,
  because the file may not exist yet.
- **`write_file` refuses to overwrite a file it cannot read as text.** The undo payload is
  the previous contents, so a binary file is one it could not put back.
- **`set_aether_setting` cannot reach `tools_enabled`, `tool_always_allow`,
  `command_allowlist` or `llm_api_key`.** Nothing should be able to widen its own
  permissions, and there is a test that says so by name.
- **`run_command` has no shell** — argv only, so pipes, redirects and globs are text — the
  program must be on an allowlist that starts empty, and the name may not contain a path
  separator, so nothing gets smuggled in as `./curl`. It is also the one tool that admits
  it cannot be undone rather than pretending.
- **`run_command` cannot be pre-approved at all.** Always-allow is per *tool*, which only
  means something when the tool's name tells you roughly what it will do. `write_file`
  qualifies — the path guard has already decided where it may write. `run_command` does
  not: ticking "stop asking" once would silently pre-approve every allowlisted program,
  with any arguments, from then on, which is the whole of the permission the allowlist
  exists to hand out one call at a time. `Tool::always_allowable` says so, the run gate
  asks the tool before it consults the operator's list, and the approval card shows a
  line explaining it instead of a checkbox.

**Not built:** `service_control`. Starting and stopping system services is `run_command`
with a longer name once an allowlist exists, and it deserves its own guard rather than
being tacked on here.

### Step 8: Native tool calling

- **`src-tauri/src/llm/providers.rs`** — real `tools` / `tool_use` blocks for Anthropic,
  the OpenAI-compatible shape, and Gemini, each with typed request/response structs the way
  the existing calls are built. Keep the step-5 prompt protocol as the automatic fallback
  for Ollama and any model that tool-calls badly.
- **Verify:** the same conversation produces the same actions on a cloud provider and on a
  local model, one through native tool calls and one through the text protocol.

---

## Phase 3 — The vault

*Changed from the original plan.* Steps 9–12 were written around growing the SQLite store:
schema columns for provenance, an FTS index, a consolidation pass, and a memory browser in
the HUD. The better answer, borrowed from
[fullstack-agent](https://github.com/jaredrhod/fullstack-agent), is that durable memory
should be **a folder of plain text notes** with an index at its root. It is inspectable
with tools you already have, syncable with git or Obsidian, portable to any other assistant,
and it makes the memory browser unnecessary. It also reuses the tools from step 5 —
`read_file` and `list_dir` are already how the companion would read it.

SQLite keeps chat transcripts, settings, and the action log.

### Step 9: The vault, and priming from it — **shipped**

- **New `src-tauri/src/vault/mod.rs`** — resolve the vault path (setting `vault_path`,
  default `~/Aether1Vault`), create it on first run with a starter layout:
  `INDEX.md` (what is here, and which notes matter for which kind of question),
  `profile.md` (who the operator is), `machine.md` (what this box is),
  `projects/`, `daily/`, `actions/`.
- **Notes link to each other with `[[wiki links]]`**, because that is what turns a folder
  into a graph an existing mind-map app can draw. Obsidian's graph view then *is* the
  visualization of the companion's mind, at no cost to us and with better layout, search
  and editing than we would ever build natively. Note-writing tools (step 10) link
  deliberately rather than incidentally: a new project note links to the profile, a daily
  note links to what it touched.
- **`llm/mod.rs`** — `system_prompt` currently pastes the first ten `long_term_memory`
  rows. Replace that with the vault's `INDEX.md` plus any notes the index marks as
  always-loaded. That is the whole priming mechanism: the index tells the model what exists
  and how to reach it, and `read_file` does the reaching.
- **Migration** — existing `long_term_memory` rows are written out as
  `vault/imported-memories.md` on first run, once, and the table is left alone.
- **Verify:** a fact written into `profile.md` by hand shows up in the companion's answers
  in the next conversation, with no restart and no import step.

Landed as `src/vault/`. `system_prompt` no longer pastes the first ten `long_term_memory`
rows: it carries the index and the always-loaded notes, and the model reaches for the rest
with `read_file`. Old rows are *copied* into `imported-memories.md` rather than migrated,
so a bad import loses nothing and the note can simply be deleted. `list memory` now
describes the vault and points at the folder instead of dumping key-value pairs.

An existing note is never overwritten, so pointing the vault at a folder you already have
is safe.

### Step 10: Writing back — **shipped**

- **New tools** (mutating, so they land after step 6's consent path):
  `append_note`, `write_note`, `update_index`. `remember that …` becomes an append to the
  right note rather than a key-value row.
- **Session close** — a short note per conversation in `daily/`, written by the model
  itself: what was discussed, what was decided, what changed.
- **Verify:** a week of conversations leaves a readable trail of notes, and the index knows
  about them.

Landed as `tools/notes.rs`: `append_note` and `write_note`, both mutating, so what the
companion decides to record about you is proposed and waits — an approval card is a chance
to correct a fact before it becomes one. `remember that …` is the exception and writes
straight through: that is the operator's own instruction, and asking them to approve it
would be ceremony rather than consent.

Notes that get created also get linked from `INDEX.md` if they aren't already, because a
note the index never mentions is a note nothing will go looking for. A separate
`update_index` tool turned out to be unnecessary — `write_note` on `INDEX.md` is the same
operation with one fewer thing to explain.

**Not done in this pass:** step 11 (retrieval beyond the index, consolidation, decay) and
the session-close note. The index plus `read_file` is the whole retrieval mechanism today,
which is enough while a vault is small and is the thing to measure before adding search.

### Step 11: Retrieval, consolidation, decay

- **Retrieval** — start with the index plus filename/heading search over the vault, since
  that is what the priming design actually asks for. Add a real index (SQLite FTS over note
  contents, or embeddings) only if the simple thing measurably fails.
- **Consolidation** — daily notes fold into topic notes; a note that has stopped being true
  gets edited or moved to `archive/` rather than silently contradicting a newer one.
- **Verify:** with 200 notes, asking about one topic pulls that topic's notes and not the
  ten most recent.

### Step 12: Vault in the HUD *(reduced)*

Not a memory browser any more — the browser is the operator's editor. What is still worth
building is small: show which notes were loaded for the current answer, and a button that
opens the vault folder. Seeing *why* it said something matters more than another file list.

## Phase 4 — Situation

### Step 13: Crash and log capture

- **New `src-tauri/src/watchers/crash.rs`** — watch `systemd-coredump` (or poll
  `coredumpctl list`) on Linux; feature-gate Windows out for now.
- **`src-tauri/src/main.rs`** — on a crash, set the tray to amber and post a notification
  offering the companion as first responder, with the failing process, its exit signal, and
  the tail of its journal already assembled as context.
- **Settings** — an off switch and a per-program mute list.
- **Verify:** a deliberately segfaulted test binary produces a notification whose
  conversation starts with the crash already in context.

### Step 14: Honest AI telemetry

- **`src-tauri/src/llm/mod.rs`** — `estimate_tokens` is a 4-chars-per-token guess. Every
  provider returns real usage counts; parse them in `providers.rs` and feed
  `record_usage` actual numbers, falling back to the estimate only where a provider gives
  nothing.
- **`frontend/js/app.js`** — extend the existing token panel with provider, model, and
  (for cloud providers) spend.
- **Verify:** the counter matches the provider's own dashboard within rounding.

---

## Phase 5+ — Reach and continuity

Sketched rather than specified, because the earlier steps will change what these should be:

- **Step 15: agent handoff** — detect installed CLIs (`which` is already a dependency),
  hand a task to one, narrate the result in persona.
- **Step 16: MCP client** — if step 15 shows the tool ecosystem is worth borrowing, an MCP
  client behind the same consent path, so external tools inherit the approval flow rather
  than bypassing it.
- **Step 17: packaging** — AUR and `.deb` beside the existing `.tar.gz`, so a fresh machine
  is one command.
- **Step 18: memory sync** — an export/import format first, sync second, both under your
  control and neither touching a server we run.

---

### Step 18a: A fullscreen face

Cheap, and it changes how the thing feels: a fullscreen route that shows only the avatar
and its state (idle / listening / thinking / speaking), for a second monitor or a spare
screen. The hologram renderer and the state machine already exist; this is a layout and a
CLI flag (`aether1 face`) away.

### Step 18b: Separable parts

`fullstack-agent` ships its four pieces as repos you can take one at a time, and that is
worth copying as a shape even inside one binary: the vault should be usable by another
tool, the visualizer runnable on its own, the voice stack callable from a script. Concretely
that means the vault format is documented and stable, `aether1 face` and `aether1 say`
work without the HUD running, and nothing in the vault depends on Aether1 to be readable.

### Step 19: several local models, and choosing between them

**A stated core requirement, deliberately not built yet.** Recorded here so the shape of it
is known while earlier steps are designed, not so it gets started early.

The intent: the backend draws on more than one local model at a time, and sends a task to
whichever suits it -- a small fast one for a summary or a routine reply, a larger one for
reasoning, a code-specialised one for code -- with the choice improving over time from what
actually worked rather than from a table someone wrote once. **Local only.** No cloud
provider in this path at all; that is the point of it.

What already exists and points this way:

- `model_scanner.rs` finds local servers by probing loopback ports and identifies them by
  the API they speak, so *discovering* several at once is already solved.
- Providers are already an enum with one call shape, so *talking* to several is a matter of
  holding more than one configured at a time rather than new protocol work.
- `action_log` already records what the companion did and how it turned out, which is the
  raw material a routing decision would have to learn from.

The open questions, in the order they will bite:

1. ~~**Where the models live.**~~ **Settled: the local host and the local network, no
   further** (see "What 'local' means here" in the ground rules). Both are in scope, which
   makes this two features rather than one -- the host case needs no network exposure at
   all, the LAN case needs authentication before it needs anything else. See below.
2. **What "better suited" means.** A hand-written rule ("code goes to the code model") is
   knowable and debuggable. Learned routing needs a signal for what "worked", and the honest
   answer today is that we do not have one -- a reply the operator did not complain about is
   not the same as a good reply. This probably starts as declared strengths per model plus
   the operator's own corrections, and only becomes learned once there is something real to
   learn from.
3. **What the operator sees.** If a task silently goes to a different model, the reply's
   character changes for no visible reason. Which model answered has to be visible, and
   overridable, or the companion stops feeling like one thing.

**On network exposure.** Now that the LAN is confirmed in scope, this is no longer a
question but an ordering constraint.

Models on the **local host** need nothing beyond the current loopback bind: the scanner
already finds them, and `--serve` stays closed to the outside.

Models on the **local network** are a different feature with a different threat model, and
it is a bigger ask than pointing at an address. Two distinct problems, and the second is the
one people forget:

- *Aether1 reaching out* to a model server on another machine is an outbound call, and needs
  little more than an address plus a way to say which addresses are allowed.
- *Aether1 being reachable* is the dangerous half. The HTTP server has no authentication of
  any kind (see `bind_address` in `server.rs`), so anything that opens it to the LAN opens
  the conversation, the action log and the approval endpoints to everyone on it.

**Authentication is therefore step one of the LAN half, not a hardening pass afterwards.**
A shared secret the operator sets, checked on every route, is the minimum. Build the routing
on top of that, never before it -- and keep the two halves separable, so the host-only case
ships without waiting for the network one.

### Step 20: local-only mode — **shipped**

The second stated goal was Aether1 running on local systems without needing the internet.
Most of it was already true, but by accident rather than by decision: the HUD's assets are
vendored, the scanner probes loopback, whisper.cpp listens locally and the vault is a
folder of files -- so an install with Piper present and no cloud key configured never
touched the network, and an install without Piper narrated everything it said to Microsoft.
Both were the same build with the same settings. "Offline" was an emergent property of what
happened to be installed, which means it could not be checked and could change underneath
the operator when a dependency went missing.

`local_only.rs` makes it a stated mode instead. One setting, read fresh at every decision
(`local_only::enabled`), with `AETHER1_LOCAL_ONLY` in the environment able to force it on
where the setting alone is not enough. With it on:

| Path | Before | With the mode on |
| --- | --- | --- |
| Speech | `auto` fell back to Microsoft's Read Aloud when Piper was absent | Piper or nothing, and the error says which is missing |
| Update check | GitHub API call on every launch | Skipped, and the tray says so rather than looking broken |
| Applying an update | `git pull` | Refused |
| Cloud providers | Called if configured | Refused; a local answer plus a line naming who was not contacted |
| A cloud key in the environment | Silently promoted an "offline" install to a cloud one | Not scanned for |
| Model pulls | Downloaded through Ollama | Refused |
| `set_aether_setting` | Could set `llm_provider` to a cloud one | Refused, so the mode has no one-approval exit |

Two decisions worth keeping in mind:

**The boundary is the endpoint, not the provider name.** `llm_provider = "ollama"` is
allowed -- a model server on the LAN is the point of the platform -- but `llm_endpoint`
pointed at a rented box on the internet is a cloud call wearing a local provider's name.
`is_local_endpoint` decides it by parsing the host: loopback, the private ranges,
link-local, `localhost` and `.local` are in, everything else is out, and anything
unparseable fails closed. Names are judged as written rather than resolved, because asking
DNS where a host is, in order to decide whether we may talk to the internet, is not a
question worth asking.

**Refusing still answers.** Every refusal produces the local reply plus one line saying
what was not contacted. A companion that goes mute the moment the mode is switched on
teaches the operator to switch it back off.

It is off by default: turning it on for everyone would break every operator who chose a
cloud provider deliberately. It is absent from `SETTABLE` in `tools/mutating.rs`, so the
companion cannot switch it off about itself -- the same rule `tools_enabled` follows.

This does not close step 19, and is not meant to. It draws the line step 19 has to stay
inside.

### Step 21: Solar and Eclipse, and starting from the system — **shipped**

The HUD shipped with eight colour themes, six of them cyberpunk and two — corporate-light
and corporate-dark — meant to look like ordinary software. "Ordinary software" was the
whole specification, so they were only ordinary in the sense of being restrained: a Tailwind
blue on a Tailwind slate, resembling nothing in particular.

They are now **Solar** and **Eclipse**, and the thing they resemble is the Windows 11 shell:
`#f3f3f3` / `#202020` grounds, `#0067c0` / `#60cdff` accents, hairline borders at a tenth
opacity, 8px corners, shallow shadows, and labels that are neither uppercase nor
letter-spaced.

Two things made this more than a table of hex values.

**The markup names its own colours.** Roughly fifty hardcoded utility classes —
`bg-slate-950/80` on the header, `text-cyan-400` on every panel heading,
`border-cyan-500/30` throughout — bypass the theme variables entirely. That is invisible
while all eight themes sit on near-black, and obvious the moment one turns white. A block at
the end of `A1theme.css` maps each family onto the theme's own variables, with the avatar and
theme pickers excluded because their labels are coloured to preview what they select.

**The first paint is not a choice.** The HUD now opens matching the operating system's
light/dark setting, and keeps following it until someone picks a theme — at which point
the choice is saved and the OS stops being consulted. Separating those two states is the
whole design: `applyColorTheme` gained a `chosen` flag so that painting a theme and choosing
one are different acts, since otherwise the startup paint would save the system's answer as
if it were the operator's and the mode would end on the first frame. `frontend/js/theme.js`
holds the rule, because the HUD, the desktop sprite and the avatar workbench all need to give
the same answer, and each used to hardcode its own.

Anyone who had picked corporate-light or corporate-dark keeps their choice: the old names are
translated to the new ones on read, rather than being treated as unknown and silently reset.

### Step 22: a theme is a mode and three colours — **shipped**

Step 21 renamed two themes and repainted them. This replaces what a theme *is*.

There were eight themes, each a hand-written block of CSS custom properties. Eight blocks meant
eight looks and no ninth, and it meant "pick a theme" was the only control anyone had. The HUD
now separates the two things that block was conflating:

**Mode** — `solar`, `eclipse` or `cyberpunk` — decides the chrome, and is the only thing left
in `data-theme`. Cyberpunk needs no rules of its own; it is what the stylesheet already did.
Solar and Eclipse are the departure, and every rule keyed on them is that subtraction plus the
Windows-shell details that replace it.

**Colours** — background, main, highlight — are three values, and `variablesFor()` in
`theme.js` derives every property the stylesheet reads from them. That function is the eight
deleted blocks: they were eight hand-written answers to it. The presets in `palettes.js` are
now just named bundles of a mode and three colours, so the six neon themes are colour choices
rather than privileged themes, and three colour pickers in Settings can do anything a preset
can — including to Solar and Eclipse, which was the point.

Three things this forced, each worth more than the refactor:

**The markup's hardcoded colours had to go.** Around fifty utility classes name their colours
directly and bypass the theme — `text-cyan-400` on every heading, `bg-slate-950/80` on the
header. That was survivable while every theme was a dark ground with a cyan-ish accent. It is
not survivable when picking amber is supposed to turn the HUD amber. Every family is now mapped
onto the live variables, keyed on the bare `[data-theme]` attribute so all three modes get it
and the no-JavaScript fallback does not.

**Light and dark are measured, not declared.** Every light-or-dark decision asks the
background's luminance rather than which mode is active, so a Solar with a dark background
still gets light text. `color-scheme` is set from the same answer, which is what stops the
browser's own furniture — scrollbars, the list a `<select>` opens — staying dark on a white
page.

**The avatar needed a second way in.** `setColorTheme(id)` is a documented contract that
third-party avatar files depend on, and a hand-mixed set of three colours has no id.
`setColorPalette(palette)` takes the palette directly; the id-based call is kept and now routes
through it.

Each mode remembers its own colours, because carrying them across is what you must not do:
Solar with Cyberpunk's near-black ground is not a light theme, it is a broken one. Saved
single-word themes from before this — including `corporate-light` and `corporate-dark` from
before step 21 — are translated to a mode and a colour set on read.

### Step 23: the stage is always dark, and the workbench wears your theme — **shipped**

Two things step 22 left wrong.

**The avatar's panel followed the theme, and should not.** Under Solar the projection bay went
white with the rest of the window, and a hologram on white reads as a picture of a hologram.
`--viewport-bg` is now derived separately from the shell: black under Cyberpunk, a fixed dark
grey under a light shell, and the shell's own ground taken down under a dark one, so a custom
background still gets a bay that is recessed rather than one that fights it. The whole panel
takes it, not just the canvas — the waveform strip sits below `#hologram-viewport` rather than
inside it, so colouring only the viewport left a lit ledge under a dark bay. One surface reads
as a screen; two read as a mistake. The labels laid over it get their own light ink, since
`--text-dim` is derived for the shell's background and on Solar is a grey meant for white.

**The workbench ignored the theme entirely**, opening on the Cyan preset because that was the
hardcoded default from when it was written. It now opens on `Aether1Theme.current()` and
listens for the storage event the HUD's own writes fire, so it follows a live change — unless
something is being previewed, because a preview is a deliberate override and should not be
yanked away mid-check. Its picker gained a **Your theme (as set in the HUD)** entry to get
back. Its own controls had fixed near-blacks written when every theme was one; those follow the
theme now too.

While in there: `data-theme="halcy"` was still hardcoded on `<html>` in all three pages, naming
a theme that no longer exists and a value that is now a *mode*. Removed — JavaScript is the
only writer. That exposed a flash: with `theme.js` loaded at the foot of the body, the page
painted once in the fallback palette first, which on a light theme is a dark flash. `palettes.js`
and `theme.js` are pure data and pure computation, touching no DOM beyond `<html>`'s own
attribute, so they moved to the head and the theme is applied before anything renders.

### Step 24: the top bar says what is on screen, not every choice at once — **shipped**

The bar laid every option out flat: eight avatar pills, a theme picker, the state, and five
buttons, all in one row. Most of its width went on telling you about the seven avatars you had
not picked, and at a narrow window the right-hand buttons were simply pushed off the edge —
Settings among them.

Four changes, and each is a different kind of the same mistake.

**Only the selection is shown.** Avatar collapses to a chip carrying the current one; the
others slide out on click, positioned rather than in flow so opening one never reflows the bar
under the pointer. Theme sits beneath it. Both are the same shape, so there is one thing to
learn.

**The controls are behind one control.** SFX, Activity, Clear and Settings moved into a `☰`
menu. "Adjusts dynamically" and "never overflows" are the same requirement, and one menu
satisfies it at every width without measuring anything. Only one slide-out is open at a time;
a click elsewhere, Escape, or opening the other closes it.

**Genesis left the bar.** It opened the same Forge that Settings already shows, one tab away —
a second door onto one room, which is the kind of duplication that makes a bar look full of
features and be hard to read.

**State moved to a chin bar.** A slim strip along the bottom, always present. What the
companion is doing right now is the one thing that should never lose a fight for space, and in
the top bar it was competing with everything above.

The wordmark now carries the companion's own name once one is set, turning over to AETHER1
PLATFORM on a timer so the product underneath stays visible without spending a permanent line
on saying so. The avatar badge that used to sit beside it is gone: it is the avatar chip now,
and the same value in two places is the Genesis problem in miniature.

One pre-existing bug surfaced: the version badge was `hidden sm:inline-flex`, so the `sm:`
half overrode the `hidden` half at any normal width and the browser — which has no version to
report — showed an empty `--` forever. It now starts hidden and `initVersionAndUpdates`
reveals it, which is what that function always intended.

**Refined after first use.** The avatar and theme chips moved to the left of the bar and the
brand to the middle, with the two outer clusters equal-flex so the wordmark sits in the true
centre of the window rather than the middle of whatever space is left over. Theme stopped
being a native `<select>` and became a chip and a slide-out like the avatar — a bar with one
of each taught two interactions for the same kind of choice.

Stacking the two chips exposed a real bug in the first version: each panel was anchored to its
own chip, so the avatar's panel opened *over* the theme chip beneath it, and a click aimed at
THEME landed on an avatar instead. Both chips and both panels now share one positioning box,
so a panel opens below the pair. Worth recording because the failure was invisible in the
markup and only showed up when something was actually clicked.

### Step 25: personas are jobs, not costumes — **shipped**

The nine personas described voices. Read them side by side and they were the same assistant in
different adjectives: warm, or composed, or booming. Nothing in any of them said what a good
answer looked like, so picking one changed the tone of the reply and almost nothing else.

Each directive now has a "What that means in practice" half — a test enforces that phrase is
present in all seven speciality personas, which is a crude check for a real property. To the
Point must put the answer in the first line and not restate the question. Coding must produce
runnable code and name the failure mode. Cites Sources must say *which kind* of source a claim
has — read this session, recalled from training and unverifiable, or inferred — and never
invent one to fill the shape. Security must ask whether a target is the operator's to test.

**The list changed shape.** Tactical and Cyberpunk went: one was System Diagnosis in different
words, the other was Coding in different words. Two arrived — Default (system diagnosis and
event-log checking) and Llm (no directive at all, whatever the model brings). Both retired
keys still resolve, to the persona that absorbed them, with a test naming why.

**Settings builds its list from the enum.** `Persona::catalogue()` is served through both
transports and the `<select>` is rendered from it. The list was previously written out in the
HTML, which would have drifted from the enum the first time either moved — and this change
moved both. The unknown-key fallback moved from Conversational to System Diagnosis: an
unrecognised key is usually a stale setting, and the safer default for a companion attached to
a live machine is the one that reports on it plainly.

**Every avatar now switches persona.** It was a seven-branch if/else covering five of eight, so
A1 and hAlcy silently kept whatever was set and the pairing only half existed. It is a table
now, carrying name, persona, voice and greeting. *Your own* is the deliberate omission: an
avatar you designed has no persona of its own, and quietly selecting Custom would hand over the
fallback directive without saying so.

### Step 26: per-persona access, elevated one request at a time — **shipped**

**Designed first, then built.** The design is written out in full in
[PERSONA_ACCESS.md](PERSONA_ACCESS.md); this is the summary and why it is a document before it
is code.

Read-only tools currently run automatically for every persona, so Creative Work can read your
firewall rules and Conversational can read your source tree. Nothing connects what the
companion is *for* to what it reaches for without asking. The design gives each persona a
domain — a set of read-only tools and path roots — that runs without a prompt, and makes
everything else take the proposal path mutating tools already use. Elevation lasts exactly one
call: no mode, no session flag, no timed grant, because a grant that stays on is the current
behaviour wearing a restriction's clothes.

It is a document first because of the second-order effect. The moment a persona carries
permissions, **switching persona becomes privilege escalation** — and `set_aether_setting` can
currently change `persona_type` with a single approval that does not read like a permission
grant. So `persona_type` and `avatar` have to leave `SETTABLE`, joining the rule the entries
there already follow: the companion may not change the thing that decides what the companion
may do. That is the kind of consequence worth finding on paper.

That consequence, found on paper, is now in the code: `persona_type`, `avatar` and
`custom_directive` are out of `SETTABLE`, and the companion cannot switch its own persona,
its avatar or its own directive by any route.

The three questions the design left open are answered in PERSONA_ACCESS.md. In short:
`custom_directive` went too; elevation does **not** batch per turn — one call, and the test
`approving_an_elevation_buys_exactly_one_call` approves a call and then makes the identical
call again to prove it asks a second time; and `ProjectTree` refuses to resolve to the home
directory, so the Coding persona asks before reading files until it is started inside a project.

Settings now shows each persona's field beside its speciality, and an out-of-domain read gets a
card headed **OUTSIDE ITS FIELD** rather than **APPROVAL REQUIRED** — two different questions
deserve two different headings, or the operator learns to read neither.

Worth stating plainly: this makes Aether1 **stricter** than it was, not looser.

### Step 27: reading the event log, not just listing it — **shipped**

Step 25 named the diagnostic persona for "system diagnosis and event viewer checking", and
step 26 gave it the event log directory as part of its field. Neither made the second half
possible: `.evtx` files are binary, so `read_file` could report their size and `list_dir`
could say which logs existed, and that was the end of it.

`read_event_log` closes that. It shells out to `wevtutil qe`, newest first, with optional
filters on severity, age and source.

It is a dedicated tool rather than a `run_command` allowlist entry, and that is the whole
design decision. Reading events is a read; `run_command` is the mutating,
never-pre-approvable escape hatch. Putting `wevtutil` on the command allowlist would have
meant an approval for every single query *and* would have allowed every other subcommand of
the same program — including `wevtutil cl`, which clears a log. Here the program is fixed,
the subcommand is fixed at `qe`, and every argument is built from validated input.

The validation that earns its place: a channel name may not begin with `/` or `-`. wevtutil
takes options as `/c:20` and the channel positionally, so a "channel" spelled `/uni:true`
would have been read as an option rather than as a log — argument injection with no shell
anywhere in sight. Quotes and brackets are refused in channel and provider names for the
same reason one level up, where they would reshape the XPath query around them.

Severity filters enumerate levels (`Level=1 or Level=2`) rather than comparing (`Level<=2`),
because Level 0 means "undefined" and providers use it for informational events. A range
would have swept it into every filter, so "show me the errors" would have returned chatter.
Event Viewer's own Information filter matches `Level=4 or Level=0`; this follows it.

## Where this stands

Steps 1–5 are shipped: the companion is summonable by hotkey and from a terminal, replies
stream and are spoken as they arrive, and with inspection switched on it can read files,
list directories and check processes — every call logged, nothing able to change anything.

Next, in order:

1. **Step 6, the consent path.** It has to exist before any tool that can change the
   machine, and everything in phase 3 that writes to the vault is such a tool.
2. **Steps 3a/3b, local voice.** The correction that makes the offline claim true, and the
   change that most affects what using this feels like day to day.
3. **Phase 3, the vault.** The largest change in direction, and the one that turns a
   companion that answers well into one that knows you.
