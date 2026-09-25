//! Updating a copy that was installed rather than cloned.
//!
//! Two things were wrong with the updater before this. It compared the running build's
//! commit against the tip of `main`, while `release.yml` builds tagged, versioned bundles
//! and attaches them to a Release -- so the check watched one thing and the pipeline shipped
//! another, and a fixed README told the operator they were "behind". And when it found a
//! difference its only answer was `git pull` in a checkout that an installed copy does not
//! have.
//!
//! So: the check reads `/repos/{owner}/{repo}/releases/latest` with the signed-in token (see
//! `github_auth`), and the fetch pulls the release asset for this platform.
//!
//! **Authentication proves who may download; a signature proves what was downloaded.** They
//! are different questions and both get asked. A token can be stolen, a release asset can be
//! replaced, and a bundle can be handed over on a USB stick -- none of which the sign-in
//! notices. Every asset is signed with minisign in the release pipeline, the public key is
//! compiled into this binary, and a bundle whose signature does not verify is deleted rather
//! than installed. That inverts the problem from "keep the download secret" to "make the
//! location irrelevant", which is the only version of it that stays true.
//!
//! **Nothing installs itself.** The offline bundles are around half a gigabyte and they
//! replace the app the operator is looking at. This says a version is available, downloads it
//! when asked with the progress the HUD already knows how to draw, verifies it, and then
//! hands over the installer and the file's location. Pressing the last button is the
//! operator's.
//!
//! **The checkout keeps its own path.** Where a `.git` directory is present, pull-and-rebuild
//! is still the right update and `main.rs` still does that. Which of the two modes a copy is
//! in is shown, not inferred silently -- see [`Mode`].

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::github_auth;
use crate::installs::Version;

/// The minisign public key the release pipeline's private key matches, baked in from
/// `AETHER1_MINISIGN_PUBLIC_KEY` (see `build.rs`). The bare base64 line out of a
/// `minisign.pub` file, without the comment line above it.
///
/// Empty in a build made without one. Then no download is offered at all: an unverifiable
/// half-gigabyte binary is worse than no update mechanism, because it looks like one.
pub const MINISIGN_PUBLIC_KEY: &str = match option_env!("AETHER1_MINISIGN_PUBLIC_KEY") {
    Some(key) => key,
    None => "",
};

/// The extension `minisign -Sm <file>` gives its signature, and therefore the name of the
/// sibling asset the pipeline attaches next to each bundle.
const SIGNATURE_SUFFIX: &str = ".minisig";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// How a copy of AETHER1 updates itself. Kept explicit and reported to the operator, because
/// the two modes fail in completely different ways and "it says the update failed" is not a
/// bug report until you know which one was running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Mode {
    /// A git checkout: `git pull --ff-only` and rebuild, which is what the developer wants
    /// and is strictly better than downloading a bundle of your own work.
    Checkout,
    /// Installed from a bundle: compare against the latest Release and fetch its asset.
    Release,
}

pub fn mode() -> Mode {
    if crate::project_root().join(".git").exists() {
        Mode::Checkout
    } else {
        Mode::Release
    }
}

#[derive(Debug, Clone, Deserialize)]
struct RawAsset {
    id: u64,
    name: String,
    size: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct RawRelease {
    tag_name: String,
    name: Option<String>,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<RawAsset>,
}

/// One downloadable file on a release, and the signature that goes with it.
#[derive(Debug, Clone, Serialize)]
pub struct Asset {
    pub name: String,
    pub size: u64,
    /// The API id, not a browser URL. A private repository's `browser_download_url` redirects
    /// to storage that the token cannot follow; `/releases/assets/{id}` with
    /// `Accept: application/octet-stream` is the endpoint that works while the repo is
    /// private, and it keeps working after it is public.
    pub id: u64,
    /// The `.minisig` sibling, when the pipeline attached one. `None` means this asset cannot
    /// be verified, and [`download_and_verify`] refuses it rather than trusting it.
    pub signature_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Release {
    pub tag: String,
    pub name: String,
    pub html_url: String,
    /// The asset for the platform this binary is running on, if the release has one.
    pub asset: Option<Asset>,
}

#[derive(Debug)]
pub enum ReleaseError {
    /// Nobody is signed in and `gh` is not standing in for them either. The one failure with
    /// an obvious next step, so the callers offer it specifically.
    NotSignedIn,
    /// Signed in, but GitHub will not show this repository's releases -- which for a private
    /// repository means this account is not a collaborator on it. Distinguished from
    /// `NotSignedIn` because the fix is completely different: ask the owner, do not sign in
    /// again.
    NoAccess,
    /// The repository has no published release yet.
    NoRelease,
    Network(String),
    Other(String),
}

impl ReleaseError {
    pub fn message(&self) -> String {
        match self {
            ReleaseError::NotSignedIn => {
                "sign in to GitHub to check for new versions -- AETHER1's releases are on a \
                 private repository, so it has to be you asking for them"
                    .to_string()
            }
            ReleaseError::NoAccess => {
                "signed in, but this GitHub account cannot see AETHER1's releases -- ask the \
                 project's owner to add you to the repository"
                    .to_string()
            }
            ReleaseError::NoRelease => {
                "there are no published releases to compare against yet".to_string()
            }
            ReleaseError::Network(e) => format!("could not reach GitHub: {e}"),
            ReleaseError::Other(e) => e.clone(),
        }
    }
}

/// The bundle name this platform installs, as `release.yml` names it. Matched as a prefix so
/// a future `aether1-offline-linux-aarch64.tar.gz` needs nothing here, and so the `.minisig`
/// beside it is never mistaken for the bundle itself.
fn wanted_asset_prefixes() -> &'static [&'static str] {
    if cfg!(target_os = "windows") {
        &["Aether1-Setup"]
    } else {
        // The offline bundle first: it is the one that needs no network on the target
        // machine, which is the point of having it. The slim one is the fallback for a
        // release that only carries that.
        &["aether1-offline-linux", "aether1-slim-linux"]
    }
}

fn pick_asset(assets: &[RawAsset]) -> Option<Asset> {
    for prefix in wanted_asset_prefixes() {
        let found = assets
            .iter()
            .find(|a| a.name.starts_with(prefix) && !a.name.ends_with(SIGNATURE_SUFFIX));
        if let Some(asset) = found {
            let signature = format!("{}{SIGNATURE_SUFFIX}", asset.name);
            return Some(Asset {
                name: asset.name.clone(),
                size: asset.size,
                id: asset.id,
                signature_id: assets.iter().find(|a| a.name == signature).map(|a| a.id),
            });
        }
    }
    None
}

/// Ask GitHub for the newest published release of the update repository. Blocking; never call
/// it on the main thread.
pub fn latest(repo: &str, agent_label: &str) -> Result<Release, ReleaseError> {
    let token = github_auth::token_for_requests().ok_or(ReleaseError::NotSignedIn)?;
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let response = ureq::get(&url)
        .config()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .build()
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", agent_label)
        .call()
        .map_err(|e| match &e {
            // GitHub hides a private repository behind a 404 rather than a 403, so a token
            // that is valid but not a collaborator's looks identical to a repository that
            // does not exist. Given the token got this far, "you are not on the list" is the
            // honest reading.
            ureq::Error::StatusCode(404) => ReleaseError::NoAccess,
            ureq::Error::StatusCode(401) => ReleaseError::NotSignedIn,
            ureq::Error::StatusCode(403) => ReleaseError::NoAccess,
            _ => ReleaseError::Network(e.to_string()),
        })?;

    let raw: RawRelease = response
        .into_body()
        .read_json()
        .map_err(|e| ReleaseError::Other(format!("GitHub sent back something unreadable: {e}")))?;

    // `/releases/latest` excludes drafts and prereleases already; checked anyway because a
    // draft release's assets are visible to the people who can see the repository, and
    // offering one as an update is offering something the owner has not published.
    if raw.draft || raw.prerelease {
        return Err(ReleaseError::NoRelease);
    }

    Ok(Release {
        asset: pick_asset(&raw.assets),
        name: raw.name.unwrap_or_else(|| raw.tag_name.clone()),
        tag: raw.tag_name,
        html_url: raw.html_url,
    })
}

/// Whether `latest` is newer than what is running. `None` when the two cannot be compared --
/// a hand-made tag that is not a version, which is reported as "cannot tell" rather than
/// guessed at in either direction.
pub fn is_newer(running: &str, latest_tag: &str) -> Option<bool> {
    // `v0.4.152` is the tag `release.yml` is triggered by, and the `v` has to come off first:
    // `Version::parse` reads three dot-separated numbers, and `v0` is not one, so the whole tag
    // falls through to its opaque branch and compares against nothing. The symptom is an
    // installed copy that can never tell whether it is behind.
    let latest = Version::parse(latest_tag.trim().trim_start_matches(['v', 'V']))?;
    let running = Version::parse(running)?;
    running.older_than(&latest)
}

// ---------------------------------------------------------------------------------------
// Fetching, and refusing to accept anything unsigned.
// ---------------------------------------------------------------------------------------

#[derive(Debug)]
pub enum FetchError {
    /// No public key compiled in, so nothing downloaded could be checked. Refused up front:
    /// a build that cannot verify a bundle must not be the thing that downloads one.
    NoPublicKey,
    /// The release has the bundle but no `.minisig` beside it.
    Unsigned,
    /// The signature does not match: either the file is not what was signed, or it was signed
    /// by a key that is not this project's. One variant for both, because from the operator's
    /// side they are the same fact -- this file is not the one AETHER1 published -- and telling
    /// them which would be telling them about somebody else's key.
    ///
    /// `deleted` is true when the file was one this downloaded and has now removed. A bundle
    /// that failed verification must not be left somewhere an operator can find and run by
    /// hand; a file they already had is theirs, and deleting it would be taking something away
    /// that was not ours to take.
    BadSignature {
        deleted: bool,
    },
    NotSignedIn,
    Network(String),
    Disk(String),
}

impl FetchError {
    pub fn message(&self) -> String {
        match self {
            FetchError::NoPublicKey => {
                "this build has no signing key compiled into it, so it cannot check that a \
                 download is genuine -- and will not install one it cannot check"
                    .to_string()
            }
            FetchError::Unsigned => {
                "that release has no signature attached, so there is no way to tell whether \
                 the download is the one the project built"
                    .to_string()
            }
            FetchError::BadSignature { deleted: true } => {
                "the download's signature did not match -- the file has been deleted. This is \
                 either a corrupted download or a file that is not the one the project \
                 published; trying again is the right first move."
                    .to_string()
            }
            FetchError::BadSignature { deleted: false } => {
                "that file's signature does not match the key built into this copy of AETHER1 \
                 -- it is not the file this project published, or not all of it arrived. It has \
                 been left where it is; nothing here will install it."
                    .to_string()
            }
            FetchError::NotSignedIn => ReleaseError::NotSignedIn.message(),
            FetchError::Network(e) => format!("the download failed: {e}"),
            FetchError::Disk(e) => e.clone(),
        }
    }
}

/// Where a downloaded bundle is put: beside the app's own data rather than a temporary
/// directory, because it is half a gigabyte the operator may want to keep, move to another
/// machine, or delete deliberately.
pub fn download_dir() -> PathBuf {
    crate::project_root().join("downloads")
}

fn asset_url(repo: &str, id: u64) -> String {
    format!("https://api.github.com/repos/{repo}/releases/assets/{id}")
}

fn fetch_asset_bytes(repo: &str, id: u64, agent_label: &str) -> Result<Vec<u8>, FetchError> {
    let token = github_auth::token_for_requests().ok_or(FetchError::NotSignedIn)?;
    let response = ureq::get(asset_url(repo, id))
        .config()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .build()
        .header("Authorization", format!("Bearer {token}"))
        // Without this the endpoint answers with the asset's JSON metadata rather than the
        // asset, which reads as a corrupt download rather than as the wrong request.
        .header("Accept", "application/octet-stream")
        .header("User-Agent", agent_label)
        .call()
        .map_err(|e| FetchError::Network(e.to_string()))?;
    let mut bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| FetchError::Network(e.to_string()))?;
    Ok(bytes)
}

/// Download the release asset and verify it, calling `progress` with (bytes so far, total)
/// as it goes so the HUD's progress bar has something to draw. Blocking, for as long as half
/// a gigabyte takes.
///
/// On success the verified file's path is returned and nothing has been installed. On a
/// signature failure the file is gone.
pub fn download_and_verify(
    repo: &str,
    asset: &Asset,
    agent_label: &str,
    mut progress: impl FnMut(u64, u64),
) -> Result<PathBuf, FetchError> {
    if MINISIGN_PUBLIC_KEY.is_empty() {
        return Err(FetchError::NoPublicKey);
    }
    let public_key = minisign_verify::PublicKey::from_base64(MINISIGN_PUBLIC_KEY)
        .map_err(|e| FetchError::Disk(format!("the compiled-in signing key is unreadable: {e}")))?;
    let signature_id = asset.signature_id.ok_or(FetchError::Unsigned)?;

    // The signature first. It is a few hundred bytes, and fetching it before the bundle means
    // a release that was published without one costs nothing instead of half a gigabyte.
    let signature_bytes = fetch_asset_bytes(repo, signature_id, agent_label)?;
    let signature_text = String::from_utf8(signature_bytes)
        .map_err(|_| FetchError::Disk("the release's signature is not text".to_string()))?;
    let signature = minisign_verify::Signature::decode(&signature_text)
        .map_err(|e| FetchError::Disk(format!("the release's signature is unreadable: {e}")))?;

    let dir = download_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| FetchError::Disk(format!("could not make somewhere to download to: {e}")))?;
    let path = dir.join(&asset.name);

    let token = github_auth::token_for_requests().ok_or(FetchError::NotSignedIn)?;
    let response = ureq::get(asset_url(repo, asset.id))
        .config()
        // No global timeout on this one: a global deadline on a half-gigabyte download over a
        // slow line is a timeout on the size of the file, not on anything being wrong.
        .timeout_global(None)
        .build()
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/octet-stream")
        .header("User-Agent", agent_label)
        .call()
        .map_err(|e| FetchError::Network(e.to_string()))?;

    let mut reader = response.into_body().into_reader();
    let mut file = std::fs::File::create(&path)
        .map_err(|e| FetchError::Disk(format!("could not write the download: {e}")))?;

    // Hashed as it is written rather than read back afterwards: these bundles do not belong in
    // memory, and reading half a gigabyte off disk a second time to verify it is a minute of
    // nothing happening.
    // A key-id mismatch fails here rather than at the end, which is the whole reason the
    // signature is fetched first: a release signed by a key this build does not know is half a
    // gigabyte not worth downloading. Nothing has been written yet, so nothing is deleted.
    let mut verifier = public_key
        .verify_stream(&signature)
        .map_err(|_| FetchError::BadSignature { deleted: false })?;

    let mut buffer = vec![0u8; 1 << 20];
    let mut written: u64 = 0;
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                let _ = std::fs::remove_file(&path);
                return Err(FetchError::Network(e.to_string()));
            }
        };
        verifier.update(&buffer[..read]);
        if let Err(e) = std::io::Write::write_all(&mut file, &buffer[..read]) {
            let _ = std::fs::remove_file(&path);
            return Err(FetchError::Disk(format!(
                "could not write the download: {e}"
            )));
        }
        written += read as u64;
        progress(written, asset.size);
    }
    drop(file);

    if verifier.finalize().is_err() {
        // Deleted, not kept and reported. A file that failed this check is the one case where
        // leaving it on disk is actively harmful: it is exactly what somebody would run by
        // hand after reading "the update failed".
        let _ = std::fs::remove_file(&path);
        return Err(FetchError::BadSignature { deleted: true });
    }

    Ok(path)
}

/// Verify a bundle already sitting on disk -- the USB-stick case, and the way to check a
/// download that was interrupted and resumed by hand.
pub fn verify_file(path: &Path, signature_path: &Path) -> Result<(), FetchError> {
    if MINISIGN_PUBLIC_KEY.is_empty() {
        return Err(FetchError::NoPublicKey);
    }
    let public_key = minisign_verify::PublicKey::from_base64(MINISIGN_PUBLIC_KEY)
        .map_err(|e| FetchError::Disk(format!("the compiled-in signing key is unreadable: {e}")))?;
    let signature_text = std::fs::read_to_string(signature_path)
        .map_err(|e| FetchError::Disk(format!("could not read the signature: {e}")))?;
    let signature = minisign_verify::Signature::decode(&signature_text)
        .map_err(|e| FetchError::Disk(format!("the signature is unreadable: {e}")))?;
    let mut verifier = public_key
        .verify_stream(&signature)
        .map_err(|_| FetchError::BadSignature { deleted: false })?;
    let mut file = std::fs::File::open(path)
        .map_err(|e| FetchError::Disk(format!("could not read the file: {e}")))?;
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => verifier.update(&buffer[..n]),
            Err(e) => return Err(FetchError::Disk(format!("could not read the file: {e}"))),
        }
    }
    // Not deleted: this file is the operator's own, and a verify that removes what it was
    // pointed at is a verify nobody runs twice.
    verifier
        .finalize()
        .map_err(|_| FetchError::BadSignature { deleted: false })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str, id: u64) -> RawAsset {
        RawAsset {
            id,
            name: name.to_string(),
            size: 1,
        }
    }

    #[test]
    fn the_signature_is_found_beside_the_bundle_it_signs() {
        let assets = vec![
            asset("aether1-offline-linux-x86_64.tar.gz", 1),
            asset("aether1-offline-linux-x86_64.tar.gz.minisig", 2),
            asset("Aether1-Setup.exe", 3),
            asset("Aether1-Setup.exe.minisig", 4),
        ];
        let picked = pick_asset(&assets).expect("this platform has an asset in that list");
        assert_eq!(picked.signature_id, Some(if cfg!(windows) { 4 } else { 2 }));
    }

    #[test]
    fn a_signature_is_never_mistaken_for_the_bundle() {
        // The `.minisig` starts with the same prefix, so a plain `starts_with` would pick it
        // and then look for `....minisig.minisig`, which does not exist -- turning a properly
        // signed release into "that release has no signature attached".
        let assets = vec![
            asset("aether1-offline-linux-x86_64.tar.gz.minisig", 2),
            asset("aether1-offline-linux-x86_64.tar.gz", 1),
            asset("Aether1-Setup.exe.minisig", 4),
            asset("Aether1-Setup.exe", 3),
        ];
        let picked = pick_asset(&assets).expect("this platform has an asset in that list");
        assert!(!picked.name.ends_with(SIGNATURE_SUFFIX));
        assert!(picked.signature_id.is_some());
    }

    #[test]
    fn an_unsigned_release_is_reported_as_unsigned_rather_than_as_having_no_bundle() {
        let assets = vec![
            asset("aether1-offline-linux-x86_64.tar.gz", 1),
            asset("Aether1-Setup.exe", 3),
        ];
        let picked = pick_asset(&assets).expect("this platform has an asset in that list");
        assert_eq!(picked.signature_id, None);
    }

    #[test]
    fn a_release_without_this_platforms_bundle_picks_nothing() {
        assert!(pick_asset(&[asset("source-code.zip", 9)]).is_none());
    }

    #[test]
    fn the_slim_bundle_is_the_fallback_and_never_the_first_choice() {
        if cfg!(windows) {
            return;
        }
        let both = vec![
            asset("aether1-slim-linux-x86_64.tar.gz", 1),
            asset("aether1-offline-linux-x86_64.tar.gz", 2),
        ];
        assert_eq!(pick_asset(&both).unwrap().id, 2);
        let slim_only = vec![asset("aether1-slim-linux-x86_64.tar.gz", 1)];
        assert_eq!(pick_asset(&slim_only).unwrap().id, 1);
    }

    #[test]
    fn a_tag_is_compared_against_the_running_version_by_its_pull_request_number() {
        assert_eq!(is_newer("Ver 0.4.140", "v0.4.152"), Some(true));
        assert_eq!(is_newer("Ver 0.4.152", "v0.4.152"), Some(false));
        assert_eq!(is_newer("Ver 0.4.160", "v0.4.152"), Some(false));
        // A tag that is not a version says "cannot tell", which is what makes the caller show
        // the tag rather than claim the operator is up to date or behind.
        assert_eq!(is_newer("Ver 0.4.152", "nightly"), None);
        // Without the `v` too, in case a tag is ever cut without one.
        assert_eq!(is_newer("Ver 0.4.140", "0.4.152"), Some(true));
    }

    #[test]
    fn every_failure_says_what_to_do_about_it() {
        for message in [
            ReleaseError::NotSignedIn.message(),
            ReleaseError::NoAccess.message(),
            ReleaseError::NoRelease.message(),
            FetchError::NoPublicKey.message(),
            FetchError::Unsigned.message(),
            FetchError::BadSignature { deleted: true }.message(),
            FetchError::BadSignature { deleted: false }.message(),
        ] {
            assert!(!message.is_empty());
            // Nothing in here tells anyone to install the `gh` CLI any more: that was the old
            // mechanism's requirement, and the sign-in replaced it.
            assert!(!message.contains("gh auth"));
        }
    }

    #[test]
    fn nothing_downloads_without_a_key_to_check_it_with() {
        if MINISIGN_PUBLIC_KEY.is_empty() {
            let asset = Asset {
                name: "whatever.tar.gz".to_string(),
                size: 1,
                id: 1,
                signature_id: Some(2),
            };
            let result = download_and_verify("owner/repo", &asset, "test", |_, _| {});
            assert!(matches!(result, Err(FetchError::NoPublicKey)));
        }
    }
}
