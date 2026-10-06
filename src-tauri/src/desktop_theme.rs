//! What the desktop itself is wearing: light or dark, and the accent colour.
//!
//! AETHER1 already followed the operating system's light/dark setting, but it did so from
//! inside the webview, through the `prefers-color-scheme` media query, and it never asked
//! about the accent colour at all. Both of those are gaps:
//!
//! - `prefers-color-scheme` is the browser's guess. On Windows and macOS the webview is told
//!   the answer and the guess is right. On Linux WebKitGTK infers it from the GTK theme, so a
//!   machine set to dark through the XDG appearance portal with a light GTK theme name
//!   reports light, and the HUD opens white on a dark desktop.
//! - The accent colour has no media query at all. There is no way to ask for it from CSS or
//!   JavaScript on any platform, so the only way to wear the colour somebody picked in their
//!   system settings is to go and read it.
//!
//! So this is the native half of "follow the desktop theme". Each platform is asked the way
//! that platform answers, and every answer is optional: a probe that finds nothing says so
//! rather than inventing a default, because the frontend's fallback (the designed palette,
//! and `prefers-color-scheme` for the shell) is better than a wrong colour.
//!
//! - **Windows** reads two registry keys through PowerShell, the way [`crate::gpu`] reads the
//!   display adapters. `Themes\Personalize\AppsUseLightTheme` is the light/dark choice as it
//!   applies to application windows -- not `SystemUsesLightTheme`, which is the taskbar and
//!   can differ. `DWM\AccentColor` is the accent, with `DWM\ColorizationColor` as the older
//!   fallback. The two are stored in *different channel orders*, which is the whole reason
//!   the decoding below is a pair of named functions with tests rather than one shift.
//! - **Linux** asks the XDG appearance portal first (`org.freedesktop.appearance`), through
//!   `gdbus`. That is the cross-desktop answer, it is what GNOME and KDE both publish, and it
//!   carries the accent as well as the colour scheme. Where there is no portal it falls back
//!   to `gsettings` (GNOME's own keys, where the accent is one of nine names rather than a
//!   colour) and then to `~/.config/kdeglobals`.
//! - **macOS** asks `defaults`. `AppleInterfaceStyle` exists only when dark is on, which is
//!   why its absence is light rather than unknown, and `AppleAccentColor` is a small integer
//!   naming one of the eight swatches in System Settings.
//! - **Anywhere else** reports nothing and says which platform it was asked on, so the HUD
//!   can tell an operator that following the desktop is not available here instead of
//!   silently doing nothing.
//!
//! Every probe is split the same way the rest of the machine-facing code in this crate is
//! split: something that runs a process or reads a file, and a pure function that reads what
//! came back. The parsers are the half that can be quietly wrong -- an accent decoded with
//! its red and blue swapped is still a colour, and nothing but a test notices.

use serde::Serialize;

/// The desktop's own appearance, as far as this machine will say.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct DesktopTheme {
    /// `"light"`, `"dark"`, or `None` when nothing here answered.
    ///
    /// `None` is not "light". It means the frontend should keep using its own fallback --
    /// the `prefers-color-scheme` media query -- rather than being told an answer this side
    /// does not have.
    pub mode: Option<String>,
    /// The accent colour as `#rrggbb`, when the desktop publishes one.
    pub accent: Option<String>,
    /// Which probe answered, for the diagnostics panel: `"windows-registry"`,
    /// `"xdg-portal"`, `"gsettings"`, `"kdeglobals"`, `"macos-defaults"`, or `"none"`.
    pub source: String,
    /// What to tell an operator when something is missing or unsupported. Empty when the
    /// answer is complete, so the HUD can show this line only when there is something to say.
    pub note: String,
}

impl DesktopTheme {
    fn unavailable(note: impl Into<String>) -> Self {
        Self {
            mode: None,
            accent: None,
            source: "none".to_string(),
            note: note.into(),
        }
    }
}

/// Asks this machine what the desktop looks like.
///
/// Not cached. The whole point is that it is re-read when the desktop changes, and the cost
/// is one short-lived process on a path that runs when a window opens or a setting is
/// toggled, never in a loop.
pub fn read() -> DesktopTheme {
    #[cfg(target_os = "windows")]
    {
        read_windows()
    }
    #[cfg(target_os = "linux")]
    {
        read_linux()
    }
    #[cfg(target_os = "macos")]
    {
        read_macos()
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        DesktopTheme::unavailable(format!(
            "Following the desktop theme is not implemented on {}. Pick Daylight, Midnight or \
             Cyberpunk in Settings > Appearance instead.",
            std::env::consts::OS
        ))
    }
}

// ----------------------------------------------------------------------------------------
// Colour helpers
// ----------------------------------------------------------------------------------------

fn hex(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// A colour channel given as a 0..1 float, as the XDG portal and macOS both give them.
///
/// Clamped rather than rejected: a desktop reporting 1.0000001 for white is not an error
/// worth refusing a colour over.
fn channel_from_unit(value: f64) -> Option<u8> {
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    Some((value.min(1.0) * 255.0).round() as u8)
}

// ----------------------------------------------------------------------------------------
// Windows
// ----------------------------------------------------------------------------------------

/// What the PowerShell probe below hands back. Every field is optional because every key is:
/// `AccentColor` is absent on a fresh install that has never had an accent picked, and
/// `AppsUseLightTheme` is absent on builds older than Windows 10 1903.
#[derive(Debug, Default, serde::Deserialize)]
struct WindowsProbe {
    apps_use_light_theme: Option<i64>,
    /// `DWM\AccentColor`, stored 0xAABBGGRR -- alpha, then blue, green, red.
    accent_color: Option<i64>,
    /// `DWM\ColorizationColor`, stored 0xAARRGGBB -- the usual order, and the older key.
    colorization_color: Option<i64>,
}

/// Reads the two personalisation keys out of the registry, through PowerShell.
///
/// PowerShell rather than a registry crate for the same reason [`crate::gpu`] uses it: it is
/// already the way this crate reads `HKCU`, it needs no new dependency, and the values come
/// back as JSON that a pure function can be tested against.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn read_windows() -> DesktopTheme {
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
$personalize = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
$dwm = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\DWM'
[pscustomobject]@{
  apps_use_light_theme = $personalize.AppsUseLightTheme
  accent_color         = $dwm.AccentColor
  colorization_color   = $dwm.ColorizationColor
} | ConvertTo-Json -Compress
"#;
    let Ok(output) = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
        .output()
    else {
        return DesktopTheme::unavailable(
            "Could not run powershell to read the Windows personalisation settings. Pick a \
             theme in Settings > Appearance instead.",
        );
    };
    parse_windows(&String::from_utf8_lossy(&output.stdout))
}

/// 0xAABBGGRR, which is how `DWM\AccentColor` and the `Explorer\Accent` values are stored.
///
/// The byte order is the trap. Windows writes the accent with blue nearest the top, so
/// reading it as the familiar 0xAARRGGBB turns a blue accent orange and an orange one blue --
/// a bug that looks like a colour somebody chose.
fn accent_from_abgr(raw: i64) -> String {
    let v = raw as u32;
    hex(
        (v & 0xff) as u8,
        ((v >> 8) & 0xff) as u8,
        ((v >> 16) & 0xff) as u8,
    )
}

/// 0xAARRGGBB, which is how the older `DWM\ColorizationColor` is stored.
fn accent_from_argb(raw: i64) -> String {
    let v = raw as u32;
    hex(
        ((v >> 16) & 0xff) as u8,
        ((v >> 8) & 0xff) as u8,
        (v & 0xff) as u8,
    )
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn parse_windows(json: &str) -> DesktopTheme {
    let probe: WindowsProbe = serde_json::from_str(json.trim()).unwrap_or_default();
    let mode = probe
        .apps_use_light_theme
        .map(|v| if v == 0 { "dark" } else { "light" }.to_string());
    let accent = probe
        .accent_color
        .map(accent_from_abgr)
        .or_else(|| probe.colorization_color.map(accent_from_argb));

    let note = match (&mode, &accent) {
        (None, None) => {
            "Windows reported neither a light/dark setting nor an accent colour. The HUD is \
             following the webview's own idea of light or dark instead."
                .to_string()
        }
        (Some(_), None) => {
            "Windows has no accent colour set, so AETHER1 is keeping the accent it was \
             designed with."
                .to_string()
        }
        (None, Some(_)) => {
            "Windows reported an accent colour but not a light/dark setting, so the shell is \
             following the webview instead."
                .to_string()
        }
        (Some(_), Some(_)) => String::new(),
    };
    DesktopTheme {
        mode,
        accent,
        source: "windows-registry".to_string(),
        note,
    }
}

// ----------------------------------------------------------------------------------------
// Linux
// ----------------------------------------------------------------------------------------

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn read_linux() -> DesktopTheme {
    // The portal first: it is the cross-desktop answer, and the only one of the three that
    // carries both the colour scheme and the accent.
    if let Ok(output) = std::process::Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.portal.Desktop",
            "--object-path",
            "/org/freedesktop/portal/desktop",
            "--method",
            "org.freedesktop.portal.Settings.ReadAll",
            "['org.freedesktop.appearance']",
        ])
        .output()
    {
        if output.status.success() {
            let answer = parse_xdg_portal(&String::from_utf8_lossy(&output.stdout));
            if answer.mode.is_some() || answer.accent.is_some() {
                return answer;
            }
        }
    }

    // GNOME's own keys. `color-scheme` has been there since 42; `accent-color` only since
    // 47, and it is a name rather than a colour.
    let scheme = gsettings("org.gnome.desktop.interface", "color-scheme");
    let accent_name = gsettings("org.gnome.desktop.interface", "accent-color");
    if scheme.is_some() || accent_name.is_some() {
        let answer = parse_gsettings(scheme.as_deref(), accent_name.as_deref());
        if answer.mode.is_some() || answer.accent.is_some() {
            return answer;
        }
    }

    // KDE writes both into one file, so there is nothing to run.
    if let Some(home) = std::env::var_os("HOME") {
        let path = std::path::Path::new(&home).join(".config/kdeglobals");
        if let Ok(text) = std::fs::read_to_string(&path) {
            let answer = parse_kdeglobals(&text);
            if answer.mode.is_some() || answer.accent.is_some() {
                return answer;
            }
        }
    }

    DesktopTheme::unavailable(
        "No desktop appearance setting could be read: the XDG appearance portal did not \
         answer, and neither gsettings nor ~/.config/kdeglobals carried one. Install \
         xdg-desktop-portal for your desktop, or pick a theme in Settings > Appearance.",
    )
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn gsettings(schema: &str, key: &str) -> Option<String> {
    let output = std::process::Command::new("gsettings")
        .args(["get", schema, key])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// The portal's reply is GVariant text, not JSON:
///
/// ```text
/// ({'org.freedesktop.appearance': {'color-scheme': <uint32 1>, 'accent-color': <(0.2, 0.4, 0.9)>}},)
/// ```
///
/// Read with two patterns rather than a GVariant parser: this is one shape, from one method,
/// and a dependency that can parse every GVariant in order to read two values out of this one
/// would be the larger thing to be wrong about.
///
/// `color-scheme` is 0 (no preference), 1 (dark) or 2 (light) -- note that 1 is *dark*, which
/// is the opposite of the order anyone guesses. No preference is `None`, not light: it means
/// the desktop has no opinion, and the webview's own answer is as good as ours.
pub(crate) fn parse_xdg_portal(text: &str) -> DesktopTheme {
    let mut mode = None;
    if let Some(found) = after(text, "'color-scheme':") {
        let digits: String = found
            .trim_start()
            .trim_start_matches('<')
            .trim_start()
            .trim_start_matches("uint32")
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        mode = match digits.as_str() {
            "1" => Some("dark".to_string()),
            "2" => Some("light".to_string()),
            _ => None,
        };
    }

    let mut accent = None;
    if let Some(found) = after(text, "'accent-color':") {
        // `<(0.2, 0.4, 0.9)>`, and `(-1.0, -1.0, -1.0)` for "no accent set".
        if let Some(open) = found.find('(') {
            if let Some(close) = found[open..].find(')') {
                let parts: Vec<f64> = found[open + 1..open + close]
                    .split(',')
                    .filter_map(|p| p.trim().parse::<f64>().ok())
                    .collect();
                if parts.len() == 3 {
                    let channels: Vec<u8> =
                        parts.iter().filter_map(|v| channel_from_unit(*v)).collect();
                    if channels.len() == 3 {
                        accent = Some(hex(channels[0], channels[1], channels[2]));
                    }
                }
            }
        }
    }

    let note = if mode.is_none() && accent.is_some() {
        "The desktop publishes an accent colour but no light or dark preference, so the shell \
         is following the webview instead."
            .to_string()
    } else if accent.is_none() && mode.is_some() {
        "The desktop publishes no accent colour, so AETHER1 is keeping the accent it was \
         designed with."
            .to_string()
    } else {
        String::new()
    };

    DesktopTheme {
        mode,
        accent,
        source: "xdg-portal".to_string(),
        note,
    }
}

/// Everything after the first occurrence of `needle`, or `None`.
fn after<'a>(text: &'a str, needle: &str) -> Option<&'a str> {
    text.find(needle).map(|at| &text[at + needle.len()..])
}

/// The nine accents libadwaita defines, which is what GNOME 47's `accent-color` key names.
///
/// The hexes are libadwaita's own light-theme values. A name that is not one of these is
/// treated as no accent rather than guessed at: GNOME may add a tenth, and a wrong colour is
/// worse than the designed one.
fn gnome_accent_hex(name: &str) -> Option<&'static str> {
    Some(match name {
        "blue" => "#3584e4",
        "teal" => "#2190a4",
        "green" => "#3a944a",
        "yellow" => "#c88800",
        "orange" => "#ed5b00",
        "red" => "#e62d42",
        "pink" => "#d56199",
        "purple" => "#9141ac",
        "slate" => "#6f8396",
        _ => return None,
    })
}

/// `gsettings get` quotes its strings and prints enums bare, so both arrive with stray
/// quotes worth stripping before anything is matched on.
pub(crate) fn parse_gsettings(scheme: Option<&str>, accent: Option<&str>) -> DesktopTheme {
    let unquote = |s: &str| s.trim().trim_matches('\'').trim_matches('"').to_string();
    let mode = scheme.map(unquote).and_then(|s| match s.as_str() {
        "prefer-dark" => Some("dark".to_string()),
        "prefer-light" => Some("light".to_string()),
        // "default" is GNOME's way of saying the user has not chosen, which is not light.
        _ => None,
    });
    let accent = accent
        .map(unquote)
        .and_then(|name| gnome_accent_hex(&name).map(|h| h.to_string()));
    let note = if mode.is_none() && accent.is_none() {
        "GNOME reported no light/dark preference and no accent colour.".to_string()
    } else {
        String::new()
    };
    DesktopTheme {
        mode,
        accent,
        source: "gsettings".to_string(),
        note,
    }
}

/// KDE's `~/.config/kdeglobals`.
///
/// `[General] AccentColor` is the colour somebody picked, as `r,g,b`, and it is absent when
/// the accent is coming from the colour scheme instead -- in which case
/// `[Colors:Selection] BackgroundNormal` is the same colour under another name. The scheme's
/// own name is the only light/dark signal in the file, so it is matched on the substring
/// "dark", which is how every KDE dark scheme is named (BreezeDark, CatppuccinMacchiatoDark).
pub(crate) fn parse_kdeglobals(text: &str) -> DesktopTheme {
    let mut section = String::new();
    let mut accent_general = None;
    let mut selection_background = None;
    let mut scheme_name = None;

    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match (section.as_str(), key) {
            ("General", "AccentColor") => accent_general = parse_kde_rgb(value),
            ("General", "ColorScheme") => scheme_name = Some(value.to_string()),
            ("Colors:Selection", "BackgroundNormal") => selection_background = parse_kde_rgb(value),
            _ => {}
        }
    }

    let mode = scheme_name.as_deref().map(|name| {
        if name.to_ascii_lowercase().contains("dark") {
            "dark".to_string()
        } else {
            "light".to_string()
        }
    });
    let accent = accent_general.or(selection_background);
    let note = if accent.is_none() {
        "No accent colour in ~/.config/kdeglobals, so AETHER1 is keeping the accent it was \
         designed with."
            .to_string()
    } else {
        String::new()
    };
    DesktopTheme {
        mode,
        accent,
        source: "kdeglobals".to_string(),
        note,
    }
}

/// `61,174,233`, and sometimes `61,174,233,255` with an alpha nobody here needs.
fn parse_kde_rgb(value: &str) -> Option<String> {
    let parts: Vec<u8> = value
        .split(',')
        .filter_map(|p| p.trim().parse::<u8>().ok())
        .collect();
    if parts.len() < 3 {
        return None;
    }
    Some(hex(parts[0], parts[1], parts[2]))
}

// ----------------------------------------------------------------------------------------
// macOS
// ----------------------------------------------------------------------------------------

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn read_macos() -> DesktopTheme {
    let interface_style = defaults_read("AppleInterfaceStyle");
    let accent = defaults_read("AppleAccentColor");
    let highlight = defaults_read("AppleHighlightColor");
    parse_macos(
        interface_style.as_deref(),
        accent.as_deref(),
        highlight.as_deref(),
    )
}

/// `defaults read -g <key>`, where a missing key is an error exit rather than empty output.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn defaults_read(key: &str) -> Option<String> {
    let output = std::process::Command::new("defaults")
        .args(["read", "-g", key])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// The eight swatches in System Settings > Appearance, as `AppleAccentColor` numbers them.
///
/// The key is *absent* when the accent is "multicolour", which is the default and means blue
/// for a window's own controls -- so absence is handled by the caller as "no accent", and the
/// `AppleHighlightColor` fallback below covers it with the colour the system actually uses.
fn macos_accent_hex(value: i64) -> Option<&'static str> {
    Some(match value {
        -1 => "#8c8c8c", // graphite
        0 => "#ff5257",  // red
        1 => "#f7821b",  // orange
        2 => "#ffc600",  // yellow
        3 => "#62ba46",  // green
        4 => "#007aff",  // blue
        5 => "#953d96",  // purple
        6 => "#f74f9e",  // pink
        _ => return None,
    })
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn parse_macos(
    interface_style: Option<&str>,
    accent: Option<&str>,
    highlight: Option<&str>,
) -> DesktopTheme {
    // `AppleInterfaceStyle` is written only while dark is on, and removed rather than set to
    // "Light" when it is turned off. Absence is therefore light, not unknown -- the one
    // platform here where that is true.
    let mode = Some(match interface_style {
        Some(s) if s.eq_ignore_ascii_case("dark") => "dark".to_string(),
        _ => "light".to_string(),
    });

    let accent = accent
        .and_then(|s| s.trim().parse::<i64>().ok())
        .and_then(macos_accent_hex)
        .map(|h| h.to_string())
        .or_else(|| highlight.and_then(parse_macos_highlight));

    let note = if accent.is_none() {
        "macOS reported no accent colour, so AETHER1 is keeping the accent it was designed \
         with."
            .to_string()
    } else {
        String::new()
    };
    DesktopTheme {
        mode,
        accent,
        source: "macos-defaults".to_string(),
        note,
    }
}

/// `AppleHighlightColor` is three floats and then the swatch's name:
/// `0.698039 0.843137 1.000000 Blue`. Only the floats are wanted.
fn parse_macos_highlight(value: &str) -> Option<String> {
    let channels: Vec<u8> = value
        .split_whitespace()
        .filter_map(|p| p.parse::<f64>().ok())
        .filter_map(channel_from_unit)
        .collect();
    if channels.len() < 3 {
        return None;
    }
    Some(hex(channels[0], channels[1], channels[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Windows ----------------------------------------------------------------------

    /// The byte order is the whole test. `DWM\AccentColor` holds 0xAABBGGRR, so a Windows
    /// default blue of #0078d4 is stored as 0xd47800 with the alpha on top. Reading it as
    /// ARGB would hand back #78d4-something and nobody would notice until a blue desktop
    /// made the HUD orange.
    #[test]
    fn a_windows_accent_is_read_blue_end_first() {
        // ff d4 78 00 -> alpha ff, blue d4, green 78, red 00
        assert_eq!(accent_from_abgr(0xffd47800), "#0078d4");
        assert_eq!(accent_from_argb(0xff0078d4), "#0078d4");
    }

    #[test]
    fn windows_reports_its_mode_and_accent() {
        // PowerShell reads a REG_DWORD as a signed Int32, so an accent with the alpha byte
        // set arrives negative. That sign has to survive the widening to i64 and the cast
        // back, which is what these literals are really testing.
        let theme = parse_windows(
            r#"{"apps_use_light_theme":0,"accent_color":-2852864,"colorization_color":-16777216}"#,
        );
        assert_eq!(theme.mode.as_deref(), Some("dark"));
        // -2852864 as u32 is 0xffd4_7800: Windows' own default blue, blue end first.
        assert_eq!(theme.accent.as_deref(), Some("#0078d4"));
        assert_eq!(theme.source, "windows-registry");
        assert!(theme.note.is_empty());
    }

    #[test]
    fn windows_light_mode_is_one_not_zero() {
        let theme = parse_windows(r#"{"apps_use_light_theme":1}"#);
        assert_eq!(theme.mode.as_deref(), Some("light"));
        assert_eq!(theme.accent, None);
        assert!(theme.note.contains("no accent colour"));
    }

    /// The older key is used only when the newer one is absent, and it is in the other byte
    /// order, so a machine that falls back must not have its channels swapped.
    #[test]
    fn windows_falls_back_to_the_older_colorization_key() {
        let theme = parse_windows(r#"{"apps_use_light_theme":1,"colorization_color":-16746296}"#);
        // -16746296 as u32 is 0xff00_78c8 -- the same channels in the other order, which is
        // the point: decoded as ABGR it would come back #c87800.
        assert_eq!(theme.accent.as_deref(), Some("#0078c8"));
    }

    #[test]
    fn windows_saying_nothing_is_not_an_error() {
        for empty in ["", "null", "not json", "{}"] {
            let theme = parse_windows(empty);
            assert_eq!(theme.mode, None, "{empty:?}");
            assert_eq!(theme.accent, None, "{empty:?}");
            assert!(!theme.note.is_empty(), "{empty:?}");
        }
    }

    // ---- XDG portal -------------------------------------------------------------------

    /// 1 is dark and 2 is light, which is the opposite of what the numbers suggest. Getting
    /// this backwards is a one-character bug that opens the HUD white on a dark desktop.
    #[test]
    fn the_portals_colour_scheme_numbers_one_as_dark() {
        let dark =
            parse_xdg_portal("({'org.freedesktop.appearance': {'color-scheme': <uint32 1>}},)");
        assert_eq!(dark.mode.as_deref(), Some("dark"));
        let light =
            parse_xdg_portal("({'org.freedesktop.appearance': {'color-scheme': <uint32 2>}},)");
        assert_eq!(light.mode.as_deref(), Some("light"));
    }

    /// 0 means the desktop has no preference. That is not light: answering light would
    /// override the webview's own answer with a guess.
    #[test]
    fn no_preference_is_not_light() {
        let theme =
            parse_xdg_portal("({'org.freedesktop.appearance': {'color-scheme': <uint32 0>}},)");
        assert_eq!(theme.mode, None);
    }

    #[test]
    fn the_portal_accent_arrives_as_three_floats() {
        let theme = parse_xdg_portal(
            "({'org.freedesktop.appearance': {'color-scheme': <uint32 1>, \
             'accent-color': <(0.2078431372549, 0.51764705882353, 0.89411764705882)>}},)",
        );
        assert_eq!(theme.mode.as_deref(), Some("dark"));
        assert_eq!(theme.accent.as_deref(), Some("#3584e4"));
        assert!(theme.note.is_empty());
    }

    /// The portal's way of saying "no accent chosen" is a negative triple, not an absent
    /// key, and a naive clamp to 0 would paint the HUD black.
    #[test]
    fn a_negative_portal_accent_is_no_accent() {
        let theme = parse_xdg_portal(
            "({'org.freedesktop.appearance': {'color-scheme': <uint32 2>, \
             'accent-color': <(-1.0, -1.0, -1.0)>}},)",
        );
        assert_eq!(theme.mode.as_deref(), Some("light"));
        assert_eq!(theme.accent, None);
        assert!(theme.note.contains("no accent colour"));
    }

    #[test]
    fn a_portal_that_answers_nothing_useful_reports_nothing() {
        let theme = parse_xdg_portal("({'org.freedesktop.appearance': {}},)");
        assert_eq!(theme.mode, None);
        assert_eq!(theme.accent, None);
    }

    // ---- gsettings --------------------------------------------------------------------

    #[test]
    fn gnome_names_its_accent_rather_than_giving_a_colour() {
        let theme = parse_gsettings(Some("'prefer-dark'"), Some("'purple'"));
        assert_eq!(theme.mode.as_deref(), Some("dark"));
        assert_eq!(theme.accent.as_deref(), Some("#9141ac"));
        assert_eq!(theme.source, "gsettings");
    }

    /// GNOME's "default" means nobody has chosen, so it must not be read as light.
    #[test]
    fn gnomes_default_scheme_is_not_a_choice() {
        let theme = parse_gsettings(Some("'default'"), None);
        assert_eq!(theme.mode, None);
    }

    /// A GNOME release adding a tenth accent name must leave the designed colour alone
    /// rather than produce something invented here.
    #[test]
    fn an_unknown_gnome_accent_name_is_declined() {
        let theme = parse_gsettings(Some("'prefer-light'"), Some("'chartreuse'"));
        assert_eq!(theme.mode.as_deref(), Some("light"));
        assert_eq!(theme.accent, None);
    }

    // ---- kdeglobals -------------------------------------------------------------------

    #[test]
    fn kde_reports_its_accent_and_scheme() {
        let theme = parse_kdeglobals(
            "[General]\nColorScheme=BreezeDark\nAccentColor=61,174,233\n\n\
             [Colors:Selection]\nBackgroundNormal=100,100,100\n",
        );
        assert_eq!(theme.mode.as_deref(), Some("dark"));
        // The explicitly picked accent wins over the scheme's selection colour.
        assert_eq!(theme.accent.as_deref(), Some("#3daee9"));
        assert_eq!(theme.source, "kdeglobals");
    }

    /// With no accent picked, KDE's accent *is* the scheme's selection colour, and that is
    /// in a different section of the same file.
    #[test]
    fn kde_falls_back_to_the_schemes_selection_colour() {
        let theme = parse_kdeglobals(
            "[General]\nColorScheme=Breeze\n\n[Colors:Selection]\nBackgroundNormal=61,174,233\n",
        );
        assert_eq!(theme.mode.as_deref(), Some("light"));
        assert_eq!(theme.accent.as_deref(), Some("#3daee9"));
    }

    /// The same key name exists in several `[Colors:*]` sections; reading it outside
    /// `[Colors:Selection]` would pick up a panel background and call it the accent.
    #[test]
    fn kde_reads_the_selection_section_and_not_its_neighbours() {
        let theme = parse_kdeglobals(
            "[Colors:Window]\nBackgroundNormal=255,0,0\n\n\
             [Colors:Selection]\nBackgroundNormal=61,174,233\n",
        );
        assert_eq!(theme.accent.as_deref(), Some("#3daee9"));
    }

    #[test]
    fn kde_rgb_with_an_alpha_still_reads() {
        assert_eq!(parse_kde_rgb("61,174,233,255").as_deref(), Some("#3daee9"));
        assert_eq!(parse_kde_rgb("61,174"), None);
        assert_eq!(parse_kde_rgb("not,a,colour"), None);
    }

    // ---- macOS ------------------------------------------------------------------------

    /// macOS removes the key rather than writing "Light", so its absence is the light theme
    /// and not an unknown answer.
    #[test]
    fn a_missing_interface_style_is_light_on_macos() {
        assert_eq!(
            parse_macos(None, Some("4"), None).mode.as_deref(),
            Some("light")
        );
        assert_eq!(
            parse_macos(Some("Dark"), Some("4"), None).mode.as_deref(),
            Some("dark")
        );
    }

    #[test]
    fn a_macos_accent_number_becomes_its_swatch() {
        assert_eq!(
            parse_macos(None, Some("5"), None).accent.as_deref(),
            Some("#953d96")
        );
        assert_eq!(
            parse_macos(None, Some("-1"), None).accent.as_deref(),
            Some("#8c8c8c")
        );
    }

    /// The default "multicolour" accent writes no `AppleAccentColor` at all, so the
    /// highlight colour is what is left to read.
    #[test]
    fn macos_falls_back_to_the_highlight_colour() {
        let theme = parse_macos(Some("Dark"), None, Some("0.698039 0.843137 1.000000 Blue"));
        assert_eq!(theme.accent.as_deref(), Some("#b2d7ff"));
        assert!(theme.note.is_empty());
    }

    #[test]
    fn macos_with_nothing_to_read_keeps_the_designed_accent() {
        let theme = parse_macos(None, None, None);
        assert_eq!(theme.accent, None);
        assert!(theme.note.contains("no accent colour"));
    }
}
