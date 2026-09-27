# Reporting a security problem

**Please do not open a public issue for a security problem.** Use GitHub's private
reporting instead: the **Security** tab on this repository → **Report a vulnerability**.
That opens a thread only the maintainer can see, and it does not need an email address
from either of us.

If private reporting is unavailable to you for any reason, open an ordinary issue saying
only that you have found something and would like somewhere private to describe it. Say
nothing about the problem itself in that issue.

## What is worth reporting

Aether1 reads files, runs programs, speaks, remembers, and can serve its interface to your
local network. Anything that crosses one of these lines is worth a report:

- A way to make the companion run a shell command. The model is deliberately kept out of
  the terminal (`scripts/check_terminal_isolation.sh` guards it); a route around that
  boundary is the most serious thing you could find here.
- A way to read or write files outside what the operator has granted — the path guard in
  `src-tauri/src/tools/fs_guard.rs` denies `~/.ssh`, `~/.aws`, credential stores and the
  like, and a bypass of it matters.
- A way for something on the LAN to reach the served HUD without a valid device token, or
  to use another device's token.
- A way to make **Local only** mode send something off the machine anyway. That switch is
  a promise, and a hole in it is a broken promise rather than a missing feature.
- Anything that gets a stored credential — a model provider's API key, the GitHub sign-in
  token in the keychain — into a log, a conversation, a crash report or a diagnostic
  bundle.

Prompt injection deserves a word of its own. Aether1 reads files and web pages that the
operator points it at, and content in them can try to redirect the companion. Reports here
are welcome, and the interesting ones are those where injected text causes something to
happen **without the operator confirming it** — the consent prompts are the control, so a
way around a prompt is a real finding, while "the model said something it was told to say"
generally is not.

## What to include

What you did, what happened, and what you expected instead. A reproduction beats a
description. `aether1 doctor --report` writes a diagnostic bundle — read it before you
attach it and remove anything you would rather not share.

## What to expect

This is a one-person project, so there is no response-time commitment to give you honestly.
You will get an acknowledgement, and you will be credited in the fix unless you ask not to
be.
