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
    // the most recently merged pull request reachable from HEAD (i.e. the nearest ancestor
    // commit whose message starts with GitHub's standard "Merge pull request #N..."). This
    // is what turns into "Rev13" in APP_VERSION (main.rs) -- deliberately not a hand-maintained
    // version number, since PR merges already happen on every release and a human would
    // inevitably forget to bump a separate counter.
    let pr_rev = Command::new("git")
        .args(["log", "-1", "--grep=^Merge pull request #", "--format=%s", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|subject| {
            subject
                .trim()
                .split('#')
                .nth(1)
                .and_then(|rest| rest.split(|c: char| !c.is_ascii_digit()).next())
                .map(|s| s.to_string())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "0".to_string());

    println!("cargo:rustc-env=AETHER1_PR_REV={pr_rev}");

    // Re-run this build script (and thus refresh the embedded commit/PR rev) whenever the
    // repo's HEAD moves -- a branch switch changes .git/HEAD itself, while a same-branch
    // commit or `git pull` only moves the ref file under .git/refs/heads.
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/refs/heads");
}
