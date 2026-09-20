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
  Native tool calling arrived in step 8; this stays as the fallback for Ollama and LM Studio.
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

### Step 6: The consent path — **shipped**

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

### Step 8: Native tool calling ✅

Built as planned. Four providers now carry the tool list in their own request format and
answer with structured calls; the step-5 fenced text protocol stays exactly as it was and is
what the two local providers still use.

- **`src-tauri/src/llm/providers.rs`** — `ChatContext` gained `tools` (the registry's
  schemas) and `exchanges` (this turn's rounds). A round is an `Exchange::Called { text,
  calls }` followed by an `Exchange::Returned(Vec<CallResult>)`, and each provider bends
  that into its own shape:
  - **Anthropic** — `tools: [{name, description, input_schema}]`; the round replays as
    assistant `tool_use` blocks and then *one* user message holding every `tool_result`.
    Splitting the results across messages teaches the model to ask for one tool at a time,
    so they stay together.
  - **OpenAI-compatible** (OpenAI, Groq) — `tools: [{type:"function", function:{…}}]`; the
    call rides on the assistant message and each result is its own `role:"tool"` message
    quoting the `tool_call_id` it answers. Arguments go back as the JSON *string* they
    arrived as.
  - **Gemini** — `tools[0].functionDeclarations`; parts carry `functionCall` and
    `functionResponse`, and `response` is an object rather than a bare string.
- Streaming is where the work is. A call does not arrive whole: OpenAI sends fragments whose
  only reliable field is `index`, and Anthropic spreads one call across
  `content_block_start`, a run of `input_json_delta` fragments, and `content_block_stop`.
  `OpenAiCallBuilder` and `AnthropicCallBuilder` reassemble them, both keyed by index so the
  reassembly does not depend on blocks never interleaving. An empty argument buffer means
  `{}` — an argument-less tool sends no fragments at all — rather than a parse failure.
- **`Provider::supports_native_tools()`** is the switch, and the two local providers are
  deliberately out. Ollama is driven through `/api/generate`, which has no tools field at
  all — tools live on `/api/chat`, a different endpoint with a different shape. LM Studio's
  server does accept tools on recent builds, but it is the provider most likely to be an
  older install on someone's desktop, and a silent 400 there costs a working setup.
- **`src-tauri/src/llm/mod.rs`** — `tool_loop` drives both paths from one list of calls. The
  differences are three: what goes into the request, where the calls are read from, and how
  the round is carried forward. A native round becomes an `Exchange`; a text round becomes
  two more history messages, as before. The `FenceFilter` is skipped on the native path,
  where withholding a fence could only swallow prose the operator should see. The system
  prompt loses the fenced-block instructions and the catalog on that path — the provider
  already has every name and schema, and repeating them is both wasted tokens and a second
  copy to drift out of date.
- Tool exchanges live only for the length of a turn. The SQLite message store stays plain
  text: nothing reads these back afterwards, so there is no migration for scaffolding.
- **Verified by unit tests against the published wire formats**, not against live providers
  — this was built in a container with no API keys. The Anthropic shapes came from the
  Messages API documentation, the OpenAI shapes from the official `openai-openapi` spec, and
  the Gemini shapes from the API's own discovery document. What the tests cover: each
  provider's tool declaration, each provider's replay of a completed round, fragment
  reassembly for both streaming formats, an argument-less call, a text block closing without
  being mistaken for a call, and a reply that is *only* a tool call not counting as an empty
  stream. **First run against a real key is the real test.**

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

*Since then:* step 11 landed the search and the archive. The session-close note is still
unwritten — see step 12, which is where showing what was loaded belongs.

### Step 11: Retrieval, consolidation, decay ✅

Built as planned, and deliberately without an index.

- **New `src-tauri/src/vault/search.rs`** — a scan, not an FTS table and not embeddings. A
  vault is a few hundred markdown files totalling a couple of megabytes; scanning it costs
  milliseconds, needs no index to keep in sync, no migration, and no second copy of the
  operator's memory in a format they cannot open. The plan said to add a real index only if
  the simple thing measurably fails, and it has not been measured failing yet.
- **The ranking is the part that matters**, and it is written against one specific failure:
  "asking about one topic pulls the ten most recent notes". Where a word appears is what
  counts — the note's own path (8), a heading (4), a body line (1, and only the first three,
  so a note cannot climb by repetition). A breadth bonus (5 per extra word matched) puts a
  note matching *both* search words above one matching either word repeatedly. Recency is a
  tiebreaker and never a reason to rank one note above a better-matching one.
- **Function words are dropped from a query**, which exists only because of the breadth
  bonus: almost every note contains "the", so without a stop list that bonus would go to
  whichever notes are longest.
- **`search_memory` now searches the vault** rather than the old key-value table. It keeps
  its name and its shape — the shape was right, only what it searched was stale. The table is
  still read when there is no vault at all, which is only true before the first run creates
  one; after that `ensure` has copied those rows into `imported-memories.md` and the vault
  search covers them, so reading both would answer the same fact twice under two names.
- **`archive_note`** (mutating, reversible) — moves a note to `archive/`, preserving its
  subpath, refusing the three always-loaded notes (those get *corrected* with `write_note`,
  not filed away), and suffixing rather than overwriting on a name collision. The index line
  is annotated `(archived)` rather than deleted: `[[wiki links]]` resolve by name, so the link
  still works after the move, and silently removing a line from a file the operator writes in
  themselves is not a thing a memory system should do.
- **Consolidation is prompted, never automatic.** Folding a fortnight of dailies into a topic
  note is a judgement about what mattered, and code that did it on its own would be rewriting
  the operator's memory without being asked. What the code does is *notice*: past 14 notes in
  `daily/`, priming carries one line telling the model to offer at a natural pause, not to
  interrupt, and not to do it without asking. The line disappears once the pile is dealt with.
- **`search_memory` added to the two machine-facing personas' domains.** `MINIMUM_TOOLS`
  already called it something every persona keeps; leaving it off `INSPECTS_THE_MACHINE` meant
  System Diagnosis and Security fell *below* the stated minimum, and "have I told you about
  this box before?" became a proposal. Searching the operator's own notes is the least
  dangerous read there is.
- **Verify — done, as a test.** `with_two_hundred_notes_a_topic_question_finds_the_topic_note`
  builds 200 daily notes written *after* the topic note, so every one of them is more recent
  than the answer, and asserts the topic note still ranks first. If recency were doing any of
  the ranking, that test fails.

### Step 12: Vault in the HUD ✅

Built as reduced: no memory browser — the operator's own editor is the memory browser. What
was worth building was the citation, and the one click that gets to the folder.

- **New `src-tauri/src/vault/consulted.rs`** — a per-turn record of which notes reached the
  answer, and by which of the three routes. A note can be `Primed` (pasted into the system
  prompt because it is always loaded), `Read` (fetched by name with `read_file`) or `Found`
  (offered by search, and possibly ignored). Those are not the same claim and the HUD does
  not render them as one: ● loaded, ◆ read, ○ found.
- **A thread-local, not a field on the engine.** A turn *is* a thread here —
  `generate_response_streamed` runs to completion on one blocking thread, and priming, the
  provider call and the whole tool loop run inside it. So two operators on the LAN cannot
  bleed into each other's footer, and none of the three recording sites needs a handle
  threaded down to it through code with no interest in reporting. Recording is off unless a
  turn opened it, so an approval executed from the HUD ten minutes later leaves nothing
  behind for the next answer to claim.
- **Three call sites**: `vault::prime` records each always-loaded note it actually pasted;
  `vault::search::search` records its shortlist; `ReadFile` records the path *if* it is
  inside the vault, which is what the new `vault::note_in_vault` decides — canonicalising
  both sides first, because the vault path is typed by the operator and the read path has
  been through `fs_guard`, and a string comparison would answer "not in the vault" for a
  file plainly in it.
- **`commands::generate_response_streamed`** opens the record and drains it around the one
  call that reads the vault, and returns it as `notes` on the reply. Both transports carry
  it for free: the Tauri command returns that value, and the WebSocket's `done` frame is
  that value.
- **`commands::open_vault_folder`** hands the folder to the operator's own file manager
  (`xdg-open`/`explorer`/`open`, spawned with no shell), creating the vault first if this is
  the first run. Reachable **only** from the desktop app's Settings pane: there is no HTTP
  route, because a phone on the LAN asking a desktop in another room to pop open a file
  manager is not a feature anyone asked for. The browser fallback copies the path and says
  why it cannot do more. The path comes from the vault setting, which is not in `SETTABLE`,
  so nothing a model says can steer it.
- **Verify — done, as tests.** Six on the record itself (nothing recorded outside a turn;
  a note found and then read reports the reading; draining ends the turn; the footer is
  capped at twelve), plus one that primes a real vault and asserts the footer names exactly
  the always-loaded set, and one that reads a vault note and `/etc/hostname` in the same
  turn and asserts only the note appears.

What is **not** tested is the part that matters most: whether the notes it names are the
notes that actually shaped the answer. That is a judgement about a live model, and it needs
a real conversation against a real vault to make.

Landed as a one-line trace, the same idiom a tool call already gets (see
`tools/protocol.rs`'s `trace_of`): `LlmEngine::vault_trace` names whichever of
`INDEX.md`/`profile.md`/`machine.md` actually exist right now — exactly the list
`vault::prime` loaded into this turn's system prompt, via the new `vault::primed_notes` —
and it's prepended to the reply in the two code paths that actually send that prompt to a
model (`tool_loop` and the plain-provider branch). The offline and local-only-refusal
replies never see it, on purpose: neither ever reads `system_prompt`, so claiming the vault
either would be exactly the kind of dressed-up-as-a-measurement claim step 14 already
refuses to make elsewhere. An ad hoc `read_file` mid-turn still gets its own trace line as
before — this just covers what silent priming never surfaced at all.

The folder button (`vault::open_folder`, `paths::open_in_file_manager`) shells out to
Explorer, Finder, or `xdg-open` — no new Tauri plugin dependency, matching how `winget` and
`git`/`cargo` invocations already work in `main.rs`. It creates the starter vault first if
this is the very first thing to touch it, so the button never opens a folder that isn't
there yet. Wired through both transports (`POST /api/vault/open` and
`open_vault_folder_rust`) per the two-transport rule, even though `--lan` opens it on the
server's own desktop rather than the requester's -- there is no way to hand a remote
browser a file-manager window, only the machine this process is actually running on.

## Phase 4 — Situation

### Step 13: Crash and log capture — **shipped**

Crashes only. The wider sweep of the event log is something you ask for, not something it
volunteers: an event log is mostly noise, and a companion that reads the noise aloud is a
companion you turn off.

Windows is a peer here rather than a feature gate. Both platforms offer the same three things
under different names — an event stream, a crash record, and a log tail. On Linux that is the
systemd journal, `coredumpctl`, and `/var/log` where there is no journal; on Windows it is the
event log and Windows Error Reporting. One design, two readers.

Reading them is local-model work. Summarising a crash means handing something your machine's
recent failures, and that is not a thing to spend a cloud call on; `local_only.rs` and the
per-persona routing already give a way to pin it. With no local model present it shows the raw
event and says nothing further, rather than waiting for one to exist.

- ~~**New `src-tauri/src/watchers/crash.rs`**~~ **Shipped.** `CrashReader` is the one design;
  `LinuxCrashReader` polls `coredumpctl list --json=short` and pulls the failing program's
  last lines out of the journal, `WindowsCrashReader` reads event 1000 from the Application
  log. Polling rather than a watch, because there is no supported event for "a core was just
  collected" and a process that answers in milliseconds costs nothing at a crash-shaped
  interval. Both readers are compiled and tested on every platform, so the peer platform is
  not a thing that only exists on the machine it runs on.
- ~~**`src-tauri/src/main.rs`**~~ **Shipped.** On a crash the tray goes amber — tinted from
  the existing icon rather than a second asset, so it keeps working if the icon is ever
  redrawn — the tooltip says what died, a notification offers to look into it, and the HUD is
  handed the whole assembled context. The amber clears itself after two minutes: it is a
  notice, not a state to live in.
- ~~**The diagnostics command**~~ **Shipped** as `aether1 status --events`, plus
  `aether1 crashes` for the crash list on its own. Muted programs still appear there, marked:
  the mute list stops AETHER1 interrupting you, and asking is not being interrupted.
- ~~**Settings**~~ **Shipped** as `crash_capture_enabled` (on by default — a watcher nobody
  turned on watches nothing) and `crash_capture_muted`, which reads a JSON array or a
  comma-separated line, matches case-insensitively, and ignores a `.exe` on either side, so
  one list works on both platforms.

**The parsing is a free function over each tool's output, and that is the design decision
worth keeping.** The machine this was built on has neither `coredumpctl` nor a journal nor
Windows, so a reader that could only be tested by crashing something would have been a reader
written by guesswork. It also caught a real bug: the first Windows implementation read the
rendered message and took the first eight-digit hex value as the exception code, which is the
faulting module's PE timestamp. Event 1000 declares its fields in a fixed template, so reading
the properties array is both simpler and the only version that works on an install that is not
in English.

- **Verified:** with no `coredumpctl` present, `aether1 crashes` says what is missing and which
  package provides it, rather than reporting a machine that never crashes. With a stand-in
  `coredumpctl` and `journalctl` on `PATH` the real reader ran end to end — two crashes listed
  oldest first, paths reduced to program names, signals named rather than numbered, the journal
  tail attached, and `crash_capture_muted` marking firefox while still showing it.
  `aether1 status --events` returns the journal's error-level lines with the hostname dropped
  and pids stripped from unit names, and says "nothing at error level" rather than printing a
  blank where a report should be. **Not verified here:** the tray, the notification and the
  Windows reader, none of which a headless Linux container can run.

**Its seam with step 15.** These are not one step. Crash capture has to stand on its own — no
agent installed, no local model, still shows you the crash — and most hand-offs have nothing to
do with a crash. What they share is exactly one thing: the assembled context. Step 15 installs a
standing context file into the agents' own skill directories; step 13 produces a crash bundle. A
crash handed to an agent is the second plus the first, which is how Omarchy does it too, with
`diagnose-crash` as a skill of its own. Build the context bundle once and let both call it.

### Step 14: Honest AI telemetry — **shipped**

Every provider returns real token counts, and every one of them was being thrown away and
replaced with a four-characters-per-token guess. Now they are parsed where they arrive:
Ollama's `prompt_eval_count`/`eval_count` on the final NDJSON object, the OpenAI-compatible
`usage` object (behind `stream_options.include_usage`), Gemini's `usageMetadata`, and
Anthropic's, split across `message_start` for the input and `message_delta` for the output.

The estimate stays as the fallback -- a server that reports nothing still has to produce a
number -- but it is never dressed up as a measurement. `measured_requests` against
`total_requests` is what lets the panel say `counted`, `counted 2/3` or `estimated`, and that
distinction is the whole point of the step: "1,204 tokens" and "about 1,200 tokens" are
different claims, and only one of them can be checked against a provider's own dashboard.

Three decisions worth keeping:

**`include_usage` goes only to OpenAI and Groq.** Both document it. LM Studio does not get it,
because it is the local one and the one most likely to be an older build or a look-alike
server, and a request rejected for carrying an unknown field would cost a working setup to
gain a count that matters less locally than throughput does.

**Throughput comes from the model's own clock.** Ollama reports `eval_duration`, the time it
actually spent generating. Wall clock includes loading the model off disk and queueing, which
makes a fast model look half as fast on its first reply and quicker on every one after -- a
cold start that reads like a fault.

**The session budget is labelled as ours.** No provider API returns "tokens you have left";
that is a billing question, answered on a dashboard rather than in a response body. Inventing
one and calling it *Available* would have been a new fiction replacing the one being removed,
so the panel says *Budget left* and the constant says what it is in its own doc comment.
*(Superseded by step 34: labelling an invented number honestly in a doc comment did not stop
the panel from presenting it as a measurement. The budget is gone.)*

The panel became two views, because a cloud model and a local one raise different questions --
what have I spent, versus how fast is this and how much can it hold. They alternate while idle
and pin to whichever is in use once something is generating. `/api/show` fills in the local
model's context length, parameter count and quantisation, fetched once per model at the end of
a turn rather than on the one-second telemetry tick, because a getter that quietly does I/O on
a timer is a getter that will one day be the reason the HUD stutters.

---

## Phase 5+ — Reach and continuity

Every step in this phase is specified below. They were sketches for a long time because the
earlier steps kept changing what they should be; they no longer do.

---

### Step 15: Agent handoff

A coding agent is already installed on both machines, and the non-goal that says "not a
coding agent" has always ended "Aether1 may hand work to one". This is that hand-off: per
hand-off, with permission, and narrated back in persona rather than left in a terminal you
have to go and read.

**What it hands to.** A list of agent commands in settings, seeded on first run from what is
actually on PATH, with a row for adding your own. A fixed list would mean every new agent
needs a new release; a seeded list you can edit means the next one works the day you install
it. Omarchy arrived at the same shape — agents pre-wired, a default you set, and nothing
stopping you pointing it elsewhere.

**Where it runs.** The operator terminal's current directory when the terminal is open,
because a hand-off that came out of a real problem is almost always about the thing you were
just doing. A work directory you nominate once when it is not. Never `$HOME`: Omarchy refuses
it outright and starts such launches in `~/Work` instead, which is the right instinct — an
agent that goes wrong should go wrong inside a blast radius you chose. Reaching a project
outside either directory is a separate request, authorised before anything runs, and the
directory appears in the approval line every time, so a wrong one takes one glance to catch.

**Foreground and background.** With the terminal open the agent lands in it and you watch it
work. With the window closed it runs unattended and you get an account of what happened
afterwards. Unattended is only honest if the agent cannot stall on a permission prompt nobody
is there to answer, so the background path is the case the consent rule already allows: the
plan is disclosed and approved whole up front, and then it runs. Consent per step stays the
default for the foreground path.

**What the agent is told.** The machine description step 37 already generates, and the
conversation that led to the task, every time. The relevant vault notes and the crash and log
data only when the task plainly came from them. The standing part of that does not belong in
every prompt: it is written once into the skill directories agents read at startup
(`~/.claude/skills`, `~/.codex/skills`, and the generic `~/.agents/skills`), the way Omarchy
installs its own, so a hand-off prompt only has to carry the task. This is also where step 13
meets this step — a crash hand-off is the crash-capture context plus the same installed skill.

- **New `src-tauri/src/handoff.rs`** — the agent list and its seeding from PATH via `which`
  (already a dependency), directory resolution by the rules above, and running the chosen
  command through the existing consent path in `tools/mutating.rs`.
- **A skill writer** — generates and refreshes the Aether1 context file in each known agent's
  skill directory, and removes it cleanly when an agent is dropped from the list.
- **`src-tauri/src/cli.rs`** — a subcommand that hands a task over from outside the HUD, so a
  compositor binding or another program can reach it.
- **Settings** — the agent list, the default agent, the nominated work directory, and an off
  switch.
- **Verify:** a hand-off with the terminal open lands in that terminal in the expected
  directory; the same hand-off with the window closed produces an account of what the agent
  did; a hand-off aimed outside the allowed directories stops and asks first; and an agent
  installed after Aether1 started appears in the list without a code change.

---

### Step 16: MCP, the server half first

MCP is the Model Context Protocol: a small JSON-RPC protocol where a server advertises tools,
resources and prompts, and any client that speaks it can use them. The sketch had this the way
everyone writes it — a client, so Aether1 can borrow other people's tools. It is worth doing the
other way round, for two reasons.

The first is a limit, not a preference. A client is only as good as the model driving it: using
someone else's tools means tool calling, and that is exactly what a small local model is worst
at. An MCP client would be close to useless on the floor this project commits to, and genuinely
useful only on a capable model, usually a paid one. That is not an argument against building it.
It does mean it is a feature for the far end of the range rather than the near one, and it should
be described that way instead of as a general capability.

The second is that the server half is the one nothing else here covers. A netbook serving models
in another room, a desktop that knows your vault, a laptop that does not — the discovery and the
pairing phrase for that already exist in `discovery.rs` and `serve_auth.rs`. Speaking MCP over
that link means another assistant, on another machine you own, can reach this machine's tools and
this machine's vault without a second integration being written for each one. That is the peers
idea with a protocol attached.

It costs less than it sounds, because the pieces are already here. `tools::Registry` already has
`schemas()` and `run()`, the headless `--serve` router already exposes `/api/tools`,
`/api/actions` and `/api/actions/pending`, and the pending-actions surface is already how a tool
call waits for a person to say yes. MCP's `tools/list` is `schemas()`; MCP's `tools/call` is
`run()` through that same pending-action path. Consent stays per call — an approved server is not
a trusted one, and an external caller must not get a way in that a local one does not have.

- **New `src-tauri/src/mcp.rs`** — the protocol itself, mounted on the existing axum router
  behind the pairing auth in `serve_auth.rs`, mapping `tools/list` to `tools::Registry::schemas`
  and `tools/call` to `tools::run` through the pending-action consent path.
- **Resources** — the vault, read-only, under the same per-call consent as everything else.
- **Discovery** — advertise the MCP endpoint in the existing mDNS record so a paired machine
  finds it the way it already finds everything else.
- **Settings** — an off switch, a per-tool allow list of what a remote caller may reach at all,
  and a visible record of which paired machine called what.
- **The client half is optional and comes after** — if it is built, it goes behind the same
  consent path, and its own doc comment should say plainly that it needs a capable model.
- **Verify:** a paired machine on the LAN lists this machine's tools over MCP, a call to one
  stops for consent before it runs, an unpaired caller gets nothing, and turning the switch off
  removes the endpoint from discovery as well as from the router.

---

### Step 17: Packaging

What is left of this is Linux only. Step 46 settled the Windows side — an Inno Setup installer
signed through Azure Trusted Signing, and an in-app updater that signs the operator in to GitHub
for a repository that stays private.

**AppImage is the Linux artifact.** It matches the shape that already exists: the Linux bundle
carries Piper, whisper.cpp and their models rather than trusting the host to have them, which is
exactly what an AppImage is for, so this is a repackage rather than a rethink. It needs no store,
which matters while releases are gated. And it carries update information — a string embedded at
build time that lets `AppImageUpdate`, and the desktop managers built on it, fetch a delta instead
of the whole file.

**That string points at the GitHub repository from the first build**, even though the repository
is private today. It costs nothing to embed, and it is the difference between the day the
repository goes public being a switch and being a rebuild. Until then it simply does not resolve
for an outside manager, and updating happens inside the app through the signed-in path from step
46. Both are true at once: the app updates itself now, and every AppImage already in someone's
hands starts working with an external manager the moment the repository is public. The manager
must fail politely against a private repository rather than crash, which is a thing to test
rather than assume.

**Flatpak and Snap are the wrong universal here**, and the reason belongs in writing so it is not
revisited. This app reads the journal and the Windows event log, watches for crashes, runs
commands, and hands work to agent CLIs on the host. Under either sandbox every one of those needs
a hole punched through it, and a package carrying `--filesystem=host` plus host command execution
is a sandbox in name only: all of the packaging overhead, none of the safety.

**AUR and `.deb` stay.** On Arch and CachyOS the AUR is a PKGBUILD and the operator's own helper
keeps it current, which is the native answer on the machine this is developed on; `.deb` covers
the Debian and Ubuntu side.

- **`.github/workflows/release.yml`** — build the AppImage beside the existing bundles, with the
  update information string embedded and pointing at the repository.
- **minisign** — the Linux bundles are signed by nothing today. The keypair planned in step 46
  covers the AppImage too: private key in Actions secrets, public key compiled in.
- **AUR and `.deb`** — a PKGBUILD and a `.deb` beside the tarball, so a fresh machine is one
  command.
- **Verify:** the AppImage runs on a distribution that is not the build host; an external manager
  reading its embedded update information reports something sensible against the private
  repository instead of failing hard; and the in-app updater still works for an AppImage that no
  manager has ever registered.

---

### Step 18: Memory sync, which is three different problems

"Memory" here is all three of the things it could mean — the vault, the conversations, and the
settings and personas — and the reason this step was a sketch for so long is that they do not
sync the same way. Treating them as one thing is what makes this look hard.

**The vault is a folder, and that is the answer.** Markdown files merge per file, and Syncthing,
a git remote or a synced directory already do it better than anything written here would. So this
step does not sync the vault. What it owes the vault is not corrupting it: notice when files
change underneath the app, and never hold them open in a way that fights whatever is already
syncing them.

**Conversations cannot conflict, because nobody edits the past.** `sessions` and `messages` are
append-only in practice — a finished conversation is never rewritten — so merging is just
insertion. `sessions.id` is already `TEXT` and unique per machine, so the two sides never collide.
`messages.id` is a local `AUTOINCREMENT` and must not cross the wire; an import renumbers on
insert and recognises a message it already has by session, sender and timestamp, which makes
importing the same export twice a no-op rather than a duplicate transcript.

**Settings are the hard third, because they do not all belong to you.** Some describe the machine
— which model, which endpoint, which voice engine, a hotkey chord that works on this desktop and
not the other one — and copying those across is not sync, it is damage: a laptop's model choice
landing on a desktop with a better GPU makes the desktop worse. Others describe you: personas,
the tone sliders, consent preferences, the agent list from step 15. Those are the whole point.
So every key gets a scope, machine or identity, and only identity-scoped keys travel.

**The existing keys are not defaulted, they are classified.** A blanket default is wrong in both
directions: default them all to machine and the feature arrives empty, which reads as broken and
leaves you flipping flags key by key; default them all to identity and the first sync quietly
makes the other machine worse, with an endpoint pointing at something that is not on the network
and no error to connect it to. Neither is necessary. There are a few dozen keys and every one of
them is in this repository, so they get read once and marked when the column is added. That is an
afternoon, and it is the difference between a rule and a guess.

The default only governs keys added later, and there `machine` is right, because a key that fails
to sync is a nuisance while a key that syncs when it should not is a regression you have to go and
find. What keeps that from rotting is a test: adding a settings key with no explicit scope fails
the build. Without it, a year from now someone adds something persona-shaped, it inherits the
fallback, and the feature is quietly wrong in whichever direction the fallback points.

`long_term_memory` already carries `updated_at`, so newest wins there and no schema change is
needed. `settings` carries no timestamp at all, so it cannot be merged as it stands — adding one,
with the scope column, is this step's own schema change.

**What is left of conflict is small enough to ask about.** The vault belongs to whatever syncs it,
conversations cannot collide, and machine-scoped keys never leave. That leaves two machines
editing the same identity-scoped key while apart, which is rare and always meaningful, so both
versions are kept and you are asked, rather than one being picked silently.

**Transport: a file first, the LAN second.** Export and import of a single file works between two
machines that never see each other and needs nothing running, which is why it comes first. Direct
transfer over the LAN comes after, on the pairing phrase in `serve_auth.rs` and the discovery in
`discovery.rs` that already exist. Neither touches a server anyone else runs. The phone is not a
third party to this: it is a face for a desktop instance and keeps no memory of its own.

- **New `src-tauri/src/sync.rs`** — export to and import from one file; conversation import keyed
  on session, sender and timestamp so a repeat import is a no-op; identity-scoped key merge by
  `updated_at`.
- **`src-tauri/src/llm/db.rs`** — `settings` gains `updated_at` and a `scope` column of
  `machine` or `identity`; the keys that exist when the column lands are classified by hand, one
  by one, and `machine` is only the fallback for keys added afterwards.
- **A test that fails on an unclassified key**, so the fallback stays nearly unreachable.
- **LAN transfer** — over the existing pairing and discovery, under the same per-call consent as
  everything else.
- **Settings** — an off switch, which machine is paired, and a record of what travelled last time.
- **The vault is deliberately out of scope**, and the doc should say why rather than leaving it
  looking forgotten.
- **Verify:** an export taken on one machine and imported on the other brings the conversations
  and the personas across and leaves the second machine's model choice untouched; importing the
  same file twice changes nothing; and two machines that edited the same identity-scoped key while
  apart produce a question rather than a silent overwrite; and a settings key added without a
  scope fails the build.

---

### Step 18a: A fullscreen face — **shipped**

Cheap, and it changes how the thing feels: a fullscreen route that shows only the avatar
and its state (idle / listening / thinking / speaking), for a second monitor or a spare
screen. The hologram renderer and the state machine already exist; this is a layout and a
CLI flag (`aether1 face`) away.

*Built in step 44 below — where the "cheap" estimate turned out to be right about the
window and wrong about the avatar in it.*

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
2. ~~**What "better suited" means.**~~ **Settled: the speciality is the key.** Not a
   classification of each request, and not a learned signal -- the personas already carve
   the work up by job, so "which model for which job" is a question the design can already
   ask. It reads as a sentence the operator can check: a code-specialised model like
   Qwen-Coder suits L'kemi, something chatty and personable suits Halcy. The scoreboard was
   never going to answer this on its own; it measures tokens per second, which is speed, and
   fitness is a different axis.

   The suggestion beside each model comes from **a table shipped in the binary**, matching
   model families to specialities by name, with `model_benchmarks` breaking ties on measured
   speed. Deliberately not a Hugging Face or Ollama library lookup: those rank by downloads,
   which is popularity rather than fitness, and principle 2 means a machine with no network
   still has to see a sensible default. The table ages, and is updated with releases, which
   is the honest cost of the rule.
3. ~~**What the operator sees.**~~ **Settled: a dropdown beside the avatar**, listing the
   models actually available, with one marked *(suggested)*. The choice is explicit rather
   than inferred -- no thumbs-down teaching it quietly -- and it is stored per speciality in
   the `settings` table, which is already the key-value store the rest of the HUD's choices
   live in. A model that disappears from the list leaves the setting in place and falls back
   to the suggestion, saying so once, so uninstalling a model is not a silent change of who
   you are talking to.

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

### Step 28: two sliders for tone, because it was two complaints — **shipped**

Feedback on the themes said two things: the colours are too bright, and Solar and Eclipse are
not dark enough. The request was for a saturation slider to fix both.

Saturation alone cannot. Draining the colour out of a background moves it toward **grey at the
same brightness**, not toward black — so it answers the first complaint exactly and the second
not at all. They are two axes, so they are two sliders:

- **Saturation** (0–100%) mixes each colour toward the grey of its own Rec. 601 luminance, so
  chroma goes and brightness stays. Verified: Night City's `#fcee0a` at full is luminance
  0.848, and fully drained it is 0.847.
- **Depth** (−40…+40) mixes the background toward black or white, and everything derived from
  it — panels, hover states, the avatar bay — follows, because they were already computed from
  the background rather than written down separately.

Both live in each mode's stored colours, both default to "as the preset was drawn", and both
route through one new `derive()` that `variablesFor` and `paletteFor` call. That is why the
sliders reach the neon glow and the avatar's own palette without a line of code in either.

Depth stops at 40 for a reason worth keeping: `isLight()` measures the background rather than
trusting the mode, so a background dragged far enough crosses 0.5 and the shell inverts. At the
cap, Solar's darkest is luminance 0.573 and Eclipse's lightest is 0.475 — both still on their
own side of the line, so the slider cannot flip the window out from under you.

The colour swatches keep showing the colours **as picked**, not as painted. That is what makes
the sliders non-destructive — otherwise nudging a picker after moving a slider would bake the
adjustment in permanently — and since it means the swatch and the screen disagree, a note says
so while the tone is off default.

### Step 29: a fresh install has no brain, and nothing said so — **shipped**

The report was "a good looking app with no substance". It was accurate, and the cause was
not the chat path — streaming, deltas, TTS chunking and history all worked. It was that a
fresh install defaults to `llm_provider = "offline"`, so every answer came from
`Persona::offline_reply`: canned text with nothing thinking behind it, and no route from
that state to a working model that a non-technical person could find.

`setup.rs` answers one question — *where is this machine, and what is the single next
thing to do?* — from a live probe, as five stages: `NothingInstalled`,
`InstalledNotRunning`, `RunningNoModel`, `ReadyToSelect`, `Configured`. Two properties
make it honest:

- **The stage is derived, never counted.** There is no step counter anywhere; the wizard
  re-asks the backend after every action. A stage cannot be skipped past or claimed
  falsely, and closing the app mid-way loses nothing.
- **A finished download is a model the server reports.** `ollama pull` is spawned and
  returns immediately, so "done" cannot come from the pull. The wizard polls
  `/api/setup/advice` every five seconds and watches `installed_models` for the chosen
  name to appear — true by construction, and it survives Aether1 being closed, because
  the download was never Aether1's job.

`models_for(ram_total_gb)` sizes the offer to the machine: usable memory is 70% of total,
and the largest model that fits is marked recommended, so 4 GB is offered a 1B, 8 GB a 3B,
16 GB `llama3.1:8b`, 32 GB a 14B and 64 GB+ `llama3.3:70b`. The catalogue is nineteen
models across five memory tiers — the popular Llama, Gemma, Qwen, Mistral, Phi and
DeepSeek R1 sizes — grouped ascending by `needs_gb`, with the intended recommendation
*last* inside each tier, because the pick is `rposition(|m| m.needs_gb <= usable)`. That
makes intra-tier ordering load-bearing rather than cosmetic, so
`catalogue_is_ordered_by_memory` pins the ascent as a test.

The whole catalogue is always listed — the recommendation is a default, not a gate — but
nineteen radio buttons is its own kind of unhelpful, so each `ModelChoice` now carries
`fits`, and the HUD folds the ones this machine has no memory for behind a `<details>`
("Show N bigger models"). Already-downloaded models count as fitting whatever the memory
says: they are on the disk, and hiding one would mean offering a download instead.

Two things are deliberately *not* automatic. The cloud route is offered and never taken,
because it means the words you type leave the machine and that is a decision. And the
download button is hidden outright when `can_install_from_here` is false, rather than
shown as a button that cannot work.

Falling out of the same work: `pull_model`'s HTTP branch has a five-second timeout, so it
can never complete a real download — it always fell through to the CLI and reported
"Started in background" with no further signal. Both messages now say what actually
happened.

### Step 30: the settings panel, grouped by the question you arrived with — **shipped**

`Agent & System` was one flat column. It is now six `<details>` groups — The Brain, Voice
& Sound, Memory, What it may do, Network, The app itself — with the things almost nobody
needs nested one level further inside the group they belong to. `<details>` rather than
swapping panels for a concrete reason: a closed group still has all of its inputs in the
DOM, so `loadSettings` and `saveSettings` address fields by id and need to know nothing
about the grouping.

Auditing every field against both halves of that round trip found four settings that were
stored, some of them writable by the companion itself, and read by nothing or settable
from nowhere:

- `enable_sfx` — stored and AI-writable, but `voice.js` hardcoded `sfxEnabled = true` and
  the menu toggle forgot on reload. Now one switch in two places, both saving.
- `color_theme` — AI-writable, but the HUD only ever read the browser's own copy, so
  asking the companion to change its colours changed nothing visible. Now reconciled on
  load, and given a default in `get_settings` so the key comes back at all.
- `tts_local_voice`, `stt_model_path`, `stt_language` — all three read by `voice_status`
  and `transcribe_audio`, none settable from the HUD. Now three fields nested under Voice
  & Sound, where empty means "find them yourself", which is the working default.

### Step 31: a download you can watch, and a missing piece you cannot miss — **shipped**

Two complaints from the first person to use Step 29 in anger, and they turn out to be the
same complaint: *the app knows something is wrong and says so too quietly to hear.*

**The progress bar.** Step 29's download spawned `ollama pull` as a child process and
returned. A child process reports nothing, so the wizard polled the model list every five
seconds and said "still going" until the name appeared — true, and for four minutes
indistinguishable from a hang. `downloads.rs` replaces it: `POST /api/pull` with
`{"stream": true}` returns newline-delimited JSON, one line per progress tick, and a
thread per download reads it into a registry the HUD polls once a second.

The one thing that needed care is that a model is several blobs, and the stream reports
`completed`/`total` for whichever layer is moving. A bar wired straight to those numbers
drops to zero at every layer boundary. `Tracker` keeps the last figure *per digest* and
sums them, so the bar only ever goes forwards. Ten tests cover the shape of that stream:
a second layer adding rather than replacing, `verifying sha256 digest` not throwing the
bar away, `success` finishing it even when the final line carries no numbers.

Because the registry is a map with a thread per entry, several downloads at once fell out
for free. It is capped at three, and the refusal past that says why: *they share one
connection, so starting more would not make any of them finish sooner.*

**The notice.** The "no AI connected" card was a small amber line in the chat stream, and
sending a message with nothing configured returned a canned offline reply — which reads
exactly like an answer. So the app looked like it worked, badly. Now: the card is a
full-width bordered block with an Orbitron headline; the wizard opens by itself on launch
when `needs_attention` is set (once per launch — re-opening a window somebody just closed
teaches people to close windows without reading them); and the send path re-probes and
refuses rather than answering, because a canned reply in place of a real one is the app
lying about its own state.

**The one thing it can fix itself.** Four of the five setup stages are things Aether1 can
only describe. `installed-not-running` is not: the binary is there and nothing is using
it. `start_local_server` spawns it — no shell, no caller-supplied argument, the binary
`which` finds under exactly the name `ollama`, so the whole of what it can be made to run
is "the ollama already installed here, serving". It is not a tool, so nothing the
companion says in a conversation can reach it.

### Step 32: the window stopped answering, and the voice failed silently — **shipped**

Three reports from the same sitting, and two of them turned out to be one bug.

**"8 seconds when I click Status. Any request freezes the app to 'not responding'."**
Every `#[tauri::command]` in `main.rs` was synchronous. A synchronous Tauri command runs
*on the main thread* — the same thread that draws the window and answers the operating
system when it asks whether the app is alive. So every command that blocked (ureq for
HTTP, rusqlite for the database, a WebSocket to Microsoft for the cloud voice, a spawned
`ollama`) blocked the window for exactly as long as it took. The first token of a reply
is not instant, so the window was dead for the whole of that wait, and Windows painted it
"Not responding". Nothing was slow; the wait was simply happening in the one place it
must not.

The fix is one annotation: `#[tauri::command(async)]` on a function that is *not* itself
`async fn` makes Tauri run it on a thread pool instead of the main thread
(`ExecutionContext::Async` → `sync_threadpool` in `tauri-macros`). 32 commands were
converted. Four are deliberately left synchronous — `toggle_sprite_window_rust`,
`open_avatar_lab_rust`, `show_main_window_rust`, `start_window_drag_rust` — because they
create, show and drag windows, which *must* happen on the main thread and never block.

Because "somebody adds a command and forgets" is how this comes back, `mod
ipc_thread_tests` reads `main.rs` with `include_str!` and fails if any command is
synchronous without being on that four-name allow-list — and fails the other way too, if
a name on the list no longer exists. It was verified by reverting one command and
watching the test catch it.

**"Still no voice."** `synthesizeSpeechUrl` caught its own exception, wrote
`console.warn` and returned null. So the entire evidence available to the person using it
was silence — which is indistinguishable from a reply that had nothing to say. Two
changes. `tts.rs` grew `generate_speech_reporting`, which records an `Attempt { engine,
ok, detail }` for every tier of the Auto chain rather than only reporting the last error;
`generate_speech_with` is now a thin wrapper over it, so the existing path is unchanged.
And the frontend shows a card in the chat stream, once per launch, saying speech was
attempted and failed, with the reason and a button into the voice wizard.

**"Can I have a similar settings flow for the voice side."** `voice_setup.rs` is the brain
wizard's twin, and deliberately the same shape: a stage derived from probing the machine
rather than from a step counter, one command or one link per step, never both. It reports
the two halves separately — `Speaking { Silent, BasicVoice, GoodVoice }` and `Listening {
Deaf, ModelMissing, Ready }` — because either can be broken while the other is fine, and
a single "voice: not working" line for both is how you spend an evening reinstalling the
half that already worked.

The one judgement call in it: **the OS voice counts as working.** `BasicVoice` is
`working: true` and still `needs_attention()`. It really does speak, so calling it broken
would be a lie that sends people installing Piper to fix a problem they do not have; but
it sounds like a robot, so saying nothing would leave the better voice undiscovered. Both
are true, and the type says both.

`POST /api/voice/test` and `test_speech_rust` synthesize the test sentence and return the
per-engine attempt list either way — on success *and* on failure, because "which one
spoke" is as useful as "why none did". The frontend plays it back through
`voiceEngine.playTTSAudio`, the same function a real reply uses, on purpose: a test that
passes through a private code path proves only that the private code path works.

### Step 33: making it small, and making the wait honest — **shipped**

*"The goal is for the actual app to be small (only dependencies are large). The interactions
need to be quick and responsive with good feedback if that is not the case."*

The app itself was never the weight. The measuring found two things carrying it and one
thing lying about it.

**three.js, 589 KB of which the HUD uses about forty functions.** The vendored bundle was
the whole library. `scripts/build_vendor_three.sh` rebuilds it from the same pinned r128
source with only the exports the frontend actually names, 603,445 bytes down to 435,994.
The revision stays r128 on purpose: r152 changed colour management in a way that visibly
alters what is already on screen, and how the avatar looks is close to the point of the
project. `scripts/check_vendor_three.sh` greps the frontend for `THREE.Something` at
CI time and fails if the bundle does not export it — so an avatar that reaches for a part
that was trimmed is a failed check, not a button that throws when somebody presses it.

**Nineteen avatar files loaded to show one.** Every page listed all of them in `<script>`
tags: about 195 KB and eighteen requests, at launch, to put a single avatar on screen.
`frontend/js/hologram/avatar-loader.js` is now the one place that says which avatar lives
in which file, and fetches it the first time it is asked for. `core.js` gained
`materialiseAvatar`, which builds an avatar if its file is present, starts the fetch if it
is not, and rebuilds when it lands. The workbench still asks for all of them, because it
is a picker over all of them. Repeating the list in three HTML files had already drifted —
the same avatar was pinned to three different cache-busting versions — and one list cannot
drift from itself. `scripts/check_avatar_loader.sh` globs the folder and fails on any file
the manifest does not name, and any name with no file.

Startup payload: **1317 KB across 39 files → 963 KB across 20**.

**Debug symbols in the shipped binary.** `strip = true`, plus `opt-level = "s"`,
`lto`, one codegen unit and `panic = "abort"` in the release profile. A backtrace from a
release build is read from the source anyway. **12.98 MB → 10.54 MB.**

**Drawing a window nobody is looking at.** The render loop ran sixty times a second while
minimised, in the tray, or behind another window. One line in `animate.js` returns early
when `document.hidden`, after the next frame is already requested, so it resumes on the
frame after the window comes back with nothing to restart.

**The wait.** A local model's first answer after launch takes as long as it takes to read
the model off disk, and the entire feedback for that was a blinking cursor on an empty
line — which says exactly the same thing at two seconds and at two minutes, one of which
is normal and the other of which is a crash. `startWaitFeedback` in `app.js` puts three
things under the unanswered question: a counter, so the wait is a number; on the first
question of the session, the one sentence that explains it (*the model is being loaded into
memory, which only happens once*); and past thirty seconds, that this is still normal.

The subtlety is when to stop counting. The stream carries trace lines as well as the
answer — `⚙ vault notes loaded: ...` goes out the instant the turn starts, before the model
has been asked anything — so "a delta arrived" was already the wrong test, and the hologram
had been leaving its THINKING state on it too. `withoutTraceLines` is the test now: the
counter and the thinking state end when the model says a word of its own.

**Three bugs found while looking.**

`.hidden` did nothing to ten elements. Tailwind's `.hidden { display: none }` is one class;
so is `.cyber-btn { display: inline-flex }` in A1theme.css, which loads after it. CSS
settles a tie of equal specificity by source order, so the later file won and the element
stayed on screen — including the three locked Trace Protocols avatars, which gave away the
unlock they exist to wait for. `.hidden.hidden` is two classes and wins; the one responsive
pairing in use is restored with a third. `scripts/check_hidden_utilities.sh` reads the
`hidden <breakpoint>:<display>` pairings out of the markup and fails on one with no rule.

Telemetry printed `23.456789012` in the HUD and `23.5` in the CLI, from the same reading.
The rounding lived in the CLI's formatter. It now lives in `to_wire_json`, so both ends of
the app agree about what the machine is doing.

And the light theme was not readable. A proper WCAG audit of every text-bearing element,
with the themes actually applied, found **34 failures on Solar** — almost all of them one
cause: `--text-dim` was the ink mixed 62% towards the ground, which on a near-white page
is `#a0a0a0`, about 2:1. 36% is the same idea at a readable weight. The rest were an
alert card written for a dark ground (pale amber on translucent brown, which over white
composites to a muddy grey), two bay controls wearing the page's ink instead of the
projection bay's, and a heading in an accent colour that missed the line by a
twentieth. Accents are now emitted twice: `--neon-*` as picked, for borders, fills and
glows where contrast does not apply, and `--text-accent` / `--text-highlight` walked
towards the ink until they read, for accents that are words. Any colour somebody mixes
themselves gets the same treatment. **Solar, Eclipse and Cyberpunk now all measure zero
failures**, and `scripts/check_theme_remap.sh` fails on a Tailwind text colour added to the
markup and never mapped onto a theme variable — the exact mistake that is invisible in the
dark theme the work is usually done in.

### Step 34: a performance panel that measures something — **shipped**

The operator's verdict on the telemetry panel: *"9 times out of 10 it's made up limits or
numbers and nothing really meaningful happens here. The idea was to benchmark and see the
amount of tok/s you generate locally to get an idea, and then on cloud models to track usage
in some way or form."*

They were right, and the audit is worth writing down because the failure was structural
rather than a bug. `SESSION_TOKEN_BUDGET: u64 = 100_000` was a constant invented in
`llm/mod.rs`. Three of the panel's readings were derived from it — *SESSION TOKENS* against a
gauge bar, *Used: 1.3%*, *Budget left: 98.7k* — and every one of them was that constant being
divided into and displayed back as though it had been measured. Nothing enforced it, no
provider reported it, and on a local model, where tokens cost nothing, there was no budget to
have any of left. The doc comment was honest about this. The panel was not, and the panel is
what people read.

The sparkline went the same way. Fifteen per-request token totals as a filled area chart is a
shape, not a finding: nobody can act on "that reply was longer than this one", and with one
reply recorded it drew a diagonal line across the panel that looked like a trend.

**What replaced it is the two questions that were actually being asked.**

**SPEED is a scoreboard, not a readout.** One row per model, ordered fastest first, built out
of ordinary use with nothing extra run to fill it. A single live `15.7 tok/s` cannot answer
the question worth asking, which is comparative — *is mistral faster than llama3 on this
machine?* — and a row per model can. The average is **token-weighted**: total tokens over
total generation time, not the mean of the per-reply rates, because a four-token "Yes." is
mostly measurement noise and averaging rates would let it count for as much as the reply that
actually shows what the model does.

**A sample is only kept when the server timed it itself.** `record_benchmark_sample` requires
both reported token counts and `eval_nanos`. Without the counts the numerator is a
characters-over-four guess; without the server's own generation time the denominator is a wall
clock that was also running while the model was read off disk. Either alone turns a speed
ranking into a ranking of measurement error. Today that means Ollama. A server that reports
neither never appears, and the panel says exactly why — an empty row is a true statement about
what can be measured, and an invented one is not. *(An honest fallback for LM Studio is
possible — time from first streamed token to last excludes the model load — but it has to
survive tool-call rounds and the vault trace line that is streamed before the model is asked
anything, so it is deliberately not in this change.)*

**USAGE shows money where money exists.** `pricing.rs` multiplies reported counts by a
published price. Three outcomes, worded differently on purpose and none of them `$0.00`: a
local model says its tokens are free, a priced cloud model shows a figure, and an unpriced one
says `unpriced` and points at the file to fix it. Matching is longest-prefix so a pinned build
(`gpt-4o-mini-2024-07-18`) finds its base model, and longest wins because `gpt-4o` prefixes
`gpt-4o-mini` and they are priced sixteen times apart. Costs round to five decimals, not to
cents: a short exchange with a cheap model genuinely costs a fraction of a penny, and rounding
that to `$0.00` would say the same thing the panel says about a free local model.

**Prices are dated and overridable.** `PRICES_AS_OF` is shown beside any figure derived from
the built-in table, because a six-month-old price is a guess. `model_prices.json` beside the
database overrides or extends it, re-read on every tick so a correction lands without a
restart, with a malformed file falling back to the built-ins rather than costing everything at
zero. A model released after the table was written has no entry, and that is the correct
outcome rather than a gap to fill with something plausible.

**The headline number reads off the board, not off the last reply.** Live verification caught
this one: with a cloud provider misconfigured, the panel showed `448.2 tok/s` — a genuine
wall-clock measurement of a canned local error answer that no model had generated a token of
— directly above a note describing the scoreboard. `last_tps` falls back to a wall clock
whenever the provider reports no generation time, so it is exactly the figure the rest of this
step argues against, and it was sitting in the largest text on the panel. The header and
`ON THIS MACHINE` now both read the current model's row on the board, and show `--` when it
has no row. A real number about the wrong thing is the failure mode this whole step exists to
remove, and it had survived into the replacement.

**An empty board has two causes, and the engine is what can tell them apart.** After RESET
READINGS the panel said *"Ollama does not report how long it spent generating… Ollama does."*
— a sentence that is both self-contradicting and the one a reader would act on. The frontend
cannot distinguish "this provider can never be timed" from "nothing has been measured yet",
so `UsageSnapshot` now carries `provider_times_itself` (from `providers::reports_generation_time`)
and the two cases get opposite sentences. The same guard rail applies where the board is
non-empty but the *current* model is absent from it: a cloud model sitting above a board of
local rows now says why it is not on it, rather than leaving the reader to assume one of those
rows is theirs.

**RESET READINGS** exists because the numbers describe a machine and machines change — a new
graphics card, a different quantisation, an Ollama release that got faster. Without it the old
readings keep being averaged in for as long as the database survives.

`scripts/check_panel_fields.sh` guards the class of bug this was. The panel read
`used_percent`, `available_tokens` and `sparkline` for months after they stopped meaning
anything and nothing complained, because JavaScript hands back `undefined` for a field that
was never sent and `undefined` in a gauge width is a bar of zero width rather than an error.
The check reads the field list out of `UsageSnapshot` at run time and fails the build on any
`tokens.<field>` in `app.js` that the struct does not send.

### Step 35: the three ways it could not touch the machine — **shipped**

The operator's verdict on pillar 2, in full: *"This I feel it doesn't do at all 'it can
actually operate your PC'. It asks for access and doesn't remember the answer. There is no
Terminal for it to work on. No chat storage via markdown files because of no access from what
I can see."*

Three complaints, and the audit matters because **two of the three were features that already
existed and were switched off or invisible**, not features that were missing. Building what
the words asked for would have built the wrong thing twice.

**"It asks for access and doesn't remember the answer."** There are two consent paths in this
codebase and only one of them has a memory. `tool_always_allow` does exist, does work, and
does stop a tool being asked about again — but it governs *which tool*, not *which folder*.
The prompt the operator was actually hitting is `domain::elevation_needed` (step 26): a read
of a folder outside the speciality's declared field. That path was designed with no "stop
asking" at all — elevation lasts exactly one call, by intent — so every read of the same
folder asked again, forever. Working as designed, and wrong as experienced.

**"There is no Terminal for it to work on."** `run_command` has shipped since step 7. But
`command_allowlist` defaulted to `[]`, and an allowlist of nothing refuses everything, so the
tool was present in the catalogue and refused every call. A capability that always says no is
indistinguishable from a capability that does not exist.

**"No chat storage via markdown files."** This one was real. The vault (steps 9–12) only ever
received notes the model deliberately chose to `remember`, each behind an approval prompt. An
ordinary conversation left nothing behind in the folder the operator can open.

**What shipped, with the choice the operator made on each.**

*Consent — widen the field rather than remember the answer.* Offered a choice between
session-length approvals, per-tool memory, and widening what the speciality may touch, the
operator chose the third: *"make it easy to say 'this persona may always read this folder' in
Settings, so the request never comes up."* `domain::set_extra_roots` stores folders **per
speciality** — `{"nexus": ["/home/you/Projects"]}` — because a list that widens for everyone
at once is not a field, and would have quietly deleted the point of specialities while
leaving them on screen. `within_roots` now consults the extras as well as the declared roots,
and the elevation message ends by naming the cure: *"add the folder to Nexus's field in
Settings to stop being asked about it."*

Two things keep this from being a hole. Every path is validated at save time through
`fs_guard::resolve_readable` — the same function the tools themselves call — so a folder the
guard would refuse can never be stored, and the panel can never advertise access that does
not exist. And the deny list is untouched and unreachable from here: `fs_guard` runs inside
each tool, *after* the domain check, and it beats a domain, an approval, an elevation and an
operator's setting alike. `~/.ssh` added to a field is refused at the point of saving, with
the reason shown.

*A terminal — a starter list that can only look.* Offered "stays empty", "safe starter list",
or "anything that does not need admin", the operator chose the middle. `STARTER_ALLOWLIST` is
seeded once, on first run only, behind `command_allowlist_seeded`, so clearing the list stays
cleared — `save_settings` writes back whatever the panel sends, so a getter-side fallback
would have fought the operator's own choice every time they emptied the box.

The rule the list is built on is one line, and the test enforces it: **no argument to any of
these programs can change the machine.** That excludes more than it sounds like. `systemctl`
is out because it has `stop`. `git` is out because it has `reset --hard`. `ipconfig` is out
because it has `/release`; `hostname` and `date` because they set as well as show. The
allowlist matches on program name only and the *model* chooses the arguments, so a program
with one destructive flag is a program that can be destructive. What is left is `uname`,
`uptime`, `df`, `free`, `lsblk`, `lscpu`, `lspci`, `lsusb`, `nproc`, `arch`, `ps`, `whoami`,
`id` on unix and `systeminfo`, `tasklist`, `driverquery`, `whoami` on Windows. Every run
still asks first: being on the list makes a command *proposable*, not automatic, and
`run_command` remains the one tool that can never be pre-approved.

*Chat notes — automatic, because the consent was already given.* `vault::journal_exchange`
appends every exchange to `daily/YYYY-MM-DD.md` in the vault, hooked into
`generate_response_streamed`, which is the single chokepoint both transports pass through. It
is a plain function rather than a tool the model calls, and that is the whole design: asking
the operator to approve their own conversation being remembered would be a consent prompt
with no question in it, and it only ever writes inside the folder that exists to hold it. The
consent that matters is the checkbox in Settings, given once — and it can be switched off,
after which conversations stay in the database and nowhere else. A failed write is logged to
stderr and nowhere else: a reply that was generated has been generated, and a full disk is
not a reason to replace it with an error.

Dates come from SQLite (`db::local_now`, `date('now','localtime')`) rather than a date crate,
consistent with step 33's binary-size goal — the connection is already open on every path
that needs it, and `localtime` is load-bearing: a note filed under yesterday because the
machine is west of UTC is a note the operator cannot find by looking for the day it happened
on.

**What this does not fix.** Pillar 2's "change" and "do" verbs still route entirely through
the consent path one action at a time; nothing here makes the companion an agent that
executes a plan. The gap named in step 26 stays named.

### Step 36: two programs called piper, and a voice that is two files — **shipped**

The operator, after step 35: *"voice could be due to the alma voice downloading. Piper is
also a mouse app?"*

Both halves of that are right, and between them they explain a complaint that has been
standing since the voice work shipped: *"STILL voice eludes me, and that is with things
installed."* It was not eluding them. Aether1 was reporting success and then producing
silence, twice over, and the guide was sending them somewhere that does not exist.

**`piper` is two unrelated programs.** [libratbag/piper](https://github.com/libratbag/piper)
is a GTK application for configuring gaming mice. It is the one in Arch's official
repositories, so on the operator's CachyOS box `pacman -S piper` installs a mouse utility,
and a search for "piper linux" finds it first. Piper TTS is in the AUR as `piper-tts-bin`.
Our own wizard was telling people to run `sudo pacman -S piper-tts`, which is not a package
on Arch at all, and then falling through to `apt` on a machine that has no `apt`.

Worse, `PIPER_BINARIES` listed `"piper"` **first**, so a machine with the mouse app and no
Piper TTS found `/usr/bin/piper`, reported the good offline voice as installed, and then
piped chat text into a mouse configurator. Reordering is not the fix — a box with only the
mouse app still matches. `is_piper_tts` now asks each candidate `--help` and accepts it only
if it mentions `--model` or `onnx`, which is the one flag a speech synthesiser cannot do
without and a mouse configurator has no reason to mention. `--version` would not
discriminate: both answer it. The result is cached in a `OnceLock`, because this spawns a
process and `voice_status` is re-asked on every settings change.

A binary that cannot be run, or that says nothing recognisable, is treated as not-Piper. The
asymmetry is deliberate: a wrong "no" costs the OS voice instead of the better one, a wrong
"yes" costs silence with a tick beside it, and silence with a tick is the exact failure this
module exists to prevent.

**A voice is two files.** Every voice on Hugging Face is a large `.onnx` and a small
`.onnx.json` beside it, with separate download buttons, and Piper will not load the model
without the sidecar. `piper_voice` accepted any `.onnx` it found — so the natural mistake of
taking only the obvious file produced, again, a tick and then silence. `usable_voice` now
requires the sidecar and a plausible size (`MIN_VOICE_BYTES`, one megabyte: the smallest
real voices are the `x_low` models at five). The size check catches the other shape of the
same problem — a download that stopped early, an error page saved under the model's name, a
git-lfs pointer — each of which exists on disk and satisfies `exists()`.

A configured path is checked exactly as an auto-detected one is. Pointing the setting at a
half-downloaded file is as easy as leaving one in the folder, and trusting the setting
because a human typed it would only move the silent failure somewhere harder to see.

**Saying which.** `voice_problem` is split out from `usable_voice` so the failure names
itself: *"en_GB-alba-medium.onnx is missing the small en_GB-alba-medium.onnx.json file that
has to sit beside it"*, or *"is only 4 KB ... the download probably stopped early"*. The old
message would have said "no .onnx voice was found" to someone looking directly at the voice
file they had just downloaded, which is worse than saying nothing: it sends them to download
it a second time.

The wizard's Linux steps now lead with the warning about the name, give `yay -S
piper-tts-bin` for Arch with `apt install piper-tts` named in the detail, offer the
distribution-agnostic tarball as a peer route rather than a footnote, and retitle the voice
step *"Download a voice -- BOTH files"*.

**What this does not fix.** Piper is still a manual install: Aether1 does not fetch the
binary or the voice for you, and every route above ends with a human unpacking something.
That, not the diagnosis, is what would actually make this one step.

### Step 37: describing the machine it is actually on — **shipped**

The operator, from the Windows laptop: *"Is it possible to hide linux specific things on the
windows install and vice versa?"*

It already was, in the places built with the question in mind. `Os::current()` drives the
setup wizard and the voice wizard, `STARTER_ALLOWLIST` is two lists chosen at compile time,
and `tools/domain.rs::directories()` resolves every symbolic root — `SystemLogs`,
`ServiceState`, `NetworkConfig` — to real Windows or Unix paths. What leaked was everything
downstream of that: the parts written as static text, where nobody had to decide.

**The Settings panel had no idea what it was running on.** The help under *Let it look at
this computer* named `/etc`, `/proc` and `/var/log` in hard-coded markup, and the allowlist
box suggested `git, docker, systemctl`. On Windows the first is a claim about folders that
cannot exist — and an operator has no way to check it, so it is not a cosmetic slip but a
false statement about what the companion can reach. The second was wrong on both platforms
for a different reason: those three programs are precisely the ones `STARTER_ALLOWLIST`
turns away, because each has a destructive subcommand. The box's own rule was contradicted
by the example printed inside it.

`get_settings` now returns `os` **beside** the settings rather than inside them — the
operating system is a fact about the machine, not a preference, and a key inside `settings`
would look saveable. `applyOsWording()` in app.js keys the two strings off it. The markup
keeps wording that is true everywhere and specific nowhere, so a backend too old to send the
field, or one built for a platform not in the table, reads vague instead of wrong.

The answer comes from the backend for the reason already written on `Os::current`: when the
Windows laptop browses to the Linux desktop's HUD, the machine being described is the Linux
one. `navigator.platform` would have got that exactly backwards, and this project's whole
serve-to-a-browser story is that arrangement.

**Two constants were leaking the same way.** `PIPER_VOICE_DIRS` and whisper's `MODEL_DIRS`
are printed verbatim in the "nothing found" messages, and both listed `/usr/share/...` on
Windows. The `~/.local/share` entries stay on both lists on purpose — installer/aether1.iss
writes the bundled voice and model to `%USERPROFILE%\.local\share\...`, so that one
location means the same thing on every platform — but the `/usr` entries are now
`#[cfg(not(windows))]`, with a test asserting every named folder is one this platform could
have. Sending someone hunting through a folder tree that cannot exist is worse than saying
nothing, and it is the same failure mode step 36 was about: a message that is confidently
unhelpful.

### Step 38: fetching the voice, and never the engine — **shipped**

Step 36 made the voice failures legible. It did not remove them: every route to the good
voice still ended with a human downloading two files by hand and putting them in the right
folder, which was the remaining half of *"STILL voice eludes me."*

Four options were on the table, and the one chosen draws a line that is worth writing down.

**Piper is two things, and only one of them runs.** The engine is a program; a voice is a
`.onnx` file of weights plus a small `.onnx.json` beside it. A voice fetched wrong costs
garbled speech, because nothing in it is ever executed, marked executable, or put on a
PATH — it is numbers handed to a program the operator installed themselves. An engine
fetched wrong costs code execution. Those are not the same risk and `voice_download.rs`
does not treat them as one: it fetches voices and never programs, and the engine stays a
package-manager job in the wizard's steps.

**The catalogue being a fixed table is a security property, not an editorial one.** Five
voices, each a compile-time name paired with a compile-time path under the voice
collection. A name that is not in the table is refused, so nothing the operator or the page
can type becomes part of a URL or part of a file path: there is no request to redirect and
no directory to traverse out of. A test spells this out with `../../../etc/passwd`,
`en_GB-alba-medium/../../../tmp/x`, an absolute `https://` name and the empty string, and a
second test asserts every catalogue entry is a plain name and a path under the collection.

**Local-only mode refuses it, and the refusal lives beside the connection.** Not in
`commands.rs`, which is one door of three, but in `voice_download::start`, which is where
the socket is opened — so it cannot be walked around by reaching the HTTP route instead of
the Tauri command. The picker asks the probe rather than guessing and greys its buttons out
with the reason beside them; a button that silently refuses is how somebody decides the app
is broken rather than doing as it was told.

**A download in flight is never mistaken for a voice.** Bytes stream into `<name>.onnx.part`
in 64 KiB chunks, the ceiling is enforced *while* they arrive rather than only from the
announced `content-length`, a short read is detected and reported as *"the download stopped
early"*, and the `.part` is removed on any failure. Only after the sidecar parses as JSON
and the whole body has landed is the file renamed into place and put through
`tts::voice_problem`. This closes the loop on step 36 from the other end: that step taught
Aether1 to recognise a half-finished voice, and this one makes sure it never creates one.

Hugging Face is unreachable from the build container, so `fetch` takes its base URL as a
parameter and the eight tests drive the real code path against a real `TcpListener` — what
is under test is that function, not a mock of it.

The wizard changed with it. The by-hand voice step is now the button that is sitting
directly beneath it, with the manual route kept underneath for a voice outside the short
list; and the steps stopped pointing at `rhasspy/piper` as if it were current. It was
archived in October 2025 and relicensed, development moved to `OHF-Voice/piper1-gpl`, and
that project ships no pre-built binaries — so `pip install piper-tts` is now the first
route on every platform and the old release zips are labelled for what they are: the last
ready-made builds the original project made, frozen.

### Step 39: reading the vault without leaving — **shipped**

The vault was always readable: it is markdown in a folder, and Obsidian opens it. The
operator's question was whether it could be read *in Aether1*, and the honest answer was
that everything needed already existed on disk and nothing exposed it.

**Read-only is a boundary, not a missing half.** `vault/reader.rs` does not write, rename,
create or delete, and no part of it can be extended to without that being a visible change.
Notes are changed through the tools — `write_note`, `archive_note` and the rest — which ask
before they touch a file and record a way back. A reader with a Save button would be a
second door into the same folder with neither the consent path nor the undo attached to it,
and the first thing that door would be used for is `profile.md`, which is loaded into every
single conversation.

**Every name is checked twice, and the second check is the one that matters.**
`vault::resolve_note` refuses an empty, absolute or non-`.md` name and any `..` or `.`
segment, all before a filesystem call is made. That is a check on the *shape* of a name,
and shape is exactly what a symlink defeats: `link.md` sitting inside the vault and
pointing at `~/.ssh/id_ed25519` is a perfectly ordinary relative name. So every read also
goes through `vault::note_in_vault`, which canonicalises both sides and asks where the path
actually lands. Both are pinned by tests rather than left to be inferred from the code, and
the HTTP route answers 404 identically for a note that does not exist and a name that was
never allowed — a probe cannot use the difference between those two answers to map the
disk.

**Links are resolved by name, and the unresolved ones are reported rather than hidden.**
`[[profile]]` means `profile.md` wherever it currently lives, which is what lets a note be
archived without breaking a single link pointing at it. A link naming a note nobody has
written yet comes back with `note: null` and the reader says so, because a link to a note
that does not exist is usually the companion telling you what it meant to write next.
Backlinks — which notes point *at* this one — are the half of the graph an editor hides,
and they are the reason `reader.rs` scans the folder rather than just reading one file.

**The markdown renderer is small on purpose.** Note text is written half by the operator
and half by the model, and it ends up on a page: a full markdown library would mean
inheriting its opinions about raw HTML, which is the one opinion that matters here. The
renderer in `app.js` handles headings, lists, quotes, fenced code, `**bold**`, `*italic*`,
`` `code` `` and `[[links]]`, and every piece of note text reaches the page as a text node.
A note containing `<script>` renders the word `<script>`. That is checked in a browser, not
assumed.

Four routes, wired through all three transports as everything here is: `/api/vault/notes`,
`/api/vault/note`, `/api/vault/graph` and `/api/vault/search`, with `vault_*_rust`
alongside them. The graph route had no UI when this shipped — it is step 40's, below — and
was shipped here because it is the same scan the backlinks already do.

### Step 40: the graph — **shipped**

`/api/vault/graph` had a caller and no picture. `frontend/js/notes-graph.js` is the picture:
a dot per note, a line per `[[link]]`, laid out by a small force simulation and reached from
**🕸 Graph** at the top of the Notes window. It is the one view that answers two questions a
list cannot — which notes have become the hubs everything hangs off, and which are floating
unlinked — and on a vault the companion has been writing into daily, both answers change
without anybody deciding they should.

**It draws and it does not fetch.** `notes-graph.js` is handed a `{nodes, edges, partial}`
and a callback for *the operator clicked this note*. It has no idea whether that object came
over Tauri IPC or HTTP; `app.js` keeps that branch, as every other feature here does. The
file can be read, and reasoned about, as geometry.

**The loop stops.** A settled graph draws nothing, a hidden one draws nothing, and a closed
one is torn down. This runs on a machine that is also running a language model, so a picture
nobody is looking at has no business holding a core — the same discipline `animate.js`
already applies to the avatar canvas, including the `offsetParent` check for a canvas with no
layout box. That it genuinely stops is checked by watching the canvas for changes while idle,
not assumed from reading the code.

**The physics runs on the clock, not on the frame.** A fixed 60th-of-a-second step, as many
steps per frame as the elapsed time asks for, capped at four so a stall is not replayed in
one frame. Tied to the frame rate instead, the same vault would settle in two seconds on a
144 Hz screen and twenty in a throttled tab — and settle into a *different* arrangement in
each. Positions are seeded deterministically for the same reason: opening the graph twice
gives you the same picture rather than a new one to relearn.

**A big vault is the design case, not the edge case.** Three things come from testing it at
nine hundred notes rather than at nine. Repulsion is computed through a uniform grid, so a
note is only pushed by the ones near enough to matter — pairwise would be four million sums a
tick at the 2000-note cap. The repulsion force is *capped*, because two notes starting almost
on top of each other divide by nearly nothing, and an uncapped pair leaves at a speed no
spring reverses before the simulation cools: on the first big run that showed up as a single
hub sitting a thousand pixels from everything it linked to. And the opening spiral is seeded
at roughly the density a settled graph reaches, because packing nine hundred notes into the
room for nine and letting repulsion sort it out is what blows a graph apart in its first
second.

**Colour is read from the theme, not hardcoded.** A canvas inherits nothing, so it goes and
reads `--text-main`, `--text-dim`, `--text-accent` and `--neon-amber` — and decides whether
it is drawing on a light or a dark ground from the brightness of `--bg-core`, rather than
from a theme's name, so a theme added later gets it right for free. HUD cyan on Solar's white
would have been invisible, which is exactly the class of thing that ships unnoticed when the
developer only ever looks at the dark theme.

**What the picture says.** Size is how many notes point *at* a note, not how many it points
at — a note is a hub because other things need it. Colour is the folder. A ring is one of the
three notes read before every single answer, which are worth being able to pick out of a
cloud. Labels are drawn for every note on a small vault and only for the hovered note on a
big one until you zoom in, because a thousand overlapping filenames is a grey smudge rather
than a label.

**Nothing new is exposed.** No route, no command, no tool: this is a second reader for a
scan that already shipped, and read-only for the same reason the reader is.

### Step 41: conversations you can reopen — **shipped**

The `messages` table has carried a `session_id` column since the first Rust commit, and every
caller passed the literal `"default"` into it. There was exactly one conversation, growing
forever, and the only thing you could do with it was delete it. **💬 Conversations** in the
HUD menu is the rest of that column: a list of every conversation with something in it, a way
back into any of them, a ＋ New that starts a clean one, and a name you can give one.

**The list is derived, not kept.** `list_sessions` reads the `messages` table and left-joins
`sessions` for a title, rather than maintaining its own row per conversation. Two things fall
out of that. Every conversation that already existed on disk — including the original
`"default"`, ninety-four messages deep on the development machine — appears the first time
the panel is opened, with no migration and no backfill. And a listing can never disagree with
its own transcript: there is no row to go stale, because the only thing the `sessions` table
stores is the one thing a transcript cannot supply, which is the operator's name for it. A
conversation with no name is titled by the first thing the *operator* said in it, not the
first thing the companion replied.

**A bad id is refused, never repaired.** `valid_session_id` takes 1–64 characters of
`[A-Za-z0-9_-]` and returns an error for anything else. The tempting alternative — strip the
characters you don't like and carry on — is the dangerous one: a sanitiser turns a bad id
into a *different valid id*, so the caller reads one conversation while writing to another,
and nothing anywhere reports a problem. `None` still means `"default"`, so every caller that
predates sessions keeps working untouched. The gate sits at the `commands.rs` boundary, which
is the one place both transports pass through.

**A conversation is a privacy boundary, not just a filing cabinet.** The model is handed the
history of the session it is answering in — `get_messages(session_id, 8)`, as it always was —
and has no way to reach across to another one or to learn that another one exists. That
holds because of what is *absent*: no tool lists, opens, renames, deletes or switches a
conversation, and nothing was added to `SETTABLE`. The companion cannot change which
conversation it is in, exactly as it cannot switch its own tools or panels on.

**Deleting takes the words with it.** A conversation the operator asked to be gone that
leaves its transcript sitting in the database is not gone, so `delete_session` removes the
messages and the title together. `"default"` is the one id it refuses, because that is the
fallback every session-unaware caller lands in — deleting it would not remove a conversation
so much as empty the one the app falls back to, and **🗑️ Clear conversation** is the honest
name for that.

**The id is a clock, not a secret.** There is no `rand`, `uuid` or `chrono` crate in this
build, and this does not need one: ids are minted one at a time by a single operator on a
single machine, and are never guessed at, never authorise anything and never cross a machine
boundary. `SystemTime` nanoseconds in base 36 is the whole requirement.

**The journal is deliberately untouched.** The dated vault notes stay per-day and
per-exchange. Threading session boundaries through them would turn a journal into a
transcript, and the journal is the part that was meant to be readable a year later.

### Step 42: a terminal that is the operator's alone — **shipped**

A shell in the HUD, and the reason it took care rather than an afternoon: every other feature
in this codebase is built to be *reachable* — two transports, a tool the model can ask for, a
route the browser can call — and this one had to be built to be unreachable, by the model and
by the network both, without that being a promise anybody has to take on trust.

**It is a pty, not a command runner.** `src-tauri/src/terminal.rs` opens a real pseudo-
terminal through `portable-pty` and spawns the operator's `$SHELL -l` in it. The cheaper
design — run a command, capture stdout, print it — fails exactly when it matters: `sudo`
cannot prompt for a password, `yay` cannot ask which package, `less` cannot page and `htop`
cannot draw, because none of them will do any of that unless something on the other end is a
terminal. `it_is_a_terminal_and_not_a_command_runner` asserts that `test -t 0` inside the
shell reports `ON-A-TTY`, which is the property the whole panel rests on.

**The isolation is structural, not a policy.** Three separate facts, each of which would be
enough on its own:

- *No tool.* Nothing in `src-tauri/src/tools/` mentions the terminal, so there is no tool for
  the model to ask for and nothing a prompt — or a web page, or a note the model reads — can
  steer into it. This is the same absence that keeps the AI out of `tools_enabled`, its own
  panels and, since step 41, its own conversations.
- *No route.* The four commands live in `main.rs`, not `commands.rs`. That is the one
  deliberate break in this codebase's "shared implementation, two transports" rule, and it is
  the point: `commands.rs` is what `server.rs` calls, so anything put there is reachable over
  HTTP and, with `--lan`, over the network. Better still, `main()` handles
  `Invocation::Serve` by calling `server::run` and *returning before `tauri::Builder` is ever
  constructed* — so in a headless run the terminal code is not merely unrouted, it is never
  reached, and the Tauri-managed `Terminals` map has no way to come into existence.
- *No record.* `terminal.rs` touches no database, no vault and no action log. Keystrokes go
  to the shell and bytes come back to the screen; nothing in between is kept, which also
  means nothing about the session can later be read back into a prompt.

`scripts/check_terminal_isolation.sh` asserts all three in CI — including that `main.rs`
still contains `terminal_open_rust`, so the other greps cannot pass by the terminal having
quietly been deleted.

**The bug worth recording.** The first working version leaked a reader thread and a zombie
child per terminal, and a shell that had exited still looked alive in the panel forever. A
pty reports end-of-file on the master only when the *last* slave handle closes, and `Session`
was holding the whole `PtyPair` — so the parent's copy of the slave kept the read loop from
ever returning zero. `Session` now stores only the master and `open()` does `drop(pair.slave)`
immediately after `spawn_command`. `the_shell_exiting_reports_itself` is the regression test,
and it failed by *timing out* rather than by asserting, which is what makes this class of bug
worth a test rather than a code read.

**Ceilings before ioctls.** `MAX_SESSIONS = 8` means a runaway frontend hits a wall instead
of forking shells forever, and `clamp_size` bounds rows and columns to 1..2000 before either
reaches a `TIOCSWINSZ` — a size is a number that arrives from the page, and a number that
arrives from the page gets clamped where it lands, not where it is used.

**Browser mode removes the panel rather than disabling it.** `app.js` deletes the
`data-panel="terminal"` element outright when it is not running under Tauri. There is no
route behind it there, and a terminal that looks like it works is worse than no terminal; it
also keeps it out of the module-switch list, which is built from the DOM. Switching panels
now dispatches `aether1:panels-changed`, which is what tells xterm to re-fit when the
terminal is switched back on.

**A leak that predated the terminal, closed with it.** The home folder is a readable root for
`fs_guard`, and shell history files were not on any deny list — so the companion could read
`~/.bash_history`, which is a verbatim record of every command the operator has typed, and
people type API keys and passwords into commands. `DENIED_NAMES` now covers the bash, zsh,
sh, python, node, psql, mysql and sqlite history files, `.lesshst` and PowerShell's
`ConsoleHost_history.txt`, on the same permanent list as SSH keys — beyond the reach of
domain, approval, elevation or any operator setting. The shell is not asked to stop keeping
history; that file is the operator's own tool. Building a terminal into the app made this
urgent rather than theoretical, because Aether1 is now the thing writing that file.

### Step 43: the grid follows the window in both directions — **shipped**

Columns were `minmax(0, 1fr)` and rows were a flat `24px`, so the HUD had always been half
responsive: panels tracked the window's width and ignored its height entirely. Measured, the
default arrangement wanted 1248px of panel and 1280px of page — on a 1000px-tall window the
avatar panel stayed 1248px tall and more than a third of it sat below the fold, and the
window getting taller bought dead space rather than bigger panels.

`--cell-h` is now computed by `layout.js` on every resize instead of being a constant, so the
shipped arrangement fills the height it is given. Three things make that safe rather than
clever:

**It is scaled against the default layout, not the live one.** `DESIGN_ROWS` is the lowest
row the markup reaches (37), captured from `data-grid-*` alongside the existing defaults.
Scaling against the *current* placement is the obvious move and the wrong one: the grid would
then always exactly fill the window, so dragging a panel taller would shrink every row to
compensate and the panel would not visibly grow — a resize handle that appears not to work.
Against a fixed reference the shipped layout fits at any height and an operator who builds
something taller gets a scrollbar, which is what they asked for by building it.

**The clamp is the interesting part, not the arithmetic.** `ROW_PX_MIN = 14` is where the
grid stops shrinking and lets the window scroll instead; below it panels stop being readable,
and a scrollbar is the honest failure. `ROW_PX_MAX = 48` keeps a very large monitor from
handing panels height their contents have no use for. The result rounds *down*: half a pixel
of generosity per row is 37 half-pixels of overflow, and a scrollbar offering eleven pixels
of nothing is worse than eleven pixels of margin.

**The drag math reads the same number.** `ROW_PX` was a constant used in exactly one place —
`metrics().rowPitch` — so it became the live `rowPx` and the pointer-to-cell conversion
follows the scale for free. Verified by dragging a resize handle three row-pitches at both
24px and 14px rows and checking the panel grew by exactly three cells each time; the risk
being guarded against is a grid that looks right and no longer lines up with the mouse.

Below 1024px the existing media query replaces the grid with a stacked column, where rows are
not used at all, so `fitRowHeight` removes the property and stands down rather than computing
a number nothing reads. `--cell-h: 24px` stays in the stylesheet as the value the grid uses
before the script runs and falls back to if it never does — a layout that works rather than
one that collapses. Resize events are coalesced to one pass per frame, and that pass ends by
dispatching `aether1:panels-changed`, which is how the terminal hears that its box moved.

### Step 44: a fullscreen face, and the avatar that would not fill it — **shipped**

Step 18a called this "a layout and a CLI flag away". The layout and the CLI flag took an
afternoon and the estimate was right about both. It was wrong about the third thing, which
is that an avatar drawn on a whole monitor was still the size of the panel it came from.

**The window.** `frontend/face.html` is an undecorated, fullscreen window holding the
hologram and one word — IDLE, LISTENING, THINKING or SPEAKING. `build_face_window` puts it
on a monitor the HUD is *not* on when the machine has one, because a fullscreen window that
lands on the screen you are working on is not a second screen feature. Three ways in, all
of them the same Rust command (`toggle_face_window_rust`): `aether1 face`, the tray's
**🙂 Fullscreen Face** item, and a **⛶** button beside the avatar panel's undock control.
Not persisted: asking for the face is asking for it now, not forever.

**A mirror in one direction only.** Like the desktop sprite it shows the HUD's own avatar,
driven by the four events the HUD pushes (`avatar-changed`, `color-theme-changed`,
`hologram-state-changed`, `hologram-audio-changed`). Unlike the sprite it cannot be clicked
to start the microphone. The sprite can, because it replaces the HUD on the desktop in front
of you; this window is on a screen across the room, and a fullscreen surface that starts
recording when something brushes the mouse is not a feature. Esc closes it, and that is the
whole of its input.

**The gate became a set.** State and audio are per-frame-ish traffic across the IPC
boundary, so `app.js` only emitted them while the sprite was open. That boolean is now an
`avatarMirrors` set: a window says `avatar-mirror-attached` when it opens and
`avatar-mirror-detached` when it closes. A set rather than a counter, so a face that reloads
cannot double-count itself into a gate that never closes. Attaching also triggers a snapshot
push — avatar, theme and current state — which closes a gap the sprite had all along: a
mirror opened mid-sentence used to sit on IDLE until the next thing happened.

**The postage stamp.** The first fullscreen screenshot had the avatar at 30% of the screen
height, and the reason was in `applyContentFit`: it only ever *widens* the camera's field of
view above `baseFov`, never narrows it. That is right for the HUD, where every avatar reads
at the resting size it was designed at and a panel with room to spare simply has room to
spare. On a monitor it means the avatar keeps its panel size and the monitor supplies the
empty space. So `setFillFraction`/`applyFillZoom` are the opt-in other half: the face asks
to fill the frame, nothing else does, and the HUD's zoom slider is untouched because the
default is null.

**The first fix did not work, and measuring said why.** With the fill zoom in, the avatar was
the same size. Reading the engine's own numbers out of the running page rather than guessing
gave it away in one line: `contentHalfHeight` was 80 for an avatar whose lettering is 40
units tall. a1 wears a soft glow — a 160-unit sprite at 0.18 opacity — and the bounding box
auto-fit measures is the glow's, not the avatar's. The fill was working perfectly and filling
the screen with haze.

So `expandBySolidParts` measures the same objects again, skipping anything you can see
through, and the fill path uses that box while auto-fit keeps using the full one. A glow
clipped against a panel edge would show; a glow running off the edge of a monitor is what
glow is supposed to do. The bounce allowance is dropped from the fill target for a related
reason — it is a flat 35 world units sized against a panel, while an avatar's actual idle
motion scales with the zoom applied here, so adding it again would reserve a third of a
monitor for a wobble of a few pixels. `fitMargin`'s 30% covers the motion at any size
because it is a proportion. Measured across every avatar, height went from 30% of the screen
to 47–86%, and the HUD's numbers came back identical.

**The zoom ceiling had to move, and only here.** `setZoom` clamped at 2.5, which is as far
as the HUD's slider goes; the a1 fill wants 2.65. `maxZoom` is now a field, raised to 6 only
for a window that has opted into filling, and put back — with the current zoom re-clamped —
if fill is ever switched off.

**The word needed its own room.** With the avatar actually filling the frame it grew straight
through the state label. The viewport now stops 9vmin short of the bottom and the word lives
in that band, which is a caption rather than an overlay.

### Step 45: the phrase is not the weak part -- **items 1, 2 and 3 shipped**

`serve_auth.rs` protects the pairing phrase carefully at rest -- 12 BIP-39 words, shown
once, never written down, with only the SHA-256 of a derived token on disk and a
constant-time comparison to check it. **Settled: 12 words stays.** 128 bits is past the
point where more words buy anything, so a longer phrase would be ceremony rather than
security.

What is missing is everything around it, in the order it matters:

1. ~~**TLS, self-signed.**~~ **Shipped.** `serve_tls.rs` generates a certificate on first
   `--lan` run, keeps it beside the pairing token so the fingerprint stays the same across
   restarts, and `server.rs` serves over TLS through `axum-server`. The whole SHA-256
   fingerprint is printed at startup, grouped in eights for reading aloud; truncating it
   would only have made the line shorter. Self-signed was never a compromise -- no public CA
   will issue for a `.local` name or a private address, so the alternative was owning a
   domain. Loopback deliberately stays plain HTTP: nothing off the machine can reach
   127.0.0.1, so there is no wire to listen to.
2. ~~**Rate limiting on the auth route.**~~ **Shipped.** An address gets five wrong guesses
   a minute, then waits a minute, counted across both doors -- a wrong token and a wrong
   phrase are the same attack seen twice, so they share one budget, and a request carrying
   no credential counts too. Getting it right clears the record. The table of addresses is
   capped and never evicts a live lockout, since filling it from other addresses would
   otherwise erase the evidence of one's own failures. `WORD_COUNT`'s comment about "a
   rate-limited LAN auth endpoint" is now true rather than aspirational.
3. ~~**A token per paired device.**~~ **Shipped.** The phrase and a device's credential are
   now two different things. The phrase opens the door once, at `/api/pair`, and is not
   accepted as a bearer token afterwards; each device that pairs gets its own 256-bit token
   from the operating system's generator, derived from nothing, so learning one says nothing
   about the phrase or about any other device's. Only the hash is kept, in
   `backend/serve_devices.json` at 0600, with a short id taken from that hash, a label the
   device sent, and when it paired. `accepts` folds over the whole list without stopping at
   a match, so the time a rejection takes says nothing about how many devices there are.
   `aether1 devices` lists them, `aether1 revoke <id>` cuts one off and leaves the rest
   alone, and `aether1 revoke all` cuts off every one while keeping the phrase you wrote
   down -- rotating with `aether1 pair` is still the harsher door, and it clears the device
   list too, because "any phrase paired before this no longer works" would otherwise have
   quietly stopped being true. An install from before this step keeps working: its one
   phrase-derived credential is recorded as a device, so nothing has to pair again on the
   upgrade, and that entry can be revoked like any other once it has.

   **Revoking had to take hold without a restart, and at first it did not.** `aether1
   revoke` is a separate process from `aether1 --serve`, so the running server went on
   honouring a token whose entry had just been deleted -- exactly the moment revoking is
   for. That was caught by running the two side by side rather than by a test; the server
   now stamps the device file's modified time and length and re-reads it when either
   changes, one `stat` per request, and there is a test standing on it. Verified live: a
   revoked device gets a 401 on its next request while the others keep their 200.
4. **A PAKE, later, if the rigour is wanted.** `discovery.rs` announces over DNS-SD, which
   anyone on the network can impersonate, so a phrase can be typed into a convincing fake.
   SPAKE2 -- what Matter and Thread commissioning use -- ends that class of attack by never
   letting the phrase cross the wire at all. It is real work and it is last, because items
   1 and 2 close the holes that are open today.

Items 1 and 2 were the ones with a hole behind them, and both are closed: the token no
longer crosses the network in the clear, and guessing costs something. Item 3 turned out to
be worth doing immediately after rather than later, because it is what makes the other two
recoverable -- TLS and rate limiting stop a stranger getting in, and per-device tokens are
how you get a device *out* once it is lost. Item 4 is the one that can still wait: it is
rigour on a path that is no longer broken.

### Step 46: updating a copy that was installed rather than cloned

**The updater that exists today only works for the developer.** `main.rs` asks
`api.github.com` for the latest commit on `main`, and `perform_update_core` runs
`git pull --ff-only` in `project_root()` and rebuilds. Both halves assume a git checkout,
and the check assumes `gh auth token` returns something -- the repository is private, so an
unauthenticated request 404s and `UpdateCheckError::NoGithubAuth` tells the operator to
install the `gh` CLI and log in.

That is a reasonable developer workflow and it is not a product. Someone who installed from
`aether1-offline-linux-x86_64.tar.gz` or the Windows installer has no checkout, no `gh`, and
no path to a newer version at all. **And the two halves of the project disagree about what
an update even is:** the tray compares commits on `main`, while `release.yml` builds tagged,
versioned bundles and attaches them to a GitHub Release on `v*`. The updater cannot see
releases; the release pipeline has no client.

**A credential shipped in the binary is not the fix.** Anything in a distributed app can be
read out of it, GitHub's secret scanning revokes tokens that appear in public artifacts, and
there is no obfuscation that changes either fact.

**The operator does not have to be the one holding the credential, though.** If each person
signs in to GitHub as themselves, the app carries no secret at all and access becomes a real
check rather than a guess: reading the private repository's releases requires being a
collaborator on it, so adding someone grants access and removing them revokes it. That is
what "subscribed to the project" means in practice, and it is the mechanism GitHub already
has.

The flow is the **device flow**: the app shows a short code, the person opens
`github.com/login/device` in a browser and approves, and the app receives a token for that
person. It exists precisely for applications that cannot keep a secret -- the only thing
compiled into the binary is a client ID, which is public by design and grants nothing on its
own.

**Use a GitHub App rather than a classic OAuth App.** A classic OAuth App would have to ask
for the `repo` scope, which is read *and write* to every repository that person can reach --
an alarming thing to request for the sake of a version check, and rightly so. A GitHub App
scopes down to `contents: read` on the repositories it is installed on, so the token the app
holds can do one thing. Device flow has to be enabled in the App's settings; it is off by
default.

**Signing matters more than where the file lives.** Once artifacts are signed and the app
verifies the signature against a public key compiled into it, the download can come from
anywhere, over any mirror, and a tampered file simply fails to install. That inverts the
problem from "keep the location secret" to "make the location irrelevant", which is the only
version of this that stays true.

The plan:

1. **Sign in to GitHub in the app, and keep the releases where they already are.** The
   private repository's own Releases are the distribution; `release.yml` already puts the
   bundles there. Nothing new is hosted, nothing is published, and the source stays private
   and unforkable. Access is whoever the owner has added to the repository, which is a list
   that can be added to and taken away from.

   *Not a public artifacts repository*, which was this step's first answer and is wrong
   here: a public repository appears on the owner's profile and in search, so it fails the
   requirement that releases reach only people who were let in. *Not an unlisted bucket*
   either, which was its second: it works, and it asks nobody to sign in, but an unguessable
   URL is obscurity rather than access control -- it cannot be taken back from someone, and
   the URL would have to live in the binary, which makes it exactly as private as the binary
   is. Worth keeping in mind only if signing in ever becomes the wrong price to ask.
2. **Sign every artifact, and verify on the client.** Windows installers are already signed
   through Azure Trusted Signing in `release.yml`; the Linux bundles are signed by nothing at
   all. A minisign keypair covers both uniformly: the private key lives in Actions secrets,
   the public key is compiled into the binary, and no release is installable without it.
3. **Point the check at releases, not at `main`.** Compare this build's version against the
   latest published release -- `/repos/{owner}/{repo}/releases/latest`, with the signed-in
   token, and the asset fetched from the release's asset endpoint. A commit on `main` is not
   a release, and telling someone they are "behind" because a README was fixed is noise.
4. **Keep the git path, but as the developer path.** When a `.git` directory is present the
   current pull-and-rebuild behaviour is the right one and should stay. When it is absent,
   the app fetches the signed bundle instead. Which mode a copy is in should be visible, not
   inferred silently.
5. **Do not self-install silently.** The offline bundles are around half a gigabyte. The
   honest flow is to say a version is available, ask, download with a progress bar the HUD
   already knows how to draw (step 31), verify the signature, and hand over to the
   installer.
6. **Keep the token somewhere better than the settings table.** `llm_api_key` already lives
   there as plain JSON, so there is precedent, but a GitHub token is a different kind of
   secret: it is an identity on someone else's account rather than a key to a service they
   chose to pay for. The OS keychain -- Credential Manager on Windows, libsecret or
   kwallet on Linux -- is where it belongs, and `contents: read` on one repository keeps the
   damage bounded if it ends up somewhere else anyway.
7. **Not signing in is not an error.** Someone who declines still has a working companion;
   they just do not get told about new versions. The check should say so plainly once and
   stop asking, rather than nagging or degrading anything else.

**On Tauri's own updater.** Tauri v2 ships `tauri-plugin-updater`, which does exactly the
signature-verified flow described above, and it is not configured here -- there is no
updater block in `tauri.conf.json` and no pubkey. It is worth adopting only if the bundles
move to formats it can install (AppImage, `.deb`, NSIS or MSI). This project's bundles are
bespoke: an Inno Setup installer on Windows, and Linux tarballs carrying Piper, whisper.cpp
and their models. Verifying minisign signatures directly is the smaller change and keeps the
packaging that already works; the plugin is the better answer only if the packaging is being
revisited anyway.

**Settled: the source stays private, access is by GitHub identity, and nothing is hosted.**
The project is not to be forkable while it is still being built, which the private source
already achieves; releases reach the people the owner has let in and nobody else; and no
endpoint is to be run.

**What this costs, stated plainly:** a GitHub account and a one-time browser approval become
the price of automatic updates. For a project whose access model is "subscribed to the
project" that is the point rather than a drawback, and it buys something obscurity cannot --
access that can be taken back. A link, once given, is given forever.

**It also replaces, rather than removes, the current `gh` requirement.** Today the check
shells out to `gh auth token`, so it needs the GitHub CLI installed and logged in; the device
flow needs a browser, which every machine running this already has. It is strictly less to
ask for than what is there now.

Signing still matters and is still item 2. Authentication proves who may download; a
signature proves what was downloaded. A token that is stolen, a release asset replaced, or a
copy passed hand to hand on a USB stick are all cases where the second question is the one
that counts.

### Step 47: diagnostics that fixes things, and a fix that has to prove itself

**What this app calls diagnostics today is a report about the machine, not about itself.**
`aether1 status` prints `Telemetry::diagnostic_report()` -- CPU, RAM, disk, network, top
processes -- and `aether1 status --events` sweeps the system log for anything that went
wrong recently. Both describe the computer AETHER1 is sitting on. Neither answers the
question an operator actually asks when something feels off, which is *is AETHER1 working?*
The closest thing to that answer is `setup::advise()`, and it looks at exactly one
subsystem, the model, in the five stages a fresh install moves through and no further. Every
other subsystem reports its own health in its own way, to whoever happened to call it: the
voice fails inside `tts.rs`, the hotkey never fires on Wayland and says nothing, a missing
`coredumpctl` is mentioned only if you run `aether1 crashes`.

So the first half of this step is not self-healing at all. It is deciding what "everything
is running" means, as a list, in one place.

#### A check is a pure function of an observation

This is the shape the whole step turns on, so it comes first. Every check is split in two:
a **probe** that touches the world and returns a plain data structure, and a **verdict** that
reads that structure and returns `Ok`, `Degraded` or `Failed` with a `setup::Step` attached
for the human. `advise()` is already written this way -- it takes a `ScanResult` and two
facts and decides, which is why its whole table of cases is unit-tested without an Ollama
anywhere near it -- and that is the pattern the rest should follow.

Three things follow, and they matter more than the tidiness:

- An observation can be **recorded**. The bytes that made a check fail on one machine can be
  written down, carried to another machine, and replayed.
- A check can therefore be **tested against a machine it never saw**, in CI, for ever after.
- A repair can be **verified** by re-running the same verdict against a fresh probe, which is
  the difference between fixing something and hoping.

The second half of this step -- the part about whether a fix is a real build requirement --
is impossible without this and trivial with it.

#### What "everything is running" means

Two classes, and the difference between them decides everything downstream.

**Things AETHER1 owns.** A fault here is AETHER1's own bug, and no amount of installing
things on the machine is the answer.

1. **The memory database.** `MemoryDb` opens, the schema is at the expected version, a write
   round-trips, and the filesystem under it has room. A read-only or full disk is the most
   common way this one fails.
2. **The HUD is answering.** Step 32 already deals with a window that stopped responding;
   this makes that a named check rather than a symptom.
3. **The tray is present.** On Linux a tray icon needs a StatusNotifierItem host, and on a
   bare window manager there is not one -- the icon simply never appears, which today looks
   identical to the app not starting.
4. **Background services match what was asked for.** Autostart is on and `ManagedOllama`
   holds a live child, or autostart is off and it does not. A child that died is a fault;
   an Ollama the operator started themselves is not AETHER1's to account for.
5. **The watchers are watching.** The crash watcher has its reader (`coredumpctl` on Linux,
   the event log on Windows), the poll is ticking, and the last sweep was recent.
6. **The server, when it is meant to be up.** With `--serve`, the port is bound; with
   `--lan`, the TLS certificate loads, its fingerprint matches the one that was shown, and
   mDNS is announcing.
7. **Consent is not silently stuck.** Proposals expire after fifteen minutes by design, so a
   queue of expired ones nobody ever answered is a sign the card is not reaching the
   operator at all.

**Things AETHER1 depends on.** A fault here is about the machine, and this is the class where
"fix it" can mean anything.

8. **A model answers.** `model_scanner::models_at(endpoint)` returns something, and the
   distinction step 19 insists on holds: no answer is a different fault from an empty list.
9. **The configured model is installed.** Routing already knows this (`reported_missing`) and
   already falls back with a notice; the check makes it visible before someone types at it.
10. **Voice out.** An engine is found -- Piper, espeak-ng, or SAPI through PowerShell -- and
    the voice files a Piper voice needs are both present, not just one.
11. **Voice in.** The whisper binary runs and the model file is there and complete.
12. **The hotkey is registered.** And, on Wayland, the check must say that a registered
    global shortcut never fires there and the compositor binding is the supported route.
    A known-impossible thing reported as healthy is worse than reporting it broken.
13. **Room to work.** Enough disk for the model, voice and STT downloads that are configured
    but not yet fetched.
14. **The update path.** Signed in, or declined once and deliberately quiet (step 46). Never
    nagging.

`aether1 doctor` runs the list and prints it; `aether1 doctor --fix` runs the ladder below.
It is a new verb rather than another flag on `status` because `status` is about the machine
and this is about the app, and conflating them is what got us here. The HUD gets the same
list in the settings panel, since the person most likely to need it is the one who cannot
get a sentence out of the companion.

**Three moments, and no fourth.** The whole list runs once at startup, so the app knows what
it is standing on before the operator asks it for anything. After that nothing sweeps on a
timer: a subsystem that fails during ordinary use runs its own check there and then, which
is both cheaper and more precise than polling, and everything else waits for the operator to
press Diagnose or type `aether1 doctor`. A companion quietly probing ports and spawning
processes every minute costs more than it is worth, and these checks are individually cheap
only for as long as they are not all running all the time.

#### The ladder: how a failed check gets fixed

Four rungs, cheapest first, and nothing skips a rung.

**Rung 0 -- say it.** Every verdict carries a `setup::Step` already: an imperative, a
sentence of why, and a command or a link. For most failures this is the whole fix and the
operator does it in five seconds. `install_steps()` and `start_steps()` are already written
per-OS with links; the rest of the checks owe the same.

**Rung 1 -- the known repairs.** A table in the binary, `CheckId -> a repair already
expressible as an existing tool`: start the managed Ollama, re-register the hotkey, retry
the voice download that was interrupted, re-open the database. These are deterministic, they
are written by hand, and **they need no model at all** -- which is the point, because a
machine whose model endpoint is down is exactly the machine that cannot ask a model for
help. Each one goes through `tools::consent`: proposed, shown, approved, logged with who
approved it. A repair that is on the always-allow list runs without asking, and the log says
it was a rule rather than a person, as it already does.

**Rung 2 -- an agent.** Only when no known repair matches the check, or the known repair ran
and the re-check still fails. This is step 15's hand-off with the failed check's bundle as
context: the observation, the verdict, what rung 1 tried, and the step 37 machine
description. The bundle is prose rather than JSON, for the reason step 13 already settled --
small local models do better with sentences.

The consent model does not loosen because the task is a repair:
- The agent runs where step 15 says it runs -- the operator terminal's directory, or the
  nominated work directory, never `$HOME`.
- Foreground by default, with the operator watching. Background only under step 15's rule:
  the plan is disclosed and approved whole up front, because an unattended agent must not
  stall on a permission prompt.
- Per-persona access and per-request elevation apply unchanged. "It is fixing something" is
  not a reason to widen a grant, and a repair that needs elevation asks for it like anything
  else.
- **It still never installs an engine on its own.** That principle predates this step and
  survives it. An agent may tell you what to install and why, and may run the package
  manager if you approve that specific command; it does not decide to.

**What the agent may repair is the ground AETHER1 stands on, not AETHER1.** The foundation
is fair game: a service that is not running, a missing package, a permission, a
half-downloaded file, a path pointing at nothing. AETHER1's own source is not -- on any
machine, including a developer machine with the checkout sitting right there. And when the
fault turns out to be in *another program*, the agent stops at words: it says what it found
and what it would do, and the operator decides. Diagnosing is allowed everywhere; acting is
allowed on AETHER1's foundation and nowhere else.

**Rung 3 -- stop.** One known repair and one agent hand-off per check per session, then it
stops and says plainly that it could not fix this. A loop that keeps trying is worse than a
broken check, because it burns tokens and the operator stops reading it.

**The re-check is mandatory.** After every repair, the probe runs again and the same verdict
decides. A repair whose re-check still fails is recorded as a failed repair, which is
evidence too -- knowing that a plausible fix does not work is worth as much as knowing one
does.

**And self-healing must not become a place bugs go to hide.** If the same check is repaired
on every launch, that is not healing, it is a defect being suppressed once a day. The third
identical repair of the same check should be reported as its own finding, separately from
whatever it is nominally fixing.

#### Feeding a fix back, and finding out whether it is a real requirement

**A fix that worked on one machine is evidence, not a requirement.** That is the whole
difficulty. The repair succeeded, so something was wrong; but "this machine was missing a
package" and "every build is missing that package" look exactly the same from inside the
machine that was missing it. Nothing about a successful fix distinguishes them. Only a test
does, and the test has to run somewhere that machine is not.

**1. The repair record.** Every attempt -- rung 1 or rung 2, succeeded or failed -- writes
one record: the check, the recorded observation that produced the verdict, the OS, desktop
and versions that matter to that check, exactly what was run or changed, and the re-check
result. Successes and failures are recorded identically. This lands in the existing action
log, which already keeps what was proposed, who approved it, and what happened.

**2. Nothing leaves the machine unseen.** A repair record contains paths, a hostname, a
package list and possibly a command line the operator typed. It is shown in full, it is
editable before it goes anywhere, and it goes nowhere by default. Consent is per report, not
a standing switch -- the same reasoning as step 16's per-call MCP consent: agreeing to send
one report is not agreeing to be a telemetry source.

**3. What "reported back to the repo" is, and when it is offered.** When the record looks
like a missing dependency there is nobody to tell: the fix is a line in `setup.rs` so the
next person is told about it, and that is a change a person makes. When it looks like an
actual bug in AETHER1, the app offers to notify the developers, and taking that offer opens
an **issue** carrying the record and the proposed test.

An issue, and deliberately not a pull request: a machine-generated patch to the build is
precisely the thing that must not arrive ready to merge, and the issue is where the triage
below happens anyway. Step 46 already
brings a GitHub identity into the app via device flow, so there is somebody to open it as --
but that App is scoped `contents: read`, and opening issues needs `issues: write` added to
it. That is a real widening of what the app can do to the repository and is called out here
rather than assumed.

**4. The proposed test is the point.** Because a check is a pure function of an observation,
the observation that failed can be committed as a fixture, and the test reads: *given this
observation, the check fails; given the observation after the repair, it passes.* It needs
no Ollama, no Windows, no particular desktop -- it is a table entry beside the ones
`setup.rs` already has. That is the mechanism by which a one-machine patch is promoted to a
build requirement: it stops being a story about a machine and becomes a row in `cargo test`
that will fail for everyone, for ever, if the check regresses.

**5. Three verdicts, and a human picks.**
   - **A bug, which is to say a build requirement.** The check was wrong or missing, or the app genuinely needs
     something it does not ship or look for. The fix belongs in the code or the packaging --
     a wider search path, a bundled dependency, a corrected probe.
   - **A dependency, which is to say an environment fact.** The machine really was missing something the operator has to
     install. Then the fix belongs in `setup.rs` as a per-OS step with a link, so the *next*
     person is told rather than silently patched. This is the case that gets mistaken for
     the first one most often.
   - **One machine.** Nothing to change; the record stays as history and the repair table
     keeps its entry.

   The evidence that separates the first from the third is **repetition and replay**: the
   same observation arriving from a second machine, or the fixture failing on a clean CI
   runner. One report is an anecdote.

**6. CI is the arbiter, and it has a hole worth naming.** `ci.yml` builds, tests, clippies
and formats on `ubuntu-latest` only. A repair observed on Windows cannot be proven by CI as
it stands. This is the strongest argument for the fixture approach being the primary
mechanism rather than a nicety -- a recorded observation replays on any runner, so a
Windows-shaped fault can be tested on the Linux runner that already exists. Adding a
`windows-latest` job is a separate and larger question; step 13 shipped two platform readers
with only one of them ever executed in CI, and that is the debt this would start paying.

**7. The loop has to close, or the repair table rots.** When a repair becomes a build change,
its entry in the rung-1 table is removed in the same commit -- the check now passes on its
own. Otherwise the table grows into a pile of workarounds quietly papering over bugs that
were fixed years earlier, and every one of them costs a consent prompt on somebody's
machine.

**What this step is not.** It is not an app that rewrites its own source. An agent at rung 2
repairs the *machine*; a change to AETHER1's code leaves as a report for a person to act on,
even on a developer machine that happens to have a checkout sitting right there. The
difference between a program that fixes its environment and a program that edits itself is
worth keeping sharp, and nothing here needs the second one.

**Settled 2026-09-20.** The agent repairs the machine and never AETHER1's code; the point of
repairing the machine is that the app should be running on a solid foundation, so the
foundation is what it is allowed to touch. A fault in another program earns a notification
and a suggestion, and the operator decides from there. A record that looks like a bug rather
than a missing dependency gets an offer to notify the developers, which is a bug report on
this repository, reviewed like any other. And it runs at startup, again the moment something
actually fails, and whenever the operator asks -- never on a timer, because the cost of
watching has to stay smaller than the cost of the thing being watched.

## Where this stands

*Rewritten. The list below had gone stale: it still named the consent path, local voice and
the vault as what to do next, and all three shipped some time ago.*

**Done.** Phase 1 entire (CLI, hotkey, streaming, local TTS and STT). Phase 2 entire (tool
registry, the read-only loop, the consent path, mutating tools and undo). Phase 3 entire (the
vault, priming from it, writing back, search and archiving, and — since step 35 — every
conversation folded into a dated note without being asked, and since steps 39–40 read and
drawn inside the app, and since step 41 kept in conversations you can reopen). Plus, since
step 42, a real terminal in the HUD that the companion provably cannot reach, and since
step 44 the avatar on a spare screen of its own. Plus local-only
mode, the theme engine, the top bar, personas as specialities, per-persona access with per-request elevation
*and* operator-widened fields, a command allowlist that ships usable, reading the Windows
event log, honest token telemetry, and native tool calling.

**Outstanding, in the order they are worth doing:**

1. **Installing the Piper engine automatically.** Step 38 took the voice half: Aether1
   fetches the two files itself now. The engine is deliberately still a package-manager
   job, because downloading something that runs is a different risk from downloading
   something that is read — so this stays outstanding on purpose rather than by omission.
2. **Push-to-talk from outside the HUD.** Both engines work; holding a key to talk from
   another application needs an OS-level press-and-hold the global-shortcut plugin does not
   express yet.
3. ~~**Step 13, crash capture.**~~ **Shipped**, on both platforms. What is left is what a
   headless container cannot check: the tray colour, the notification, and the Windows reader
   against a real event log.
4. **Step 19, several local models.** A stated core requirement, and still deliberately not
   started. Step 29 makes it closer than it was: the catalogue, the memory sizing and the
   download path it would need already exist in `setup.rs`. Its two design questions are
   now answered -- routing is keyed on speciality and chosen from a dropdown -- so what
   remains is the building.
5. **Step 45, the LAN transport.** TLS, attempt limiting and per-device tokens shipped, so
   a lost machine can now be cut off on its own with `aether1 revoke <id>`. What remains is
   a PAKE, if the rigour is ever wanted -- it closes a convincing-fake attack rather than a
   hole that is open today.
6. **Step 46, updates for installed copies.** The updater in `main.rs` needs a git checkout
   and a logged-in `gh`, so everyone who installed from a Release is on whatever version
   they downloaded, permanently.
7. **Step 47, diagnostics that fixes things.** `aether1 status` describes the machine and
   nothing describes the app, so "is AETHER1 working?" has no answer and every subsystem
   reports its health to whoever happens to call it. Specified, not started.

Steps 15–18 (agent handoff, MCP, packaging, memory sync) are now specs rather than sketches,
settled in that order and written up in full above.
