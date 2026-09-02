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

/// Where the vault lives unless the operator says otherwise. Under home so that the
/// existing tool path guards already reach it.
pub const DEFAULT_VAULT_DIR: &str = "Aether1Vault";
const VAULT_PATH_SETTING: &str = "vault_path";
const IMPORT_FLAG_SETTING: &str = "vault_imported_memories";

/// Notes loaded into every prompt, in this order. Kept deliberately short: this is the
/// part that costs context on every single turn, and everything else is one read away.
const ALWAYS_LOADED: &[&str] = &["INDEX.md", "profile.md", "machine.md"];

/// How much of the always-loaded set to accept. A note that grows past this is a note that
/// should have been split, and truncating is better than crowding out the conversation.
const MAX_PRIMED_BYTES: usize = 16 * 1024;

fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => match std::env::var_os("HOME") {
            Some(home) => PathBuf::from(home).join(rest),
            None => PathBuf::from(path),
        },
        None => PathBuf::from(path),
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The configured vault location.
pub fn vault_path(db: &MemoryDb) -> PathBuf {
    let configured = db.get_setting_string(VAULT_PATH_SETTING, "");
    if configured.trim().is_empty() {
        home().join(DEFAULT_VAULT_DIR)
    } else {
        expand_home(configured.trim())
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
        let path = if dir.is_empty() { root.clone() } else { root.join(dir) };
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
         question calls for them, following the index below. Write with append_note and \
         write_note; both need the operator's approval, so say what you intend to record \
         and why.\n",
        root.display()
    );

    for name in ALWAYS_LOADED {
        let path = root.join(name);
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        let contents = if contents.len() > MAX_PRIMED_BYTES {
            format!("{}\n[note truncated -- it has grown too large to load every turn]", &contents[..MAX_PRIMED_BYTES])
        } else {
            contents
        };
        block.push_str(&format!("\n--- {name} ---\n{contents}"));
    }

    block
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
    link_from_index(&root, "memories", "things the operator asked to be remembered");

    Ok(relative_name(db, &path))
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

/// A note's path relative to the vault, for reporting which notes were consulted.
pub fn relative_name(db: &MemoryDb, path: &Path) -> String {
    path.strip_prefix(vault_path(db))
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
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
        assert!(index.contains("[[profile]]"), "the index links notes together");
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
        db.set_memory("favorite_editor", "helix", "general").unwrap();
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
        assert!(block.contains("Prefers helix."), "the profile is loaded every turn");
        assert!(block.contains("--- INDEX.md ---"));
    }

    #[test]
    fn priming_an_absent_vault_says_nothing_at_all() {
        let (db, _root) = fixture("no_vault");
        assert_eq!(prime(&db), "", "no vault means no vault section in the prompt");
    }

    #[test]
    fn an_oversized_note_is_truncated_rather_than_crowding_out_the_conversation() {
        let (db, root) = fixture("oversize");
        ensure(&db).unwrap();
        std::fs::write(root.join("profile.md"), "x".repeat(MAX_PRIMED_BYTES * 2)).unwrap();

        let block = prime(&db);
        assert!(block.contains("note truncated"), "an enormous note must not be pasted whole");
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
        assert!(contents.contains("tycho"), "the earlier fact is still there");
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
}
