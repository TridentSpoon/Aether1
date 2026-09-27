//! What graphics hardware this machine has, and how much memory is on it.
//!
//! The hardware monitor reported CPU, RAM, disk, network and battery, and said nothing
//! about the GPU -- on a program whose whole job is running models, which is the one number
//! that decides what it can run. `code_setup` sized its catalogue against system RAM for
//! exactly that reason, and on a machine with a 16 GB card and 16 GB of system memory that
//! recommends something two sizes too small.
//!
//! There is no crate that answers this on all three platforms without pulling in a driver
//! binding, so this asks each platform the way that platform answers:
//!
//! - **Linux** reads sysfs (`/sys/class/drm/card*/device`), which is already there, needs no
//!   process, and covers AMD and Intel. NVIDIA's proprietary driver does not publish the
//!   memory size there, so `nvidia-smi` is asked as well when it exists.
//! - **Windows** reads the display adapter's registry key, because `Win32_VideoController`'s
//!   `AdapterRAM` is a 32-bit field and any card above 4 GB reports exactly 4 GB through it.
//! - **macOS** asks `system_profiler`. On Apple Silicon there is no separate video memory to
//!   report and the machine's RAM figure is already the right one.
//!
//! Every probe is split the way `setup.rs` splits its own: something that touches the
//! machine, and a pure function that reads what came back. The parsers below are what the
//! tests exercise, against recorded output and a sysfs tree built in a temp directory --
//! this is the half that can be wrong in a way nobody notices until somebody's card is
//! reported as 512 MB.

use serde::Serialize;

/// One graphics adapter.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Gpu {
    /// What to show a person. The marketing name where the machine gives one, the vendor
    /// otherwise -- never an empty string, because a blank row in the monitor reads as a
    /// fault rather than as a card that did not introduce itself.
    pub name: String,
    /// Dedicated video memory in gigabytes, when the machine says. `None` is not zero: it
    /// means nothing here reported a figure, and a monitor that prints 0 GB for that is
    /// lying about the hardware rather than admitting it did not find out.
    pub vram_gb: Option<f64>,
    /// Whether this shares system memory rather than having its own.
    ///
    /// It matters because it changes what the number above means. A dedicated card's memory
    /// is a separate budget a model has to fit inside; an integrated one is already counted
    /// in the RAM figure, so adding it to anything would be counting the same gigabytes
    /// twice.
    pub integrated: bool,
}

impl Gpu {
    /// The one-line form for a report or a HUD row.
    pub fn summary(&self) -> String {
        match (self.vram_gb, self.integrated) {
            (Some(vram), false) => format!("{} ({vram:.0} GB)", self.name),
            (_, true) => format!("{} (shares system memory)", self.name),
            (None, false) => self.name.clone(),
        }
    }
}

/// Video memory below this is taken to be a shared carve-out rather than a card's own.
///
/// An APU reports a few hundred megabytes of stolen system memory through the same sysfs
/// file a discrete card reports its real memory through, and nothing distinguishes the two
/// reliably across drivers. No discrete card worth sizing a model against has under two
/// gigabytes, and an integrated one essentially never has more, so the number itself is the
/// most honest signal available. It only decides whether the figure is *treated* as a
/// separate budget -- it never hides a card from the monitor.
const SHARED_MEMORY_CEILING_GB: f64 = 2.0;

/// The card a model should be sized against: the one with the most memory of its own.
///
/// Integrated adapters are deliberately skipped rather than counted: their memory is system
/// memory, which the caller already has, and adding it would count the same gigabytes twice.
pub fn dedicated(gpus: &[Gpu]) -> Option<&Gpu> {
    gpus.iter()
        .filter(|gpu| !gpu.integrated && gpu.vram_gb.is_some())
        .fold(None, |best: Option<&Gpu>, gpu| match best {
            Some(b) if b.vram_gb >= gpu.vram_gb => Some(b),
            _ => Some(gpu),
        })
}

fn bytes_to_gb(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0 * 1024.0)
}

/// Builds a `Gpu` from the parts a probe found, applying the shared-memory rule in one place
/// so every platform agrees about what "integrated" means.
fn gpu(name: String, vram_gb: Option<f64>, known_integrated: bool) -> Gpu {
    let integrated = known_integrated || vram_gb.is_none_or(|v| v < SHARED_MEMORY_CEILING_GB);
    Gpu {
        name,
        vram_gb,
        integrated,
    }
}

/// The adapters on this machine, found once and kept.
///
/// `Telemetry::snapshot` runs on a tick, and two of the three probes below start a process.
/// Graphics hardware does not change while the program is running -- a card is not hot
/// plugged into a desktop -- so paying for that on every tick would be spending a process
/// launch a second to re-learn a constant.
pub fn cached() -> &'static [Gpu] {
    static GPUS: std::sync::OnceLock<Vec<Gpu>> = std::sync::OnceLock::new();
    GPUS.get_or_init(detect)
}

/// Reads every adapter this machine reports.
///
/// Returns an empty list rather than an error when nothing can be found: a machine whose
/// graphics hardware cannot be identified -- a container, a headless server, a driver that
/// publishes nothing -- is a normal case, and the monitor simply leaves the row out.
pub fn detect() -> Vec<Gpu> {
    #[cfg(target_os = "linux")]
    {
        detect_linux(std::path::Path::new("/sys/class/drm"), pci_ids().as_deref())
    }
    #[cfg(target_os = "windows")]
    {
        detect_windows()
    }
    #[cfg(target_os = "macos")]
    {
        detect_macos()
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        Vec::new()
    }
}

// ----------------------------------------------------------------------------------------
// Linux
// ----------------------------------------------------------------------------------------

/// PCI vendor ids, as sysfs writes them.
const VENDOR_AMD: &str = "0x1002";
const VENDOR_NVIDIA: &str = "0x10de";
const VENDOR_INTEL: &str = "0x8086";

/// The system's PCI id database, which is what `lspci` reads to turn ids into names.
///
/// Shipped by `hwdata` (or `pciutils` on some distributions) and present on essentially any
/// desktop Linux. Read rather than bundled on purpose: a table compiled into this binary
/// would be a list of graphics cards that stops gaining new ones the day it is built, and
/// the machine already has one that its package manager keeps current. `None` when the file
/// is not installed, which costs nothing -- the vendor string still stands.
///
/// Only reached when a card publishes no `product_name`, and `gpu::cached` reads the
/// adapters once for the life of the process, so this is at most one file read at startup.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn pci_ids() -> Option<String> {
    ["/usr/share/hwdata/pci.ids", "/usr/share/misc/pci.ids"]
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
}

/// The model name a vendor and device id resolve to in the PCI id database.
///
/// The format is indentation-significant and has been for thirty years: a vendor at the
/// left margin, its devices one tab in, and each device's subsystems two tabs in. So the
/// parser has to respect depth rather than just search for the id -- a device id is only
/// four hex digits and collides freely with a subsystem id under some other vendor. It
/// takes the first device line under the right vendor and stops at the next vendor.
///
/// Ids arrive from sysfs as `0x1002` and are written in the file as `1002`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn name_from_pci_ids(db: &str, vendor: &str, device: &str) -> Option<String> {
    let vendor = vendor.trim_start_matches("0x").to_ascii_lowercase();
    let device = device.trim_start_matches("0x").to_ascii_lowercase();
    let mut in_vendor = false;

    for line in db.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        // A class section (`C 03  Display controller`) ends the vendor list entirely.
        if !line.starts_with('\t') {
            if in_vendor {
                return None;
            }
            in_vendor = line
                .split_once("  ")
                .is_some_and(|(id, _)| id.trim().eq_ignore_ascii_case(&vendor));
            continue;
        }
        if !in_vendor || line.starts_with("\t\t") {
            continue;
        }
        let entry = line.trim_start_matches('\t');
        if let Some((id, name)) = entry.split_once("  ") {
            if id.trim().eq_ignore_ascii_case(&device) {
                let name = name.trim();
                return (!name.is_empty()).then(|| name.to_string());
            }
        }
    }
    None
}

/// Walks the DRM tree and reports what each card says about itself.
///
/// Takes the root as an argument rather than reaching for `/sys` directly, which is what
/// lets `a_discrete_card_and_an_apu_are_told_apart` build both cases in a temp directory and
/// check them on a machine that has neither. `pci_ids` is the contents of the system PCI id
/// database, passed in for the same reason: a test that read the real file would pass or
/// fail depending on whether the machine running it happens to have `hwdata` installed.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn detect_linux(drm_root: &std::path::Path, pci_ids: Option<&str>) -> Vec<Gpu> {
    let nvidia = nvidia_smi_gpus();
    let mut found = Vec::new();

    let Ok(entries) = std::fs::read_dir(drm_root) else {
        return nvidia;
    };
    let mut cards: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                // `card0`, but not `card0-DP-1`, which is a connector on that card and not a
                // second card. Without this an ordinary desktop reports its GPU five times.
                .is_some_and(|n| n.starts_with("card") && !n.contains('-'))
        })
        .collect();
    cards.sort();

    for card in cards {
        let device = card.join("device");
        let read = |file: &str| {
            std::fs::read_to_string(device.join(file))
                .ok()
                .map(|s| s.trim().to_string())
        };
        let Some(vendor) = read("vendor") else {
            continue;
        };

        // The proprietary driver publishes no memory size in sysfs, so an NVIDIA card here
        // is reported from nvidia-smi if that answered, and skipped if it did not -- naming
        // a card whose memory is unknown adds nothing the monitor can use.
        if vendor == VENDOR_NVIDIA {
            continue;
        }

        let vram = read("mem_info_vram_total")
            .and_then(|s| s.parse::<u64>().ok())
            .map(bytes_to_gb);

        // Three sources, best first. `product_name` is the card introducing itself and is
        // what a marketing name would come from -- but amdgpu leaves it empty on plenty of
        // machines, which is how a 6800 XT came to be reported as "AMD graphics". The PCI
        // device id next to it is always there, and the system's own PCI id database turns
        // it into a model name. Only when neither answers does the vendor string stand, and
        // it stands as a last resort rather than as the usual outcome.
        let name = read("product_name")
            .filter(|s| !s.is_empty())
            .or_else(|| {
                let device = read("device")?;
                name_from_pci_ids(pci_ids?, &vendor, &device)
            })
            .unwrap_or_else(|| match vendor.as_str() {
                VENDOR_AMD => "AMD graphics".to_string(),
                VENDOR_INTEL => "Intel graphics".to_string(),
                other => format!("Graphics adapter ({other})"),
            });

        found.push(gpu(name, vram, false));
    }

    found.extend(nvidia);
    found
}

/// Asks `nvidia-smi`, which is the only thing that knows an NVIDIA card's memory on Linux.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn nvidia_smi_gpus() -> Vec<Gpu> {
    let Ok(output) = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    parse_nvidia_smi(&String::from_utf8_lossy(&output.stdout))
}

/// `nvidia-smi`'s CSV, one card per line: `NVIDIA GeForce GTX 1050 Ti, 4096`.
///
/// The memory figure is mebibytes because `--format` asked for it without units; reading it
/// as megabytes would under-report every card by about five percent, which is enough to move
/// a model across a size threshold.
pub(crate) fn parse_nvidia_smi(text: &str) -> Vec<Gpu> {
    text.lines()
        .filter_map(|line| {
            let (name, mib) = line.split_once(',')?;
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            let vram = mib
                .trim()
                .parse::<f64>()
                .ok()
                .map(|mib| mib * 1024.0 * 1024.0 / (1024.0 * 1024.0 * 1024.0));
            Some(gpu(name.to_string(), vram, false))
        })
        .collect()
}

// ----------------------------------------------------------------------------------------
// Windows
// ----------------------------------------------------------------------------------------

/// Reads the display adapters out of the registry, through PowerShell.
///
/// `Win32_VideoController.AdapterRAM` is the obvious source and is wrong: it is a 32-bit
/// field, so every card above four gigabytes reports exactly four. The driver's own key
/// carries `HardwareInformation.qwMemorySize`, which is 64-bit and right, and the WMI name
/// is joined to it for something readable.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn detect_windows() -> Vec<Gpu> {
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
$class = 'HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}'
Get-ChildItem $class | ForEach-Object {
  $p = Get-ItemProperty $_.PSPath
  if ($p.'DriverDesc') {
    [pscustomobject]@{ name = $p.'DriverDesc'; bytes = [int64]$p.'HardwareInformation.qwMemorySize' }
  }
} | ConvertTo-Json -Compress
"#;
    let Ok(output) = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
        .output()
    else {
        return Vec::new();
    };
    parse_windows_adapters(&String::from_utf8_lossy(&output.stdout))
}

/// `ConvertTo-Json` emits a bare object for one adapter and an array for several, which is
/// the usual way a script that works on a laptop breaks on a workstation. Both are accepted.
pub(crate) fn parse_windows_adapters(json: &str) -> Vec<Gpu> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json.trim()) else {
        return Vec::new();
    };
    let items = match &value {
        serde_json::Value::Array(items) => items.clone(),
        serde_json::Value::Object(_) => vec![value],
        _ => return Vec::new(),
    };
    items
        .iter()
        .filter_map(|item| {
            let name = item.get("name")?.as_str()?.trim();
            if name.is_empty() {
                return None;
            }
            // A missing or zero size is "the key did not say", not "this card has none".
            let vram = item
                .get("bytes")
                .and_then(serde_json::Value::as_u64)
                .filter(|bytes| *bytes > 0)
                .map(bytes_to_gb);
            Some(gpu(name.to_string(), vram, false))
        })
        .collect()
}

// ----------------------------------------------------------------------------------------
// macOS
// ----------------------------------------------------------------------------------------

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn detect_macos() -> Vec<Gpu> {
    let Ok(output) = std::process::Command::new("system_profiler")
        .args(["SPDisplaysDataType", "-json"])
        .output()
    else {
        return Vec::new();
    };
    parse_macos_displays(&String::from_utf8_lossy(&output.stdout))
}

/// `system_profiler`'s display section. An Intel Mac with a discrete card reports
/// `sppci_vram` as a string with its unit in it ("8 GB"); Apple Silicon reports no VRAM key
/// at all, because there is none -- the memory is the machine's, and the RAM figure the
/// monitor already shows is the honest one.
pub(crate) fn parse_macos_displays(json: &str) -> Vec<Gpu> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json.trim()) else {
        return Vec::new();
    };
    let Some(items) = value.get("SPDisplaysDataType").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let name = item
                .get("sppci_model")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())?;
            let vram = item
                .get("sppci_vram")
                .or_else(|| item.get("spdisplays_vram_shared"))
                .and_then(|v| v.as_str())
                .and_then(parse_size_with_unit);
            // Apple Silicon names its GPU after the chip and shares the machine's memory.
            let unified =
                item.get("sppci_vram").is_none() && item.get("spdisplays_vram_shared").is_none();
            Some(gpu(name.to_string(), vram, unified))
        })
        .collect()
}

/// "8 GB", "1536 MB" -> gigabytes. Anything else is not a size and is treated as absent.
fn parse_size_with_unit(text: &str) -> Option<f64> {
    let text = text.trim();
    let (number, unit) = text.split_once(' ')?;
    let number: f64 = number.trim().parse().ok()?;
    match unit.trim().to_ascii_uppercase().as_str() {
        "GB" => Some(number),
        "MB" => Some(number / 1024.0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A fake `/sys/class/drm`, so the Linux probe can be exercised on a machine with no
    /// graphics hardware of its own -- which is every machine this is tested on. Built under
    /// the system temp directory the way the rest of this crate's tests do, and removed on
    /// drop so a failing test does not leave a tree behind for the next run to read.
    struct DrmTree(std::path::PathBuf);

    impl DrmTree {
        fn new(cards: &[(&str, &[(&str, &str)])]) -> DrmTree {
            static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let root =
                std::env::temp_dir().join(format!("aether1_drm_{}_{unique}", std::process::id()));
            for (card, files) in cards {
                let device = root.join(card).join("device");
                fs::create_dir_all(&device).unwrap();
                for (name, contents) in *files {
                    fs::write(device.join(name), contents).unwrap();
                }
            }
            DrmTree(root)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for DrmTree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn drm_tree(cards: &[(&str, &[(&str, &str)])]) -> DrmTree {
        DrmTree::new(cards)
    }

    /// The case this whole module exists for: a card with its own memory has a budget a
    /// model must fit inside, and an APU's carve-out is system memory already counted
    /// elsewhere. Reading the second as the first is how a machine gets told it has 16.5 GB
    /// of video memory when it has 16.
    #[test]
    fn a_discrete_card_and_an_apu_are_told_apart() {
        let tree = drm_tree(&[
            (
                "card0",
                &[
                    ("vendor", "0x1002\n"),
                    // 16 GiB, as an RX 6800 XT reports it.
                    ("mem_info_vram_total", "17179869184\n"),
                ],
            ),
            (
                "card1",
                &[
                    ("vendor", "0x1002\n"),
                    // Half a gigabyte of stolen system memory, as an APU reports it.
                    ("mem_info_vram_total", "536870912\n"),
                ],
            ),
        ]);
        let gpus = detect_linux(tree.path(), None);
        assert_eq!(gpus.len(), 2);
        assert_eq!(gpus[0].vram_gb, Some(16.0));
        assert!(!gpus[0].integrated);
        assert!(gpus[1].integrated, "a 512 MB carve-out is not a card's own");

        // And only the real one is offered as a budget to fit a model inside.
        assert_eq!(dedicated(&gpus).and_then(|gpu| gpu.vram_gb), Some(16.0));
    }

    /// A connector is not a card. Without the filter an ordinary desktop reports its GPU
    /// once per display output it has.
    #[test]
    fn display_connectors_are_not_counted_as_cards() {
        let tree = drm_tree(&[
            (
                "card0",
                &[("vendor", "0x1002"), ("mem_info_vram_total", "17179869184")],
            ),
            ("card0-DP-1", &[("vendor", "0x1002")]),
            ("card0-HDMI-A-1", &[("vendor", "0x1002")]),
        ]);
        assert_eq!(detect_linux(tree.path(), None).len(), 1);
    }

    /// Intel's integrated graphics publish no memory file at all. That is not zero, and the
    /// monitor still names the adapter -- it simply has no separate budget to report.
    #[test]
    fn an_adapter_with_no_memory_file_is_named_and_marked_shared() {
        let tree = drm_tree(&[("card0", &[("vendor", "0x8086")])]);
        let gpus = detect_linux(tree.path(), None);
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].name, "Intel graphics");
        assert_eq!(gpus[0].vram_gb, None);
        assert!(gpus[0].integrated);
        assert_eq!(dedicated(&gpus).and_then(|gpu| gpu.vram_gb), None);
    }

    /// A slice of the real pci.ids, with the shape that matters: tab-indented devices under
    /// a vendor, two-tab subsystems under those, and a second vendor after.
    const PCI_IDS: &str = "\
# comment at the left margin
1002  Advanced Micro Devices, Inc. [AMD/ATI]
\t73bf  Navi 21 [Radeon RX 6800/6800 XT / 6900 XT]
\t\t1002 0e3a  Radeon RX 6900 XT
\t73ff  Navi 23 [Radeon RX 6600/6600 XT/6600M]
8086  Intel Corporation
\t73bf  A device that merely shares an id with the card above
C 03  Display controller
\t00  VGA compatible controller
";

    /// The bug Trident caught: amdgpu leaves `product_name` empty on plenty of machines, and
    /// a 16 GB 6800 XT was reported as "AMD graphics". The device id beside it is always
    /// there, and the system's own database knows what it is.
    #[test]
    fn a_card_with_no_product_name_is_named_from_its_pci_id() {
        let tree = drm_tree(&[(
            "card0",
            &[
                ("vendor", "0x1002"),
                ("device", "0x73bf"),
                ("mem_info_vram_total", "17179869184"),
            ],
        )]);
        let gpus = detect_linux(tree.path(), Some(PCI_IDS));
        assert_eq!(gpus[0].name, "Navi 21 [Radeon RX 6800/6800 XT / 6900 XT]");
        assert_eq!(gpus[0].vram_gb, Some(16.0));
    }

    /// The card's own name outranks the database when it has one -- a marketing name from
    /// the driver reads better than a codename from a table.
    #[test]
    fn product_name_wins_over_the_pci_id_database() {
        let tree = drm_tree(&[(
            "card0",
            &[
                ("vendor", "0x1002"),
                ("device", "0x73bf"),
                ("product_name", "Radeon RX 6800 XT"),
            ],
        )]);
        assert_eq!(
            detect_linux(tree.path(), Some(PCI_IDS))[0].name,
            "Radeon RX 6800 XT"
        );
    }

    /// Without the vendor above it a device id is four hex digits that collide freely. This
    /// same id sits under two vendors in the fixture, and under a subsystem line as well.
    #[test]
    fn a_device_id_is_only_read_under_its_own_vendor() {
        assert_eq!(
            name_from_pci_ids(PCI_IDS, "0x8086", "0x73bf").as_deref(),
            Some("A device that merely shares an id with the card above")
        );
        assert_eq!(name_from_pci_ids(PCI_IDS, "0x1002", "0x0e3a"), None);
        assert_eq!(name_from_pci_ids(PCI_IDS, "0x10de", "0x73bf"), None);
    }

    /// The database is not installed everywhere, and a machine without it must still get a
    /// row -- the vendor string, exactly as before this lookup existed.
    #[test]
    fn no_database_leaves_the_vendor_string_standing() {
        let tree = drm_tree(&[("card0", &[("vendor", "0x1002"), ("device", "0x73bf")])]);
        assert_eq!(detect_linux(tree.path(), None)[0].name, "AMD graphics");
        assert_eq!(name_from_pci_ids(PCI_IDS, "0x1002", "0xffff"), None);
    }

    /// sysfs has nothing to say about an NVIDIA card under the proprietary driver, so
    /// listing it from there would produce a nameless row with no memory. nvidia-smi is the
    /// only source, and when it is absent the card is left out rather than half-reported.
    #[test]
    fn an_nvidia_card_is_not_reported_from_sysfs() {
        let tree = drm_tree(&[("card0", &[("vendor", "0x10de")])]);
        assert!(detect_linux(tree.path(), None).is_empty());
    }

    #[test]
    fn nvidia_smi_output_is_read_as_mebibytes() {
        // What a GTX 1050 Ti Mobile prints.
        let gpus = parse_nvidia_smi("NVIDIA GeForce GTX 1050 Ti, 4096\n");
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].name, "NVIDIA GeForce GTX 1050 Ti");
        assert_eq!(gpus[0].vram_gb, Some(4.0));
        assert!(!gpus[0].integrated, "4 GB is a card's own memory");
    }

    #[test]
    fn two_nvidia_cards_are_both_read() {
        let gpus =
            parse_nvidia_smi("NVIDIA GeForce RTX 4090, 24564\nNVIDIA GeForce GTX 1080 Ti, 11264\n");
        assert_eq!(gpus.len(), 2);
        assert_eq!(
            dedicated(&gpus)
                .and_then(|gpu| gpu.vram_gb)
                .map(|v| v.round()),
            Some(24.0)
        );
    }

    /// The 32-bit field this deliberately avoids: a 16 GB card through `AdapterRAM` reads as
    /// 4294967295. The registry value is 64-bit, and this is the test that the right one is
    /// being read.
    #[test]
    fn a_windows_adapter_reports_its_real_memory() {
        let gpus =
            parse_windows_adapters(r#"{"name":"AMD Radeon RX 6800 XT","bytes":17179869184}"#);
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].vram_gb, Some(16.0));
    }

    /// ConvertTo-Json emits an object for one and an array for several. A parser that only
    /// handles the array works on the workstation and reports nothing on the laptop.
    #[test]
    fn windows_reports_one_adapter_and_several_the_same_way() {
        let one = parse_windows_adapters(r#"{"name":"Intel UHD Graphics","bytes":0}"#);
        assert_eq!(one.len(), 1);
        assert_eq!(
            one[0].vram_gb, None,
            "a zero size is 'did not say', not 'none'"
        );
        assert!(one[0].integrated);

        let several = parse_windows_adapters(
            r#"[{"name":"Intel UHD Graphics","bytes":0},{"name":"NVIDIA GeForce GTX 1050 Ti","bytes":4294967296}]"#,
        );
        assert_eq!(several.len(), 2);
        assert_eq!(dedicated(&several).and_then(|gpu| gpu.vram_gb), Some(4.0));
    }

    #[test]
    fn nothing_parseable_reports_nothing_rather_than_failing() {
        assert!(parse_windows_adapters("").is_empty());
        assert!(parse_windows_adapters("not json").is_empty());
        assert!(parse_macos_displays("").is_empty());
        assert!(parse_nvidia_smi("").is_empty());
    }

    #[test]
    fn a_mac_with_a_discrete_card_reports_its_memory() {
        let gpus = parse_macos_displays(
            r#"{"SPDisplaysDataType":[{"sppci_model":"AMD Radeon Pro 5500M","sppci_vram":"8 GB"}]}"#,
        );
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].vram_gb, Some(8.0));
        assert!(!gpus[0].integrated);
    }

    /// Apple Silicon has no separate video memory, and the RAM figure the monitor already
    /// shows is the honest one. Reporting it as a card with unknown memory would invite
    /// sizing a model against a number that does not exist.
    #[test]
    fn apple_silicon_is_reported_as_sharing_memory() {
        let gpus =
            parse_macos_displays(r#"{"SPDisplaysDataType":[{"sppci_model":"Apple M2 Pro"}]}"#);
        assert_eq!(gpus.len(), 1);
        assert!(gpus[0].integrated);
        assert_eq!(dedicated(&gpus).and_then(|gpu| gpu.vram_gb), None);
        assert_eq!(gpus[0].summary(), "Apple M2 Pro (shares system memory)");
    }

    #[test]
    fn the_summary_says_what_is_known_and_no_more() {
        assert_eq!(
            gpu("AMD Radeon RX 6800 XT".into(), Some(16.0), false).summary(),
            "AMD Radeon RX 6800 XT (16 GB)"
        );
        assert_eq!(
            gpu("Intel graphics".into(), None, false).summary(),
            "Intel graphics (shares system memory)"
        );
    }
}
