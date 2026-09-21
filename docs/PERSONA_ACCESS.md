# Per-persona access, and elevation that lasts one request

**Status: built.** Written as a design first, so the security decisions were made deliberately
rather than discovered halfway through — and the writing is what surfaced the escalation path
in "The escalation path this opens", which would otherwise have been found late. What follows
describes what the code now does; the three questions the design left open are answered at the
bottom, with the reasoning.

---

## What exists today

Aether1 already splits tools two ways, in `src-tauri/src/tools/mod.rs`:

- **Read-only tools run automatically.** `read_file`, `list_dir`, `list_processes`,
  `telemetry_detail`, `search_memory`, `read_event_log`.
- **Mutating tools are proposed and wait for the operator.** `write_file`, `set_aether_setting`,
  `run_command`, `append_note`, `write_note`. `run_command` additionally refuses to be
  pre-approved at all (`always_allowable() == false`).

On top of that, `fs_guard` denies a fixed list of paths outright — SSH and GPG keys, AWS and
Azure credentials, `gh` and Docker config, `.netrc`, `/etc/shadow`, DPAPI, credential stores,
`NTUSER.DAT` — regardless of tool, persona or approval.

The gap this design closes: **read-only access is currently unscoped.** Any persona can read
any file that is not on the deny list. The Signal & Logic persona can read your firewall rules;
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
| **System Diagnosis** | `list_processes`, `telemetry_detail`, `read_file`, `list_dir`, `read_event_log` | SystemLogs, ServiceState |
| **Security & White Hat** | `list_processes`, `telemetry_detail`, `read_file`, `list_dir`, `read_event_log` | NetworkConfig, ServiceState |
| **Coding** | `read_file`, `list_dir`, `search_memory` | ProjectTree |
| **Defence & Hardening** | `list_processes`, `telemetry_detail`, `read_file`, `list_dir`, `read_event_log` | NetworkConfig, ServiceState |
| **Assistant & System Ops** | `list_processes`, `telemetry_detail`, `read_file`, `list_dir`, `read_event_log` | SystemLogs, ServiceState |
| **Software Development** | `read_file`, `list_dir`, `search_memory` | ProjectTree |
| **Scanning & Extraction** | `search_memory`, `read_file`, `list_dir` | Vault, ProjectTree |
| **Reference & Fact-Checking** | `search_memory`, `read_file`, `list_dir` | Vault, ProjectTree |
| **Teaching & Docs** | `search_memory`, `read_file`, `list_dir` | Vault, ProjectTree |
| **Worldbuilding & Fiction** | `search_memory`, `read_file`, `list_dir` | Vault |
| **Signal & Logic** | `search_memory`, `read_file`, `list_dir` | Vault |
| **Cost & Budget** | `telemetry_detail`, `search_memory` | Vault |
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
| `llm/persona.rs` | `Domain`, `Root`, `Persona::domain()`, and the table above. `Domain::field()` renders the roots as a phrase, so the sentence an operator reads cannot drift from the access it describes. `catalogue()` carries the field, and Settings shows it. |
| `tools/domain.rs` | **New.** Resolves a `Root` to real directories on this platform, decides whether a path falls inside them, and writes the reason an elevation card shows. Split out of `persona.rs` so the platform path work sits beside `fs_guard`, which it works with. |
| `tools/mod.rs` | `ToolContext` carries the active `Persona`, read from the settings table by `ToolContext::new` and nowhere else. `run` checks the domain **before** the `mutating()` branch, so an out-of-domain read takes the same proposal path a mutating call does. |
| `tools/consent.rs` | `propose` carries the mutating flag and the reason. Pre-approval is refused outright for read-only tools, and an elevation card never offers the "stop asking" checkbox — the gate it would lift is the persona's, not the tool's. |
| `tools/protocol.rs` | The prompt tells the model its field and that anything outside it is a single approved call. |
| `llm/db.rs` | `action_log` gains a `reason` column (additive migration, same shape as `approved_by`), so the log can still answer "why did it read that?" a week later. |
| `tools/mutating.rs` | `persona_type`, `avatar` and `custom_directive` leave `SETTABLE`. |
| `fs_guard` | Unchanged. It is the floor, and this design does not touch the floor. |

One thing changed outside the table. `load_config` defaulted `persona_type` to `"halcy"` while
`get_settings` defaulted it to `"default"` — harmless while a persona only chose a tone, and not
harmless once it chooses access, because the persona writing the reply would not have been the
persona whose field was enforced. Both now say `default`.

### Where the roots actually are

| Root | Linux | Windows |
|---|---|---|
| SystemLogs | `/var/log` | `%SystemRoot%\System32\winevt\Logs`, `%SystemRoot%\Logs` |
| ServiceState | `/etc/systemd`, `/etc/init.d`, `/lib/systemd`, `/usr/lib/systemd`, `/run/systemd` | `%SystemRoot%\System32\winevt\Logs` — services are registry entries, not files, so this points at the log that records them starting and stopping |
| NetworkConfig | `/etc/hosts`, `/etc/resolv.conf`, `/etc/network`, `/etc/netplan`, `/etc/NetworkManager`, `/etc/iptables`, `/proc/net` | `%SystemRoot%\System32\drivers\etc` |
| ProjectTree | the working directory, when it is inside home and is not home itself | same |
| Vault | the configured vault path | same |

`%SystemRoot%` is read from the environment, never assumed to be `C:\Windows`.

**A domain root the path guard refuses is worse than no root at all**, because it reads as
access the persona has and does not — and elevation cannot conjure it either, since `fs_guard`
is checked after the approval too. `every_resolved_root_is_readable_by_the_path_guard` asserts
the two lists agree, and it caught two cases where they did not: `ServiceState` named
`/lib/systemd/system` and `/usr/lib/systemd/system`, which `fs_guard` had never allowed, and
`ProjectTree` resolved to any working directory including ones outside home. The unit
directories were added to the guard; `ProjectTree` now requires the project to sit inside home.

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

## The open questions, answered

1. **`custom_directive` is out of `SETTABLE`.** It was the recommendation, and the cost is
   small: the companion cannot author its own instructions. That is a property you can state
   in one sentence and keep, which is worth more than the convenience it costs.

2. **Elevation does not batch. It is one call, and nothing more.** A persona reading three log
   files outside its field prompts three times. Per-turn batching is friendlier and it is
   genuinely tempting — but "one request" was the rule, and the first bend is the one that
   turns a per-call grant into a short-lived mode. `approving_an_elevation_buys_exactly_one_call`
   is the test that holds this: it approves a call, watches it run, then makes the identical
   call again and asserts it is proposed all over again.

   The friendliness problem is real and is answered somewhere better: by making the minimum
   domain genuinely usable, so the everyday case never prompts at all.

3. **ProjectTree is the working directory, unless that is home.** It resolves to the directory
   Aether1 was started in — but not when that is the operator's home directory, the filesystem
   root, or an ancestor of home. Resolving to `~` would have made the Coding persona's field
   nearly the whole disk, which is the opposite of what a field is for. The cost is that the
   Coding persona asks before reading files until it is started inside a project, and that is
   the right way round: a click, rather than a quiet grant.

## What the operator sees

Settings shows each persona's field under its speciality, in the same words the elevation card
uses ("Reads your notes without asking. Anything else asks you first, once, for that one call.").
Picking a persona is picking a level of access now, and nobody should learn that by watching it
ask — or, worse, by watching it not ask.

The card itself is labelled apart from a mutating approval. **APPROVAL REQUIRED** asks whether
something may change the machine; **OUTSIDE ITS FIELD** asks whether something unusual may look,
once. Giving both the same heading would train the operator to read neither.
