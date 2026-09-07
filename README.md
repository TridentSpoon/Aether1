**The Aether1 Platform**

A basic project for self-hosted or API connected AI companions with a sleek holographic presence, built in the spirit of the the future we saw in Sci-Fi growing up.

Designed natively on Linux and with future OS compatibility in mind, featuring real-time system and AI rellevant telemetry, an interactive 3D holographic avatar, with speech capabilities, a local persistent memory, and a system tray companion in or hovering next to your notification bar.

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
    -- see "Offline install" below.

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
  a normal installer, no PowerShell required.

Both bundle Piper (TTS) and whisper.cpp (STT) with a voice and a model already inside, so
speech works fully offline immediately, not just once you separately track those down --
see the "Speech" section below for what that buys you either way. `THIRD_PARTY_NOTICES.md`
in each bundle credits what's inside.

This is a different distribution from cloning the repo: it installs a fixed version rather
than a live checkout, so `git pull` isn't how you update it -- download a newer release
instead. If you want to build and modify the code, use `setup.sh`/`setup.bat` above instead.

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
| Arch, CachyOS, Manjaro, EndeavourOS | `base-devel curl wget file openssl webkit2gtk-4.1 gtk3 libappindicator-gtk3 librsvg xdotool libnotify espeak-ng` |
| Fedora, Nobara, RHEL | `webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel librsvg2-devel openssl-devel curl wget file xdotool libnotify espeak-ng` plus the `c-development` group |
| Debian, Ubuntu, Pop!\_OS, Mint | `build-essential pkg-config curl wget file libssl-dev libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libnotify-bin espeak-ng` |
| openSUSE | `webkit2gtk3-soup2-devel gtk3-devel libappindicator3-devel librsvg-devel libopenssl-devel curl wget file xdotool libnotify-tools espeak-ng` |

If the build fails, the error names the missing piece: look for a package ending in `-dev`
or `-devel`, install it, and re-run `./setup.sh`.

### Speech: what's local, what isn't, and what always works

Speaking (TTS) has three tiers, tried in that order by the default **Auto** engine so there
is always something to speak with, on a machine that has done nothing but run setup:

1. **[Piper](https://github.com/rhasspy/piper)** -- the best-sounding local voice, if you
   install its binary plus a `.onnx` voice in `~/.local/share/piper/voices` (the offline
   installer above does this for you).
2. **Cloud** (Microsoft) -- better than the OS voice, but the text of everything the AI
   says leaves your machine, and it needs a network.
3. **This OS's own voice** -- SAPI on Windows (ships with every edition, nothing to
   install) or `espeak-ng` on Linux (installed by `./setup.sh`). Lower audio quality than
   the other two, but it cannot be "not installed" the way Piper can or offline the way the
   cloud engine is, which is what makes it the guaranteed fallback rather than an optional
   extra.

Listening (STT) only has the first two of those -- there is no universal OS-level
equivalent to fall back to yet:

- **[whisper.cpp](https://github.com/ggml-org/whisper.cpp)** for local listening, plus a
  `.bin` model in `~/.local/share/whisper` (also handled by the offline installer).
- Cloud, otherwise.

Settings → Speech Engine reports which of these are actually available on this machine and
what's missing for the rest, and lets you pin a specific tier instead of Auto (e.g. "Local
only" to guarantee nothing ever leaves the machine, refusing to speak rather than silently
falling back to the cloud).

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

## Choosing a model

Open **Settings** and Aether1 looks for model servers already running on this machine. Any
that answer appear in **LOCAL SERVERS FOUND ON THIS MACHINE**; picking one fills in the
provider, the address and the list of models it can run, so there is nothing to look up.

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

The bar along the top carries what this is and what is on screen:

- **The wordmark** shows your companion's own name once you have given it one, turning over
  to AETHER1 PLATFORM every so often so what it is running on stays visible. The version sits
  directly under it in the desktop app.
- **Avatar** shows the one that is selected. Click it and the other seven slide out, along
  with the way into the workbench when *Your own* is the one you are using.
- **Theme** sits under the avatar and switches between Solar, Eclipse and Cyberpunk.
- **The menu** (☰) holds SFX, Activity, Clear conversation and Settings. They live behind one
  control so they are reachable at any window width rather than being the first thing pushed
  off the edge of a narrow one.

**The chin bar** is the slim strip along the bottom. It shows what the companion is doing --
IDLE, LISTENING, THINKING, SPEAKING -- and is always there, so the state never has to compete
for room with anything else.

## Rearranging the HUD

The main window is three columns of panels, and both what is in them and how wide
they are is yours to change.

- **Move a panel:** drag it by the dotted strip along its top edge. Drop it above or
  below another panel, or in a different column -- a glowing line shows where it will
  land. If you empty a column completely it shrinks to a narrow strip labelled
  DROP A PANEL HERE, so you can always put something back.
- **Resize the columns:** drag the divider between two of them. Only those two
  change; the third stays where it is. Double-click a divider to put the widths back.
- **Keyboard:** tab to a panel's grip and use the arrow keys to move it, or to a
  divider and use left/right to resize.

Both are remembered on this machine and survive a restart. Settings has a
**Reset panels to their default places** button when you want the original layout
back.

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

That default matters, because the server has **no password of any kind**. Every part of it
is open to whoever can reach the port: your conversation, the log of everything the
companion has done, and the buttons that approve actions waiting for your permission.

`aether1 --serve --lan` opens it to your whole network, for the case where you genuinely
want the HUD on your phone. On a network you do not control — a cafe, a hotel, a shared
office — that means anyone there can do all of the above. The app says so, every time you
start it that way.

## Project goals

Where this is headed, and why: [docs/GOALS.md](docs/GOALS.md).
