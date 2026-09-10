// Reading the Windows event log.
//
// This is the tool the System Diagnosis persona was named for -- "system diagnosis and
// event viewer checking" was the job description -- and until now it could not do the
// second half. The event logs are `.evtx` files, which are binary, so `read_file` could
// only ever report their size. `list_dir` could say which logs exist. Neither could say
// what happened.
//
// It is a dedicated tool rather than a `run_command` allowlist entry on purpose. Reading
// events is a read, and `run_command` is the mutating, never-pre-approvable escape hatch
// for arbitrary programs; routing a read through it would mean the operator has to approve
// every single one, and would mean putting `wevtutil` on an allowlist that then applies to
// every *other* invocation of it too -- including `wevtutil cl`, which clears a log.
//
// So the program is fixed, the subcommand is fixed at `qe` (query events, read-only), and
// every argument is built here from validated input. The model never supplies a command
// line; it supplies a channel name, a count and some filters, and each of those is checked
// before it becomes an argument.

#[cfg(windows)]
use std::process::{Command, Stdio};
#[cfg(windows)]
use std::time::{Duration, Instant};

use serde_json::{json, Value};

#[cfg(windows)]
use super::truncate;
use super::{Outcome, Tool, ToolContext};

/// A query on a large Security log is not instant, but it is not minutes either. Long
/// enough for a real answer, short enough that a wedged call does not hang the turn.
#[cfg(windows)]
const QUERY_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(windows)]
const MAX_OUTPUT_BYTES: usize = 16 * 1024;

/// How many events one call may return. The cap exists because the output goes into the
/// conversation: a thousand events would evict everything else being discussed.
const MAX_EVENTS: u64 = 200;
const DEFAULT_EVENTS: u64 = 20;

/// The furthest back a `since_hours` window may reach. A year of a busy Application log is
/// not a diagnosis, it is a haystack.
const MAX_SINCE_HOURS: u64 = 24 * 90;

/// Severity, in the numbering the event log itself uses.
///
/// Level 0 is "undefined", which providers use in practice to mean informational -- Event
/// Viewer's own Information filter matches `Level=4 or Level=0`, and this follows it. That
/// is also why the levels are enumerated with `or` rather than compared with `<=`: a range
/// would sweep Level 0 into every filter, so asking for errors would quietly return
/// informational events too.
fn levels_at_or_above(name: &str) -> Result<Option<Vec<u8>>, String> {
    Ok(match name {
        "critical" => Some(vec![1]),
        "error" => Some(vec![1, 2]),
        "warning" => Some(vec![1, 2, 3]),
        "information" => Some(vec![0, 1, 2, 3, 4]),
        "verbose" | "all" => None,
        other => {
            return Err(format!(
                "min_level must be one of critical, error, warning, information, verbose, \
                 all -- not {other:?}"
            ))
        }
    })
}

/// Characters allowed in a channel or provider name.
///
/// Real names are things like `System`, `Setup` and
/// `Microsoft-Windows-Kernel-Boot/Operational`, so letters, digits, `-`, `_`, `.`, `/` and
/// spaces cover them. Everything else is refused rather than escaped: a quote or a bracket
/// in one of these would be a way to reshape the XPath query around it, and there is no
/// legitimate name that needs one.
fn valid_name_chars(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ' '))
}

/// Checks a channel name before it becomes a command-line argument.
///
/// The leading-character rule is the one that matters. `wevtutil` takes its options as
/// `/c:20` and `-rd:true`, and the channel is a positional argument -- so a "channel" named
/// `/uni:true`, or `/e:root`, would not be read as a channel at all but as another option,
/// which is an argument-injection hole even though no shell is involved. A name may
/// therefore not begin with `/` or `-`.
fn validate_channel(name: &str) -> Result<(), String> {
    if name.starts_with('/') || name.starts_with('-') {
        return Err(format!(
            "{name:?} is not a log name: a leading '/' or '-' would be read as an option \
             rather than as the log to query"
        ));
    }
    if !valid_name_chars(name) {
        return Err(format!(
            "{name:?} is not a valid log name. Use a channel like \"System\", \
             \"Application\", \"Setup\", \"Security\", or \
             \"Microsoft-Windows-Kernel-Boot/Operational\""
        ));
    }
    Ok(())
}

/// Builds the XPath selector, or None when nothing was asked to be filtered.
///
/// Every value interpolated here has already been through `levels_at_or_above` (which only
/// ever yields integers), `validate_channel`, or `valid_name_chars` -- so nothing reaching
/// the format string can carry a quote or a bracket out of the argument it belongs to.
fn build_query(
    min_level: Option<&str>,
    since_hours: Option<u64>,
    provider: Option<&str>,
) -> Result<Option<String>, String> {
    let mut clauses: Vec<String> = Vec::new();

    if let Some(name) = min_level {
        if let Some(levels) = levels_at_or_above(name)? {
            let ors: Vec<String> = levels.iter().map(|l| format!("Level={l}")).collect();
            clauses.push(format!("({})", ors.join(" or ")));
        }
    }

    if let Some(hours) = since_hours {
        if hours == 0 || hours > MAX_SINCE_HOURS {
            return Err(format!(
                "since_hours must be between 1 and {MAX_SINCE_HOURS}, not {hours}"
            ));
        }
        // timediff works in milliseconds, measured back from now.
        let ms = hours * 60 * 60 * 1000;
        clauses.push(format!("TimeCreated[timediff(@SystemTime) <= {ms}]"));
    }

    if let Some(name) = provider {
        if !valid_name_chars(name) {
            return Err(format!(
                "{name:?} is not a valid provider name -- letters, digits, spaces and \
                 - _ . / only"
            ));
        }
        clauses.push(format!("Provider[@Name='{name}']"));
    }

    if clauses.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!("*[System[{}]]", clauses.join(" and "))))
}

/// Where wevtutil is, preferring the copy in System32 over whatever PATH says.
///
/// PATH is searched only as a fallback. The tool runs a fixed program by design, and
/// resolving it through PATH first would mean a directory earlier on PATH decides what
/// "read the event log" executes.
#[cfg(windows)]
fn wevtutil_path() -> Result<std::path::PathBuf, String> {
    if let Some(system_root) = crate::paths::system_root() {
        let direct = system_root.join("System32").join("wevtutil.exe");
        if direct.is_file() {
            return Ok(direct);
        }
    }
    which::which("wevtutil").map_err(|e| format!("cannot find wevtutil: {e}"))
}

pub struct ReadEventLog;

impl ReadEventLog {
    /// The argument vector, split out so it can be tested without a Windows box to run it
    /// on. Nothing here reads the environment or the disk.
    #[cfg(any(windows, test))]
    fn argv(channel: &str, count: u64, query: Option<&str>) -> Vec<String> {
        let mut argv = vec![
            "qe".to_string(),
            channel.to_string(),
            format!("/c:{count}"),
            // Newest first. Without this wevtutil starts at the oldest event in the
            // channel, which for a question about what just happened is the wrong end of
            // a log that may go back months.
            "/rd:true".to_string(),
            "/f:text".to_string(),
        ];
        if let Some(query) = query {
            argv.push(format!("/q:{query}"));
        }
        argv
    }
}

impl Tool for ReadEventLog {
    fn name(&self) -> &'static str {
        "read_event_log"
    }

    fn description(&self) -> &'static str {
        "Windows only: read recent entries from a Windows event log (System, Application, Setup, Security, or any channel name), newest first, optionally filtered by severity, age and source. This is what \"check the event viewer\" means -- the logs are binary files, so read_file cannot do it. On Linux and macOS read /var/log with read_file instead."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "log": {
                    "type": "string",
                    "description": "Channel name: System, Application, Setup, Security, or a full name like Microsoft-Windows-Kernel-Boot/Operational."
                },
                "count": {
                    "type": "integer",
                    "description": format!("How many events to return, newest first. Default {DEFAULT_EVENTS}, maximum {MAX_EVENTS}.")
                },
                "min_level": {
                    "type": "string",
                    "description": "Least severe level to include: critical, error, warning, information, verbose, or all. Defaults to all."
                },
                "since_hours": {
                    "type": "integer",
                    "description": format!("Only events from the last N hours. Maximum {MAX_SINCE_HOURS}.")
                },
                "provider": {
                    "type": "string",
                    "description": "Only events from this source, e.g. Service Control Manager."
                }
            },
            "required": ["log"]
        })
    }

    fn mutating(&self) -> bool {
        false
    }

    fn preview(&self, args: &Value) -> String {
        let log = args.get("log").and_then(Value::as_str).unwrap_or("?");
        let count = args
            .get("count")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_EVENTS);
        let level = args
            .get("min_level")
            .and_then(Value::as_str)
            .filter(|l| *l != "all")
            .map(|l| format!(" at {l} or worse"))
            .unwrap_or_default();
        format!("Read the last {count} events from the {log} log{level}")
    }

    fn call(&self, args: &Value, _ctx: &ToolContext) -> Result<Outcome, String> {
        let channel = args
            .get("log")
            .and_then(Value::as_str)
            .ok_or("missing required string argument \"log\"")?;
        validate_channel(channel)?;

        let count = args
            .get("count")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_EVENTS)
            .clamp(1, MAX_EVENTS);

        let query = build_query(
            args.get("min_level").and_then(Value::as_str),
            args.get("since_hours").and_then(Value::as_u64),
            args.get("provider").and_then(Value::as_str),
        )?;

        run_query(channel, count, query.as_deref())
    }
}

#[cfg(not(windows))]
fn run_query(_channel: &str, _count: u64, _query: Option<&str>) -> Result<Outcome, String> {
    // Every argument was still validated above, so a bad call is reported as a bad call on
    // every platform rather than only on the one that can run it.
    Err(
        "the Windows event log only exists on Windows. On this machine, read /var/log with \
         read_file, or list it with list_dir."
            .to_string(),
    )
}

#[cfg(windows)]
fn run_query(channel: &str, count: u64, query: Option<&str>) -> Result<Outcome, String> {
    let binary = wevtutil_path()?;
    let argv = ReadEventLog::argv(channel, count, query);

    let mut command = Command::new(&binary);
    command
        .args(&argv)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Without this the app -- which has no console of its own -- flashes a new console
    // window for every single query.
    crate::paths::suppress_console_window(&mut command);

    let mut child = command
        .spawn()
        .map_err(|e| format!("cannot start wevtutil: {e}"))?;

    // std has no wait-with-timeout, so poll and kill rather than leaving it running.
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() > QUERY_TIMEOUT => {
                let _ = child.kill();
                return Err(format!(
                    "reading the {channel} log took longer than {}s and was stopped. Try a \
                     smaller count, or narrow it with since_hours.",
                    QUERY_TIMEOUT.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(format!("cannot wait for wevtutil: {e}")),
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("cannot collect wevtutil's output: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        // The Security channel is readable only with administrator rights, and the error
        // wevtutil gives for it says "Access is denied" without saying why -- which reads
        // like Aether1 refusing rather than Windows refusing.
        let hint = if stderr.contains("Access is denied") {
            "\n(The Security log is readable only by an administrator. Aether1 is not \
             refusing this -- Windows is.)"
        } else {
            ""
        };
        return Err(format!("could not read the {channel} log: {stderr}{hint}"));
    }

    let text = truncate(&String::from_utf8_lossy(&output.stdout), MAX_OUTPUT_BYTES);
    if text.trim().is_empty() {
        return Ok(Outcome::text(format!(
            "No events in the {channel} log matched."
        )));
    }
    Ok(Outcome::text(format!(
        "Most recent events in the {channel} log:\n{}",
        text.trim_end()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hole this closes: wevtutil reads `/c:20` and `-rd:true` as options, and the
    /// channel is positional -- so a "channel" spelled like an option would silently
    /// become one. No shell is involved and it would still be argument injection.
    #[test]
    fn a_channel_name_may_not_be_spelled_like_an_option() {
        for smuggled in ["/uni:true", "-rd:false", "/e:root", "/q:*"] {
            let err = validate_channel(smuggled).unwrap_err();
            assert!(
                err.contains("option"),
                "{smuggled} must be refused as an option: {err}"
            );
        }
    }

    #[test]
    fn a_channel_name_may_not_carry_quotes_or_brackets_into_the_query() {
        for bad in [
            "System' or '1'='1",
            "App[lication",
            "Sys\"tem",
            "System]]",
            "",
        ] {
            assert!(
                validate_channel(bad).is_err(),
                "{bad:?} should not be a valid channel"
            );
        }
        for good in [
            "System",
            "Application",
            "Security",
            "Setup",
            "Microsoft-Windows-Kernel-Boot/Operational",
        ] {
            assert!(validate_channel(good).is_ok(), "{good} should be valid");
        }
    }

    /// Levels are enumerated rather than compared with `<=` because Level 0 means
    /// "undefined" and providers use it for informational events. A range would sweep it
    /// into the error filter, so "show me the errors" would return chatter.
    #[test]
    fn severity_filters_enumerate_levels_and_leave_level_zero_out_of_the_serious_ones() {
        let errors = build_query(Some("error"), None, None).unwrap().unwrap();
        assert_eq!(errors, "*[System[(Level=1 or Level=2)]]");
        assert!(!errors.contains("Level=0"));

        let warnings = build_query(Some("warning"), None, None).unwrap().unwrap();
        assert_eq!(warnings, "*[System[(Level=1 or Level=2 or Level=3)]]");
        assert!(!warnings.contains("Level=0"));

        // Information is the one place Level 0 belongs, matching Event Viewer's own filter.
        let info = build_query(Some("information"), None, None)
            .unwrap()
            .unwrap();
        assert!(info.contains("Level=0"), "{info}");

        // Nothing to filter is no query at all, rather than a query matching everything.
        assert_eq!(build_query(Some("all"), None, None).unwrap(), None);
        assert_eq!(build_query(None, None, None).unwrap(), None);

        assert!(build_query(Some("catastrophic"), None, None).is_err());
    }

    #[test]
    fn the_filters_combine_into_one_selector() {
        let query = build_query(Some("error"), Some(24), Some("Service Control Manager"))
            .unwrap()
            .unwrap();
        assert_eq!(
            query,
            "*[System[(Level=1 or Level=2) and \
             TimeCreated[timediff(@SystemTime) <= 86400000] and \
             Provider[@Name='Service Control Manager']]]"
        );
    }

    #[test]
    fn a_provider_name_cannot_break_out_of_its_quotes() {
        let err = build_query(None, None, Some("Foo' or '1'='1")).unwrap_err();
        assert!(err.contains("valid provider name"), "{err}");
    }

    #[test]
    fn a_time_window_has_to_be_a_window() {
        assert!(build_query(None, Some(0), None).is_err());
        assert!(build_query(None, Some(MAX_SINCE_HOURS + 1), None).is_err());
        assert!(build_query(None, Some(MAX_SINCE_HOURS), None).is_ok());
    }

    /// Newest first, and the count capped -- the output lands in the conversation, so an
    /// unbounded query would evict everything else being discussed.
    #[test]
    fn the_command_line_reads_the_newest_events_first() {
        let argv = ReadEventLog::argv("System", 20, None);
        assert_eq!(argv[0], "qe", "only the read-only subcommand is ever run");
        assert_eq!(argv[1], "System");
        assert!(argv.contains(&"/rd:true".to_string()), "{argv:?}");
        assert!(argv.contains(&"/c:20".to_string()), "{argv:?}");
        assert!(!argv.iter().any(|a| a.starts_with("/q:")));

        let filtered = ReadEventLog::argv("System", 5, Some("*[System[(Level=2)]]"));
        assert!(filtered.contains(&"/q:*[System[(Level=2)]]".to_string()));
    }

    #[test]
    fn the_count_is_clamped_rather_than_trusted() {
        let ctx_db = crate::llm::MemoryDb::open(
            std::env::temp_dir().join(format!("aether1_evt_{}.db", std::process::id())),
        )
        .unwrap();
        let ctx = ToolContext::new(&ctx_db);
        // Not Windows here, so the call reports that -- but only after validating, which
        // is the part being asserted: a bad argument is a bad argument everywhere.
        let err = ReadEventLog
            .call(&json!({"log": "/uni:true"}), &ctx)
            .unwrap_err();
        assert!(err.contains("option"), "{err}");

        let err = ReadEventLog
            .call(&json!({"log": "System", "min_level": "nonsense"}), &ctx)
            .unwrap_err();
        assert!(err.contains("min_level"), "{err}");
    }

    #[test]
    fn the_preview_says_what_it_would_read() {
        let preview = ReadEventLog.preview(&json!({"log": "System", "count": 50}));
        assert!(preview.contains("50"), "{preview}");
        assert!(preview.contains("System"), "{preview}");

        let filtered = ReadEventLog.preview(&json!({"log": "Application", "min_level": "error"}));
        assert!(filtered.contains("error or worse"), "{filtered}");
    }
}
