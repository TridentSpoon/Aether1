// The two tools that answer "are you up to date?" and act on the answer.
//
// This exists because of a desktop log of 2026-11-03. The HUD had already said, in the
// chat, that build 63b095c was available and that Settings -> Updates could fetch it. The
// operator then asked the companion three times, in plain words -- "Are you up to date?",
// "Can you pull your latest update please", "check if you are on the latest build and if
// not run your update" -- and got, in order: a tool call written out as text and never run
// (see `protocol::salvage_calls`), a disclaimer about the model's training cutoff with an
// invitation to go and read `check_for_update` in two unrelated projects of the operator's
// own, and an offer to proceed once its intentions were confirmed.
//
// None of that is the model being stupid. There was no tool. Every tool it had was about
// the machine -- the load average, the process table, the file tree -- and nothing at all
// about the program it is part of, so "are *you* up to date" was a question with no
// instrument behind it and the model did what a model does with one of those. #286 had
// already built both halves of the answer (`aether1 update`, Settings -> Updates); they
// were simply not reachable from a conversation.
//
// So: `update_status` reads the same comparison the HUD panel and the tray draw, and
// `install_update` starts the same install the tray's second click does. The split is the
// consent boundary: knowing is free, and replacing the running program is proposed to the
// operator and waits.

use serde_json::{json, Value};

use super::{Outcome, Tool, ToolContext};

/// Starts an update, app-side. Returns the sentence to hand back to the model.
///
/// The install is the one thing in this file that cannot be done from here.
/// `perform_update_core` needs a `tauri::AppHandle`: it emits the five `update-progress`
/// steps the HUD strip and the Updates panel draw, and on success it relaunches the
/// freshly built binary and exits this process. A tool call has none of that, and giving
/// the tool layer its own copy of a git fetch, a `cargo build --release` and a relaunch
/// would be a second update mechanism to keep in step with the first.
///
/// So the running desktop app installs one of these at startup and the tool calls it. In
/// `--serve`, in a CLI subcommand and in a test nothing is installed, and the tool says so
/// instead of pretending -- the same line `doctor::RepairKind::InApp` draws for a repair
/// only the app can make.
type Installer = Box<dyn Fn() -> Result<String, String> + Send + Sync>;

static INSTALLER: std::sync::OnceLock<Installer> = std::sync::OnceLock::new();

/// Called once, from the desktop app's `setup`. A second call is ignored rather than
/// panicking: there is exactly one app per process and nothing here is worth taking the
/// process down for.
pub fn set_installer(installer: Installer) {
    let _ = INSTALLER.set(installer);
}

/// Everything either tool needs to say, read fresh.
///
/// Deliberately the same `compute_update_status` the HUD's own check and the tray's call,
/// rather than a reading of its own: an operator who is told two different things about
/// one version by one program has no way to decide which to believe.
fn status(ctx: &ToolContext) -> crate::UpdateStatus {
    let local_only = crate::local_only::env_forced() || crate::local_only::enabled(ctx.db);
    let declined = ctx
        .db
        .get_setting_bool(crate::github_auth::DECLINED_SETTING, false);
    crate::compute_update_status(local_only, declined)
}

/// The status as a few lines for the model, written so that the only sentence it can
/// reasonably produce from them is a true one.
///
/// The version and the build go first whatever happened, because they are the half of the
/// question that never needs the network: "which build am I" is answerable on a machine
/// with no internet at all, and a check that failed should not take the knowable part down
/// with it.
fn describe(status: &crate::UpdateStatus) -> String {
    let mut out = format!(
        "This is {} ({}), built from commit {}.\n",
        "Aurora1", status.version, status.built_commit_short,
    );
    out.push_str(match status.mode {
        crate::releases::Mode::Checkout => {
            "It is a git checkout, so its update is a fetch, a fast-forward and a rebuild \
             of this same tree.\n"
        }
        crate::releases::Mode::Release => {
            "It was installed from a release bundle, so its update is a newer signed bundle \
             downloaded from GitHub.\n"
        }
    });

    if let Some(why) = &status.error {
        out.push_str(&format!(
            "\nThe check for a newer version did not conclude: {why}\n\
             Say that, and say which build is running. Do not guess whether it is current.\n"
        ));
        if status.needs_sign_in {
            out.push_str(
                "The operator can fix this one: signing in to GitHub under Settings -> The \
                 Brain -> Connections is what lets this copy see releases.\n",
            );
        }
        return out;
    }

    if !status.checked {
        out.push_str(
            "\nThe comparison did not run, so whether this is the latest build is unknown. \
             Say so rather than answering either way.\n",
        );
        return out;
    }

    if status.up_to_date {
        out.push_str("\nThis is the latest version. There is nothing to install.\n");
    } else {
        match status.mode {
            crate::releases::Mode::Checkout => out.push_str(&format!(
                "\nA newer build is available: commit {}. This copy is on {}.\n",
                status
                    .latest_commit
                    .as_deref()
                    .map(crate::short_hash)
                    .unwrap_or("unknown"),
                status.built_commit_short,
            )),
            crate::releases::Mode::Release => out.push_str(&format!(
                "\nA newer release is available: {}. This copy is {}.\n",
                status.latest_tag.as_deref().unwrap_or("unknown"),
                status.version,
            )),
        }
        if !status.changes.is_empty() {
            out.push_str("What is in it:\n");
            for change in &status.changes {
                out.push_str(&format!("  - {change}\n"));
            }
        }
        /* The instruction matters as much as the fact. The log this file is named after
        has the operator asking twice more after being told a build was available, because
        nothing in front of the model said what to do about it -- so it offered to explain
        how one might go about finding out instead. */
        out.push_str(
            "To install it, call `install_update`. That is proposed to the operator and \
             runs only if they approve it, so say you are asking and then ask -- do not \
             tell them to go to Settings themselves, and do not ask them in prose whether \
             they are sure first.\n",
        );
    }
    out
}

// --------------------------------------------------------------- update_status

pub struct UpdateStatus;

impl Tool for UpdateStatus {
    fn name(&self) -> &'static str {
        "update_status"
    }

    fn description(&self) -> &'static str {
        "Whether this copy of Aurora1 is the latest build, read from GitHub: the version and commit running now, whether a newer one exists, and what is in it. Call this for any question about AETHER1's or Aurora1's own version, build, update or whether it is current -- it is the program asking about itself, so your training date has nothing to do with the answer."
    }

    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {}, "required": [] })
    }

    fn mutating(&self) -> bool {
        // A version string and one GitHub read. It changes nothing.
        false
    }

    fn call(&self, _args: &Value, ctx: &ToolContext) -> Result<Outcome, String> {
        Ok(Outcome::text(describe(&status(ctx))))
    }
}

// -------------------------------------------------------------- install_update

pub struct InstallUpdate;

impl Tool for InstallUpdate {
    fn name(&self) -> &'static str {
        "install_update"
    }

    fn description(&self) -> &'static str {
        "Install the newer build of Aurora1 that `update_status` found. On a git checkout this fetches, fast-forwards, rebuilds and restarts the app; on an installed copy it downloads the signed bundle for the operator to run. Call it only after `update_status` has said a newer build exists and the operator has asked for it to be installed. The operator approves it before anything happens."
    }

    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {}, "required": [] })
    }

    fn mutating(&self) -> bool {
        // It replaces the running program and restarts it. Every call is proposed.
        true
    }

    fn always_allowable(&self) -> bool {
        /* "Never ask me about install_update again" would hand a conversation standing
        permission to rebuild and relaunch the app, which is the one action in the registry
        whose side effect is that the conversation itself stops. The operator says yes to
        each one, the way they click the tray item each time. */
        false
    }

    fn preview(&self, _args: &Value) -> String {
        match crate::releases::mode() {
            crate::releases::Mode::Checkout => format!(
                "Update {}: fetch, fast-forward and rebuild this checkout, then restart \
                 the app",
                "Aurora1"
            ),
            crate::releases::Mode::Release => format!(
                "Download the newer signed {} bundle (nothing is installed for you)",
                "Aurora1"
            ),
        }
    }

    fn call(&self, _args: &Value, ctx: &ToolContext) -> Result<Outcome, String> {
        /* A fresh check, not the one the model read, for the reason `self_repair` takes
        one: an approval sits in front of the operator for as long as it takes them to look
        at it, and "install the update" decided against a comparison made ten minutes ago
        could rebuild over a build that has since become current. */
        let status = status(ctx);
        if let Some(why) = &status.error {
            return Err(format!(
                "the check for a newer version did not conclude, so there is nothing to \
                 install from: {why}"
            ));
        }
        if status.checked && status.up_to_date {
            return Ok(Outcome::text(format!(
                "Already on the latest version ({}, commit {}). Nothing was installed.",
                status.version, status.built_commit_short
            )));
        }
        if !status.checked {
            return Err(
                "whether a newer version exists is not known, so nothing was installed -- \
                 call update_status and tell the operator what it says"
                    .to_string(),
            );
        }

        let Some(installer) = INSTALLER.get() else {
            return Err(format!(
                "only the running {} desktop app can install its own update, and this call \
                 did not come from it. Tell the operator to use Settings -> Updates, the \
                 tray's \"Check for Updates\", or `aether1 update` in a terminal.",
                "Aurora1"
            ));
        };
        installer().map(Outcome::text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> crate::UpdateStatus {
        crate::UpdateStatus {
            checked: true,
            up_to_date: false,
            version: "Aurora1 rev0.261103.294".to_string(),
            built_commit: "b8af0890000000000000000000000000000000ff".to_string(),
            built_commit_short: "b8af089".to_string(),
            latest_commit: Some("63b095c0000000000000000000000000000000ff".to_string()),
            error: None,
            mode: crate::releases::Mode::Checkout,
            latest_tag: None,
            release_url: None,
            asset_name: None,
            asset_size: None,
            asset_signed: false,
            signed_in: true,
            sign_in_available: true,
            needs_sign_in: false,
            declined: false,
            changes: vec!["Heal the window when its renderer dies".to_string()],
        }
    }

    /// The question from the log: a build is available, and the answer has to name both
    /// halves of the comparison and what to do about it.
    #[test]
    fn a_newer_build_is_reported_with_the_call_that_installs_it() {
        let said = describe(&base());
        assert!(said.contains("rev0.261103.294"), "{said}");
        assert!(said.contains("b8af089"), "{said}");
        assert!(said.contains("63b095c"), "{said}");
        assert!(said.contains("install_update"), "{said}");
    }

    #[test]
    fn being_current_does_not_mention_installing_anything() {
        let said = describe(&crate::UpdateStatus {
            up_to_date: true,
            ..base()
        });
        assert!(said.contains("latest version"), "{said}");
        assert!(!said.contains("install_update"), "{said}");
    }

    /// A check that could not run must not be read either way, and the build it is running
    /// is still knowable -- that half needs no network.
    #[test]
    fn a_failed_check_says_so_and_still_names_the_build() {
        let said = describe(&crate::UpdateStatus {
            checked: false,
            error: Some("GitHub was not contacted".to_string()),
            ..base()
        });
        assert!(said.contains("did not conclude"), "{said}");
        assert!(said.contains("b8af089"), "{said}");
        assert!(!said.contains("install_update"), "{said}");
    }

    /// The tool is the one the model should reach for when it is asked about itself, so
    /// its own description has to say the words an operator uses.
    #[test]
    fn the_description_names_the_question_it_answers() {
        let said = UpdateStatus.description().to_lowercase();
        for word in ["version", "build", "update", "latest"] {
            assert!(said.contains(word), "{word} missing from {said}");
        }
        assert!(!UpdateStatus.mutating());
        assert!(InstallUpdate.mutating());
        assert!(!InstallUpdate.always_allowable());
    }
}
