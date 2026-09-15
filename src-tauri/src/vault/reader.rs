// Reading the vault from inside Aether1.
//
// The vault has always been a folder you can open, and the answer to "can I see my notes?"
// has always been "open them in Obsidian". That answer is still right -- Obsidian draws a
// better graph than this ever will, and the notes were written to be read that way. But it
// is a strange thing for a companion to know its own memory well enough to search it and
// quote it, and for the operator to have to leave the app to read the same file.
//
// **Everything here is read-only, and that is a boundary rather than a scope.** Nothing in
// this module writes, renames, creates or deletes; the paths that change notes already
// exist as tools, behind the consent path, with undo. A reader that could also write would
// be a second way to change the vault that nothing had approved.
//
// Every name arriving from outside goes through `vault::resolve_note`, which refuses `..`,
// absolute paths and anything not ending in `.md` -- and then through a canonicalised
// containment check, which is the part `resolve_note` cannot do on shape alone: a symlink
// sitting inside the vault and pointing at `~/.ssh` has a perfectly innocent relative path.

use std::path::Path;

use serde::Serialize;

use crate::llm::MemoryDb;

/// How much of one note is handed to the reader.
///
/// Generous, because this is a person reading their own file rather than a model being
/// handed context: a daily note can reach a little over `ROLL_NOTE_AT_BYTES` now that
/// exchanges are verbatim, and a reader that cut it off would have reintroduced the very
/// truncation that change was made to remove. The cap is here so that one pathological
/// file cannot become one enormous response, not to shorten anything real.
const MAX_NOTE_BYTES: usize = 4 * 1024 * 1024;

/// How many notes the graph will draw.
///
/// The same reasoning as `search::MAX_NOTES_SCANNED`: past this it is not a vault, and
/// saying the picture is partial is more honest than spending a long time drawing it.
const MAX_GRAPH_NOTES: usize = 2000;

/// One note as the list shows it. No content: a list that carried every note's text would
/// be the whole vault in one response, and the list exists to choose from.
#[derive(Serialize)]
pub struct NoteSummary {
    pub name: String,
    pub bytes: u64,
    /// Seconds since the epoch, or 0 when the filesystem will not say. The HUD formats it;
    /// sending a formatted date would be this module deciding the operator's locale.
    pub modified: u64,
    /// Loaded into every conversation, and so not archivable. Worth marking in the list
    /// because it is the difference between a note the companion always sees and one it
    /// only reads when the question calls for it.
    pub core: bool,
}

/// One note, opened.
#[derive(Serialize, Debug)]
pub struct NoteView {
    pub name: String,
    pub text: String,
    /// True when `MAX_NOTE_BYTES` cut it. Said out loud rather than left to be noticed:
    /// silently showing part of a file is how somebody concludes something was deleted.
    pub truncated: bool,
    /// `[[wiki links]]` in this note, in the order they appear, each resolved to a real
    /// note name where one exists.
    pub links: Vec<Link>,
    /// Notes that link *here*. Obsidian's backlinks pane, and the reason a note is worth
    /// reading in a graph rather than as a list.
    pub backlinks: Vec<String>,
}

/// A `[[wiki link]]`, and whether it goes anywhere.
///
/// An unresolved link is not an error and is not hidden. Obsidian draws them differently
/// and so should this: a link to a note that has not been written yet is a normal state of
/// a vault, and quietly dropping it would hide the fact that something is missing.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct Link {
    /// What the link says, with any `|alias` already removed.
    pub target: String,
    /// The note it resolves to, or nothing when no note has that name.
    pub note: Option<String>,
}

#[derive(Serialize)]
pub struct Graph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    /// True when the vault has more notes than the graph will draw.
    pub partial: bool,
}

#[derive(Serialize)]
pub struct GraphNode {
    pub name: String,
    /// The folder the note sits in (`daily`, `projects`, `archive`, or empty at the top).
    /// The HUD colours by this, which is what makes the shape of the vault legible at a
    /// glance rather than a uniform cloud of dots.
    pub folder: String,
    pub core: bool,
    /// How many links point at this note. The graph sizes its dots by it, so the notes
    /// everything refers back to are the ones you can see.
    pub links_in: usize,
}

#[derive(Serialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
}

/// Every note in the vault, newest first.
///
/// Newest rather than alphabetical because the question this list answers is almost always
/// "what has been happening", and a vault sorted by name buries today's note in the middle
/// of a year of dailies.
pub fn notes(db: &MemoryDb) -> Vec<NoteSummary> {
    let root = super::vault_path(db);
    let mut names = Vec::new();
    super::collect_notes(&root, &root, &mut names, 0);

    let mut out: Vec<NoteSummary> = names
        .into_iter()
        .map(|name| {
            let meta = std::fs::metadata(root.join(&name)).ok();
            NoteSummary {
                bytes: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                modified: meta
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
                core: super::is_core_note(&name),
                name,
            }
        })
        .collect();

    out.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

/// Opens one note.
pub fn read(db: &MemoryDb, name: &str) -> Result<NoteView, String> {
    let root = super::vault_path(db);
    let path = super::resolve_note(db, name)?;

    // `resolve_note` has already refused the dangerous *shapes* of a path. This refuses the
    // dangerous destinations: a symlink inside the vault pointing anywhere else resolves to
    // a name outside it, and `note_in_vault` compares both sides canonicalised.
    let name = super::note_in_vault(db, &path)
        .ok_or_else(|| format!("{name} is not a note in this vault"))?;

    let bytes = std::fs::read(&path).map_err(|e| format!("could not read {name}: {e}"))?;
    let truncated = bytes.len() > MAX_NOTE_BYTES;
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_NOTE_BYTES)]).to_string();

    let all: Vec<String> = {
        let mut v = Vec::new();
        super::collect_notes(&root, &root, &mut v, 0);
        v
    };

    let links = resolve_links(&links_in(&text), &all);

    // Backlinks are found by reading the other notes, because nothing indexes them. A vault
    // is small enough that this is cheaper than the index would be to keep correct, and an
    // index that can be stale is worse than a scan that cannot.
    let mut backlinks = Vec::new();
    for other in &all {
        if *other == name {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(root.join(other)) else {
            continue;
        };
        if resolve_links(&links_in(&body), &all)
            .iter()
            .any(|l| l.note.as_deref() == Some(name.as_str()))
        {
            backlinks.push(other.clone());
        }
    }
    backlinks.sort();

    Ok(NoteView {
        name,
        text,
        truncated,
        links,
        backlinks,
    })
}

/// The whole vault as notes and the links between them.
pub fn graph(db: &MemoryDb) -> Graph {
    let root = super::vault_path(db);
    let mut names = Vec::new();
    super::collect_notes(&root, &root, &mut names, 0);
    names.sort();
    let partial = names.len() > MAX_GRAPH_NOTES;
    names.truncate(MAX_GRAPH_NOTES);

    let mut edges: Vec<GraphEdge> = Vec::new();
    for name in &names {
        let Ok(body) = std::fs::read_to_string(root.join(name)) else {
            continue;
        };
        for link in resolve_links(&links_in(&body), &names) {
            let Some(to) = link.note else { continue };
            if to == *name {
                continue;
            }
            // One edge per pair per direction. A note that mentions [[profile]] six times
            // is not six times more connected to it; it is connected to it.
            if edges.iter().any(|e| e.from == *name && e.to == to) {
                continue;
            }
            edges.push(GraphEdge {
                from: name.clone(),
                to,
            });
        }
    }

    let nodes = names
        .iter()
        .map(|name| GraphNode {
            folder: Path::new(name)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
            core: super::is_core_note(name),
            links_in: edges.iter().filter(|e| e.to == *name).count(),
            name: name.clone(),
        })
        .collect();

    Graph {
        nodes,
        edges,
        partial,
    }
}

/// Every `[[wiki link]]` in a note, in order, with any `|alias` removed.
///
/// A deliberately small parser rather than a markdown library: the whole of the syntax this
/// needs to understand is two brackets, and pulling in a parser would mean inheriting its
/// opinions about everything else in the file.
fn links_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("]]") else { break };
        let inner = &after[..end];
        // A link cannot span a line. Without this an unclosed `[[` swallows the rest of
        // the note and reports one enormous target -- and skipping ahead to the `]]` that
        // finally turned up would swallow every real link in between with it, so the scan
        // resumes just past the stray brackets instead.
        if inner.contains('\n') {
            rest = after;
            continue;
        }
        let target = inner.split('|').next().unwrap_or("").trim();
        if !target.is_empty() {
            out.push(target.to_string());
        }
        rest = &after[end + 2..];
    }
    out
}

/// Matches link text against real notes.
///
/// By file stem, case-insensitively, because that is how the vault's own notes write them:
/// `[[profile]]` means `profile.md` wherever it happens to live, and archiving a note moves
/// it between folders without breaking a single link. A link that names a folder too is
/// honoured as written, so `[[daily/2026-09-15]]` also resolves.
fn resolve_links(targets: &[String], notes: &[String]) -> Vec<Link> {
    let mut out: Vec<Link> = Vec::new();
    for target in targets {
        let wanted = target.trim_end_matches(".md");
        let note = notes.iter().find(|n| {
            let full = n.trim_end_matches(".md");
            let stem = Path::new(n)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            full.eq_ignore_ascii_case(wanted) || stem.eq_ignore_ascii_case(wanted)
        });
        let link = Link {
            target: target.clone(),
            note: note.cloned(),
        };
        // The same link twice in one note is one link as far as anything reading this is
        // concerned -- the graph, the backlinks pane and the list under a note all want
        // distinct destinations.
        if !out.contains(&link) {
            out.push(link);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault;

    fn fixture(name: &str) -> (MemoryDb, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("aether1_reader_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = MemoryDb::open(dir.join("test.db")).unwrap();
        db.set_setting(
            "vault_path",
            &serde_json::json!(dir.join("vault").to_string_lossy()),
        )
        .unwrap();
        vault::ensure(&db).unwrap();
        (db, dir.join("vault"))
    }

    /// The point of the feature: the notes on disk are the notes the HUD lists.
    #[test]
    fn the_list_is_what_is_in_the_folder() {
        let (db, root) = fixture("list");
        std::fs::write(root.join("projects").join("sourdough.md"), "# Sourdough\n").unwrap();

        let names: Vec<String> = notes(&db).into_iter().map(|n| n.name).collect();
        assert!(names.iter().any(|n| n == "INDEX.md"), "{names:?}");
        assert!(
            names.iter().any(|n| n == "projects/sourdough.md"),
            "{names:?}"
        );
        assert!(
            notes(&db)
                .iter()
                .find(|n| n.name == "INDEX.md")
                .unwrap()
                .core,
            "the index is a core note"
        );
    }

    /// Links are found, resolved by name across folders, and the note they point at knows
    /// it is pointed at. This is the whole of what makes the folder a graph.
    #[test]
    fn a_link_is_found_at_both_ends() {
        let (db, root) = fixture("links");
        std::fs::write(
            root.join("projects").join("bread.md"),
            "# Bread\n\nSee [[profile]] and [[starter]].\n",
        )
        .unwrap();

        let view = read(&db, "projects/bread.md").unwrap();
        let profile = view.links.iter().find(|l| l.target == "profile").unwrap();
        assert_eq!(profile.note.as_deref(), Some("profile.md"));

        // A link to a note nobody has written is reported, not swallowed.
        let starter = view.links.iter().find(|l| l.target == "starter").unwrap();
        assert!(starter.note.is_none(), "unwritten notes stay visible");

        let seen_from = read(&db, "profile.md").unwrap();
        assert!(
            seen_from
                .backlinks
                .contains(&"projects/bread.md".to_string()),
            "{:?}",
            seen_from.backlinks
        );
    }

    /// An alias, a repeat and an unclosed bracket are all things a person writes. None of
    /// them may turn into a wrong link or a runaway one.
    #[test]
    fn the_link_parser_survives_what_people_type() {
        let found = links_in("[[profile|me]] again [[profile]] and [[oops\nnext line [[machine]]");
        assert_eq!(found, vec!["profile", "profile", "machine"]);

        let notes = vec!["profile.md".to_string(), "machine.md".to_string()];
        let resolved = resolve_links(&found, &notes);
        assert_eq!(resolved.len(), 2, "the repeat collapses: {resolved:?}");
    }

    /// The security boundary, stated as a test rather than left to the shape of the code.
    /// Nothing the HUD or a paired browser can type reaches a file outside the vault.
    #[test]
    fn nothing_outside_the_vault_can_be_opened() {
        let (db, _root) = fixture("escape");
        for name in [
            "../../../etc/passwd",
            "../outside.md",
            "/etc/passwd",
            "daily/../../secrets.md",
            "profile.md/../../../etc/hosts",
            "",
        ] {
            assert!(read(&db, name).is_err(), "{name} was allowed through");
        }
    }

    /// A symlink is the case a path check cannot catch on shape alone: `link.md` is a
    /// perfectly ordinary relative name, and it is the destination that is the problem.
    #[cfg(unix)]
    #[test]
    fn a_symlink_pointing_out_of_the_vault_is_refused() {
        let (db, root) = fixture("symlink");
        let secret = root.parent().unwrap().join("secret.md");
        std::fs::write(&secret, "the passphrase is hunter2").unwrap();
        std::os::unix::fs::symlink(&secret, root.join("link.md")).unwrap();

        let err = read(&db, "link.md").unwrap_err();
        assert!(err.contains("not a note in this vault"), "{err}");
    }

    /// The graph is the same links, arranged. A note everything points at is drawn bigger,
    /// which is the whole reason the count is on the node.
    #[test]
    fn the_graph_counts_what_points_at_a_note() {
        let (db, root) = fixture("graph");
        std::fs::write(root.join("projects").join("a.md"), "[[profile]]\n").unwrap();
        std::fs::write(
            root.join("projects").join("b.md"),
            "[[profile]] [[profile]]\n",
        )
        .unwrap();

        let g = graph(&db);
        let profile = g.nodes.iter().find(|n| n.name == "profile.md").unwrap();
        // Three notes point at it: the two written here and the index the vault scaffolds.
        // b.md names it twice and still counts once, which is the part being tested.
        assert_eq!(profile.links_in, 3, "one edge per note, not per mention");
        assert!(
            g.edges
                .iter()
                .any(|e| e.from == "INDEX.md" && e.to == "profile.md"),
            "the scaffolded index is in the graph too"
        );
        assert!(!g.partial);

        let folders: Vec<&str> = g
            .nodes
            .iter()
            .filter(|n| n.name.starts_with("projects/"))
            .map(|n| n.folder.as_str())
            .collect();
        assert!(folders.iter().all(|f| *f == "projects"), "{folders:?}");
    }
}
