# Working in Aether1

These instructions apply to Codex, Claude Code, AETHER CODE, and other coding tools working
in this repository.

## Before changing code

- Read the relevant module and its callers before editing. Follow the existing Rust, JavaScript,
  and UI patterns instead of introducing a parallel path.
- Read `docs/HOW_THIS_IS_BUILT.md` first. It lists the properties that must stay true and
  the script that proves each one, including every way out to the network. Both network
  holes this codebase has had were introduced by authors who had a good reason for what they
  wrote and no reason to think about the rule they were breaking.
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
- Run the guard scripts before proposing a change: `for s in scripts/check_*.sh; do bash "$s" || echo "FAILED: $s"; done`.
  They are the machine-checked half of `docs/HOW_THIS_IS_BUILT.md`, and each failure message
  says what to do next.
- Run focused checks for the affected code when practical. State exactly what was run and
  what could not be verified, especially when a live model, GitHub authentication, or a
  platform-specific environment is needed.
- When a change adds a way out to the network -- an HTTP request, a socket, or a crate that
  does its own -- say in the pull request which gate it passes through and cite the rule in
  `docs/HOW_THIS_IS_BUILT.md` it answers to.
- Do not claim that an external agent, provider, or repository operation ran unless it did.
