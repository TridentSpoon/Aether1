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

### Step 7: Mutating tools and undo

- **`src-tauri/src/tools/`** — `set_aether_setting`, `write_file` (snapshot the previous
  contents into `undo_json` first), `run_command` (allowlist-gated, no shell interpolation,
  argv only, with a timeout), `service_control` (allowlist-gated).
- **`src-tauri/src/tools/mod.rs`** — an optional `undo` on `Tool`, and
  `commands::undo_action(id)` replaying it from `undo_json`.
- **`frontend/js/app.js`** — an action history panel listing recent actions with an Undo
  button where one is available.
- **Verify:** the companion changes its own persona on request via the approval flow; a
  written file can be reverted from the history panel.

### Step 8: Native tool calling

- **`src-tauri/src/llm/providers.rs`** — real `tools` / `tool_use` blocks for Anthropic,
  the OpenAI-compatible shape, and Gemini, each with typed request/response structs the way
  the existing calls are built. Keep the step-5 prompt protocol as the automatic fallback
  for Ollama and any model that tool-calls badly.
- **Verify:** the same conversation produces the same actions on a cloud provider and on a
  local model, one through native tool calls and one through the text protocol.

---

## Phase 3 — Memory that earns its name

### Step 9: Schema for real memory

- **`src-tauri/src/llm/db.rs`** — `long_term_memory` gains `source` (`stated` / `observed`
  / `derived`), `confidence`, `last_seen`, `expires_at`. Add a `sessions` table with a
  summary column. Keep the existing columns and defaults so old databases keep working —
  `ALTER TABLE ... ADD COLUMN` guarded by a `PRAGMA table_info` check, in the same
  `open()` path as the creates.
- **`src-tauri/src/llm/mod.rs`** — `check_instant_commands`'s `remember that` path writes
  `source = 'stated'`; tool executions write `source = 'observed'` facts about what was
  done.
- **Verify:** an existing `aether1_memory.db` opens unchanged and gains the columns.

### Step 10: Retrieval

- **`src-tauri/src/llm/db.rs`** — an FTS5 virtual table over memory and message text
  (`rusqlite`'s bundled SQLite has FTS5), kept in sync by triggers. `search_memory(query,
  limit)`.
- **`src-tauri/src/llm/mod.rs`** — `system_prompt` currently pastes the first 10 memories
  verbatim. Replace that with a retrieval against the prompt, so what's recalled is what's
  relevant. This is the single highest-value change in Phase 3.
- **Later, optionally:** swap FTS for embeddings once there's a local embedding model worth
  the RAM. Keep the `search_memory` signature stable so that swap is one module.
- **Verify:** with 200 stored facts, asking about one topic recalls that topic's facts and
  not the ten oldest.

### Step 11: Consolidation and decay

- **New `src-tauri/src/llm/consolidate.rs`** — on session end (or a timer), summarise the
  session into the `sessions` table and extract durable facts from it using the configured
  model itself. Facts not seen for a long time lose confidence; expired ones stop being
  retrieved.
- **Verify:** a long conversation leaves behind a short summary and a handful of facts
  instead of unbounded history.

### Step 12: Memory browser

- **`frontend/js/app.js` + `index.html`** — a panel listing memories with source,
  confidence and age; edit and delete per row; a full-text search box; the action log
  beside it.
- **`commands.rs`** — `list_memories`, `update_memory`, `delete_memory`, both transports.
- **Verify:** you can read everything the companion believes about you and delete any of
  it, without touching SQLite by hand. This is the promise in GOALS.md principle 5 — it
  isn't kept until this ships.

---

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

## Suggested first move

Steps 1 and 2 together are a weekend and change the daily experience more than anything
else on this list: the companion becomes something you summon rather than something you
open. Step 3 is the one to budget properly — streaming touches every layer, and doing it
before the tool loop saves redoing it after.

Step 4 is worth landing early even though nothing is user-visible when it does, because the
registry and the action log are what the whole capability layer hangs from, and step 6's
consent path must exist before the first mutating tool does.
