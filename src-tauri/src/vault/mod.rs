// The vault: durable memory as a folder of notes you can open.
//
// Everything the companion knows about you that is worth keeping lives here as plain
// markdown, not as rows in a database. That is a deliberate reversal of the original
// design, and the reasons are all the same reason: a folder is *yours*. You can read it in
// any editor, grep it, keep it in git, sync it with Obsidian, hand it to a different
// assistant, and delete a line you disagree with without asking us for a feature. A
// SQLite blob can do none of that, and "your memory is inspectable" is not a promise you
// can keep through an interface you have not built yet.
//
// Notes link to each other with [[wiki links]], which is what turns the folder into a
// graph. Obsidian, Logseq and the rest already draw that graph better than we ever would,
// so the mind map comes free -- our job is to write notes such an app is good at reading.
//
// The index is the retrieval mechanism. INDEX.md says what is here and which notes matter
// for which kind of question; the model reads it every turn and reaches for the rest with
// read_file. There is no ceiling, because only the relevant part is ever loaded.

use std::path::{Path, PathBuf};

use crate::llm::MemoryDb;

pub mod consulted;
pub mod search;

/// Cuts `text` to at most `limit` *characters*, adding an ellipsis when it does.
///
/// Characters rather than bytes: everything trimmed here is prose written by a person, and
/// a snippet that ends mid-character is a bug looking for a name to be reported under.
fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    format!("{}…", text.chars().take(limit).collect::<String>())
}

/// Where the vault lives unless the operator says otherwise. Under home so that the
/// existing tool path guards already reach it.
pub const DEFAULT_VAULT_DIR: &str = "Aether1Vault";
const VAULT_PATH_SETTING: &str = "vault_path";
const IMPORT_FLAG_SETTING: &str = "vault_imported_memories";

/// Notes loaded into every prompt, in this order. Kept deliberately short: this is the
/// part that costs context on every single turn, and everything else is one read away.
const ALWAYS_LOADED: &[&str] = &["INDEX.md", "profile.md", "machine.md"];

/// How many notes may pile up in `daily/` before priming mentions it.
///
/// Two weeks of conversations. Below that there is nothing to fold and saying so every turn
/// would be nagging; above it the dailies have started to be where things are remembered,
/// which is the opposite of what they are for.
const DAILY_BEFORE_CONSOLIDATING: usize = 14;

/// How much of the always-loaded set to accept. A note that grows past this is a note that
/// should have been split, and truncating is better than crowding out the conversation.
const MAX_PRIMED_BYTES: usize = 16 * 1024;

/// The configured vault location.
pub fn vault_path(db: &MemoryDb) -> PathBuf {
    let configured = db.get_setting_string(VAULT_PATH_SETTING, "");
    if configured.trim().is_empty() {
        // No home found (neither HOME nor USERPROFILE) is rare but real; putting the vault
        // beside the working directory at least keeps it findable rather than scattering
        // notes at the filesystem root.
        crate::paths::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(DEFAULT_VAULT_DIR)
    } else {
        crate::paths::expand_home(configured.trim())
    }
}

fn starter_index(agent_name: &str) -> String {
    format!(
        "# Index\n\n\
         This is {agent_name}'s memory. It is a folder of plain markdown notes: read them, \
         edit them, delete them, keep them in git. Nothing here is a database.\n\n\
         ## How to use this\n\n\
         {agent_name} reads this file, [[profile]] and [[machine]] at the start of every \
         conversation, and reads anything else listed below when the question calls for it. \
         Keep this file short -- it is loaded every single turn. Everything else is one read \
         away.\n\n\
         ## What is here\n\n\
         - [[profile]] — who the operator is, how they like to work.\n\
         - [[machine]] — what this computer is and what runs on it.\n\
         - [[memories]] — things the operator asked to be remembered, newest last.\n\
         - `projects/` — one note per project, linked from here as they are created.\n\
         - `daily/` — what happened on a given day.\n\
         - `archive/` — notes that stopped being true, kept rather than deleted.\n\n\
         ## Which notes for which question\n\n\
         | When the question is about | Read |\n\
         | --- | --- |\n\
         | the operator, their preferences, how they work | [[profile]] |\n\
         | this computer, its hardware, what is installed | [[machine]] |\n\
         | something the operator told you to remember | [[memories]] |\n\
         | a named project | `projects/<name>.md` |\n\
         | something that happened recently | the newest notes in `daily/` |\n"
    )
}

const STARTER_PROFILE: &str = "# Profile\n\n\
     Who the operator is, and how they like things done. This note is loaded every turn, so \
     keep it to what is durably true rather than to a running log.\n\n\
     ## Preferences\n\n\
     _Nothing recorded yet._\n\n\
     ## Projects\n\n\
     _Nothing recorded yet._\n";

const STARTER_MACHINE: &str = "# Machine\n\n\
     What this computer is, what is installed on it, and anything worth remembering about \
     how it behaves. Live telemetry is in the prompt already -- this is for what telemetry \
     cannot tell you.\n\n\
     ## Setup\n\n\
     _Nothing recorded yet._\n\n\
     ## Quirks\n\n\
     _Nothing recorded yet._\n";

/// Creates the vault if it isn't there, and returns its path. Never overwrites a note that
/// already exists: an operator's own INDEX.md is theirs, and adopting an existing folder
/// has to be safe or nobody will point this at one.
pub fn ensure(db: &MemoryDb) -> Result<PathBuf, String> {
    let root = vault_path(db);
    for dir in ["", "projects", "daily", "archive"] {
        let path = if dir.is_empty() {
            root.clone()
        } else {
            root.join(dir)
        };
        std::fs::create_dir_all(&path)
            .map_err(|e| format!("could not create {}: {e}", path.display()))?;
    }

    let agent_name = db.get_setting_string("agent_name", "AETHER1");
    for (name, contents) in [
        ("INDEX.md", starter_index(&agent_name)),
        ("profile.md", STARTER_PROFILE.to_string()),
        ("machine.md", STARTER_MACHINE.to_string()),
    ] {
        let path = root.join(name);
        if !path.exists() {
            std::fs::write(&path, contents)
                .map_err(|e| format!("could not write {}: {e}", path.display()))?;
        }
    }

    import_old_memories(db, &root)?;
    Ok(root)
}

/// Writes the old key-value memories out as a note, once. The rows are left alone: this is
/// a copy, not a migration, so nothing is lost if the import is wrong and the operator can
/// delete the note without consequence.
fn import_old_memories(db: &MemoryDb, root: &Path) -> Result<(), String> {
    if db.get_setting_bool(IMPORT_FLAG_SETTING, false) {
        return Ok(());
    }
    let memories = db.get_all_memories().unwrap_or_default();
    if !memories.is_empty() {
        let mut note = String::from(
            "# Imported memories\n\n\
             Facts recorded before Aether1 kept notes as files. Move anything worth keeping \
             into [[profile]] or a project note; delete the rest.\n\n",
        );
        for memory in &memories {
            note.push_str(&format!("- **{}**: {}\n", memory.key, memory.value));
        }
        let path = root.join("imported-memories.md");
        if !path.exists() {
            std::fs::write(&path, note)
                .map_err(|e| format!("could not write {}: {e}", path.display()))?;
        }
    }
    let _ = db.set_setting(IMPORT_FLAG_SETTING, &serde_json::Value::Bool(true));
    Ok(())
}

/// Resolves a note path *relative to the vault*, refusing anything that would land outside
/// it. Notes are markdown by definition here -- a tool that could write any file anywhere
/// under a "note" name would just be write_file with a friendlier description.
pub fn resolve_note(db: &MemoryDb, relative: &str) -> Result<PathBuf, String> {
    let root = vault_path(db);
    let relative = relative.trim();

    if relative.is_empty() {
        return Err("no note name given".to_string());
    }
    // Refused, not silently reinterpreted: quietly turning /etc/passwd.md into a note
    // inside the vault would mean a caller asking for one thing and getting another.
    if Path::new(relative).is_absolute() {
        return Err(format!("{relative} must be relative to the vault"));
    }
    if !relative.ends_with(".md") {
        return Err(format!("{relative} is not a note: notes end in .md"));
    }
    // Checked before any filesystem call, because the point is to refuse the shape of the
    // path, not to discover where it happens to land.
    if relative.split('/').any(|part| part == ".." || part == ".") {
        return Err(format!("{relative} must not step outside the vault"));
    }

    Ok(root.join(relative))
}

/// The notes that are loaded every turn, and so are never archived. A vault whose index
/// has been moved into `archive/` is a vault with no map; a profile that has stopped being
/// true gets *edited*, which is what write_note is for.
pub fn is_core_note(relative: &str) -> bool {
    ALWAYS_LOADED
        .iter()
        .any(|name| name.eq_ignore_ascii_case(relative.trim()))
}

/// How many `-2`, `-3` … suffixes to try before giving up on a name collision in the
/// archive. Past this, two notes are being archived under one name repeatedly and the
/// honest answer is to say so rather than to keep inventing filenames.
const MAX_ARCHIVE_SUFFIX: u32 = 20;

/// Where a note goes when it stops being true, and the source it moves from.
///
/// Archiving rather than deleting is the whole point: a note that contradicted a newer one
/// is evidence about what the companion used to believe, and the operator is the only one
/// who gets to destroy their own memory.
pub fn archive_destination(
    db: &MemoryDb,
    relative: &str,
) -> Result<(PathBuf, String, PathBuf), String> {
    let relative = relative.trim();
    let from = resolve_note(db, relative)?;
    if !from.exists() {
        return Err(format!("{relative} is not a note in the vault"));
    }
    if relative.starts_with("archive/") {
        return Err(format!("{relative} is already archived"));
    }
    if is_core_note(relative) {
        return Err(format!(
            "{relative} is loaded into every conversation and is not archived -- correct it \
             with write_note instead"
        ));
    }

    let root = vault_path(db);
    let stem = relative.trim_end_matches(".md");
    for attempt in 1..=MAX_ARCHIVE_SUFFIX {
        let candidate = if attempt == 1 {
            format!("archive/{relative}")
        } else {
            format!("archive/{stem}-{attempt}.md")
        };
        let path = root.join(&candidate);
        if !path.exists() {
            return Ok((from, candidate, path));
        }
    }
    Err(format!(
        "{relative} has been archived {MAX_ARCHIVE_SUFFIX} times already; tidy the archive \
         before adding another"
    ))
}

/// Marks a note archived in the index without removing its line.
///
/// The line stays because `[[wiki links]]` resolve by note name rather than by folder, so
/// the link still works after the move -- and because silently deleting a line from a file
/// the operator writes in themselves is not a thing a memory system should do. Best-effort:
/// failing to annotate the index is not a reason to fail the archive.
fn mark_archived_in_index(root: &Path, relative: &str) {
    let index = root.join("INDEX.md");
    let Ok(contents) = std::fs::read_to_string(&index) else {
        return;
    };
    let stem = Path::new(relative)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let link = format!("[[{stem}]]");

    let mut changed = false;
    let updated: Vec<String> = contents
        .lines()
        .map(|line| {
            if line.contains(&link) && !line.contains("(archived)") {
                changed = true;
                format!("{} (archived)", line.trim_end())
            } else {
                line.to_string()
            }
        })
        .collect();
    if changed {
        let _ = std::fs::write(&index, format!("{}\n", updated.join("\n")));
    }
}

/// Moves a note into `archive/`, returning its new path relative to the vault.
pub fn archive(db: &MemoryDb, relative: &str) -> Result<String, String> {
    let (from, destination, to) = archive_destination(db, relative)?;
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    std::fs::rename(&from, &to)
        .map_err(|e| format!("could not move {} to {}: {e}", from.display(), to.display()))?;
    mark_archived_in_index(&vault_path(db), relative.trim());
    Ok(destination)
}

/// Puts an archived note back where it came from. The undo half of `archive`.
pub fn unarchive(db: &MemoryDb, archived: &str, original: &str) -> Result<(), String> {
    let from = resolve_note(db, archived)?;
    let to = resolve_note(db, original)?;
    if to.exists() {
        return Err(format!("{original} exists again; nothing was moved back"));
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    std::fs::rename(&from, &to).map_err(|e| {
        format!(
            "could not move {} back to {}: {e}",
            from.display(),
            to.display()
        )
    })?;
    Ok(())
}

/// The vault's contribution to the system prompt: the index, the always-loaded notes, and
/// an instruction about how to reach the rest.
pub fn prime(db: &MemoryDb) -> String {
    let root = vault_path(db);
    if !root.exists() {
        return String::new();
    }

    let mut block = format!(
        "\n[MEMORY VAULT: {}]\n\
         These notes are your memory of this operator. Read others with read_file when the \
         question calls for them, following the index below. The index does not list every \
         note -- use search_memory when the index has no obvious answer, and prefer what it \
         returns over guessing. Write with append_note and write_note; both need the \
         operator's approval, so say what you intend to record and why.\n",
        root.display()
    );

    for name in ALWAYS_LOADED {
        let path = root.join(name);
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        let contents = if contents.len() > MAX_PRIMED_BYTES {
            format!(
                "{}\n[note truncated -- it has grown too large to load every turn]",
                &contents[..MAX_PRIMED_BYTES]
            )
        } else {
            contents
        };
        block.push_str(&format!("\n--- {name} ---\n{contents}"));
        consulted::record(name, consulted::How::Primed);
    }

    block.push_str(&consolidation_note(&root));
    block
}

/// The one line of housekeeping the prompt is allowed to carry, and only once there is
/// housekeeping to do.
///
/// Consolidation is the model's job rather than the code's: folding a fortnight of daily
/// notes into a topic note is a judgement about what mattered, and a routine that did it
/// automatically would be rewriting the operator's memory without being asked. What the
/// code can do is notice, and say so once the pile is real -- and say plainly that it is not
/// worth interrupting anyone over, so this does not become a companion that nags.
fn consolidation_note(root: &Path) -> String {
    let daily = root.join("daily");
    let count = std::fs::read_dir(&daily)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
                .count()
        })
        .unwrap_or(0);

    if count <= DAILY_BEFORE_CONSOLIDATING {
        return String::new();
    }

    format!(
        "\n[CONSOLIDATION]\nThere are {count} notes in `daily/`. When the conversation \
         reaches a natural pause, offer to fold the older ones into topic notes under \
         `projects/` or into [[profile]], then archive_note the dailies you folded. A fact \
         worth keeping belongs in the note about its subject, not in the note about the day \
         it was mentioned. Do not interrupt the operator to do this, and do not do it \
         without asking.\n"
    )
}

/// Records something the operator explicitly asked to be remembered. Appends to
/// `memories.md` rather than guessing which note it belongs in -- sorting it is a job for
/// consolidation, and a fact in the wrong note is still a fact you can find.
pub fn remember(db: &MemoryDb, fact: &str) -> Result<String, String> {
    let root = ensure(db)?;
    let path = root.join("memories.md");

    let mut contents = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        String::from(
            "# Memories\n\nThings the operator asked to be remembered, newest last. Move \
             anything durable into [[profile]] or a project note.\n",
        )
    });
    if !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents.push_str(&format!("\n- {}\n", fact.trim()));

    std::fs::write(&path, contents)
        .map_err(|e| format!("could not write {}: {e}", path.display()))?;

    // A note the index doesn't mention is a note nothing will ever go looking for. Older
    // vaults were created before memories.md was listed, so link it in rather than
    // leaving them quietly broken.
    link_from_index(
        &root,
        "memories",
        "things the operator asked to be remembered",
    );

    Ok(relative_name(db, &path))
}

/// Setting controlling whether conversations are written to `daily/`.
const JOURNAL_SETTING: &str = "vault_journal";

/// When the day's note is full enough to start another one.
///
/// Nothing is ever cut. The journal used to trim each side of an exchange to 1200
/// characters on the grounds that the database held the full text anyway -- but the
/// database is the part you cannot open, so what that really meant was that the readable
/// copy was the incomplete one. It is verbatim now.
///
/// Verbatim needs a ceiling somewhere, though, and this is the honest place for it: a note
/// larger than `search::MAX_NOTE_BYTES` is skipped by the vault's own search, so an
/// unusually talkative day would quietly become the one day the companion cannot find
/// anything in. Rather than truncate, the day rolls into `2026-09-15-2.md` and carries on.
/// Comfortably under that limit so a single enormous exchange cannot push a note past it.
const ROLL_NOTE_AT_BYTES: u64 = 256 * 1024;

/// Whether conversations are journalled. On unless the operator turns it off.
pub fn journal_enabled(db: &MemoryDb) -> bool {
    db.get_setting_bool(JOURNAL_SETTING, true)
}

/// Appends one exchange to today's note in `daily/`, and returns its name.
///
/// This is the vault writing to its own folder as a matter of course, which is why it is a
/// plain function rather than a tool the model calls: nothing here is the model's decision,
/// there is no path to choose, and asking the operator to approve their own conversation
/// being remembered would be a consent prompt with no question in it. The consent that
/// matters is the setting above, given once.
///
/// Best-effort by design. A conversation that has already happened is not undone by a full
/// disk, so a failure to write is reported to the caller and never to the operator mid-reply.
pub fn journal_exchange(
    db: &MemoryDb,
    agent_name: &str,
    prompt: &str,
    reply: &str,
) -> Result<String, String> {
    if !journal_enabled(db) {
        return Err("journalling is switched off".to_string());
    }
    let prompt = prompt.trim();
    let reply = reply.trim();
    if prompt.is_empty() && reply.is_empty() {
        return Err("nothing was said".to_string());
    }

    let root = ensure(db)?;
    let (date, time) = db.local_now();
    let daily = root.join("daily");
    std::fs::create_dir_all(&daily)
        .map_err(|e| format!("could not create {}: {e}", daily.display()))?;
    let path = note_for_day(&daily, &date);

    let mut contents = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        format!(
            "# {date}\n\nWhat was said on this day, word for word, oldest first. Written \
             automatically; edit or delete any of it freely, and fold what matters into a \
             note of its own.\n"
        )
    });
    if !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents.push_str(&format!(
        "\n## {time}\n\n**You:** {prompt}\n\n**{agent_name}:** {reply}\n"
    ));

    std::fs::write(&path, contents)
        .map_err(|e| format!("could not write {}: {e}", path.display()))?;

    Ok(relative_name(db, &path))
}

/// Which note today's exchange is appended to.
///
/// Normally `<date>.md`. Once that has grown past `ROLL_NOTE_AT_BYTES` the day continues in
/// `<date>-2.md`, then `-3`, and so on -- so a long day becomes several readable notes
/// rather than one the search will not open. The suffix keeps the date at the front, which
/// is what makes a folder of these sort into the order they happened.
///
/// Walks forward from the newest part rather than counting files, so a part deleted from
/// the middle of a day cannot send today's writing back into an older one.
fn note_for_day(daily: &Path, date: &str) -> PathBuf {
    let mut path = daily.join(format!("{date}.md"));
    let mut part = 1_u32;
    loop {
        let full = std::fs::metadata(&path).is_ok_and(|m| m.len() >= ROLL_NOTE_AT_BYTES);
        if !full {
            return path;
        }
        part += 1;
        // A day that has somehow reached this many parts is a runaway rather than a
        // conversation. Keeping the last one is better than looping forever.
        if part > 999 {
            return path;
        }
        path = daily.join(format!("{date}-{part}.md"));
    }
}

/// Adds a link to INDEX.md if it isn't already there. Best-effort: failing to update the
/// index is not a reason to lose what was being remembered.
fn link_from_index(root: &Path, note: &str, description: &str) {
    let index = root.join("INDEX.md");
    let Ok(contents) = std::fs::read_to_string(&index) else {
        return;
    };
    let link = format!("[[{note}]]");
    if contents.contains(&link) {
        return;
    }
    let mut updated = contents;
    if !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(&format!("- {link} — {description}.\n"));
    let _ = std::fs::write(&index, updated);
}

/// A readable listing of the vault: where it is and what is in it. Answers "what do you
/// remember" with something the operator can go and open, rather than with a dump.
pub fn describe(db: &MemoryDb) -> String {
    let root = vault_path(db);
    if !root.exists() {
        return format!(
            "No memory vault yet. It will be created at `{}` the first time something is \
             worth remembering.",
            root.display()
        );
    }

    let mut notes = Vec::new();
    collect_notes(&root, &root, &mut notes, 0);
    notes.sort();

    if notes.is_empty() {
        return format!("The vault at `{}` is empty.", root.display());
    }
    format!(
        "### \u{1f9e0} Memory vault\n`{}`\n\n{}\n\nOpen any of these in an editor -- they \
         are ordinary markdown files.",
        root.display(),
        notes
            .iter()
            .map(|n| format!("- {n}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

/// Walks the vault collecting note names. Depth-limited: a vault is a folder of notes, and
/// following an arbitrarily deep tree there would be a way to spend a long time on
/// something that is not a vault.
fn collect_notes(root: &Path, dir: &Path, into: &mut Vec<String>, depth: usize) {
    if depth > 3 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_notes(root, &path, into, depth + 1);
        } else if path.extension().is_some_and(|e| e == "md") {
            into.push(
                path.strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string(),
            );
        }
    }
}

/// The note's name if `path` is inside the vault, and nothing if it is anywhere else.
///
/// Both sides are canonicalised before they are compared, because the vault path is a
/// setting the operator typed and the path being checked has been through `fs_guard`: one
/// may have a symlink or a `..` in it that the other does not, and a string comparison
/// would answer "not in the vault" for a file plainly in the vault.
pub fn note_in_vault(db: &MemoryDb, path: &Path) -> Option<String> {
    let root = vault_path(db).canonicalize().ok()?;
    let path = path.canonicalize().ok()?;
    let name = path.strip_prefix(&root).ok()?;
    Some(name.to_string_lossy().to_string())
}

/// A note's path relative to the vault, for reporting which notes were consulted.
pub fn relative_name(db: &MemoryDb, path: &Path) -> String {
    path.strip_prefix(vault_path(db))
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

/// The always-loaded notes that actually exist right now, in priming order -- exactly the
/// files `prime` would load into this turn's context. Kept separate from `prime` itself so
/// the operator can be told *which* notes are behind an answer (see llm/mod.rs's
/// vault_trace) without needing their contents, which is all `prime` actually needs this
/// list for. Empty when there's no vault yet, same as `prime`.
pub fn primed_notes(db: &MemoryDb) -> Vec<String> {
    let root = vault_path(db);
    if !root.exists() {
        return Vec::new();
    }
    ALWAYS_LOADED
        .iter()
        .filter(|name| root.join(name).exists())
        .map(|name| name.to_string())
        .collect()
}

/// Opens the vault folder in the operator's own file manager -- Explorer, Finder, or
/// whichever handler `xdg-open` resolves to on Linux. Creates the starter layout first if
/// this is the very first time anything has touched the vault: opening a folder that was
/// never created would be a worse first impression than the empty starter notes `ensure`
/// already produces.
pub fn open_folder(db: &MemoryDb) -> Result<(), String> {
    let root = ensure(db)?;
    crate::paths::open_in_file_manager(&root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> (MemoryDb, PathBuf) {
        let dir = std::env::temp_dir().join(format!("aether1_vault_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = MemoryDb::open(dir.join("memory.db")).unwrap();
        db.set_setting(
            VAULT_PATH_SETTING,
            &serde_json::Value::String(dir.join("vault").to_string_lossy().to_string()),
        )
        .unwrap();
        (db, dir.join("vault"))
    }

    /// The point of the feature: an ordinary exchange ends up as markdown in a folder the
    /// operator can open, with both halves of it readable.
    #[test]
    fn a_conversation_becomes_a_note_in_the_day_folder() {
        let (db, root) = fixture("journal");
        ensure(&db).unwrap();

        let name = journal_exchange(
            &db,
            "HALCY",
            "what is eating my disk?",
            "Your cache is 40GB.",
        )
        .unwrap();
        assert!(name.starts_with("daily/"), "filed under the day: {name}");

        let written = std::fs::read_to_string(root.join(&name)).unwrap();
        assert!(written.contains("what is eating my disk?"), "{written}");
        assert!(written.contains("Your cache is 40GB."), "{written}");
        assert!(
            written.contains("**HALCY:**"),
            "the agent is named: {written}"
        );
    }

    /// A second exchange joins the first rather than replacing it. Overwriting would lose
    /// the day, and losing the day silently is worse than never having written it.
    #[test]
    fn a_second_exchange_is_added_to_the_same_day() {
        let (db, root) = fixture("journal_append");
        ensure(&db).unwrap();

        let first = journal_exchange(&db, "HALCY", "first question", "first answer").unwrap();
        let second = journal_exchange(&db, "HALCY", "second question", "second answer").unwrap();
        assert_eq!(first, second, "the same day is the same note");

        let written = std::fs::read_to_string(root.join(&second)).unwrap();
        assert!(written.contains("first question"), "{written}");
        assert!(written.contains("second question"), "{written}");
    }

    /// Switched off means nothing is written at all -- not a shorter note, not an empty
    /// file. The setting is the consent, so it has to be the whole of it.
    #[test]
    fn journalling_off_writes_nothing() {
        let (db, root) = fixture("journal_off");
        ensure(&db).unwrap();
        db.set_setting(JOURNAL_SETTING, &serde_json::json!(false))
            .unwrap();

        assert!(journal_exchange(&db, "HALCY", "a question", "an answer").is_err());
        let daily = root.join("daily");
        let count = std::fs::read_dir(&daily).map(|d| d.count()).unwrap_or(0);
        assert_eq!(count, 0, "nothing was filed");
    }

    /// The point of the change: a long reply is written out in full. The readable copy
    /// being the incomplete one was the whole complaint.
    #[test]
    fn a_long_reply_is_written_out_in_full() {
        let (db, root) = fixture("journal_long");
        ensure(&db).unwrap();

        let huge = "x".repeat(20_000);
        let name = journal_exchange(&db, "HALCY", "go on", &huge).unwrap();
        let written = std::fs::read_to_string(root.join(&name)).unwrap();
        assert!(written.contains(&huge), "the reply is there word for word");
        assert!(!written.contains('…'), "and nothing was cut: {name}");
    }

    /// A day long enough to outgrow one note continues in the next rather than being
    /// truncated -- and every part stays small enough for the vault's own search to open.
    #[test]
    fn a_very_long_day_rolls_into_a_second_note() {
        let (db, root) = fixture("journal_roll");
        ensure(&db).unwrap();

        // Each exchange is a good fraction of the roll threshold, so this takes a handful
        // of turns rather than thousands.
        let chunk = "y".repeat(64 * 1024);
        let mut names = Vec::new();
        for _ in 0..8 {
            names.push(journal_exchange(&db, "HALCY", "go on", &chunk).unwrap());
        }

        let first = &names[0];
        let last = names.last().unwrap();
        assert_ne!(first, last, "the day rolled into another note: {names:?}");
        assert!(
            last.contains("-2") || last.contains("-3"),
            "numbered: {last}"
        );

        for name in &names {
            let size = std::fs::metadata(root.join(name)).unwrap().len();
            assert!(
                size < 512 * 1024,
                "{name} is {size} bytes -- the search would skip it"
            );
        }
    }

    /// What the HUD says under an answer starts here: every note priming pasted in is a note
    /// that was in front of the model, whether or not it used it.
    #[test]
    fn priming_reports_the_notes_it_loaded() {
        let (db, _root) = fixture("consulted");
        ensure(&db).unwrap();

        consulted::begin();
        let block = prime(&db);
        let notes = consulted::taken();

        let names: Vec<&str> = notes.iter().map(|c| c.note.as_str()).collect();
        assert_eq!(names, ALWAYS_LOADED.to_vec());
        assert!(
            notes.iter().all(|c| c.how == consulted::How::Primed),
            "priming is the reason these are here: {notes:?}"
        );
        assert!(
            block.contains("profile.md"),
            "and they really were pasted in"
        );
    }

    /// The check that keeps read_file's report honest: only files actually under the vault
    /// count as memory, and the answer must not depend on how the path was spelled.
    #[test]
    fn a_file_outside_the_vault_is_not_a_note() {
        let (db, root) = fixture("in_vault");
        ensure(&db).unwrap();
        let outside = root.parent().unwrap().join("memory.db");

        assert_eq!(
            note_in_vault(&db, &root.join("projects").join("..").join("profile.md")),
            Some("profile.md".to_string())
        );
        assert_eq!(note_in_vault(&db, &outside), None);
        assert_eq!(note_in_vault(&db, Path::new("/etc/hostname")), None);
    }

    #[test]
    fn a_new_vault_gets_a_starter_layout() {
        let (db, root) = fixture("starter");
        ensure(&db).unwrap();

        for note in ["INDEX.md", "profile.md", "machine.md"] {
            assert!(root.join(note).exists(), "{note} should exist");
        }
        for dir in ["projects", "daily", "archive"] {
            assert!(root.join(dir).is_dir(), "{dir}/ should exist");
        }

        let index = std::fs::read_to_string(root.join("INDEX.md")).unwrap();
        assert!(
            index.contains("[[profile]]"),
            "the index links notes together"
        );
    }

    #[test]
    fn an_existing_note_is_never_overwritten() {
        let (db, root) = fixture("adopt");
        ensure(&db).unwrap();
        std::fs::write(root.join("INDEX.md"), "# My own index\n").unwrap();

        ensure(&db).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("INDEX.md")).unwrap(),
            "# My own index\n",
            "adopting an existing vault must not clobber it"
        );
    }

    #[test]
    fn old_key_value_memories_are_copied_in_once() {
        let (db, root) = fixture("import");
        db.set_memory("favorite_editor", "helix", "general")
            .unwrap();
        ensure(&db).unwrap();

        let imported = std::fs::read_to_string(root.join("imported-memories.md")).unwrap();
        assert!(imported.contains("favorite_editor"));
        assert!(imported.contains("helix"));

        // The rows are left alone -- this is a copy, so a bad import loses nothing.
        assert_eq!(db.get_all_memories().unwrap().len(), 1);

        // And it doesn't happen twice.
        std::fs::write(root.join("imported-memories.md"), "edited by hand").unwrap();
        ensure(&db).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("imported-memories.md")).unwrap(),
            "edited by hand"
        );
    }

    #[test]
    fn priming_carries_the_index_and_the_always_loaded_notes() {
        let (db, root) = fixture("prime");
        ensure(&db).unwrap();
        std::fs::write(root.join("profile.md"), "# Profile\n\nPrefers helix.\n").unwrap();

        let block = prime(&db);
        assert!(block.contains("MEMORY VAULT"));
        assert!(
            block.contains("Prefers helix."),
            "the profile is loaded every turn"
        );
        assert!(block.contains("--- INDEX.md ---"));
    }

    #[test]
    fn priming_an_absent_vault_says_nothing_at_all() {
        let (db, _root) = fixture("no_vault");
        assert_eq!(
            prime(&db),
            "",
            "no vault means no vault section in the prompt"
        );
    }

    #[test]
    fn primed_notes_lists_exactly_what_prime_loads() {
        let (db, root) = fixture("primed_notes");
        assert!(
            primed_notes(&db).is_empty(),
            "no vault means no notes to report either"
        );

        ensure(&db).unwrap();
        assert_eq!(
            primed_notes(&db),
            vec!["INDEX.md", "profile.md", "machine.md"]
        );

        // A note the operator deleted is a note that did not load, so it must not be
        // claimed as part of the answer.
        std::fs::remove_file(root.join("machine.md")).unwrap();
        assert_eq!(primed_notes(&db), vec!["INDEX.md", "profile.md"]);
    }

    #[test]
    fn an_oversized_note_is_truncated_rather_than_crowding_out_the_conversation() {
        let (db, root) = fixture("oversize");
        ensure(&db).unwrap();
        std::fs::write(root.join("profile.md"), "x".repeat(MAX_PRIMED_BYTES * 2)).unwrap();

        let block = prime(&db);
        assert!(
            block.contains("note truncated"),
            "an enormous note must not be pasted whole"
        );
        assert!(block.len() < MAX_PRIMED_BYTES * 2);
    }

    #[test]
    fn describing_the_vault_lists_its_notes_and_where_they_are() {
        let (db, root) = fixture("describe");
        ensure(&db).unwrap();
        std::fs::write(root.join("projects/aether1.md"), "# Aether1\n").unwrap();

        let described = describe(&db);
        assert!(described.contains("INDEX.md"));
        assert!(described.contains("projects/aether1.md"));
        assert!(described.contains(&root.display().to_string()));
    }

    #[test]
    fn describing_an_absent_vault_says_where_it_would_go() {
        let (db, root) = fixture("describe_absent");
        let described = describe(&db);
        assert!(described.contains("No memory vault yet"), "{described}");
        assert!(described.contains(&root.display().to_string()));
    }

    #[test]
    fn remember_appends_to_a_note_and_creates_the_vault_if_needed() {
        let (db, root) = fixture("remember");

        let note = remember(&db, "the laptop is called tycho").unwrap();
        assert_eq!(note, "memories.md");
        let contents = std::fs::read_to_string(root.join("memories.md")).unwrap();
        assert!(contents.contains("the laptop is called tycho"));

        remember(&db, "and it runs arch").unwrap();
        let contents = std::fs::read_to_string(root.join("memories.md")).unwrap();
        assert!(
            contents.contains("tycho"),
            "the earlier fact is still there"
        );
        assert!(contents.contains("arch"));
    }

    #[test]
    fn remembering_links_the_note_from_the_index() {
        let (db, root) = fixture("index_link");
        ensure(&db).unwrap();
        // An older vault, created before memories.md was part of the starter index.
        std::fs::write(root.join("INDEX.md"), "# Index\n\n- [[profile]]\n").unwrap();

        remember(&db, "the laptop is called tycho").unwrap();
        let index = std::fs::read_to_string(root.join("INDEX.md")).unwrap();
        assert!(
            index.contains("[[memories]]"),
            "a note the index never mentions is one nothing will look for: {index}"
        );

        // And it is not linked twice.
        remember(&db, "and it runs arch").unwrap();
        let index = std::fs::read_to_string(root.join("INDEX.md")).unwrap();
        assert_eq!(index.matches("[[memories]]").count(), 1);
    }

    #[test]
    fn note_paths_cannot_leave_the_vault() {
        let (db, root) = fixture("escape");
        for bad in [
            "../outside.md",
            "notes/../../outside.md",
            "/etc/passwd.md",
            "./sneaky.md",
        ] {
            assert!(resolve_note(&db, bad).is_err(), "{bad} should be refused");
        }

        let good = resolve_note(&db, "projects/aether1.md").unwrap();
        assert!(good.starts_with(&root));
    }

    #[test]
    fn only_markdown_counts_as_a_note() {
        let (db, _root) = fixture("md_only");
        assert!(resolve_note(&db, "profile.txt").is_err());
        assert!(resolve_note(&db, ".bashrc").is_err());
        assert!(resolve_note(&db, "profile.md").is_ok());
    }

    // ------------------------------------------------------ retrieval

    /// The acceptance test from the plan, written out: with two hundred notes, asking
    /// about one topic pulls that topic's note and not the ten most recent.
    ///
    /// The dailies are written *after* the topic note on purpose, so every one of them is
    /// more recent than the answer. If recency were doing any of the ranking this test
    /// would fail, which is the point of it.
    #[test]
    fn with_two_hundred_notes_a_topic_question_finds_the_topic_note() {
        let (db, root) = fixture("bulk");
        ensure(&db).unwrap();
        std::fs::create_dir_all(root.join("projects")).unwrap();
        std::fs::write(
            root.join("projects/sourdough.md"),
            "# Sourdough\n\nThe starter lives in the fridge and gets fed on Sundays.\n",
        )
        .unwrap();

        for n in 0..200 {
            std::fs::write(
                root.join(format!("daily/2026-01-{n:03}.md")),
                format!("# Day {n}\n\nOrdinary conversation. Nothing about baking at all.\n"),
            )
            .unwrap();
        }

        let results = search::search(&db, "sourdough starter");
        assert!(results.scanned > 200, "scanned {}", results.scanned);
        assert_eq!(
            results.hits.first().map(|h| h.note.as_str()),
            Some("projects/sourdough.md"),
            "got {:?}",
            results.hits.iter().map(|h| &h.note).collect::<Vec<_>>()
        );
    }

    /// And the shortlist stays a shortlist: a word every note contains must not return
    /// every note.
    #[test]
    fn a_search_returns_a_shortlist_rather_than_the_whole_vault() {
        let (db, root) = fixture("shortlist");
        ensure(&db).unwrap();
        for n in 0..50 {
            std::fs::write(
                root.join(format!("daily/day-{n:03}.md")),
                "# Day\n\nTelemetry looked normal.\n",
            )
            .unwrap();
        }
        let results = search::search(&db, "telemetry");
        assert!(results.hits.len() <= 8, "{} hits", results.hits.len());
        assert!(results.scanned >= 50);
    }

    #[test]
    fn a_search_that_finds_nothing_says_so_rather_than_inventing_something() {
        let (db, _) = fixture("empty_search");
        ensure(&db).unwrap();
        let results = search::search(&db, "sourdough");
        assert!(results.hits.is_empty());
        let rendered = search::render(&db, "sourdough", &results);
        assert!(rendered.contains("No note matches"), "{rendered}");
    }

    // ---------------------------------------------------- consolidation

    #[test]
    fn a_handful_of_daily_notes_is_not_worth_mentioning() {
        let (db, root) = fixture("quiet");
        ensure(&db).unwrap();
        for n in 0..3 {
            std::fs::write(root.join(format!("daily/day-{n}.md")), "# Day\n").unwrap();
        }
        assert!(!prime(&db).contains("CONSOLIDATION"));
    }

    /// Once the dailies are where things are being remembered -- which is the opposite of
    /// what they are for -- the prompt says so, once, and says not to interrupt over it.
    #[test]
    fn a_pile_of_daily_notes_prompts_consolidation_without_nagging() {
        let (db, root) = fixture("pile");
        ensure(&db).unwrap();
        for n in 0..30 {
            std::fs::write(root.join(format!("daily/day-{n:03}.md")), "# Day\n").unwrap();
        }
        let primed = prime(&db);
        assert!(primed.contains("CONSOLIDATION"), "{primed}");
        assert!(primed.contains("30 notes"), "{primed}");
        assert!(primed.contains("Do not interrupt"), "{primed}");
    }

    // --------------------------------------------------------- archive

    #[test]
    fn archiving_moves_a_note_and_marks_it_in_the_index() {
        let (db, root) = fixture("archive");
        ensure(&db).unwrap();
        std::fs::write(root.join("projects/sourdough.md"), "# Sourdough\n").unwrap();
        link_from_index(&root, "sourdough", "the starter");

        let destination = archive(&db, "projects/sourdough.md").unwrap();
        assert_eq!(destination, "archive/projects/sourdough.md");
        assert!(!root.join("projects/sourdough.md").exists());
        assert!(root.join(&destination).exists());

        // The line stays -- wiki links resolve by name, so it still works -- but it says
        // what happened.
        let index = std::fs::read_to_string(root.join("INDEX.md")).unwrap();
        assert!(index.contains("[[sourdough]]"), "{index}");
        assert!(index.contains("(archived)"), "{index}");
    }

    /// Nothing archives the map or the two notes loaded every turn: those get corrected,
    /// not filed away.
    #[test]
    fn the_always_loaded_notes_are_never_archived() {
        let (db, _) = fixture("core");
        ensure(&db).unwrap();
        for note in ["INDEX.md", "profile.md", "machine.md"] {
            let refused = archive(&db, note).unwrap_err();
            assert!(refused.contains("write_note"), "{refused}");
        }
    }

    #[test]
    fn archiving_the_same_name_twice_does_not_overwrite_the_first() {
        let (db, root) = fixture("collide");
        ensure(&db).unwrap();
        std::fs::write(root.join("notes.md"), "first").unwrap();
        assert_eq!(archive(&db, "notes.md").unwrap(), "archive/notes.md");

        std::fs::write(root.join("notes.md"), "second").unwrap();
        assert_eq!(archive(&db, "notes.md").unwrap(), "archive/notes-2.md");

        assert_eq!(
            std::fs::read_to_string(root.join("archive/notes.md")).unwrap(),
            "first"
        );
    }

    #[test]
    fn an_archive_can_be_undone() {
        let (db, root) = fixture("unarchive");
        ensure(&db).unwrap();
        std::fs::write(root.join("projects/old.md"), "# Old\n").unwrap();

        let destination = archive(&db, "projects/old.md").unwrap();
        unarchive(&db, &destination, "projects/old.md").unwrap();

        assert!(root.join("projects/old.md").exists());
        assert!(!root.join(&destination).exists());
    }

    /// Undo must not clobber a note that came back by another route while the archive sat
    /// there -- the whole reason archiving exists is that memory is not ours to destroy.
    #[test]
    fn undoing_an_archive_refuses_to_overwrite_a_note_written_since() {
        let (db, root) = fixture("unarchive_clash");
        ensure(&db).unwrap();
        std::fs::write(root.join("projects/old.md"), "original").unwrap();
        let destination = archive(&db, "projects/old.md").unwrap();
        std::fs::write(root.join("projects/old.md"), "written since").unwrap();

        let refused = unarchive(&db, &destination, "projects/old.md").unwrap_err();
        assert!(refused.contains("exists again"), "{refused}");
        assert_eq!(
            std::fs::read_to_string(root.join("projects/old.md")).unwrap(),
            "written since"
        );
    }

    #[test]
    fn an_already_archived_note_is_not_archived_again() {
        let (db, root) = fixture("double");
        ensure(&db).unwrap();
        std::fs::write(root.join("x.md"), "x").unwrap();
        let destination = archive(&db, "x.md").unwrap();
        let refused = archive(&db, &destination).unwrap_err();
        assert!(refused.contains("already archived"), "{refused}");
    }
}
