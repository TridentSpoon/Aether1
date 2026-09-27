// The wider sweep of the system's event log -- the part of step 13 that is deliberately
// *not* a watcher.
//
// An event log is mostly noise. Warnings that are always there, units that restart by
// design, drivers complaining about hardware the machine does not have. A companion that
// reads that aloud is a companion you turn off, and turning it off would lose the crashes
// too. So the sweep is something you ask for -- `aether1 status --events` -- and crashes,
// which are unambiguous, are the only thing that ever speaks on its own.
//
// Same shape as crash.rs: the parsing is a free function over the tool's output, so the
// formats are testable on a machine that has neither systemd nor Windows.

use std::process::Command;

/// How many lines the sweep returns. Long enough to cover a bad morning, short enough to
/// read and to put in front of a model with a small context.
const SWEEP_LINES: usize = 60;

/// How many distinct lines the report prints. `SWEEP_LINES` bounds what journalctl is asked
/// for, which is not the same thing: one journal entry can be a hundred lines of stack trace,
/// and a report is read by a person and spoken aloud.
const REPORT_LINES: usize = 40;

/// One noteworthy line from the system's log, already reduced to something readable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub when: String,
    pub source: String,
    pub text: String,
}

impl Event {
    pub fn one_line(&self) -> String {
        format!("{}  {}: {}", self.when, self.source, self.text)
    }
}

/// Error-level events from this boot, newest last. An `Err` means the log could not be read
/// at all, which is worth saying out loud rather than showing as a clean bill of health.
pub fn sweep() -> Result<Vec<Event>, String> {
    #[cfg(target_os = "windows")]
    {
        windows_sweep()
    }
    #[cfg(not(target_os = "windows"))]
    {
        linux_sweep()
    }
}

#[cfg(not(target_os = "windows"))]
fn linux_sweep() -> Result<Vec<Event>, String> {
    let output = Command::new("journalctl")
        .args([
            "--no-pager",
            "--priority=err",
            "--boot",
            "--output=short-iso",
            "--lines",
            &SWEEP_LINES.to_string(),
        ])
        .output()
        .map_err(|e| {
            format!(
                "could not run journalctl, so the system log cannot be swept here: {e}. \
                 On a machine without systemd the logs are in /var/log instead."
            )
        })?;
    Ok(parse_journal_short_iso(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

#[cfg(target_os = "windows")]
fn windows_sweep() -> Result<Vec<Event>, String> {
    let script = format!(
        "Get-WinEvent -FilterHashtable @{{LogName='System','Application'; Level=1,2}} \
         -MaxEvents {SWEEP_LINES} -ErrorAction SilentlyContinue | \
         Sort-Object TimeCreated | ForEach-Object {{ \
           '{{0}}  {{1}}: {{2}}' -f $_.TimeCreated.ToString('yyyy-MM-ddTHH:mm:ssK'), \
           $_.ProviderName, ($_.Message -split \"`r?`n\")[0] }}"
    );
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|e| format!("could not read the Windows event log: {e}"))?;
    Ok(parse_two_space_lines(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

/// Reads `journalctl --output=short-iso`, whose lines are
/// `2026-09-20T14:03:11+0000 hostname unit[pid]: the message`.
///
/// The hostname is dropped -- on your own machine it is the same word on every line, and
/// what is wanted is when, what, and what it said.
pub fn parse_journal_short_iso(raw: &str) -> Vec<Event> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with("-- "))
        .filter(|line| !line.starts_with("No journal files were found"))
        .filter_map(|line| {
            let mut parts = line.splitn(3, ' ');
            let when = parts.next()?.to_string();
            /* A journal *entry* can be many lines -- a coredump carries the stack of every
            thread -- and journalctl prints them all under one `--lines` budget. Those
            continuation lines have no timestamp and no unit, so parsed as entries they
            became hundreds of events attributed to "the system", each one a frame address.
            Asking for 60 lines returned 1388 of them, and the companion read every one out
            loud. A line that does not begin with a timestamp is a continuation of the line
            above, so it belongs to an entry that is already in the list. */
            if !looks_like_timestamp(&when) {
                return None;
            }
            let _hostname = parts.next()?;
            let rest = parts.next()?;
            let (source, text) = match rest.split_once(": ") {
                Some((source, text)) => (strip_pid(source), text.trim().to_string()),
                // A line with no `unit: ` prefix is still worth keeping; it just has
                // nothing to attribute it to.
                None => ("the system".to_string(), rest.trim().to_string()),
            };
            if text.is_empty() {
                return None;
            }
            Some(Event { when, source, text })
        })
        .collect()
}

/// Whether a token is the `2026-09-20T14:03:11+0000` a `short-iso` line starts with. Only
/// the shape is checked, not the calendar: this separates an entry from a continuation line,
/// and a frame address or a `#3` never looks like this by accident.
fn looks_like_timestamp(token: &str) -> bool {
    let bytes = token.as_bytes();
    bytes.len() >= 19
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && bytes[10] == b'T'
}

/// `sshd[1234]` is the same source as `sshd`, and the pid is noise in a summary.
fn strip_pid(source: &str) -> String {
    match source.split_once('[') {
        Some((name, _)) => name.to_string(),
        None => source.to_string(),
    }
}

/// Reads the `when  source: text` lines the PowerShell above prints, which are already in
/// the shape `Event` wants.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn parse_two_space_lines(raw: &str) -> Vec<Event> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let (when, rest) = line.split_once("  ")?;
            let (source, text) = rest.split_once(": ")?;
            let text = text.trim();
            if text.is_empty() {
                return None;
            }
            Some(Event {
                when: when.trim().to_string(),
                source: source.trim().to_string(),
                text: text.to_string(),
            })
        })
        .collect()
}

/// The same error, logged four hundred times, is one thing that is wrong with the machine and
/// not four hundred. Events with the same source and the same text collapse into one line
/// carrying a count and the time it was last seen, in the order they were first seen. What
/// this deliberately does not do is normalise the text: two lines differing only in a pid or
/// an address stay two kinds, because guessing which digits are incidental is how a summary
/// starts lying about what the log said.
fn collapse(events: &[Event]) -> Vec<String> {
    let mut order: Vec<(String, String)> = Vec::new();
    // first seen, last seen, how many.
    let mut seen: std::collections::HashMap<(String, String), (String, String, usize)> =
        std::collections::HashMap::new();
    for event in events {
        let key = (event.source.clone(), event.text.clone());
        match seen.get_mut(&key) {
            Some((_, last, count)) => {
                last.clone_from(&event.when);
                *count += 1;
            }
            None => {
                seen.insert(key.clone(), (event.when.clone(), event.when.clone(), 1));
                order.push(key);
            }
        }
    }
    order
        .into_iter()
        .map(|key| {
            let (first, last, count) = &seen[&key];
            let (source, text) = &key;
            let once = Event {
                when: first.clone(),
                source: source.clone(),
                text: text.clone(),
            }
            .one_line();
            if *count == 1 {
                once
            } else {
                format!("{once}  (x{count}, last at {last})")
            }
        })
        .collect()
}

/// The sweep as it appears under `aether1 status --events`, including the two cases that
/// are not a list of problems: a log that could not be read, and a machine with nothing
/// wrong with it. Both are said plainly, because a blank space where a report should be
/// reads as a broken command.
pub fn report() -> String {
    match sweep() {
        Err(reason) => format!("Recent system errors\n\n    {reason}"),
        Ok(events) if events.is_empty() => {
            "Recent system errors\n\n    Nothing at error level since this machine booted."
                .to_string()
        }
        Ok(events) => {
            let total = events.len();
            let lines = collapse(&events);
            let shown = lines.len().min(REPORT_LINES);
            let mut out = if lines.len() == total {
                format!("Recent system errors ({total} since boot)\n")
            } else {
                // Both numbers, because "9 kinds" and "1388 lines" are different facts about
                // the same morning and a reader wants the second one to not be hidden.
                format!(
                    "Recent system errors ({} kinds, {total} in all, since boot)\n",
                    lines.len()
                )
            };
            for line in lines.iter().take(shown) {
                out.push_str("\n    ");
                out.push_str(line);
            }
            if lines.len() > shown {
                out.push_str(&format!(
                    "\n\n    ... and {} more kinds. `aether1 status --events` prints the log itself.",
                    lines.len() - shown
                ));
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JOURNAL: &str = "\
-- Journal begins at Sat 2026-09-19 08:00:00 UTC. --
2026-09-20T14:03:11+0000 tundra kernel: EXT4-fs error (device sda1): bad block
2026-09-20T14:04:02+0000 tundra sshd[1234]: error: kex_exchange_identification failed
2026-09-20T14:05:00+0000 tundra something-with-no-colon
";

    #[test]
    fn a_journal_line_becomes_when_what_and_what_it_said() {
        let events = parse_journal_short_iso(JOURNAL);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].when, "2026-09-20T14:03:11+0000");
        assert_eq!(events[0].source, "kernel");
        assert!(events[0].text.contains("bad block"));
        assert!(
            !events[0].text.contains("tundra"),
            "the hostname is the same word on every line and is dropped"
        );
    }

    #[test]
    fn a_pid_is_dropped_from_the_source() {
        let events = parse_journal_short_iso(JOURNAL);
        assert_eq!(events[1].source, "sshd", "sshd[1234] is still just sshd");
    }

    /// The coredump case that made "diagnostics" read a stack trace out loud: journalctl
    /// prints every line of a multi-line entry under one `--lines` budget, and the
    /// continuation lines carry neither a timestamp nor a unit.
    #[test]
    fn the_continuation_lines_of_one_entry_are_not_separate_events() {
        let raw = "\
2026-09-26T20:44:37+0200 norberta systemd-coredump[4]: Process 415397 dumped core.
                                 Stack trace of thread 415397:
                                 #0  0x00007f71a854c86d syscall (libc.so.6 + 0x14c86d)
                                 #1  0x00007f71a493287c g_cond_wait_until (libglib-2.0.so.0)
2026-09-26T20:45:51+0200 norberta kioworker: Cannot load metadata from file
";
        let events = parse_journal_short_iso(raw);
        assert_eq!(events.len(), 2, "got {events:#?}");
        assert!(events[0].text.contains("dumped core"));
        assert_eq!(events[1].source, "kioworker");
    }

    /// A frame number is not a date, and neither is an address.
    #[test]
    fn only_a_timestamp_starts_an_entry() {
        assert!(looks_like_timestamp("2026-09-26T20:44:37+0200"));
        assert!(!looks_like_timestamp("#3"));
        assert!(!looks_like_timestamp("0x00007f71a854c86d"));
        assert!(!looks_like_timestamp("Stack"));
    }

    /// The other half of what he reported: the same line, hundreds of times.
    #[test]
    fn the_same_error_many_times_over_is_one_line_with_a_count() {
        let mut events = Vec::new();
        for i in 0..5 {
            events.push(Event {
                when: format!("2026-09-26T20:4{i}:00+0200"),
                source: "kioworker".to_string(),
                text: "Cannot load metadata from file".to_string(),
            });
        }
        events.push(Event {
            when: "2026-09-26T21:00:00+0200".to_string(),
            source: "kernel".to_string(),
            text: "bad block".to_string(),
        });
        let lines = collapse(&events);
        assert_eq!(lines.len(), 2, "got {lines:#?}");
        assert!(
            lines[0].contains("(x5, last at 2026-09-26T20:44:00+0200)"),
            "{}",
            lines[0]
        );
        assert!(
            lines[0].starts_with("2026-09-26T20:40:00+0200"),
            "first seen leads the line"
        );
        assert!(
            !lines[1].contains("(x"),
            "one occurrence carries no count: {}",
            lines[1]
        );
    }

    /// Two lines differing by a pid stay two kinds. Collapsing them would need a guess about
    /// which digits are incidental, and a summary that guesses is a summary that misreports.
    #[test]
    fn two_errors_that_merely_look_alike_are_not_collapsed() {
        let events = vec![
            Event {
                when: "2026-09-26T20:40:00+0200".to_string(),
                source: "sshd".to_string(),
                text: "connection from 10.0.0.1 closed".to_string(),
            },
            Event {
                when: "2026-09-26T20:41:00+0200".to_string(),
                source: "sshd".to_string(),
                text: "connection from 10.0.0.2 closed".to_string(),
            },
        ];
        assert_eq!(collapse(&events).len(), 2);
    }

    #[test]
    fn a_report_of_many_kinds_is_cut_short_and_says_so() {
        let events: Vec<Event> = (0..REPORT_LINES + 7)
            .map(|i| Event {
                when: "2026-09-26T20:40:00+0200".to_string(),
                source: "kernel".to_string(),
                text: format!("distinct problem {i}"),
            })
            .collect();
        let lines = collapse(&events);
        assert_eq!(lines.len(), REPORT_LINES + 7);
        // The report itself is what caps; collapse reports everything it found.
        assert!(lines.len() > REPORT_LINES);
    }

    #[test]
    fn a_line_with_no_unit_prefix_is_kept_rather_than_dropped() {
        let events = parse_journal_short_iso(JOURNAL);
        assert_eq!(events[2].source, "the system");
        assert_eq!(events[2].text, "something-with-no-colon");
    }

    #[test]
    fn the_journals_own_preamble_is_not_an_event() {
        let events = parse_journal_short_iso(JOURNAL);
        assert!(!events
            .iter()
            .any(|event| event.text.contains("Journal begins")));
        assert!(parse_journal_short_iso("-- No entries --\n").is_empty());
        assert!(parse_journal_short_iso("No journal files were found\n").is_empty());
    }

    #[test]
    fn the_windows_lines_parse_into_the_same_shape() {
        let raw = "2026-09-20T14:03:11Z  Service Control Manager: The Print Spooler service terminated unexpectedly.\n";
        let events = parse_two_space_lines(raw);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].source, "Service Control Manager");
        assert!(events[0].text.contains("Print Spooler"));
    }

    #[test]
    fn a_machine_with_nothing_wrong_says_so_rather_than_printing_nothing() {
        // Whatever this machine's log holds, the report must never be blank -- a blank
        // report reads as a broken command rather than a clean bill of health.
        let report = report();
        assert!(report.starts_with("Recent system errors"));
        assert!(report.lines().count() >= 3);
    }
}
