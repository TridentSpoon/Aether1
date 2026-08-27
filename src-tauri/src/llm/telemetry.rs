// Cross-platform system telemetry, replacing the Linux-leaning bits of Python's
// backend/system_monitor.py (which shells out to /etc/os-release for distro detection)
// with the `sysinfo` crate, which already knows how to ask each OS for this the right
// way -- the same code path here runs on Linux and Windows.

use std::thread;
use sysinfo::{Disks, ProcessesToUpdate, System};

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
    pub uptime: String,
    pub status: &'static str,
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
        thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        sys.refresh_cpu_usage();
        sys.refresh_memory();
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
            uptime: format_uptime(System::uptime()),
            status,
            top_processes,
        }
    }

    pub fn diagnostic_report(&self) -> String {
        let mut report = format!(
            "SYSTEM DIAGNOSTIC REPORT // {} [{}]\n\
             \u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\n\
             Status: {} | Uptime: {}\n\
             CPU Load: {:.1}% across {} cores\n\
             RAM Usage: {:.2} GB / {:.2} GB ({:.1}%)\n\
             Storage: {:.1} GB / {:.1} GB ({:.1}% used)\n",
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
        );

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
