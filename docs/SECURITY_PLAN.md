# Closing the known security limitations

`docs/SECURITY_MODEL.md` ends with a "Still open" list. This document is the plan for that
list: what each gap actually is, how bad it is today, what closing it costs, and in what
order. It exists so that the open items are scheduled work rather than a paragraph of
honesty that never turns into a commit.

Every item below is stated the way the model document states things: what an attacker has to
already have in order for the gap to matter. That is the whole basis of the ordering, because
three of these six need the attacker to already be running code on the machine, and two of
them need only that they be on the same network as the operator.

## The six, at a glance

| | Item | Gap needs | Severity | Effort | Order |
|---|---|---|---|---|---|
| **S1** | Domain policy has no UI | nothing; it is a usability gap | Low (security), High (adoption) | S | **1** |
| **S2** | WebSocket token is the standing device token | a log reader | Medium | S–M | **2** |
| **S3** | One outbound policy object for local-only | a future contributor's omission | Medium (regression risk) | M | **3** |
| **S5** | DNS-SD announcement is spoofable | LAN presence + timing | **High** | M–L | **4** |
| **S6a** | No Windows confinement for `run` | Windows + the operator opting in | Medium (fails closed today) | **L** | **5** |
| **S6b** | No Windows `openat` equivalent | Windows + local code execution | Low–Medium | M–L | **6** |

Effort: **S** = under a day, **M** = a few days, **L** = a week or more of focused work.

---

## S1. The domain policy has no Settings surface

**Where it is.** `src-tauri/src/code_proxy.rs` reads `allow` and `deny` from the project's
`.aether/policy.json` (`allowed()`, line 105), records refused hosts in the database
(`pending()`, line 193), and `allow_domain()` (line 140) writes a host into the file. The
only way to reach any of it is `aether1 code net` and `aether1 code net-allow <host>`
(`cli.rs:1370`). There is **no Tauri IPC command and no HTTP route** for the net policy at
all — I checked `commands.rs`, `server.rs` and the frontend; nothing references `code_proxy`
outside the CLI and the proxy itself.

**What the gap is.** Not a hole; the enforcement is in the proxy and is identical whichever
way the list is edited. The gap is that an operator watching a build fail in the HUD has to
leave the app, find a terminal, and run a CLI command to let the build reach `pypi.org`. In
practice that means people will either abandon the sandbox or turn on
`code_run_unconfined` — so the usability gap converts, over time, into a real security
regression chosen by the operator.

**Severity.** Low as a vulnerability. High as a reason the stronger setting gets switched off.

**Approach.**
1. Three IPC commands plus the matching HTTP routes, so the Tauri and `--serve` paths stay in
   sync as `AGENTS.md` requires: `code_net_state` (allowed list + pending refusals),
   `code_net_allow(domain)`, `code_net_deny(domain)`. `allow_domain` already exists; `deny`
   needs the mirror function in `code_proxy.rs`.
2. A Settings → AETHER CODE → Network panel listing the allowed domains with a remove
   control, and the refused list with an **Allow** button per host. The refused list is
   already capped at the last twenty hosts (`code_proxy.rs:186`) so it cannot grow without
   bound behind the UI.
3. The allow-once card in the HUD, which is the part that makes the sandbox liveable: when a
   command is refused a host, a card appears with **Allow once**, **Allow for this project**,
   **Deny**. "Allow for this project" is the existing `allow_domain` write; "Allow once" is
   new state — a per-run allowance the proxy consults alongside `allowed()` and drops when
   the run ends. Copy the shape of the existing LAN pairing gate card, which is the same
   interaction already built and already looks right in both themes.

**Watch out for.** The proxy reads the policy per connection (`code_proxy.rs:400`), so an
allow takes effect on the next request with no restart — that is already true and is what
makes the card feel instant. Do not cache `allowed()` to "optimise" the panel.

**Cost.** Small. Steps 1 and 2 are a day. Step 3 is another day, mostly frontend. ~80–120k
tokens for the whole item.

---

## S2. The WebSocket credential is the device token, not a ticket

**Where it is.** `server.rs` reads `aether1.token.<token>` out of `Sec-WebSocket-Protocol`
(`WS_TOKEN_PREFIX`, `token_from_subprotocol`); `frontend/js/lan-auth.js` sends it via
`wsProtocols()`. This is already the fix for the worse version — `?token=` in the URL — and
`?token=` is no longer read at all.

**What the gap is.** A header is not immune to being logged. Whatever logs it, logs the same
256-bit token that authenticates every other request that device makes, valid until
`aether1 revoke`. The blast radius of one leaked handshake is the whole device.

**Severity.** Medium. It needs someone who can read logs — a reverse proxy in front of the
server, an error page, a shipped log aggregator. That is a real configuration, not a
contrived one, but it is not a network attacker.

**Approach.** A short-lived single-use ticket, exchanged over the already-authenticated HTTPS
path:
1. `POST /api/ws-ticket`, authenticated by the device token through the existing middleware.
   Returns a fresh random 128-bit ticket with a ~30 second expiry, bound to the device id and
   to the socket path being opened.
2. The server keeps tickets in memory only, in a small map with the same fail-closed capacity
   discipline `AttemptLimiter` uses (`serve_auth.rs`) — a full table refuses rather than
   evicting, because evicting is how you lose the record that matters.
3. The subprotocol carries `aether1.ticket.<ticket>` instead of the token. First use consumes
   it; a replayed ticket is refused. The plain `aether1` subprotocol still has to be offered
   and selected — a browser closes a socket whose requested subprotocol the server did not
   select, so that part of the mechanism does not change.
4. Keep the token path working for one release behind a deprecation note, because a paired
   browser holding a cached `lan-auth.js` would otherwise lose every socket with nothing
   readable to say why. The existing unit test that reads the JS file to check the Rust and JS
   constants agree should be extended to cover the ticket names for the same reason.

**Severity after.** A leaked handshake is worthless 30 seconds later and worthless on second
use.

**Cost.** Small-to-medium. The route and the map are half a day; the client change and the
dual-path release are another. ~60–100k tokens.

**Wire test** (the standard this project holds itself to — verify over the wire, not just
compile): `POST /api/pair` with the phrase for a token, `POST /api/ws-ticket` with the token,
then the handshake with `--http1.1` and `Sec-WebSocket-Protocol: aether1.ticket.$T, aether1`
expecting `101` and `sec-websocket-protocol: aether1`; repeat the same ticket and expect a
refusal.

---

## S3. One outbound-policy object instead of a check per subsystem

**Where it is.** `local_only::enabled(db)` is asked by **27 separate call sites** across
`commands.rs`, `voice_download.rs`, `code_perms.rs`, `llm/mod.rs` and `main.rs`.

**What the gap is.** Local-only mode is correct today. It is correct because 27 places each
remembered to ask. The 28th subsystem is the one that forgets, and the failure mode is
silent: traffic leaves a machine whose operator was told it would not. There is no
compile-time or test-time reason that cannot happen.

**Severity.** Medium, and it is the only item on this list whose severity *grows on its own*
as the codebase grows. Everything else is as bad today as it will be next year.

**Approach.** Make the check structural rather than remembered. Two candidate shapes, and I
recommend the first:
1. **One egress function all outbound work goes through.** A single
   `net::outbound(db, host) -> Result<Client, Refusal>` that owns the local-only decision and
   returns the HTTP client. A subsystem that wants the network has to ask for a client, and
   the only way to get one carries the check. Add a CI grep — the project already enforces
   terminal isolation this way with `scripts/check_terminal_isolation.sh`, which is the
   precedent — refusing any new `reqwest::Client::new` outside that module. The CI script is
   what actually closes the gap; the refactor just makes it enforceable.
2. A policy object threaded through the engine. More faithful, much larger diff, and it does
   not stop a new `reqwest` call anyway, so it buys less for more.

**Watch out for.** `is_local_endpoint()` (`local_only.rs:78`) exists because a local model
server is outbound traffic that local-only mode must still allow. The egress function has to
keep that distinction, not flatten it, or local-only mode breaks the local engine — which is
the whole product.

**Cost.** Medium. A mechanical refactor across 27 sites plus the CI script. ~120–160k tokens,
and it is the most parallelisable item here because each call site is independent.

---

## S5. The DNS-SD announcement is spoofable

**Where it is.** `discovery.rs:55` announces `_aether1._tcp` with TXT properties that today
carry only `("version", APP_VERSION)` (`server.rs:267`, `cli.rs:2118`). `discover()` returns
whatever answers. Pairing then exchanges a 12-word BIP-39 phrase at `/api/pair` for a
per-device token (`serve_auth.rs`), with constant-time comparison, per-IP attempt limiting,
and a printed certificate fingerprint.

**What the gap is.** Anyone on the LAN can answer an mDNS browse. So the operator's "scan for
machines" list can contain an attacker's entry with a convincing name, and the operator types
the real phrase into it. The phrase is a bearer credential: once typed into the wrong box it
pairs the attacker with the real machine. The certificate fingerprint is the existing defence
and it is a manual one — it works only if the operator compares it, and the pairing UI
currently invites them to trust the list.

**Severity.** **The highest on this list.** It is the only item that needs nothing but
presence on the same network — no code execution, no log access, no Windows. Coffee shop,
shared flat, office LAN, a compromised IoT device. It is also the only one where the operator
is the one being attacked rather than the machine.

Two things keep it from being critical: the attacker has to be present at the moment the
operator pairs, and `--lan` is explicit opt-in that most installs never turn on.

**Approach.** SPAKE2, which is exactly what a PAKE is for: both sides prove they know the
phrase without either sending it, so a fake that does not already know the phrase learns
nothing from the operator typing it in and cannot complete the exchange.
1. Add a SPAKE2 implementation — `spake2` on crates.io is the obvious candidate; audit it
   before adopting, since this is the one dependency on this list that sits directly on a
   security boundary. If it is not in good shape, the fallback is to bind the phrase to the
   certificate: derive the pairing key as `HKDF(phrase, cert_fingerprint)`, so a fake with a
   different certificate cannot complete pairing even though it can still answer the browse.
   That is strictly weaker than a PAKE but much smaller, and it is a sound stopgap.
2. Replace the `/api/pair` phrase POST with a two-round exchange. Keep the existing
   `AttemptLimiter` in front of it — a PAKE still needs rate limiting, because each failed
   round is one online guess at the phrase, and the limiter's "a request with no credential
   counts as a failure" rule must keep applying.
3. The one-time pairing code path (`code_matches`/`spend_code`, `serve_auth.rs:184`, added for
   the pairing-sequence UX) is already a single-use credential and should get the same
   treatment rather than being left as the easy way round the new one.
4. Show the fingerprint in the pairing UI next to the machine name, not only at startup in the
   terminal, whatever else happens. That is a few hours and it raises the floor immediately.

**Cost.** Medium-to-large, and the only item here with genuine cryptographic risk — a PAKE
implemented loosely is worse than the phrase, because it looks stronger. Budget ~200–280k
tokens for the SPAKE2 route, ~80k for the HKDF-binding stopgap plus the fingerprint-in-UI
change.

**Recommendation.** Do step 4 and the HKDF binding now as their own small PR, then schedule
SPAKE2 properly. This is the one item where shipping something partial is clearly better than
waiting, because the partial version removes the "convincing fake collects the phrase"
scenario outright.

---

## S6a. Windows has no confinement for `run`

**Where it is.** `code_sandbox.rs` detects bubblewrap (`detect()`, line 223, with a real
probe rather than a binary-exists check) and builds the box in `bwrap_args()` (line 477).
Where there is no sandbox, `run` **refuses** (`unconfined_refusal()`, line 361), and the
operator can override with `code_run_unconfined` — which both the CLI and the Settings row
describe in those words.

**What the gap is.** On Windows, `run` does not work. That is the correct behaviour and it is
stated honestly, so there is no misrepresentation to fix. The gap is capability: Windows
operators get either no agent `run` at all, or `code_run_unconfined`, which is a command
running as them with everything their account can reach.

**Severity.** Medium, and it is a product gap more than a vulnerability. It **fails closed**,
which is why it is fifth on this list and not first, despite being the largest piece of work.

**Approach.** A restricted token or an AppContainer with an explicit ACL boundary:
1. Create a capability SID for the project folder, grant it on the folder's ACL, and launch
   with `CreateProcessAsUser` under a restricted token in an AppContainer profile. That gives
   the filesystem half of the box: the project folder and nothing else.
2. The network half already exists platform-independently — the proxy and `HTTPS_PROXY` are
   not a Linux mechanism. What Windows lacks is the *enforcement* that bubblewrap's network
   namespace gives (`can_cut_network()`, line 175); a Windows Filtering Platform rule scoped
   to the AppContainer SID is the equivalent, and it is a second substantial piece of work.
3. `Sandbox::detect()` gains a Windows arm, and crucially `confines()` and
   `can_cut_network()` must answer honestly for whatever subset actually lands. The module's
   existing design already handles partial confinement — on a host that forbids unsharing the
   network it keeps the filesystem box and says "this machine will not let it cut the
   network" — so a Windows arm with filesystem-only confinement fits the existing shape
   without a new concept.

**Cost.** **Large**, and the largest on this list. Win32 security APIs through `windows-rs`,
and the end-to-end test has to run on Windows, which means CI work before the feature work is
even verifiable. ~400k+ tokens, and realistically it wants a Windows machine to iterate on
rather than CI round-trips.

**Recommendation.** Not next, and not without a Windows machine to test on. Split it: the
filesystem AppContainer first as its own PR, reporting `can_cut_network() == false`, then WFP
later. Half the box honestly described is the project's established pattern.

---

## S6b. Windows has no `openat` equivalent

**Where it is.** `code_openat.rs` — the Unix arm (line 80) walks down from the project root's
descriptor with `O_NOFOLLOW` per step; the `cfg(not(unix))` arm (line 249) checks the descent
with `descent()` and then hands the path to `std::fs`, which walks it again. `confines()`
(line 40) is `cfg!(unix)` and both `aether1 code workspace` and the Settings page report it
from there.

**What the gap is.** On Windows, `edit_file` and `create_file` still resolve the path twice,
so the TOCTOU window the Unix arm was written to close stays open: something that can write a
directory along the way can swap a component for a reparse point in the gap.

**Severity.** Low-to-medium. It needs an attacker already executing code on the machine with
write access to a directory inside or above the project — at which point they can usually
edit the project's files directly and do not need the race. The gap is real and narrow, and
on Windows creating directory symlinks has historically needed privilege or developer mode,
which narrows it further.

**Approach.** `NtCreateFile` with a root directory handle and `FILE_FLAG_OPEN_REPARSE_POINT`,
mirroring the Unix descent one component at a time. The module is already structured for
this: `descent()` is shared, `imp` is the platform arm, and the public `read`/`write` do not
change. So this is one new `imp` and nothing else, which is the cheapest structure this could
have had.

**Watch out for.** The Unix arm's hard-won lesson is that `O_NOFOLLOW` plus `O_DIRECTORY`
reports `ENOTDIR` rather than `ELOOP`, so the code asks the filesystem whether a step is a
link instead of inferring it from the errno. Expect an equivalent trap in the NT status codes
and plan to ask rather than infer. The test shape is already set too: the Unix tests stage the
swap by hand and assert on the host disk rather than on the refusal message
(`code_workspace::toctou_tests`), and the Windows tests should do the same.

**Cost.** Medium-to-large, and gated on the same Windows CI that S6a needs. ~150–200k tokens
once a Windows test environment exists. Cheaper than S6a and shares all its setup cost, which
is the argument for doing them adjacently.

---

## Sequencing

**Do first, in this order, as four separate PRs.**

1. **S5 partial** — fingerprint in the pairing UI, phrase bound to the certificate. Highest
   severity on the list, and the partial version removes the main scenario for a few hours'
   work. ~80k tokens.
2. **S1** — the domain policy UI and the allow-once card. Small, visible, and it stops the
   sandbox from being the setting people switch off. ~80–120k tokens.
3. **S2** — the short-lived WebSocket ticket. Small, self-contained, and it closes the last
   item from the original audit's own findings. ~60–100k tokens.
4. **S3** — the single egress function and the CI script that enforces it. The only item whose
   severity grows by itself, so it is worth doing before the codebase grows further.
   ~120–160k tokens.

That is roughly **340–460k tokens** for four closed items, all of them verifiable on Linux
with no new environment.

**Then, as a block.** S5 proper (SPAKE2) and the Windows pair S6a/S6b. These need things the
first four do not: a crypto dependency audit, and a Windows machine with CI. Doing S6a and
S6b adjacently shares the Windows test setup, which is most of the cost of either.

**What can run in parallel.** S1, S2 and S3 touch disjoint code — frontend plus `code_proxy`,
`server.rs` plus `lan-auth.js`, and a sweep of `local_only` call sites. They could be three
concurrent threads. S5 and S3 both touch the LAN path lightly and should not be concurrent.
S3 is the one that conflicts with everything if left late, because it moves call sites that
other work adds to.

**What is not on this list.** The second-pass audit of `tools/`, `vault/`, the LLM
prompt/tool boundary and the Tauri capability set. That is `S4` on the open-work list and it
is not a limitation with a known fix — it is a search for the next class of issue, prompt
injection and tool confusion. It should be scheduled as an audit in its own right rather than
queued behind these six, and it is plausibly more valuable than S6a and S6b, because nobody
has looked there yet.

## Honest statement of what this document is

A plan, read from the code on 2026-10-03 at `42ff79a`. Nothing here has been built or
verified, the token figures are estimates, and the Windows items in particular cannot be
properly estimated without a Windows machine to try the first step on. The severity ordering
is the part worth arguing with: it is deliberately ordered by what an attacker must already
have rather than by how bad the outcome is, which is why the largest piece of work is fifth.
