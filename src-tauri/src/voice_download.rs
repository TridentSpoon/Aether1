//! Fetching a Piper voice, so that a human does not have to.
//!
//! Step 36 made the two ways a voice fails legible: `piper` is the name of two unrelated
//! programs, and a voice is two files where only one of them looks important. Naming a
//! trap is not the same as removing it, and the second one was still there -- the wizard's
//! own advice ended with *"download the .onnx file AND the small .onnx.json sitting next to
//! it"*, which is a sentence that exists only because the operation is easy to get wrong.
//!
//! So this does it instead. Both files, into one folder, with the same validity rule the
//! rest of tts.rs already enforces applied at the end rather than hours later at the moment
//! of speaking.
//!
//! **This fetches voices and never programs.** A `.onnx` is weights -- numbers handed to an
//! engine the operator installed through their own package manager. Nothing downloaded here
//! is ever executed, marked executable, or placed on a PATH, which is the entire reason this
//! half was worth automating while the engine half was not. Corrupt the download and the
//! worst outcome is that speech sounds wrong; corrupt an engine download and the worst
//! outcome is code running as the operator. Those are not the same risk and are not treated
//! as one.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::{LazyLock, Mutex};

use serde::Serialize;

use crate::downloads::Phase;
use crate::llm::MemoryDb;

/// Where the voices come from: the Piper project's own collection on Hugging Face, the same
/// place scripts/package_offline_linux.sh takes the bundled voice from.
const VOICE_BASE_URL: &str = "https://huggingface.co/rhasspy/piper-voices/resolve/main";

/// A ceiling on the settings sidecar, which is a few kilobytes of JSON. Generous enough to
/// never refuse a real one, small enough that a server answering this request with something
/// enormous cannot be read into memory.
const MAX_SIDECAR_BYTES: u64 = 1024 * 1024;

/// A ceiling on what one voice may write to disk, well clear of the largest published voice
/// (the `high` models run to a bit over a hundred megabytes). A server that answers a voice
/// request with something enormous should not be able to fill the disk while a progress bar
/// reports cheerful progress.
const MAX_VOICE_BYTES: u64 = 400 * 1024 * 1024;

/// One voice on offer.
#[derive(Debug, Clone, Serialize)]
pub struct Voice {
    /// The Piper name, which is also both filenames and the key everything here is keyed by.
    pub name: &'static str,
    /// How a person would describe it, since `en_GB-alba-medium` does not say "Scottish".
    pub label: &'static str,
    /// Roughly how big, for the decision made before the real figure is known. The bar uses
    /// the server's own Content-Length once bytes are moving; this is only the hint shown
    /// beforehand, and it is approximate on purpose rather than precise and stale.
    pub size_hint: &'static str,
    /// Whether this one is already sitting in the voices folder, complete.
    pub installed: bool,
    /// Where it lives, or would live. Shown in the hub, and what the "use this one"
    /// button puts in the Piper voice-file box -- so that field is something a person can
    /// fill by pressing a button rather than by typing a path they have to go and find.
    pub path: String,
}

/// The voices offered, chosen rather than listed.
///
/// Hugging Face carries several hundred across every language, and a wall of them is its own
/// kind of unhelpful -- the operator's complaint was never that there was too little choice.
/// These are English, `medium` quality (the tier that sounds natural without being slow on a
/// laptop), and spread across accents so the list answers "which of these sounds like me?"
///
/// **Being a fixed table is a security property, not just an editorial one.** The name the
/// HUD sends is looked up here and the URL is built from what this table says; a name that
/// is not in it is refused. Nothing the operator or the page can type becomes part of a URL
/// or a file path, so there is no request to redirect and no directory to traverse out of.
const CATALOGUE: &[(&str, &str, &str, &str)] = &[
    (
        "en_GB-alba-medium",
        "British, Scottish, female",
        "about 60 MB",
        "en/en_GB/alba/medium",
    ),
    (
        "en_GB-northern_english_male-medium",
        "British, northern English, male",
        "about 60 MB",
        "en/en_GB/northern_english_male/medium",
    ),
    (
        "en_US-amy-medium",
        "American, female",
        "about 60 MB",
        "en/en_US/amy/medium",
    ),
    (
        "en_US-lessac-medium",
        "American, female, clear and neutral",
        "about 60 MB",
        "en/en_US/lessac/medium",
    ),
    (
        "en_US-ryan-medium",
        "American, male",
        "about 60 MB",
        "en/en_US/ryan/medium",
    ),
];

fn find(name: &str) -> Option<(&'static str, &'static str, &'static str, &'static str)> {
    CATALOGUE.iter().copied().find(|(n, _, _, _)| *n == name)
}

/// The catalogue, each entry saying whether it is already here.
///
/// "Already here" is the same question `piper_voice` asks -- both files, big enough --
/// rather than "a file with this name exists". A half-finished voice must not read as
/// installed, or the button that would fix it is the one thing the page hides.
pub fn catalogue() -> Vec<Voice> {
    let dir = crate::llm::tts::voices_dir();
    CATALOGUE
        .iter()
        .map(|(name, label, size_hint, _)| {
            let file = dir.join(format!("{name}.onnx"));
            Voice {
                name,
                label,
                size_hint,
                installed: crate::llm::tts::voice_problem(&file).is_none(),
                path: file.display().to_string(),
            }
        })
        .collect()
}

/// What one voice download has reported so far. Deliberately the same shape as
/// `downloads::Download` so the HUD's existing progress row renders it unchanged.
#[derive(Debug, Clone, Serialize)]
pub struct VoiceFetch {
    pub voice: String,
    pub phase: Phase,
    pub detail: String,
    pub completed: u64,
    pub total: u64,
    pub percent: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl VoiceFetch {
    fn new(voice: &str) -> VoiceFetch {
        VoiceFetch {
            voice: voice.to_string(),
            phase: Phase::Starting,
            detail: "starting".to_string(),
            completed: 0,
            total: 0,
            percent: None,
            error: None,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self.phase, Phase::Done | Phase::Failed)
    }
}

static FETCHES: LazyLock<Mutex<Vec<VoiceFetch>>> = LazyLock::new(|| Mutex::new(Vec::new()));

/// Same reasoning as downloads::registry -- a status board is not worth taking the app down
/// over.
fn registry() -> std::sync::MutexGuard<'static, Vec<VoiceFetch>> {
    FETCHES.lock().unwrap_or_else(|e| e.into_inner())
}

fn update(voice: &str, f: impl FnOnce(&mut VoiceFetch)) -> bool {
    let mut reg = registry();
    match reg.iter_mut().find(|d| d.voice == voice) {
        Some(entry) => {
            f(entry);
            true
        }
        // Cleared while running: nobody is watching, so the worker can stop.
        None => false,
    }
}

/// Starts fetching one voice by name.
///
/// Refuses in local-only mode for the same reason model downloads are refused there: that
/// switch is a promise that nothing reaches off this machine, and a voice is a download like
/// any other. Refuses a name that is not in the catalogue, which is what keeps every URL
/// built here one this file wrote.
pub fn start(db: &MemoryDb, name: &str) -> Result<VoiceFetch, String> {
    if crate::local_only::enabled(db) {
        return Err(crate::local_only::refusal("no voice was downloaded"));
    }
    let Some((name, _, _, dir)) = find(name.trim()) else {
        return Err(format!(
            "{:?} is not one of the voices Aether1 offers.",
            name.trim()
        ));
    };

    {
        let mut reg = registry();
        if let Some(existing) = reg.iter().find(|d| d.voice == name) {
            if !existing.is_terminal() {
                // A second press of the button is not an error, it is impatience.
                return Ok(existing.clone());
            }
            reg.retain(|d| d.voice != name);
        }
        if reg.iter().any(|d| !d.is_terminal()) {
            return Err(
                "A voice is already downloading. They are large files sharing one \
                 connection, so starting a second would not make either arrive sooner."
                    .to_string(),
            );
        }
        reg.push(VoiceFetch::new(name));
    }

    let target = crate::llm::tts::voices_dir();
    std::thread::spawn(move || {
        if let Err(message) = fetch(VOICE_BASE_URL, dir, name, &target) {
            update(name, |entry| {
                entry.phase = Phase::Failed;
                entry.detail = "failed".to_string();
                entry.error = Some(message);
            });
        }
    });

    Ok(registry()
        .iter()
        .find(|d| d.voice == name)
        .cloned()
        .unwrap_or_else(|| VoiceFetch::new(name)))
}

/// Fetches both halves of one voice into `target`, reporting as it goes.
///
/// `base` is a parameter rather than the constant so the tests can drive this whole path
/// against a local server. What is under test is this function, not a mock of it.
///
/// **The sidecar is fetched first, and kept in memory until the voice has landed.** It is a
/// few kilobytes against sixty megabytes, so a pair that is going to fail fails in a second
/// rather than after a long wait. Holding it back until the end means the folder never gains
/// a file from a download that did not finish -- neither half is written anywhere visible
/// until both are in hand.
fn fetch(base: &str, dir: &str, name: &str, target: &Path) -> Result<(), String> {
    std::fs::create_dir_all(target)
        .map_err(|e| format!("Could not make the voices folder {}: {e}", target.display()))?;

    update(name, |entry| {
        entry.phase = Phase::Starting;
        entry.detail = "fetching the settings file".to_string();
    });

    let sidecar = format!("{name}.onnx.json");
    let sidecar_bytes = get_small(&format!("{base}/{dir}/{sidecar}"))?;
    // It must parse, because an HTML error page saved under a .json name is a file that
    // exists, satisfies every check tts.rs makes, and breaks Piper at the moment of
    // speaking. This is the one integrity check worth making on data: not "did the bytes
    // arrive" but "are they the kind of thing we asked for".
    serde_json::from_slice::<serde_json::Value>(&sidecar_bytes).map_err(|_| {
        format!("What came back for {sidecar} was not a settings file. Try again later.")
    })?;

    update(name, |entry| {
        entry.phase = Phase::Downloading;
        entry.detail = "downloading the voice".to_string();
    });

    // Streamed to a .part file rather than held in memory. A voice is tens of megabytes and
    // there is no reason for all of it to be resident at once, but the bigger point is that
    // the partial file is *visible as a partial file*: a rename within one directory is
    // atomic on every platform this runs on, so the voices folder never contains a
    // half-written .onnx. An interrupted download leaves a .part, which everything in tts.rs
    // ignores, instead of a plausible-looking voice that fails at the moment of speaking --
    // which is the failure this module exists to end.
    let onnx = format!("{name}.onnx");
    let onnx_part = target.join(format!("{onnx}.part"));
    if let Err(message) = stream_to_file(&format!("{base}/{dir}/{onnx}"), &onnx_part, name) {
        // A failed attempt leaves nothing to clean up by hand before pressing the button
        // again. Best-effort: if this cannot be removed, the .part is still inert.
        let _ = std::fs::remove_file(&onnx_part);
        return Err(message);
    }

    update(name, |entry| {
        entry.phase = Phase::Finishing;
        entry.detail = "saving".to_string();
        entry.percent = None;
    });

    // The sidecar lands first: tts.rs judges a voice by its .onnx and then looks beside it,
    // so making the .onnx visible last means the pair is never briefly half present.
    write_file(&target.join(&sidecar), &sidecar_bytes)?;
    rename(&onnx_part, &target.join(&onnx))?;

    // Judged by the same rule everything else uses, rather than by "the transfer returned
    // Ok". A voice that cannot be used is a failed download however cleanly it arrived.
    if let Some(problem) = crate::llm::tts::voice_problem(&target.join(&onnx)) {
        update(name, |entry| {
            entry.phase = Phase::Failed;
            entry.detail = "failed".to_string();
            entry.error = Some(problem);
        });
        return Ok(());
    }

    update(name, |entry| {
        entry.phase = Phase::Done;
        entry.detail = "ready".to_string();
        if entry.total > 0 {
            entry.completed = entry.total;
        }
        entry.percent = Some(100);
    });
    Ok(())
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("Could not write {}: {e}", path.display()))
}

fn rename(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::rename(from, to).map_err(|e| format!("Could not save {}: {e}", to.display()))
}

/// Opens a URL and returns its reader along with the size it announced, refusing an
/// announced size no voice could have before a byte of the body is accepted.
fn open(url: &str) -> Result<(Box<dyn Read + Send + Sync>, u64), String> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| format!("Could not fetch the voice: {e}"))?;
    let total: u64 = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if total > MAX_VOICE_BYTES {
        return Err(format!(
            "That download says it is {} MB, which is far larger than any Piper voice.              Nothing was saved.",
            total / (1024 * 1024)
        ));
    }
    Ok((Box::new(response.into_body().into_reader()), total))
}

/// Fetches one small file into memory. Only used for the settings sidecar, which is a few
/// kilobytes and has to be parsed before it is worth writing anything.
fn get_small(url: &str) -> Result<Vec<u8>, String> {
    let (reader, _) = open(url)?;
    let mut out = Vec::new();
    reader
        .take(MAX_SIDECAR_BYTES)
        .read_to_end(&mut out)
        .map_err(|e| format!("The download stopped early: {e}"))?;
    Ok(out)
}

/// Streams one URL into `dest`, reporting progress as it goes.
///
/// The ceiling is enforced *while* the bytes arrive as well as from the announced size,
/// because a server that lies in its Content-Length header, or omits it, is exactly the
/// case the announced-size check cannot cover.
fn stream_to_file(url: &str, dest: &Path, voice: &str) -> Result<(), String> {
    let (mut reader, total) = open(url)?;
    update(voice, |entry| entry.total = total);

    let mut file = std::fs::File::create(dest)
        .map_err(|e| format!("Could not write {}: {e}", dest.display()))?;
    let mut buffer = vec![0u8; 64 * 1024];
    let mut written: u64 = 0;
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|e| format!("The download stopped early: {e}"))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .map_err(|e| format!("Could not write {}: {e}", dest.display()))?;
        written += read as u64;
        if written > MAX_VOICE_BYTES {
            return Err(
                "That download kept going past any plausible size for a voice, so it was                  stopped. Nothing was saved."
                    .to_string(),
            );
        }
        let done = written;
        let still_watched = update(voice, |entry| {
            entry.completed = done;
            entry.percent = (total > 0).then(|| ((done.min(total) * 100) / total) as u8);
        });
        if !still_watched {
            return Err("The download was cleared while it was running.".to_string());
        }
    }
    // A server that announced a size and then sent less is the truncated-download case
    // arriving over the wire instead of off the disk. Caught here so the .part is discarded
    // rather than renamed into place and judged later.
    if total > 0 && written < total {
        return Err(format!(
            "The download stopped early -- {} of {} MB arrived. Nothing was saved;              press Download to try again.",
            written / (1024 * 1024),
            total / (1024 * 1024)
        ));
    }
    file.sync_all()
        .map_err(|e| format!("Could not finish writing {}: {e}", dest.display()))?;
    Ok(())
}

/// Everything the registry holds, ordered by name so the list does not shuffle between polls.
pub fn snapshot() -> Vec<VoiceFetch> {
    let mut all = registry().clone();
    all.sort_by(|a, b| a.voice.cmp(&b.voice));
    all
}

/// Drops one finished or failed row. Same contract as downloads::forget: a running one is
/// left alone, because this is a "clear this row" and not a cancel.
pub fn forget(voice: &str) -> bool {
    let mut reg = registry();
    let removable = reg.iter().any(|d| d.voice == voice && d.is_terminal());
    if removable {
        reg.retain(|d| d.voice != voice);
    }
    removable
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::path::PathBuf;

    /// One canned reply per request, served in order, by a real socket.
    ///
    /// The alternative was to factor the HTTP out behind a trait and hand the tests a fake.
    /// That would test the wiring around the download rather than the download: the parts
    /// most worth pinning here are the ones that only exist because a real server is on the
    /// other end -- a Content-Length that disagrees with the body, a connection that stops
    /// mid-file, an error page arriving where a settings file was asked for.
    fn serve(replies: Vec<(&'static str, Vec<u8>)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for (i, (status_and_headers, body)) in replies.into_iter().enumerate() {
                let Ok((mut socket, _)) = listener.accept() else {
                    return;
                };
                // The request is read only far enough to let the client finish sending it;
                // which URL was asked for does not matter, because the replies are ordered.
                let mut scratch = [0u8; 2048];
                let _ = socket.read(&mut scratch);
                let _ = socket.write_all(status_and_headers.as_bytes());
                let _ = socket.write_all(&body);
                let _ = socket.flush();
                let _ = i;
            }
        });
        base
    }

    /// A 200 that ends by closing the connection. No Content-Length, which is also the
    /// "the server told us nothing about the size" case the progress code has to survive.
    fn sized_reply(body: Vec<u8>) -> (&'static str, Vec<u8>) {
        ("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n", body)
    }

    fn scratch_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("aether1_voice_dl_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir should be creatable");
        dir
    }

    /// Seeds the registry so `fetch` has somewhere to report, exactly as `start` would.
    fn watching(name: &'static str) {
        let mut reg = registry();
        reg.retain(|d| d.voice != name);
        reg.push(VoiceFetch::new(name));
    }

    fn phase_of(name: &str) -> Phase {
        registry()
            .iter()
            .find(|d| d.voice == name)
            .map(|d| d.phase)
            .expect("the entry should still be there")
    }

    /// A body big enough to clear MIN_VOICE_BYTES, which is what tts.rs uses to tell a voice
    /// from a truncated download.
    fn plausible_voice() -> Vec<u8> {
        vec![7u8; 1_200_000]
    }

    #[test]
    fn both_halves_land_and_the_result_is_a_voice_that_would_be_used() {
        let name = "test-happy";
        let dir = scratch_dir("happy");
        watching(name);
        let base = serve(vec![
            sized_reply(br#"{"audio":{"sample_rate":22050}}"#.to_vec()),
            sized_reply(plausible_voice()),
        ]);

        fetch(&base, "en/test", name, &dir).expect("the fetch should succeed");

        assert_eq!(phase_of(name), Phase::Done);
        // Judged by the same rule that decides what gets spoken with, not by "a file exists".
        assert!(crate::llm::tts::voice_problem(&dir.join(format!("{name}.onnx"))).is_none());
        assert!(dir.join(format!("{name}.onnx.json")).is_file());
        // No debris: a .part left behind would be a file the operator has to understand.
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".part"))
            .collect();
        assert!(leftovers.is_empty(), "left behind {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The failure this guards is specific: Hugging Face answering with an HTML error page,
    /// which is a 200 with a body, saved under a .json name. It exists, it is the right size,
    /// and every check tts.rs makes passes -- and then Piper fails at the moment of speaking,
    /// which is the exact silent failure this module was built to end.
    #[test]
    fn an_error_page_where_the_settings_file_should_be_is_refused() {
        let name = "test-htmljson";
        let dir = scratch_dir("htmljson");
        watching(name);
        let base = serve(vec![sized_reply(
            b"<html><body>Not Found</body></html>".to_vec(),
        )]);

        let result = fetch(&base, "en/test", name, &dir);

        assert!(
            result.is_err(),
            "an HTML settings file should not be accepted"
        );
        // And crucially nothing was written: the big file is never fetched, so the folder
        // cannot end up holding a lone .onnx.
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A connection that dies partway through the big file must not leave something that
    /// looks like a voice. This is the half-downloaded case from step 36, arriving by a
    /// different route.
    #[test]
    fn a_download_cut_off_partway_leaves_no_voice_behind() {
        let name = "test-cutoff";
        let dir = scratch_dir("cutoff");
        watching(name);
        // Content-Length promises far more than is sent, then the socket closes.
        let base = serve(vec![
            sized_reply(br#"{"audio":{"sample_rate":22050}}"#.to_vec()),
            (
                "HTTP/1.1 200 OK\r\nContent-Length: 1200000\r\nConnection: close\r\n\r\n",
                vec![7u8; 4096],
            ),
        ]);

        let result = fetch(&base, "en/test", name, &dir);

        let why = result.expect_err("a truncated transfer is a failed download");
        assert!(
            why.contains("stopped early"),
            "the reason should name what happened, got {why:?}"
        );
        // Not "no voice was used" but "no voice was left": the folder must be as it was.
        let left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            left.is_empty(),
            "a cut-off download should leave nothing behind, found {left:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The ceiling is enforced from the announced size, before any of it is accepted.
    #[test]
    fn a_download_larger_than_any_voice_is_refused_before_it_starts() {
        let name = "test-huge";
        let dir = scratch_dir("huge");
        watching(name);
        let base = serve(vec![
            sized_reply(br#"{"audio":{"sample_rate":22050}}"#.to_vec()),
            (
                "HTTP/1.1 200 OK\r\nContent-Length: 999999999999\r\nConnection: close\r\n\r\n",
                vec![],
            ),
        ]);

        let result = fetch(&base, "en/test", name, &dir);

        assert!(result.is_err(), "an implausible size should be refused");
        assert!(!dir.join(format!("{name}.onnx")).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Nothing the HUD sends becomes part of a URL or a path. This is the property that
    /// makes the fixed table a security boundary rather than an editorial choice, so it is
    /// pinned rather than left to be obvious from reading `start`.
    #[test]
    fn a_name_outside_the_catalogue_is_refused() {
        for attempt in [
            "../../../etc/passwd",
            "en_GB-alba-medium/../../../tmp/x",
            "https://example.com/evil",
            "",
        ] {
            assert!(
                find(attempt).is_none(),
                "{attempt:?} should not resolve to a voice"
            );
        }
    }

    #[test]
    fn every_catalogue_entry_is_a_plain_name_and_a_path_under_the_voice_collection() {
        for (name, label, size, dir) in CATALOGUE {
            assert!(
                !name.contains('/') && !name.contains(".."),
                "{name} would not be safe as a filename"
            );
            assert!(
                !dir.contains("..") && !dir.starts_with('/'),
                "{dir} would not stay under the voice collection"
            );
            assert!(!label.is_empty() && !size.is_empty());
        }
    }

    #[test]
    fn the_catalogue_reports_a_voice_that_is_not_there_as_not_installed() {
        // The machine running the tests has no voices folder of its own to speak of; what
        // matters is that the answer comes from voice_problem rather than from a name match.
        for voice in catalogue() {
            let expected = crate::llm::tts::voice_problem(
                &crate::llm::tts::voices_dir().join(format!("{}.onnx", voice.name)),
            )
            .is_none();
            assert_eq!(voice.installed, expected);
        }
    }

    #[test]
    fn a_finished_row_can_be_cleared_and_a_running_one_cannot() {
        let name = "test-forget";
        watching(name);
        assert!(!forget(name), "a running download is not a row to clear");
        update(name, |entry| entry.phase = Phase::Done);
        assert!(forget(name));
        assert!(
            !forget(name),
            "clearing twice is not an error but is not a change"
        );
    }
}
