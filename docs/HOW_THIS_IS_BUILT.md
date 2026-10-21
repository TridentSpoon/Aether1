# How this app is built

The rules this codebase holds itself to, and — for each one — the script that proves it
still holds.

This is not a style guide. Everything here is a property that, if it quietly stopped being
true, would not look wrong in a diff. That is the whole selection criterion: a rule nobody
can accidentally break does not need writing down, and a rule that can only be remembered
will eventually be forgotten by someone reading a diff that looks fine.

**The governing idea: a rule that is not checked is not a rule, it is a hope.** Every
invariant below names the script that enforces it. When you add an invariant, add its
check in the same commit, or you have added a hope.

Run them all before opening a pull request:

```sh
for s in scripts/check_*.sh; do bash "$s" || echo "FAILED: $s"; done
```

They run in CI too, so a pull request that breaks one will say so. The point of running
them first is that the message is written for whoever broke it, and it tells you what to do.

---

## The invariants

| What must stay true | Enforced by |
| --- | --- |
| **Nothing leaves this machine when local-only mode is on.** One way to build an HTTP request; one HTTP client in the tree; every outbound socket enumerated with its reason and its gate; the TTS engine resolved against the mode; the policy actually loaded. | `check_egress_gate.sh` |
| **The companion cannot reach the terminal.** No tool, no route, no record. | `check_terminal_isolation.sh` |
| **The content security policy stays strict.** It is what stands between a stray string and the whole program. | `check_csp.sh` |
| **Every undockable panel's window is covered by a Tauri capability.** | `check_panel_capability.sh` |
| **Every argument passed to a Tauri command is named the way Tauri will look for it.** | `check_invoke_arg_names.sh` |
| **Every field the Model Performance panel reads is one the engine actually sends.** | `check_panel_fields.sh` |
| **Every avatar file is named in the loader, and every name in the loader exists.** | `check_avatar_loader.sh` |
| **Every Tailwind text colour in the HUD is remapped to a theme variable.** | `check_theme_remap.sh` |
| **Tailwind's `hidden` beats component display rules and loses to responsive utilities.** | `check_hidden_utilities.sh` |
| **The vendored three.js bundle contains everything the frontend asks of it.** | `check_vendor_three.sh` |

---

## Reaching the network

This one has the most ways to get it wrong, and has been got wrong twice, so it is worth
stating in full rather than by reference.

**The boundary is the local host and the local network, and no further.** Loopback and the
private ranges are in; a public address is out. That judgement lives in exactly one place,
`local_only::is_local_endpoint` and `local_only::is_local_ip`, and is made on the address
rather than on a provider's name — an "Ollama" endpoint pointing at a rented box is still
traffic leaving the machine.

There are three ways out, and each has its own gate:

1. **An HTTP request.** Built by `net::get` or `net::post`, which ask first. Only
   `src/net.rs` may name ureq's request verbs, and a second HTTP client in the manifest
   fails the build.
2. **A raw socket.** Sometimes the right answer — ureq cannot report which certificate it
   accepted, and a CONNECT proxy is not an HTTP client. Every file that opens one is listed
   in `check_egress_gate.sh` with the reason it exists and what gates it. A new one fails
   the check, which is the review conversation happening at the moment the socket is added
   rather than months later.
3. **A crate that does its own networking.** `msedge-tts` speaks WebSocket to Microsoft and
   sends the text of everything spoken aloud. Its gate is `TtsEngine::resolve(local_only)`,
   and the check requires engine selection and resolution to be one expression.

**Both holes found in this codebase were category 2**, written outside `net.rs` by authors
who had good reasons for the socket and no reason to think about the mode:

- `peers.rs` wrote its pairing requests by hand over rustls, so a machine advertising itself
  over DNS-SD could name a public address and receive every question typed into this one.
- `code_proxy.rs` relayed the sandbox's traffic through a bare `TcpStream`, so a command in
  the box could reach `crates.io` and the rest of the starter list while Settings reported
  that local-only mode was on and nothing reached the internet.

Neither named a ureq verb, so the guard of the day passed both. That is why the guard now
enumerates sockets, and why this section exists.

**If you need a new way out**, the sequence is: gate it first —
`local_only::is_local_ip` for something that must not leave the LAN,
`local_only::enabled` for something that must not leave the machine — then add it to the
list in `check_egress_gate.sh` with a line saying which. Prefer putting the check at the one
chokepoint every call passes through, rather than at each call site; `peers.rs::request` is
the shape to copy.

---

## Overruling, not rewriting

Where a safety switch beats a feature setting, it **overrules** it and leaves it alone.
`local_only` outranks the internet permission (`code_perms::check_url`), the TTS engine
choice (`TtsEngine::resolve`) and the sandbox's network switch
(`code_sandbox::network_for`) — and in each case the operator's own setting still reads back
as what they chose.

This matters for a reason that is easy to miss. If the switch rewrote the setting, turning
local-only mode off later would silently leave the other thing off too, and the operator
would be looking at a switch that no longer means what it says. And any report must ask the
same function the behaviour asks: the sandbox network row calls `network_for`, not
`network_allowed`, which is why it now agrees with what actually happens. **A report that
consults a different source than the behaviour will eventually contradict it**, and the
operator believes the report.

---

## Adding an invariant

1. Write the check first, and watch it fail. A check that has never failed is a check that
   may not work — both of this codebase's network guards were confirmed by temporarily
   removing the thing they protect.
2. Put the reason in the script, at length, in the file where someone who trips it will read
   it. The existing scripts open with the shape of the bug that prompted them; that
   paragraph is the most valuable part of the file.
3. Write the failure message to whoever broke it: what is wrong, why it matters, and the
   specific thing to do next.
4. Name it `scripts/check_<thing>.sh` so the loop above picks it up with no registration
   step.

---

## Where the rest is written down

- `docs/GOALS.md` — what this is for, and what it will not become.
- `docs/SECURITY_MODEL.md` — the full trust model, its limits, and what is still open.
- `docs/AGENT_RUNTIME.md` — the zones AETHER CODE runs in.
- `docs/IMPLEMENTATION.md` — every step shipped, and why it was built the way it was.
- `AGENTS.md` — what to read before changing code.
