// Many pieces of work in AETHER CODE, instead of one conversation that never ends.
//
// Trident, 2026-10-13: "One thing I'd like from OpenWork is similar to Projects and threads
// here."
//
// `code_chat.rs` pins one session id, so the coding panel has had exactly one conversation
// since it was written. You cannot run two pieces of work, you cannot come back to
// yesterday's, and `clear` -- throwing the whole thing away -- is the only way out of it.
// A task is that conversation made plural: a title, a state, and a transcript of its own.
//
// **Why a settings row and not a table.** `code_chat.rs` says it, and it is still true: the
// transcript already lives in `messages`, keyed by session, and a second storage shape for
// the same thing would be two things to migrate later. What a task adds over a session is a
// handful of fields -- a state, a pin, the engine's own session id -- and fifty of those
// read together on every panel open is one row, not fifty.
//
// **Why the session id is derived and never stored.** A task's transcript lives under
// `aether-code-<id>`, worked out from the id each time. The tempting alternative is a
// `session` field, and it is wrong: a settings row is a text blob an operator can edit, and
// a stored session id is an aimable pointer -- at the companion's own conversation, at
// another task's transcript. Deriving it means the only thing that can be edited is the id,
// `valid_id` is the one place that decides what an id may contain, and there is no shape of
// row that makes a task read somebody else's words.
//
// **Why the old conversation becomes a task rather than a migration.** Whatever is already
// in `aether-code` is the operator's work; it is the one transcript that exists. So the
// reserved id `main` names a task whose session *is* that one, and it is present whether or
// not anything has been stored -- the panel on a machine that has never seen this code opens
// on the conversation it opened on yesterday. Nothing is rewritten, and a database that goes
// back to an older build still shows that conversation where it always was.
//
// **What a task does not hold yet: the engine's own session.** `code_engine` runs a pinned
// engine inside the sandbox, and that engine keeps conversations of its own -- so the obvious
// next field is the engine session each task continues. It is not here because there is no
// honest way to fill it yet. The engine mints its own session ids and refuses one it did not
// mint ("Session not found"), `--continue` continues whichever session ran last in the state
// directory rather than this task's, and the state directory cannot be made per task because
// the provider package it holds is 63 MB of it. The one place the id does appear is on every
// event of `--format json`, which is also what a tool-activity rail would be built from; that
// is where this field belongs, and it waits for that work rather than being guessed at here.
//
// **Why there is no "running" state.** A task is running while a question is in flight, which
// is a fact about this instant and not about the task. Stored, it survives a crash as a lie:
// the panel reopens claiming work is going on inside a process that no longer exists. What is
// running is whatever the window is streaming into, and the window already knows.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::llm::MemoryDb;

/// The settings row holding every task. One read on panel open.
pub const SETTING: &str = "code_tasks";

/// The settings row naming the task in focus, so a window opened later lands where the last
/// one was left rather than on whichever task happens to sort first.
pub const CURRENT: &str = "code_task_current";

/// The task that is always there: the conversation AETHER CODE had before it had tasks.
/// Reserved, so `create` can never mint it and a hand-edited row cannot duplicate it.
pub const MAIN: &str = "main";

/// Ceilings. Fifty is far more pieces of work than anyone has in flight, and the point of
/// the limit is that a stuck "new task" button cannot grow a settings row without end.
pub const MAX_TASKS: usize = 50;
pub const MAX_TITLE: usize = 120;

/// How far down a transcript the fallback title looks for something to call the task.
const TITLE_FROM_CHARS: usize = 56;

/// Where a piece of work stands. Two states and a pin, rather than OpenWork's three
/// sidebar groups, because pinning and being finished are separate facts: a finished task
/// worth keeping in front of you is a real thing, and a single list of three cannot hold it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum State {
    #[default]
    Open,
    Done,
}

/// One piece of work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    /// Lower-case letters, digits and dashes. The transcript's session id is built from it,
    /// which is why `valid_id` is strict rather than forgiving.
    pub id: String,
    /// What the operator called it. Empty means "no name yet", and the panel shows the first
    /// thing that was asked instead -- see `titled`.
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub state: State,
    #[serde(default)]
    pub pinned: bool,
}

impl Task {
    /// The `main` task, which exists whether or not anything has been written down.
    fn main() -> Task {
        Task {
            id: MAIN.to_string(),
            title: String::new(),
            state: State::Open,
            pinned: false,
        }
    }

    /// Where this task's words are kept.
    ///
    /// `main` is the conversation that was there before tasks existed, so it keeps its
    /// session id exactly; everything else hangs off it. Derived rather than stored on
    /// purpose -- see the note at the top of this file.
    pub fn session(&self) -> String {
        if self.id == MAIN {
            crate::code_chat::SESSION_ID.to_string()
        } else {
            format!("{}-{}", crate::code_chat::SESSION_ID, self.id)
        }
    }
}

/// What an id may contain. Everything a task's session id is built from goes through here,
/// so this function is the whole of the answer to "what can a settings row make the panel
/// read".
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

/// Every task, pinned first, then open, then finished, and newest activity first inside each
/// group. A row that will not parse reads as no stored tasks at all, which leaves `main` and
/// the operator's existing conversation -- the next save rewrites it.
pub fn all(db: &MemoryDb) -> Vec<Task> {
    let raw = db.get_setting_string(SETTING, "");
    let stored: Vec<Task> = if raw.trim().is_empty() {
        Vec::new()
    } else {
        serde_json::from_str(&raw).unwrap_or_default()
    };

    let mut kept: Vec<Task> = Vec::new();
    let mut has_main = false;
    for task in stored {
        if !valid_id(&task.id) || kept.iter().any(|seen| seen.id == task.id) {
            continue;
        }
        has_main |= task.id == MAIN;
        kept.push(task);
        if kept.len() >= MAX_TASKS {
            break;
        }
    }
    if !has_main {
        kept.insert(0, Task::main());
    }
    kept
}

fn store(db: &MemoryDb, tasks: &[Task]) -> Result<(), String> {
    let value = serde_json::to_string(tasks).map_err(|why| why.to_string())?;
    db.set_setting(SETTING, &Value::String(value))
        .map_err(|why| format!("could not save the task list: {why}"))
}

/// The task in focus. A stored id naming a task that has since been deleted reads as `main`
/// rather than as nothing, because there is always somewhere for the next question to go.
pub fn current(db: &MemoryDb) -> Task {
    let want = db.get_setting_string(CURRENT, MAIN);
    let tasks = all(db);
    tasks
        .iter()
        .find(|task| task.id == want)
        .cloned()
        .unwrap_or_else(|| tasks.into_iter().next().unwrap_or_else(Task::main))
}

/// The task a request named, or the one in focus when it named none.
///
/// Every surface that asks a question goes through here, so an id the operator cannot have
/// is refused with a reason rather than quietly answered in the wrong conversation. Falling
/// back to the current task on an unknown id would be the worse failure: the words land
/// somewhere, and the panel showing them is not the panel they went to.
pub fn resolve(db: &MemoryDb, id: Option<&str>) -> Result<Task, String> {
    let Some(id) = id.map(str::trim).filter(|id| !id.is_empty()) else {
        return Ok(current(db));
    };
    all(db)
        .into_iter()
        .find(|task| task.id == id)
        .ok_or_else(|| format!("there is no task called \"{id}\" -- it may have been deleted"))
}

/// Puts a task in focus, and says which it was so the caller need not read it back.
pub fn focus(db: &MemoryDb, id: &str) -> Result<Task, String> {
    let task = resolve(db, Some(id))?;
    db.set_setting(CURRENT, &Value::String(task.id.clone()))
        .map_err(|why| format!("could not remember which task is open: {why}"))?;
    Ok(task)
}

/// Starts a piece of work and puts it in focus.
///
/// The title may be empty: a task is usually started before there is anything to call it,
/// and the first thing asked makes a better name than anything typed into a dialog first.
pub fn create(db: &MemoryDb, title: &str) -> Result<Task, String> {
    let title = checked_title(title)?;
    let mut tasks = all(db);
    if tasks.len() >= MAX_TASKS {
        return Err(format!(
            "there are already {MAX_TASKS} tasks -- finish or delete one before starting another"
        ));
    }
    let id = fresh_id(&tasks);
    let task = Task {
        id,
        title,
        state: State::Open,
        pinned: false,
    };
    tasks.insert(0, task.clone());
    store(db, &tasks)?;
    db.set_setting(CURRENT, &Value::String(task.id.clone()))
        .map_err(|why| format!("could not remember which task is open: {why}"))?;
    Ok(task)
}

/// A title that is safe to store and to draw. Newlines go because a task name is one line in
/// a list, and a pasted paragraph that keeps its line breaks makes a row the height of the
/// panel.
fn checked_title(title: &str) -> Result<String, String> {
    let flat: String = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > MAX_TITLE {
        return Err(format!(
            "a task name is up to {MAX_TITLE} characters -- that one is {}",
            flat.chars().count()
        ));
    }
    Ok(flat)
}

/// An id nothing else is using. Time-based rather than random so that the stored order and
/// the order they were started agree, which is what makes a list of them readable when the
/// titles are all some variation of "fix the build".
fn fresh_id(tasks: &[Task]) -> String {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis())
        .unwrap_or(0);
    let mut id = format!("t{stamp:x}");
    let mut nudge = 0u32;
    // Two tasks started inside the same millisecond is not a thing an operator can do by
    // hand, but a script driving the HTTP path can, and a duplicate id would mean two tasks
    // sharing one transcript.
    while id == MAIN || tasks.iter().any(|task| task.id == id) {
        nudge += 1;
        id = format!("t{stamp:x}-{nudge}");
    }
    id
}

/// Renames a task. An empty name puts it back on the fallback -- the first thing asked in it
/// -- rather than leaving a blank row.
pub fn rename(db: &MemoryDb, id: &str, title: &str) -> Result<Task, String> {
    let title = checked_title(title)?;
    edit(db, id, |task| task.title = title.clone())
}

/// Marks a task finished, or puts it back to work.
pub fn set_state(db: &MemoryDb, id: &str, state: State) -> Result<Task, String> {
    edit(db, id, |task| task.state = state)
}

/// Pins a task to the top of the list, or unpins it.
pub fn set_pinned(db: &MemoryDb, id: &str, pinned: bool) -> Result<Task, String> {
    edit(db, id, |task| task.pinned = pinned)
}

fn edit(db: &MemoryDb, id: &str, change: impl Fn(&mut Task)) -> Result<Task, String> {
    let mut tasks = all(db);
    let at = tasks
        .iter()
        .position(|task| task.id == id)
        .ok_or_else(|| format!("there is no task called \"{id}\" -- it may have been deleted"))?;
    change(&mut tasks[at]);
    let task = tasks[at].clone();
    store(db, &tasks)?;
    Ok(task)
}

/// Deletes a task and the words in it.
///
/// `main` is the one that cannot go: it is where a question with no task goes, and the panel
/// needs somewhere to be. Asking to delete it empties its transcript instead, which is what
/// the old `clear` button did and the only thing "delete" can honestly mean for it.
pub fn delete(db: &MemoryDb, id: &str) -> Result<(), String> {
    let tasks = all(db);
    let task = tasks
        .iter()
        .find(|task| task.id == id)
        .ok_or_else(|| format!("there is no task called \"{id}\" -- it may already be gone"))?
        .clone();

    // The transcript goes first. A task removed from the list whose words are still in the
    // database is a transcript nothing points at; a transcript deleted for a task still in
    // the list is an empty task, which is a state the panel already draws.
    db.delete_session(&task.session())
        .map_err(|why| format!("could not delete what was said in that task: {why}"))?;

    if task.id == MAIN {
        return Ok(());
    }
    let kept: Vec<Task> = tasks.into_iter().filter(|task| task.id != id).collect();
    store(db, &kept)?;
    if db.get_setting_string(CURRENT, MAIN) == id {
        db.set_setting(CURRENT, &Value::String(MAIN.to_string()))
            .map_err(|why| format!("could not move off the deleted task: {why}"))?;
    }
    Ok(())
}

/// A task with the things only the transcript knows: how much has been said, when, and the
/// name to show when the operator has not given one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Listed {
    pub id: String,
    /// What to draw. The operator's name for it, or the first thing asked in it, or a
    /// placeholder -- the panel never has to decide which.
    pub title: String,
    /// True when `title` is the operator's own, so a rename box can start from the right
    /// text instead of offering the fallback as something to edit.
    pub named: bool,
    pub state: State,
    pub pinned: bool,
    pub messages: u32,
    /// When something was last said in it, or empty for a task nothing has been asked in.
    pub last: String,
    /// True for the one the panel should open on.
    pub current: bool,
}

/// The whole list, ready to draw: pinned first, then open, then finished, each group newest
/// first.
///
/// Ordering is worked out here rather than stored, because what it depends on -- when
/// something was last said -- is in the transcript and changes without the list being
/// touched. Sorting on save would leave the list in whatever order the last save happened
/// to produce.
pub fn listed(db: &MemoryDb) -> Vec<Listed> {
    let open = current(db).id;
    let mut rows: Vec<Listed> = all(db)
        .into_iter()
        .map(|task| {
            let session = task.session();
            let messages = db.get_messages(&session, 200).unwrap_or_default();
            let named = !task.title.trim().is_empty();
            let title = if named {
                task.title.trim().to_string()
            } else {
                fallback_title(&messages)
            };
            Listed {
                id: task.id.clone(),
                title,
                named,
                state: task.state,
                pinned: task.pinned,
                messages: messages.len() as u32,
                last: messages
                    .last()
                    .map(|message| message.timestamp.clone())
                    .unwrap_or_default(),
                current: task.id == open,
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        let group = |row: &Listed| match (row.pinned, row.state) {
            (true, _) => 0,
            (false, State::Open) => 1,
            (false, State::Done) => 2,
        };
        group(a)
            .cmp(&group(b))
            .then_with(|| b.last.cmp(&a.last))
            .then_with(|| b.id.cmp(&a.id))
    });
    rows
}

/// What to call a task nobody has named: the first thing asked in it.
///
/// The first question, not the last, and not the model's answer. A task is the thing the
/// operator set out to do, and that is in their opening sentence; the newest message is
/// wherever the work has wandered to, and the model's prose is about the code rather than
/// about the job.
fn fallback_title(messages: &[crate::llm::Message]) -> String {
    let first = messages
        .iter()
        .find(|message| message.sender == "user")
        .map(|message| message.text.as_str())
        .unwrap_or("");
    let flat: String = first.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.is_empty() {
        // A name, not a sentence about the state. This string goes in the chip at the top of
        // the panel where every other value is a name, and "Nothing asked yet" read there as
        // a status line rather than as what the task is called.
        return "New task".to_string();
    }
    if flat.chars().count() <= TITLE_FROM_CHARS {
        return flat;
    }
    let mut out: String = flat.chars().take(TITLE_FROM_CHARS).collect();
    out.push('\u{2026}');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> MemoryDb {
        let path = std::env::temp_dir().join(format!(
            "aether1_code_tasks_{}_{:?}.db",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&path);
        MemoryDb::open(&path).expect("open test db")
    }

    #[test]
    fn the_conversation_that_was_already_there_is_a_task() {
        let db = db();
        // Nothing has been stored, which is every machine upgrading to this code.
        let tasks = all(&db);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, MAIN);
        // And it reads the transcript the panel has always read, not a new empty one.
        assert_eq!(tasks[0].session(), crate::code_chat::SESSION_ID);
    }

    #[test]
    fn a_new_task_gets_a_transcript_of_its_own() {
        let db = db();
        db.add_message(crate::code_chat::SESSION_ID, "user", "the old conversation")
            .unwrap();
        let task = create(&db, "").unwrap();
        assert_ne!(task.session(), crate::code_chat::SESSION_ID);
        assert!(db.get_messages(&task.session(), 10).unwrap().is_empty());
        // The old words are untouched.
        assert_eq!(
            db.get_messages(crate::code_chat::SESSION_ID, 10)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn creating_a_task_puts_it_in_focus() {
        let db = db();
        assert_eq!(current(&db).id, MAIN);
        let task = create(&db, "fix the build").unwrap();
        assert_eq!(current(&db).id, task.id);
    }

    #[test]
    fn a_question_naming_no_task_goes_to_the_one_in_focus() {
        let db = db();
        let task = create(&db, "fix the build").unwrap();
        assert_eq!(resolve(&db, None).unwrap().id, task.id);
        assert_eq!(resolve(&db, Some("  ")).unwrap().id, task.id);
    }

    #[test]
    fn a_question_naming_a_task_that_is_gone_is_refused_not_rerouted() {
        let db = db();
        let task = create(&db, "fix the build").unwrap();
        delete(&db, &task.id).unwrap();
        let why = resolve(&db, Some(&task.id)).expect_err("a deleted task cannot answer");
        assert!(why.contains("no task"), "{why}");
        // Focus moved off it rather than pointing at nothing.
        assert_eq!(current(&db).id, MAIN);
    }

    #[test]
    fn a_hand_edited_row_cannot_aim_a_task_at_another_conversation() {
        let db = db();
        // Ids that would build a session id belonging to something else, or that are not
        // ids at all.
        db.set_setting(
            SETTING,
            &Value::String(
                serde_json::json!([
                    { "id": "../default" },
                    { "id": "" },
                    { "id": "WORK" },
                    { "id": "has space" },
                    { "id": "fine-1" },
                ])
                .to_string(),
            ),
        )
        .unwrap();
        let ids: Vec<String> = all(&db).into_iter().map(|task| task.id).collect();
        assert_eq!(ids, vec![MAIN.to_string(), "fine-1".to_string()]);
        // And the one that survived reads its own transcript, nothing else's.
        assert_eq!(
            all(&db)[1].session(),
            format!("{}-fine-1", crate::code_chat::SESSION_ID)
        );
    }

    #[test]
    fn a_row_that_will_not_parse_leaves_the_old_conversation_reachable() {
        let db = db();
        db.set_setting(SETTING, &Value::String("{not json".into()))
            .unwrap();
        let tasks = all(&db);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].session(), crate::code_chat::SESSION_ID);
    }

    #[test]
    fn deleting_a_task_takes_its_words_with_it() {
        let db = db();
        let task = create(&db, "fix the build").unwrap();
        db.add_message(&task.session(), "user", "why is it red")
            .unwrap();
        delete(&db, &task.id).unwrap();
        assert!(db.get_messages(&task.session(), 10).unwrap().is_empty());
        assert!(!all(&db).iter().any(|seen| seen.id == task.id));
    }

    #[test]
    fn the_main_task_empties_rather_than_disappearing() {
        let db = db();
        db.add_message(crate::code_chat::SESSION_ID, "user", "something")
            .unwrap();
        delete(&db, MAIN).unwrap();
        assert!(db
            .get_messages(crate::code_chat::SESSION_ID, 10)
            .unwrap()
            .is_empty());
        // Still there: it is where a question with no task goes.
        assert!(all(&db).iter().any(|task| task.id == MAIN));
    }

    #[test]
    fn create_cannot_mint_the_reserved_id() {
        let db = db();
        for _ in 0..3 {
            let task = create(&db, "").unwrap();
            assert_ne!(task.id, MAIN);
        }
    }

    #[test]
    fn a_name_is_the_operators_until_they_take_it_away() {
        let db = db();
        let task = create(&db, "fix  the\nbuild").unwrap();
        assert_eq!(task.title, "fix the build");
        db.add_message(&task.session(), "user", "why is the build red on main")
            .unwrap();

        let row = listed(&db)
            .into_iter()
            .find(|row| row.id == task.id)
            .expect("the task is in the list");
        assert_eq!(row.title, "fix the build");
        assert!(row.named);

        rename(&db, &task.id, "   ").unwrap();
        let row = listed(&db)
            .into_iter()
            .find(|row| row.id == task.id)
            .expect("the task is in the list");
        assert_eq!(row.title, "why is the build red on main");
        assert!(!row.named);
    }

    #[test]
    fn a_title_longer_than_a_title_is_refused_with_the_length() {
        let db = db();
        let why = create(&db, &"x".repeat(MAX_TITLE + 1)).expect_err("too long");
        assert!(why.contains(&MAX_TITLE.to_string()), "{why}");
        assert!(why.contains(&(MAX_TITLE + 1).to_string()), "{why}");
    }

    #[test]
    fn the_list_puts_pinned_first_and_finished_last() {
        let db = db();
        let a = create(&db, "a").unwrap();
        let b = create(&db, "b").unwrap();
        let c = create(&db, "c").unwrap();
        set_state(&db, &a.id, State::Done).unwrap();
        set_pinned(&db, &c.id, true).unwrap();

        let order: Vec<String> = listed(&db).into_iter().map(|row| row.id).collect();
        let at = |id: &str| order.iter().position(|seen| seen == id).unwrap();
        assert!(at(&c.id) < at(&b.id), "pinned comes first: {order:?}");
        assert!(at(&b.id) < at(&a.id), "finished comes last: {order:?}");
    }

    #[test]
    fn there_is_a_ceiling_on_how_many_tasks_there_can_be() {
        let db = db();
        // `main` counts, so one fewer than the ceiling can be created.
        for _ in 0..(MAX_TASKS - 1) {
            create(&db, "").expect("under the ceiling");
        }
        let why = create(&db, "").expect_err("at the ceiling");
        assert!(why.contains(&MAX_TASKS.to_string()), "{why}");
    }

    #[test]
    fn what_an_id_may_contain() {
        assert!(valid_id("main"));
        assert!(valid_id("t19a2b3c4"));
        assert!(valid_id("fine-1"));
        assert!(!valid_id(""));
        assert!(!valid_id("Work"));
        assert!(!valid_id("has space"));
        assert!(!valid_id("../default"));
        assert!(!valid_id("a/b"));
        assert!(!valid_id(&"t".repeat(33)));
    }
}
