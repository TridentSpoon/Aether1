// Which notes went into this answer.
//
// The vault reaches an answer by three different routes -- priming pastes the always-loaded
// notes into the system prompt, search hands back a shortlist, and read_file fetches one by
// name -- and until now none of them left a trace the operator could see. That is the gap
// this closes: an answer that quotes something about you should be able to say which note it
// came from, because the alternative is a companion whose recall is indistinguishable from
// invention.
//
// The record is a thread-local rather than a field on the engine, because a turn *is* a
// thread here: generate_response_streamed runs to completion on one blocking thread, and the
// tool loop, the providers and priming all run inside it. So two operators talking to the
// same process over the LAN cannot bleed into each other's list, and none of the three call
// sites needs a handle threaded down to it through code that otherwise has no interest in
// reporting.
//
// Recording is off unless a turn opened it. Outside `begin()` -- a test, an approval executed
// minutes later from the HUD -- `record` does nothing, so nothing accumulates in a thread
// nobody is going to drain.

use std::cell::RefCell;

use serde_json::{json, Value};

/// How many notes are worth naming under one answer. Past this the footer stops being a
/// citation and starts being a second answer to read.
const MAX_REPORTED: usize = 12;

/// How a note reached the answer. The distinction matters: primed and read notes were put in
/// front of the model in full, whereas a search hit was only offered -- it came back as a
/// name and a snippet on a shortlist the model was free to ignore.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    /// Pasted into the system prompt because it is always loaded.
    Primed,
    /// Fetched deliberately with read_file.
    Read,
    /// Returned by search as a candidate.
    Found,
}

impl How {
    fn label(self) -> &'static str {
        match self {
            How::Primed => "loaded",
            How::Read => "read",
            How::Found => "found",
        }
    }

    /// Which record survives when a note arrives twice. A note that was searched for and
    /// then actually read should report the reading: it is the stronger claim, and the one
    /// that explains the answer.
    fn rank(self) -> u8 {
        match self {
            How::Read => 2,
            How::Primed => 1,
            How::Found => 0,
        }
    }
}

/// One note, and what it did for this answer.
#[derive(Clone, Debug)]
pub struct Consulted {
    pub note: String,
    pub how: How,
}

thread_local! {
    /// `None` means no turn is open on this thread and nothing should be recorded.
    static TURN: RefCell<Option<Vec<Consulted>>> = const { RefCell::new(None) };
}

/// Opens the record for a turn, discarding anything left over from the last one.
pub fn begin() {
    TURN.with(|turn| *turn.borrow_mut() = Some(Vec::new()));
}

/// Notes that a note was consulted, if a turn is open on this thread.
pub fn record(note: &str, how: How) {
    let note = note.trim();
    if note.is_empty() {
        return;
    }
    TURN.with(|turn| {
        let mut turn = turn.borrow_mut();
        let Some(list) = turn.as_mut() else {
            return;
        };
        if let Some(existing) = list.iter_mut().find(|c| c.note == note) {
            if how.rank() > existing.how.rank() {
                existing.how = how;
            }
            return;
        }
        if list.len() < MAX_REPORTED {
            list.push(Consulted {
                note: note.to_string(),
                how,
            });
        }
    });
}

/// Closes the record and returns it. Closing is the point: what is not drained here would
/// otherwise be reported under the *next* answer as though it had been read for it.
pub fn taken() -> Vec<Consulted> {
    TURN.with(|turn| turn.borrow_mut().take())
        .unwrap_or_default()
}

/// The record as the HUD reads it.
pub fn to_json(consulted: &[Consulted]) -> Value {
    Value::Array(
        consulted
            .iter()
            .map(|c| json!({"note": c.note, "how": c.how.label()}))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each test gets its own thread, which is also the isolation the real code relies on.
    fn on_a_turn<T: Send + 'static>(body: impl FnOnce() -> T + Send + 'static) -> T {
        std::thread::spawn(move || {
            begin();
            body()
        })
        .join()
        .unwrap()
    }

    #[test]
    fn nothing_is_recorded_outside_a_turn() {
        let empty = std::thread::spawn(|| {
            record("profile.md", How::Primed);
            taken()
        })
        .join()
        .unwrap();
        assert!(
            empty.is_empty(),
            "a tool run outside a turn must not leave a note behind for the next one"
        );
    }

    #[test]
    fn a_turn_reports_what_it_consulted_in_order() {
        let notes = on_a_turn(|| {
            record("INDEX.md", How::Primed);
            record("projects/aether1.md", How::Found);
            taken()
        });
        let names: Vec<&str> = notes.iter().map(|c| c.note.as_str()).collect();
        assert_eq!(names, vec!["INDEX.md", "projects/aether1.md"]);
        assert_eq!(notes[1].how, How::Found);
    }

    #[test]
    fn a_note_that_was_found_and_then_read_reports_the_reading() {
        let notes = on_a_turn(|| {
            record("projects/aether1.md", How::Found);
            record("projects/aether1.md", How::Read);
            record("projects/aether1.md", How::Found);
            taken()
        });
        assert_eq!(notes.len(), 1, "one note must not be listed twice");
        assert_eq!(notes[0].how, How::Read);
    }

    #[test]
    fn draining_the_record_ends_the_turn() {
        let second = on_a_turn(|| {
            record("profile.md", How::Primed);
            let _first = taken();
            record("machine.md", How::Primed);
            taken()
        });
        assert!(
            second.is_empty(),
            "notes read after the turn closed belong to no answer"
        );
    }

    #[test]
    fn the_footer_is_capped() {
        let notes = on_a_turn(|| {
            for i in 0..(MAX_REPORTED + 10) {
                record(&format!("daily/{i}.md"), How::Found);
            }
            taken()
        });
        assert_eq!(notes.len(), MAX_REPORTED);
    }

    #[test]
    fn the_json_names_the_note_and_how_it_was_reached() {
        let notes = on_a_turn(|| {
            record("profile.md", How::Primed);
            taken()
        });
        assert_eq!(
            to_json(&notes),
            serde_json::json!([{"note": "profile.md", "how": "loaded"}])
        );
    }
}
