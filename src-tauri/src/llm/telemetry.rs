// Cross-platform system telemetry, replacing the Linux-leaning bits of Python's
// backend/system_monitor.py (which shells out to /etc/os-release for distro detection)
// with the `sysinfo` crate, which already knows how to ask each OS for this the right
// way -- the same code path here runs on Linux and Windows.

use std::thread;
// Aliased: this crate already has a local `percent` variable (the disk-usage calculation
// below), and uom's unit marker types double as values, so the bare name collides with it.
use starship_battery::units::ratio::percent as ratio_percent;
use sysinfo::{Disks, Networks, ProcessesToUpdate, System};

/// A laptop's battery, when one is present -- desktops report no batteries at all, which
/// `battery_status` below treats as `None` rather than an error.
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
    pub top_processes: Vec<(String, f32)>,
}

fn bytes_to_gb(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0 * 1024.0)
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

        let disks = Disks::new_with_refreshed_list();
        let (disk_used_gb, disk_total_gb, disk_percent) = disks
            .list()
            .first()
            .map(|d| {
                let total = d.total_space();
                let available = d.available_space();
                let used = total.saturating_sub(available);
                let percent = if total > 0 {
                    (used as f64 / total as f64) * 100.0
                } else {
                    0.0
                };
                (bytes_to_gb(used), bytes_to_gb(total), percent)
            })
            .unwrap_or((0.0, 0.0, 0.0));

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
            top_processes,
        }
    }

    /// Nested JSON shape matching system_monitor.py's get_telemetry() -- lets the frontend's
    /// existing updateHardwareTelemetry() handle both the Python websocket and this Tauri
    /// event with the same code, instead of needing a second, Rust-specific handler.
    pub fn to_wire_json(&self) -> serde_json::Value {
        serde_json::json!({
            "cpu": { "total_percent": self.cpu_percent },
            "ram": {
                "percent": self.ram_percent,
                "used_gb": self.ram_used_gb,
                "total_gb": self.ram_total_gb,
            },
            "disk": {
                "percent": self.disk_percent,
                "used_gb": self.disk_used_gb,
                "total_gb": self.disk_total_gb,
            },
            "network": {
                "download_kbps": self.network_download_kbps,
                "upload_kbps": self.network_upload_kbps,
            },
            // null on a desktop -- the frontend hides the battery row entirely for that,
            // rather than showing a permanent, meaningless 0%.
            "battery": self.battery.as_ref().map(|b| serde_json::json!({
                "percent": b.percent,
                "state": b.state,
                "on_battery": b.on_battery,
            })),
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
