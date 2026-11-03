//! Hearing a voice before choosing it.
//!
//! The picker used to be fourteen rows of text, four of which read "American, male". There
//! is no reading of that list that answers the only question anyone brings to it, and the
//! only way to find out was to spend sixty megabytes and a few minutes on a guess, then do
//! it again. A list of voices you cannot hear is a list of filenames.
//!
//! So each row plays. Which clip it plays depends on what is already on the machine, and
//! the difference is worth being exact about, because the two are not equally truthful:
//!
//!   * **A voice that is installed is spoken here, now, by Piper.** Same engine, same
//!     model file, same pronunciation list the operator has been editing -- so what the
//!     button plays is literally what the companion will sound like, not an approximation
//!     of it. Nothing leaves the machine, and it works with local-only mode on.
//!
//!   * **A voice that is not installed plays the Piper project's own published sample.**
//!     There is no way to synthesize a voice whose weights are not here, so the honest
//!     alternatives were a published clip or nothing. It is a download like the voice
//!     itself: same egress gate, same refusal under local-only mode, and the URL is built
//!     from `voice_download`'s fixed table rather than from anything the page sent.
//!
//! The page is told which of the two it got (`source`), because "this is the voice" and
//! "this is a recording of the voice reading somebody else's sentence" are different
//! claims and only one of them is about this computer.
//!
//! **Samples are cached.** A preview is a hundred kilobytes, but it is also a button
//! people press repeatedly while comparing four voices against each other, and re-fetching
//! on every press would make the one feature whose point is a quick comparison the slowest
//! thing in the panel.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::llm::LlmEngine;

/// Where the published clips live: the Piper project's own samples, the same recordings
/// its documentation site plays. Deliberately not Hugging Face -- the voice *files* are
/// there, the sample clips are not (`voices.json` lists a voice's files, and no sample is
/// among them), so this is the only place to get them from short of generating our own.
const SAMPLE_BASE_URL: &str =
    "https://raw.githubusercontent.com/rhasspy/piper-samples/master/samples";

/// A ceiling on one sample. The published clips run to about a hundred kilobytes; this is
/// a long way clear of that and still far too small to be used to fill a disk.
const MAX_SAMPLE_BYTES: u64 = 4 * 1024 * 1024;

/// What the installed voices are made to say.
///
/// One sentence, with a comma in it and a word that is not a word, because the thing worth
/// hearing before committing to a voice is its rhythm and how it handles a name -- which is
/// exactly what a single word said in isolation cannot show.
const PREVIEW_TEXT: &str =
    "Hello. I am Aurora1, and this is how I will sound when I read something back to you.";

/// One preview, ready to play.
#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    /// The catalogue name this is a preview of.
    pub voice: String,
    pub bytes: Vec<u8>,
    /// Returned rather than guessed, for the same reason `speech_clip_rust` returns one:
    /// it decides whether the blob is playable at all, and the two sources disagree --
    /// Piper writes WAV, the published samples are MP3.
    pub mime: &'static str,
    /// `spoken` when Piper said it on this machine just now, `sample` when it is the
    /// published recording. The page says which, because they mean different things.
    pub source: &'static str,
}

/// Where a fetched sample is kept between presses.
///
/// Beside the voices rather than in a temp directory: a preview is part of the voice the
/// same way the settings sidecar is, it is the folder the operator already knows about,
/// and it survives a reboot -- which a temp directory does not, making the second day of
/// comparing voices as slow as the first. In its own subfolder, so nothing scanning the
/// voices folder for `.onnx` files has a new kind of file to ignore.
fn sample_cache_dir() -> PathBuf {
    crate::llm::tts::voices_dir().join("samples")
}

fn sample_cache_path(name: &str) -> PathBuf {
    sample_cache_dir().join(format!("{name}.mp3"))
}

/// Whether these bytes are an MP3 at all.
///
/// The same check, and the same reason, as the settings sidecar being parsed before it is
/// written: what comes back from a file server when something is wrong is usually an HTML
/// error page with status 200, which is a file that exists, has a plausible size, and plays
/// as silence. Saving one would cache the failure, so the next press has no way to recover.
fn looks_like_mp3(bytes: &[u8]) -> bool {
    // An ID3 tag, or a bare frame header (0xFF then 0xEx/0xFx).
    bytes.starts_with(b"ID3") || (bytes.len() > 2 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0)
}

/// A preview of one catalogue voice.
///
/// Refuses a name the catalogue does not carry, before anything else happens: that refusal
/// is what keeps every URL built here one `voice_download`'s table wrote, and it is also
/// the only thing standing between a page-supplied string and a path on disk.
pub fn preview(engine: &LlmEngine, name: &str) -> Result<Preview, String> {
    let name = name.trim();
    let Some(dir) = crate::voice_download::remote_dir(name) else {
        return Err(format!(
            "{name:?} is not one of the voices Aurora1 offers, so there is nothing to play."
        ));
    };
    // `remote_dir` answering means the name came out of the table, so from here on it is a
    // `&'static str` in all but type and is safe to put in a path.

    if let Some(model) = crate::llm::tts::installed_catalogue_voice(name) {
        return speak_here(engine, name, &model);
    }

    let cached = sample_cache_path(name);
    if let Ok(bytes) = std::fs::read(&cached) {
        if looks_like_mp3(&bytes) {
            return Ok(Preview {
                voice: name.to_string(),
                bytes,
                mime: "audio/mpeg",
                source: "sample",
            });
        }
        // Cached rubbish from an older attempt: drop it and fetch again rather than play
        // silence forever.
        let _ = std::fs::remove_file(&cached);
    }

    let bytes = fetch_sample(SAMPLE_BASE_URL, dir)?;
    // Best-effort: a cache that cannot be written costs a fetch per press, which is worth
    // less than failing a preview that already arrived.
    if std::fs::create_dir_all(sample_cache_dir()).is_ok() {
        let _ = write_atomically(&cached, &bytes);
    }

    Ok(Preview {
        voice: name.to_string(),
        bytes,
        mime: "audio/mpeg",
        source: "sample",
    })
}

/// Says the preview line with Piper, using this exact model file.
///
/// `Engine::Local` rather than the configured engine, and the model path rather than the
/// configured voice: the question being answered is "what does *this* voice sound like",
/// so an `auto` that fell through to the cloud voice would answer a different one -- and
/// would do it by sending the line off the machine, from a button whose whole promise is
/// that the local voices are local.
fn speak_here(engine: &LlmEngine, name: &str, model: &Path) -> Result<Preview, String> {
    let cache_dir = crate::project_root().join("backend").join("audio_cache");
    // The operator's own respellings apply, because a voice that mangles the words they
    // have taught it is a voice they want to know about before downloading the next one.
    let speller = crate::speech_words::speller(engine.db());
    let path = crate::llm::tts::generate_speech_with(
        &cache_dir,
        PREVIEW_TEXT,
        crate::llm::tts::Engine::Local,
        None,
        Some(&model.to_string_lossy()),
        &speller,
    )
    .map_err(|why| format!("Piper could not say anything in that voice ({why})."))?;

    let bytes = std::fs::read(&path)
        .map_err(|e| format!("could not read the preview at {}: {e}", path.display()))?;
    let mime = match path.extension().and_then(|e| e.to_str()) {
        Some("mp3") => "audio/mpeg",
        _ => "audio/wav",
    };
    Ok(Preview {
        voice: name.to_string(),
        bytes,
        mime,
        source: "spoken",
    })
}

/// Fetches one published sample. `base` is a parameter so the tests drive the real
/// function against a real server, the same way the download tests do.
fn fetch_sample(base: &str, dir: &str) -> Result<Vec<u8>, String> {
    let url = format!("{base}/{dir}/speaker_0.mp3");
    let response = crate::net::get(&url, "no preview was played")?
        .call()
        .map_err(|e| format!("Could not fetch the preview clip: {e}"))?;

    let announced: u64 = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if announced > MAX_SAMPLE_BYTES {
        return Err(
            "That preview clip is far larger than any sample. Nothing was played.".to_string(),
        );
    }

    let mut bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        // One more than the ceiling, so a body that ignores its own Content-Length is
        // caught by its size rather than by the header it lied in.
        .take(MAX_SAMPLE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("The preview clip stopped arriving: {e}"))?;

    if bytes.len() as u64 > MAX_SAMPLE_BYTES {
        return Err(
            "That preview clip kept going past any plausible size, so it was stopped.".to_string(),
        );
    }
    if !looks_like_mp3(&bytes) {
        return Err(
            "What came back was not an audio clip, so there is nothing to play. Try again later."
                .to_string(),
        );
    }
    Ok(bytes)
}

/// Writes through a temporary file in the same directory, so a press interrupted halfway
/// never leaves a half-written clip that the next press would read, pass as an MP3 and
/// play as a click.
fn write_atomically(dest: &Path, bytes: &[u8]) -> Result<(), String> {
    let part = dest.with_extension("mp3.part");
    std::fs::write(&part, bytes).map_err(|e| format!("could not write {}: {e}", part.display()))?;
    std::fs::rename(&part, dest).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        format!("could not save {}: {e}", dest.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    /// The same shape of test server the download tests use, and for the same reason: what
    /// is worth pinning here only exists because something real is on the other end.
    fn serve(status_and_headers: &'static str, body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            if let Ok((mut socket, _)) = listener.accept() {
                let mut scratch = [0u8; 2048];
                let _ = socket.read(&mut scratch);
                let _ = socket.write_all(status_and_headers.as_bytes());
                let _ = socket.write_all(&body);
                let _ = socket.flush();
            }
        });
        base
    }

    fn mp3(len: usize) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xFB];
        bytes.resize(len.max(2), 0x44);
        bytes
    }

    #[test]
    fn a_published_sample_comes_back_as_bytes() {
        let base = serve("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n", mp3(4096));
        let got = fetch_sample(&base, "en/en_US/amy/medium").expect("the sample should arrive");
        assert_eq!(got.len(), 4096);
    }

    /// The failure that a status code cannot catch. A file server answering with an HTML
    /// error page at status 200 is the case that would otherwise be cached and played as
    /// silence from then on.
    #[test]
    fn an_error_page_is_not_accepted_as_a_clip() {
        let base = serve(
            "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n",
            b"<html><body>Not Found</body></html>".to_vec(),
        );
        let why = fetch_sample(&base, "en/en_US/amy/medium").expect_err("that is not audio");
        assert!(why.contains("not an audio clip"), "{why}");
    }

    /// A body far past the ceiling is stopped by its size, not by the header it announced.
    #[test]
    fn an_enormous_body_is_stopped_whatever_it_announced() {
        let base = serve(
            "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n",
            mp3(MAX_SAMPLE_BYTES as usize + 64),
        );
        let why = fetch_sample(&base, "en/en_US/amy/medium").expect_err("that is too big");
        assert!(why.contains("plausible size"), "{why}");
    }

    #[test]
    fn id3_and_bare_frames_both_read_as_mp3_and_html_does_not() {
        assert!(looks_like_mp3(b"ID3\x04\x00\x00"));
        assert!(looks_like_mp3(&[0xFF, 0xFB, 0x90, 0x00]));
        assert!(!looks_like_mp3(b"<html>"));
        assert!(!looks_like_mp3(b""));
    }

    /// The name is the only thing a caller controls, and it decides both a URL and a path.
    /// Anything not in the table has to be refused before either is built.
    #[test]
    fn only_catalogue_names_resolve_to_a_folder() {
        assert!(crate::voice_download::remote_dir("en_US-amy-medium").is_some());
        assert!(crate::voice_download::remote_dir("../../etc/passwd").is_none());
        assert!(crate::voice_download::remote_dir("en_US-amy-medium/../../x").is_none());
        assert!(crate::voice_download::remote_dir("").is_none());
    }

    /// The cache path stays inside the samples folder for every name the table carries --
    /// which is the property the refusal above exists to protect.
    #[test]
    fn every_cache_path_sits_under_the_samples_folder() {
        let root = sample_cache_dir();
        for voice in crate::voice_download::catalogue() {
            let path = sample_cache_path(voice.name);
            assert_eq!(
                path.parent(),
                Some(root.as_path()),
                "{} escaped",
                voice.name
            );
        }
    }

    #[test]
    fn the_preview_line_is_a_sentence_worth_hearing() {
        assert!(PREVIEW_TEXT.contains(','), "no phrasing to hear");
        assert!(
            PREVIEW_TEXT.split_whitespace().count() > 8,
            "too short to judge"
        );
    }
}
