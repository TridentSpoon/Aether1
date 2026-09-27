//! Which speaker it talks out of, and which microphone it listens on.
//!
//! Everywhere else in AETHER1 the audio devices were whatever the machine happened to
//! hand over. That is fine on a laptop with one speaker and wrong on every desk with a
//! headset, an interface and an HDMI monitor that also claims to be a sound card: the
//! companion speaks into whichever of those the system picked that morning, and the
//! report available to the person sitting there is silence -- the same failure mode
//! `voice_setup.rs` exists to make legible, arriving by a different door.
//!
//! So this asks the operating system what it actually has, by name, and the answer is
//! shown in Settings as a list to pick from.
//!
//! **Read-only, and it runs nothing it was told to run.** Every command below is a fixed
//! program with fixed arguments; nothing from the database, the page or the network
//! becomes part of a command line. What comes back is *text from the system* and is
//! treated as such -- parsed, truncated, and never handed to a shell. Choosing a device
//! writes a name into settings; it does not reconfigure the machine's audio, which stays
//! the operating system's business.
//!
//! The parsers are separate from the commands on purpose: a container with no sound
//! server can still run every test in this file, which is the only way this code was
//! testable at all.

use std::process::Command;

use serde::Serialize;

/// A ceiling on what any of these commands may hand back. `system_profiler` in particular
/// is verbose, and a machine with a deep audio tree should not be able to pull megabytes
/// of text into memory because somebody opened a settings pane.
const MAX_OUTPUT_BYTES: usize = 256 * 1024;

/// Which way the sound goes. Kept as a type rather than a bool because "is_input" reads
/// backwards half the time it is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    /// A speaker, headset or anything else sound comes out of.
    Output,
    /// A microphone or other capture device.
    Input,
}

/// One device the system says it has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AudioDevice {
    /// What the system calls it. On Linux this is the PulseAudio/PipeWire node name,
    /// which is also what `PULSE_SINK` wants; elsewhere it is the display name, because
    /// that is the only identifier those tools give out.
    pub id: String,
    /// What a person would call it.
    pub label: String,
    pub direction: Direction,
    /// Whether the system is currently sending sound here by default. Only Linux and
    /// macOS report this; on Windows every entry reads false and the note says why.
    pub is_default: bool,
}

/// Everything the scan found, plus an honest account of how it found it.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceReport {
    pub outputs: Vec<AudioDevice>,
    pub inputs: Vec<AudioDevice>,
    /// The tool that answered: `pactl`, `wpctl`, `powershell`, `system_profiler`, or
    /// `none` when nothing did. Shown in the HUD, because "no devices" and "nothing here
    /// can tell me about devices" are different problems with different fixes.
    pub source: &'static str,
    /// One sentence for the person reading the panel. Empty when the list speaks for
    /// itself.
    pub note: String,
}

impl DeviceReport {
    fn empty(source: &'static str, note: impl Into<String>) -> Self {
        DeviceReport {
            outputs: Vec::new(),
            inputs: Vec::new(),
            source,
            note: note.into(),
        }
    }
}

/// Runs one fixed command and hands back its stdout, capped.
fn run(program: &str, args: &[&str]) -> Option<String> {
    if which::which(program).is_err() {
        return None;
    }
    let out = Command::new(program).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    if text.len() > MAX_OUTPUT_BYTES {
        text.truncate(MAX_OUTPUT_BYTES);
    }
    Some(text)
}

/// What this machine has. Never fails: a machine nothing can be learned about gets an
/// empty report with a reason in it, and the HUD falls back to "whatever the system
/// picks", which is exactly what it did before this module existed.
pub fn scan() -> DeviceReport {
    #[cfg(target_os = "linux")]
    {
        linux_scan()
    }
    #[cfg(target_os = "windows")]
    {
        windows_scan()
    }
    #[cfg(target_os = "macos")]
    {
        mac_scan()
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        DeviceReport::empty("none", "This system is not one AETHER1 knows how to ask.")
    }
}

/* ----------------------------- Linux ----------------------------- */

#[cfg(target_os = "linux")]
fn linux_scan() -> DeviceReport {
    if let Some(sinks) = run("pactl", &["list", "sinks"]) {
        let sources = run("pactl", &["list", "sources"]).unwrap_or_default();
        let default_sink = run("pactl", &["get-default-sink"]).unwrap_or_default();
        let default_source = run("pactl", &["get-default-source"]).unwrap_or_default();
        let mut report = parse_pactl(&sinks, &sources, default_sink.trim(), default_source.trim());
        if report.outputs.is_empty() && report.inputs.is_empty() {
            report.note =
                "The sound server answered but listed nothing. Aether1 will use whatever \
                 the system picks."
                    .into();
        }
        return report;
    }

    if let Some(text) = run("wpctl", &["status"]) {
        return parse_wpctl(&text);
    }

    DeviceReport::empty(
        "none",
        "Neither pactl nor wpctl is on this machine, so the list of devices cannot be \
         read. Speech goes to whatever the system picks.",
    )
}

/// `pactl list sinks` / `list sources` are blocks separated by a blank line, each with a
/// `Name:` (the identifier) and a `Description:` (the human one).
///
/// Monitor sources are dropped: every sink has one, they are loopbacks of what is already
/// playing rather than microphones, and a list where half the entries record the speakers
/// is a list nobody can choose from.
pub fn parse_pactl(
    sinks: &str,
    sources: &str,
    default_sink: &str,
    default_source: &str,
) -> DeviceReport {
    let mut outputs = parse_pactl_blocks(sinks, Direction::Output, default_sink);
    let mut inputs = parse_pactl_blocks(sources, Direction::Input, default_source);
    inputs.retain(|d| !d.id.ends_with(".monitor"));
    outputs.sort_by_key(|d| std::cmp::Reverse(d.is_default));
    inputs.sort_by_key(|d| std::cmp::Reverse(d.is_default));
    DeviceReport {
        outputs,
        inputs,
        source: "pactl",
        note: String::new(),
    }
}

fn parse_pactl_blocks(text: &str, direction: Direction, default: &str) -> Vec<AudioDevice> {
    let mut devices = Vec::new();
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;

    let mut flush = |name: &mut Option<String>, description: &mut Option<String>| {
        if let Some(id) = name.take() {
            let label = description.take().unwrap_or_else(|| id.clone());
            let is_default = !default.is_empty() && id == default;
            devices.push(AudioDevice {
                id,
                label,
                direction,
                is_default,
            });
        } else {
            *description = None;
        }
    };

    for line in text.lines() {
        let trimmed = line.trim();
        // A new block starts at "Sink #n" / "Source #n" -- which is also the only place
        // the previous one can be flushed from, since blocks are not blank-line separated
        // on every pactl version.
        if trimmed.starts_with("Sink #") || trimmed.starts_with("Source #") {
            flush(&mut name, &mut description);
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Name:") {
            name = Some(rest.trim().to_string());
        } else if let Some(rest) = trimmed.strip_prefix("Description:") {
            description = Some(rest.trim().to_string());
        }
    }
    flush(&mut name, &mut description);
    devices
}

/// `wpctl status` draws a tree. The rows that matter look like
/// ` │  *   49. Built-in Audio Analogue Stereo   [vol: 0.40]`, under a `Sinks:` or
/// `Sources:` heading, with `*` marking the default.
///
/// The id here is the label, because the tree gives no node name -- which is stated in
/// the note rather than papered over, since it means `PULSE_SINK` cannot be set from it.
pub fn parse_wpctl(text: &str) -> DeviceReport {
    let mut outputs = Vec::new();
    let mut inputs = Vec::new();
    let mut direction: Option<Direction> = None;

    for line in text.lines() {
        let bare = line.trim_matches(|c: char| {
            c.is_whitespace() || c == '│' || c == '├' || c == '└' || c == '─'
        });
        if bare.starts_with("Sinks:") {
            direction = Some(Direction::Output);
            continue;
        }
        if bare.starts_with("Sources:") {
            direction = Some(Direction::Input);
            continue;
        }
        if bare.starts_with("Filters:")
            || bare.starts_with("Streams:")
            || bare.starts_with("Audio")
            || bare.starts_with("Video")
            || bare.starts_with("Settings")
        {
            direction = None;
            continue;
        }
        let Some(dir) = direction else { continue };

        let is_default = bare.starts_with('*');
        let rest = bare.trim_start_matches('*').trim();
        // "49. Built-in Audio [vol: 0.40]"
        let Some((number, tail)) = rest.split_once('.') else {
            continue;
        };
        if number.trim().parse::<u32>().is_err() {
            continue;
        }
        let label = tail
            .split_once("[vol:")
            .map(|(before, _)| before)
            .unwrap_or(tail)
            .trim()
            .to_string();
        if label.is_empty() {
            continue;
        }
        let device = AudioDevice {
            id: label.clone(),
            label,
            direction: dir,
            is_default,
        };
        match dir {
            Direction::Output => outputs.push(device),
            Direction::Input => inputs.push(device),
        }
    }

    DeviceReport {
        outputs,
        inputs,
        source: "wpctl",
        note: String::new(),
    }
}

/* ---------------------------- Windows ---------------------------- */

#[cfg(target_os = "windows")]
fn windows_scan() -> DeviceReport {
    // Win32_SoundDevice is the one list available without a module nobody has installed.
    // It lists adapters rather than endpoints and says nothing about which is default or
    // which way the sound goes, all of which the note below admits to.
    let script = "Get-CimInstance Win32_SoundDevice | \
                  Where-Object { $_.Status -eq 'OK' } | \
                  ForEach-Object { $_.Name }";
    match run(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", script],
    ) {
        Some(text) => {
            let mut report = parse_windows(&text);
            if report.outputs.is_empty() {
                report.note = "Windows listed no working sound devices.".into();
            }
            report
        }
        None => DeviceReport::empty(
            "none",
            "Windows would not list the sound devices, so speech goes to whatever it \
             picks.",
        ),
    }
}

/// One device name per line, which is all Windows gives without an extra module.
///
/// Every entry is offered as an output *and* an input, because the list does not say and
/// guessing from the name would be worse than offering both: a person can see which of
/// their own devices is a microphone.
///
/// Compiled on Windows, and in every test build so the parser is covered from a Linux
/// container -- which is the only place it ever gets run against a fixture.
#[cfg(any(target_os = "windows", test))]
pub fn parse_windows(text: &str) -> DeviceReport {
    let names: Vec<String> = text
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    let make = |direction: Direction| -> Vec<AudioDevice> {
        names
            .iter()
            .map(|name| AudioDevice {
                id: name.clone(),
                label: name.clone(),
                direction,
                is_default: false,
            })
            .collect()
    };

    DeviceReport {
        outputs: make(Direction::Output),
        inputs: make(Direction::Input),
        source: "powershell",
        note: if names.is_empty() {
            String::new()
        } else {
            "Windows lists sound hardware rather than which of it is default, so none is \
             marked. Pick the one you use."
                .into()
        },
    }
}

/* ----------------------------- macOS ----------------------------- */

#[cfg(target_os = "macos")]
fn mac_scan() -> DeviceReport {
    match run("system_profiler", &["SPAudioDataType"]) {
        Some(text) => parse_system_profiler(&text),
        None => DeviceReport::empty(
            "none",
            "system_profiler would not answer, so speech goes to whatever macOS picks.",
        ),
    }
}

/// `system_profiler SPAudioDataType` prints each device as an indented heading ending in
/// a colon, followed by `Key: Value` lines. A device with `Input Channels` is a
/// microphone, one with `Output Channels` is a speaker, and an aggregate device can be
/// both -- so this reads the channel lines rather than assuming.
///
/// Compiled on macOS and in test builds, for the same reason as `parse_windows`.
#[cfg(any(target_os = "macos", test))]
pub fn parse_system_profiler(text: &str) -> DeviceReport {
    struct Pending {
        name: String,
        indent: usize,
        input: bool,
        output: bool,
        default_in: bool,
        default_out: bool,
    }

    let mut outputs = Vec::new();
    let mut inputs = Vec::new();
    let mut current: Option<Pending> = None;

    fn flush(p: Option<Pending>, outputs: &mut Vec<AudioDevice>, inputs: &mut Vec<AudioDevice>) {
        let Some(p) = p else { return };
        if p.output {
            outputs.push(AudioDevice {
                id: p.name.clone(),
                label: p.name.clone(),
                direction: Direction::Output,
                is_default: p.default_out,
            });
        }
        if p.input {
            inputs.push(AudioDevice {
                id: p.name.clone(),
                label: p.name,
                direction: Direction::Input,
                is_default: p.default_in,
            });
        }
    }

    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();

        if let Some((key, value)) = trimmed.split_once(':') {
            let value = value.trim();
            if value.is_empty() {
                // A heading. Devices sit deeper than the "Audio:" and "Devices:" ones.
                if indent >= 8 {
                    flush(current.take(), &mut outputs, &mut inputs);
                    current = Some(Pending {
                        name: key.trim().to_string(),
                        indent,
                        input: false,
                        output: false,
                        default_in: false,
                        default_out: false,
                    });
                }
                continue;
            }
            let Some(dev) = current.as_mut() else {
                continue;
            };
            if indent <= dev.indent {
                continue;
            }
            let yes = value.eq_ignore_ascii_case("yes");
            match key.trim() {
                "Input Channels" => dev.input = true,
                "Output Channels" => dev.output = true,
                "Default Input Device" if yes => {
                    dev.input = true;
                    dev.default_in = true;
                }
                "Default Output Device" if yes => {
                    dev.output = true;
                    dev.default_out = true;
                }
                _ => {}
            }
        }
    }
    flush(current.take(), &mut outputs, &mut inputs);

    DeviceReport {
        outputs,
        inputs,
        source: "system_profiler",
        note: String::new(),
    }
}

/* --------------------------- Using one --------------------------- */

/// The environment a child audio player should inherit so it plays on the chosen output.
///
/// Only PulseAudio/PipeWire can be steered this way, and only when the identifier is a
/// real node name -- which is why `wpctl`'s labels are not used for this and the caller
/// gets nothing back rather than a guess. Everywhere else the chosen device is applied in
/// the HUD, where the browser's own audio graph can be pointed at it.
pub fn player_env(device_id: &str) -> Option<(&'static str, String)> {
    let id = device_id.trim();
    if id.is_empty() || id.contains(char::is_whitespace) {
        return None;
    }
    Some(("PULSE_SINK", id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SINKS: &str = "\
Sink #49
\tState: RUNNING
\tName: alsa_output.pci-0000_0c_00.4.analog-stereo
\tDescription: Starship/Matisse HD Audio Analogue Stereo
\tDriver: PipeWire

Sink #52
\tState: SUSPENDED
\tName: alsa_output.usb-Logitech_PRO_X-00.analog-stereo
\tDescription: PRO X Analogue Stereo
";

    const SOURCES: &str = "\
Source #50
\tName: alsa_output.pci-0000_0c_00.4.analog-stereo.monitor
\tDescription: Monitor of Starship/Matisse
\tDriver: PipeWire

Source #51
\tName: alsa_input.usb-Logitech_PRO_X-00.mono-fallback
\tDescription: PRO X Mono
";

    #[test]
    fn pactl_names_and_descriptions() {
        let r = parse_pactl(
            SINKS,
            SOURCES,
            "alsa_output.usb-Logitech_PRO_X-00.analog-stereo",
            "",
        );
        assert_eq!(r.outputs.len(), 2);
        // The default is listed first, so the list opens on what is in use.
        assert_eq!(r.outputs[0].label, "PRO X Analogue Stereo");
        assert!(r.outputs[0].is_default);
        assert!(!r.outputs[1].is_default);
        assert_eq!(
            r.outputs[1].id,
            "alsa_output.pci-0000_0c_00.4.analog-stereo"
        );
    }

    #[test]
    fn pactl_drops_monitor_sources() {
        let r = parse_pactl(SINKS, SOURCES, "", "");
        assert_eq!(r.inputs.len(), 1, "the monitor source is not a microphone");
        assert_eq!(r.inputs[0].label, "PRO X Mono");
        assert_eq!(r.inputs[0].direction, Direction::Input);
    }

    #[test]
    fn pactl_falls_back_to_the_name_when_there_is_no_description() {
        let r = parse_pactl("Sink #1\n\tName: bare_sink\n", "", "", "");
        assert_eq!(r.outputs[0].label, "bare_sink");
    }

    #[test]
    fn pactl_empty_input_is_an_empty_list_not_a_panic() {
        let r = parse_pactl("", "", "", "");
        assert!(r.outputs.is_empty() && r.inputs.is_empty());
    }

    #[test]
    fn wpctl_tree() {
        let text = "\
Audio
 ├─ Devices:
 │      47. Starship/Matisse HD Audio      [alsa]
 │
 ├─ Sinks:
 │  *   49. Built-in Audio Analogue Stereo [vol: 0.40]
 │      52. PRO X Analogue Stereo          [vol: 1.00]
 │
 ├─ Sources:
 │  *   51. PRO X Mono                     [vol: 1.00]
 │
 ├─ Filters:
 │
Video
";
        let r = parse_wpctl(text);
        assert_eq!(r.source, "wpctl");
        assert_eq!(r.outputs.len(), 2);
        assert_eq!(r.outputs[0].label, "Built-in Audio Analogue Stereo");
        assert!(r.outputs[0].is_default);
        assert_eq!(r.outputs[1].label, "PRO X Analogue Stereo");
        assert_eq!(r.inputs.len(), 1);
        assert!(r.inputs[0].is_default);
    }

    #[test]
    fn windows_offers_each_name_both_ways() {
        let r = parse_windows("Realtek High Definition Audio\n\nNVIDIA Virtual Audio\n");
        assert_eq!(r.outputs.len(), 2);
        assert_eq!(r.inputs.len(), 2);
        assert!(r.outputs.iter().all(|d| !d.is_default));
        assert!(
            !r.note.is_empty(),
            "the panel has to say why none is marked"
        );
    }

    #[test]
    fn system_profiler_reads_channels_and_defaults() {
        let text = "\
Audio:

    Devices:

        MacBook Pro Speakers:

          Default Output Device: Yes
          Default System Output Device: Yes
          Output Channels: 2
          Manufacturer: Apple Inc.

        MacBook Pro Microphone:

          Default Input Device: Yes
          Input Channels: 1

        Scarlett 2i2:

          Input Channels: 2
          Output Channels: 2
";
        let r = parse_system_profiler(text);
        let out: Vec<&str> = r.outputs.iter().map(|d| d.label.as_str()).collect();
        assert_eq!(out, vec!["MacBook Pro Speakers", "Scarlett 2i2"]);
        let inp: Vec<&str> = r.inputs.iter().map(|d| d.label.as_str()).collect();
        assert_eq!(inp, vec!["MacBook Pro Microphone", "Scarlett 2i2"]);
        assert!(r.outputs[0].is_default);
        assert!(!r.outputs[1].is_default);
        assert!(r.inputs[0].is_default);
    }

    #[test]
    fn player_env_only_for_a_real_node_name() {
        assert_eq!(
            player_env("alsa_output.usb-Logitech_PRO_X-00.analog-stereo"),
            Some((
                "PULSE_SINK",
                "alsa_output.usb-Logitech_PRO_X-00.analog-stereo".to_string()
            ))
        );
        assert_eq!(player_env(""), None);
        // A wpctl label, which is not a node name and must not be passed off as one.
        assert_eq!(player_env("PRO X Analogue Stereo"), None);
    }
}
