# Closing the known security limitations

`docs/SECURITY_MODEL.md` ends with a "Still open" list. This document is the plan for that
list: what each gap actually is, how bad it is today, what closing it costs, and in what
order. It exists so that the open items are scheduled work rather than a paragraph of
honesty that never turns into a commit.

Every item is stated the way the model document states things: what an attacker has to
already have in order for the gap to matter. That is the whole basis of the ordering, because
the remaining items need the attacker to already be running code on the machine, or to be
running Windows, where the gap fails closed.

**Status, 2026-10-11.** The list was six items on 2026-10-03. Four are closed. The ordering
below was the recommended order and it is the order they landed in.

| | Item | Status |
|---|---|---|
| **S5a** | Pairing credentials bound to the certificate | **Closed** — `f780078` (#215) |
| **S1** | Domain policy UI and the allow-once card | **Closed** — `3dcc8c2`, `53b26ca` |
| **S2** | Short-lived single-use WebSocket ticket | **Closed** — `aaef282` (#213) |
| **S3** | One outbound policy object | **Closed** — `13b8fdb` |
| **S5b** | SPAKE2 for pairing | Open — needs a dependency audit |
| **S6a** | Windows confinement for `run` | Open — needs a Windows machine |
| **S6b** | Windows `openat` equivalent | Open — needs a Windows machine |
| **S4** | Second-pass audit of the prompt/tool boundary | Open — not a limitation; an audit |

## What the four closed items did

Recorded because the reasoning is worth keeping and because each one went further than the
plan asked for.

**S5a, `f780078`.** The plan proposed `HKDF(phrase, cert_fingerprint)` as a stopgap until
SPAKE2. What shipped is that, with the credential's stored hash as input material rather than
the credential itself — the one form both ends already hold — so a leaked `serve_token.hash`
still hands out nothing usable. The pairing box reads the fingerprint before anything is
typed, shows it against the one the server prints at startup, will not pair until the operator
confirms, and re-checks at the moment of pairing so a machine that swaps certificates in
between is refused rather than pinned. The one-time code got the same treatment, which the
plan flagged as necessary or it would have been the easy way round.

**S1, `3dcc8c2`.** The allow-once card, the Settings list, and five commands on both the Tauri
IPC and HTTP transports. Two properties that were not in the plan and should have been:
*nobody watching means no* — the HUD marks itself present each time it asks what is waiting,
and without that mark (the CLI, `--serve` with nobody logged in, the test suite) an unlisted
host is refused at once, as before; and an unanswered card is a refusal after ninety seconds,
worded differently from a policy refusal, because "nobody answered" sends the operator to the
HUD and "the policy says no" sends them to the policy. It also fixed a latent bug the plan
missed: `allow_domain` did not clear a `deny` row, so putting back a starter domain wrote a
row that changed nothing and reported success.

**S2, `aaef282`.** `POST /api/ws-ticket` behind the token wall, 32 bytes from the OS
generator, SHA-256 held in memory for one minute, spent before the upgrade. `/ws/*` takes a
ticket and *only* a ticket — accepting the device token there as well would have left the
weaker credential in the handshake and made the ticket decoration, which is the trap the plan
did not see. A stale tab offering a device token gets its own refusal telling the operator to
reload and is not counted against the attempt limiter, because a stale tab is a client
version, not a guess. The ticket store drops its oldest entry when full, which is safe here
and specifically unlike the attempt limiter, where the oldest record is the live lockout.

**S3, `13b8fdb`.** `src-tauri/src/net.rs` is the only way to build an outbound request, and
`scripts/check_egress_gate.sh` fails the build if a request verb appears anywhere else, if a
second HTTP client lands in the manifest, if no entry point loads the policy, or if the gate
stops consulting the setting. Asking for a request is passing the check, so there is nothing
left to remember — which was the point. The sweep found **two call sites that had no
local-only check at all**: the companion's own `search_web` tool would still have reached
DuckDuckGo with the mode on, and voice downloads were gated only at `start`. The plan counted
27 call sites from a grep; the real number was 25 going through the gate, including those two.

---

## S5b. SPAKE2 for pairing

**Where it is.** `discovery.rs:55` announces `_aether1._tcp` with TXT properties carrying only
`("version", APP_VERSION)`. Pairing exchanges a 12-word BIP-39 phrase or a one-time code at
`/api/pair`, now as an HKDF proof salted with the certificate fingerprint
(`serve_auth.rs:363` for the code path), with constant-time comparison and per-IP attempt
limiting.

**What is left after S5a.** A spoofed announcement can no longer collect a usable credential:
a proof collected by an impostor is a proof for the impostor's own certificate, and the real
machine refuses it. Two things remain.

1. **The browser case.** A browser pairing with a one-time code cannot check a fingerprint by
   eye — there is no startup banner in front of the person using it. The fingerprint binding
   still holds, so a collected proof is useless, but the operator has no way to tell they are
   talking to the right machine before typing.
2. **Comparing a fingerprint by eye at all.** S5a made the comparison possible and mandatory.
   It did not make it unnecessary. An operator who clicks through the confirmation without
   reading it has the old exposure back, and that is the normal way such a control is used.

**Severity.** Low-to-medium now, down from the highest on the original list. The attacker needs
LAN presence *and* an operator who confirms a fingerprint without comparing it. S5a converted
a protocol weakness into a human one, which is a real improvement and not a closure.

**Approach.** SPAKE2: both sides prove they know the credential without either sending it, so
there is nothing to collect and no fingerprint to compare.
1. Audit `spake2` on crates.io before adopting it. This is the one dependency on this list that
   sits directly on a security boundary, and a PAKE implemented or integrated loosely is worse
   than the phrase, because it looks stronger.
2. Replace the HKDF proof at `/api/pair` with a two-round exchange, keeping `AttemptLimiter` in
   front of it — a PAKE still needs rate limiting, because each failed round is one online
   guess, and the limiter's "a request with no credential counts as a failure" rule must keep
   applying.
3. Keep the fingerprint display. It stops being load-bearing but it is still the only thing
   that tells an operator which machine they reached.

**Cost.** ~200–280k tokens. Medium-to-large, and the only item here with genuine cryptographic
risk.

---

## S6a. Windows has no confinement for `run`

**Where it is.** `code_sandbox.rs` detects bubblewrap (`detect()`, line 223, with a real probe
rather than a binary-exists check) and builds the box in `bwrap_args()` (line 477). Where there
is no sandbox, `run` **refuses** (`unconfined_refusal()`, line 361), and the operator can
override with `code_run_unconfined` — which both the CLI and the Settings row describe in those
words.

**What the gap is.** On Windows, `run` does not work. That is the correct behaviour and it is
stated honestly, so there is no misrepresentation to fix. The gap is capability: Windows
operators get either no agent `run` at all, or `code_run_unconfined`, which is a command
running as them with everything their account can reach.

**Severity.** Medium, and a product gap more than a vulnerability. It **fails closed**, which
is why it is not first despite being the largest piece of work.

**Approach.** A restricted token or an AppContainer with an explicit ACL boundary:
1. Create a capability SID for the project folder, grant it on the folder's ACL, and launch
   with `CreateProcessAsUser` under a restricted token in an AppContainer profile. That is the
   filesystem half of the box: the project folder and nothing else.
2. The network half already exists platform-independently — the proxy and `HTTPS_PROXY` are not
   a Linux mechanism, and since `13b8fdb` the egress gate is not either. What Windows lacks is
   the *enforcement* bubblewrap's network namespace gives (`can_cut_network()`, line 175); a
   Windows Filtering Platform rule scoped to the AppContainer SID is the equivalent, and it is a
   second substantial piece of work.
3. `Sandbox::detect()` gains a Windows arm, and `confines()` and `can_cut_network()` must answer
   honestly for whatever subset lands. The module already handles partial confinement — on a
   host that forbids unsharing the network it keeps the filesystem box and says "this machine
   will not let it cut the network" — so a Windows arm with filesystem-only confinement fits the
   existing shape without a new concept.

**Cost.** **Large**, the largest on this list. Win32 security APIs through `windows-rs`, and the
end-to-end test has to run on Windows, which means CI work before the feature work is
verifiable. ~400k+ tokens, and it wants a Windows machine to iterate on rather than CI
round-trips.

**Recommendation.** Split it: the filesystem AppContainer first as its own PR, reporting
`can_cut_network() == false`, then WFP later. Half the box honestly described is this project's
established pattern.

---

## S6b. Windows has no `openat` equivalent

**Where it is.** `code_openat.rs` — the Unix arm walks down from the project root's descriptor
with `O_NOFOLLOW` per step; the `cfg(not(unix))` arm (line 249) checks the descent with
`descent()` and then hands the path to `std::fs`, which walks it again. `confines()` (line 40)
is `cfg!(unix)`, and both `aether1 code workspace` and the Settings page report it from there.

**What the gap is.** On Windows, `edit_file` and `create_file` still resolve the path twice, so
the TOCTOU window the Unix arm closes stays open: something that can write a directory along the
way can swap a component for a reparse point in the gap.

**Severity.** Low-to-medium. It needs an attacker already executing code on the machine with
write access to a directory inside or above the project — at which point they can usually edit
the project's files directly and do not need the race. Creating directory symlinks on Windows
has historically needed privilege or developer mode, which narrows it further.

**Approach.** `NtCreateFile` with a root directory handle and `FILE_FLAG_OPEN_REPARSE_POINT`,
mirroring the Unix descent one component at a time. The module is already structured for it:
`descent()` is shared, `imp` is the platform arm, and the public `read`/`write` do not change.
One new `imp` and nothing else.

**Watch out for.** The Unix arm's hard-won lesson is that `O_NOFOLLOW` with `O_DIRECTORY`
reports `ENOTDIR` rather than `ELOOP`, so the code asks the filesystem whether a step is a link
instead of inferring it from the errno. Expect an equivalent trap in the NT status codes and
plan to ask rather than infer. The test shape is set too: the Unix tests stage the swap by hand
and assert on the host disk rather than on the refusal message (`code_workspace::toctou_tests`).

**Cost.** ~150–200k tokens once a Windows test environment exists. Cheaper than S6a and shares
all its setup cost, which is the argument for doing them adjacently.

---

## Sequencing from here

**The four cheap, Linux-verifiable items are done.** What remains splits by what it needs
rather than by severity, because nothing left is urgent in the way S5 was.

1. **S4, the second-pass audit** of `tools/`, `vault/`, the LLM prompt/tool boundary and the
   Tauri capability set. Recommended next. It is not a limitation with a known fix — it is a
   search for the next class of issue, prompt injection and tool confusion — which is exactly
   why it should not keep waiting behind work that is already understood. Nobody has looked
   there, and `13b8fdb` found two unguarded outbound call sites in `tools/` while doing
   something else, which is weak evidence that looking will find things.
2. **S5b, SPAKE2.** Gated on a dependency audit, not on hardware. Do the audit as a small piece
   of work first and decide from its result; if `spake2` is not in good shape, the honest answer
   may be that S5a is where this stops, stated as such in the model document.
3. **S6a and S6b as a block.** Gated on a Windows machine with CI. Doing them adjacently shares
   the Windows test setup, which is most of the cost of either. S6b first — it is a quarter of
   the work and a self-contained `imp`.

**What can run in parallel.** S4 is reading and touches nothing, so it runs alongside anything.
S5b and the Windows pair touch disjoint code and could be concurrent given the hardware.

## Honest statement of what this document is

A plan. The four closed items are closed — the commits are named and the behaviour is in the
tree. Everything below them is unbuilt, the token figures are estimates, and the Windows items
in particular cannot be properly estimated without a Windows machine to try the first step on.
The severity ordering is the part worth arguing with: it is ordered by what an attacker must
already have rather than by how bad the outcome is, which is why the largest piece of work is
last.
