//! Signing in to GitHub as yourself, so an installed copy can see the releases.
//!
//! The updater this replaces the credential half of (see `main.rs`) shelled out to
//! `gh auth token`. That works on the machine this project is written on and nowhere else:
//! someone who installed a bundle has no checkout, no `gh`, and no way to read a private
//! repository's releases at all. Shipping a token in the binary is not the fix -- anything
//! distributed can be read back out of, and GitHub revokes tokens that turn up in
//! artifacts.
//!
//! So the person signs in, and the app carries no secret. GitHub's **device flow** is the
//! shape for an app that cannot keep one: it asks for a short code, the person types it in
//! at `github.com/login/device` in a browser they already trust, and the token that comes
//! back belongs to *them*. The only thing compiled in is a client ID, which is public by
//! design and grants nothing on its own.
//!
//! **A GitHub App, not a classic OAuth App.** A classic OAuth App can only ask for `repo`,
//! which is read *and write* to every repository that person can reach -- an absurd thing
//! to want for a version check. A GitHub App scopes to `contents: read` on the repositories
//! it is installed on, so the worst a leaked token here can do is read what its holder
//! could already read. Device flow is off by default in an App's settings and has to be
//! turned on.
//!
//! Access becomes a real check rather than a guess, which was the point: reading a private
//! repository's releases requires being a collaborator on it, so adding someone grants
//! access and removing them takes it away. That is what "subscribed to the project" means
//! in practice.
//!
//! **Declining is not an error.** Someone who never signs in still has a working companion;
//! they are told once that they will not hear about new versions, the answer is remembered,
//! and nothing asks again.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::Deserialize;

/// The client ID of the GitHub App, baked in at build time from `AETHER1_GITHUB_CLIENT_ID`
/// (see `build.rs`). Public by design: it names the App to GitHub and authorises nothing,
/// which is the whole reason the device flow can be used by a distributed binary at all.
///
/// Empty in a build made without it -- a fork, or this repository before the App exists.
/// Every entry point checks [`configured`] first and says "this build has no sign-in" in
/// plain words, rather than sending a request that comes back as an opaque 401.
pub const CLIENT_ID: &str = match option_env!("AETHER1_GITHUB_CLIENT_ID") {
    Some(id) => id,
    None => "",
};

/// Where the token is kept, under the operating system's own credential store: Credential
/// Manager on Windows, Keychain on macOS, the Secret Service (libsecret, kwallet) on Linux.
///
/// `llm_api_key` sits in the settings table as plain JSON, so there is precedent for
/// keeping it there instead -- and it is the wrong precedent for this one. An LLM key is a
/// key to a service the operator chose to pay for; a GitHub token is an identity on their
/// own account. `contents: read` bounds the damage, but the keychain is where it belongs.
const KEYCHAIN_SERVICE: &str = "aether1-github-updates";
const KEYCHAIN_ACCOUNT: &str = "update-token";

/// The settings key remembering that the sign-in was offered and turned down. A decision the
/// next launch has forgotten is a prompt that never goes away -- the same reasoning as
/// `installs::NOTICE_SETTING`, and it is why this is stored rather than inferred from "no
/// token".
pub const DECLINED_SETTING: &str = "github_signin_declined";

/// How long any one request to GitHub is allowed to take. The polling loop below runs for
/// minutes, but no single call should hang the thread it is on for longer than this.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

pub fn configured() -> bool {
    !CLIENT_ID.is_empty()
}

/// What the person has to do, handed back to whoever is showing it: the HUD draws it, the
/// CLI prints it. `verification_uri` is GitHub's own page rather than anything of ours, so
/// the code is typed into github.com and nowhere else.
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    /// Seconds until this code stops working. GitHub currently says 900.
    pub expires_in: u64,
    /// Seconds GitHub asks to be left between polls. Polling faster earns a `slow_down`,
    /// which is handled below rather than raced.
    pub interval: u64,
}

/// Why a sign-in did not finish. Separate variants rather than one string because each one
/// wants different words in front of the operator, and matching on rendered prose is how
/// that silently breaks the next time the wording changes.
#[derive(Debug)]
pub enum SignInError {
    /// No client ID compiled in: this build cannot sign in at all.
    NotConfigured,
    /// The person never approved the code before it expired.
    Expired,
    /// The person pressed Cancel on GitHub's page.
    Denied,
    /// Device flow is switched off in the App's settings (GitHub's `device_flow_disabled`).
    /// Worth naming: it is off by default, so this is the first thing a new App gets wrong.
    FlowDisabled,
    Network(String),
    Other(String),
}

impl SignInError {
    pub fn message(&self) -> String {
        match self {
            SignInError::NotConfigured => {
                "this build has no GitHub sign-in compiled into it, so it cannot check for \
                 releases -- a build from the project's own pipeline can"
                    .to_string()
            }
            SignInError::Expired => {
                "the code expired before it was approved -- start the sign-in again".to_string()
            }
            SignInError::Denied => "the sign-in was turned down at github.com".to_string(),
            SignInError::FlowDisabled => {
                "GitHub says device flow is disabled for this app -- it has to be turned on in \
                 the GitHub App's settings"
                    .to_string()
            }
            SignInError::Network(e) => format!("could not reach GitHub: {e}"),
            SignInError::Other(e) => e.clone(),
        }
    }
}

#[derive(Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
}

/// Step one: ask GitHub for a code to show the person. Blocking; call it off the main
/// thread like every other request in this file.
///
/// No scopes are sent. A GitHub App's permissions are fixed when the App is configured and
/// when it is installed on a repository -- asking for them per-request is the classic OAuth
/// App's model, and the reason that model has to ask for `repo`.
pub fn start(agent_label: &str) -> Result<DeviceCode, SignInError> {
    if !configured() {
        return Err(SignInError::NotConfigured);
    }
    let response = ureq::post("https://github.com/login/device/code")
        .config()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .build()
        .header("Accept", "application/json")
        .header("User-Agent", agent_label)
        .send_form([("client_id", CLIENT_ID)])
        .map_err(|e| SignInError::Network(e.to_string()))?;

    let body: DeviceCodeResponse = response
        .into_body()
        .read_json()
        .map_err(|e| SignInError::Other(format!("GitHub sent back something unreadable: {e}")))?;

    Ok(DeviceCode {
        device_code: body.device_code,
        user_code: body.user_code,
        verification_uri: body.verification_uri,
        expires_in: body.expires_in,
        // GitHub has sent a zero here before. One second of politeness is the floor, not a
        // reason to hammer the endpoint.
        interval: body.interval.max(1),
    })
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// One poll of the token endpoint. `Ok(None)` means "not yet, keep waiting" -- the normal
/// answer for as long as the browser tab is open and nobody has pressed the button.
///
/// Split out from [`wait_for_token`] so a caller that owns its own clock -- the HUD, which
/// wants to keep drawing while this runs -- can drive the loop itself instead of handing a
/// thread over for fifteen minutes.
pub fn poll_once(device_code: &str, agent_label: &str) -> Result<Option<String>, SignInError> {
    if !configured() {
        return Err(SignInError::NotConfigured);
    }
    let response = ureq::post("https://github.com/login/oauth/access_token")
        .config()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .build()
        .header("Accept", "application/json")
        .header("User-Agent", agent_label)
        .send_form([
            ("client_id", CLIENT_ID),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])
        .map_err(|e| SignInError::Network(e.to_string()))?;

    let body: TokenResponse = response
        .into_body()
        .read_json()
        .map_err(|e| SignInError::Other(format!("GitHub sent back something unreadable: {e}")))?;

    if let Some(token) = body.access_token {
        return Ok(Some(token));
    }
    // Everything below is a 200 with an `error` field -- the device flow reports its
    // in-progress states as successful responses, so a status-code check alone sees nothing.
    match body.error.as_deref() {
        Some("authorization_pending") => Ok(None),
        // Told to back off: also "not yet", and the caller widens its interval on seeing it.
        Some("slow_down") => Ok(None),
        Some("expired_token") => Err(SignInError::Expired),
        Some("access_denied") => Err(SignInError::Denied),
        Some("device_flow_disabled") => Err(SignInError::FlowDisabled),
        Some(other) => Err(SignInError::Other(
            body.error_description
                .unwrap_or_else(|| format!("GitHub refused the sign-in: {other}")),
        )),
        None => Err(SignInError::Other(
            "GitHub sent neither a token nor an error".to_string(),
        )),
    }
}

/// Step two, for a caller happy to give up a thread: poll until the person approves, the
/// code expires, or they say no. Blocking for up to `code.expires_in` seconds.
pub fn wait_for_token(code: &DeviceCode, agent_label: &str) -> Result<String, SignInError> {
    let deadline = Instant::now() + Duration::from_secs(code.expires_in);
    let mut interval = Duration::from_secs(code.interval);
    loop {
        if Instant::now() >= deadline {
            return Err(SignInError::Expired);
        }
        std::thread::sleep(interval);
        match poll_once(&code.device_code, agent_label) {
            Ok(Some(token)) => return Ok(token),
            Ok(None) => {
                // Cheaper than tracking which "not yet" it was: creeping the interval up on
                // every miss keeps a long wait from being a tight loop, and GitHub's
                // `slow_down` asks for exactly this five-second step.
                interval = (interval + Duration::from_secs(1)).min(Duration::from_secs(15));
            }
            Err(e) => return Err(e),
        }
    }
}

// ---------------------------------------------------------------------------------------
// Where the token lives.
// ---------------------------------------------------------------------------------------

/// The file the token falls back to when there is no credential store to put it in. A
/// headless box -- the homelab case -- often has no Secret Service running at all, and
/// refusing to sign in there would make the update check a desktop-only feature for no
/// reason. Mode 0600, and [`storage_kind`] says out loud which of the two is in use rather
/// than letting the weaker one pass for the stronger.
fn fallback_path() -> PathBuf {
    crate::project_root().join("backend").join("github_token")
}

/// Which store the token is actually in, for showing back to the operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Storage {
    Keychain,
    File,
    None,
}

fn entry() -> Option<keyring::Entry> {
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT).ok()
}

pub fn store_token(token: &str) -> Result<Storage, String> {
    if let Some(entry) = entry() {
        if entry.set_password(token).is_ok() {
            // A token that was in the file before now would otherwise outlive the keychain
            // copy and keep being readable by anything that can read the home directory.
            let _ = std::fs::remove_file(fallback_path());
            return Ok(Storage::Keychain);
        }
    }
    let path = fallback_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not make somewhere to keep the token: {e}"))?;
    }
    std::fs::write(&path, token).map_err(|e| format!("could not write the token: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(Storage::File)
}

pub fn load_token() -> Option<String> {
    if let Some(entry) = entry() {
        if let Ok(token) = entry.get_password() {
            if !token.trim().is_empty() {
                return Some(token);
            }
        }
    }
    let token = std::fs::read_to_string(fallback_path()).ok()?;
    let token = token.trim().to_string();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

pub fn storage_kind() -> Storage {
    if let Some(entry) = entry() {
        if entry.get_password().is_ok() {
            return Storage::Keychain;
        }
    }
    if fallback_path().exists() {
        Storage::File
    } else {
        Storage::None
    }
}

/// Forget the token. Both places, always: signing out has to mean the token is gone, not
/// gone from whichever store was asked about first.
pub fn clear_token() -> bool {
    let mut removed = false;
    if let Some(entry) = entry() {
        if entry.delete_credential().is_ok() {
            removed = true;
        }
    }
    if fallback_path().exists() && std::fs::remove_file(fallback_path()).is_ok() {
        removed = true;
    }
    removed
}

pub fn signed_in() -> bool {
    load_token().is_some()
}

/// The token the update check should use: the signed-in one, or -- only on a machine that
/// has it -- whatever `gh` is already logged in as.
///
/// The `gh` path is kept because it is what the developer machine has and it costs nothing
/// to try, but it is no longer the mechanism: it is a fallback behind the sign-in, not the
/// other way round. Nothing tells anyone to install `gh` any more.
pub fn token_for_requests() -> Option<String> {
    load_token().or_else(gh_cli_token)
}

fn gh_cli_token() -> Option<String> {
    let mut cmd = std::process::Command::new("gh");
    cmd.args(["auth", "token"]);
    crate::paths::suppress_console_window(&mut cmd);
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let token = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_build_without_a_client_id_refuses_before_it_asks() {
        // The point of the check is that it happens *before* the request: a build with no
        // App behind it should say so, not send GitHub an empty client_id and relay the 401.
        if !configured() {
            assert!(matches!(start("test"), Err(SignInError::NotConfigured)));
            assert!(matches!(
                poll_once("nope", "test"),
                Err(SignInError::NotConfigured)
            ));
        }
    }

    #[test]
    fn every_sign_in_failure_says_something_a_person_can_act_on() {
        for error in [
            SignInError::NotConfigured,
            SignInError::Expired,
            SignInError::Denied,
            SignInError::FlowDisabled,
            SignInError::Network("connection refused".to_string()),
            SignInError::Other("something else".to_string()),
        ] {
            let message = error.message();
            assert!(!message.is_empty());
            // No bare error codes in front of the operator: GitHub's `device_flow_disabled`
            // and friends are translated, not passed through.
            assert!(!message.contains("device_flow_disabled"));
        }
    }
}
