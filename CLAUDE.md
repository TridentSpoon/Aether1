# Working in this repository

## Use Graft. Always.

Graft is this repository's code graph, and it is the first thing to reach for when a question
is about where something lives, what calls it, or what would break if it changed. Not grep,
not opening files and reading until the answer turns up. This is a standing instruction from
the repository's owner, not a suggestion.

```bash
graft ask --json -n 8 "<what you want to know>"     # add --source for the code itself
graft check                                          # exit 1 when the graph is stale
graft build                                          # first build only; minutes, not seconds
```

`ask` re-indexes whatever changed before it answers, so there is no rebuild schedule to keep
and no staleness to reason about. The progress line goes to **stderr**; stdout is clean JSON.

**Install it if it is missing** — every cloud container starts without it:

```bash
npm install -g @nanonets/graft
```

It is on npm and nowhere else. `cargo install graft` does not exist, bare `graft` on npm is an
unrelated package, and `pip install graft` installs a squatter that will shadow the real
binary. Only `@nanonets/graft` is the tool.

**Where Graft is and is not the right instrument.** The graph and the structural queries are
what it is for: call chains, usages, what a symbol touches. Its ranking for a question phrased
as a whole sentence is only fair — when you already know the symbol you want, grep is faster
and no less correct. Build the graph first either way, because the rest of this file assumes
it is there.

## Before you build or test

The crate needs the Tauri system libraries, which a fresh container does not have. Without
them the failure is `failed to run custom build command for gdk-sys`, which does not name
what is missing:

```bash
sudo apt-get install -y --no-install-recommends build-essential pkg-config libssl-dev \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev \
  libsoup-3.0-dev libjavascriptcoregtk-4.1-dev
```

Everything below runs from `src-tauri/`, except the guard scripts, which run from the root:

```bash
cargo build --bin aether1      # before cargo test -- two tests shell out to the binary
cargo test --bin aether1
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
for s in scripts/check_*.sh; do bash "$s"; done
```

**Build before you test.** Two `code_workspace` tests invoke the built binary and fail with
`error: Unrecognized option: 'net-relay'` when it is stale or absent. That is not a real
failure and it reproduces on a clean checkout.

## Seeing a change actually work

`aether1 --serve` binds loopback on 8378 with no token, so the real app can be driven over
HTTP and with Playwright (Chromium is preinstalled). Point `HOME` at a scratch directory — the
database, the vault and the settings all hang off it, so each run starts clean:

```bash
HOME=/tmp/scratch ./src-tauri/target/debug/aether1 --serve
```

A first run shows `#setup-modal` over the page; dismiss `#btn-close-setup` before any click.

**This matters more than it sounds.** Several faults in this repository were found by driving
the binary after sessions had reasoned from the source and missed them. If a change is
supposed to alter what the operator sees, look at it.
