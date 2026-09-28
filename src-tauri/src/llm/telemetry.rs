// Cross-platform system telemetry, replacing the Linux-leaning bits of Python's
// backend/system_monitor.py (which shells out to /etc/os-release for distro detection)
// with the `sysinfo` crate, which already knows how to ask each OS for this the right
// way -- the same code path here runs on Linux and Windows.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
// Aliased: this crate already has a local `percent` variable (the disk-usage calculation
// below), and uom's unit marker types double as values, so the bare name collides with it.
use starship_battery::units::ratio::percent as ratio_percent;
use sysinfo::{Disks, Networks, ProcessesToUpdate, System};

/// A laptop's battery, when one is present -- desktops report no batteries at all, which
/// `battery_status` below treats as `None` rather than an error.
#[derive(Default)]
pub struct BatteryInfo {
    pub percent: f32,
    /// "charging" | "discharging" | "full" | "empty" | "unknown", matching
    /// starship_battery::State's Display impl (kept as a string so the frontend doesn't
    /// need to know about the Rust enum).
    pub state: String,
    /// True while running on battery power (i.e. actually discharging) -- what the HUD
    /// needs to decide whether to warn the operator the machine has gone unplugged.
    pub on_battery: bool,
}

/// Reads the first battery this machine reports, if any. A desktop with no battery at all
/// is the common case, not an error, so this returns `None` for it the same as it does for
/// a genuine read failure (no OS API most laptops need is going to be flaky in a way that's
/// worth surfacing as distinct from "no battery" -- either way there's nothing to show).
fn battery_status() -> Option<BatteryInfo> {
    let manager = starship_battery::Manager::new().ok()?;
    let battery = manager.batteries().ok()?.next()?.ok()?;
    Some(BatteryInfo {
        percent: battery.state_of_charge().get::<ratio_percent>(),
        state: battery.state().to_string(),
        on_battery: battery.state() == starship_battery::State::Discharging,
    })
}

/// Default exists so a test can state the one or two readings it is about and leave the
/// rest at zero, rather than inventing a whole plausible machine each time.
#[derive(Default)]
pub struct Telemetry {
    pub os_name: String,
    pub architecture: String,
    pub cpu_cores_logical: usize,
    pub cpu_percent: f32,
    pub ram_used_gb: f64,
    pub ram_total_gb: f64,
    pub ram_percent: f32,
    pub disk_used_gb: f64,
    pub disk_total_gb: f64,
    pub disk_percent: f64,
    pub network_download_kbps: f64,
    pub network_upload_kbps: f64,
    pub uptime: String,
    pub status: &'static str,
    /// None on a desktop (or anywhere the OS reports no battery) rather than an error --
    /// see battery_status.
    pub battery: Option<BatteryInfo>,
    /// The graphics adapters on this machine. Empty where nothing could be identified -- a
    /// container, a headless server, a driver that publishes nothing -- which the monitor
    /// treats as "leave the row out", not as a fault.
    ///
    /// On a program whose job is running models this is the number that decides what it can
    /// run, and it was the one piece of the machine this report did not mention.
    pub gpus: Vec<crate::gpu::Gpu>,
    pub top_processes: Vec<(String, f32)>,
}

fn bytes_to_gb(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0 * 1024.0)
}

/// Rounds a reading to the number of decimals it is actually meaningful to. See
/// to_wire_json below for why this exists at all.
fn round_to(value: f64, decimals: u32) -> f64 {
    let factor = 10_f64.powi(decimals as i32);
    (value * factor).round() / factor
}

fn format_uptime(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    format!("{hours:02}h {minutes:02}m {secs:02}s")
}

impl Telemetry {
    /// Takes a real reading of the host. CPU usage needs two samples spaced apart to be
    /// meaningful (sysinfo returns 0% on a single reading right after construction), so
    /// this blocks for MINIMUM_CPU_UPDATE_INTERVAL (~200ms) -- fine for an on-demand
    /// snapshot, but don't call this in a hot loop.
    pub fn snapshot() -> Telemetry {
        let mut sys = System::new_all();
        let mut networks = Networks::new_with_refreshed_list();
        thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        // received()/transmitted() are deltas since the *previous* refresh (not running
        // totals), so this second refresh -- after the sleep above -- gives real throughput
        // over that interval, the same way psutil-based system_monitor.py diffs two
        // net_io_counters() readings itself.
        networks.refresh();
        let interval_secs = sysinfo::MINIMUM_CPU_UPDATE_INTERVAL
            .as_secs_f64()
            .max(0.001);
        let (rx_bytes, tx_bytes) = networks.iter().fold((0u64, 0u64), |(rx, tx), (_, data)| {
            (rx + data.received(), tx + data.transmitted())
        });
        let network_download_kbps =
            (rx_bytes as f64 / interval_secs / 1024.0 * 10.0).round() / 10.0;
        let network_upload_kbps = (tx_bytes as f64 / interval_secs / 1024.0 * 10.0).round() / 10.0;
        // Per-process CPU usage needs the same two-samples-apart treatment as the global
        // number above -- without this second call, every process's cpu_usage() stays 0.0
        // from the single sample taken inside System::new_all(), making "top processes"
        // meaningless (sorted by a constant).
        sys.refresh_processes(ProcessesToUpdate::All, true);

        let os_name = System::long_os_version()
            .or_else(System::name)
            .unwrap_or_else(|| "Unknown OS".to_string());
        let architecture = std::env::consts::ARCH.to_string();

        let cpu_percent = sys.global_cpu_usage();
        let ram_used_gb = bytes_to_gb(sys.used_memory());
        let ram_total_gb = bytes_to_gb(sys.total_memory());
        let ram_percent = if sys.total_memory() > 0 {
            (sys.used_memory() as f32 / sys.total_memory() as f32) * 100.0
        } else {
            0.0
        };

        let (disk_used_gb, disk_total_gb, disk_percent) = disk_usage();

        let mut top_processes: Vec<(String, f32)> = sys
            .processes()
            .values()
            .map(|p| (p.name().to_string_lossy().to_string(), p.cpu_usage()))
            .collect();
        top_processes.sort_by(|a, b| b.1.total_cmp(&a.1));
        top_processes.truncate(3);

        let status = if cpu_percent < 85.0 && ram_percent < 90.0 {
            "NOMINAL"
        } else {
            "HIGH_LOAD"
        };

        Telemetry {
            os_name,
            architecture,
            cpu_cores_logical: sys.cpus().len(),
            cpu_percent,
            ram_used_gb,
            ram_total_gb,
            ram_percent,
            disk_used_gb,
            disk_total_gb,
            disk_percent,
            network_download_kbps,
            network_upload_kbps,
            uptime: format_uptime(System::uptime()),
            status,
            battery: battery_status(),
            // Read once and kept: see gpu::cached. A snapshot on a tick must not start a
            // process to re-learn something that cannot change.
            gpus: crate::gpu::cached().to_vec(),
            top_processes,
        }
    }

    /// Nested JSON shape matching system_monitor.py's get_telemetry() -- lets the frontend's
    /// existing updateHardwareTelemetry() handle both the Python websocket and this Tauri
    /// event with the same code, instead of needing a second, Rust-specific handler.
    ///
    /// Rounded here rather than in the HUD's JavaScript, to the same number of decimals
    /// diagnostic_report() prints below. A raw reading is a float with seventeen digits
    /// behind it, and the HUD put every one of them on screen: "RAM 63.74251937894533%"
    /// next to a CLI saying "63.7%" reads as two different machines, and the number is
    /// jittering through the last ten digits every second besides. Both answers now come
    /// from the same place, so they cannot disagree.
    pub fn to_wire_json(&self) -> serde_json::Value {
        serde_json::json!({
            // The core count is a constant, sent every tick with the load because the hub
            // prints it as part of "what this machine has" and a second probe for a number
            // that cannot change would be a process spawned to learn nothing.
            "cpu": {
                "total_percent": round_to(self.cpu_percent as f64, 1),
                "cores": self.cpu_cores_logical,
            },
            "ram": {
                "percent": round_to(self.ram_percent as f64, 1),
                "used_gb": round_to(self.ram_used_gb, 2),
                "total_gb": round_to(self.ram_total_gb, 2),
            },
            "disk": {
                "percent": round_to(self.disk_percent, 1),
                "used_gb": round_to(self.disk_used_gb, 1),
                "total_gb": round_to(self.disk_total_gb, 1),
            },
            "network": {
                "download_kbps": round_to(self.network_download_kbps, 1),
                "upload_kbps": round_to(self.network_upload_kbps, 1),
            },
            // null on a desktop -- the frontend hides the battery row entirely for that,
            // rather than showing a permanent, meaningless 0%.
            "battery": self.battery.as_ref().map(|b| serde_json::json!({
                "percent": round_to(b.percent as f64, 0),
                "state": b.state,
                "on_battery": b.on_battery,
            })),
            // An empty list rather than null: the frontend hides the row either way, and a
            // list it can always iterate is one fewer thing for it to get wrong.
            "gpus": self.gpus.iter().map(|g| serde_json::json!({
                "name": g.name,
                "summary": g.summary(),
                "vram_gb": g.vram_gb.map(|v| round_to(v, 1)),
                "integrated": g.integrated,
            })).collect::<Vec<_>>(),
        })
    }

    pub fn diagnostic_report(&self) -> String {
        let mut report = format!(
            "SYSTEM DIAGNOSTIC REPORT // {} [{}]\n\
             \u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\n\
             Status: {} | Uptime: {}\n\
             CPU Load: {:.1}% across {} cores\n\
             RAM Usage: {:.2} GB / {:.2} GB ({:.1}%)\n\
             Storage: {:.1} GB / {:.1} GB ({:.1}% used)\n\
             Network I/O: \u{2193} {:.1} KB/s | \u{2191} {:.1} KB/s\n",
            self.os_name.to_uppercase(),
            self.architecture,
            self.status,
            self.uptime,
            self.cpu_percent,
            self.cpu_cores_logical,
            self.ram_used_gb,
            self.ram_total_gb,
            self.ram_percent,
            self.disk_used_gb,
            self.disk_total_gb,
            self.disk_percent,
            self.network_download_kbps,
            self.network_upload_kbps,
        );

        if !self.gpus.is_empty() {
            report.push_str(&format!(
                "Graphics: {}\n",
                self.gpus
                    .iter()
                    .map(|gpu| gpu.summary())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        if let Some(battery) = &self.battery {
            report.push_str(&format!(
                "Battery: {:.0}% ({})\n",
                battery.percent, battery.state
            ));
        }

        let processes = self
            .top_processes
            .iter()
            .map(|(name, cpu)| format!("{name} ({cpu:.1}%)"))
            .collect::<Vec<_>>()
            .join(", ");
        report.push_str("Top Processes: ");
        report.push_str(&processes);
        report
    }
}

// ---- Disk usage -------------------------------------------------------------------------
//
// This is the one reading that cannot be taken on the caller's thread.
//
// `Disks::new_with_refreshed_list()` does not ask about a disk; it enumerates every mount
// on the machine and stats each one. On a desktop that is instant. On a machine with an
// automounted network share it is not: stat'ing the mount point is what *triggers* the
// automount, and if the server is gone the call sits in uninterruptible I/O until the
// automounter gives up -- fifteen seconds, by default, per attempt. Nothing in the process
// can interrupt it, because a thread in D state does not take signals.
//
// That is not a hypothetical. It is what a 1 Hz telemetry tick on a laptop with a dead NFS
// entry in /etc/fstab actually did: the tick's real cadence became ~20s, the hardware panel
// updated three times a minute instead of sixty, and a thread sat pinned in disk wait about
// three quarters of the time -- which the operator reads, correctly, as the app lagging.
// A sleeping USB drive or an unreachable SMB share does the same thing, so fixing the one
// mount would have been fixing the symptom.
//
// The rule this settles on: a telemetry reading is never worth blocking for. The figures are
// taken on a thread of their own and the tick takes whatever the last completed reading was.
// Disk usage moves by gigabytes an hour at worst, so a reading up to a minute old is not
// meaningfully different from a fresh one -- and a reading that never arrives because the
// filesystem is wedged is strictly better than a HUD that freezes waiting for it.

/// How stale a reading may be before another is started. Deliberately far longer than the
/// tick: the old code re-read this every second, which on the machine above meant triggering
/// a dead automount every second.
const DISK_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

struct DiskReading {
    used_gb: f64,
    total_gb: f64,
    percent: f64,
    taken_at: Option<Instant>,
}

fn disk_cache() -> &'static Mutex<DiskReading> {
    static CACHE: OnceLock<Mutex<DiskReading>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(DiskReading {
            used_gb: 0.0,
            total_gb: 0.0,
            percent: 0.0,
            taken_at: None,
        })
    })
}

/// Set while a refresh thread is alive. Without it, a wedged filesystem would have the tick
/// start a new thread every second, each one parking in the same place, and the count would
/// climb for as long as the mount stayed dead.
fn disk_refresh_running() -> &'static AtomicBool {
    static RUNNING: OnceLock<AtomicBool> = OnceLock::new();
    RUNNING.get_or_init(|| AtomicBool::new(false))
}

/// The last completed reading, and a refresh started if that reading is old. Never blocks:
/// before the first one finishes this reports zeroes, which is what the HUD already shows
/// for a machine that reports no disks at all.
fn disk_usage() -> (f64, f64, f64) {
    let (reading, stale) = {
        let cache = disk_cache().lock().unwrap_or_else(|e| e.into_inner());
        let stale = match cache.taken_at {
            None => true,
            Some(at) => at.elapsed() >= DISK_REFRESH_INTERVAL,
        };
        ((cache.used_gb, cache.total_gb, cache.percent), stale)
    };

    if stale
        && disk_refresh_running()
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    {
        thread::spawn(|| {
            let measured = measure_disk();
            if let Some((used_gb, total_gb, percent)) = measured {
                let mut cache = disk_cache().lock().unwrap_or_else(|e| e.into_inner());
                cache.used_gb = used_gb;
                cache.total_gb = total_gb;
                cache.percent = percent;
                cache.taken_at = Some(Instant::now());
            }
            // Stamped even when nothing was measured, so a machine that reports no disks
            // does not start a thread every tick forever.
            else {
                let mut cache = disk_cache().lock().unwrap_or_else(|e| e.into_inner());
                cache.taken_at = Some(Instant::now());
            }
            disk_refresh_running().store(false, Ordering::Release);
        });
    }

    reading
}

/// The blocking part, run only on the refresh thread. Picks the filesystem AETHER1 itself is
/// installed on rather than whichever one the OS happened to list first -- `.first()` was
/// arbitrary, and on a machine with several drives it could report a disk the operator has
/// nothing to do with. Falls back to the largest, which is the better guess than the first
/// when the executable's path cannot be matched to a mount.
fn measure_disk() -> Option<(f64, f64, f64)> {
    let disks = Disks::new_with_refreshed_list();
    let here = std::env::current_exe().ok();

    let chosen = here
        .as_ref()
        .and_then(|exe| {
            // The longest mount point that is a prefix of our own path: with / and /home
            // both matching, /home is the one the file is actually on.
            disks
                .list()
                .iter()
                .filter(|d| exe.starts_with(d.mount_point()))
                .max_by_key(|d| d.mount_point().as_os_str().len())
        })
        .or_else(|| disks.list().iter().max_by_key(|d| d.total_space()))?;

    let total = chosen.total_space();
    let used = total.saturating_sub(chosen.available_space());
    let percent = if total > 0 {
        (used as f64 / total as f64) * 100.0
    } else {
        0.0
    };
    Some((bytes_to_gb(used), bytes_to_gb(total), percent))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug this whole arrangement exists for. `disk_usage` used to be
    /// `Disks::new_with_refreshed_list()` taken inline on the telemetry tick, which stats
    /// every mount on the machine -- and stat'ing an automounted share whose server is gone
    /// blocks in uninterruptible I/O for about fifteen seconds, per attempt, once a second.
    ///
    /// There is no dead NFS server in a test runner, so this cannot reproduce the stall. What
    /// it can pin is the property that makes the stall survivable: the call returns to its
    /// caller promptly whatever the filesystem is doing, because the filesystem is somebody
    /// else's thread's problem. A generous bound -- the point is "did not wait on a mount",
    /// not a benchmark, and a loaded CI runner is allowed to be slow.
    #[test]
    fn disk_usage_returns_without_waiting_on_the_filesystem() {
        let started = Instant::now();
        let _ = disk_usage();
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "disk_usage blocked for {:?}; it must never wait on a mount",
            started.elapsed()
        );
    }

    /// A wedged mount means the refresh thread is parked for as long as the mount stays dead.
    /// If a stale cache started a fresh one on every tick, the parked threads would pile up at
    /// one a second for as long as the operator left the app open. The in-flight flag is what
    /// stops that, so repeated calls with nothing cached must stay cheap.
    #[test]
    fn repeated_calls_stay_cheap_while_a_refresh_is_in_flight() {
        let started = Instant::now();
        for _ in 0..200 {
            let _ = disk_usage();
        }
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "200 calls took {:?}",
            started.elapsed()
        );
    }

    /// `.first()` was arbitrary: on a machine with several drives the HUD could report one the
    /// operator has nothing to do with. Whatever is chosen now, it has to be internally
    /// consistent -- used never above total, percent agreeing with both.
    #[test]
    fn the_chosen_disk_reports_consistent_figures() {
        let Some((used_gb, total_gb, percent)) = measure_disk() else {
            return; // A machine reporting no disks at all is not a failure; see disk_usage.
        };
        assert!(used_gb <= total_gb, "used {used_gb} > total {total_gb}");
        assert!(
            (0.0..=100.0).contains(&percent),
            "percent out of range: {percent}"
        );
        if total_gb > 0.0 {
            let expected = used_gb / total_gb * 100.0;
            assert!(
                (percent - expected).abs() < 0.5,
                "percent {percent} disagrees with {used_gb}/{total_gb}"
            );
        }
    }

    /// The HUD and the CLI read the same numbers and must say the same thing about them.
    /// They did not: `aether1 status` printed "63.7%" while the HUD's panel showed the raw
    /// float behind it, all seventeen digits, re-jittering every second.
    #[test]
    fn wire_json_rounds_the_way_the_report_prints() {
        let telemetry = Telemetry {
            cpu_percent: 63.742_52,
            ram_used_gb: 7.111_111_1,
            ram_total_gb: 15.999_999,
            ram_percent: 44.444_44,
            disk_used_gb: 123.456_78,
            disk_total_gb: 500.987_65,
            disk_percent: 24.681_357,
            network_download_kbps: 1_024.555_5,
            network_upload_kbps: 0.049_9,
            ..Telemetry::default()
        };

        let wire = telemetry.to_wire_json();
        assert_eq!(wire["cpu"]["total_percent"], serde_json::json!(63.7));
        assert_eq!(wire["ram"]["percent"], serde_json::json!(44.4));
        assert_eq!(wire["ram"]["used_gb"], serde_json::json!(7.11));
        assert_eq!(wire["ram"]["total_gb"], serde_json::json!(16.0));
        assert_eq!(wire["disk"]["percent"], serde_json::json!(24.7));
        assert_eq!(wire["disk"]["used_gb"], serde_json::json!(123.5));
        assert_eq!(wire["disk"]["total_gb"], serde_json::json!(501.0));
        assert_eq!(wire["network"]["download_kbps"], serde_json::json!(1024.6));
        // Rounds to zero rather than disappearing: a quiet link reads "0.0 KB/s".
        assert_eq!(wire["network"]["upload_kbps"], serde_json::json!(0.0));
    }

    /// A desktop has no battery, and the HUD hides the row entirely for that -- which it
    /// can only do if this stays null rather than becoming a permanent, meaningless 0%.
    #[test]
    fn wire_json_keeps_a_missing_battery_null() {
        let wire = Telemetry::default().to_wire_json();
        assert!(wire["battery"].is_null());
    }

    #[test]
    fn wire_json_rounds_battery_to_whole_percent() {
        let telemetry = Telemetry {
            battery: Some(BatteryInfo {
                percent: 87.6,
                state: "Discharging".to_string(),
                on_battery: true,
            }),
            ..Telemetry::default()
        };
        let wire = telemetry.to_wire_json();
        assert_eq!(wire["battery"]["percent"], serde_json::json!(88.0));
        assert_eq!(wire["battery"]["on_battery"], serde_json::json!(true));
    }
}
