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

## Command line

Once installed (`./setup.sh`, or `scripts/install_desktop_app.sh`), the same binary that
runs the desktop app answers from a terminal without the HUD open:

```sh
aether1 prompt "what is eating my RAM"   # ask; the reply goes to stdout
echo "status" | aether1 prompt           # or pipe the question in
aether1 status                           # system diagnostic report (--json for raw)
aether1 say "systems nominal"            # speak, in the configured persona voice
aether1 --help
```

`prompt` shares one conversation history and one memory store with the HUD, so anything
you tell it from a script is there next time you open the window.

## Project goals

Where this is headed, and why: [docs/GOALS.md](docs/GOALS.md).
