# Working in Aether1

These instructions apply to Codex, Claude Code, AETHER CODE, and other coding tools working
in this repository.

## Before changing code

- Read the relevant module and its callers before editing. Follow the existing Rust, JavaScript,
  and UI patterns instead of introducing a parallel path.
- Read `docs/GOALS.md` for product boundaries. For agent permissions, repository access, or
  sandbox changes, also read `docs/AGENT_RUNTIME.md` and `docs/SECURITY_MODEL.md`.
- Preserve Linux and Windows support. If a capability is platform-specific or unsupported,
  report that explicitly and fail closed.
- Check `git status` before work. Preserve existing user changes; do not reset, clean, rebase,
  commit, or push unless the user explicitly asks.

## Implementation

- Keep changes focused and make refusal/error messages tell the operator what to do next.
- For Rust, keep decisions testable and document why a boundary exists. Do not weaken path,
  GitHub, permission, sandbox, or terminal-isolation checks to make a flow appear functional.
- Keep frontend and backend contracts in sync, including both the Tauri IPC and HTTP paths.
- AETHER CODE changes belong only in the nominated workspace and must use its existing
  permission and checkpoint mechanisms.

## Review and verification

- For reviews, report actionable findings first, ordered by severity, with file and line
  references. Separate confirmed defects from questions and residual risks.
- Run focused checks for the affected code when practical. State exactly what was run and
  what could not be verified, especially when a live model, GitHub authentication, or a
  platform-specific environment is needed.
- Do not claim that an external agent, provider, or repository operation ran unless it did.
