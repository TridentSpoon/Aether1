// Borrowing a pair of eyes for one turn.
//
// A model that cannot see an image used to end the matter. The attachment was dropped, the
// question was asked again without it, and the operator was told to go and pick a vision
// model under The Brain -- which means abandoning the model they were talking to, for one
// message, to have a screenshot looked at. The companion was blind by configuration on a
// machine that had something on it that could see perfectly well.
//
// Nothing new is needed to do better. Ollama already answers, per model, whether it can see
// (`providers::ollama_facts(...).vision`, three answers, read off the same `/api/show` body
// as `tools` and `thinking`), the server already says which models are resident
// (`providers::loaded_models`), and `fit`'s arithmetic already knows what loading one more
// costs. So the attachment goes to a model that can see, its description comes back as
// text, and the model having the conversation answers the operator's actual question with
// that in hand. **This is a capability of owning the machine**: a client holding one
// endpoint cannot do it, because it has nothing to ask and nowhere else to send the image.
//
// Three rules it keeps, and they are the design rather than caution:
//
//   * **The description is never passed off as sight.** It arrives in the system prompt
//     under a heading that says which model wrote it, and the answering model is told, in
//     the same words, that it did not look itself. The operator gets a trace line naming
//     both models. Nothing anywhere claims the main model saw anything.
//   * **It costs nothing when it cannot help.** No vision model installed, a server too old
//     to report capabilities, a provider that is not Ollama, or a look that fails: every
//     one of those falls back to exactly the behaviour that shipped before this existed --
//     the attachment is dropped and the model is told so.
//   * **It is bounded by residency, not by hope.** The probe order prefers a model the
//     server already has in memory, and the look is paced like the judge is: a resident
//     model is given a short bound, a cold one the longer bound that belongs to a load.
//
// Ollama only, the same limit `known_blind_to_images` has and for the same reason: the
// capability list is what makes "can this model see" decidable, and no other local server
// answers it. An OpenAI-compatible server that refuses the image is handled the other way
// round -- see `Engine::without_images`, which asks this module after the refusal rather
// than before it.

use crate::llm::persona::Provider;
use crate::llm::providers::{self, MediaAttachment, Pace};

/// The model the operator has pinned to do the looking. Empty, which is the default, means
/// whatever is installed and can see.
///
/// No Settings control yet, on purpose: the automatic choice is right on a machine with one
/// vision model on it, which is every machine this has been tried on, and a control for
/// picking between two of them is worth adding when somebody has two. Same position
/// `auto_router_judge` is in.
pub const EYES_SETTING: &str = "vision_eyes_model";

/// A description of what is attached, and who wrote it.
#[derive(Debug, Clone, PartialEq)]
pub struct Seen {
    /// The model that did the looking. Named to the operator and to the answering model --
    /// an unattributed description is indistinguishable from the main model having seen it.
    pub model: String,
    pub description: String,
}

/// Which models to ask about, in the order worth asking.
///
/// Pure, so the whole of the preference is testable without a server -- the probing version
/// below is a loop over this.
///
/// The turn's own model is dropped: it is the one that cannot see, and on the refusal path
/// it is the one that just said so. The operator's pinned choice goes first wherever it is
/// installed. Then the models the server already has in memory, because a description from
/// a resident model costs a round trip and one from a cold model costs a load -- and on a
/// 16 GB card a load can turn out the model the reply is about to use. Everything else
/// follows in the order the server listed it.
pub fn probe_order(
    installed: &[String],
    turn_model: &str,
    loaded: &[String],
    stored: &str,
) -> Vec<String> {
    let same = |a: &str, b: &str| a.trim().eq_ignore_ascii_case(b.trim());
    let usable: Vec<&String> = installed
        .iter()
        .filter(|name| !name.trim().is_empty() && !same(name, turn_model))
        .collect();
    let resident = |name: &str| loaded.iter().any(|held| same(held, name));

    let mut order: Vec<String> = Vec::new();
    let push = |name: &str, order: &mut Vec<String>| {
        if !order.iter().any(|seen| same(seen, name)) {
            order.push(name.to_string());
        }
    };
    if let Some(pinned) = usable.iter().find(|name| same(name, stored)) {
        push(pinned, &mut order);
    }
    for name in usable.iter().filter(|name| resident(name)) {
        push(name, &mut order);
    }
    for name in &usable {
        push(name, &mut order);
    }
    order
}

/// Finds a model that can see and asks it what is attached.
///
/// `None` for every way this can decline, and declining is always safe: the caller's
/// fallback is the behaviour that shipped before this existed. The probing stops at the
/// first model the server says can see, so the usual cost on a machine with one vision
/// model installed is one `/api/show` (cached per process after that) and one look.
pub fn borrow(
    db: &crate::llm::db::MemoryDb,
    provider: Provider,
    endpoint: &str,
    turn_model: &str,
    question: &str,
    images: &[MediaAttachment],
) -> Option<Seen> {
    if provider != Provider::Ollama || images.is_empty() {
        return None;
    }
    let installed = crate::model_scanner::models_at(endpoint)?;
    let loaded = providers::loaded_models(provider, endpoint);
    let stored = db.get_setting_string(EYES_SETTING, "");
    let order = probe_order(&installed, turn_model, &loaded, &stored);

    let model = order
        .into_iter()
        .find(|name| providers::ollama_facts(endpoint, name).vision == Some(true))?;
    // Paced like a judgment: a resident model is answering now, a cold one has to be read
    // off the disk first and that wait belongs to the look rather than to a timeout.
    let pace = if loaded
        .iter()
        .any(|held| held.trim().eq_ignore_ascii_case(model.trim()))
    {
        Pace::Warm
    } else {
        Pace::Loading
    };
    let description = providers::look(endpoint, &model, question, images, pace).ok()?;
    Some(Seen { model, description })
}

/// What the answering model is told, in place of the picture it cannot see.
///
/// The description is fenced off under its own heading and attributed, and the three
/// refusals at the end are the point of the whole note: a model that is handed a
/// description and not told where it came from writes "I can see that..." , and an operator
/// reading that has been told something untrue about their own machine.
pub fn seen_note(count: usize, seen: &Seen) -> String {
    let subject = if count == 1 {
        "an image".to_string()
    } else {
        format!("{count} images")
    };
    format!(
        "\n\n[ATTACHMENT DESCRIBED BY ANOTHER MODEL]\nThe operator attached {subject} to this message. You cannot see images, so {} -- a model on this machine that can -- was asked to look and wrote the description below. The operator has been told that this is what happened.\n\n---\n{}\n---\n\nAnswer their question using that description as your only knowledge of the attachment. Say \"{}\" looked at it rather than writing as though you had seen it yourself, treat anything the description calls unclear as unknown, and never add a detail it does not contain.",
        seen.model,
        seen.description.trim(),
        seen.model,
    )
}

/// The line the operator reads above the answer: which model could not see, which one
/// looked. Both names, because "a model looked at it" invites the question this answers.
pub fn seen_trace(turn_model: &str, seen: &Seen) -> String {
    format!(
        "{turn_model} cannot see images, so {} looked at the attachment and described it -- the answer is built from that description, not from the picture",
        seen.model
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(of: &[&str]) -> Vec<String> {
        of.iter().map(|s| s.to_string()).collect()
    }

    /// A stand-in Ollama that answers one `/api/chat` and hands back what it was asked, so
    /// that what the look actually sends is checkable without a model.
    fn recording_ollama(reply: &'static str) -> (u16, std::sync::mpsc::Receiver<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
        let port = listener.local_addr().unwrap().port();
        let (sent, seen) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            // Two requests, not one: `look` asks `/api/show` whether the model can think
            // before it asks `/api/chat` anything, and only the second is the one under
            // test. A stand-in that answered once would be answering the wrong question.
            for stream in listener.incoming().take(2) {
                let Ok(mut stream) = stream else { continue };
                let mut raw = Vec::new();
                let mut buf = [0u8; 8192];
                // Read until the body is as long as the headers said. The image in these
                // tests is tiny, but a real one arrives over several reads and a test that
                // only ever saw the first would be checking the wrong string.
                let wanted = loop {
                    let read = stream.read(&mut buf).unwrap_or(0);
                    if read == 0 {
                        break 0;
                    }
                    raw.extend_from_slice(&buf[..read]);
                    let text = String::from_utf8_lossy(&raw).to_string();
                    let Some(head) = text.split("\r\n\r\n").next() else {
                        continue;
                    };
                    let length: usize = head
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|n| n.trim().parse().ok())
                        })
                        .unwrap_or(0);
                    if text.contains("\r\n\r\n") && text.len() >= head.len() + 4 + length {
                        break length;
                    }
                };
                let _ = wanted;
                let request = String::from_utf8_lossy(&raw).to_string();
                let body = if request.contains("/api/chat") {
                    let _ = sent.send(request);
                    reply.to_string()
                } else {
                    "{}".to_string()
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        (port, seen)
    }

    fn one_image() -> Vec<MediaAttachment> {
        vec![MediaAttachment {
            mime_type: "image/png".to_string(),
            data_base64: "aGVsbG8tcGl4ZWxz".to_string(),
        }]
    }

    #[test]
    fn the_look_sends_the_image_and_the_question_and_asks_for_a_description() {
        let (port, seen) = recording_ollama(r#"{"message":{"content":"A label reading A1."}}"#);
        let endpoint = format!("http://127.0.0.1:{port}");
        let description = providers::look(
            &endpoint,
            "lfm3-vl",
            "what does this label say?",
            &one_image(),
            Pace::Warm,
        )
        .expect("the stand-in answered");
        assert_eq!(description, "A label reading A1.");

        let request = seen.recv().expect("the stand-in was asked something");
        assert!(request.contains("/api/chat"));
        assert!(
            request.contains("aGVsbG8tcGl4ZWxz"),
            "the image goes with it"
        );
        assert!(request.contains("what does this label say?"));
        assert!(request.contains("do not answer the person's question yourself"));
        // Warm: naming a context size for a resident model loads a second copy of it.
        assert!(!request.contains("num_ctx"), "{request}");
    }

    #[test]
    fn a_cold_model_is_given_a_context_size_because_it_is_being_loaded_anyway() {
        let (port, seen) = recording_ollama(r#"{"message":{"content":"A photograph."}}"#);
        let endpoint = format!("http://127.0.0.1:{port}");
        providers::look(
            &endpoint,
            "lfm3-vl",
            "what is this?",
            &one_image(),
            Pace::Loading,
        )
        .expect("the stand-in answered");
        assert!(seen.recv().expect("asked").contains("num_ctx"));
    }

    #[test]
    fn an_empty_description_is_an_error_rather_than_an_empty_heading() {
        let (port, _seen) = recording_ollama(r#"{"message":{"content":"   "}}"#);
        let endpoint = format!("http://127.0.0.1:{port}");
        assert!(providers::look(
            &endpoint,
            "lfm3-vl",
            "what is this?",
            &one_image(),
            Pace::Warm
        )
        .is_err());
    }

    #[test]
    fn nothing_attached_is_never_sent_anywhere() {
        assert!(
            providers::look("http://127.0.0.1:1", "lfm3-vl", "hello", &[], Pace::Warm).is_err()
        );
        assert!(providers::look(
            "http://127.0.0.1:1",
            "  ",
            "hello",
            &one_image(),
            Pace::Warm
        )
        .is_err());
    }

    #[test]
    fn the_turn_s_own_model_is_never_asked_to_describe_what_it_cannot_see() {
        let order = probe_order(&names(&["qwen3:14b", "lfm3-vl"]), "qwen3:14b", &[], "");
        assert_eq!(order, names(&["lfm3-vl"]));
    }

    #[test]
    fn a_resident_model_is_asked_before_one_that_would_have_to_be_loaded() {
        let order = probe_order(
            &names(&["llava:13b", "lfm3-vl", "qwen3:14b"]),
            "qwen3:14b",
            &names(&["lfm3-vl"]),
            "",
        );
        assert_eq!(order, names(&["lfm3-vl", "llava:13b"]));
    }

    #[test]
    fn the_operator_s_pinned_choice_goes_first_even_when_it_is_cold() {
        let order = probe_order(
            &names(&["llava:13b", "lfm3-vl", "qwen3:14b"]),
            "qwen3:14b",
            &names(&["lfm3-vl"]),
            "llava:13b",
        );
        assert_eq!(order, names(&["llava:13b", "lfm3-vl"]));
    }

    #[test]
    fn a_pinned_model_that_is_not_installed_is_simply_not_there() {
        let order = probe_order(
            &names(&["lfm3-vl", "qwen3:14b"]),
            "qwen3:14b",
            &[],
            "moondream",
        );
        assert_eq!(order, names(&["lfm3-vl"]));
    }

    #[test]
    fn every_model_is_asked_about_once_however_the_lists_overlap() {
        let order = probe_order(
            &names(&["lfm3-vl", "lfm3-vl", "", "  "]),
            "qwen3:14b",
            &names(&["lfm3-vl", "lfm3-vl"]),
            "lfm3-vl",
        );
        assert_eq!(order, names(&["lfm3-vl"]));
    }

    #[test]
    fn a_tag_is_matched_however_it_is_cased_or_spaced() {
        let order = probe_order(
            &names(&[" LFM3-VL ", "Qwen3:14B"]),
            "qwen3:14b",
            &names(&["lfm3-vl"]),
            "",
        );
        assert_eq!(order, names(&[" LFM3-VL "]));
    }

    #[test]
    fn nothing_installed_but_the_blind_model_leaves_nothing_to_ask() {
        assert!(probe_order(&names(&["qwen3:14b"]), "qwen3:14b", &[], "").is_empty());
    }

    #[test]
    fn the_note_attributes_the_description_and_forbids_writing_as_though_it_had_looked() {
        let seen = Seen {
            model: "lfm3-vl".to_string(),
            description: "A terminal showing a panic in fit.rs.".to_string(),
        };
        let note = seen_note(1, &seen);
        assert!(note.contains("an image"));
        assert!(note.contains("lfm3-vl"));
        assert!(note.contains("A terminal showing a panic in fit.rs."));
        assert!(note.contains("never add a detail it does not contain"));
        assert!(note.contains("rather than writing as though you had seen it yourself"));
    }

    #[test]
    fn the_note_counts_the_attachments_it_is_speaking_about() {
        let seen = Seen {
            model: "lfm3-vl".to_string(),
            description: "Two photographs.".to_string(),
        };
        assert!(seen_note(3, &seen).contains("3 images"));
    }

    #[test]
    fn the_trace_names_both_models_so_the_operator_can_tell_who_saw_what() {
        let seen = Seen {
            model: "lfm3-vl".to_string(),
            description: "A label.".to_string(),
        };
        let trace = seen_trace("qwen3:14b", &seen);
        assert!(trace.contains("qwen3:14b cannot see images"));
        assert!(trace.contains("lfm3-vl looked at the attachment"));
    }
}
