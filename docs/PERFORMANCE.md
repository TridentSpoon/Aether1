# What AETHER1 costs while it is doing nothing

AETHER1 is meant to be left running. It sits in the tray, the hotkey summons it, the crash
watcher waits for an editor to die. A program with that shape is judged on what it costs when
nobody is talking to it, and on a laptop that cost is measured in battery.

This file is the record of that number: how it was measured, what it was, and what it is now.

## The three things it was spending on

**A full reading of the machine, once a second, forever.** The HUD's telemetry panel is fed by
a loop that called `Telemetry::snapshot()`. That was not a reading -- it was a whole new model
of the machine every tick: `System::new_all()` enumerating every process, disk and network
interface from scratch, a deliberate 200ms block so CPU usage had two samples to sit between, a
*second* walk over every process to get per-process usage, a new battery manager, and the
operating system's name and CPU core count re-read as though either could change while the
program was running. All of it thrown away a second later and built again. It ran whether or not
any window was on screen, so a minimised AETHER1, or one closed to the tray, or one on a laptop
asleep in a bag, was doing exactly this.

**The avatar drawing itself for nobody.** The hologram's render loop already skipped the scene
when `document.hidden` -- but it re-queued `requestAnimationFrame` *before* that check, so a
hidden window still woke sixty times a second to decide it had nothing to draw. A desktop
webview does not throttle `requestAnimationFrame` the way a background browser tab does.

**A model server held open for nobody.** Ollama, once AETHER1 started it, ran until AETHER1 was
quit -- and then kept running, because `app.exit(0)` does not reap a child. So "Quit AETHER1"
left a model server behind, holding its whole model in memory, until the machine was rebooted
or somebody found it in a task manager. It is the single most expensive idle thing on the
machine, and nothing ever stopped it.

## What it costs now

Idle means: started, nothing asked of it, nobody looking.

| | before | after |
|---|---|---|
| `aether1 --serve`, no browser attached, 5 minutes | 4.78s of CPU (1.59% of one core) | **0 CPU ticks** (below the kernel's 10ms resolution, for 300 seconds) |
| one telemetry reading, wall clock | 209ms | 1.7ms |
| one telemetry reading, scan work only (the 200ms block taken out) | 8.95ms | 1.7ms |

The reading is ~5x cheaper in work done and ~120x cheaper in wall time, and the loop that takes
it does not run at all when there is nobody to feed.

Measured on 2026-09-28, in a container with few processes running. A machine with hundreds of
processes pays more per scan than this, on both sides of the table -- the ratio is the part that
travels, not the absolute numbers.

The "before" column is `main` as it stands *after* the fix that moved the disk figures off the
tick, not before it. That fix removed a different and worse problem -- a `Disks` enumeration
inline on the tick, which blocks in uninterruptible I/O for fifteen seconds at a time on a dead
automount -- and measuring against the state before it would have credited its win to this
change. The two fixes also pull against each other in one place, which is worth knowing: holding
a `Disks` in the sampler across ticks, which is what the rest of this change does with the
`System` and `Networks`, would have put that blocking call straight back on the tick, no cheaper
for being held. So the sampler holds everything except the disks.

## What each fix was

**The telemetry sampler is kept, not rebuilt** (`src-tauri/src/llm/telemetry.rs`). `Sampler`
holds the `System`, `Networks` and battery handle across ticks and refreshes them. The
previous tick is the earlier of the two CPU samples, so the 200ms block is gone from every
reading but the first; the delta readings (CPU, network throughput) now get a full second of
spacing instead of 200ms, which makes them *more* accurate; the constants are read once. One
walk over the processes per tick where there were two. `Telemetry::snapshot()` still exists for
the callers that want one number and will not ask again -- the doctor report and `/api/doctor`.

**The loop parks when nobody is looking** (`src-tauri/src/main.rs`,
`src-tauri/src/server.rs`). In the desktop app, `any_window_on_screen` asks the window manager
whether any window AETHER1 owns is visible and not minimised; with none, the loop waits on a
condition variable (`llm::Pulse`) and is off the scheduler entirely. A window event wakes it,
and so does the HUD itself through `wake_telemetry_rust` -- the webview knows it is back before
the window manager tells anyone, and on some desktops it is the only one that knows. A 30-second
timeout backs that up, so a missed wake costs a few stale seconds rather than a dead panel.

In `--serve` the same rule applies to a different question: the loop parks until a
`/ws/telemetry` client subscribes. `aether1 --serve` left running in a terminal with no tab open
now costs nothing at all.

**The avatar loop parks too** (`frontend/js/hologram/animate.js`). It stops queueing frames
rather than queueing one to throw away, and something has to wake it: `visibilitychange`,
`focus`, `pageshow`, `resize`, or the `hud-visibility` event the Rust side emits when the window
manager's answer changes -- which is the only signal that catches a minimised WebKitGTK window,
whose page stays "visible" as far as the browser is concerned. Two of the reasons to park raise
no event at all (the panel switched off in Settings, a lost WebGL context), so a parked loop also
looks again every 500ms: two wake-ups a second instead of sixty. The clock keeps counting wall
time while parked, so the avatar comes back where it would have been rather than frozen
mid-gesture.

**The model server has a life now** (`src-tauri/src/background_services.rs`). It wakes when
AETHER1 starts if autostart is on, or on the first turn that needs it. It stops itself after
`ollama_idle_minutes` (default 15, 0 means never) with no local reply, and the next local turn
starts it again before the request goes out. A turn answered in the cloud counts for nothing
either way. And it dies with AETHER1: the exit handler stops it, along with a `--lan` server
AETHER1 started, before the process goes. An operator's own Ollama is never touched, the same
rule the manual start button already followed.

A server asleep on purpose is not a fault, so the diagnostics know the difference -- otherwise
the idle timeout would have looked like a bug every time it worked.

## Measuring it again

Idle cost of the headless server, which needs no display and is the one number reproducible
anywhere:

```sh
cargo build --release
AETHER1_DATA_DIR=$(mktemp -d) ./target/release/aether1 --serve &
PID=$!
sleep 5                                     # let it bind and settle
A=$(awk '{print $14+$15}' /proc/$PID/stat)  # utime + stime, in clock ticks
sleep 300
B=$(awk '{print $14+$15}' /proc/$PID/stat)
echo "$((B-A)) ticks over 300s"             # getconf CLK_TCK ticks to the second
```

The desktop app needs a screen, so it is a hands-on check rather than a script:

1. Start AETHER1 and leave the HUD open. `top` should show it ticking over -- it is drawing an
   animated avatar and reading the machine once a second, which is the point.
2. Close the window to the tray, or minimise it. Give it five minutes.
3. CPU should be at or near zero, and the process should be doing no per-second work at all.
   `sudo strace -f -c -p <pid>` over ten seconds is the sharper version of the same question:
   before this change it showed thousands of `openat` calls walking `/proc`; there should now be
   almost nothing.
4. Open the window again. The panel should be live on the next reading, not a stale one and not
   half a minute later.

The disk figures are the exception, and stay on their own thread behind `disk_usage()` for the
reason above.

The regression that hid inside the first measurement is worth knowing about, because it is the
kind that would come back: the `--serve` loop parks on `receiver_count() == 0`, and `run()` was
holding a receiver of its own for the life of the server (`let (tx, _rx) = broadcast::channel`),
so the count was never zero and the loop never parked. It measured at 0.71% of a core instead of
0. Nothing in the code read wrong; only the measurement caught it.
