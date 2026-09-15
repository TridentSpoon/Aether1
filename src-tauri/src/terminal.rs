// A real terminal, and it is the operator's alone.
//
// Everything else in this project is something the companion can reach: a tool it may call,
// a route it may be asked to hit, a setting it may change with consent. This is the one
// thing that is not. It exists because an AI companion that helps you run your machine is
// only useful if you can also *run your machine* -- `sudo pacman -Syu` has to be able to ask
// for a password, `yay` has to be able to ask which package you meant -- and because the
// alternative, handing the model a shell, is the single change that would turn a mistake or
// a prompt injection into an unrecoverable afternoon.
//
// So the boundary is structural rather than a policy the code remembers to check:
//
//   * There is no tool. Nothing in tools/ mentions this module, so there is no name the
//     model can emit that reaches it, whatever it is persuaded to say.
//   * There is no route. server.rs never imports this module, so `--serve` and `--lan`
//     expose nothing here -- not to the browser fallback, and not to the network.
//   * Nothing is stored. Output goes to one Tauri event and is never written to the
//     database, the vault or the action log, so there is no file the model can be
//     pointed at afterwards to read what you typed.
//
// scripts/check_terminal_isolation.sh fails the build if any of those three stop being
// true, because a boundary maintained by good intentions is not a boundary. What that
// leaves is a terminal that only exists in the native desktop window, driven only by the
// keyboard, and that is the whole design.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use portable_pty::{CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};

/// How much of a single read is forwarded to the window at once. A terminal that pastes a
/// kernel build log is producing megabytes a second, and a 4 KiB chunk is roughly what one
/// read of a pty returns anyway.
const READ_CHUNK: usize = 4096;

/// The ceiling on live terminals. Not a resource concern so much as an accident one: every
/// terminal is a real shell process, and a bug in the frontend that opens one per render
/// should hit a wall rather than fork-bomb the machine.
const MAX_SESSIONS: usize = 8;

/// A terminal's size, in character cells. Clamped rather than trusted: these numbers come
/// from a frontend measuring a div, and a zero or a preposterous value reaches an ioctl.
const MIN_DIM: u16 = 1;
const MAX_COLS: u16 = 2000;
const MAX_ROWS: u16 = 2000;

fn clamp_size(rows: u16, cols: u16) -> PtySize {
    PtySize {
        rows: rows.clamp(MIN_DIM, MAX_ROWS),
        cols: cols.clamp(MIN_DIM, MAX_COLS),
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// One live terminal: the master side of the pty, and the handle its keystrokes go through.
///
/// The writer is separate from the master because the reader half lives on its own thread
/// for the life of the session; holding one lock for both would mean every keystroke
/// waiting on a blocking read that only returns when the shell says something.
///
/// The *slave* side is deliberately not here. See `open`.
struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
}

/// Every terminal currently open, keyed by the id handed back to the window that asked for
/// it. Tauri-managed state, which is itself part of the boundary: `--serve` never builds a
/// Tauri app, so in a headless run this map has no way to come into existence at all.
#[derive(Default)]
pub struct Terminals {
    sessions: Mutex<HashMap<String, Session>>,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// What the operator's shell is on this machine.
///
/// `$SHELL` first, because the operator has already told the system which shell is theirs
/// and disagreeing with that is not this code's place. The fallbacks are per-platform and
/// deliberately boring. `ComSpec` on Windows is how the OS itself names the command
/// processor, so it survives the unusual installs where `cmd.exe` is not where you expect.
fn operator_shell() -> CommandBuilder {
    #[cfg(windows)]
    {
        let shell = std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".to_string());
        return CommandBuilder::new(shell);
    }

    #[cfg(not(windows))]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let mut cmd = CommandBuilder::new(shell);
        // A login shell, so the profile that defines the operator's aliases, prompt and
        // PATH is the one that runs. A terminal that does not look like their terminal is
        // a worse terminal.
        cmd.arg("-l");
        cmd
    }
}

/// Where a new terminal starts. The home directory rather than wherever Aether1 happened
/// to be launched from -- which, started from a launcher entry or the tray, is `/` or
/// somewhere equally unhelpful.
fn start_directory() -> Option<std::path::PathBuf> {
    crate::paths::home_dir().filter(|p| p.is_dir())
}

/// Opens a terminal and starts pumping its output to `on_output`.
///
/// The returned id is what the window uses to write to and resize this terminal. It is a
/// counter, not a secret: there is nothing to guess past, because reaching this function at
/// all already means running inside the native window.
///
/// `on_output` is called on a thread of this function's making, for as long as the shell
/// lives. It is handed raw bytes, not text: a pty carries escape sequences, and a partial
/// UTF-8 character can and does land on a chunk boundary, so decoding belongs downstream in
/// the terminal emulator that understands both.
pub fn open<F>(
    terminals: &Terminals,
    rows: u16,
    cols: u16,
    on_output: F,
    on_exit: impl FnOnce(String) + Send + 'static,
) -> Result<String, String>
where
    F: Fn(&str, &[u8]) + Send + 'static,
{
    let mut sessions = terminals.sessions.lock().map_err(|_| poisoned())?;
    if sessions.len() >= MAX_SESSIONS {
        return Err(format!(
            "that is already {MAX_SESSIONS} terminals open -- close one before opening another"
        ));
    }

    let pty = NativePtySystem::default();
    let pair = pty
        .openpty(clamp_size(rows, cols))
        .map_err(|e| format!("could not open a terminal: {e}"))?;

    let mut cmd = operator_shell();
    if let Some(dir) = start_directory() {
        cmd.cwd(dir);
    }
    // What the shell and everything it runs will believe it is talking to. xterm-256color
    // is what the frontend emulator actually implements, and a program that trusts $TERM
    // and gets it wrong draws garbage.
    cmd.env("TERM", "xterm-256color");

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("could not start your shell: {e}"))?;

    // The shell has its own handle on the slave now, and this process must let go of its
    // copy. A pty reports end-of-file on the master only when the *last* slave handle
    // closes, so keeping this one alive means the read loop below never returns zero: the
    // operator types `exit`, the shell dies, and the terminal sits there looking alive
    // forever while its reader thread and its zombie child are never cleaned up. Dropping
    // it here is what makes an exited shell observable at all.
    drop(pair.slave);

    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("could not write to the terminal: {e}"))?;
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("could not read from the terminal: {e}"))?;

    let id = format!("t{}", NEXT_ID.fetch_add(1, Ordering::Relaxed));

    // The reader thread. It owns nothing but its own handle and the callback, so it cannot
    // deadlock against a keystroke being written, and it ends when the shell does.
    {
        let id = id.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; READ_CHUNK];
            loop {
                match reader.read(&mut buf) {
                    // A pty read returning zero means the slave side is gone: the shell
                    // exited, and there will never be more output.
                    Ok(0) | Err(_) => break,
                    Ok(n) => on_output(&id, &buf[..n]),
                }
            }
            // Reaped so the shell does not linger as a zombie once it has exited. The wait
            // is after the read loop rather than on its own thread because the loop ending
            // is precisely the signal that the child is on its way out.
            let _ = child.wait();
            on_exit(id);
        });
    }

    sessions.insert(
        id.clone(),
        Session {
            master: pair.master,
            writer,
        },
    );
    Ok(id)
}

/// Keystrokes, straight through. Whatever the operator typed goes to the shell unexamined:
/// this is a terminal, and a terminal that second-guesses your keys is broken.
pub fn write(terminals: &Terminals, id: &str, data: &[u8]) -> Result<(), String> {
    let mut sessions = terminals.sessions.lock().map_err(|_| poisoned())?;
    let session = sessions.get_mut(id).ok_or_else(|| gone(id))?;
    session
        .writer
        .write_all(data)
        .map_err(|e| format!("could not send that to the terminal: {e}"))?;
    session
        .writer
        .flush()
        .map_err(|e| format!("could not send that to the terminal: {e}"))
}

/// Tells the shell the window changed shape, which is what makes `less`, `htop` and a
/// wrapped command line redraw at the right width instead of tearing.
pub fn resize(terminals: &Terminals, id: &str, rows: u16, cols: u16) -> Result<(), String> {
    let sessions = terminals.sessions.lock().map_err(|_| poisoned())?;
    let session = sessions.get(id).ok_or_else(|| gone(id))?;
    session
        .master
        .resize(clamp_size(rows, cols))
        .map_err(|e| format!("could not resize the terminal: {e}"))
}

/// Closes a terminal. Dropping the session drops the master, which hangs up the pty; the shell
/// gets a hangup, the reader thread's next read returns zero, and the thread ends on its
/// own. Closing one that is already gone is success, not an error -- a window that has been
/// shut and a shell that has exited race, and both mean the same thing to the caller.
pub fn close(terminals: &Terminals, id: &str) -> Result<(), String> {
    let mut sessions = terminals.sessions.lock().map_err(|_| poisoned())?;
    sessions.remove(id);
    Ok(())
}

/// How many terminals are open. Test-only: the window tracks its own one terminal, and an
/// accessor for this that the app called would be a second answer to a question the app
/// already knows, free to drift from the first.
#[cfg(test)]
pub fn count(terminals: &Terminals) -> usize {
    terminals
        .sessions
        .lock()
        .map(|s| s.len())
        .unwrap_or_default()
}

fn gone(id: &str) -> String {
    format!("terminal {id} is not open any more")
}

/// A poisoned lock means a thread panicked while holding it, which here would mean the map
/// of live sessions is in an unknown state. Reported rather than unwrapped: the terminal
/// failing to accept a keystroke is recoverable, and taking the whole app down with it is
/// not.
fn poisoned() -> String {
    "the terminal's bookkeeping is in a bad state; close it and open a new one".to_string()
}

pub type Handle = Arc<Terminals>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    /// Waits for `want` to appear in what the shell has said so far, up to a timeout. A pty
    /// arrives in pieces at times the test cannot predict, so this accumulates rather than
    /// assuming any one chunk holds the whole answer.
    fn wait_for(rx: &mpsc::Receiver<Vec<u8>>, want: &str) -> String {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut seen = String::new();
        while std::time::Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(chunk) => {
                    seen.push_str(&String::from_utf8_lossy(&chunk));
                    if seen.contains(want) {
                        return seen;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        seen
    }

    fn opened(terminals: &Terminals) -> (String, mpsc::Receiver<Vec<u8>>) {
        let (tx, rx) = mpsc::channel();
        let id = open(
            terminals,
            24,
            80,
            move |_id, bytes| {
                let _ = tx.send(bytes.to_vec());
            },
            |_id| {},
        )
        .expect("a shell should start");
        (id, rx)
    }

    #[test]
    fn a_command_typed_into_it_actually_runs() {
        let terminals = Terminals::default();
        let (id, rx) = opened(&terminals);

        write(&terminals, &id, b"echo aether1-terminal-lives\n").unwrap();
        let seen = wait_for(&rx, "aether1-terminal-lives");
        assert!(
            seen.contains("aether1-terminal-lives"),
            "the shell should have run the command; saw: {seen:?}"
        );

        close(&terminals, &id).unwrap();
    }

    #[test]
    fn it_is_a_terminal_and_not_a_command_runner() {
        // The point of a pty rather than Command::output(): the program on the other end
        // believes it has a terminal, which is what lets sudo prompt, less page, and a
        // progress bar redraw. `test -t 0` is the shell's own way of asking.
        let terminals = Terminals::default();
        let (id, rx) = opened(&terminals);

        write(
            &terminals,
            &id,
            b"test -t 0 && echo ON-A-TTY || echo NOT-A-TTY\n",
        )
        .unwrap();
        let seen = wait_for(&rx, "ON-A-TTY");
        assert!(
            seen.contains("ON-A-TTY"),
            "stdin should be a tty; saw: {seen:?}"
        );

        close(&terminals, &id).unwrap();
    }

    #[test]
    fn the_shell_is_told_how_big_the_window_is() {
        let terminals = Terminals::default();
        let (tx, rx) = mpsc::channel();
        let id = open(
            &terminals,
            24,
            80,
            move |_id, bytes| {
                let _ = tx.send(bytes.to_vec());
            },
            |_id| {},
        )
        .unwrap();

        resize(&terminals, &id, 40, 132).unwrap();
        // Given a moment, because the resize and the command race otherwise.
        std::thread::sleep(Duration::from_millis(300));
        write(&terminals, &id, b"tput cols\n").unwrap();
        let seen = wait_for(&rx, "132");
        assert!(
            seen.contains("132"),
            "the shell should see the new width; saw: {seen:?}"
        );

        close(&terminals, &id).unwrap();
    }

    #[test]
    fn closing_a_terminal_forgets_it_and_closing_it_twice_is_fine() {
        let terminals = Terminals::default();
        let (id, _rx) = opened(&terminals);
        assert_eq!(count(&terminals), 1);

        close(&terminals, &id).unwrap();
        assert_eq!(count(&terminals), 0);
        close(&terminals, &id).expect("closing a closed terminal is not an error");

        // Writing to one that is gone is, though -- silently accepting keystrokes that go
        // nowhere is how a window ends up looking alive when it isn't.
        assert!(write(&terminals, &id, b"x").is_err());
        assert!(resize(&terminals, &id, 10, 10).is_err());
    }

    #[test]
    fn a_runaway_frontend_hits_a_wall_rather_than_forking_forever() {
        let terminals = Terminals::default();
        let mut ids = Vec::new();
        for _ in 0..MAX_SESSIONS {
            let (id, _rx) = opened(&terminals);
            ids.push(id);
        }
        assert_eq!(count(&terminals), MAX_SESSIONS);

        let err = open(&terminals, 24, 80, |_, _| {}, |_| {}).unwrap_err();
        assert!(err.contains("close one"), "should explain itself: {err}");

        for id in ids {
            close(&terminals, &id).unwrap();
        }
    }

    #[test]
    fn a_nonsense_size_is_clamped_before_it_reaches_an_ioctl() {
        let zero = clamp_size(0, 0);
        assert_eq!((zero.rows, zero.cols), (MIN_DIM, MIN_DIM), "never zero");

        let huge = clamp_size(u16::MAX, u16::MAX);
        assert_eq!((huge.rows, huge.cols), (MAX_ROWS, MAX_COLS));

        let ordinary = clamp_size(24, 80);
        assert_eq!((ordinary.rows, ordinary.cols), (24, 80), "left alone");
    }

    #[test]
    fn the_shell_exiting_reports_itself() {
        let terminals = Terminals::default();
        let (done_tx, done_rx) = mpsc::channel();
        let id = open(
            &terminals,
            24,
            80,
            |_id, _bytes| {},
            move |id| {
                let _ = done_tx.send(id);
            },
        )
        .unwrap();

        write(&terminals, &id, b"exit\n").unwrap();
        let exited = done_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("the window should be told the shell is gone");
        assert_eq!(exited, id);

        close(&terminals, &id).unwrap();
    }
}
