use std::process::Command;

fn main() {
    tauri_build::build();

    // Bake the exact commit this binary was built from into AETHER1_GIT_COMMIT, so the
    // running app can compare itself against the latest commit on GitHub (see main.rs's
    // update check) without needing a separate release/version number scheme.
    let commit = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=AETHER1_GIT_COMMIT={commit}");

    // Bake in the human-readable "revision" half of the version scheme: the PR number of
    // the most recently merged pull request reachable from HEAD. This is what turns into
    // "Rev13" in APP_VERSION (main.rs) -- deliberately not a hand-maintained version
    // number, since PR merges already happen on every release and a human would inevitably
    // forget to bump a separate counter.
    //
    // **Two subject shapes count, not one.** This used to look only for GitHub's default
    // merge-commit subject, "Merge pull request #N from ...", which silently misreports the
    // build whenever a PR lands any other way: a squash merge is titled "Whatever it did
    // (#N)", and so is a merge commit given a custom title. Both have happened on this
    // repo, and the symptom is a binary that names an older PR than the code it contains --
    // which is worse than no revision at all, because it is read as proof that a rebuild
    // did not take. Subjects are scanned newest-first and the first recognisable one wins.
    let subjects = Command::new("git")
        .args(["log", "-60", "--format=%s", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .unwrap_or_default();

    /// The PR number a commit subject claims, in either of the shapes a merge can take.
    fn pr_number(subject: &str) -> Option<String> {
        let digits_at = |rest: &str| -> Option<String> {
            let n: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            (!n.is_empty()).then_some(n)
        };
        // A squash or a custom-titled merge: "... (#126)". Taken from the right, so a "#"
        // earlier in the title cannot be mistaken for it.
        if let Some(head) = subject.strip_suffix(')') {
            if let Some((_, rest)) = head.rsplit_once("(#") {
                if let Some(n) = digits_at(rest) {
                    if rest.len() == n.len() {
                        return Some(n);
                    }
                }
            }
        }
        // GitHub's default merge commit: "Merge pull request #126 from owner/branch".
        subject
            .strip_prefix("Merge pull request #")
            .and_then(digits_at)
    }

    let pr_rev = subjects
        .lines()
        .find_map(pr_number)
        .unwrap_or_else(|| "0".to_string());

    println!("cargo:rustc-env=AETHER1_PR_REV={pr_rev}");

    // The version as a person reads it: `Ver 0.4.126`. The major and minor come from
    // Cargo.toml (which tauri.conf.json's `version` tracks), so the printed version can no
    // longer drift from the package's own -- it used to be hand-written as "0.3" in main.rs
    // while the package said 0.4.0, and nothing made the two agree.
    let major = std::env::var("CARGO_PKG_VERSION_MAJOR").unwrap_or_else(|_| "0".to_string());
    let minor = std::env::var("CARGO_PKG_VERSION_MINOR").unwrap_or_else(|_| "0".to_string());
    println!("cargo:rustc-env=AETHER1_VERSION=Ver {major}.{minor}.{pr_rev}");

    // Re-run this build script (and thus refresh the embedded commit/PR rev) whenever the
    // repo's HEAD moves -- a branch switch changes .git/HEAD itself, while a same-branch
    // commit or `git pull` only moves the ref file under .git/refs/heads.
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/refs/heads");
}
