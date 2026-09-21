**The Aether1 Platform**

A basic project for self-hosted or API connected AI companions with a sleek holographic presence, built in the spirit of the the future we saw in Sci-Fi growing up.

Built for Linux and Windows as peer platforms -- developed on Linux, installed on both, and a capability is not finished until it works on each -- with macOS to follow if the hardware ever does. It features real-time system and AI relevant telemetry, an interactive 3D holographic avatar, with speech capabilities, a local persistent memory, and a system tray companion in or hovering next to your notification bar.

Intended features

- **3D Holographic Avatar**: 5 inspired avatars with what will hopefully be familiar personas.
- **Futuristic HUD & Live Telemetry**: Real-time monitoring of Hardware Telemetry.
- **System Notification Bar Integration**:
  - Displays a dynamic glowing status icon in your system tray (Cyan = Active, Green = Listening, Purple = Processing, Amber = Alert/Offline).
  - Quick action menu on click to open the HUD, Trigger instant diagnostics, show the avatar or close out the program.
- **Multi-Provider AI Engine**:
  - **Local Offline AI**: Connect or Install a Local AI with zero cloud dependencies.
  - **Cloud AI APIs**: Support using different popular services via API or Local App installation.
  - **Offline Standby Mode**: While offline the Platform can still use TTS to read out the basic local system stats/information.
- **Neural Voice Synthesis & Speech Recognition**:
  - Neural text-to-speech. With Persona specific voices
  - Speech-to-Text via Web Speech API with auto-send voice chat.
  - Built-in Web Audio API sci-fi synthesizer for UI sound effects.
- **Persistent Long-Term Memory**:
  - Persistent database storing chat sessions, user preferences, and memories across system reboots.
  - Type `remember that [fact]` or `save memory [key]: [value]` to store data permanently and locally.
- **1-Click Packaging & Portability**:
  - Auto-installer for Linux and Windows (`setup.sh` / `setup.bat`).
  - Offline installers for both platforms with speech (Piper + whisper.cpp) already bundled
    -- see "Offline install" below. A slim Linux installer trades that for a much smaller
    download, fetching speech via pip instead -- see "Slim install" below.

## Installing

### Linux

```sh
git clone https://github.com/TridentSpoon/Aether1.git
cd Aether1
./setup.sh
```

`setup.sh` installs the build dependencies for your distribution, builds the app, and puts
it in your launcher and on your `PATH` as `aether1`. It is safe to re-run.

### Windows

Install [Rust](https://rustup.rs) — that is the only prerequisite, since Tauri uses the
WebView2 runtime that ships with Windows. Then, in PowerShell:

```powershell
mkdir "$env:USERPROFILE\Projects" -Force
cd "$env:USERPROFILE\Projects"
git clone https://github.com/TridentSpoon/Aether1.git
cd Aether1
.\setup.bat
```

`C:\Users\<you>\Projects` is where this expects to live. The checkout is not a temporary
build directory: the app runs *from* it — `backend\` holds your database and the shortcuts
point at it — so it needs somewhere permanent, and moving or deleting the folder later
breaks both shortcuts.

Clone it rather than downloading the ZIP. A ZIP arrives without a `.git` directory, so
`git pull` answers *"not a git repository"* and there is no way to update; with a clone,
`git pull` is the update, and setup installs hooks that rebuild the app whenever you do.

The leading `.\` is required in PowerShell, which does not run scripts from the current
directory without it. In `cmd.exe`, plain `setup.bat` works.

Setup builds the app and adds Start Menu and desktop shortcuts (no administrator rights
needed). Launch it from either, or run `.\start.bat`.

Aether1 lives in the **notification area** — click its icon to show or hide the HUD, and
closing the window leaves it running there rather than quitting.

`start.bat --browser` runs the headless server and opens the HUD in a browser tab instead.
That is the development flow, and the fallback if the webview misbehaves; there is no tray
icon on that path, because the tray belongs to the native app.

### Offline install (no internet needed on the target machine)

`setup.sh`/`setup.bat` both need a network the whole way through -- system packages, Rust
itself if it's missing, and this repo. For a machine with none (an air-gapped box, a slow
or metered connection, a fresh install before Wi-Fi is configured), each platform has an
offline bundle instead, built by whoever cuts a release (see `.github/workflows/release.yml`,
which does this for every tagged release) and requiring nothing but itself once built:

- **Linux**: download `aether1-offline-linux-x86_64.tar.gz` from
  [Releases](../../releases), then:
  ```sh
  tar -xzf aether1-offline-linux-x86_64.tar.gz
  cd aether1-offline-linux-x86_64
  ./install-offline.sh
  ```
- **Windows**: download `Aether1-Setup.exe` from [Releases](../../releases) and run it --
  a normal installer, no PowerShell required. It is unsigned, so Windows will have something
  to say about it first -- see "Windows blocked it" below, and read the warning there before
  touching Smart App Control.

Both bundle Piper (TTS) and whisper.cpp (STT) with a voice and a model already inside, so
speech works fully offline immediately, not just once you separately track those down --
see the "Speech" section below for what that buys you either way. `THIRD_PARTY_NOTICES.md`
in each bundle credits what's inside.

This is a different distribution from cloning the repo: it installs a fixed version rather
than a live checkout, so `git pull` isn't how you update it -- download a newer release
instead. If you want to build and modify the code, use `setup.sh`/`setup.bat` above instead.

### Slim install (Linux, small download, needs a network once)

The trade-off in the other direction: `aether1-slim-linux-x86_64.tar.gz` (also on
[Releases](../../releases)) ships just the binary -- no Piper, no whisper.cpp, no models --
and has its install script fetch speech separately into a Python environment of
Aether1's own (`~/.local/share/aether1/pyenv` -- not the system Python, which current
distributions refuse to let `pip` write to at all), plus `python3`, `espeak-ng` and the
GStreamer audio decoders via your distribution's package manager if any are missing. A fraction of the offline bundle's size, at the cost of needing a
network for that one install step (and the first time each engine's model downloads,
which happens automatically the first time you actually speak or listen -- after that it's
cached and works offline like everything else here).

```sh
tar -xzf aether1-slim-linux-x86_64.tar.gz
cd aether1-slim-linux-x86_64
./install-slim-linux.sh
```

### Windows blocked it: SmartScreen and Smart App Control

Aether1's builds are **not code-signed**, so Windows has no publisher to check them against.
Two different things can stop it, and only one of them can be clicked past.

**"Windows protected your PC"** (blue dialog, SmartScreen). Click **More info** → **Run
anyway**. If the installer was downloaded rather than built locally, right-clicking the file
→ **Properties** → **Unblock** before running it avoids the prompt in the first place.

**"Smart App Control blocked an app that may be unsafe"** (grey dialog, only *Okay* and *Get
apps from the Store*). This one has no override. Smart App Control is stricter than
SmartScreen: it runs everything past Microsoft's own reputation service and refuses anything
unsigned, wherever it came from. There is no per-app allow list, and marking the file as
unblocked does not help.

> [!WARNING]
> **Turning Smart App Control off is a one-way door.** Microsoft documents that it cannot be
> switched back on afterwards — the only way back is a clean reinstall of Windows. It is on
> by default on clean installs of Windows 11 22H2 and later, so if you have it, you have it
> for the life of the installation. Do not turn it off casually, and not just to run this.

Check which state you are in under **Windows Security → App & browser control → Smart App
Control settings**. There are three: **On**, **Off**, and **Evaluation** (Windows is still
deciding, and will pick one for you). If it already says Off, none of this applies to you.

Switching install methods does not help: the clone-and-`setup.bat` path produces an unsigned
`aether1.exe` of its own and hits exactly the same wall, and so does `start.bat --browser`,
since the browser fallback still runs that same executable. What is left is:

1. **Run it where Smart App Control is not on.** Any Windows install upgraded from an earlier
   version, or one already Off or in Evaluation, runs Aether1 normally after the SmartScreen
   prompt.
2. **Sign the builds.** The real fix, and the only one that makes Aether1 installable by
   anyone else on a current Windows 11. It needs a code-signing certificate:
   [Azure Trusted Signing](https://learn.microsoft.com/azure/trusted-signing/) is Microsoft's
   own service and the cheapest route for an individual developer (a monthly subscription
   rather than the few hundred a year a traditional OV or EV certificate costs), and being
   Microsoft-issued it earns reputation quickly. A traditional certificate works too, but a
   plain OV one still has to build SmartScreen reputation over time.

A self-signed certificate does **not** work here, even installed into Trusted Root. Smart App
Control judges signatures against Microsoft's own trust and reputation service, not against
your machine's certificate store, so signing it yourself changes nothing.

#### Signing releases

`.github/workflows/release.yml` signs both the binary and the installer when — and only when
— six repository secrets are set. With none of them set it builds exactly as before and warns
in the job log that the release is unsigned, so a fork still builds.

| Secret | What it is |
|---|---|
| `AZURE_SIGNING_TENANT_ID` | Directory (tenant) ID |
| `AZURE_SIGNING_CLIENT_ID` | App registration's client ID |
| `AZURE_SIGNING_CLIENT_SECRET` | That app registration's client secret |
| `AZURE_SIGNING_ENDPOINT` | Regional endpoint, e.g. `https://eus.codesigning.azure.net/` |
| `AZURE_SIGNING_ACCOUNT` | Signing account name |
| `AZURE_SIGNING_CERT_PROFILE` | Certificate profile name |

The app registration needs the **Trusted Signing Certificate Profile Signer** role on the
certificate profile. The last three are configuration rather than secrets, but they live
alongside the other three so there is one place to set signing up and one condition deciding
whether it is on.

Three things the workflow does deliberately:

- **`aether1.exe` is signed before Inno Setup packages it**, not only the installer
  afterwards. Smart App Control judges the binary that ends up running, so an installer
  signed around an unsigned payload installs cleanly and is then blocked on launch — which is
  exactly the failure above, one step later. The packaging script takes `-SignedExe` so the
  signed binary is staged as-is and nothing relinks over the signature.
- **Setting some of the six but not all fails the build.** Anyone who set four of them meant
  to sign, and a release that quietly comes out unsigned is discovered by whoever downloads
  it rather than by CI.
- **Signatures are verified after the fact.** If signing was configured and a file came out
  unsigned anyway, the job fails rather than publishing it. Only presence is asserted, not
  chain validity — a runner that cannot build the chain would otherwise fail for a reason
  unrelated to whether the release is signed.

Signatures are timestamped (`timestamp.acs.microsoft.com`), so they stay valid after the
certificate expires rather than every released installer going bad on the same day.

### Building from source

On Linux, Aether1 links against your system's webview and GTK stack. `setup.sh` installs
these for you on Arch, Fedora, Debian/Ubuntu and openSUSE; on anything else, install the
equivalents by hand and re-run it. Windows needs none of them — only Rust.

**Rust** is required and is not in any distribution list — install it with
[rustup](https://rustup.rs):

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

**System libraries**, by distribution:

| Distribution | Packages |
| --- | --- |
| Arch, CachyOS, Manjaro, EndeavourOS | `base-devel curl wget file openssl webkit2gtk-4.1 gtk3 libappindicator-gtk3 librsvg xdotool libnotify espeak-ng gst-plugins-good gst-plugins-bad gst-libav` |
| Fedora, Nobara, RHEL | `webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel librsvg2-devel openssl-devel curl wget file xdotool libnotify espeak-ng gstreamer1-plugins-good gstreamer1-plugins-bad-free` plus the `c-development` group |
| Debian, Ubuntu, Pop!\_OS, Mint | `build-essential pkg-config curl wget file libssl-dev libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libnotify-bin espeak-ng gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-libav` |
| openSUSE | `webkit2gtk3-soup2-devel gtk3-devel libappindicator3-devel librsvg-devel libopenssl-devel curl wget file xdotool libnotify-tools espeak-ng gstreamer-plugins-good gstreamer-plugins-bad` |

If the build fails, the error names the missing piece: look for a package ending in `-dev`
or `-devel`, install it, and re-run `./setup.sh`.

The GStreamer entries are not build dependencies and they are the ones worth not skipping.
Aether1's window is WebKitGTK, and WebKitGTK plays `<audio>` through GStreamer -- but every
distribution above packages the plugins that do the *decoding* as optional for it. On Arch,
`gst-plugins-good` and `gst-libav` are optdepends of `webkit2gtk-4.1`, so installing the
webview does not install them. Without them, Piper synthesizes correctly, every status
screen in the app says "installed", the voice test reports that it spoke, and you hear
nothing at all. The voice panel now checks for this directly and names the packages, but it
is cheaper to just have them.

### Speech: what's local, what isn't, and what always works

Speaking (TTS) has three tiers, tried in that order by the default **Auto** engine so there
is always something to speak with, on a machine that has done nothing but run setup:

1. **[Piper](https://github.com/OHF-Voice/piper1-gpl)** -- the best-sounding local voice,
   if you install its binary (the offline installer above does this for you; otherwise
   your package manager, or the Python route under "The two Piper traps" below). The voice
   file itself Aether1 can
   fetch for you -- see below. **Two traps here, and between them they account for
   most "I installed it and it still doesn't talk" reports** -- see below.
2. **Cloud** (Microsoft) -- better than the OS voice, but the text of everything the AI
   says leaves your machine, and it needs a network.
3. **This OS's own voice** -- SAPI on Windows (ships with every edition, nothing to
   install) or `espeak-ng` on Linux (installed by `./setup.sh`). Lower audio quality than
   the other two, but it cannot be "not installed" the way Piper can or offline the way the
   cloud engine is, which is what makes it the guaranteed fallback rather than an optional
   extra.

#### The two Piper traps

**`piper` is the name of two unrelated programs.** The one you want is
[Piper TTS](https://github.com/OHF-Voice/piper1-gpl), which turns text into speech. The one you
will find first is [Piper](https://github.com/libratbag/piper), a GTK app for configuring
gaming mice — and on Arch and CachyOS that is exactly what `sudo pacman -S piper` installs,
because the mouse app is the one in the official repositories. Piper TTS is in the AUR:
`yay -S piper-tts-bin`. On Debian and Ubuntu, `sudo apt install piper-tts`. On any
distribution, the Python package works without anything your distribution has to carry --
but not via `pip install --user`, which Arch, Debian 12+, Ubuntu 23.04+ and Fedora all
refuse now with `error: externally-managed-environment` (PEP 668). Put it in Aether1's own
environment instead, which it knows to look in:
`python3 -m venv ~/.local/share/aether1/pyenv && ~/.local/share/aether1/pyenv/bin/pip
install piper-tts`. (The old `linux_x86_64.tar.gz` release still works too, but it
is frozen: the original `rhasspy/piper` repository was archived in October 2025 and
development moved to `OHF-Voice/piper1-gpl`, which ships no pre-built binaries.)

Aether1 no longer takes the name at its word. It asks each candidate binary what its
options are and only accepts one that knows about `--model`, so a machine with the mouse app
and no Piper TTS now correctly reports the good voice as missing instead of ticking the box
and going silent.

**A voice is two files, not one.** On the
[voices page](https://huggingface.co/rhasspy/piper-voices/tree/main/en), every voice is a
large `.onnx` *and* a small `.onnx.json` beside it, with separate download buttons. Piper
cannot load the model without the `.json`, and downloading only the big obvious one is the
natural mistake. Aether1 now checks for the sidecar — and for a plausible file size, which
catches a download that stopped halfway — before it reports a voice as installed.

`en_GB-alba-medium` and `en_US-amy-medium` are good, ordinary-sounding places to start.

**And you no longer have to fetch them yourself.** *Set up the voice* now has a short list
of voices with a Download button beside each. Aether1 downloads both of the files a voice
is made of, into the folder it already looks in, and shows a progress bar while it does —
so the "two files, not one" trap above is one you can only fall into if you go and do it by
hand anyway.

It fetches voices and never programs. A voice file is sound turned into numbers, handed to
an engine you installed yourself; nothing downloaded here is executed, marked executable or
put on your PATH, and Piper itself still comes from your package manager. The list is a
fixed table built into Aether1, so nothing you or the page can type ever becomes part of a
web address or a folder name. Bytes land in a `.part` file that is only renamed once the
whole voice has arrived and been checked, so an interrupted download leaves nothing behind
that looks usable. And in **local-only mode it does not run at all** — the buttons go dead
with the reason printed beside them, rather than quietly reaching the internet.

Listening (STT) only has the first two of those -- there is no universal OS-level
equivalent to fall back to yet:

- **[whisper.cpp](https://github.com/ggml-org/whisper.cpp)** for local listening, plus a
  `.bin` model in `~/.local/share/whisper` (also handled by the offline installer).
- Cloud, otherwise.

Settings → Speech Engine reports which of these are actually available on this machine and
what's missing for the rest, and lets you pin a specific tier instead of Auto (e.g. "Local
only" to guarantee nothing ever leaves the machine, refusing to speak rather than silently
falling back to the cloud).

**🗣 Set up the voice** (in the HUD menu, and at the top of Settings → Voice & Sound) is
the guided version of all of that: it checks speaking and listening separately, gives the
install steps for this operating system, and its **Say something** button makes a real
attempt out loud and then lists every engine it tried and why each one did or didn't work.
A voice that fails during a reply now says so in the chat rather than going quiet.

## Running with no internet at all

Most of Aether1 already works with the cable out: it finds model servers by probing your
own machine, remembers things in a folder of files, listens through whisper.cpp, and every
font, stylesheet and script the HUD needs is stored in this repository rather than fetched
from anywhere.

Two things still reached out on their own, and **Settings → Network → Local only** stops
them. With it ticked:

- **Speech never falls back to the cloud.** Without it, a missing Piper means the text of
  everything the AI says is sent to Microsoft to be spoken. With it, Piper speaks or
  nothing does, and Settings says which.
- **No update check.** Aether1 asks GitHub for the latest commit on every launch. That
  stops, and so does installing an update.
- **Cloud providers are refused.** Gemini, Groq, OpenAI and Anthropic are not contacted;
  you get a local answer and a line saying who was not called. A cloud key sitting in your
  environment no longer switches an offline install to a cloud one behind your back.
- **No model downloads**, since pulling a model is a download.

What it does **not** stop is your own network. A model server on another machine in your
house is still fine — that is the whole idea. The line is drawn at the internet: this
machine and your LAN, and no further.

Nothing is deleted by turning it on. Untick it and your cloud settings are exactly as you
left them. To nail it on for good — a shared machine, a locked-down install — set
`AETHER1_LOCAL_ONLY=1` in the environment and the checkbox can no longer switch it off.

## Giving it a brain

A fresh install has no AI behind it: it talks, reads out your system stats and answers
with a handful of canned lines, because `llm_provider` starts at `offline`. Nothing is
broken — there is simply no model yet.

Press **🧠 Set up the AI** in the HUD menu. It looks at the machine, works out
which of four things is true (nothing installed / installed but not running / running
with no models / a model is there), and asks for exactly one thing at a time. It sizes
the model list to the memory this computer actually has, marks one **Best for this
computer**, downloads it, and fills the settings in for you.

The list is the nineteen most-used local models — Llama, Gemma, Qwen, Mistral, Phi and
DeepSeek R1 — across five memory tiers, from a 400 MB one that runs on almost anything
to a 43 GB one for a machine with 64 GB of memory. What this computer can run is listed
straight away; the rest is one click away behind **Show N bigger models**, because the
recommendation is a default and not a gate.

Downloads show a real progress bar — the percentage and the byte count the model server
itself reports, not a spinner — and up to three can run at once. If the model server is
installed but not running, **▶ Start it for me** starts it; that is the one gap Aether1
can close by itself rather than describe.

Until a brain is connected the wizard opens on launch, the dialogue stream carries a
notice you cannot miss, and sending a message says plainly that nothing is behind it
rather than returning a canned line that looks like an answer.

The same journey written out, per operating system, is in
[docs/GETTING_STARTED.md](docs/GETTING_STARTED.md) — written for someone who has never
installed a developer tool in their life.

## Choosing a model

Once there is a brain, the rest of this is for changing it. Open **Settings** and
Aether1 looks for model servers already running on this machine. Any
that answer appear under **AI SERVERS FOUND ON THIS COMPUTER**, inside the
**🧠 The Brain** group; picking one fills in the provider, the address and the list
of models it can run, so there is nothing to look up.

The **Agent & System** tab is grouped by the question you came in with rather than by
which module implements it: **🧠 The Brain**, **🗣 Voice & Sound**,
**📓 Memory**, **🛡 What it may do**, **🌐 Network** and
**⚙ The app itself**. Each group is collapsed until you open it, and the things
almost nobody needs — the Piper voice file, the Whisper model file — are nested one level
further inside the group they belong to.

The scan probes the loopback ports these tools tend to use and identifies them by the API
they speak, not by which program they are — so it finds the popular runners, most of the
less popular ones, and anything else that has adopted a common port. A server on an
unusual port isn't lost: type its address into the endpoint box and pick the matching API
shape (**OpenAI-compatible** for most things, **native** for the `/api/tags` style).

A server on another machine on your network works the same way: type its address in. That
stays true with **Local only** on — it is the internet that is closed off, not your LAN.

Cloud providers — Gemini, Groq, OpenAI, Anthropic — need a key in the **API KEY** box and a
model name typed in. If a call fails, the HUD says why: a rejected key, a model name that
doesn't exist, a proxy in the way and an unreachable server each say so in those words.

## The top bar and the chin bar

The bar along the top carries what is on screen on the left, what this is in the middle, and
the controls on the right:

- **Avatar** shows the one that is selected. Click it and the other seven slide out, along
  with the way into the workbench when *Your own* is the one you are using.
- **Theme** sits under it and works the same way: the current mode, and Solar, Eclipse and
  Cyberpunk sliding out when you click it.
- **The wordmark** sits in the centre. It shows your companion's own name once you have given
  it one, turning over to AETHER1 PLATFORM every so often so what it is running on stays
  visible. The version sits directly under it in the desktop app.
- **The menu** (☰) holds SFX, Activity, Clear conversation and Settings. They live behind one
  control so they are reachable at any window width rather than being the first thing pushed
  off the edge of a narrow one.

**The chin bar** is the slim strip along the bottom. It shows what the companion is doing --
IDLE, LISTENING, THINKING, SPEAKING -- and is always there, so the state never has to compete
for room with anything else.

## The model performance panel

A cloud model and a local one raise different questions, so the panel has two views and
alternates between them while nothing is happening. Once something is generating, it pins to
whichever view describes what is doing the work. Clicking a tab holds it there.

**SPEED** — how fast each model runs **on this machine**. One row per model, fastest first,
with the bar scaled against the quickest one on the board rather than a fixed ceiling, so the
comparison reads the same on a laptop running a 3B model and a desktop running a 70B one.

The rows build themselves out of ordinary use — nothing extra is run to fill them — and the
average is **weighted by tokens, not by reply**: a four-token "Yes." is mostly measurement
noise, and counting it for as much as a five-hundred-token answer would make the board a
ranking of measurement error. RESET READINGS throws the board away, which is what you want
after a new graphics card or a different quantisation of the same model.

A model only appears once it has been **timed by the server that ran it**. Ollama reports
`eval_duration`, the time it actually spent generating; wall clock includes reading the model
off disk and queueing behind another request, which makes a fast model look half as fast on
its first reply and quicker on every one after — a cold start that reads like a fault. A
server that reports no generation time simply never appears on the board, and the panel says
so. An empty row is a true statement about what can be measured; an invented one is not.

The big number at the top reads off that same board — the current model's measured average —
and shows `--` when the current model has no row. It deliberately does **not** show the speed
of the last reply, because that figure falls back to a wall clock whenever the provider
reports no generation time of its own, and a wall clock will happily time a canned local
answer that no model was involved in. A real number about the wrong thing, under a `tok/s`
label, is the habit this panel was rebuilt to break.

**USAGE** — what this session actually used, and what that cost.

The counts are **what the provider reported**, not a guess: Ollama's `prompt_eval_count` and
`eval_count`, the OpenAI-compatible `usage` object, Gemini's `usageMetadata`, and Anthropic's
split across `message_start` and `message_delta`. Where a provider says nothing, the
four-characters-per-token estimate still fills the gap — but the panel says `estimated`
rather than `counted`, because "1,204 tokens" and "about 1,200 tokens" are different claims
and only one of them can be checked against a provider's own dashboard.

**Cost is shown only where cost exists.** A local model's tokens are free, and the panel says
that rather than printing $0.00 — which would look identical to a cloud model nobody has
priced. For a cloud model the figure is real token counts multiplied by the model's published
price, shown in cents below a penny so a genuinely small cost does not round away to "free".
Prices are stamped with when they were last checked, because a six-month-old price is a guess
and you are entitled to know that before trusting it.

A model with no known price reads `unpriced`, not $0.00. To price it, drop a
`model_prices.json` next to the database (`backend/aether1_memory.db`):

```json
{ "claude-opus-5": { "input_per_million": 15.0, "output_per_million": 75.0 } }
```

Names match by prefix, so `gpt-4o-mini` covers `gpt-4o-mini-2024-07-18` too. The file is
re-read every second, so a correction shows up without restarting. A typo falls back to the
built-in prices with a line on the console rather than costing everything at zero.

Nothing here is invented. There is **no session budget** — an earlier version of this panel
divided against a 100,000-token figure hardcoded in the source and showed the result as
"Used: 1.3%" and "Budget left: 98.7k", which was a made-up number being displayed back as a
measurement. A model that reports no context length shows `--` rather than a plausible
default.

## Rearranging the HUD

The main window is a free-form grid, a bit like the home screen on a phone: every panel
has a place on it and a size, and both are yours to change.

- **Move a panel:** drag it by the dotted strip along its top edge. Drop it anywhere
  there is room.
- **Resize a panel:** drag the little handle in its bottom-right corner. It grows until
  it would run into a neighbour, then stops rather than shoving anything aside.
- **Keyboard:** tab to a panel's grip and use the arrow keys to move it.
- **Switch a panel off entirely:** Settings → **Panels to show**, in the HUD LAYOUT
  section. Untick one and it is gone — not hidden behind something, *not running*. The
  avatar in particular genuinely stops drawing when its panel is off, rather than carrying
  on burning the graphics card behind a box you cannot see.

**Resizing the window resizes the panels.** Sideways it always has: a panel that takes a
third of the width takes a third of the width at any size. Up and down it now does too --
the shipped arrangement is scaled to fit the height of the window, so on a laptop screen you
see the whole avatar instead of scrolling down to find the bottom of it. There is a floor:
on a very short window the panels stop shrinking, because a panel squeezed past the point of
being readable is worse than a scrollbar. Narrower than about 1000 pixels the grid gives up
on arranging things altogether and stacks everything into one readable column.

All of this is remembered on this machine and survives a restart, and none of it is saved
with your settings — a layout is a property of the screen you are sitting at, so a second
machine pointed at the same companion can be arranged completely differently. Settings has a
**Reset panels to their default places** button when you want the original layout back,
which also switches every panel back on.

## The avatar on a spare screen

If you have a second monitor, a telly on the wall, or the little screen on a laptop dock
doing nothing, you can put the avatar on it and nothing else:

```sh
aether1 face
```

There is also a **⛶** button in the top corner of the avatar panel, and a **🙂 Fullscreen
Face** item in the tray menu. All three do the same thing.

What you get is one screen with the avatar on it, filling the height, and a single word
underneath saying what it is doing — IDLE, LISTENING, THINKING or SPEAKING. No panels, no
chat, no buttons, no mouse pointer. It picks a monitor you are *not* working on when there
is one, so it does not land on top of whatever you were reading.

**It is a mirror, not a second companion.** It shows whatever the main window's avatar is
doing, in the same shape and the same colours, and changes the moment you change them. It
has no conversation of its own and nothing to type into.

**It never listens.** The small desktop avatar can be clicked to start the microphone,
because it sits on the desktop in front of you. This one deliberately cannot: it is on a
screen across the room, and a screen that starts recording because the cat walked past the
mouse is not something worth having.

**Press Esc to close it** — the reminder in the corner says so, and fades out once you have
read it. Moving the mouse brings it (and the pointer) back. The tray item and the **⛶**
button close it again too.

It is not remembered between restarts. Asking for it is asking for it now, not forever.

## Themes

A theme is two choices, and they are independent.

**The mode** decides the shape of everything -- pick it from the dropdown in the top bar or
the THEME row in Settings:

- **Solar** and **Eclipse** are flat window shells modelled on Windows 11: plain surfaces,
  hairline borders, one accent colour, no glow. Solar is the light one, Eclipse the dark one.
- **Cyberpunk** is the HUD the app started as -- scanlines, corner brackets, a grid backdrop
  and real neon.

**The colours** are three, and they work the same way in all three modes:

- **Background** -- the ground everything sits on. It is deliberately stable: changing an
  accent never moves it, so you can try colours out without the page jumping around.
- **Main** -- headings, borders, gauges, and the avatar itself.
- **Highlight** -- the companion, so the main colour is not the only one on screen.

Settings has a picker for each, plus eight presets. The six neon ones -- Cyan, Green, Amber,
Magenta, Crimson and Night -- are the themes Aether1 used to ship as fixed choices; Daylight
and Midnight are Solar's and Eclipse's own. A preset is nothing more than a named set of those
three colours and a mode, so anything a preset does you can do by hand -- including putting a
Cyberpunk accent on Solar, or a colour of your own on any of them.

**Two sliders adjust the tone** of whatever you have picked, without changing the picks
themselves:

- **Saturation** (0--100%) is how much colour the accents carry. Drag it down and the neon
  calms without anything getting darker or lighter -- each colour is mixed toward the grey of
  its own brightness, so only the chroma goes. At 0 the whole thing is greyscale.
- **Depth** (-40 to +40) is how dark the ground sits. Left is deeper, right is lighter, and it
  carries the panels, borders and the avatar bay with it rather than leaving them floating at
  the old lightness.

They are separate on purpose, because "the colours are too bright" and "Eclipse is not dark
enough" are two different complaints. Draining the colour out of a background moves it toward
grey at the same brightness, not toward black, so one slider could not have answered both.

Depth stops at 40 rather than 100 because light-or-dark is *measured* from the background
rather than declared by the mode. Dragged far enough, a background crosses the line and the
whole shell inverts -- light text on what is still nominally the light theme. At 40 the darkest
Solar is still light and the lightest Eclipse is still dark, so the slider cannot flip the
window out from under you.

Each mode remembers its own tone, the colour swatches keep showing the colours **as picked**
rather than as painted -- which is what makes the sliders non-destructive -- and *Reset this
mode's colours and tone* puts everything back.

Each mode remembers its own colours, so switching to Solar to read something in daylight and
back to Cyberpunk afterwards does not cost you the accent you had picked. **Reset this mode's
colours** puts one mode back to its default without touching the others.

**The first run follows your operating system**: Solar on a machine set to light, Eclipse on
one set to dark, and it keeps following as you flip that setting. Picking a mode is also how
you opt out -- from then on it stays where you put it, across restarts.

Any theme can be worn by any avatar; the two are independent choices.

The avatar itself always sits on a dark stage -- black under Cyberpunk, a dark grey under
Solar and Eclipse -- because a hologram projected onto a white page reads as a picture of one
rather than a projection. That panel is the one part of the window that does not follow the
background colour.


## What it remembers

Everything the companion keeps about you is a folder of ordinary markdown files, by default
`~/Aether1Vault`. Not a database — a folder. You can open it in any editor, search it, put it
in git, sync it with Obsidian, hand it to a different assistant, or delete a line you disagree
with, and none of that needs a feature from us.

```
Aether1Vault/
  INDEX.md        what is here, and which notes matter for which question
  profile.md      who you are and how you like things done
  machine.md      what this computer is
  memories.md     things you asked it to remember
  projects/       one note per project
  daily/          what happened on a given day
  archive/        notes that stopped being true, kept rather than deleted
```

Notes link to each other with `[[wiki links]]`, which is what turns the folder into a graph —
open it in Obsidian and the graph view *is* a picture of what your companion knows.

**You can read them inside Aether1 too.** Menu → **📓 Notes**, or the **Read Them Here**
button beside the folder path in Settings. You get the list of notes, the note itself with
its headings and formatting laid out properly, every `[[link]]` clickable, and — the part a
plain editor hides — a *Linked from* line at the bottom showing which other notes point at
the one you are reading. The search box runs the same search the companion itself uses, so
what you find is what it would have found.

**And you can see the shape of them.** The **🕸 Graph** button at the top of the Notes
window draws the same notes as a picture: a dot for each note, a line for each `[[link]]`,
and the note's name underneath. Bigger dots are the notes more things point at, notes in
the same folder share a colour, and the three ringed dots are the ones it reads before
every single answer. Drag to move around, scroll to zoom, hover a note to light up what it
is joined to, and click one to read it.

It is worth a look every so often for the two things a list cannot show you: which notes
have quietly become the hubs everything hangs off, and which ones are floating on their own
with nothing pointing at them any more.

It reads and never writes. Notes are changed by asking the companion to change them, which
goes through the same approval and undo as anything else it does to your files; a reader
with a Save button would be a second way into the same folder with neither of those things
attached. To edit a note yourself, open the folder — it is your folder, and it is just
markdown.

### More than one conversation

**💬 Conversations** in the ☰ menu is the list of everything you have talked about. Click one
to pick up exactly where you left off, or **＋ New** to start a fresh one. The ✎ gives a
conversation a name — until you do, it is named after the first thing you said in it — and
the 🗑 deletes one along with everything said in it.

Conversations do not leak into each other. Your companion is given the history of the one it
is answering in and nothing else, so a new conversation genuinely starts clean. It still has
your notes — those are the part that is meant to carry across — but not what was said next
door. That is useful when you want to change the subject without dragging an hour of
something else along, and it matters when you would rather one conversation simply never came
up in another.

It is yours to move around in, not the companion's. It cannot start a conversation, switch to
one, rename one or delete one, in the same way it cannot switch its own tools on. The app
remembers which conversation you were in, so closing the window and coming back puts you back
where you were rather than somewhere new.

**How it finds things.** `INDEX.md`, `profile.md` and `machine.md` are loaded into every
conversation; everything else it goes and reads when the question calls for it. Once the vault
outgrows its index it searches instead, ranking by *where* a word appears rather than by how
recent a note is: a note named for the topic comes first, then one with the topic in a heading,
then one that just mentions it in passing. That ordering is the whole point. Asking about
sourdough should find your sourdough note, not last Tuesday's conversation.

**How it forgets.** Nothing is deleted. A note that has stopped being true, or whose contents
have been folded into a better note, gets moved to `archive/` — it keeps its text, its links
and its searchability, and the index line says `(archived)` rather than vanishing. Once the
`daily/` folder passes a fortnight's worth of notes, the companion is told to *offer* to tidy
them into topic notes at a natural pause. It will not interrupt you to do it and it will not do
it without asking, because a fact worth keeping belongs in the note about its subject, and
deciding which facts those are is a judgement you should get a say in.

**How you can tell.** Under an answer that used the vault, the HUD lists the notes behind it:
● a note that is loaded every turn, ◆ one it went and fetched while answering, ○ one
search offered it as a candidate and which it may not have used at all. Those three are
deliberately not the same claim. A companion that quotes something about you should be able to
say where it got it, because otherwise recall and invention look identical from the outside.
Settings has an **Open Folder** button next to the vault path, so the notes it names are one
click from being open in your own editor. (In a browser tab it copies the path instead --
a tab on your phone cannot open a folder on your desktop, and pretending otherwise would be
worse than saying so.)

**Your conversations are in there too, word for word.** Every exchange is appended to a note
named after today's date inside `daily/`, as it happens, with no approval card, and nothing
is shortened on the way in. A day talkative enough to outgrow one note carries on in
`2026-09-15-2.md` rather than being cut — partly so each note stays readable, and partly
because a note past half a megabyte is one the companion's own search will not open, and the
one day it cannot search should not be the day you said the most. That is deliberate: this
only ever writes into the folder that exists to hold your memory, and asking you to approve
your own conversation being remembered would be a question with one answer. There is a
checkbox for it under the vault path in Settings — **Keep a dated note of each conversation**
— and switching it off means conversations stay inside the app and nowhere else.

Writing *facts about you* to the vault is different, and still shows you an approval card
first — what it wants to record, and where. The one exception is `remember that …`, which
writes straight through: that is your own instruction, and asking you to approve your own
sentence would be ceremony rather than consent.

## Personas

A persona is a **job**, not a character. The character is how the job sounds. Settings leads
with what each one is for, and names the avatar it belongs to:

| Persona | What it is for | Avatar |
|---|---|---|
| **System Diagnosis** | Logs, services and what this machine is doing. Starts from evidence, quotes the line that shows the problem, separates what it observed from what it inferred. | A1 |
| **Conversational** | Thinking a problem through with you. Asks the one clarifying question that would change the answer instead of guessing. | hAlcy |
| **To the Point** | The answer in the first line. No preamble, no restating the question, no padding. | R.E.D. 9000 |
| **Coding** | Working code, complete enough to run, with the failure mode named -- what breaks it, what it does not handle, what it costs. | The Nexus |
| **Cites Sources** | Where every claim came from, and how sure it is: read from a file this session, recalled from training and unverifiable, or inferred. Never invents a citation. | A.R.X.LIMES |
| **Creative Work** | Writing, design, and the shape of a sentence. Produces the draft rather than describing it. | A.R.X.LOGOS |
| **Security & White Hat** | Exposure, hardening and authorised testing -- attack surface, blast radius, and asking whether a target is yours to test. | A1ter_nul |
| **Model's Own** | No directive at all. Whatever the model brings on its own. | -- |
| **Custom** | Your own directive, written in the box below the field. | -- |

Picking an avatar switches to its persona, its voice and its name -- one choice you can make
from either end. *Your own* is the exception: an avatar you designed has no persona of its
own, so it leaves yours alone.

### What each persona reaches without asking

A persona reads its own field automatically. Everything else — another tool, or a path outside
that field — is shown to you first and runs only if you approve it, **for that one call**. The
next call asks again. There is no elevated mode and no timed grant.

**If it keeps asking about the same folder, widen the field instead of approving forever.**
Settings has a box — *Folders it may read without asking* — under the persona picker. Put a
folder in it (`~/Projects, ~/Documents`, comma separated) and that persona stops asking about
it. The list belongs to the persona selected above it and to no other, because a list that
widened everything at once would leave the specialities on screen while quietly deleting the
point of them. Up to twelve folders each. Anything the guard would refuse is refused as you
save it, with the reason shown, rather than being stored and failing later.

| Persona | Reads without asking |
|---|---|
| **System Diagnosis** | System logs and service state |
| **Security & White Hat** | Network configuration and service state |
| **Coding** | The project directory (the folder Aether1 was started in — never your home directory, and only when it sits inside it) |
| **Cites Sources** | Your notes and the project directory |
| **Creative Work** | Your notes |
| **Conversational**, **To the Point**, **Model's Own**, **Custom** | Your notes, and the telemetry the HUD already shows |

The last row is deliberate. Those four are styles rather than specialities, and inventing a
field for them to make the table symmetrical would hand out access nothing asked for.

On **Linux** those roots are `/var/log`, the systemd unit directories, and the networking
files in `/etc`. On **Windows** they are the event logs under
`%SystemRoot%\System32\winevt\Logs`, the servicing logs in `%SystemRoot%\Logs`, and
`%SystemRoot%\System32\drivers\etc` — the `hosts`, `services`, `protocol` and `networks`
files, which is where Windows keeps what Unix keeps in `/etc`. Windows has no service-state
*file* — services live in the registry — so **Security & White Hat** and **System Diagnosis**
answer "what is running" from `list_processes` and the System event log instead. Note that
`.evtx` event logs are binary, so `read_file` only reports their size — **`read_event_log`
is the tool that actually reads them**, via `wevtutil`. It takes a channel (`System`,
`Application`, `Setup`, `Security`, or a full name like
`Microsoft-Windows-Kernel-Boot/Operational`), returns the newest entries first, and can filter
by severity (`min_level`), age (`since_hours`) and source (`provider`). "Show me today's disk
errors" is one call. It is in the field of **System Diagnosis** and **Security & White Hat**,
so for those two it runs without asking; every other persona proposes it first.

The **Security** channel is readable only by an administrator — that is Windows refusing, not
Aether1, and the error says so.

Two things this does *not* change. Every persona has the same tools available — the field
decides what runs without a prompt, not what is possible. And the paths Aether1 never reads at
all (SSH and GPG keys, cloud credentials, `/etc/shadow`, and the rest) stay off limits inside a
persona's own field and after an approval alike: a field can only narrow.

**Settings describes your machine, not a generic one.** The help text under *Let it look at
this computer* names `/etc`, `/proc` and `/var/log` on Linux, and the Windows event logs and
`System32\drivers\etc` on Windows. It asks the running copy of Aether1 which it is, never the
browser — so when you open the HUD on a Windows laptop pointed at a Linux desktop, you get the
Linux wording, because the Linux machine is the one being described. The same goes for the
setup and voice guides, and for the folders named when no Piper voice or Whisper model can be
found.

### Running programs

Aether1 can run programs on your machine, and it starts able to run a few. The list is in
Settings under **Programs it may ask to run**, and on a fresh install it holds only programs
that *look* at the machine and cannot change it:

- **Linux and macOS:** `uname`, `uptime`, `df`, `free`, `lsblk`, `lscpu`, `lspci`, `lsusb`,
  `nproc`, `arch`, `ps`, `whoami`, `id`
- **Windows:** `systeminfo`, `tasklist`, `driverquery`, `whoami`

The rule behind that list is stricter than it looks: **no option you could give any of those
programs changes anything**. That is why `systemctl` is not on it (it has `stop`), nor `git`
(it has `reset --hard`), nor `ipconfig` (it has `/release`), nor even `hostname` or `date`,
which set as well as show. The list matches on the program's *name* only, and the AI chooses
what to put after it — so one destructive option is enough to keep a program off.

Add whatever you like to the box, or empty it to allow nothing. It is seeded once, on first
run, so emptying it stays empty.

**Every run still asks you first, always.** Being on the list makes a command something the
companion may *propose*; it never makes one run by itself. Running a command is the one thing
that cannot be put on "don't ask me again", no matter what you tick. There is also no shell
behind it — no pipes, no redirects, no wildcards, no chaining two commands with `&&`.

### How it asks for a tool

Two ways, chosen by whichever provider you are pointed at, and you should not be able to tell
the difference from the chat.

**OpenAI, Groq, Gemini and Anthropic** are given the tool list as part of the request, in the
format each of them documents, and ask for a tool through their own machinery. Nothing about
the request appears in the reply text.

**Ollama and LM Studio** use the original approach instead: the tools are described in the
system prompt and the model asks for one by writing a small block of JSON, which Aether1
recognises mid-stream and hides from you. This is on purpose. Ollama is driven through an
endpoint that has no tools field at all, and LM Studio is the one most likely to be an older
install that would reject the request outright — better the way that works everywhere than a
failure you have to diagnose.

Either way you see the same thing: a one-line trace of what actually ran, and then the answer.

Because a persona now carries access, the companion cannot change its own persona, avatar or
directive — those settings are out of reach of `set_aether_setting`, alongside API keys and its
own permissions. "Switch persona to Security" is not something it may approve itself into.

## Your own terminal

There is a terminal in the HUD. It is a real one — the same shell you get from a terminal
app, running as you, in your home folder, with a proper keyboard attached to it. `sudo` can
ask you for your password. `yay` can ask you which package you meant. `htop` and `less`
redraw when you resize the panel. That is the whole point: a box that only runs a command
and prints what came back is not a terminal, and the moment you need it, it fails you.

Click **▶ Start a shell** in the Terminal panel and type. Type `exit`, or press the button
again, and it closes. If you don't want it there at all, switch the panel off like any other
(**Rearranging the HUD**, above).

### It is yours, and only yours

The obvious worry about putting a shell inside an AI companion is the AI. So it was built so
that your companion cannot reach it — not "is not allowed to", cannot:

- **It is not a tool.** There is no `run_in_terminal` for the model to ask for. Nothing you
  say in a conversation, and nothing the model reads from a web page or a note, can put a
  character into it. The refusal list that already keeps the AI out of its own permissions
  and its own conversations now keeps it out of the terminal too.
- **It is not on the network.** The terminal exists only in the desktop app. It has no
  address — there is no route for it in the browser server, so there is nothing to reach
  over your LAN even with `--lan` on and a pairing token in hand. The code that runs it
  isn't merely unrouted in `--serve` mode; that mode returns before the desktop app is ever
  built, so it is never loaded at all. Open the HUD in a browser and the panel isn't there.
- **Nothing is written down.** What you type and what comes back go from your keyboard to
  the shell and back to the screen. None of it is stored, logged, put in the vault, or added
  to the conversation the model sees.

`scripts/check_terminal_isolation.sh` checks all three every time CI runs, so a future
change that quietly connects the terminal to the AI fails the build rather than shipping.

### One thing that came with it

Shells keep a history file — `~/.bash_history` and friends — which is a verbatim record of
every command you have typed, and people type passwords and API keys into commands. Your
home folder is somewhere the AI may read, so until now it could have read that file. It
can't any more: history files are on the same permanent deny list as your SSH keys, which no
setting, persona or approval of yours can lift. That was true before there was a terminal
here; building one made it urgent, because Aether1 would now be writing that file itself.

## Making your own avatar

The avatar is a module of its own -- everything that draws it lives in
`frontend/js/hologram/`, and the rest of Aether1 talks to it through five function calls.
`frontend/js/hologram/README.md` is the guide; the short version:

**Build one without writing code.** In the HUD, pick **Your own** in the avatar row and
press **Customise** beside it -- or open Settings and use *Design your own avatar*. Either
opens the avatar workbench. Pick a core, a body and a voice equaliser, set the sizes and
the motion, and press *Use this in Aether1*; the HUD updates as soon as you save, with no
reload. What gets saved is a *recipe*: a few lines of settings you can paste to someone
else safely, because settings cannot run.

**Or write one.** Copy `frontend/js/hologram/avatar-template.js` -- a working avatar with
every line explained. An avatar is an id and three functions: build it, animate it, and
recolour it when the theme changes.

The workbench opens wearing whatever theme the HUD is wearing, and follows along if you
change it while both are open -- an avatar previewed in a theme you do not use tells you less
than one previewed in the theme you will actually see it in. Its COLOUR THEME dropdown is for
checking a design against a colour you might switch to; picking one there changes only that
window, and **Your theme (as set in the HUD)** puts it back.

The workbench loads the avatar and nothing else -- no backend, no model server, no
network -- and fakes what the HUD would normally supply: the four states as buttons, and
a voice simulator that feeds the same 64 frequency bins a real voice would, so an
equaliser can be built in silence. It also loads someone else's avatar file, for trying
one before installing it.

An avatar file is a program and runs with the same access as the page, so load files you
wrote or trust. A recipe carries no code.

## Command line

Once installed (`./setup.sh`, or `scripts/install_desktop_app.sh`), the same binary that
runs the desktop app answers from a terminal without the HUD open:

```sh
aether1 prompt "what is eating my RAM"   # ask; the reply goes to stdout
echo "status" | aether1 prompt           # or pipe the question in
aether1 status                           # system diagnostic report (--json for raw)
aether1 say "systems nominal"            # speak, in the configured persona voice
aether1 toggle                           # summon/dismiss the HUD of a running instance
aether1 face                             # avatar fullscreen on a spare screen (Esc closes it)
aether1 --help
```

The HUD is also bound to a global hotkey — `Super+Shift+A` by default, changeable under
Settings (empty disables it). On Wayland, where no application is allowed to grab keys
system-wide, bind your compositor to `aether1 toggle` instead; for Hyprland:

```
bind = SUPER SHIFT, A, exec, aether1 toggle
```

On **Windows**, `setup.bat` creates shortcuts but does not put anything on `PATH`, so
`aether1` is not a command there — call the executable by path from the checkout:

```powershell
.\src-tauri\target\release\aether1.exe prompt "what is eating my RAM"
```

To type `aether1` instead, add that folder to your own `PATH` once (no administrator
rights needed, and it takes effect in new terminals):

```powershell
[Environment]::SetEnvironmentVariable(
    'PATH',
    [Environment]::GetEnvironmentVariable('PATH', 'User') + ';' + (Resolve-Path .\src-tauri\target\release),
    'User')
```

`prompt` shares one conversation history and one memory store with the HUD, so anything
you tell it from a script is there next time you open the window.

### Serving the HUD to a browser

`aether1 --serve` runs the app headless and serves the HUD at
<http://localhost:8378> — the development flow, and the fallback if the native
webview misbehaves. It listens on this machine only.

That default matters: reaching 127.0.0.1 already means being a process on this machine, so
loopback mode needs no password of its own -- the operating system is the password.

`aether1 --serve --lan` opens it to your whole network, for the case where you genuinely
want the HUD on your phone or another machine in the house. Unlike loopback mode, this
*does* need a password: the first time you run it, AETHER1 prints a 12-word pairing phrase
(shown once — write it down). Type that phrase into the other device's pairing prompt (or
`POST` it as `{"phrase": "..."}` to `/api/pair`) to get back a token; without it, every
route refuses — your conversation, the action log, everything. Run `aether1 pair` any time
to generate a new phrase and revoke the old one.

## Project goals

Where this is headed, and why: [docs/GOALS.md](docs/GOALS.md).
