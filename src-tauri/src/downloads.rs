//! Model downloads you can watch while they happen.
//!
//! Spawning `ollama pull` as a child process reports nothing: the old path started it,
//! returned, and left the wizard polling the server's model list until the name appeared.
//! That is true, and for several minutes it is all it says -- which reads as a hang.
//!
//! Ollama's own HTTP API streams progress line by line, so this drives that instead and
//! keeps what it reports. Each download runs on its own thread and writes into one
//! registry; asking what is in flight is then a read, cheap enough to do every second.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::sync::{LazyLock, Mutex};

use serde::{Deserialize, Serialize};

/// How many pulls may be in flight at once.
///
/// More than this is not faster -- they share one line to the same registry -- and an
/// unbounded queue is a way to fill a disk without meaning to. Three is enough to answer
/// "can I get the small one going while the big one runs?" with yes.
pub const MAX_CONCURRENT: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// Asked for; nothing has come back yet.
    Starting,
    /// Bytes are moving, and `completed`/`total` mean something.
    Downloading,
    /// Checking, unpacking, writing the manifest -- real work with no byte count to show.
    Finishing,
    Done,
    Failed,
}

/// What one download has reported so far.
#[derive(Debug, Clone, Serialize)]
pub struct Download {
    pub model: String,
    pub phase: Phase,
    /// Ollama's own word for what it is doing right now, passed through unedited rather
    /// than mapped to something friendlier: when a download misbehaves, the phrase the
    /// operator can search for is worth more than the phrase we would have written.
    pub detail: String,
    pub completed: u64,
    pub total: u64,
    /// Whole percent, or `None` when this phase has no byte count to take one from. The
    /// HUD draws an indeterminate bar for `None`, which is the honest shape for "working,
    /// and it cannot say how far along".
    pub percent: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Download {
    fn new(model: &str) -> Download {
        Download {
            model: model.to_string(),
            phase: Phase::Starting,
            detail: "starting".to_string(),
            completed: 0,
            total: 0,
            percent: None,
            error: None,
        }
    }

    /// Whether this one has stopped moving, one way or the other.
    pub fn is_terminal(&self) -> bool {
        matches!(self.phase, Phase::Done | Phase::Failed)
    }
}

/// One line of Ollama's pull stream. Every field is optional because which of them appear
/// depends on the phase, and a line carrying only `status` is normal.
#[derive(Debug, Deserialize)]
struct PullLine {
    status: Option<String>,
    digest: Option<String>,
    total: Option<u64>,
    completed: Option<u64>,
    error: Option<String>,
}

/// Accumulates the per-layer numbers Ollama reports into one pair that only ever grows.
///
/// A model is several blobs, and the stream reports `completed`/`total` for whichever one
/// is moving -- so a bar wired straight to those figures drops back to zero at every layer
/// boundary. Keeping the last figure seen for each digest and summing them means the bar
/// only ever goes forwards. `total` can still grow as later layers are announced, which is
/// honest: until a layer is mentioned, nothing knew it was coming.
#[derive(Default)]
struct Tracker {
    layers: HashMap<String, (u64, u64)>,
}

impl Tracker {
    /// Folds one stream line into the download's public state.
    fn apply(&mut self, line: &PullLine, into: &mut Download) {
        if let Some(error) = &line.error {
            into.phase = Phase::Failed;
            into.error = Some(error.clone());
            into.detail = "failed".to_string();
            return;
        }

        if let Some(status) = &line.status {
            into.detail = status.clone();
            if status == "success" {
                into.phase = Phase::Done;
                // A finished download is shown full even if the last line carried no
                // numbers, which is the usual case -- the alternative is a bar frozen at
                // 98% above the word "success".
                if into.total > 0 {
                    into.completed = into.total;
                }
                into.percent = Some(100);
                return;
            }
        }

        // A digest with a size is a layer moving; anything else is work without a
        // measurable size, and claiming a percentage for it would be inventing one.
        match (&line.digest, line.total) {
            (Some(digest), Some(total)) if total > 0 => {
                self.layers
                    .insert(digest.clone(), (line.completed.unwrap_or(0), total));
                let (completed, total) = self
                    .layers
                    .values()
                    .fold((0u64, 0u64), |(c, t), (lc, lt)| (c + lc, t + lt));
                into.phase = Phase::Downloading;
                into.completed = completed;
                into.total = total;
                into.percent = Some(((completed.min(total) * 100) / total.max(1)) as u8);
            }
            _ => {
                if into.phase != Phase::Downloading || into.total == 0 {
                    into.phase = Phase::Finishing;
                    into.percent = None;
                }
            }
        }
    }
}

/// Every download this process has started, finished ones included until they are cleared.
static DOWNLOADS: LazyLock<Mutex<HashMap<String, Download>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// A poisoned lock here means a pump thread panicked mid-update. The registry is a status
/// board, not a source of truth about anything on disk, so carrying on with whatever is in
/// it beats taking the app down over a progress bar.
fn registry() -> std::sync::MutexGuard<'static, HashMap<String, Download>> {
    DOWNLOADS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Starts a download, unless this model is already going or too much else is.
///
/// Asking twice for the same model is not an error -- it is what a second press of the
/// button means -- so it returns the state that already exists rather than complaining.
/// The endpoint a pull request belongs at, given whatever address the operator's server
/// was discovered under.
///
/// The address handed in can carry a `/v1` suffix: `scan_local_servers` prefers the
/// OpenAI-compatible shape whenever a port answers both, so a plain Ollama install that
/// also speaks that dialect gets recorded with it. Ollama's pull API is only ever native --
/// there's no such thing as an OpenAI-compatible pull -- and it lives at the bare origin
/// regardless of which shape the operator chats through, so a `/v1` here is stripped rather
/// than sent into `/api/pull`, where it 404s.
fn native_pull_endpoint(endpoint: &str) -> String {
    let endpoint = endpoint.trim_end_matches('/');
    let endpoint = endpoint.strip_suffix("/v1").unwrap_or(endpoint);
    endpoint.trim_end_matches('/').to_string()
}

pub fn start(endpoint: &str, model: &str) -> Result<Download, String> {
    let model = model.trim().to_string();
    if model.is_empty() {
        return Err("No model was named.".to_string());
    }

    {
        let mut reg = registry();
        if let Some(existing) = reg.get(&model) {
            if !existing.is_terminal() {
                return Ok(existing.clone());
            }
            // A finished or failed one is cleared out of the way so this genuinely restarts.
            reg.remove(&model);
        }
        let active = reg.values().filter(|d| !d.is_terminal()).count();
        if active >= MAX_CONCURRENT {
            return Err(format!(
                "{active} downloads are already running, which is as many as this will do at \
                 once. They share one connection, so starting more would not make any of them \
                 finish sooner. Wait for one to land and try again."
            ));
        }
        reg.insert(model.clone(), Download::new(&model));
    }

    let endpoint = native_pull_endpoint(endpoint);
    let name = model.clone();
    std::thread::spawn(move || {
        let outcome = pump(&endpoint, &name);
        if let Err(message) = outcome {
            let mut reg = registry();
            if let Some(entry) = reg.get_mut(&name) {
                // Only if nothing else already settled it: a stream that ended in
                // "success" and then dropped the connection is a success.
                if !entry.is_terminal() {
                    entry.phase = Phase::Failed;
                    entry.detail = "failed".to_string();
                    entry.error = Some(message);
                }
            }
        }
    });

    Ok(registry()
        .get(&model)
        .cloned()
        .unwrap_or(Download::new(&model)))
}

/// Drives one pull to the end of its stream, writing progress into the registry as it goes.
fn pump(endpoint: &str, model: &str) -> Result<(), String> {
    // No global timeout: this is a download of several gigabytes, and a deadline that can
    // expire mid-transfer is exactly the bug the old five-second path had.
    let response = ureq::post(format!("{endpoint}/api/pull"))
        .send_json(serde_json::json!({ "name": model, "stream": true }))
        .map_err(|e| format!("Could not reach Ollama at {endpoint}: {e}"))?;

    let reader = BufReader::new(response.into_body().into_reader());
    let mut tracker = Tracker::default();

    for line in reader.lines() {
        let line = line.map_err(|e| format!("The download stream broke: {e}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(parsed) = serde_json::from_str::<PullLine>(&line) else {
            continue;
        };

        let mut reg = registry();
        let Some(entry) = reg.get_mut(model) else {
            // Cleared while running: nobody is watching any more, so stop reading.
            return Ok(());
        };
        tracker.apply(&parsed, entry);
        if entry.is_terminal() {
            return Ok(());
        }
    }

    // The stream ended without saying "success". Ollama does say it, so this is a
    // connection that went away rather than a download that finished quietly.
    Err(
        "The download stopped before it finished. Press Download to pick it back up -- \
         Ollama keeps what it already fetched."
            .to_string(),
    )
}

/// Everything in the registry, newest state, ordered by name so the list does not shuffle
/// itself between polls.
pub fn snapshot() -> Vec<Download> {
    let mut all: Vec<Download> = registry().values().cloned().collect();
    all.sort_by(|a, b| a.model.cmp(&b.model));
    all
}

/// Drops one finished or failed entry. A running one is left alone -- the button that
/// calls this is a "clear this row", not a cancel, and pretending otherwise would leave a
/// download running with nothing showing it.
pub fn forget(model: &str) -> bool {
    let mut reg = registry();
    match reg.get(model) {
        Some(entry) if entry.is_terminal() => {
            reg.remove(model);
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(json: &str) -> PullLine {
        serde_json::from_str(json).expect("test line should parse")
    }

    #[test]
    fn a_layer_with_a_size_gives_a_percentage() {
        let mut tracker = Tracker::default();
        let mut download = Download::new("llama3.2:3b");
        tracker.apply(
            &line(r#"{"status":"pulling abc","digest":"abc","total":1000,"completed":250}"#),
            &mut download,
        );
        assert_eq!(download.phase, Phase::Downloading);
        assert_eq!(download.percent, Some(25));
        assert_eq!(download.completed, 250);
        assert_eq!(download.total, 1000);
    }

    #[test]
    fn a_second_layer_adds_to_the_first_rather_than_replacing_it() {
        let mut tracker = Tracker::default();
        let mut download = Download::new("llama3.2:3b");
        tracker.apply(
            &line(r#"{"status":"pulling abc","digest":"abc","total":1000,"completed":1000}"#),
            &mut download,
        );
        tracker.apply(
            &line(r#"{"status":"pulling def","digest":"def","total":1000,"completed":0}"#),
            &mut download,
        );
        // The whole point: starting a new layer must not send the bar backwards.
        assert_eq!(download.completed, 1000);
        assert_eq!(download.total, 2000);
        assert_eq!(download.percent, Some(50));
    }

    #[test]
    fn progress_within_a_layer_replaces_that_layers_own_figure() {
        let mut tracker = Tracker::default();
        let mut download = Download::new("llama3.2:3b");
        for completed in [100, 200, 300] {
            tracker.apply(
                &line(&format!(
                    r#"{{"status":"pulling abc","digest":"abc","total":1000,"completed":{completed}}}"#
                )),
                &mut download,
            );
        }
        assert_eq!(
            download.completed, 300,
            "figures must replace, not accumulate"
        );
    }

    #[test]
    fn a_phase_without_a_size_claims_no_percentage() {
        let mut tracker = Tracker::default();
        let mut download = Download::new("llama3.2:3b");
        tracker.apply(&line(r#"{"status":"pulling manifest"}"#), &mut download);
        assert_eq!(download.phase, Phase::Finishing);
        assert_eq!(download.percent, None);
        assert_eq!(download.detail, "pulling manifest");
    }

    #[test]
    fn verifying_after_a_download_does_not_throw_the_bar_away() {
        let mut tracker = Tracker::default();
        let mut download = Download::new("llama3.2:3b");
        tracker.apply(
            &line(r#"{"status":"pulling abc","digest":"abc","total":1000,"completed":1000}"#),
            &mut download,
        );
        tracker.apply(
            &line(r#"{"status":"verifying sha256 digest"}"#),
            &mut download,
        );
        assert_eq!(
            download.phase,
            Phase::Downloading,
            "still the download's bar"
        );
        assert_eq!(download.percent, Some(100));
        assert_eq!(download.detail, "verifying sha256 digest");
    }

    #[test]
    fn success_finishes_the_bar_even_with_no_numbers_on_the_last_line() {
        let mut tracker = Tracker::default();
        let mut download = Download::new("llama3.2:3b");
        tracker.apply(
            &line(r#"{"status":"pulling abc","digest":"abc","total":1000,"completed":900}"#),
            &mut download,
        );
        tracker.apply(&line(r#"{"status":"success"}"#), &mut download);
        assert_eq!(download.phase, Phase::Done);
        assert_eq!(download.percent, Some(100));
        assert_eq!(download.completed, download.total);
        assert!(download.is_terminal());
    }

    #[test]
    fn an_error_line_fails_the_download_and_keeps_what_it_said() {
        let mut tracker = Tracker::default();
        let mut download = Download::new("nope:1b");
        tracker.apply(
            &line(r#"{"error":"model 'nope:1b' not found"}"#),
            &mut download,
        );
        assert_eq!(download.phase, Phase::Failed);
        assert_eq!(download.error.as_deref(), Some("model 'nope:1b' not found"));
        assert!(download.is_terminal());
    }

    #[test]
    fn a_percentage_never_exceeds_a_hundred() {
        let mut tracker = Tracker::default();
        let mut download = Download::new("llama3.2:3b");
        // Ollama has been known to report a completed figure past the total on the last
        // chunk of a layer; a bar that reads 103% looks broken even though it is nearly done.
        tracker.apply(
            &line(r#"{"status":"pulling abc","digest":"abc","total":1000,"completed":1030}"#),
            &mut download,
        );
        assert_eq!(download.percent, Some(100));
    }

    #[test]
    fn a_download_with_no_name_is_refused() {
        assert!(start("http://localhost:11434", "   ").is_err());
    }

    #[test]
    fn a_v1_suffix_is_stripped_before_the_native_pull_api() {
        // This is the shape scan_local_servers hands back for an Ollama install that also
        // answers the OpenAI-compatible probe -- exactly the case that used to send the pull
        // request to ".../v1/api/pull" and get a 404 back for it.
        assert_eq!(
            native_pull_endpoint("http://127.0.0.1:11434/v1"),
            "http://127.0.0.1:11434"
        );
        assert_eq!(
            native_pull_endpoint("http://127.0.0.1:11434/v1/"),
            "http://127.0.0.1:11434"
        );
    }

    #[test]
    fn an_endpoint_with_no_v1_suffix_is_left_alone() {
        assert_eq!(
            native_pull_endpoint("http://localhost:11434/"),
            "http://localhost:11434"
        );
        assert_eq!(
            native_pull_endpoint("http://localhost:11434"),
            "http://localhost:11434"
        );
    }

    #[test]
    fn forgetting_leaves_a_running_download_alone() {
        // Built by hand rather than started: this asserts the guard, not the network.
        registry().insert("busy:1b".to_string(), Download::new("busy:1b"));
        assert!(!forget("busy:1b"), "a running download must not be cleared");
        let mut done = Download::new("done:1b");
        done.phase = Phase::Done;
        registry().insert("done:1b".to_string(), done);
        assert!(forget("done:1b"));
        registry().remove("busy:1b");
    }
}
