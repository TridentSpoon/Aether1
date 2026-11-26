# AETHER CODE implementation handoff

Read `AGENTS.md`, `HOW_THIS_IS_BUILT.md`, `GOALS.md`, `AGENT_RUNTIME.md`, and `SECURITY_MODEL.md` before continuing. Check `git status` and the active GitHub identity before any write. Preserve the operator's existing untracked `.mcp.json` and `scripts/check_unix_net_gated.sh`.

The full backlog and acceptance criteria are in [CODING_PLATFORM_IMPROVEMENTS.md](CODING_PLATFORM_IMPROVEMENTS.md). Treat each package below as independently reviewable. Do not claim one is complete until its verification is recorded here.

## Package A: engine continuity and event stream (items 1–2)

Status: in progress. `code_tasks.rs` stores an optional OpenCode session per task; `code_engine.rs` requests JSON events for panel runs; `commands.rs` passes the stored ID back and returns tool summaries. The original `aether1 code do` CLI still uses plain output. The panel renders completed actions from the returned events on both Tauri and HTTP paths; Tauri also streams brief action lines during execution. Unit tests cover session storage and structured event parsing. Next: verify the exact JSON event shape against installed OpenCode 1.18.31 with a live local model, confirm session IDs resume across two interleaved tasks and a restart, and test the activity view in the installed app.

## Package B: evaluation and recovery (items 3–4)

Status: partially implemented. Both the panel and CLI take a checkpoint before launching the external engine. The panel shows its reference and offers a two-step preview and restore action. The preview and restore use the same backend functions through Tauri and HTTP. A checkpoint test covers modified, deleted, staged new, and untracked new files. The task list now preserves the latest checkpoint after reopening. Restore compares a fingerprint of tracked and untracked changes against the preview and refuses a stale preview. A three-case evaluation runner now creates disposable repositories and isolated Aether1 databases; direct OpenCode uses Bubblewrap plus a local model relay. Older checkpoints are now listed in the coding panel through both Tauri and HTTP, and each uses the same guarded preview and restore flow. New checkpoint refs are unique even for rapid successive saves; legacy refs remain readable. Next: run both evaluation modes against the same installed local model on a host that permits Unix sockets and network namespaces, and test the panel controls in an installed app.

## Package C: long-task context and journal (items 6–7)

Status: partially implemented. The engine session from package A is reused across turns. Completed and failed structured tool events are saved in the existing action log, scoped to the task session and linked to the run's checkpoint. The coding panel exposes the latest 100 actions through Tauri and HTTP; only bounded tool, title, and target summaries are stored, without tool input contents. Journal write failures are shown after the run. Remaining: inspectable task decisions and a long-task context strategy, plus a live engine run to verify event delivery end to end.

## Package D: Windows confinement (item 5)

Status: not started. Requires a Windows implementation and Windows CI validation. Preserve the existing refusal until the OS boundary is demonstrated. Do not use an allowlist or working directory as a substitute for confinement.

## Package E: bounded host and browser tools (item 8)

Status: not started. Add one narrow capability at a time. Any new socket, HTTP path, or networking crate needs the gate described in `HOW_THIS_IS_BUILT.md` and a corresponding guard update.

## Package F: installed experience (item 9)

Status: not started. Run only after packages A–E have reviewable outcomes. Exercise fresh install, local model, project task, action inspection, restart, and recovery on each supported platform. Record unavailable hardware or platforms explicitly.

## Current verification

`cargo check` and `cargo fmt --check` passed after adding the evaluation runner. Python syntax parsing passed. The fixed-fixture smoke runs produced result files and classified missing-model and socket-permission refusals as blocked. `cargo test code_tasks` passed (17 tests); `cargo test code_checkpoint` passed (4 tests, including stale-preview refusal and rapid checkpoint refs); structured event parser and session capture tests passed. `cargo test code_engine` had 15 passes and one existing Bubblewrap test failure because this host cannot create a NETLINK_ROUTE socket for its loopback namespace. `node --check frontend/js/app.js` and `git diff --check` passed. The guard loop passed except the pre-existing untracked `scripts/check_unix_net_gated.sh`, which reports two `UnixStream` uses inside functions marked `#[cfg(unix)]`; inspect the script before treating this as a code defect. `check_voice_catalogue.sh` skipped its remote index because DNS was unavailable. A no-model Aether1 smoke run confirmed that the evaluation database was created under /tmp and was reported as blocked. A direct-mode smoke run was blocked because this host forbids Unix socket creation. No live model score or Windows validation has happened in this task.
