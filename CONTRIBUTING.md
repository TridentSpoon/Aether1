# Contributing to Aether1

Fixes and improvements are welcome. This file is short on purpose: it is what you need to
get a change building and accepted, and nothing else.

## Before you write anything

**Read [docs/GOALS.md](docs/GOALS.md).** It says what Aether1 is for and — more usefully —
what it deliberately is not. A well-built change that pulls against it is a change that
gets turned down, and neither of us wants to find that out after you have written it.

Two boundaries in particular are not up for negotiation, because the project's whole
premise rests on them:

- **Nothing leaves the machine that the operator did not send.** Local-only mode must keep
  working. A new feature that phones somewhere has to be off by default and say so.
- **The terminal stays out of the model's reach.** `scripts/check_terminal_isolation.sh`
  enforces this and the build fails if it is broken. The companion suggests commands; the
  operator's own keypress runs them.

## Building it

```sh
git clone https://github.com/TridentSpoon/Aether1.git
cd Aether1
./setup.sh          # or setup.bat on Windows
```

`setup.sh` installs your distribution's build dependencies, builds the app, and puts it on
your `PATH` as `aether1`. It is safe to re-run.

## What has to pass

Run these before you open a pull request. CI runs the same two jobs and nothing else, so a
clean run here is a clean run there.

```sh
cd src-tauri
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cd ..

# The frontend job: every file has to parse, and six guards have to hold.
find frontend -name '*.js' -not -path 'frontend/vendor/*' -exec node --check {} \;
for s in scripts/check_*.sh; do bash "$s"; done
```

**If you touched anything in `frontend/`, re-run `./scripts/build_vendor_css.sh` and commit
the result.** The Tailwind stylesheet is a static build containing exactly the classes the
scanner found, so adding or removing markup changes it. CI diffs it and fails if it is
stale. This catches more first-time contributors than anything else here.

## Pull requests

- **Say what changed for the person using it**, not only what changed in the code. "Before:
  … After: …" is the house style and it is worth the two minutes.
- **One change per pull request.** A fix and a refactor in the same diff are two reviews
  wearing one hat.
- **Comments explain why, not what.** The code says what. This codebase leans on comments
  that record the reasoning and the thing that was tried and did not work — match that.
- **A bug fix comes with a test that fails without it.** Not negotiable for anything in
  `src-tauri/src`.

## Licensing of what you send

Aether1 is GPL-3.0-only. By opening a pull request you are offering your contribution under
that same licence. There is no separate agreement to sign and nobody asks you to hand over
your copyright — you keep it. This is just what keeps the guarantee in
[the README's licence section](README.md#licence-and-what-a-fork-owes-you) true for whoever
comes next.

## Reporting something instead

A bug report that says what you did, what happened, and what you expected is worth more
than a patch that guesses. `aether1 doctor --report` writes a diagnostic bundle you can
attach; read it first and take out anything you would rather not post.

If what you have found is a security problem, please do not open a public issue — see
[SECURITY.md](SECURITY.md).
