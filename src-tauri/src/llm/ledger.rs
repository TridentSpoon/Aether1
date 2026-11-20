// What each provider has left, as the provider itself reports it on every reply.
//
// The problem this exists for: the Auto router decides where a question goes from how hard
// it reads and what the card can hold, and knows nothing at all about whether the key it is
// about to use has any allowance left. So a hard question goes out, comes back as a 429, and
// the operator reads a refusal -- on a machine with a perfectly good local model sitting
// warm in memory, which is the one place the turn could have gone instead.
//
// The data is already arriving and was being thrown away. Every cloud provider here answers
// with its own headroom in the response headers of the reply it just gave, which makes this
// the cheapest possible source: no polling, no extra request, no account API to sign into,
// and the figure is exactly as current as the last thing that happened. `checked` in
// `providers.rs` sees every response, success or refusal, so that is where the reading is
// taken.
//
// **Two headers families are read for every online provider, not one per provider.**
// Anthropic spells it `anthropic-ratelimit-requests-remaining` with an RFC 3339 reset;
// OpenAI and Groq spell it `x-ratelimit-remaining-requests` with a duration like `6m0s`. A
// table mapping provider to spelling would be a table that goes stale the first time one of
// them changes, and a gateway is the case that settles it: a gateway passing an upstream's
// headers through could emit either family, and which one depends on what an administrator
// put behind it. So both are tried, whichever is present is used, and a provider that
// reports nothing -- Gemini reports nothing of this kind -- simply leaves the ledger empty
// and the router behaving exactly as it did before this module existed.
//
// **What this is not.** It is not a plan or subscription meter. AETHER1 signs into no
// consumer plan -- it takes API keys -- so there is no Pro-plan allowance here to show a
// bar for, and inventing one from token counts would be a guess presented as a balance. What
// is here is what a key reports about itself, which is less than a plan meter and true.
//
// **Only one of the three standings changes routing: `Spent`.** A reading that says four
// requests remain is not a reason to send the turn somewhere else -- the window it belongs
// to may reset in two seconds, and a router that flinches at a thin margin would move work
// off the cloud the operator chose and paid for on the strength of a number it cannot
// interpret. So a thin margin is shown and not acted on, and the only thing the router
// treats as a closed door is a door the provider has actually closed.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use super::Provider;

/// How long a bare remaining-count stays meaningful when nothing said when it resets.
///
/// Fifteen minutes. The figure is a judgement rather than a measurement, and the reason it
/// can be a loose one is that it only ever decides whether a count is *shown*: a reading
/// with no reset window cannot produce `Spent` with a time, so nothing routed depends on
/// this number. A count from an hour ago describes a window that has almost certainly rolled
/// over since, and showing it would be stating a balance that is no longer anybody's.
const STALE_AFTER_SECONDS: i64 = 900;

/// What one response said about what is left.
///
/// Durations rather than instants, deliberately, and it is the one decision in this file
/// worth arguing. Anthropic reports an absolute time and OpenAI reports a duration, so one
/// of the two has to be converted either way -- and converting to a duration means every
/// stored row is read as "this long after we saw it", which is arithmetic that survives a
/// machine whose clock is wrong, is adjusted by NTP between the reply and the question, or
/// crosses a daylight-saving boundary. An absolute reset time stored from a skewed clock is
/// wrong forever; a duration stored from one is wrong by the drift over a few minutes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reading {
    pub requests_left: Option<u64>,
    pub requests_limit: Option<u64>,
    pub tokens_left: Option<u64>,
    pub tokens_limit: Option<u64>,
    /// Seconds from the reply until the allowance refills, where the provider said.
    pub resets_in: Option<u64>,
    /// Seconds from the reply for which the provider refused to be asked again: a
    /// `retry-after` on a 429, and nothing else. This is the one unambiguous fact in the
    /// whole module -- not a count to interpret, but the provider saying "not yet".
    pub refused_for: Option<u64>,
}

impl Reading {
    /// Whether anything was actually learned. A response with none of these headers must
    /// leave the ledger untouched rather than overwrite a real reading with an empty one.
    pub fn says_anything(&self) -> bool {
        self.requests_left.is_some()
            || self.tokens_left.is_some()
            || self.resets_in.is_some()
            || self.refused_for.is_some()
    }
}

/// A reading as it comes back out of the database, with when it was taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stored {
    pub reading: Reading,
    /// Unix seconds at which the reply this came from arrived.
    pub seen_at: i64,
}

/// What the ledger can say about a provider right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    /// Nothing recorded, or nothing that still describes anything. The honest answer for a
    /// provider that reports no such headers, a key that has never been used, and a count
    /// so old that the window it belonged to is gone.
    Unknown,
    /// There is headroom. The figures are what was reported, and any of them may be absent.
    Clear {
        requests_left: Option<u64>,
        tokens_left: Option<u64>,
        /// The thinner of the two margins as a percentage of its own limit, where both a
        /// remaining count and a limit were reported. The thinner one, because an allowance
        /// is spent when its *first* half runs out: plenty of requests left is no comfort
        /// when the tokens are gone.
        left_percent: Option<u64>,
    },
    /// The provider has refused, or has reported nothing left and the window it belongs to
    /// has not yet rolled over.
    Spent {
        /// Seconds until it clears, where that is known. None is a real answer and not a
        /// zero: a provider can report `remaining: 0` without saying when it refills, and
        /// "clears in 0 seconds" would be a promise nothing supports.
        clears_in: Option<u64>,
    },
}

/// Unix seconds now, as the only clock reading in this module.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

/// What a stored reading means at a given moment.
///
/// A pure function of the row and the time, so every branch below is testable without a
/// database, a clock or a provider -- the same split `auto::decide` is built on, and for the
/// same reason: this is the part that has to be right.
pub fn standing(stored: Option<Stored>, now: i64) -> Standing {
    let Some(stored) = stored else {
        return Standing::Unknown;
    };
    let reading = stored.reading;
    let age = now.saturating_sub(stored.seen_at);
    let remaining = |window: u64| (window as i64).saturating_sub(age).max(0) as u64;

    // The provider's own refusal, first and on its own terms. A `retry-after` outranks every
    // count in the row: the counts are a description of an allowance, and this is an
    // instruction about a connection.
    if let Some(refused) = reading.refused_for {
        let left = remaining(refused);
        if left > 0 {
            return Standing::Spent {
                clears_in: Some(left),
            };
        }
    }

    /* The window has rolled over since this was read, so the counts in it describe an
    allowance that no longer exists -- including a zero. Reported as clear with no figures
    rather than as unknown, because one thing *is* known and it is the thing that matters:
    an allowance that has refilled is not spent. Saying "unknown" here would make a provider
    disappear from the picker's figures for the whole of the window it had just been given. */
    if let Some(resets_in) = reading.resets_in {
        if remaining(resets_in) == 0 {
            return Standing::Clear {
                requests_left: None,
                tokens_left: None,
                left_percent: None,
            };
        }
    }

    let exhausted = reading.requests_left == Some(0) || reading.tokens_left == Some(0);
    if exhausted {
        return Standing::Spent {
            clears_in: reading.resets_in.map(remaining).filter(|left| *left > 0),
        };
    }

    /* No reset window and an old reading: the counts are from a window that has very likely
    rolled over, and there is nothing in the row that can say whether it has. Unknown rather
    than clear-with-figures, because the figures are the part that has gone bad. */
    if reading.resets_in.is_none() && age > STALE_AFTER_SECONDS {
        return Standing::Unknown;
    }

    let share = |left: Option<u64>, limit: Option<u64>| match (left, limit) {
        (Some(left), Some(limit)) if limit > 0 => Some(left.saturating_mul(100) / limit),
        _ => None,
    };
    let left_percent = [
        share(reading.requests_left, reading.requests_limit),
        share(reading.tokens_left, reading.tokens_limit),
    ]
    .into_iter()
    .flatten()
    .min();

    Standing::Clear {
        requests_left: reading.requests_left,
        tokens_left: reading.tokens_left,
        left_percent,
    }
}

// ------------------------------------------------------------- reading the headers

/// The reading in one response's headers, for a provider whose replies leave this machine.
///
/// `header` is asked for a name in lower case and must be case-insensitive about matching
/// it, which is what the HTTP header map in `providers.rs` already is. Taking a lookup
/// closure rather than the response itself is what lets every case below be a test with a
/// handful of strings in it instead of a live provider.
///
/// Local providers are refused outright rather than simply reporting nothing: Ollama and LM
/// Studio meter nothing because there is nothing to meter, and a row in this table for a
/// model on the operator's own disk would be the one claim the whole design is against.
pub fn reading_from(
    provider: Provider,
    header: impl Fn(&str) -> Option<String>,
    now: i64,
) -> Option<Reading> {
    if !super::auto::is_online(provider) {
        return None;
    }
    let count = |names: [&str; 2]| {
        names
            .into_iter()
            .filter_map(&header)
            .find_map(|value| value.trim().parse::<u64>().ok())
    };
    /* Both spellings of the reset, and both readings of what a reset *is*: Anthropic names
    the moment, OpenAI names the wait. `seconds_until` takes either, so the two spellings
    collapse into one question here rather than into a per-provider branch. */
    let reset = [
        "anthropic-ratelimit-requests-reset",
        "x-ratelimit-reset-requests",
    ]
    .into_iter()
    .chain([
        "anthropic-ratelimit-tokens-reset",
        "x-ratelimit-reset-tokens",
    ])
    .filter_map(&header)
    .filter_map(|value| seconds_until(&value, now))
    // The longest of the resets reported, because the allowance is not back until the
    // last of its halves is: a requests window that refills in one second while the
    // tokens window has four minutes to go is still four minutes from useful.
    .max();

    let reading = Reading {
        requests_left: count([
            "anthropic-ratelimit-requests-remaining",
            "x-ratelimit-remaining-requests",
        ]),
        requests_limit: count([
            "anthropic-ratelimit-requests-limit",
            "x-ratelimit-limit-requests",
        ]),
        tokens_left: count([
            "anthropic-ratelimit-tokens-remaining",
            "x-ratelimit-remaining-tokens",
        ]),
        tokens_limit: count([
            "anthropic-ratelimit-tokens-limit",
            "x-ratelimit-limit-tokens",
        ]),
        resets_in: reset,
        // Read on every response rather than only on a 429, because it is only ever sent
        // when it means something, and a provider that volunteers it on a 200 is telling
        // the truth about the same thing.
        refused_for: header("retry-after").and_then(|value| seconds_until(&value, now)),
    };
    reading.says_anything().then_some(reading)
}

/// Seconds from now until the moment or the wait this header names, whichever it is.
///
/// Four shapes arrive in practice and all four are handled here, because a reset header is
/// one field whose format is a provider's own business and none of them document it as
/// stable:
///
/// * `2026-11-20T13:45:00Z` -- Anthropic, an absolute instant.
/// * `6m0s`, `1.5s`, `120ms`, `1h2m3s` -- OpenAI and Groq, a Go duration.
/// * `30` -- a bare number of seconds, which is what `retry-after` is.
/// * anything else -- None, and the field is treated as not having been sent at all.
///
/// The last line is the important one. A reset time that cannot be read is not an
/// emergency and must not become a guess: it leaves the row without a window, which
/// `standing` already knows how to be honest about.
fn seconds_until(text: &str, now: i64) -> Option<u64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Some(at) = epoch_of_rfc3339(text) {
        return Some(at.saturating_sub(now).max(0) as u64);
    }
    if let Ok(seconds) = text.parse::<f64>() {
        return (seconds.is_finite() && seconds >= 0.0).then_some(seconds.ceil() as u64);
    }
    seconds_of_duration(text)
}

/// A Go-style duration -- `1h2m3s`, `6m0s`, `1.5s`, `120ms` -- as whole seconds, rounded up.
///
/// Rounded up rather than down so that a wait of 1.2 seconds is never reported as one the
/// caller may already have finished. Anything with a unit this does not know, or a number it
/// cannot parse, gives None: a duration read wrongly is worse than one not read.
fn seconds_of_duration(text: &str) -> Option<u64> {
    let mut total = 0.0_f64;
    let mut number = String::new();
    let mut unit = String::new();
    let mut parsed_anything = false;

    let flush = |number: &mut String, unit: &mut String, total: &mut f64| -> Option<()> {
        if number.is_empty() {
            return None;
        }
        let value: f64 = number.parse().ok()?;
        let multiplier = match unit.as_str() {
            "ms" => 0.001,
            "s" => 1.0,
            "m" => 60.0,
            "h" => 3600.0,
            // Microseconds and nanoseconds are in Go's grammar and never in one of these
            // headers; they are taken rather than refused only so that a `500µs` cannot
            // make the whole field unreadable.
            "us" | "µs" | "ns" => 0.0,
            _ => return None,
        };
        *total += value * multiplier;
        number.clear();
        unit.clear();
        Some(())
    };

    for ch in text.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            if !unit.is_empty() {
                flush(&mut number, &mut unit, &mut total)?;
                parsed_anything = true;
            }
            number.push(ch);
        } else {
            unit.push(ch);
        }
    }
    flush(&mut number, &mut unit, &mut total)?;
    parsed_anything = parsed_anything || total >= 0.0;

    (parsed_anything && total.is_finite() && total >= 0.0).then(|| total.ceil() as u64)
}

/// Unix seconds for an RFC 3339 instant in the one shape these headers use: UTC, to the
/// second, with `Z` or a numeric offset.
///
/// Hand-rolled because the alternative is a date-time crate added to the build for one
/// header field, and because the shape is fixed: what arrives is a timestamp a server
/// generated, not a date a person typed. Anything that does not match is None, which is
/// read as "no reset window" rather than as a time -- so the failure mode of this function
/// is the router behaving as it did before the ledger existed.
fn epoch_of_rfc3339(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if bytes.len() < 20 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    if bytes[10] != b'T' && bytes[10] != b't' && bytes[10] != b' ' {
        return None;
    }
    let number = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let year = number(0..4)?;
    let month = number(5..7)?;
    let day = number(8..10)?;
    let hour = number(11..13)?;
    let minute = number(14..16)?;
    let second = number(17..19)?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }

    // The offset, where there is one. A `+02:00` means the instant named is two hours
    // earlier in UTC than its digits read, so the offset is subtracted.
    let tail = &text[19..];
    let tail = tail.split('.').next_back().unwrap_or(tail);
    let offset = if tail.ends_with('Z') || tail.ends_with('z') || tail.is_empty() {
        0
    } else {
        let sign = match tail.chars().next()? {
            '+' => 1,
            '-' => -1,
            _ => return None,
        };
        let digits = &tail[1..];
        let (hours, minutes) = digits.split_once(':')?;
        sign * (hours.parse::<i64>().ok()? * 3600 + minutes.parse::<i64>().ok()? * 60)
    };

    Some((days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second) - offset)
}

/// Days from 1970-01-01 to a calendar date, by Howard Hinnant's `days_from_civil`.
///
/// The shifted-era algorithm rather than a loop over years, because it is exact for every
/// date in range including leap years and century rules, and short enough to read.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

// ------------------------------------------------------------- the handover to the database

/// What the last response from each provider said, waiting to be written down.
///
/// A process-wide slot rather than a parameter threaded through every call, for the same
/// reason `LOCAL_FACTS` and `NO_NATIVE_TOOLS` in `providers.rs` are: that module holds no
/// database handle and should not start holding one. One HTTP funnel writes here, and
/// `LlmEngine::absorb_provider_readings` takes what is here and persists it -- so the
/// coupling is a small plain struct rather than a storage layer reaching into a wire format.
static PENDING: LazyLock<Mutex<HashMap<&'static str, Stored>>> = LazyLock::new(Default::default);

/// Notes what a response said, replacing anything not yet written for that provider.
///
/// Last one wins, which is right because the newest reading describes the newest state of
/// the allowance, and wrong in no case that matters: two replies from one provider inside a
/// single turn are the same window, read twice.
pub fn note(provider: Provider, reading: Reading, seen_at: i64) {
    if !reading.says_anything() {
        return;
    }
    if let Ok(mut pending) = PENDING.lock() {
        pending.insert(provider.key(), Stored { reading, seen_at });
    }
}

/// Takes everything noted since the last call, leaving the slot empty.
pub fn take_pending() -> Vec<(Provider, Stored)> {
    let Ok(mut pending) = PENDING.lock() else {
        return Vec::new();
    };
    pending
        .drain()
        .map(|(key, stored)| (Provider::from_key(key), stored))
        .collect()
}

/// Forgets what has been noted and not yet written. For tests, which share one process and
/// would otherwise see each other's readings.
#[cfg(test)]
pub fn forget_pending() {
    if let Ok(mut pending) = PENDING.lock() {
        pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_lowercase(), v.to_string()))
            .collect();
        move |name: &str| {
            owned
                .iter()
                .find(|(key, _)| key == &name.to_lowercase())
                .map(|(_, value)| value.clone())
        }
    }

    fn stored(reading: Reading, seen_at: i64) -> Option<Stored> {
        Some(Stored { reading, seen_at })
    }

    #[test]
    fn a_provider_that_reports_nothing_leaves_the_ledger_empty() {
        let reading = reading_from(
            Provider::Gemini,
            headers(&[("content-type", "application/json")]),
            1_000,
        );
        assert_eq!(reading, None);
    }

    #[test]
    fn a_local_provider_is_never_metered_even_if_something_sends_the_headers() {
        let reading = reading_from(
            Provider::Ollama,
            headers(&[("x-ratelimit-remaining-requests", "4")]),
            1_000,
        );
        assert_eq!(reading, None);
    }

    #[test]
    fn anthropics_spelling_is_read_with_its_absolute_reset() {
        let reading = reading_from(
            Provider::Anthropic,
            headers(&[
                ("anthropic-ratelimit-requests-limit", "50"),
                ("anthropic-ratelimit-requests-remaining", "11"),
                ("anthropic-ratelimit-requests-reset", "2026-11-20T13:45:00Z"),
            ]),
            // 2026-11-20T13:43:00Z, two minutes before the reset.
            1_795_182_180,
        )
        .expect("a reading");
        assert_eq!(reading.requests_left, Some(11));
        assert_eq!(reading.requests_limit, Some(50));
        assert_eq!(reading.resets_in, Some(120));
    }

    #[test]
    fn the_openai_spelling_is_read_with_its_duration_reset() {
        let reading = reading_from(
            Provider::OpenAi,
            headers(&[
                ("x-ratelimit-limit-tokens", "30000"),
                ("x-ratelimit-remaining-tokens", "1500"),
                ("x-ratelimit-reset-tokens", "6m0s"),
            ]),
            1_000,
        )
        .expect("a reading");
        assert_eq!(reading.tokens_left, Some(1500));
        assert_eq!(reading.tokens_limit, Some(30000));
        assert_eq!(reading.resets_in, Some(360));
    }

    #[test]
    fn a_gateway_is_read_in_whichever_family_it_passes_through() {
        let anthropic_shaped = reading_from(
            Provider::Gateway,
            headers(&[("anthropic-ratelimit-tokens-remaining", "9")]),
            1_000,
        )
        .expect("a reading");
        assert_eq!(anthropic_shaped.tokens_left, Some(9));

        let openai_shaped = reading_from(
            Provider::Gateway,
            headers(&[("x-ratelimit-remaining-tokens", "9")]),
            1_000,
        )
        .expect("a reading");
        assert_eq!(openai_shaped.tokens_left, Some(9));
    }

    #[test]
    fn the_longest_reset_reported_is_the_one_kept() {
        let reading = reading_from(
            Provider::Groq,
            headers(&[
                ("x-ratelimit-reset-requests", "1s"),
                ("x-ratelimit-reset-tokens", "4m"),
            ]),
            1_000,
        )
        .expect("a reading");
        assert_eq!(reading.resets_in, Some(240));
    }

    #[test]
    fn a_retry_after_is_read_as_a_refusal_in_seconds() {
        let reading = reading_from(
            Provider::Anthropic,
            headers(&[("retry-after", "30")]),
            1_000,
        )
        .expect("a reading");
        assert_eq!(reading.refused_for, Some(30));
    }

    #[test]
    fn a_reset_header_in_a_shape_nobody_recognises_is_treated_as_not_sent() {
        let reading = reading_from(
            Provider::OpenAi,
            headers(&[
                ("x-ratelimit-remaining-requests", "3"),
                ("x-ratelimit-reset-requests", "next tuesday"),
            ]),
            1_000,
        )
        .expect("a reading");
        assert_eq!(reading.requests_left, Some(3));
        assert_eq!(reading.resets_in, None);
    }

    #[test]
    fn durations_in_every_shape_these_providers_send() {
        assert_eq!(seconds_of_duration("1s"), Some(1));
        assert_eq!(seconds_of_duration("6m0s"), Some(360));
        assert_eq!(seconds_of_duration("1h2m3s"), Some(3723));
        assert_eq!(seconds_of_duration("1.5s"), Some(2));
        assert_eq!(seconds_of_duration("120ms"), Some(1));
        assert_eq!(seconds_of_duration("soon"), None);
    }

    #[test]
    fn an_rfc3339_instant_with_an_offset_is_not_read_as_utc() {
        // The same instant, written two ways. Reading the offset as nothing would put these
        // two hours apart, which on a four-minute window is the whole figure.
        let utc = epoch_of_rfc3339("2026-11-20T11:45:00Z").expect("utc");
        let offset = epoch_of_rfc3339("2026-11-20T13:45:00+02:00").expect("with an offset");
        assert_eq!(utc, offset);
    }

    #[test]
    fn fractional_seconds_do_not_stop_an_instant_being_read() {
        let plain = epoch_of_rfc3339("2026-11-20T13:45:00Z").expect("plain");
        let fractional = epoch_of_rfc3339("2026-11-20T13:45:00.123456Z").expect("fractional");
        assert_eq!(plain, fractional);
    }

    #[test]
    fn a_date_that_is_not_one_is_refused_rather_than_guessed_at() {
        assert_eq!(epoch_of_rfc3339("2026-13-01T00:00:00Z"), None);
        assert_eq!(epoch_of_rfc3339("not a time at all"), None);
        assert_eq!(epoch_of_rfc3339(""), None);
    }

    #[test]
    fn nothing_recorded_is_unknown_rather_than_clear() {
        assert_eq!(standing(None, 1_000), Standing::Unknown);
    }

    #[test]
    fn a_retry_after_that_has_not_elapsed_is_the_one_thing_that_closes_the_door() {
        let row = stored(
            Reading {
                refused_for: Some(60),
                ..Reading::default()
            },
            1_000,
        );
        assert_eq!(
            standing(row, 1_020),
            Standing::Spent {
                clears_in: Some(40)
            }
        );
    }

    #[test]
    fn a_refusal_that_has_elapsed_stops_closing_it() {
        let row = stored(
            Reading {
                refused_for: Some(60),
                requests_left: Some(5),
                ..Reading::default()
            },
            1_000,
        );
        assert!(!matches!(standing(row, 1_100), Standing::Spent { .. }));
    }

    #[test]
    fn nothing_left_is_spent_until_its_own_window_rolls_over() {
        let row = stored(
            Reading {
                requests_left: Some(0),
                resets_in: Some(100),
                ..Reading::default()
            },
            1_000,
        );
        assert_eq!(
            standing(row, 1_040),
            Standing::Spent {
                clears_in: Some(60)
            }
        );
        // Past the reset, the zero describes an allowance that has been replaced.
        assert_eq!(
            standing(row, 1_200),
            Standing::Clear {
                requests_left: None,
                tokens_left: None,
                left_percent: None,
            }
        );
    }

    #[test]
    fn nothing_left_and_no_window_is_spent_without_claiming_when_it_clears() {
        let row = stored(
            Reading {
                tokens_left: Some(0),
                ..Reading::default()
            },
            1_000,
        );
        assert_eq!(standing(row, 1_010), Standing::Spent { clears_in: None });
    }

    #[test]
    fn the_thinner_of_the_two_margins_is_the_one_reported() {
        let row = stored(
            Reading {
                requests_left: Some(45),
                requests_limit: Some(50),
                tokens_left: Some(300),
                tokens_limit: Some(30_000),
                resets_in: Some(600),
                ..Reading::default()
            },
            1_000,
        );
        assert_eq!(
            standing(row, 1_010),
            Standing::Clear {
                requests_left: Some(45),
                tokens_left: Some(300),
                // One percent of the tokens, not ninety of the requests.
                left_percent: Some(1),
            }
        );
    }

    #[test]
    fn a_count_with_no_window_goes_unknown_once_it_is_too_old_to_mean_anything() {
        let row = stored(
            Reading {
                requests_left: Some(7),
                requests_limit: Some(50),
                ..Reading::default()
            },
            1_000,
        );
        assert!(matches!(standing(row, 1_060), Standing::Clear { .. }));
        assert_eq!(
            standing(row, 1_000 + STALE_AFTER_SECONDS + 1),
            Standing::Unknown
        );
    }

    #[test]
    fn a_thin_margin_is_shown_and_never_treated_as_a_closed_door() {
        let row = stored(
            Reading {
                requests_left: Some(1),
                requests_limit: Some(50),
                resets_in: Some(60),
                ..Reading::default()
            },
            1_000,
        );
        assert!(!matches!(standing(row, 1_010), Standing::Spent { .. }));
    }

    #[test]
    fn a_reading_that_says_nothing_is_never_noted_over_one_that_said_something() {
        forget_pending();
        note(
            Provider::Anthropic,
            Reading {
                requests_left: Some(4),
                ..Reading::default()
            },
            1_000,
        );
        note(Provider::Anthropic, Reading::default(), 2_000);
        let pending = take_pending();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].1.reading.requests_left, Some(4));
        assert_eq!(pending[0].1.seen_at, 1_000);
    }
}
