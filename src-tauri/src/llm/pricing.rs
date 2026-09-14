//! What a cloud reply actually cost.
//!
//! Local models are free, so nothing here applies to them. Cloud tokens are money, and the
//! only honest way to show that is to multiply a *reported* token count by a price someone
//! published. Both halves can be missing, and this module says so rather than filling in a
//! plausible number:
//!
//! - No token count reported by the provider -> the caller never gets this far, because a
//!   cost derived from a four-characters-per-token guess is a guess with a currency symbol
//!   in front of it.
//! - No price known for the model -> `price_of` returns None, and the panel asks the
//!   operator to add one instead of showing $0.00, which reads as "free".
//!
//! Prices move. The built-in table below is a snapshot, stamped with `PRICES_AS_OF` so the
//! HUD can show how old it is, and every entry can be overridden by a JSON file next to the
//! database (see `load_overrides`) without rebuilding the app.

use std::collections::HashMap;
use std::path::Path;

/// When the built-in table below was last checked against the providers' own pricing pages.
/// Shown in the HUD next to any cost derived from it, because a six-month-old price is a
/// guess and the operator is entitled to know that before trusting the number.
pub const PRICES_AS_OF: &str = "2026-09";

/// The name of the override file, read from the same directory as the database.
pub const OVERRIDE_FILE: &str = "model_prices.json";

/// What a provider charges for a million tokens, in US dollars. Input (the prompt, which
/// includes the system prompt and the history sent with it) and output (the reply) are
/// billed at different rates by every provider here, usually four or five times apart.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelPrice {
    pub input_per_million: f64,
    pub output_per_million: f64,
}

/// Published list prices per million tokens, keyed by the longest model-name prefix that
/// identifies the model.
///
/// Prefixes, not exact names, because providers append dated build suffixes to the same
/// model -- `gpt-4o-mini-2024-07-18` is `gpt-4o-mini` -- and a table of exact names would
/// miss every pinned build. Longest match wins (see `price_of`), so `gpt-4o-mini` is found
/// before the shorter `gpt-4o` that also prefixes it.
///
/// Deliberately incomplete. A model released after this table was written has no entry, and
/// that is the correct outcome: "add a price for this model" is true, and an invented
/// number is not.
const BUILT_IN: &[(&str, ModelPrice)] = &[
    // -------- OpenAI
    (
        "gpt-4o-mini",
        ModelPrice {
            input_per_million: 0.15,
            output_per_million: 0.60,
        },
    ),
    (
        "gpt-4o",
        ModelPrice {
            input_per_million: 2.50,
            output_per_million: 10.00,
        },
    ),
    // -------- Anthropic
    (
        "claude-3-5-haiku",
        ModelPrice {
            input_per_million: 0.80,
            output_per_million: 4.00,
        },
    ),
    (
        "claude-3-5-sonnet",
        ModelPrice {
            input_per_million: 3.00,
            output_per_million: 15.00,
        },
    ),
    (
        "claude-3-opus",
        ModelPrice {
            input_per_million: 15.00,
            output_per_million: 75.00,
        },
    ),
    (
        "claude-haiku-4-5",
        ModelPrice {
            input_per_million: 1.00,
            output_per_million: 5.00,
        },
    ),
    (
        "claude-sonnet-4",
        ModelPrice {
            input_per_million: 3.00,
            output_per_million: 15.00,
        },
    ),
    (
        "claude-opus-4",
        ModelPrice {
            input_per_million: 15.00,
            output_per_million: 75.00,
        },
    ),
    // -------- Google
    (
        "gemini-1.5-flash",
        ModelPrice {
            input_per_million: 0.075,
            output_per_million: 0.30,
        },
    ),
    (
        "gemini-1.5-pro",
        ModelPrice {
            input_per_million: 1.25,
            output_per_million: 5.00,
        },
    ),
    (
        "gemini-2.0-flash",
        ModelPrice {
            input_per_million: 0.10,
            output_per_million: 0.40,
        },
    ),
    // -------- Groq
    (
        "llama-3.1-8b-instant",
        ModelPrice {
            input_per_million: 0.05,
            output_per_million: 0.08,
        },
    ),
    (
        "llama-3.3-70b-versatile",
        ModelPrice {
            input_per_million: 0.59,
            output_per_million: 0.79,
        },
    ),
];

/// Reads the operator's own price list, if they have written one.
///
/// Shape is a flat object of model prefix to prices, which is the least there is to get
/// wrong by hand:
///
/// ```json
/// { "claude-opus-5": { "input_per_million": 15.0, "output_per_million": 75.0 } }
/// ```
///
/// Every failure here -- no file, unreadable file, malformed JSON, a price that is negative
/// or not a number -- falls back to the built-in table with a line on stderr. A typo in a
/// hand-edited file must not stop the app from answering, and it must not silently become a
/// cost of zero either, which is why an entry that fails to parse is dropped rather than
/// defaulted.
pub fn load_overrides(dir: &Path) -> HashMap<String, ModelPrice> {
    let path = dir.join(OVERRIDE_FILE);
    if !path.exists() {
        return HashMap::new();
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) => {
            eprintln!(
                "[AETHER1] {} could not be read ({e}); using built-in prices.",
                path.display()
            );
            return HashMap::new();
        }
    };
    let parsed: HashMap<String, ModelPrice> = match serde_json::from_str(&text) {
        Ok(parsed) => parsed,
        Err(e) => {
            eprintln!(
                "[AETHER1] {} is not valid JSON ({e}); using built-in prices.",
                path.display()
            );
            return HashMap::new();
        }
    };
    parsed
        .into_iter()
        .filter(|(model, price)| {
            let sane = price.input_per_million >= 0.0
                && price.output_per_million >= 0.0
                && price.input_per_million.is_finite()
                && price.output_per_million.is_finite();
            if !sane {
                eprintln!(
                    "[AETHER1] ignoring the price for {model}: a price must be zero or more."
                );
            }
            sane
        })
        .collect()
}

/// The price for a model, preferring the operator's own list over the built-in one.
///
/// Matching is case-insensitive longest-prefix: providers spell the same model differently
/// in different places, and an operator writing the file by hand should not have to know
/// which capitalisation the API happened to echo back.
pub fn price_of(model: &str, overrides: &HashMap<String, ModelPrice>) -> Option<ModelPrice> {
    let model = model.to_ascii_lowercase();
    let from_overrides = overrides
        .iter()
        .filter(|(prefix, _)| model.starts_with(&prefix.to_ascii_lowercase()))
        .max_by_key(|(prefix, _)| prefix.len())
        .map(|(_, price)| *price);
    from_overrides.or_else(|| {
        BUILT_IN
            .iter()
            .filter(|(prefix, _)| model.starts_with(&prefix.to_ascii_lowercase()))
            .max_by_key(|(prefix, _)| prefix.len())
            .map(|(_, price)| *price)
    })
}

/// What a session's tokens came to, in dollars.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Cost {
    pub input_usd: f64,
    pub output_usd: f64,
    pub total_usd: f64,
}

/// Costs a session. `None` when no price is known for the model -- see the module note.
///
/// Rounded to five decimal places rather than to cents: a short exchange with a cheap model
/// is genuinely worth a fraction of a penny, and rounding that to $0.00 would tell the
/// operator their cloud usage is free.
pub fn cost_of(
    model: &str,
    prompt_tokens: u64,
    completion_tokens: u64,
    overrides: &HashMap<String, ModelPrice>,
) -> Option<Cost> {
    let price = price_of(model, overrides)?;
    let round = |v: f64| (v * 100_000.0).round() / 100_000.0;
    let input_usd = round(prompt_tokens as f64 / 1_000_000.0 * price.input_per_million);
    let output_usd = round(completion_tokens as f64 / 1_000_000.0 * price.output_per_million);
    Some(Cost {
        input_usd,
        output_usd,
        total_usd: round(input_usd + output_usd),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_overrides() -> HashMap<String, ModelPrice> {
        HashMap::new()
    }

    /// The reason the table holds prefixes at all: a pinned build is the same model.
    #[test]
    fn a_dated_build_matches_its_base_model() {
        let price = price_of("gpt-4o-mini-2024-07-18", &no_overrides()).expect("a known model");
        assert_eq!(price.input_per_million, 0.15);
    }

    /// `gpt-4o` is a prefix of `gpt-4o-mini`, and they are priced sixteen times apart.
    /// Shortest-match would bill mini traffic at the full model's rate.
    #[test]
    fn the_longest_matching_prefix_wins() {
        let mini = price_of("gpt-4o-mini", &no_overrides()).expect("mini");
        let full = price_of("gpt-4o-2024-11-20", &no_overrides()).expect("4o");
        assert_eq!(mini.output_per_million, 0.60);
        assert_eq!(full.output_per_million, 10.00);
    }

    #[test]
    fn matching_ignores_case() {
        assert!(price_of("Claude-3-5-Sonnet-20241022", &no_overrides()).is_some());
    }

    /// The whole point: a model nobody has priced is reported as unpriced, not as free.
    #[test]
    fn an_unknown_model_has_no_price_rather_than_a_zero_one() {
        assert!(price_of("some-model-released-next-year", &no_overrides()).is_none());
        assert!(cost_of("some-model-released-next-year", 1000, 1000, &no_overrides()).is_none());
    }

    #[test]
    fn an_override_beats_the_built_in_price() {
        let mut overrides = HashMap::new();
        overrides.insert(
            "gpt-4o-mini".to_string(),
            ModelPrice {
                input_per_million: 99.0,
                output_per_million: 1.0,
            },
        );
        let price = price_of("gpt-4o-mini", &overrides).expect("the override");
        assert_eq!(price.input_per_million, 99.0);
    }

    /// An operator can price a model the built-in table has never heard of, which is the
    /// only way a new release becomes costable without a new build of the app.
    #[test]
    fn an_override_can_add_a_model_that_is_not_built_in() {
        let mut overrides = HashMap::new();
        overrides.insert(
            "claude-opus-5".to_string(),
            ModelPrice {
                input_per_million: 15.0,
                output_per_million: 75.0,
            },
        );
        let cost = cost_of("claude-opus-5", 1_000_000, 1_000_000, &overrides).expect("priced");
        assert_eq!(cost.total_usd, 90.0);
    }

    #[test]
    fn cost_is_input_and_output_billed_at_their_own_rates() {
        // 2M in at $2.50, 0.5M out at $10.00.
        let cost = cost_of("gpt-4o", 2_000_000, 500_000, &no_overrides()).expect("priced");
        assert_eq!(cost.input_usd, 5.0);
        assert_eq!(cost.output_usd, 5.0);
        assert_eq!(cost.total_usd, 10.0);
    }

    /// A short exchange with a cheap model costs a fraction of a penny. Rounded to cents it
    /// would read as $0.00, which is the same thing the panel says about a local model --
    /// and the difference between "free" and "nearly free" is the entire point of showing
    /// cloud cost at all.
    #[test]
    fn a_tiny_cost_survives_rounding() {
        let cost = cost_of("gpt-4o-mini", 800, 200, &no_overrides()).expect("priced");
        assert!(
            cost.total_usd > 0.0,
            "a real cost must not round away to zero"
        );
        assert_eq!(cost.total_usd, 0.00024);
    }

    #[test]
    fn a_missing_override_file_is_not_an_error() {
        let dir = std::env::temp_dir().join("aether1-pricing-absent");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let _ = std::fs::remove_file(dir.join(OVERRIDE_FILE));
        assert!(load_overrides(&dir).is_empty());
    }

    /// A hand-edited file with a typo in it must leave the app working on the built-in
    /// prices, not stop it answering and not silently cost everything at zero.
    #[test]
    fn malformed_json_falls_back_to_the_built_in_table() {
        let dir = std::env::temp_dir().join("aether1-pricing-malformed");
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(dir.join(OVERRIDE_FILE), "{ not json ").expect("write");
        let overrides = load_overrides(&dir);
        assert!(overrides.is_empty());
        assert!(
            price_of("gpt-4o", &overrides).is_some(),
            "built-ins still work"
        );
    }

    #[test]
    fn a_negative_price_is_dropped_rather_than_used() {
        let dir = std::env::temp_dir().join("aether1-pricing-negative");
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(
            dir.join(OVERRIDE_FILE),
            r#"{"gpt-4o":{"input_per_million":-1.0,"output_per_million":2.0},
                "gpt-4o-mini":{"input_per_million":1.0,"output_per_million":2.0}}"#,
        )
        .expect("write");
        let overrides = load_overrides(&dir);
        assert!(
            !overrides.contains_key("gpt-4o"),
            "the bad entry is dropped"
        );
        assert!(
            overrides.contains_key("gpt-4o-mini"),
            "the good one survives"
        );
    }
}
