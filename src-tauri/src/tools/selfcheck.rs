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
//
// `self_repair` is the third, and it is the one that acts. Until it existed, `self_check`
// could say "AETHER1 can offer to fix this from Settings" and then leave the operator to go
// and find the panel -- the companion could name the fault precisely and not reach the one
// button that fixes it. **What it does not do is choose the fix.** The model names the part
// of the install that is unwell; `doctor::repair_for` decides what happens to it, from a
// fresh probe, at the moment of acting. That split is the whole design: the repair table is
// deterministic and hand-written precisely because the machine whose model endpoint is down
// is the machine that cannot ask a model what to do about it, and a tool that let the model
// pick the repair would put the weakest link back in the middle of it.

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
            // Says where the act lives, now that there are two places it can be asked for.
            // The sentence used to end at Settings, which left the companion naming a fault
            // it could not reach the fix for -- see the note at the top of this file.
            out.push_str(&format!(
                "      Repairable: {} -- call self_repair with check=\"{}\" if the operator \
                 wants it fixed, or they can press it in Settings > Diagnostics.\n",
                repair.title, check.key,
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

// ------------------------------------------------------------ self_repair

pub struct SelfRepair;

impl Tool for SelfRepair {
    fn name(&self) -> &'static str {
        "self_repair"
    }

    fn description(&self) -> &'static str {
        "Make the repair AETHER1's own self-check offers for one broken part of its install -- starting the model server, downloading the configured model or voice, clearing stale proposals. Name the part with `check`, as self_check reported it; which repair runs is decided by AETHER1 from a fresh check, not by you. Call this only after self_check has said that part is broken, and only when the operator wants it fixed."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "check": {
                    "type": "string",
                    "description": "Which check to repair, named exactly as `self_check` \
                                    printed it. Nothing is guessed at: a name that is not a \
                                    check is refused.",
                    "enum": doctor::CheckId::ALL.iter().map(|id| id.key()).collect::<Vec<_>>(),
                },
            },
            "required": ["check"],
        })
    }

    fn mutating(&self) -> bool {
        // It starts processes and downloads files. Every call is proposed and waits.
        true
    }

    fn always_allowable(&self) -> bool {
        // "Never ask me about self_repair again" would pre-approve starting servers and
        // fetching gigabytes for every check in the table, for ever, on the strength of one
        // tick. The tool's name says which program is acting and not what it will do, which
        // is the test `always_allowable` exists to apply -- see the note on `run_command`.
        false
    }

    fn preview(&self, args: &Value) -> String {
        let check = args.get("check").and_then(|v| v.as_str()).unwrap_or("");
        match doctor::CheckId::from_key(check) {
            // Deliberately does not promise which repair: the card is drawn when the model
            // asks, and the repair is chosen from a probe taken when the operator answers.
            // Naming one here would be naming the fix for a machine as it was minutes ago.
            Some(id) => format!(
                "Repair what is wrong with AETHER1's \"{}\" check -- the one fix it offers \
                 for that, decided from a fresh check when you approve this",
                id.title()
            ),
            None => format!("Repair {check:?}, which is not a part of AETHER1 it checks"),
        }
    }

    fn call(&self, args: &Value, ctx: &ToolContext) -> Result<Outcome, String> {
        let check = args
            .get("check")
            .and_then(|v| v.as_str())
            .ok_or("self_repair needs `check`: which part of the install to repair")?;
        let check_id = doctor::CheckId::from_key(check)
            .ok_or_else(|| format!("there is no check called {check:?}"))?;

        // The engine is built here from the database this call already has. Everything a
        // repair touches reaches it through `engine.db()`, and the engine's own fields are
        // per-turn caches for routing -- so a fresh one makes the same repair, and this
        // avoids threading an `LlmEngine` through every tool for the sake of one.
        let engine = crate::llm::LlmEngine::new(ctx.db.clone());

        // A fresh probe, not the one the model read. An approval sits in front of the
        // operator for as long as it takes them to look at it, and the repair offered for a
        // machine ten minutes ago is not necessarily the right act on it now. This is the
        // same recomputation `commands::doctor_repair` does behind the button.
        let (observation, _) = doctor::report(ctx.db, &doctor::Facts::default());
        let offered = doctor::repair_for(check_id, &observation).ok_or_else(|| {
            format!(
                "there is nothing AETHER1 knows how to repair about its \"{}\" check right \
                 now. Run self_check and tell the operator what it says.",
                check_id.title()
            )
        })?;

        match &offered.kind {
            doctor::RepairKind::Run => {}
            // Refused before `apply`, not inside it: applying records an attempt and spends
            // the one try this check gets per session, and spending it on something that
            // cannot work from here would mean the operator's own button then refuses too.
            doctor::RepairKind::InApp => {
                return Err(format!(
                    "AETHER1's \"{}\" check can only be repaired by the running desktop \
                     app, not from a tool call. Tell the operator to open Settings > \
                     Diagnostics and press \"{}\".",
                    check_id.title(),
                    offered.title
                ))
            }
            // The line doctor.rs does not cross, and neither does this. The command is
            // handed over verbatim for the operator to run.
            doctor::RepairKind::HandOver { command } => {
                return Err(format!(
                    "AETHER1's \"{}\" check needs a change only the operator can make -- \
                     AETHER1 does not run commands that need root. Give them this command to \
                     run themselves, exactly as it is written, and tell them to restart \
                     AETHER1 after it: {command}",
                    check_id.title()
                ))
            }
        }

        let outcome = doctor::apply(&engine, check_id, offered.id)?;
        Ok(Outcome::text(describe_repair(&offered, &outcome)))
    }
}

/// A repair's result, written for the model that asked for it.
///
/// The re-check is the point. `doctor::apply` re-probes and records a repair whose check is
/// still broken as a failure whatever the command's own exit code said, and this says so in
/// the same terms -- so the companion reports "that did not fix it" rather than reading a
/// zero exit status back to the operator as success.
pub fn describe_repair(repair: &doctor::Repair, outcome: &doctor::RepairOutcome) -> String {
    let mut out = format!("{}: {}\n", repair.title, outcome.message);
    match outcome.rechecked {
        Some(Verdict::Ok) => out.push_str("Checked again afterwards: that part is well now.\n"),
        Some(verdict) => {
            out.push_str(&format!(
                "Checked again afterwards and it is still not well [{}]: {}\nDo not repeat \
                 this -- AETHER1 refuses the same repair twice in one session. Tell the \
                 operator what the re-check says.\n",
                verdict.mark(),
                outcome
                    .recheck_detail
                    .as_deref()
                    .unwrap_or("no detail was available"),
            ));
        }
        None => out.push_str("It did not run, so nothing changed.\n"),
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

    fn repair(kind: doctor::RepairKind) -> doctor::Repair {
        doctor::Repair {
            id: doctor::RepairId::StartModelServer,
            title: "Start the model server".to_string(),
            detail: "Runs `ollama serve` in the background.".to_string(),
            kind,
        }
    }

    #[test]
    fn a_repairable_check_tells_the_model_how_to_ask_for_it() {
        // The sentence used to end at "from Settings", which named a fault and then left the
        // companion unable to reach the fix for it. It has to carry the tool and the exact
        // argument, or the model has to guess the key -- and a guessed key is refused.
        let mut broken = check(Verdict::Failed, "Model endpoint", "nothing answered");
        broken.repair = Some(repair(doctor::RepairKind::Run));
        let text = summarise(&health_with(vec![broken]), false);
        assert!(text.contains("self_repair"), "{text}");
        assert!(text.contains("check=\"database\""), "{text}");
        assert!(text.contains("Start the model server"), "{text}");
    }

    #[test]
    fn the_approval_card_names_the_part_and_promises_no_particular_fix() {
        // The card is drawn when the model asks and answered whenever the operator gets to
        // it. The repair is chosen from a probe taken at that second, so naming one here
        // would be describing the machine as it was before they went to make coffee.
        let preview = SelfRepair.preview(&json!({"check": "model-endpoint"}));
        // The check's own title, quoted: they are written as sentences ("A model answers"),
        // so a card that dropped them in bare would read as broken English.
        let title = doctor::CheckId::ModelEndpoint.title();
        assert!(preview.contains(&format!("\"{title}\"")), "{preview}");
        assert!(preview.contains("fresh check"), "{preview}");
    }

    #[test]
    fn a_check_that_does_not_exist_is_refused_rather_than_matched_loosely() {
        let preview = SelfRepair.preview(&json!({"check": "modelendpoint"}));
        assert!(
            preview.contains("not a part of AETHER1 it checks"),
            "{preview}"
        );

        let db = crate::llm::MemoryDb::open(
            std::env::temp_dir().join("a1-self-repair-unknown-check.sqlite"),
        )
        .expect("temp db");
        let ctx = ToolContext::new(&db);
        let err = SelfRepair
            .call(&json!({"check": "the-vibes"}), &ctx)
            .expect_err("an invented check is not repaired");
        assert!(err.contains("there is no check called"), "{err}");
    }

    #[test]
    fn every_check_the_model_may_name_is_a_real_one() {
        // The enum in the schema is the list the model picks from, so it has to be the same
        // list `CheckId::from_key` accepts -- otherwise the prompt offers names that are
        // refused the moment they are used.
        let schema = SelfRepair.parameters();
        let offered = schema["properties"]["check"]["enum"]
            .as_array()
            .expect("the check argument is an enum")
            .iter()
            .map(|v| v.as_str().expect("check names are strings").to_string())
            .collect::<Vec<_>>();
        assert_eq!(offered.len(), doctor::CheckId::ALL.len());
        for name in &offered {
            assert!(
                doctor::CheckId::from_key(name).is_some(),
                "the schema offers {name:?}, which is not a check"
            );
        }
    }

    #[test]
    fn repairing_is_never_pre_approved_and_always_mutating() {
        assert!(SelfRepair.mutating());
        assert!(
            !SelfRepair.always_allowable(),
            "one tick would pre-approve starting servers and fetching gigabytes for every \
             check in the table, for ever"
        );
    }

    fn outcome(rechecked: Option<Verdict>, detail: &str) -> doctor::RepairOutcome {
        doctor::RepairOutcome {
            check: "model-endpoint",
            repair: doctor::RepairId::StartModelServer,
            ran: true,
            message: "started".to_string(),
            rechecked,
            recheck_detail: Some(detail.to_string()),
            record: None,
        }
    }

    #[test]
    fn a_repair_that_ran_and_did_not_work_is_reported_as_not_working() {
        // The whole value of the re-check. A command that exits zero and leaves the check
        // broken is a failed repair, and reporting its exit status to the operator as
        // success is how a repair table rots into a pile of workarounds.
        let text = describe_repair(
            &repair(doctor::RepairKind::Run),
            &outcome(Some(Verdict::Failed), "still nothing at localhost:11434"),
        );
        assert!(text.contains("still not well"), "{text}");
        assert!(text.contains("still nothing at localhost:11434"), "{text}");
        assert!(text.contains("Do not repeat this"), "{text}");
    }

    #[test]
    fn a_repair_that_worked_says_the_check_passed_afterwards() {
        let text = describe_repair(
            &repair(doctor::RepairKind::Run),
            &outcome(Some(Verdict::Ok), "answering"),
        );
        assert!(text.contains("that part is well now"), "{text}");
        assert!(!text.contains("Do not repeat"), "{text}");
    }
}
