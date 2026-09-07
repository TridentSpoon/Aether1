# Per-persona access, and elevation that lasts one request

**Status: designed, not built.** Written before any code so the security decisions are made
deliberately rather than discovered halfway through. Nothing in this document is implemented.

---

## What exists today

Aether1 already splits tools two ways, in `src-tauri/src/tools/mod.rs`:

- **Read-only tools run automatically.** `read_file`, `list_dir`, `list_processes`,
  `telemetry_detail`, `search_memory`.
- **Mutating tools are proposed and wait for the operator.** `write_file`, `set_aether_setting`,
  `run_command`, `append_note`, `write_note`. `run_command` additionally refuses to be
  pre-approved at all (`always_allowable() == false`).

On top of that, `fs_guard` denies a fixed list of paths outright — SSH and GPG keys, AWS and
Azure credentials, `gh` and Docker config, `.netrc`, `/etc/shadow`, DPAPI, credential stores,
`NTUSER.DAT` — regardless of tool, persona or approval.

The gap this design closes: **read-only access is currently unscoped.** Any persona can read
any file that is not on the deny list. The Creative Work persona can read your firewall rules;
the Conversational one can read your project's source tree. Nothing is wrong with those files
being readable in principle, but nothing connects *what the companion is for* to *what it
reaches for without asking*.

## The model

Two rules.

**1. A persona reads its own field without asking.** Each persona declares a *domain*: a set of
read-only tools, and a set of path roots those tools may reach. Inside the domain, a read-only
call runs exactly as it does today — automatically, logged.

**2. Everything else asks, once, for that one call.** A read-only call outside the domain
becomes a proposal, the same machinery mutating calls already use (`consent::propose`). The
operator approves it, that single call runs, and the grant is gone. The next call — even the
identical one — asks again.

There is no elevated mode, no session flag, no "grant for the next ten minutes". A mode that
stays on is the current behaviour with extra steps, and worse, because it *looks* like a
restriction while not being one.

## What a domain is

```rust
pub struct Domain {
    /// Read-only tools this persona may call without asking.
    tools: &'static [&'static str],
    /// Path roots read_file and list_dir may reach without asking.
    roots: &'static [Root],
}

/// Symbolic rather than literal: the same domain has to mean the right directories on
/// Windows, Linux and macOS, and a hardcoded /var/log is wrong on two of the three.
pub enum Root {
    SystemLogs,     // /var/log, the journal, the Windows event log
    ServiceState,   // systemd units, Windows services
    NetworkConfig,  // /etc/hosts, resolver and firewall configuration
    ProjectTree,    // the working directory the companion was started in
    Vault,          // the operator's notes
}
```

### The proposed mapping

| Persona | Tools | Roots |
|---|---|---|
| **System Diagnosis** | `list_processes`, `telemetry_detail`, `read_file`, `list_dir` | SystemLogs, ServiceState |
| **Security & White Hat** | `list_processes`, `telemetry_detail`, `read_file`, `list_dir` | NetworkConfig, ServiceState |
| **Coding** | `read_file`, `list_dir`, `search_memory` | ProjectTree |
| **Cites Sources** | `search_memory`, `read_file`, `list_dir` | Vault, ProjectTree |
| **Creative Work** | `search_memory`, `read_file`, `list_dir` | Vault |
| **Conversational** | `telemetry_detail`, `search_memory` | Vault |
| **To the Point** | `telemetry_detail`, `search_memory` | Vault |
| **Model's Own** | `telemetry_detail`, `search_memory` | Vault |
| **Custom** | `telemetry_detail`, `search_memory` | Vault |

**Not every persona has a field, and the table should not pretend otherwise.** To the Point,
Model's Own and Custom are styles rather than specialities; Conversational is a manner. Giving
them an invented domain to make the table symmetrical would hand out access nothing asked for.
They get the minimum — the telemetry the HUD already displays, and the operator's own notes —
and reach anything else by asking.

### Deny still beats allow

`fs_guard`'s deny list is checked first and is not overridable by a domain, by an approval, or
by elevation. A domain can only narrow what a persona reaches automatically; it can never widen
what the program will read at all. Security & White Hat having NetworkConfig in its domain does
**not** give it `~/.ssh/id_rsa`, because that is denied outright and stays denied.

## How elevation reads to the operator

The existing proposal path already writes a row and shows a preview. An out-of-domain read adds
one sentence naming why it is being asked:

> **A1ter_nul wants to read `~/projects/aether1/src/main.rs`.**
> That is outside Security & White Hat's field (network configuration and service state).
> Approving runs this one read. It will ask again next time.

The last line matters. An operator who believes they are granting a standing permission will
approve differently from one who knows they are granting a single read.

## The escalation path this opens, and how it closes

Once a persona carries permissions, **switching persona becomes a privilege escalation.**

`set_aether_setting` currently allows the companion to change `persona_type` with one approval.
Under this design that single approval would silently widen every subsequent automatic read —
and it would not look like a permission grant to whoever clicked it. "Change persona to
Security" does not read as "grant standing access to network configuration".

So `persona_type` leaves `SETTABLE`, alongside `tools_enabled`, `tool_always_allow`,
`command_allowlist`, `llm_api_key` and `local_only`. The rule those share: **the companion may
not change the thing that decides what the companion may do.** Persona joins that list the
moment persona decides anything.

`avatar` has to leave too, or be decoupled. Picking an avatar switches persona in the HUD
(`AVATAR_PRESETS` in `app.js`), so leaving `avatar` settable turns it into the same escalation
with an extra hop. Decoupling — the backend ignoring `avatar` for persona purposes — is the
better fix, because an avatar is genuinely cosmetic and it would be a shame to lose the tool's
ability to change it.

`custom_directive` is the open one. It does not widen a domain (Custom gets the minimum), but it
lets the companion rewrite its own instructions, which is adjacent enough to want a decision
rather than a default. **Recommendation: remove it too.** The cost is small and the property —
the companion cannot author its own directive — is easy to state and easy to keep.

## What changes, concretely

| File | Change |
|---|---|
| `llm/persona.rs` | `Domain`, `Root`, `Persona::domain()`, and the table above. Tests that every persona has a domain and that no domain names a tool the registry does not have. |
| `tools/mod.rs` | `ToolContext` carries the active `Persona`. `dispatch` checks the domain **before** the `mutating()` branch, so an out-of-domain read takes the same proposal path a mutating call does. |
| `tools/consent.rs` | The proposal record gains the persona and the reason, so the prompt can say which field this falls outside and the log can answer "why did it read that?" later. |
| `tools/mutating.rs` | `persona_type` and `avatar` (and, recommended, `custom_directive`) leave `SETTABLE`. |
| `fs_guard` | Unchanged. It is the floor, and this design does not touch the floor. |

`Root` resolution is the only genuinely new platform work: SystemLogs and ServiceState mean
different directories on Windows and Linux, and getting that wrong fails in the safe direction
(the read is proposed rather than automatic) but is still wrong.

## Default, and the migration

This makes the app **stricter**, not looser. Every existing install currently reads any
non-denied file automatically; afterwards most reads outside a persona's field will prompt.

That is the right default and it should ship on. But it changes the feel of the product, so:

- The minimum domain has to be genuinely usable. Telemetry and the vault cover the everyday
  conversational case without a single prompt.
- The first out-of-domain prompt should explain the model once, not just the request.
- The setting that turns it off, if one exists at all, does **not** go in `SETTABLE`. A
  restriction the companion can lift about itself is not a restriction.

## What this deliberately does not do

- It does not sandbox the model. A persona can still *ask* for anything; the design governs
  what happens without asking.
- It does not replace approval for mutating tools. Those still propose, exactly as now.
- It does not make personas more capable. Every persona has the same tools available; this
  only decides which ones run without a prompt.
- It does not touch `run_command`, which stays never-pre-approvable with an empty-by-default
  allowlist.

## How it would be tested

- Every persona has a domain; no domain names a tool the registry does not contain.
- An in-domain read runs automatically and is logged as such.
- An out-of-domain read returns a proposal and does **not** execute.
- Approving an out-of-domain read runs it exactly once — a second identical call proposes
  again. This is the test that would catch elevation quietly becoming a mode.
- A path on the `fs_guard` deny list is refused even when it falls inside a persona's domain
  and even when an elevation for it was approved.
- `set_aether_setting` refuses `persona_type` and `avatar`.
- Root resolution returns the right directories per platform, or nothing at all rather than a
  wrong guess.

## Open questions

1. **`custom_directive`:** remove from `SETTABLE` as recommended, or keep? It is not an
   escalation under this design, only adjacent to one.
2. **Does elevation persist for the length of one *turn*?** A persona reading three log files
   to answer one question would prompt three times. Per-turn batching is friendlier and still
   not a mode — but "one request" is the stated rule, and this is the first place it bends.
3. **ProjectTree when there is no project.** The companion is often started from a home
   directory. ProjectTree resolving to `~` would make Coding's domain nearly everything.
