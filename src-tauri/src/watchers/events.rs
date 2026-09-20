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
            let mut out = format!("Recent system errors ({} since boot)\n", events.len());
            for event in &events {
                out.push_str("\n    ");
                out.push_str(&event.one_line());
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
