//! The Profile pane: who the operator is to AETHER1, what they have used it for, and which
//! devices have paired with this machine over `--lan`.
//!
//! Everything here is counted out of the operator's own database and read off their own
//! disk. Nothing is fetched, nothing is sent, and no number is estimated: a stat that
//! cannot be counted honestly (tokens, on a server that reports no counts) is reported as
//! not measured rather than guessed at. That is the whole reason this file computes rather
//! than the frontend -- the messages table and the device list both live on this side, and
//! a browser paired over the LAN should get the same answer as the native window.
//!
//! The machine's own identity is here too -- its hostname, plus a nickname and a kind the
//! operator sets. With more than one box running AETHER1 those are what let it say "the
//! homelab is out of disk" rather than "this machine is out of disk", which is the
//! difference between a useful notice and one you have to go and work out.
//!
//! Devices are here because this is where the operator will look for them. Step 45 gave
//! every paired machine a token of its own and `aether1 revoke <id>` to take one away; this
//! is that list and that verb, in the window, for the operator who never opens a terminal.

use serde_json::{json, Value};

use crate::llm::{DayCount, LlmEngine, MemoryDb, UsageTotals};
use crate::serve_auth;

/// How much history the activity grid shows. Forty-four weeks of seven days is what fits
/// the pane at the width Settings opens at, and it is a year-ish view without pretending to
/// be exactly a year.
const ACTIVITY_DAYS: u32 = 44 * 7;

/// The whole pane's payload, native window and browser alike.
pub fn report(engine: &LlmEngine) -> Value {
    let db = engine.db();
    let totals: Option<UsageTotals> = db.usage_totals().ok();
    let activity = db.daily_message_counts(ACTIVITY_DAYS).unwrap_or_default();
    let benchmarks = db.benchmarks().unwrap_or_default();

    // Tokens are only ever what a provider reported having generated. Ollama and the OpenAI
    // -shaped APIs send counts back; some local servers send nothing, and a model that has
    // only ever been talked to through one of those contributes nothing here rather than a
    // number made up from characters divided by four.
    let tokens: u64 = benchmarks.iter().map(|b| b.completion_tokens).sum();
    let measured_models = benchmarks.len();

    // Today as SQLite reckons it, so "is the streak still alive" uses the same local day
    // boundary the message dates were grouped by.
    let (current_streak, longest_streak, busiest) = streaks(&activity, &db.local_now().0);

    let mut models: Vec<Value> = benchmarks
        .iter()
        .map(|b| {
            json!({
                "provider": b.provider,
                "model": b.model,
                "tokens": b.completion_tokens,
                "samples": b.samples,
                "average_tps": b.average_tps,
                "last_used": b.last_used,
            })
        })
        .collect();
    // `benchmarks()` orders by speed, which answers a different question; here the ranking
    // is by how much the model has actually been used.
    models.sort_by(|a, b| {
        b["tokens"]
            .as_u64()
            .unwrap_or(0)
            .cmp(&a["tokens"].as_u64().unwrap_or(0))
    });

    json!({
        "operator": {
            "name": operator_name(engine),
            "machine_nickname": db.get_setting_string("machine_nickname", ""),
            "machine_kind": db.get_setting_string("machine_kind", ""),
            "agent_name": engine.agent_name(),
            "machine": hostname(),
            "machine_described": machine_description(db),
        },
        "stats": {
            "conversations": totals.as_ref().map(|t| t.conversations).unwrap_or(0),
            "messages": totals.as_ref().map(|t| t.messages).unwrap_or(0),
            "sent": totals.as_ref().map(|t| t.sent).unwrap_or(0),
            "received": totals.as_ref().map(|t| t.received).unwrap_or(0),
            "longest_chat": totals.as_ref().map(|t| t.longest_chat).unwrap_or(0),
            "first_day": totals.as_ref().and_then(|t| t.first_day.clone()),
            "tokens": tokens,
            "measured_models": measured_models,
            "current_streak": current_streak,
            "longest_streak": longest_streak,
            "busiest_day": busiest.as_ref().map(|d| d.day.clone()),
            "busiest_day_messages": busiest.as_ref().map(|d| d.messages).unwrap_or(0),
        },
        "activity": {
            "days": ACTIVITY_DAYS,
            "counts": activity
                .iter()
                .map(|d| json!({ "day": d.day, "messages": d.messages }))
                .collect::<Vec<_>>(),
        },
        "models": models,
        "devices": devices(),
    })
}

/// What the operator is called, defaulting to nothing rather than to a guess: an empty name
/// means the system prompt says nothing about who it is talking to, which is exactly the
/// behaviour every install had before this field existed.
fn operator_name(engine: &LlmEngine) -> String {
    engine.db().get_setting_string("operator_name", "")
}

/// What this machine is called by the operating system. Not something AETHER1 sets or can
/// change -- it is the name the rest of the network already knows the box by.
fn hostname() -> String {
    sysinfo::System::host_name().unwrap_or_else(|| "this machine".to_string())
}

/// How AETHER1 should name the box it is running on, in one phrase.
///
/// With one machine this hardly matters. With three -- a desktop, a laptop and a homelab,
/// which is the setup the LAN half of this is for -- "this machine is out of disk" is a
/// notice you have to go and investigate before you know which box it is about, and "Aegis
/// (homelab) is out of disk" is one you can act on.
pub fn machine_description(db: &MemoryDb) -> String {
    describe_machine(
        &db.get_setting_string("machine_nickname", ""),
        &db.get_setting_string("machine_kind", ""),
        &hostname(),
    )
}

/// The hostname is always in the phrase, because it is the name that is true whatever the
/// operator has or has not typed; a nickname leads, and the hostname follows in brackets so
/// the two can still be matched up. Neither field is required.
fn describe_machine(nickname: &str, kind: &str, host: &str) -> String {
    let nickname = nickname.trim();
    let kind = kind.trim();
    let mut described = if nickname.is_empty() {
        host.to_string()
    } else {
        format!("{nickname} (hostname {host})")
    };
    if !kind.is_empty() {
        described = format!("{described}, a {kind}");
    }
    described
}

/// The paired devices, plus enough context to explain an empty list. Reading this never
/// creates a pairing phrase (see `serve_auth::paired_devices`).
fn devices() -> Value {
    let devices = serve_auth::paired_devices();
    json!({
        "pairing_set_up": serve_auth::pairing_is_set_up(),
        "devices": devices
            .into_iter()
            .map(|d| json!({ "id": d.id, "label": d.label, "paired_at": d.paired_at }))
            .collect::<Vec<_>>(),
    })
}

/// One device's access, taken away. `id` is `all` for every one of them, the same spelling
/// `aether1 revoke all` uses -- an id is always eight hex characters, so the word can never
/// be one.
pub fn revoke(id: &str) -> Result<Value, String> {
    if id == "all" {
        let count = serve_auth::revoke_all_paired_devices()?;
        return Ok(json!({ "revoked": count, "label": Value::Null }));
    }
    match serve_auth::revoke_paired_device(id)? {
        Some(label) => Ok(json!({ "revoked": 1, "label": label })),
        None => Err(format!(
            "no device with the id {id} -- it may already have been revoked"
        )),
    }
}

/// Current streak, longest streak and the busiest day, from days that have something on
/// them.
///
/// The counts arrive oldest first and skip empty days, so a streak is a run of dates one
/// apart. "Current" counts back from today and tolerates today being empty: at nine in the
/// morning a streak that ran to last night is still alive, and telling somebody it broke
/// because they have not typed yet would be wrong by fifteen hours.
fn streaks(counts: &[DayCount], today: &str) -> (u32, u32, Option<DayCount>) {
    let days: Vec<i64> = counts.iter().filter_map(|c| day_number(&c.day)).collect();
    if days.is_empty() {
        return (0, 0, None);
    }

    let mut longest = 1u32;
    let mut run = 1u32;
    for pair in days.windows(2) {
        if pair[1] == pair[0] + 1 {
            run += 1;
        } else {
            run = 1;
        }
        longest = longest.max(run);
    }

    let last = *days.last().unwrap_or(&0);
    let current = match day_number(today) {
        Some(today) if last >= today - 1 => run,
        _ => 0,
    };

    let busiest = counts
        .iter()
        .max_by_key(|c| c.messages)
        .cloned()
        .filter(|c| c.messages > 0);

    (current, longest, busiest)
}

/// A `YYYY-MM-DD` date as a day count, so "one apart" is subtraction rather than calendar
/// arithmetic. Days since 1 March of year 0 in the proleptic Gregorian calendar -- the
/// civil-from-days algorithm, which puts the leap day at the end of the year and so needs
/// no special cases at all.
fn day_number(date: &str) -> Option<i64> {
    let mut parts = date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146_097 + day_of_era)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(date: &str, messages: u32) -> DayCount {
        DayCount {
            day: date.to_string(),
            messages,
        }
    }

    #[test]
    fn a_machine_is_named_by_whatever_the_operator_has_filled_in() {
        assert_eq!(describe_machine("", "", "vm"), "vm");
        assert_eq!(describe_machine("", "laptop", "vm"), "vm, a laptop");
        assert_eq!(describe_machine("Aegis", "", "vm"), "Aegis (hostname vm)");
        assert_eq!(
            describe_machine("  Aegis  ", " homelab ", "vm"),
            "Aegis (hostname vm), a homelab"
        );
    }

    #[test]
    fn day_numbers_are_one_apart_across_a_month_and_a_leap_day() {
        assert_eq!(
            day_number("2026-02-01").unwrap() + 1,
            day_number("2026-02-02").unwrap()
        );
        assert_eq!(
            day_number("2026-01-31").unwrap() + 1,
            day_number("2026-02-01").unwrap()
        );
        assert_eq!(
            day_number("2024-02-28").unwrap() + 1,
            day_number("2024-02-29").unwrap()
        );
        assert_eq!(
            day_number("2024-12-31").unwrap() + 1,
            day_number("2025-01-01").unwrap()
        );
    }

    #[test]
    fn a_streak_running_up_to_yesterday_is_still_the_current_one() {
        let counts = vec![day("2026-01-08", 3), day("2026-01-09", 1)];
        let (current, _, _) = streaks(&counts, "2026-01-10");
        assert_eq!(current, 2);
    }

    #[test]
    fn the_longest_streak_is_the_longest_run_of_consecutive_days() {
        let counts = vec![
            day("2026-01-01", 4),
            day("2026-01-02", 2),
            day("2026-01-03", 9),
            day("2026-01-09", 1),
        ];
        let (_, longest, busiest) = streaks(&counts, "2026-01-10");
        assert_eq!(longest, 3);
        assert_eq!(busiest.unwrap().day, "2026-01-03");
    }

    #[test]
    fn a_streak_that_ended_long_ago_is_not_the_current_one() {
        let counts = vec![day("2020-01-01", 1), day("2020-01-02", 1)];
        let (current, longest, _) = streaks(&counts, "2026-01-10");
        assert_eq!(longest, 2);
        assert_eq!(current, 0);
    }

    #[test]
    fn nothing_said_is_no_streak_and_no_busiest_day() {
        let (current, longest, busiest) = streaks(&[], "2026-01-10");
        assert_eq!((current, longest), (0, 0));
        assert!(busiest.is_none());
    }

    #[test]
    fn a_date_that_is_not_one_is_no_day_at_all() {
        assert!(day_number("not-a-date").is_none());
        assert!(day_number("2026-13-01").is_none());
        assert!(day_number("2026-01").is_none());
    }
}
