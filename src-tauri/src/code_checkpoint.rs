//! A way back, taken before the agent starts changing things.
//!
//! Giving a model a shell inside a sandbox settles what it can reach. It does not settle
//! what it can ruin inside the one folder it is *supposed* to reach, and that folder is the
//! operator's real work. The old answer to that was a table of refused `git` subcommands --
//! no `reset`, no `clean`, no `checkout` -- which stopped being enforceable the moment the
//! sandbox got a shell, because a shell can run git. A guard that can be walked around is
//! worse than none: it reads as protection and is not.
//!
//! So the guarantee moves from "it cannot destroy your work" to **"whatever it does, you
//! can put it back"**, which is both true and stronger.
//!
//! A checkpoint is a real commit object holding the whole working tree -- tracked files,
//! untracked files, staged and unstaged alike -- parented on HEAD and pointed at by a ref
//! under `refs/aether1/checkpoints/`. It is built through a **temporary index**, so taking
//! one does not stage anything, does not touch HEAD, does not move a branch and is invisible
//! to `git status`. Nothing the operator had in flight is disturbed by being backed up.
//!
//! Restoring is deliberately additive: files the checkpoint held are written back, and files
//! that did not exist then are left alone and named in the report. A revert that deleted
//! things would be one more way to lose work, which is the opposite of the point.
//!
//! A folder that is not a git repository gets no checkpoint and is told so rather than
//! quietly going unprotected.

use std::path::Path;
use std::process::Command;

/// Where checkpoint refs live. Outside `refs/heads` and `refs/tags`, so they are not
/// branches to be pushed, do not show up in `git branch`, and cannot be confused with the
/// operator's own history.
const REF_PREFIX: &str = "refs/aether1/checkpoints";

/// One checkpoint, as the operator sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    /// The full ref, which is also how it is named back to `revert`.
    pub reference: String,
    /// When it was taken, as git formats it.
    pub when: String,
}

/// Runs a git command in the workspace and returns its stdout, or a readable error.
fn git(root: &Path, args: &[&str], index: Option<&Path>) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(root).args(args);
    if let Some(index) = index {
        cmd.env("GIT_INDEX_FILE", index);
    }
    // Aether1's own operation, not the model's, so it runs on the host -- but with the
    // workspace as its only subject, and never with anything the model wrote as a flag.
    let out = cmd
        .output()
        .map_err(|e| format!("git could not be run: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Whether this folder is a git repository, which is what decides if a checkpoint is
/// possible at all.
pub fn is_repo(root: &Path) -> bool {
    git(root, &["rev-parse", "--git-dir"], None).is_ok()
}

/// Takes a checkpoint of the whole working tree. `Ok(None)` when the folder is not a git
/// repository, which is a fact to report rather than an error to fail on.
pub fn take(root: &Path, why: &str) -> Result<Option<Checkpoint>, String> {
    if !is_repo(root) {
        return Ok(None);
    }
    // A temporary index, so `git add -A` here stages nothing in the operator's own index.
    // Named for the process so two Aether1s on one machine cannot collide.
    let index = std::env::temp_dir().join(format!(
        "aether1_checkpoint_index_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_file(&index);
    let result = (|| {
        git(root, &["add", "-A"], Some(&index))?;
        let tree = git(root, &["write-tree"], Some(&index))?;
        // Parented on HEAD when there is one. A repository with no commits yet has none,
        // and a parentless checkpoint is still a complete snapshot of the tree.
        let head = git(root, &["rev-parse", "HEAD"], None).ok();
        let message = format!("aether1 checkpoint: {why}");
        let mut args: Vec<&str> = vec!["commit-tree", &tree, "-m", &message];
        if let Some(head) = head.as_deref() {
            args.push("-p");
            args.push(head);
        }
        let commit = git(root, &args, None)?;
        let stamp = stamp();
        let reference = format!("{REF_PREFIX}/{stamp}");
        git(root, &["update-ref", &reference, &commit], None)?;
        Ok(Checkpoint {
            reference,
            when: stamp,
        })
    })();
    let _ = std::fs::remove_file(&index);
    result.map(Some)
}

/// A sortable, readable name: the refs list in order because the names do.
fn stamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Seconds since the epoch, zero-padded so lexical order is chronological order for the
    // next few thousand years. Paired with a readable date by `list`, from git itself.
    format!("{now:012}")
}

/// Every checkpoint in this workspace, newest first.
pub fn list(root: &Path) -> Result<Vec<Checkpoint>, String> {
    if !is_repo(root) {
        return Ok(Vec::new());
    }
    let out = git(
        root,
        &[
            "for-each-ref",
            "--sort=-refname",
            "--format=%(refname)%09%(creatordate:iso)",
            REF_PREFIX,
        ],
        None,
    )?;
    Ok(out
        .lines()
        .filter_map(|line| {
            let (reference, when) = line.split_once('\t')?;
            Some(Checkpoint {
                reference: reference.to_string(),
                when: when.to_string(),
            })
        })
        .collect())
}

/// The newest checkpoint, which is what `revert` uses when it is not told which one.
pub fn latest(root: &Path) -> Result<Option<Checkpoint>, String> {
    Ok(list(root)?.into_iter().next())
}

/// Puts the working tree back to a checkpoint, additively: everything the checkpoint held is
/// written back, and anything created since is left where it is and named in the report.
///
/// Deliberately not `read-tree --reset` or `clean`. A revert that deletes is one more way to
/// lose an afternoon, and the whole point of this module is that there is now exactly one
/// direction work can go.
pub fn revert(root: &Path, reference: Option<&str>) -> Result<String, String> {
    let target = match reference {
        Some(r) => list(root)?
            .into_iter()
            .find(|c| c.reference == r || c.reference.ends_with(&format!("/{r}")))
            .ok_or_else(|| format!("there is no checkpoint called {r}"))?,
        None => latest(root)?.ok_or_else(|| {
            "there are no checkpoints in this folder yet. One is taken before AETHER CODE \
             first changes anything."
                .to_string()
        })?,
    };

    // What exists now that the checkpoint did not hold. Worked out before the restore, and
    // reported rather than removed.
    let now = git(root, &["ls-files", "--others", "--exclude-standard"], None)?;
    let then = git(
        root,
        &["ls-tree", "-r", "--name-only", &target.reference],
        None,
    )?;
    let held: Vec<&str> = then.lines().collect();
    let added: Vec<&str> = now
        .lines()
        .filter(|path| !held.contains(path))
        .take(20)
        .collect();

    git(root, &["checkout", &target.reference, "--", "."], None)?;

    let mut report = format!(
        "Put back to the checkpoint from {} ({}).\n{} file(s) restored.\n",
        target.when,
        target.reference,
        held.len()
    );
    if !added.is_empty() {
        report.push_str(&format!(
            "\nLeft alone, because the checkpoint predates them -- delete any you do not \
             want:\n  {}\n",
            added.join("\n  ")
        ));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aether1_ckpt_{name}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"], None).unwrap();
        git(&dir, &["config", "user.email", "t@example.com"], None).unwrap();
        git(&dir, &["config", "user.name", "Test"], None).unwrap();
        std::fs::write(dir.join("kept.txt"), "original\n").unwrap();
        git(&dir, &["add", "-A"], None).unwrap();
        git(&dir, &["commit", "-qm", "first"], None).unwrap();
        dir
    }

    /// The whole promise, in one test: uncommitted work, a model that ruins it in every way
    /// it can -- an edit, a delete, a `git reset --hard` -- and all of it back afterwards.
    #[test]
    fn a_checkpoint_survives_everything_a_shell_can_do() {
        let dir = repo("full");
        std::fs::write(dir.join("kept.txt"), "edited but not committed\n").unwrap();
        std::fs::write(dir.join("untracked.txt"), "never committed at all\n").unwrap();

        let made = take(&dir, "test")
            .unwrap()
            .expect("a repo gets a checkpoint");

        // Now the worst an agent with a shell could do inside the folder.
        std::fs::write(dir.join("kept.txt"), "ruined\n").unwrap();
        std::fs::remove_file(dir.join("untracked.txt")).unwrap();
        git(&dir, &["reset", "--hard", "HEAD"], None).unwrap();
        std::fs::write(dir.join("new-since.txt"), "made by the agent\n").unwrap();

        let report = revert(&dir, Some(&made.reference)).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("kept.txt")).unwrap(),
            "edited but not committed\n",
            "uncommitted edits come back: {report}"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("untracked.txt")).unwrap(),
            "never committed at all\n",
            "so do files git never tracked: {report}"
        );
        assert!(
            dir.join("new-since.txt").is_file(),
            "and nothing is deleted by a revert: {report}"
        );
        assert!(
            report.contains("new-since.txt"),
            "but it is named: {report}"
        );
    }

    /// Taking one must be invisible: no staging, no HEAD move, no branch.
    #[test]
    fn taking_a_checkpoint_does_not_touch_the_operators_git_state() {
        let dir = repo("quiet");
        std::fs::write(dir.join("kept.txt"), "changed\n").unwrap();
        let head_before = git(&dir, &["rev-parse", "HEAD"], None).unwrap();
        let status_before = git(&dir, &["status", "--porcelain"], None).unwrap();

        take(&dir, "test").unwrap().unwrap();

        assert_eq!(
            head_before,
            git(&dir, &["rev-parse", "HEAD"], None).unwrap()
        );
        assert_eq!(
            status_before,
            git(&dir, &["status", "--porcelain"], None).unwrap(),
            "a checkpoint must not stage anything"
        );
        assert!(
            !git(&dir, &["branch", "--list"], None)
                .unwrap()
                .contains("aether1"),
            "and must not leave a branch behind"
        );
    }

    #[test]
    fn a_folder_that_is_not_a_repository_says_so_rather_than_failing() {
        let dir = std::env::temp_dir().join(format!("aether1_ckpt_bare_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(take(&dir, "test").unwrap().is_none());
        assert!(list(&dir).unwrap().is_empty());
        assert!(revert(&dir, None).unwrap_err().contains("no checkpoints"));
    }
}
