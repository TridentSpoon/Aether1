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

    // Re-run this build script (and thus refresh the embedded commit) whenever the
    // repo's HEAD moves -- a branch switch changes .git/HEAD itself, while a same-branch
    // commit or `git pull` only moves the ref file under .git/refs/heads.
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/refs/heads");
}
