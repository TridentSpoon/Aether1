// Cross-platform system telemetry, replacing the Linux-leaning bits of Python's
// backend/system_monitor.py (which shells out to /etc/os-release for distro detection)
// with the `sysinfo` crate, which already knows how to ask each OS for this the right
// way -- the same code path here runs on Linux and Windows.

use std::sync::{Condvar, Mutex};
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

/// Reads the first battery this machine reports, if any -- handing back the manager
/// alongside it, because refreshing a reading later needs both.
///
/// A desktop with no battery at all is the common case, not an error, so this returns
/// `None` for it the same as it does for a genuine read failure (no OS API most laptops
/// need is going to be flaky in a way that's worth surfacing as distinct from "no
/// battery" -- either way there's nothing to show).
fn first_battery() -> Option<(starship_battery::Manager, starship_battery::Battery)> {
    let manager = starship_battery::Manager::new().ok()?;
    let battery = manager.batteries().ok()?.next()?.ok()?;
    Some((manager, battery))
}

fn battery_info(battery: &starship_battery::Battery) -> BatteryInfo {
    BatteryInfo {
        percent: battery.state_of_charge().get::<ratio_percent>(),
        state: battery.state().to_string(),
        on_battery: battery.state() == starship_battery::State::Discharging,
    }
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
    /// see first_battery.
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

/// A reading of the host that can be taken again cheaply.
///
/// This exists because the loop that feeds the HUD's telemetry panel used to call
/// `System::new_all()` once a second. That constructor is not a reading -- it is a whole
/// new model of the machine: every process, every disk, every network interface,
/// enumerated from scratch and thrown away a second later. On top of that it blocked for
/// ~200ms so CPU usage had two samples to sit between, walked every process a *second*
/// time to get per-process usage, built a fresh `Disks` and `Networks` list, opened a new
/// battery manager, and re-read the operating system's name and the CPU core count --
/// neither of which can change while the program is running.
///
/// Held across ticks instead, the same numbers cost one refresh each. The previous tick is
/// the earlier of the two CPU samples, so the blocking sleep is gone from every reading
/// but the first; the delta-based readings (CPU, network throughput) get a full second of
/// spacing rather than 200ms, which makes them *more* accurate, not less; and the
/// constants are read once, in `new`.
pub struct Sampler {
    sys: System,
    networks: Networks,
    disks: Disks,
    /// The manager is kept with the battery because refreshing a reading needs it. `None`
    /// on a machine that reported no battery when the sampler was built -- a desktop does
    /// not grow one mid-session.
    battery: Option<(starship_battery::Manager, starship_battery::Battery)>,
    /// Read once: none of these can change under a running process.
    os_name: String,
    architecture: String,
    cpu_cores_logical: usize,
    /// When the delta-based readings were last taken, so network throughput is divided by
    /// the interval that actually elapsed rather than an assumed one.
    last: Instant,
    /// False until the first `sample`, which is the only one that has to wait for a second
    /// CPU sample of its own.
    primed: bool,
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Sampler {
    pub fn new() -> Sampler {
        let sys = System::new_all();
        Sampler {
            os_name: System::long_os_version()
                .or_else(System::name)
                .unwrap_or_else(|| "Unknown OS".to_string()),
            architecture: std::env::consts::ARCH.to_string(),
            cpu_cores_logical: sys.cpus().len(),
            sys,
            networks: Networks::new_with_refreshed_list(),
            disks: Disks::new_with_refreshed_list(),
            battery: first_battery(),
            last: Instant::now(),
            primed: false,
        }
    }

    /// Takes a reading of the host. Cheap to call on a timer; see the note on `Sampler`
    /// for what that cost used to be.
    pub fn sample(&mut self) -> Telemetry {
        // The first reading is the one exception. CPU usage is a delta between two
        // samples, `new` took the first one, and a caller that asked for a reading
        // immediately would otherwise be told 0%. Every later reading has the previous
        // tick to measure against and waits for nothing.
        if !self.primed {
            thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
            self.primed = true;
        }
        let interval_secs = self.last.elapsed().as_secs_f64().max(0.001);
        self.last = Instant::now();

        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        // received()/transmitted() are deltas since the *previous* refresh (not running
        // totals), so refreshing here gives real throughput over `interval_secs`, the same
        // way psutil-based system_monitor.py diffs two net_io_counters() readings itself.
        self.networks.refresh();
        self.disks.refresh();
        // Per-process CPU usage needs the same two-samples-apart treatment as the global
        // number -- the previous tick's refresh is the first sample, so one walk per tick
        // is enough where the old code did two.
        self.sys.refresh_processes(ProcessesToUpdate::All, true);

        let (rx_bytes, tx_bytes) = self
            .networks
            .iter()
            .fold((0u64, 0u64), |(rx, tx), (_, data)| {
                (rx + data.received(), tx + data.transmitted())
            });
        let network_download_kbps =
            (rx_bytes as f64 / interval_secs / 1024.0 * 10.0).round() / 10.0;
        let network_upload_kbps = (tx_bytes as f64 / interval_secs / 1024.0 * 10.0).round() / 10.0;

        let cpu_percent = self.sys.global_cpu_usage();
        let ram_used_gb = bytes_to_gb(self.sys.used_memory());
        let ram_total_gb = bytes_to_gb(self.sys.total_memory());
        let ram_percent = if self.sys.total_memory() > 0 {
            (self.sys.used_memory() as f32 / self.sys.total_memory() as f32) * 100.0
        } else {
            0.0
        };

        let (disk_used_gb, disk_total_gb, disk_percent) = self
            .disks
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

        let mut top_processes: Vec<(String, f32)> = self
            .sys
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

        let battery = self.battery.as_mut().map(|(manager, battery)| {
            // A failed refresh leaves the previous reading in place rather than dropping
            // the row out of the panel: a battery that answered once and then hiccupped is
            // still a battery.
            let _ = manager.refresh(battery);
            battery_info(battery)
        });

        Telemetry {
            os_name: self.os_name.clone(),
            architecture: self.architecture.clone(),
            cpu_cores_logical: self.cpu_cores_logical,
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
            battery,
            // Read once and kept: see gpu::cached. A snapshot on a tick must not start a
            // process to re-learn something that cannot change.
            gpus: crate::gpu::cached().to_vec(),
            top_processes,
        }
    }
}

impl Telemetry {
    /// One reading, for a caller that wants a number now and will not ask again -- the
    /// doctor report and `/api/doctor`. Anything on a timer wants a [`Sampler`] it keeps,
    /// because this pays the whole construction cost (including a ~200ms wait for CPU
    /// usage to mean something) every time it is called.
    pub fn snapshot() -> Telemetry {
        Sampler::new().sample()
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

/// The signal that tells a parked loop to start reading again.
///
/// The telemetry loop used to sample the machine once a second forever -- minimised, closed
/// to the tray, on a laptop asleep in a bag. Nothing was reading it. The panel it feeds is
/// in a window, and a window nobody can see does not need a number a second.
///
/// So the loop parks when no window is visible and waits here. Waiting on a condition
/// variable costs nothing: the thread is off the scheduler entirely until something calls
/// `wake` -- a window event, or the HUD saying it became visible.
///
/// `park` still takes a timeout, and the loop passes a long one while it is asleep. That is
/// deliberate belt and braces: if some platform ever shows a window without any of the
/// events that call `wake` firing, the worst case is that the panel is a few seconds stale
/// rather than dead until restart. The cost of that safety net is one `is_visible()` call
/// per timeout, which is not a reading of the machine.
#[derive(Default)]
pub struct Pulse {
    woken: Mutex<bool>,
    signal: Condvar,
}

impl Pulse {
    /// Something happened that the loop should look at. Cheap and safe to call on every
    /// window event, from any thread.
    pub fn wake(&self) {
        *self.woken.lock().unwrap() = true;
        self.signal.notify_all();
    }

    /// Waits for `wake`, or for `timeout`, whichever comes first, and clears the signal.
    ///
    /// A `wake` that lands while the loop is mid-tick is not lost: the flag stays set, so
    /// the next `park` returns immediately rather than sleeping through the interaction
    /// that caused it.
    pub fn park(&self, timeout: Duration) {
        let mut woken = self.woken.lock().unwrap();
        if !*woken {
            let (guard, _) = self
                .signal
                .wait_timeout(woken, timeout)
                .expect("the pulse lock is never held across a panic");
            woken = guard;
        }
        *woken = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// The point of the sampler: the second reading must not pay what the first one did.
    /// Loose bounds deliberately -- this runs on whatever CI is given, and the claim is "a
    /// different order of magnitude", not a millisecond count.
    #[test]
    fn a_second_reading_is_far_cheaper_than_the_first() {
        let mut sampler = Sampler::new();
        let first = Instant::now();
        let _ = sampler.sample();
        let first = first.elapsed();

        let second = Instant::now();
        let _ = sampler.sample();
        let second = second.elapsed();

        // The first reading waits ~200ms for CPU usage to mean anything; no later one does.
        assert!(
            second < sysinfo::MINIMUM_CPU_UPDATE_INTERVAL,
            "a repeat reading took {second:?}, which means it is still waiting for a CPU \
             sample of its own",
        );
        assert!(
            second < first,
            "a repeat reading ({second:?}) should cost less than the first ({first:?})",
        );
    }

    /// The constants are read once and must not drift between readings -- they are the same
    /// machine.
    #[test]
    fn the_readings_that_cannot_change_do_not() {
        let mut sampler = Sampler::new();
        let first = sampler.sample();
        let second = sampler.sample();

        assert_eq!(first.os_name, second.os_name);
        assert_eq!(first.architecture, second.architecture);
        assert_eq!(first.cpu_cores_logical, second.cpu_cores_logical);
        // And it is a real reading, not a zeroed struct: a machine running this test has at
        // least one logical core and some memory.
        assert!(second.cpu_cores_logical >= 1);
        assert!(second.ram_total_gb > 0.0);
    }

    /// Telemetry::snapshot is still the one-shot the doctor report calls, and it still takes
    /// a real reading.
    #[test]
    fn the_one_shot_snapshot_still_reads_the_machine() {
        let snapshot = Telemetry::snapshot();
        assert!(snapshot.cpu_cores_logical >= 1);
        assert!(snapshot.ram_total_gb > 0.0);
    }

    /// A park that nothing wakes waits out its timeout, and does not return early. This is
    /// the parked telemetry loop's whole cost while a window is hidden.
    #[test]
    fn an_unwoken_park_waits_for_its_timeout() {
        let pulse = Pulse::default();
        let started = Instant::now();
        pulse.park(Duration::from_millis(120));
        assert!(
            started.elapsed() >= Duration::from_millis(100),
            "park returned after {:?}, which is not a wait",
            started.elapsed(),
        );
    }

    /// A wake from another thread ends the wait. The loop is parked for up to half a minute
    /// while hidden, so the window coming back has to cut that short rather than being
    /// noticed half a minute later.
    #[test]
    fn a_wake_from_another_thread_ends_the_wait() {
        let pulse = std::sync::Arc::new(Pulse::default());
        let waker = pulse.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            waker.wake();
        });

        let started = Instant::now();
        pulse.park(Duration::from_secs(30));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "park sat for {:?} after being woken",
            started.elapsed(),
        );
    }

    /// A wake that lands while the loop is mid-tick must not be lost -- otherwise an
    /// interaction during a reading is swallowed and the next one waits out the full
    /// interval.
    #[test]
    fn a_wake_during_a_tick_is_remembered() {
        let pulse = Pulse::default();
        pulse.wake(); // as if a window event arrived while the loop was sampling

        let started = Instant::now();
        pulse.park(Duration::from_secs(30));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "a wake before the park was dropped: park sat for {:?}",
            started.elapsed(),
        );

        // And it is not remembered twice: the next park waits properly.
        let started = Instant::now();
        pulse.park(Duration::from_millis(120));
        assert!(started.elapsed() >= Duration::from_millis(100));
    }
}
