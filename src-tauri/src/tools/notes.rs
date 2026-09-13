// Writing to the vault.
//
// These are the tools that let the companion remember something. They are mutating, so
// every one of them is proposed and waits for the operator -- which is the right shape for
// memory in particular: what a companion writes down about you is exactly the thing you
// should get a say over, and an approval card is a chance to correct a fact before it
// becomes one.
//
// Notes are markdown and stay inside the vault. Reading them needs no tool of its own:
// read_file and list_dir already reach the vault, and the index tells the model what to
// reach for.

use serde_json::{json, Value};

use super::{Outcome, Tool, ToolContext};
use crate::vault;

fn string_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing required string argument {key:?}"))
}

fn note_parameters(content_description: &str) -> Value {
    json!({
        "type": "object",
        "properties": {
            "note": {
                "type": "string",
                "description": "Note path relative to the vault, ending in .md -- for example profile.md or projects/aether1.md."
            },
            "content": { "type": "string", "description": content_description }
        },
        "required": ["note", "content"]
    })
}

fn summarize(content: &str) -> String {
    let first = content.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    if first.chars().count() > 70 {
        format!("{}…", first.chars().take(70).collect::<String>())
    } else {
        first.to_string()
    }
}

// -------------------------------------------------------------- append_note

pub struct AppendNote;

impl Tool for AppendNote {
    fn name(&self) -> &'static str {
        "append_note"
    }

    fn description(&self) -> &'static str {
        "Add to the end of a note in the memory vault, creating it if it does not exist. This is how you remember something new. Prefer it over write_note: appending cannot lose what was already there. Link related notes with [[wiki links]] so the vault stays a connected graph."
    }

    fn parameters(&self) -> Value {
        note_parameters(
            "Markdown to add. It is appended as-is, so include its own heading or bullet.",
        )
    }

    fn mutating(&self) -> bool {
        true
    }

    fn preview(&self, args: &Value) -> String {
        format!(
            "Add to {}: {}",
            args.get("note").and_then(Value::as_str).unwrap_or("?"),
            summarize(args.get("content").and_then(Value::as_str).unwrap_or(""))
        )
    }

    fn call(&self, args: &Value, ctx: &ToolContext) -> Result<Outcome, String> {
        let relative = string_arg(args, "note")?;
        let content = string_arg(args, "content")?;
        let path = vault::resolve_note(ctx.db, relative)?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        }

        let existed = path.exists();
        let previous = if existed {
            std::fs::read_to_string(&path)
                .map_err(|e| format!("could not read {}: {e}", path.display()))?
        } else {
            String::new()
        };

        // A blank line between entries, so appended notes stay readable markdown rather
        // than running together into one paragraph.
        let mut next = previous.clone();
        if !next.is_empty() && !next.ends_with("\n\n") {
            next.push('\n');
            if !next.ends_with("\n\n") {
                next.push('\n');
            }
        }
        next.push_str(content.trim_end());
        next.push('\n');

        std::fs::write(&path, &next)
            .map_err(|e| format!("could not write {}: {e}", path.display()))?;

        Ok(Outcome::reversible(
            format!("Added {} bytes to {relative}", content.len()),
            json!({"note": relative, "previous": previous, "created": !existed}),
        ))
    }

    fn undo(&self, undo: &Value, ctx: &ToolContext) -> Result<String, String> {
        undo_note(undo, ctx)
    }
}

// --------------------------------------------------------------- write_note

pub struct WriteNote;

impl Tool for WriteNote {
    fn name(&self) -> &'static str {
        "write_note"
    }

    fn description(&self) -> &'static str {
        "Replace a note in the memory vault entirely. Use this to reorganise or correct a note, or to update the index after adding one. It discards what was there, so append_note is usually the better choice."
    }

    fn parameters(&self) -> Value {
        note_parameters("The complete new contents of the note.")
    }

    fn mutating(&self) -> bool {
        true
    }

    fn preview(&self, args: &Value) -> String {
        let note = args.get("note").and_then(Value::as_str).unwrap_or("?");
        let content = args.get("content").and_then(Value::as_str).unwrap_or("");
        format!(
            "Replace {note} entirely ({} bytes): {}",
            content.len(),
            summarize(content)
        )
    }

    fn call(&self, args: &Value, ctx: &ToolContext) -> Result<Outcome, String> {
        let relative = string_arg(args, "note")?;
        let content = string_arg(args, "content")?;
        let path = vault::resolve_note(ctx.db, relative)?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        }

        let existed = path.exists();
        let previous = if existed {
            std::fs::read_to_string(&path)
                .map_err(|e| format!("could not read {}: {e}", path.display()))?
        } else {
            String::new()
        };

        std::fs::write(&path, content)
            .map_err(|e| format!("could not write {}: {e}", path.display()))?;

        Ok(Outcome::reversible(
            format!(
                "{} {relative} ({} bytes)",
                if existed { "Rewrote" } else { "Created" },
                content.len()
            ),
            json!({"note": relative, "previous": previous, "created": !existed}),
        ))
    }

    fn undo(&self, undo: &Value, ctx: &ToolContext) -> Result<String, String> {
        undo_note(undo, ctx)
    }
}

// -------------------------------------------------------------- archive_note

pub struct ArchiveNote;

impl Tool for ArchiveNote {
    fn name(&self) -> &'static str {
        "archive_note"
    }

    fn description(&self) -> &'static str {
        "Move a note into the vault's archive/ folder, for a note that has stopped being true or whose contents have been folded into another note. Nothing is deleted -- the note keeps its text and its links, and stays readable and searchable. Use this after consolidating daily notes into a topic note, or when a fact has been superseded. Requires approval."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "note": {
                    "type": "string",
                    "description": "Note path relative to the vault, ending in .md -- for example daily/2026-04-01.md."
                },
                "reason": {
                    "type": "string",
                    "description": "Why this note is being archived, in one line. Shown to the operator on the approval card."
                }
            },
            "required": ["note"]
        })
    }

    fn mutating(&self) -> bool {
        true
    }

    fn preview(&self, args: &Value) -> String {
        let note = args.get("note").and_then(Value::as_str).unwrap_or("?");
        match args.get("reason").and_then(Value::as_str) {
            Some(reason) if !reason.trim().is_empty() => {
                format!("Archive {note}: {}", summarize(reason))
            }
            _ => format!("Archive {note}"),
        }
    }

    fn call(&self, args: &Value, ctx: &ToolContext) -> Result<Outcome, String> {
        let relative = string_arg(args, "note")?.trim().to_string();
        let destination = vault::archive(ctx.db, &relative)?;

        Ok(Outcome::reversible(
            format!("Moved {relative} to {destination}"),
            json!({"archived": destination, "original": relative}),
        ))
    }

    fn undo(&self, undo: &Value, ctx: &ToolContext) -> Result<String, String> {
        let archived = undo
            .get("archived")
            .and_then(Value::as_str)
            .ok_or("the undo record has no archived path")?;
        let original = undo
            .get("original")
            .and_then(Value::as_str)
            .ok_or("the undo record has no original path")?;
        vault::unarchive(ctx.db, archived, original)?;
        Ok(format!("Moved {archived} back to {original}"))
    }
}

/// Both note tools undo the same way: put the previous text back, or remove a note that
/// did not exist before.
fn undo_note(undo: &Value, ctx: &ToolContext) -> Result<String, String> {
    let relative = undo
        .get("note")
        .and_then(Value::as_str)
        .ok_or("the undo record has no note")?;
    let path = vault::resolve_note(ctx.db, relative)?;

    if undo.get("created").and_then(Value::as_bool) == Some(true) {
        std::fs::remove_file(&path)
            .map_err(|e| format!("could not remove {}: {e}", path.display()))?;
        return Ok(format!("Removed {relative} again"));
    }

    let previous = undo
        .get("previous")
        .and_then(Value::as_str)
        .ok_or("the undo record has no previous contents")?;
    std::fs::write(&path, previous)
        .map_err(|e| format!("could not restore {}: {e}", path.display()))?;
    Ok(format!("Restored {relative}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::MemoryDb;

    fn fixture(name: &str) -> (MemoryDb, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("aether1_notes_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = MemoryDb::open(dir.join("memory.db")).unwrap();
        db.set_setting(
            "vault_path",
            &json!(dir.join("vault").to_string_lossy().to_string()),
        )
        .unwrap();
        let root = crate::vault::ensure(&db).unwrap();
        (db, root)
    }

    #[test]
    fn appending_adds_without_losing_what_was_there() {
        let (db, root) = fixture("append");
        let ctx = ToolContext::new(&db);
        let before = std::fs::read_to_string(root.join("profile.md")).unwrap();

        let outcome = AppendNote
            .call(
                &json!({"note": "profile.md", "content": "- Prefers helix"}),
                &ctx,
            )
            .unwrap();
        let after = std::fs::read_to_string(root.join("profile.md")).unwrap();
        assert!(after.starts_with(&before), "appending keeps what was there");
        assert!(after.contains("Prefers helix"));

        AppendNote.undo(&outcome.undo.unwrap(), &ctx).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("profile.md")).unwrap(),
            before
        );
    }

    #[test]
    fn appending_to_a_new_note_creates_it_and_undo_removes_it() {
        let (db, root) = fixture("append_new");
        let ctx = ToolContext::new(&db);

        let outcome = AppendNote
            .call(
                &json!({"note": "projects/aether1.md", "content": "# Aether1\n\nLinked from [[INDEX]]."}),
                &ctx,
            )
            .unwrap();
        assert!(root.join("projects/aether1.md").exists());

        AppendNote.undo(&outcome.undo.unwrap(), &ctx).unwrap();
        assert!(!root.join("projects/aether1.md").exists());
    }

    #[test]
    fn rewriting_restores_the_old_contents_on_undo() {
        let (db, root) = fixture("write");
        let ctx = ToolContext::new(&db);
        let before = std::fs::read_to_string(root.join("machine.md")).unwrap();

        let outcome = WriteNote
            .call(
                &json!({"note": "machine.md", "content": "# Machine\n\nRewritten.\n"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("machine.md")).unwrap(),
            "# Machine\n\nRewritten.\n"
        );

        WriteNote.undo(&outcome.undo.unwrap(), &ctx).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("machine.md")).unwrap(),
            before
        );
    }

    #[test]
    fn notes_cannot_be_written_outside_the_vault() {
        let (db, _root) = fixture("escape");
        let ctx = ToolContext::new(&db);
        for bad in ["../escape.md", "/etc/passwd.md", "notes/../../escape.md"] {
            assert!(
                AppendNote
                    .call(&json!({"note": bad, "content": "x"}), &ctx)
                    .is_err(),
                "{bad} should be refused"
            );
        }
    }

    #[test]
    fn a_note_must_be_markdown() {
        let (db, _root) = fixture("md");
        let ctx = ToolContext::new(&db);
        assert!(WriteNote
            .call(&json!({"note": ".bashrc", "content": "evil"}), &ctx)
            .is_err());
    }

    // ---------------------------------------------------- archive_note

    /// Archiving changes the vault, so it goes through the consent path like every other
    /// mutating tool. The registry gate is what enforces that; this pins the declaration
    /// the gate reads.
    #[test]
    fn archiving_is_a_change_and_says_so() {
        assert!(ArchiveNote.mutating());
        let preview = ArchiveNote.preview(&json!({
            "note": "daily/2026-04-01.md",
            "reason": "folded into projects/sourdough.md"
        }));
        assert!(preview.contains("daily/2026-04-01.md"), "{preview}");
        assert!(preview.contains("sourdough"), "{preview}");
    }

    #[test]
    fn archiving_a_note_moves_it_and_can_be_undone() {
        let (db, root) = fixture("archive_tool");
        let ctx = ToolContext::new(&db);
        std::fs::write(root.join("projects/spare.md"), "# Spare\n").unwrap();

        let outcome = ArchiveNote
            .call(&json!({"note": "projects/spare.md"}), &ctx)
            .unwrap();
        assert!(root.join("archive/projects/spare.md").exists());

        let undo = outcome.undo.expect("an archive records how to undo itself");
        ArchiveNote.undo(&undo, &ctx).unwrap();
        assert!(root.join("projects/spare.md").exists());
        assert!(!root.join("archive/projects/spare.md").exists());
    }

    /// A note that isn't there is a mistake to report, not a file to create.
    #[test]
    fn archiving_a_note_that_does_not_exist_is_an_error() {
        let (db, _) = fixture("archive_missing");
        let ctx = ToolContext::new(&db);
        let refused = ArchiveNote
            .call(&json!({"note": "projects/ghost.md"}), &ctx)
            .unwrap_err();
        assert!(refused.contains("not a note"), "{refused}");
    }

    /// The path guard that keeps notes inside the vault applies here too -- archiving is a
    /// move, and a move that could name any path would be a way out of the vault.
    #[test]
    fn archiving_cannot_name_a_path_outside_the_vault() {
        let (db, _) = fixture("archive_escape");
        let ctx = ToolContext::new(&db);
        for path in ["../secrets.md", "/etc/passwd.md", "notes.txt"] {
            assert!(
                ArchiveNote.call(&json!({"note": path}), &ctx).is_err(),
                "{path} should be refused"
            );
        }
    }
}
