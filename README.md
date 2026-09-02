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
  - Auto-installer for Linux and Windows.
  - Easy installation from GitHub via script or via packaging the entire system into a single `.tar.gz` bundle to transfer to a separate system.

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
WebView2 runtime that ships with Windows. Then, in the checkout:

```powershell
.\setup.bat
```

The leading `.\` is required in PowerShell, which does not run scripts from the current
directory without it. In `cmd.exe`, plain `setup.bat` works.

It builds the app and adds Start Menu and desktop shortcuts (no administrator rights
needed). Launch it from either, or run `.\start.bat`.

Get the checkout with git rather than as a ZIP if you can — `git pull` is then how you
update, and the setup script installs hooks that rebuild the app whenever you do:

```powershell
git clone https://github.com/TridentSpoon/Aether1.git
cd Aether1
```

Aether1 lives in the **notification area** — click its icon to show or hide the HUD, and
closing the window leaves it running there rather than quitting.

`start.bat --browser` runs the headless server and opens the HUD in a browser tab instead.
That is the development flow, and the fallback if the webview misbehaves; there is no tray
icon on that path, because the tray belongs to the native app.

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
| Arch, CachyOS, Manjaro, EndeavourOS | `base-devel curl wget file openssl webkit2gtk-4.1 gtk3 libappindicator-gtk3 librsvg xdotool libnotify` |
| Fedora, Nobara, RHEL | `webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel librsvg2-devel openssl-devel curl wget file xdotool libnotify` plus the `c-development` group |
| Debian, Ubuntu, Pop!\_OS, Mint | `build-essential pkg-config curl wget file libssl-dev libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libnotify-bin` |
| openSUSE | `webkit2gtk3-soup2-devel gtk3-devel libappindicator3-devel librsvg-devel libopenssl-devel curl wget file xdotool libnotify-tools` |

If the build fails, the error names the missing piece: look for a package ending in `-dev`
or `-devel`, install it, and re-run `./setup.sh`.

### Optional: a voice that works offline

Speech works out of the box using a cloud service, which means the text of everything the
AI says leaves your machine. To keep it local, install either or both:

- **[Piper](https://github.com/rhasspy/piper)** for speech, plus a `.onnx` voice in
  `~/.local/share/piper/voices`.
- **[whisper.cpp](https://github.com/ggml-org/whisper.cpp)** for listening, plus a `.bin`
  model in `~/.local/share/whisper`.

Aether1 finds them on its own and prefers them. Settings → Speech Engine says which of the
two are local and what is missing.

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

`prompt` shares one conversation history and one memory store with the HUD, so anything
you tell it from a script is there next time you open the window.

## Project goals

Where this is headed, and why: [docs/GOALS.md](docs/GOALS.md).
