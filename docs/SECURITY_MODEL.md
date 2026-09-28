# Aether1's security model

What each boundary in this program actually is, and -- more usefully -- what it is not. The
code states its own invariants next to the code that holds them; this file is for the
boundaries themselves, and for the promises a reader should not read into them.

Written down in this form after a source-level security review of `main` (2026-09-28) found
a real mismatch between one of those promises and the operating system underneath it. The
mismatch and the fix are section 1.

## 1. What AETHER CODE may run, and where

`code_workspace::run` spawns build and test commands for the coding panel. There are three
layers, and only one of them is a boundary.

**Inside the sandbox the allowlist is not consulted, and there is a shell.** That follows
from the paragraph below rather than contradicting it: a list permitting `python3`, `node`,
`make` and `cargo` permits arbitrary code already, so keeping it as a gate while the box
exists would be theatre, and keeping it while refusing a shell would be a distinction with
nothing behind it. It stays for the one case where it is the only thing standing --
`run-unconfined`, where it is enforced exactly as it always was.

**The allowlist is policy, not containment.** `code_run_allowlist` holds program names --
`cargo`, `npm`, `node`, `python3`, `make` and the rest of the starter list. It stops a model
reaching for `curl`, `ssh` or `rm` by name, which is worth having. It is not a sandbox, and
for a while this program's own wording implied it was. Every interpreter on that list is a
general-purpose way to execute code: `python3 -c` will read `~/.ssh/id_ed25519` if it is
asked to, `node -e` will open a socket, `cargo` runs `build.rs`, `make` runs whatever the
Makefile says, and the Makefile is a file the model can write. Spawning without a shell
removes shell metacharacters as an attack; it does nothing about any of that.

**`current_dir` is not containment either.** It says where a process starts, not what it may
touch.

**The boundary is the kernel.** `code_sandbox.rs` puts the command in a namespace:

* the host filesystem mounted read-only;
* `$HOME` replaced with an empty tmpfs, so keys, browser profiles, cloud credentials and
  dotfiles are absent rather than merely unwritten;
* the toolchain caches a build genuinely needs (`~/.cargo`, `~/.npm`, `~/.gradle`, ...) bound
  back over that tmpfs, with the credential files that live inside them masked by
  `/dev/null`;
* the nominated project folder bind-mounted read-write, and it is the only writable place;
* user, IPC, PID, UTS and network namespaces of their own -- the network one always, see
  1b;
* the environment cleared and rebuilt from a short list, so an API key Aether1 holds cannot
  be read by a build script.

This is bubblewrap (`bwrap`), which means Linux with bubblewrap installed -- and *working*,
which is not the same question. `detect()` starts `true` inside the real argument list rather
than trusting that the binary exists, because every failure here is environmental: a kernel
with unprivileged user namespaces disabled, a container, an AppArmor policy. One of those is
worth naming, because it is common and it is partial: unsharing the network makes bubblewrap
bring up a loopback interface, and a host that forbids that (a container, a CI runner) fails
the whole spawn. The filesystem half works perfectly there, so that is what happens -- the
box keeps everything it can hold, drops the network namespace, and the Confinement line says
"this machine will not let it cut the network" instead of implying an isolation that is not
there.

**Where there is no sandbox, `run` refuses.** Windows has no implementation yet -- it wants a
restricted token or an AppContainer with an explicit ACL boundary, and until that is written
the honest answer on Windows is the refusal, not a weaker sandbox described in the same
words as a strong one. The refusal says why, and says what the alternative costs. An operator
can set `code_run_unconfined` and run anyway; with that on, a command runs as them, with
everything their account can reach, and both the CLI and the Settings row say so in those
words.

`aether1 code run-unconfined` and the Confinement line in Settings → AETHER CODE both report
what this machine actually does, from the same function, so the two cannot drift.

**Tested adversarially, not asserted.** `code_workspace.rs`'s tests hand an allowlisted
interpreter a hostile argv and then look at the disk: `python3` reading a planted key out of
the home directory, `python3` writing above the workspace, a `make` recipe writing outside
it, a socket to a public address, and Aether1's own environment. On a machine with no
sandbox those same tests assert the other half of the promise -- that `run` refuses and
explains itself.

Note what the write test asserts, because it is the distinction this whole section turns on:
inside the box the write *succeeds*, against a tmpfs, and the command reports success.
Nothing reaches the operator's disk. Containment, not denial.

## 1b. The way out

The network used to be a switch: off, and `npm install` cannot work; on, and the box could
reach anything, including wherever a fetched page told the model to send a copy of the
source tree. Neither is a mode to work in, so it is now a policy.

The box keeps its network namespace in both states. What changes when the operator turns the
network on is that one unix socket is bind-mounted in -- a file, which crosses a network
namespace, rather than a route, which does not. A relay inside (`aether1 --net-relay`, which
is Aether1 re-entering itself and then running the real command) listens on loopback, and
`code_proxy.rs`, running outside where the real network is, decides one question per
connection: is this host one the project agreed to?

Three properties follow:

* **Bypassing is not possible.** A program that ignores `HTTPS_PROXY` does not reach the
  internet by ignoring it. There is no route; it reaches nothing.
* **No interception and no certificate.** The decision is taken on the `CONNECT` line, which
  names the host in the clear before TLS begins. Aether1 never terminates TLS, never sees
  inside the tunnel, and installs no certificate anywhere.
* **The question is about a domain, once.** `.aether/policy.json` in the project holds
  `allow` and `deny` lists over a starter set of package registries and source hosts; `deny`
  wins. A refused host is recorded and shown by `aether1 code net`, so "the build said it
  could not reach something" and "what did it want" are one question.

What this is not: a content filter. An allowed host is allowed entirely -- which is bounded
by the sandbox, since what the command can read is the project folder and nothing else.

Matching is exact or dot-delimited subdomain, so `crates.io` covers `static.crates.io` and
not `crates.io.evil.example`. Credentials in an authority are stripped before the host is
read, because `http://crates.io@evil.example/` goes to the second one.

The end-to-end test is the claim itself: a real command in a real sandbox tries the host
directly and is blocked, then reaches an allowed host through the proxy, with the domain
added between the two runs.

## 1a. Putting it back

Confinement settles what the agent can reach and not what it can ruin inside the folder it is
*meant* to reach. The old answer there was a table of refused `git` subcommands -- no
`reset`, no `clean` -- which stopped being enforceable when the sandbox got a shell, because
a shell can run git. So the guarantee changed from "it cannot destroy your work" to "whatever
it does, you can put it back", which is both true and stronger.

`code_checkpoint.rs` commits the entire working tree -- tracked, untracked, staged and
unstaged -- to a ref under `refs/aether1/checkpoints/` before the first change of a session,
built through a temporary index so nothing is staged and neither HEAD nor any branch moves.
`aether1 code revert` restores additively: what the checkpoint held comes back, what has
appeared since is left alone and listed. A workspace that is not a git repository gets no
checkpoint and is told so rather than quietly going unprotected.

The tests take a repository with uncommitted and untracked work, do the worst a shell could
do to it -- overwrite, delete, `git reset --hard` -- and assert every byte comes back.

## 1c. How much of the machine one project gets

Everything above describes a box. This describes how big it is for a given project, and it is
one answer rather than a stream of prompts. `.aether/policy.json` in the workspace records a
level:

| | The project folder | Other folders | Network | Commands |
|---|---|---|---|---|
| Assistant | read | read, where named | through the proxy | in the sandbox |
| Developer | read and write | read, where named | through the proxy | in the sandbox |
| Agent | read and write | as named, read or write | through the proxy | in the sandbox |
| Unrestricted | read and write | everything you can reach | anything | as you |

Developer is what a project gets when its file says nothing. The level is not enforcement of
its own: it writes `code_perm_edit`, `code_perm_run`, `code_run_network` and
`code_run_unconfined`, and it decides the mounts the sandbox is built with — the workspace is
`--ro-bind` at Assistant, and a folder named with `aether1 code share` is bound read-only
unless the level is Agent or above and the entry asked for write. Enforcement is the same
bubblewrap invocation as everything else in section 1.

Two limits worth stating plainly. **Unrestricted is not a level of the sandbox, it is the
absence of one** — it sets `code_run_unconfined`, and a command then runs as the operator with
everything their account can reach. And **a shared folder is genuinely shared**: a read-only
mount is read-only against the kernel, but anything the project can read at Agent level it can
also send anywhere the proxy allows.

## 2. What AETHER CODE may change without a sandbox

`edit_file` and `create_file` do not spawn anything, so they are guarded in-process instead:
one nominated folder, paths canonicalised before they are judged (the parent when the file
does not exist yet, so a symlink out of the project resolves to where it really goes and is
refused), `fs_guard`'s denied names and home-directory rule checked as well rather than
instead, and no delete, rename or move at all. Both grants default off.

What used to be the known limit here -- resolve, check, then hand the same string back to the
operating system to walk again -- is closed on Unix. `code_openat` opens a file by walking
**down from the project folder's own descriptor**: each directory below it is opened with
`openat` from the handle above rather than by name from `/`, so swapping or renaming a
component after it was opened cannot change where the next step happens, and every one of
those opens carries `O_NOFOLLOW`, so a link appearing anywhere on the way is an error naming
the component. `create_file` makes its directories the same way, one `mkdirat` per step,
because `create_dir_all` would have followed a link standing in for one of them. Containment
stops being a judgement about a string and becomes a property of how the file was opened;
there is no second walk left to lose the argument.

A link that stays inside the project still works: `resolve` follows it, checks where it lands,
and hands the walk the real file.

Two things worth stating. `O_NOFOLLOW` with `O_DIRECTORY` reports `ENOTDIR` rather than
`ELOOP` on Linux for a link to a directory, so the code asks the filesystem whether the step
is a link instead of inferring it from the errno -- otherwise the one case this exists to catch
would be reported as "not a directory". And **Windows is not covered**: there is no `openat`,
the equivalent is a different piece of work, and there the path is checked and then resolved
again with the old gap intact. `aether1 code workspace` says which of the two this machine
does, rather than leaving an operator to assume the better one.

## 3. The terminal

Nothing a model can call reaches `terminal.rs`. A command that would change something outside
the workspace is *typed* into the operator's terminal without a newline, and their Return key
is the whole consent model. `scripts/check_terminal_isolation.sh` enforces the isolation in
CI.

## 4. `gh`

Whitelisted by `(command, verb)` pair, on the rule that no argument to a listed pair can
change anything -- so `repo clone`, `run download`, `pr checkout`, `browse` and `auth token`
are all out, read-only or not. `gh api` is judged by its method and by the flags that imply a
body, because its verb is a flag rather than a subcommand. Fail closed: a pair this table has
not heard of is refused.

## 5. The LAN server

Loopback by default (`127.0.0.1:8378`); `--lan` is an explicit opt-in that binds `0.0.0.0`,
switches on TLS, and puts the whole router behind authentication. A 12-word BIP-39 pairing
phrase, stored hashed and compared in constant time, is exchanged at `/api/pair` for a
per-device token; devices are revocable individually, failed attempts are rate-limited per
IP, and the certificate fingerprint is printed for the operator to check.

Two consequences worth stating rather than discovering:

* **The phrase is a standing credential.** Anyone who knows it can pair a new device until it
  is rotated, and revoking a device does not revoke the phrase. `aether1 pair` rotates it and
  clears every device.
* **A WebSocket's credential travels as a subprotocol**, not in the URL. A browser's
  `WebSocket` constructor cannot send an `Authorization` header, but its second argument
  becomes `Sec-WebSocket-Protocol`, which is the one handshake header a page can set. A
  client offers `aether1.token.<token>` alongside a plain `aether1`, and the server selects
  the plain one, so the credential travels in one direction and never comes back. `?token=`
  is not read at all: a URL is the part of a request that gets written down, by access logs,
  by reverse proxies and by their error pages, and this token is not a short-lived ticket but
  the credential for every other request that device makes. A header is not immune to being
  logged either, which is why a short-lived socket ticket would still be better; it is not
  what was open, though, and the query string was.

`discovery.rs` announces over DNS-SD, which anyone on the network can impersonate, so a
phrase can be typed into a convincing fake. A PAKE (SPAKE2) is the answer and is not written
yet.

## 6. Leaving the machine

Local-only mode is enforced at the network boundary rather than in the UI: the update check,
the cloud providers, cloud speech and model downloads each refuse. The known weakness is that
each subsystem asks `local_only_enabled()` for itself, so a new one can forget to; a single
outbound-policy object would make that regression impossible rather than merely unlikely.

## 7. Updates

A release bundle is verified before it is trusted: the asset is matched, its `.minisig`
fetched, and the download streamed through a verifier against a public key compiled into the
binary. Verification failure deletes the file. An unsigned asset is never offered on the
release path. A development checkout updates itself by `git pull --ff-only` instead, which is
a different trust decision and a deliberate one.

## Still open

Listed here rather than implied by silence:

* Windows confinement for `run` (section 1).
* The same `openat` discipline on Windows, where `edit_file` and `create_file` still
  resolve a path twice (section 2).
* A short-lived socket ticket rather than the device token itself, and a prominent statement
  of what the pairing phrase is (section 5).
* A Settings surface for the domain policy; today it is `aether1 code net` and the project's
  own `.aether/policy.json` (section 1b).
* One outbound network policy object rather than a check per subsystem (section 6).
* A PAKE for pairing, so a spoofed announcement cannot collect a phrase (section 5).
* A second-pass audit of `tools/`, `vault/`, the LLM prompt/tool boundary and the Tauri
  capability set, where prompt injection and tool confusion are the next class of issue.
