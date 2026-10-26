//! Browsing Hugging Face for a model to run locally.
//!
//! Until now the model picker offered two lists compiled into the binary -- nineteen chat
//! models and a handful of coding ones -- plus whatever the local server already had. That
//! is a reasonable starting point and a bad ceiling: the catalogue is as current as the
//! last release, it cannot answer "what is good this month", and every model anybody has
//! actually heard of arrives on Hugging Face first. Hugging Face appeared in this codebase
//! only for Piper voices and whisper weights, never for a language model.
//!
//! The mechanism that makes this cheap is Ollama's own: `ollama pull hf.co/<repo>:<quant>`
//! pulls a GGUF straight from a Hugging Face repository, through the same `/api/pull`
//! stream `downloads.rs` already drives and reports. So nothing here downloads anything.
//! It answers two questions -- which repositories exist, and which quantisations one has --
//! and hands back the name to pull.
//!
//! ## What "a model that works" means here, and what it cannot mean
//!
//! A repository holding a GGUF will load. Whether the model can *call a tool* -- which is
//! what decides whether AETHER1 can search the web for somebody -- depends on the chat
//! template baked into the file, and the listing API does not report it. Guessing from the
//! name is the only thing available before a download, so that is exactly what this does
//! and exactly what it says it is: `Support::Known` for a family whose instruct models ship
//! a tool template, `Support::None` for the families that are known not to, and
//! `Support::Unknown` for everything else, which is most of the hub.
//!
//! The authoritative answer arrives after the pull, from the server, for free:
//! `providers::ollama_facts` asks Ollama what the installed model can actually do. So a
//! wrong guess here misorders a list; it cannot make the companion claim a capability it
//! does not have.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The listing API. Documented, keyless, and rate-limited per address rather than per
/// token, which is what makes it usable from an app with no account behind it.
const API: &str = "https://huggingface.co/api/models";

/// How many repositories to ask for. The list is scrolled, not paged, and past about this
/// many the results stop being things anybody has heard of.
const LIMIT: usize = 40;

/// Pulling by `hf.co/...` needs a GGUF, and a repository can hold thirty of them at
/// different quantisations. Past this many the chooser is noise.
const MAX_QUANTS: usize = 12;

const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Whether a family's instruct models can emit a tool call. Three states, because two
/// would mean recording a guess as a fact -- see the module note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Support {
    /// This family's instruct models ship a chat template that can emit a tool call.
    Known,
    /// This family is known not to. Picking one means a companion that cannot look
    /// anything up, however well it writes.
    None,
    /// Not recognised. It may well work; nothing here can say so before it is downloaded.
    Unknown,
}

/// Families whose instruct releases carry a tool template. Matched against a lowercased
/// repository id, so `bartowski/Qwen2.5-7B-Instruct-GGUF` matches on `qwen2.5`.
const CAN_CALL_TOOLS: &[&str] = &[
    "qwen3",
    "qwen2.5",
    "qwen2-5",
    "llama-3.1",
    "llama-3.2",
    "llama-3.3",
    "llama3.1",
    "llama3.2",
    "llama3.3",
    "mistral-7b-instruct-v0.3",
    "mistral-small",
    "mistral-nemo",
    "ministral",
    "hermes-3",
    "hermes-2-pro",
    "functionary",
    "firefunction",
    "watt-tool",
    "granite-3",
    "command-r",
    "smollm2",
    "phi-4-mini",
    "glm-4",
    "devstral",
];

/// Families that do not, and that somebody is likely to pick anyway because they are good
/// at everything else. Checked before `CAN_CALL_TOOLS` is, so a DeepSeek R1 distill of a
/// Qwen is reported as what it is rather than as its base.
const CANNOT_CALL_TOOLS: &[&str] = &[
    "deepseek-r1-distill",
    "deepseek-r1",
    "gemma-2",
    "gemma-3",
    "gemma2",
    "gemma3",
    "codegemma",
    "mixtral",
    "phi-3",
    "tinyllama",
    "stablelm",
];

/// What this knows about one family, from its name alone.
pub fn tool_support(repo_id: &str) -> Support {
    let id = repo_id.to_lowercase();
    if CANNOT_CALL_TOOLS.iter().any(|family| id.contains(family)) {
        return Support::None;
    }
    if CAN_CALL_TOOLS.iter().any(|family| id.contains(family)) {
        return Support::Known;
    }
    Support::Unknown
}

/// One repository, as much as a listing says about it.
#[derive(Debug, Clone, Serialize)]
pub struct Repo {
    /// `owner/name`, which is both the display name and half of the pull name.
    pub id: String,
    /// Who published it, split out because for GGUF conversions the owner is most of the
    /// signal -- `bartowski` and `unsloth` are repackagers of other people's models, and
    /// knowing that is how a list of forty near-identical names becomes readable.
    pub owner: String,
    pub downloads: u64,
    pub likes: u64,
    /// When it was last touched, as the API spells it. Shown rather than parsed: a stale
    /// repository is worth noticing and the exact day is not worth a date library.
    pub updated: Option<String>,
    pub tools: Support,
    /// The parameter count out of the name, when it is in there -- "7B", "30B-A3B". Most
    /// GGUF repositories put it in the title and nowhere machine-readable.
    pub params: Option<String>,
    pub gated: bool,
}

/// One downloadable file, and the name that pulls it.
#[derive(Debug, Clone, Serialize)]
pub struct Quant {
    /// The quantisation as the filename spells it: `Q4_K_M`, `IQ3_XXS`, `BF16`.
    pub label: String,
    pub file: String,
    /// What to hand `ollama pull`.
    pub pull: String,
    /// Whether this is the one to take unless there is a reason not to. Q4_K_M is the
    /// default for the same reason every local-model guide says so: it is the smallest
    /// quantisation that does not visibly cost quality.
    pub preferred: bool,
    /// The download, in bytes, as the repository reports it. `None` when it does not --
    /// and then every figure below is `None` too rather than estimated from nothing.
    pub bytes: Option<u64>,
    /// What this needs in memory to run, in gigabytes.
    ///
    /// The weights have to be resident, plus room for the context and the runtime's own
    /// working set. A fifth on top of the file, with a floor, is the rule of thumb every
    /// local-model guide uses and it is close enough for the question being asked -- which
    /// is "will this fit on my machine", not "how many bytes exactly".
    pub needs_gb: Option<f64>,
    /// Whether this machine has the memory, and whether a card here has room for the whole
    /// thing. Both `None` when the size is unknown; `fits_on_gpu` is also `None` when there
    /// is no discrete card to speak of -- which is not the same as "it does not fit", and
    /// the panel says so differently.
    pub fits: Option<bool>,
    pub fits_on_gpu: Option<bool>,
}

/// How much room a model wants beyond its own weights: context, the runtime, and the
/// headroom that stops a machine swapping on the first long answer.
const RUNTIME_OVERHEAD: f64 = 1.2;
/// A floor for it, because a fifth of a 400 MB file is not enough overhead for anything.
const MIN_OVERHEAD_GB: f64 = 1.0;

/// What one quantisation needs in memory, from the size of the file.
pub fn memory_needed_gb(bytes: u64) -> f64 {
    let weights = bytes as f64 / 1_000_000_000.0;
    (weights * RUNTIME_OVERHEAD).max(weights + MIN_OVERHEAD_GB)
}

/// What this machine has to judge a model against.
///
/// Passed in rather than probed here so the sizing is a pure function of two numbers and
/// can be tested without a machine. The same two fractions `setup::models_for` uses, and
/// for the same reasons stated there: the OS and the HUD are already resident, and a card
/// is holding the compositor's framebuffers and the HUD's own scene.
#[derive(Debug, Clone, Copy, Default)]
pub struct Machine {
    pub ram_total_gb: f64,
    pub vram_gb: Option<f64>,
}

impl Machine {
    fn usable_ram(&self) -> f64 {
        self.ram_total_gb * 0.7
    }

    fn usable_vram(&self) -> Option<f64> {
        self.vram_gb.filter(|gb| *gb > 0.0).map(|gb| gb * 0.9)
    }
}

#[derive(Deserialize)]
struct ListedRepo {
    #[serde(default)]
    id: String,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    likes: u64,
    #[serde(default, rename = "lastModified")]
    last_modified: Option<String>,
    #[serde(default)]
    gated: Value,
}

/// The URL for one search. `filter=gguf` is the whole reason this is usable: it is the tag
/// Hugging Face puts on a repository holding GGUF files, which is exactly the set Ollama
/// can pull from, so a listing never offers something that cannot be installed.
pub fn search_url(query: &str, limit: usize) -> String {
    let query = query.trim();
    let mut url = format!(
        "{API}?filter=gguf&sort=downloads&direction=-1&limit={}",
        limit.clamp(1, 100)
    );
    if !query.is_empty() {
        url.push_str(&format!("&search={}", urlencoding::encode(query)));
    }
    url
}

/// Searches Hugging Face for GGUF repositories.
///
/// An empty query is the hub's front page: the most downloaded GGUF repositories there
/// are. That is a deliberate default rather than an empty box -- somebody who does not
/// know what to search for is precisely the person this is for.
pub fn search(query: &str) -> Result<Vec<Repo>, String> {
    let url = search_url(query, LIMIT);
    let body: Value = crate::net::get(&url, "Hugging Face was not searched")?
        .config()
        .timeout_global(Some(PROBE_TIMEOUT))
        .build()
        .call()
        .map_err(|e| format!("cannot reach Hugging Face: {e}"))?
        .into_body()
        .read_json()
        .map_err(|e| format!("cannot read what Hugging Face sent: {e}"))?;

    Ok(parse_listing(&body))
}

/// Pulled out of the request so the shape of the answer can be tested without one.
pub fn parse_listing(body: &Value) -> Vec<Repo> {
    let Some(listed) = body.as_array() else {
        return Vec::new();
    };
    listed
        .iter()
        .filter_map(|entry| serde_json::from_value::<ListedRepo>(entry.clone()).ok())
        .filter(|repo| !repo.id.trim().is_empty())
        .map(|repo| Repo {
            owner: repo.id.split('/').next().unwrap_or_default().to_string(),
            tools: tool_support(&repo.id),
            params: parameter_size(&repo.id),
            // `gated` is `false` or a string naming the kind of gate, so anything that is
            // not literally false is a repository that will refuse an anonymous pull.
            gated: repo.gated != Value::Bool(false) && !repo.gated.is_null(),
            downloads: repo.downloads,
            likes: repo.likes,
            updated: repo.last_modified,
            id: repo.id,
        })
        .collect()
}

/// The parameter count out of a repository name.
///
/// Reads `7B`, `0.5B`, `30B-A3B` and `8x7B`, and stops at the first one, because a name
/// like `Qwen3-30B-A3B-GGUF` has the interesting number first and a quantisation after it.
pub fn parameter_size(id: &str) -> Option<String> {
    let name = id.rsplit('/').next().unwrap_or(id);
    let chars: Vec<char> = name.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        if !chars[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let start = index;
        while index < chars.len() && (chars[index].is_ascii_digit() || chars[index] == '.') {
            index += 1;
        }
        // `8x7B`: the multiplier is part of the figure, not a separator.
        if index < chars.len() && (chars[index] == 'x' || chars[index] == 'X') {
            index += 1;
            while index < chars.len() && chars[index].is_ascii_digit() {
                index += 1;
            }
        }
        if index < chars.len() && (chars[index] == 'b' || chars[index] == 'B') {
            // A digit immediately after the B is a version string, not a size: `3b1` is
            // not three billion anything.
            let after = chars.get(index + 1);
            if !after.is_some_and(|c| c.is_ascii_alphanumeric()) {
                let figure: String = chars[start..index].iter().collect();
                // "1" out of "Llama-1B" is a size; "1" out of "v1B" is not worth the risk
                // of being wrong about, and a leading dot is a parse that went wrong.
                if !figure.starts_with('.') {
                    return Some(format!("{}B", figure.to_uppercase()));
                }
            }
        }
        index += 1;
    }
    None
}

/// Lists the GGUF files in one repository, with the name that pulls each and what each
/// one costs to download and to run.
///
/// `?blobs=true` is the whole reason the sizes are here: the plain repository document
/// lists filenames and nothing else, so a picker built on it could offer twelve
/// quantisations between 400 MB and 30 GB with no way to tell which was which. The
/// operator asking "will this run on my machine" was being asked to guess.
pub fn quants(repo_id: &str, machine: Machine) -> Result<Vec<Quant>, String> {
    let repo_id = repo_id.trim().trim_matches('/');
    if repo_id.is_empty() || !repo_id.contains('/') {
        return Err("a Hugging Face model is named owner/repository".to_string());
    }
    let url = format!("{API}/{repo_id}?blobs=true");
    let body: Value = crate::net::get(&url, "the model's files were not listed")?
        .config()
        .timeout_global(Some(PROBE_TIMEOUT))
        .build()
        .call()
        .map_err(|e| format!("cannot reach Hugging Face: {e}"))?
        .into_body()
        .read_json()
        .map_err(|e| format!("cannot read what Hugging Face sent: {e}"))?;

    Ok(parse_quants(repo_id, &body, machine))
}

/// The GGUF files out of a repository document.
pub fn parse_quants(repo_id: &str, body: &Value, machine: Machine) -> Vec<Quant> {
    let mut quants: Vec<Quant> = body
        .get("siblings")
        .and_then(|s| s.as_array())
        .map(|files| {
            files
                .iter()
                .filter_map(|file| {
                    let name = file.get("rfilename").and_then(Value::as_str)?;
                    // `size` only appears with `?blobs=true`, and an older cached document
                    // or a repository that does not report it leaves this None rather than
                    // producing a figure out of nothing.
                    Some((name, file.get("size").and_then(Value::as_u64)))
                })
                .filter(|(name, _)| name.to_lowercase().ends_with(".gguf"))
                // A model split across several files is listed once, under its first part:
                // Ollama takes the whole set from the one name, and listing `-00002-of-
                // 00009` as a thing to download would be a choice nobody should make.
                .filter(|(name, _)| !is_later_shard(name))
                .filter_map(|(name, bytes)| {
                    let label = quant_label(name)?;
                    let needs_gb = bytes.map(memory_needed_gb);
                    Some(Quant {
                        pull: pull_name(repo_id, &label),
                        preferred: label.eq_ignore_ascii_case("Q4_K_M"),
                        label,
                        file: name.to_string(),
                        bytes,
                        needs_gb,
                        fits: needs_gb.map(|gb| gb <= machine.usable_ram()),
                        // None rather than false with no card: "there is no card here" and
                        // "it will not fit on the card" are different sentences.
                        fits_on_gpu: needs_gb
                            .and_then(|gb| machine.usable_vram().map(|room| gb <= room)),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    // Smallest first is the order somebody deciding reads in, and the label sorts that way
    // for the common quantisations by luck rather than by design -- so sort on the leading
    // bit count and fall back to the label.
    quants.sort_by(|a, b| {
        bit_width(&a.label)
            .cmp(&bit_width(&b.label))
            .then_with(|| a.label.cmp(&b.label))
    });
    quants.dedup_by(|a, b| a.label.eq_ignore_ascii_case(&b.label));
    quants.truncate(MAX_QUANTS);
    quants
}

/// The pull name for one quantisation. Ollama's documented form for a Hugging Face GGUF.
pub fn pull_name(repo_id: &str, label: &str) -> String {
    format!("hf.co/{repo_id}:{label}")
}

fn is_later_shard(name: &str) -> bool {
    let lowered = name.to_lowercase();
    lowered.contains("-of-") && !lowered.contains("00001-of-")
}

/// The quantisation out of a filename.
///
/// Conventionally the last dash-separated piece before `.gguf`, and conventionally is as
/// strong as it gets -- so this accepts only pieces that look like a quantisation
/// (`Q4_K_M`, `IQ3_XXS`, `F16`, `BF16`) and returns None for anything else rather than
/// offering `final` or `v2` as something to download.
pub fn quant_label(filename: &str) -> Option<String> {
    let stem = filename
        .rsplit('/')
        .next()?
        .strip_suffix(".gguf")
        .or_else(|| filename.strip_suffix(".GGUF"))?;
    let piece = stem.rsplit(['-', '.']).next()?;
    let upper = piece.to_uppercase();
    let looks_like_a_quant = (upper.starts_with('Q') || upper.starts_with("IQ"))
        && upper.chars().any(|c| c.is_ascii_digit())
        || matches!(upper.as_str(), "F16" | "F32" | "BF16" | "FP16");
    looks_like_a_quant.then_some(upper)
}

/// The leading bit count of a quantisation label, for ordering. 16-bit floats sort last
/// because they are the biggest download in the repository.
fn bit_width(label: &str) -> u32 {
    if matches!(label, "F16" | "FP16" | "BF16") {
        return 16;
    }
    if label == "F32" {
        return 32;
    }
    label
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(99)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_family_that_cannot_call_tools_is_reported_before_its_base_model() {
        // The trap this ordering exists for: an R1 distill of a Qwen contains "qwen" and
        // cannot call a tool.
        assert_eq!(
            tool_support("unsloth/DeepSeek-R1-Distill-Qwen-7B-GGUF"),
            Support::None
        );
        assert_eq!(tool_support("bartowski/gemma-3-12b-it-GGUF"), Support::None);
        assert_eq!(
            tool_support("bartowski/Qwen2.5-7B-Instruct-GGUF"),
            Support::Known
        );
        assert_eq!(tool_support("unsloth/Qwen3-14B-GGUF"), Support::Known);
        assert_eq!(
            tool_support("NousResearch/Hermes-3-Llama-3.1-8B-GGUF"),
            Support::Known
        );
    }

    /// Everything unrecognised stays unrecognised. The honest answer is the whole point --
    /// the hub is mostly models nothing here has heard of, and calling them all broken
    /// would hide the good ones while calling them all fine would be a lie.
    #[test]
    fn an_unknown_family_is_unknown_rather_than_either_answer() {
        assert_eq!(
            tool_support("somebody/Experiment-7B-GGUF"),
            Support::Unknown
        );
    }

    #[test]
    fn the_search_url_only_ever_asks_for_gguf() {
        let url = search_url("qwen3 instruct", 40);
        assert!(url.contains("filter=gguf"), "{url}");
        assert!(url.contains("search=qwen3%20instruct"), "{url}");
        assert!(url.contains("limit=40"), "{url}");
        // An empty query is the front page, not a search for nothing.
        assert!(!search_url("  ", 40).contains("search="));
        // A limit out of range is clamped rather than sent -- the API rejects a big one.
        assert!(search_url("", 500).contains("limit=100"));
    }

    #[test]
    fn a_listing_gives_up_what_it_knows_and_guesses_the_rest() {
        let body = json!([
            {
                "id": "bartowski/Qwen3-8B-GGUF",
                "downloads": 91_000,
                "likes": 412,
                "lastModified": "2026-09-01T10:00:00.000Z",
                "gated": false,
            },
            {
                "id": "meta-llama/Llama-3.1-8B-Instruct-GGUF",
                "downloads": 12,
                "likes": 3,
                "gated": "auto",
            },
            // No id: nothing can be done with it, so it is not offered.
            {"downloads": 5},
        ]);
        let repos = parse_listing(&body);
        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].owner, "bartowski");
        assert_eq!(repos[0].params.as_deref(), Some("8B"));
        assert_eq!(repos[0].tools, Support::Known);
        assert!(!repos[0].gated);
        assert!(
            repos[1].gated,
            "a gated repository refuses an anonymous pull, and saying so beforehand is the \
             difference between a choice and a failed download"
        );
    }

    #[test]
    fn the_parameter_count_comes_out_of_the_name() {
        assert_eq!(
            parameter_size("bartowski/Qwen3-8B-GGUF").as_deref(),
            Some("8B")
        );
        assert_eq!(
            parameter_size("x/Qwen2.5-0.5B-Instruct").as_deref(),
            Some("0.5B")
        );
        assert_eq!(
            parameter_size("x/Qwen3-30B-A3B-GGUF").as_deref(),
            Some("30B")
        );
        assert_eq!(
            parameter_size("x/Mixtral-8x7B-v0.1").as_deref(),
            Some("8X7B")
        );
        assert_eq!(parameter_size("x/some-model-GGUF"), None);
    }

    #[test]
    fn only_things_that_look_like_a_quantisation_are_offered() {
        assert_eq!(
            quant_label("Qwen3-8B-Q4_K_M.gguf").as_deref(),
            Some("Q4_K_M")
        );
        assert_eq!(
            quant_label("model.IQ3_XXS.gguf").as_deref(),
            Some("IQ3_XXS")
        );
        assert_eq!(quant_label("model-f16.gguf").as_deref(), Some("F16"));
        assert_eq!(quant_label("model-final.gguf"), None, "not a quantisation");
        assert_eq!(quant_label("README.md"), None);
    }

    #[test]
    fn the_files_of_a_repository_become_things_to_pull_smallest_first() {
        let body = json!({
            "siblings": [
                {"rfilename": "README.md"},
                {"rfilename": "Qwen3-8B-f16.gguf"},
                {"rfilename": "Qwen3-8B-Q4_K_M.gguf"},
                {"rfilename": "Qwen3-8B-Q8_0.gguf"},
                {"rfilename": "Qwen3-8B-Q2_K.gguf"},
                {"rfilename": "big-Q4_K_M-00002-of-00009.gguf"},
            ]
        });
        let quants = parse_quants("bartowski/Qwen3-8B-GGUF", &body, Machine::default());
        let labels: Vec<&str> = quants.iter().map(|q| q.label.as_str()).collect();
        assert_eq!(labels, ["Q2_K", "Q4_K_M", "Q8_0", "F16"]);
        assert_eq!(
            quants[1].pull, "hf.co/bartowski/Qwen3-8B-GGUF:Q4_K_M",
            "the pull name is Ollama's documented Hugging Face form"
        );
        assert!(
            quants[1].preferred,
            "Q4_K_M unless there is a reason not to"
        );
        assert!(!quants[0].preferred);
    }

    #[test]
    fn a_repository_with_no_gguf_offers_nothing_rather_than_something_broken() {
        let body = json!({"siblings": [{"rfilename": "config.json"}]});
        assert!(parse_quants("x/y", &body, Machine::default()).is_empty());
        assert!(parse_quants("x/y", &json!({}), Machine::default()).is_empty());
    }

    #[test]
    fn a_name_that_is_not_a_repository_is_refused_without_a_request() {
        assert!(quants("qwen3", Machine::default()).is_err());
        assert!(quants("", Machine::default()).is_err());
    }

    /// The sizes are the whole point of the blobs request: a repository offers a dozen
    /// quantisations between half a gigabyte and thirty, and without them the operator was
    /// being asked to guess which one their machine could run.
    #[test]
    fn a_quantisation_says_what_it_costs_to_download_and_to_run() {
        let body = json!({
            "siblings": [
                {"rfilename": "Qwen3-14B-Q4_K_M.gguf", "size": 9_000_000_000u64},
                {"rfilename": "Qwen3-14B-Q8_0.gguf", "size": 15_700_000_000u64},
            ]
        });
        // A 32 GB machine with a 16 GB card: the Q4 fits on the card, the Q8 fits in
        // memory but not on the card, which is exactly the distinction worth drawing.
        let machine = Machine {
            ram_total_gb: 32.0,
            vram_gb: Some(16.0),
        };
        let quants = parse_quants("x/Qwen3-14B-GGUF", &body, machine);
        assert_eq!(quants[0].bytes, Some(9_000_000_000));
        assert_eq!(quants[0].fits, Some(true));
        assert_eq!(quants[0].fits_on_gpu, Some(true));
        assert_eq!(quants[1].fits, Some(true));
        assert_eq!(
            quants[1].fits_on_gpu,
            Some(false),
            "15.7 GB of weights does not fit in 14.4 GB of usable card"
        );
    }

    /// A repository that does not report sizes leaves every figure absent rather than
    /// estimated from nothing -- the same rule the rest of this module follows.
    #[test]
    fn a_quantisation_with_no_reported_size_claims_nothing() {
        let body = json!({"siblings": [{"rfilename": "model-Q4_K_M.gguf"}]});
        let quants = parse_quants(
            "x/y",
            &body,
            Machine {
                ram_total_gb: 64.0,
                vram_gb: Some(24.0),
            },
        );
        assert_eq!(quants[0].bytes, None);
        assert_eq!(quants[0].needs_gb, None);
        assert_eq!(quants[0].fits, None);
        assert_eq!(quants[0].fits_on_gpu, None);
    }

    /// No dedicated card is not the same answer as "it does not fit on the card".
    #[test]
    fn a_machine_with_no_card_says_nothing_about_the_card() {
        let body = json!({"siblings": [{"rfilename": "m-Q4_K_M.gguf", "size": 1_000_000_000u64}]});
        let quants = parse_quants(
            "x/y",
            &body,
            Machine {
                ram_total_gb: 16.0,
                vram_gb: None,
            },
        );
        assert_eq!(quants[0].fits, Some(true));
        assert_eq!(quants[0].fits_on_gpu, None);
    }

    /// Overhead is a fifth on top, with a floor -- a fifth of a small file is not enough
    /// room for a context window and a runtime.
    #[test]
    fn the_memory_estimate_has_a_floor_as_well_as_a_fraction() {
        assert!((memory_needed_gb(10_000_000_000) - 12.0).abs() < 0.01);
        // 0.4 GB of weights: a fifth is 80 MB, which is not the answer.
        assert!((memory_needed_gb(400_000_000) - 1.4).abs() < 0.01);
    }
}
