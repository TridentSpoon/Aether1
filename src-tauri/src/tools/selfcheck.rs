// The two tools that let AETHER1 answer "what just went wrong?" about itself.
//
// Everything these read already existed: `watchers::crash` has been collecting crashes
// since step 13, and `doctor` has been judging fourteen parts of the install since step 47.
// Neither was reachable from a conversation. Asked why something broke, the companion could
// call `telemetry_detail` (a load average) or `list_processes` (what is alive now), and so
// it did what any model does with the wrong tools -- it read the system log and pasted it.
// A backtrace in the chat window is not a diagnosis, and it is what the operator saw.
//
// So these are deliberately *narrow*: they do not read the machine's logs, they read
// AETHER1's own two diagnostic records, and they return the prose those records already
// know how to write about themselves. `Crash::as_context` and the doctor's per-check detail
// were both written for a small local model to read, which is the whole reason this is two
// small tools rather than one "run diagnostics" that hands over a JSON dump.
//
// Both are read-only, and both are in every persona's domain (see `persona::domain`): a
// program explaining its own failure is not a privilege any character should have to be
// elevated for.

use serde_json::{json, Value};

use super::{Outcome, Tool, ToolContext};
use crate::doctor::{self, Verdict};
use crate::watchers::crash::{self, Availability};

/// How far back `recent_crashes` looks by default, and the furthest it may be asked to.
/// A day covers "it died while I was making coffee"; a week covers "it has been doing this
/// since the weekend". Beyond that the answer stops being about the thing being discussed.
const DEFAULT_HOURS: u64 = 24;
const MAX_HOURS: u64 = 24 * 7;

/// How many crashes one call may describe in full. Each carries a log tail, so a machine
/// having a bad day could otherwise fill the context with the same stack forty times.
const MAX_CRASHES: usize = 5;

// ------------------------------------------------------------ recent_crashes

pub struct RecentCrashes;

impl Tool for RecentCrashes {
    fn name(&self) -> &'static str {
        "recent_crashes"
    }

    fn description(&self) -> &'static str {
        "What has crashed on this machine recently, as AETHER1 recorded it: the program, what killed it, when, and the last lines it wrote before it stopped. Call this whenever the operator asks why something died, closed itself, disappeared or went wrong -- including when it was AETHER1 or a part of it, such as its window process."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "since_hours": {
                    "type": "integer",
                    "description": format!(
                        "How far back to look, in hours. Defaults to {DEFAULT_HOURS}, \
                         maximum {MAX_HOURS}."
                    ),
                },
                "program": {
                    "type": "string",
                    "description": "Only crashes of programs whose name contains this. \
                                    Leave it out for everything.",
                },
            },
            "required": [],
        })
    }

    fn mutating(&self) -> bool {
        false
    }

    fn call(&self, args: &Value, _ctx: &ToolContext) -> Result<Outcome, String> {
        let hours = match args.get("since_hours") {
            None | Some(Value::Null) => DEFAULT_HOURS,
            Some(v) => {
                let n = v
                    .as_u64()
                    .ok_or_else(|| "since_hours must be a whole number of hours".to_string())?;
                if n == 0 || n > MAX_HOURS {
                    return Err(format!("since_hours must be between 1 and {MAX_HOURS}"));
                }
                n
            }
        };
        let needle = args
            .get("program")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_ascii_lowercase);

        let reader = crash::reader_for_this_machine();
        // Availability is answered before the read, so "this machine cannot be watched" is
        // never reported to the model as "nothing has crashed" -- the silence step 13 was
        // built to avoid, and the one a model would confidently repeat to the operator.
        if let Availability::Unavailable(reason) = reader.availability() {
            return Ok(Outcome::text(format!(
                "Crash records cannot be read on this machine, so this is not a report that \
                 nothing has crashed -- it is a report that nothing can be seen. {reason}"
            )));
        }

        let since = now_secs().saturating_sub(hours * 3600);
        let crashes = reader.crashes_since(since)?;
        Ok(Outcome::text(describe(&crashes, hours, needle.as_deref())))
    }
}

/// The report, separated from reading the machine so the wording is testable without a
/// journal, a core dump or a Windows event log anywhere near it.
fn describe(crashes: &[crash::Crash], hours: u64, needle: Option<&str>) -> String {
    let window = if hours == 1 {
        "the last hour".to_string()
    } else if hours >= 24 && hours.is_multiple_of(24) {
        let days = hours / 24;
        if days == 1 {
            "the last 24 hours".to_string()
        } else {
            format!("the last {days} days")
        }
    } else {
        format!("the last {hours} hours")
    };

    let matching: Vec<&crash::Crash> = crashes
        .iter()
        .filter(|c| match needle {
            None => true,
            Some(needle) => c.program.to_ascii_lowercase().contains(needle),
        })
        .collect();

    if matching.is_empty() {
        return match needle {
            None => format!("Nothing has crashed in {window}."),
            Some(needle) => format!("Nothing matching {needle:?} has crashed in {window}."),
        };
    }

    // Newest first: the crash being asked about is nearly always the last one.
    let mut ordered = matching;
    ordered.sort_by_key(|c| std::cmp::Reverse(c.at));

    let shown = ordered.len().min(MAX_CRASHES);
    let mut out = format!(
        "{} in {window}{}:\n\n",
        if ordered.len() == 1 {
            "One crash".to_string()
        } else {
            format!("{} crashes", ordered.len())
        },
        if ordered.len() > shown {
            format!(", the {shown} most recent described")
        } else {
            String::new()
        }
    );
    for c in ordered.iter().take(shown) {
        out.push_str(&c.as_context());
        out.push_str("\n\n");
    }
    out.trim_end().to_string()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ------------------------------------------------------------ self_check

pub struct SelfCheck;

impl Tool for SelfCheck {
    fn name(&self) -> &'static str {
        "self_check"
    }

    fn description(&self) -> &'static str {
        "Run AETHER1's own self-check and report what is broken: the database, the model endpoint, the voice, the hotkey, disk space, crash capture and the rest. Call this when the operator says AETHER1 itself is not working, when a part of it failed, or when they ask what is wrong with the install."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "everything": {
                    "type": "boolean",
                    "description": "Include the checks that passed. Off by default: what is \
                                    working is rarely the question.",
                },
            },
            "required": [],
        })
    }

    fn mutating(&self) -> bool {
        // It looks at the world -- a few process spawns and two short-timeout connections --
        // and changes none of it. The repairs the doctor knows about are a separate act
        // with their own approval, and this tool cannot reach them.
        false
    }

    fn call(&self, args: &Value, ctx: &ToolContext) -> Result<Outcome, String> {
        let everything = args
            .get("everything")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        // Default facts: what the window knows about itself lives in the desktop process,
        // and a tool call cannot see it. Those checks come back Unknown, and the report
        // says so rather than calling them healthy.
        let (_, health) = doctor::report(ctx.db, &doctor::Facts::default());
        Ok(Outcome::text(summarise(&health, everything)))
    }
}

/// The self-check written out for a model to read. Pure, so the tests judge wording against
/// a hand-built report rather than against whatever this container happens to be missing.
pub fn summarise(health: &doctor::Health, everything: bool) -> String {
    let mut out = format!("{}\n", health.headline);

    let mut wrote_any = false;
    for check in &health.checks {
        let interesting = matches!(check.verdict, Verdict::Failed | Verdict::Degraded);
        if !interesting && !everything {
            continue;
        }
        wrote_any = true;
        out.push_str(&format!(
            "\n[{}] {}: {}\n",
            check.verdict.mark(),
            check.title,
            check.detail
        ));
        for step in &check.steps {
            out.push_str(&format!("    - {}: {}\n", step.title, step.detail));
            if let Some(command) = &step.command {
                out.push_str(&format!("      command: {command}\n"));
            }
        }
        if let Some(repair) = &check.repair {
            out.push_str(&format!(
                "      AETHER1 can offer to fix this from Settings: {}\n",
                repair.title
            ));
        }
    }

    if !wrote_any {
        out.push_str("\nEvery check passed.\n");
    }

    let unknown = health
        .checks
        .iter()
        .filter(|c| c.verdict == Verdict::Unknown)
        .count();
    if unknown > 0 && !everything {
        out.push_str(&format!(
            "\n{unknown} check(s) could not be judged from here -- they are about the window \
             and the tray, which only the desktop process can see. Settings > Diagnostics \
             shows them.\n"
        ));
    }

    if !health.repeated.is_empty() {
        out.push_str(&format!(
            "\nRepaired repeatedly across sessions, which suggests a bug rather than a \
             missing dependency: {}\n",
            health.repeated.join(", ")
        ));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crash_at(program: &str, at: u64) -> crash::Crash {
        crash::Crash {
            program: program.to_string(),
            pid: 4242,
            cause: "SIGABRT".to_string(),
            at,
            log_tail: vec!["something went wrong".to_string()],
        }
    }

    #[test]
    fn nothing_recorded_says_so_plainly() {
        assert_eq!(
            describe(&[], 24, None),
            "Nothing has crashed in the last 24 hours."
        );
        assert_eq!(
            describe(&[], 24, Some("webkit")),
            "Nothing matching \"webkit\" has crashed in the last 24 hours."
        );
    }

    #[test]
    fn a_crash_is_described_with_its_log() {
        let text = describe(&[crash_at("WebKitWebProcess", 100)], 24, None);
        assert!(
            text.starts_with("One crash in the last 24 hours:"),
            "{text}"
        );
        assert!(text.contains("WebKitWebProcess (pid 4242) stopped unexpectedly: SIGABRT."));
        assert!(text.contains("something went wrong"));
    }

    #[test]
    fn newest_first_and_capped() {
        let crashes: Vec<crash::Crash> = (0..8)
            .map(|i| crash_at(&format!("prog{i}"), 100 + i as u64))
            .collect();
        let text = describe(&crashes, 24, None);
        assert!(text.starts_with("8 crashes in the last 24 hours, the 5 most recent described:"));
        // The newest is prog7, and the three oldest are named nowhere.
        assert!(text.contains("prog7"));
        assert!(!text.contains("prog2"));
    }

    #[test]
    fn the_program_filter_is_a_substring_and_case_blind() {
        let crashes = vec![
            crash_at("/usr/libexec/WebKitWebProcess", 100),
            crash_at("ollama", 90),
        ];
        let text = describe(&crashes, 24, Some("webkit"));
        assert!(text.contains("WebKitWebProcess"), "{text}");
        assert!(!text.contains("ollama"), "{text}");
    }

    #[test]
    fn windows_are_worded_for_people() {
        assert!(describe(&[], 1, None).contains("the last hour"));
        assert!(describe(&[], 48, None).contains("the last 2 days"));
        assert!(describe(&[], 5, None).contains("the last 5 hours"));
    }

    fn health_with(checks: Vec<doctor::CheckReport>) -> doctor::Health {
        doctor::Health {
            os: Default::default(),
            headline: "Two problems.".to_string(),
            needs_attention: true,
            checks,
            repeated: Vec::new(),
        }
    }

    fn check(verdict: Verdict, title: &'static str, detail: &str) -> doctor::CheckReport {
        doctor::CheckReport {
            id: doctor::CheckId::Database,
            key: "database",
            title,
            owner: doctor::Owner::Aether1,
            verdict,
            detail: detail.to_string(),
            steps: Vec::new(),
            repair: None,
        }
    }

    #[test]
    fn the_summary_leads_with_what_is_broken_and_hides_what_is_not() {
        let health = health_with(vec![
            check(Verdict::Ok, "Database", "fine"),
            check(
                Verdict::Failed,
                "Model endpoint",
                "nothing answered at localhost:11434",
            ),
        ]);
        let text = summarise(&health, false);
        assert!(text.starts_with("Two problems."));
        assert!(text.contains("Model endpoint: nothing answered"));
        assert!(
            !text.contains("Database"),
            "a passing check is noise here: {text}"
        );
        assert!(summarise(&health, true).contains("Database"));
    }

    #[test]
    fn unknown_checks_are_named_as_unseen_rather_than_healthy() {
        let health = health_with(vec![check(
            Verdict::Unknown,
            "The window",
            "not visible here",
        )]);
        let text = summarise(&health, false);
        assert!(
            text.contains("1 check(s) could not be judged from here"),
            "{text}"
        );
        assert!(text.contains("Settings > Diagnostics"));
    }

    #[test]
    fn a_clean_report_says_so_instead_of_ending_on_the_headline() {
        let health = health_with(vec![check(Verdict::Ok, "Database", "fine")]);
        assert!(summarise(&health, false).contains("Every check passed."));
    }
}
