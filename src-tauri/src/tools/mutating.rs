// The tools that change things.
//
// Each one is reachable only through the consent path (see consent.rs): a conversation can
// propose these, never run them. They exist as a separate module from builtin.rs because
// the distinction is the one that matters most in this codebase -- if you are reading a
// tool in this file, the question to ask is "what happens when this is wrong", and every
// one of them either captures what it needs to undo itself or admits up front that it
// can't be undone.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::fs_guard;
use super::{truncate, Outcome, Tool, ToolContext};

/// A file this size is not something a chat message should be rewriting.
const MAX_WRITE_BYTES: usize = 1024 * 1024;
/// Long enough for a build or an update, short enough that a hung command doesn't wedge
/// the companion forever.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_OUTPUT_BYTES: usize = 16 * 1024;

fn string_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing required string argument {key:?}"))
}

// --------------------------------------------------------------- write_file

pub struct WriteFile;

impl Tool for WriteFile {
    fn name(&self) -> &'static str {
        "write_file"
    }

    fn description(&self) -> &'static str {
        "Write text to a file in the operator's home directory, creating it or replacing what is there. Requires approval. The previous contents are kept so the write can be undone."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path under the operator's home directory."},
                "content": {"type": "string", "description": "The complete new contents of the file."}
            },
            "required": ["path", "content"]
        })
    }

    fn mutating(&self) -> bool {
        true
    }

    fn preview(&self, args: &Value) -> String {
        let path = args.get("path").and_then(Value::as_str).unwrap_or("?");
        let bytes = args
            .get("content")
            .and_then(Value::as_str)
            .map(str::len)
            .unwrap_or(0);
        match fs_guard::resolve_writable(path) {
            Ok(resolved) if resolved.exists() => {
                format!("Replace {} with {bytes} bytes", resolved.display())
            }
            Ok(resolved) => format!("Create {} ({bytes} bytes)", resolved.display()),
            Err(_) => format!("Write {bytes} bytes to {path} (which Aether1 will refuse)"),
        }
    }

    fn call(&self, args: &Value, _ctx: &ToolContext) -> Result<Outcome, String> {
        let path = fs_guard::resolve_writable(string_arg(args, "path")?)?;
        let content = string_arg(args, "content")?;
        if content.len() > MAX_WRITE_BYTES {
            return Err(format!(
                "{} bytes is more than write_file will put in one file ({MAX_WRITE_BYTES})",
                content.len()
            ));
        }

        // Capture the undo payload before touching anything. A file that exists but can't
        // be read as text is refused rather than clobbered: there would be no way back.
        let previous = match std::fs::read(&path) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => Some(text),
                Err(_) => {
                    return Err(format!(
                        "{} is not a text file; refusing to overwrite something that cannot be restored",
                        path.display()
                    ))
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("cannot read {} first: {e}", path.display())),
        };

        std::fs::write(&path, content)
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;

        let undo = match &previous {
            Some(text) => json!({"path": path.to_string_lossy(), "previous": text}),
            None => json!({"path": path.to_string_lossy(), "created": true}),
        };
        Ok(Outcome::reversible(
            format!(
                "{} {} ({} bytes)",
                if previous.is_some() {
                    "Replaced"
                } else {
                    "Created"
                },
                path.display(),
                content.len()
            ),
            undo,
        ))
    }

    fn undo(&self, undo: &Value, _ctx: &ToolContext) -> Result<String, String> {
        let path = undo
            .get("path")
            .and_then(Value::as_str)
            .ok_or("the undo record has no path")?;
        let resolved = fs_guard::resolve_writable(path)?;

        if undo.get("created").and_then(Value::as_bool) == Some(true) {
            std::fs::remove_file(&resolved)
                .map_err(|e| format!("cannot remove {}: {e}", resolved.display()))?;
            return Ok(format!("Removed {} again", resolved.display()));
        }

        let previous = undo
            .get("previous")
            .and_then(Value::as_str)
            .ok_or("the undo record has no previous contents")?;
        std::fs::write(&resolved, previous)
            .map_err(|e| format!("cannot restore {}: {e}", resolved.display()))?;
        Ok(format!("Restored {}", resolved.display()))
    }
}

// -------------------------------------------------------- set_aether_setting

/// Settings the companion may change about itself. Everything absent from this list is
/// unreachable.
///
/// The rule the exclusions share: **the companion may not change the thing that decides
/// what the companion may do.** `tools_enabled`, `tool_always_allow`, `command_allowlist`,
/// `llm_api_key` and `local_only` were always outside it. Three more joined them the moment
/// a persona started carrying permissions:
///
/// - `persona_type`, because "change persona to Security" does not read like "grant
///   standing access to network configuration", and under one approval it would be both.
/// - `avatar`, because picking an avatar switches persona in the HUD, which is the same
///   escalation with one extra hop.
/// - `custom_directive`, because a companion that can author its own instructions is one
///   approval away from authoring a more agreeable set. It is not an escalation on its own;
///   it is close enough that the property is worth having outright.
const SETTABLE: &[&str] = &[
    "agent_name",
    "voice_name",
    "auto_speak",
    "enable_sfx",
    "hotkey_toggle",
    "llm_provider",
    "llm_model",
    "llm_endpoint",
    "color_theme",
];

pub struct SetSetting;

impl Tool for SetSetting {
    fn name(&self) -> &'static str {
        "set_aether_setting"
    }

    fn description(&self) -> &'static str {
        "Change one of Aether1's own settings: name, voice, theme, hotkey, or which model it talks to. Requires approval. Cannot touch API keys, its own permissions, its persona, its avatar or its own directive."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "key": {"type": "string", "description": "Setting name."},
                "value": {"description": "New value: string, number or boolean."}
            },
            "required": ["key", "value"]
        })
    }

    fn mutating(&self) -> bool {
        true
    }

    fn preview(&self, args: &Value) -> String {
        format!(
            "Set {} to {}",
            args.get("key").and_then(Value::as_str).unwrap_or("?"),
            args.get("value").unwrap_or(&Value::Null)
        )
    }

    fn call(&self, args: &Value, ctx: &ToolContext) -> Result<Outcome, String> {
        let key = string_arg(args, "key")?;
        if !SETTABLE.contains(&key) {
            return Err(format!(
                "{key:?} is not a setting Aether1 may change about itself; it can set: {}",
                SETTABLE.join(", ")
            ));
        }
        let value = args
            .get("value")
            .ok_or("missing required argument \"value\"")?;

        // With local-only mode on, "switch yourself to OpenAI" is not a settings change,
        // it is a way out of the mode -- so it is refused here rather than left to be
        // caught later by the provider call that would have made the request.
        if crate::local_only::enabled(ctx.db) {
            if let Some(why) = crate::local_only::setting_reaches_the_internet(key, value) {
                return Err(crate::local_only::refusal(&format!(
                    "{key} was not changed ({why})"
                )));
            }
        }

        let previous = ctx.db.get_setting(key).ok().flatten();
        ctx.db
            .set_setting(key, value)
            .map_err(|e| format!("cannot save {key}: {e}"))?;

        Ok(Outcome::reversible(
            format!("{key} is now {value}"),
            json!({"key": key, "previous": previous}),
        ))
    }

    fn undo(&self, undo: &Value, ctx: &ToolContext) -> Result<String, String> {
        let key = undo
            .get("key")
            .and_then(Value::as_str)
            .ok_or("the undo record has no key")?;
        match undo.get("previous") {
            Some(Value::Null) | None => {
                ctx.db
                    .delete_setting(key)
                    .map_err(|e| format!("cannot clear {key}: {e}"))?;
                Ok(format!("{key} is unset again"))
            }
            Some(previous) => {
                ctx.db
                    .set_setting(key, previous)
                    .map_err(|e| format!("cannot restore {key}: {e}"))?;
                Ok(format!("{key} is back to {previous}"))
            }
        }
    }
}

// -------------------------------------------------------------- run_command

/// Setting holding the programs the operator has allowed. Empty by default: an allowlist
/// that starts populated is a decision made on someone's behalf.
pub(super) const ALLOWLIST_SETTING: &str = "command_allowlist";

pub fn command_allowlist(db: &crate::llm::MemoryDb) -> Vec<String> {
    db.get_setting(ALLOWLIST_SETTING)
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
        .unwrap_or_default()
}

pub struct RunCommand;

impl Tool for RunCommand {
    fn name(&self) -> &'static str {
        "run_command"
    }

    fn description(&self) -> &'static str {
        "Run a program the operator has explicitly allowed, with arguments. Requires approval every time. There is no shell: arguments are passed to the program directly, so pipes, redirects and globs do not work. Cannot be undone."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "program": {"type": "string", "description": "Program name, which must be on the operator's allowlist."},
                "args": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Arguments, one per element. Not a shell command line."
                }
            },
            "required": ["program"]
        })
    }

    fn mutating(&self) -> bool {
        true
    }

    /// Never pre-approvable. "Always allow run_command" reads like a decision about one
    /// tool and is really a decision about every program on the allowlist, with any
    /// arguments, from then on -- which is the whole of the permission the allowlist was
    /// there to hand out one call at a time.
    fn always_allowable(&self) -> bool {
        false
    }

    fn preview(&self, args: &Value) -> String {
        let program = args.get("program").and_then(Value::as_str).unwrap_or("?");
        let argv: Vec<String> = args
            .get("args")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|v| v.as_str().unwrap_or_default().to_string())
                    .collect()
            })
            .unwrap_or_default();
        format!("Run: {program} {}", argv.join(" "))
            .trim_end()
            .to_string()
    }

    fn call(&self, args: &Value, ctx: &ToolContext) -> Result<Outcome, String> {
        let program = string_arg(args, "program")?;

        // The name must match the allowlist exactly -- no paths, so a program cannot be
        // smuggled in as ./curl or /tmp/evil/git.
        if program.contains('/') || program.contains('\\') {
            return Err("name the program, not a path to it".to_string());
        }
        let allowed = command_allowlist(ctx.db);
        if !allowed.iter().any(|a| a == program) {
            return Err(if allowed.is_empty() {
                format!(
                    "{program} is not allowed: the operator has not put any programs on the \
                     command allowlist yet"
                )
            } else {
                format!(
                    "{program} is not on the operator's command allowlist ({})",
                    allowed.join(", ")
                )
            });
        }

        let binary = which::which(program).map_err(|e| format!("cannot find {program}: {e}"))?;
        let argv: Vec<String> = args
            .get("args")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|v| v.as_str().unwrap_or_default().to_string())
                    .collect()
            })
            .unwrap_or_default();

        let mut child = Command::new(&binary)
            .args(&argv)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot start {program}: {e}"))?;

        // No wait-with-timeout in std, so poll. A command that outlives the timeout is
        // killed rather than left running unattended.
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if started.elapsed() > COMMAND_TIMEOUT => {
                    let _ = child.kill();
                    return Err(format!(
                        "{program} was still running after {}s and was stopped",
                        COMMAND_TIMEOUT.as_secs()
                    ));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => return Err(format!("cannot wait for {program}: {e}")),
            }
        }

        let output = child
            .wait_with_output()
            .map_err(|e| format!("cannot collect output from {program}: {e}"))?;

        let stdout = truncate(&String::from_utf8_lossy(&output.stdout), MAX_OUTPUT_BYTES);
        let stderr = truncate(&String::from_utf8_lossy(&output.stderr), MAX_OUTPUT_BYTES);
        let code = output
            .status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "killed by signal".to_string());

        let mut report = format!("{program} exited {code}");
        if !stdout.trim().is_empty() {
            report.push_str(&format!("\nstdout:\n{}", stdout.trim_end()));
        }
        if !stderr.trim().is_empty() {
            report.push_str(&format!("\nstderr:\n{}", stderr.trim_end()));
        }

        // Deliberately not reversible: there is no general way to undo an arbitrary
        // program, and pretending otherwise would be worse than saying so.
        Ok(Outcome::text(report))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::MemoryDb;

    fn fixture(name: &str) -> (MemoryDb, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("aether1_mut_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = MemoryDb::open(dir.join("memory.db")).unwrap();
        (db, dir)
    }

    /// These tools write to real paths, and the guard resolves against $HOME.
    fn with_home<T>(home: &std::path::Path, body: impl FnOnce() -> T) -> T {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous_home = std::env::var_os("HOME");
        let previous_profile = std::env::var_os("USERPROFILE");
        std::env::set_var("HOME", home);
        std::env::remove_var("USERPROFILE");
        let out = body();
        match previous_home {
            Some(p) => std::env::set_var("HOME", p),
            None => std::env::remove_var("HOME"),
        }
        match previous_profile {
            Some(p) => std::env::set_var("USERPROFILE", p),
            None => std::env::remove_var("USERPROFILE"),
        }
        out
    }

    #[test]
    fn writing_a_new_file_can_be_undone_by_removing_it() {
        let (db, dir) = fixture("write_new");
        with_home(&dir, || {
            let ctx = ToolContext::new(&db);
            let target = dir.join("note.md");
            let args = json!({"path": target.to_str().unwrap(), "content": "hello"});

            let outcome = WriteFile.call(&args, &ctx).unwrap();
            assert_eq!(std::fs::read_to_string(&target).unwrap(), "hello");

            let undo = outcome.undo.expect("a write records how to undo itself");
            WriteFile.undo(&undo, &ctx).unwrap();
            assert!(!target.exists(), "undoing a creation removes the file");
        });
    }

    #[test]
    fn overwriting_keeps_the_previous_contents_and_restores_them() {
        let (db, dir) = fixture("write_over");
        with_home(&dir, || {
            let ctx = ToolContext::new(&db);
            let target = dir.join("note.md");
            std::fs::write(&target, "original").unwrap();

            let outcome = WriteFile
                .call(
                    &json!({"path": target.to_str().unwrap(), "content": "replacement"}),
                    &ctx,
                )
                .unwrap();
            assert_eq!(std::fs::read_to_string(&target).unwrap(), "replacement");

            WriteFile.undo(&outcome.undo.unwrap(), &ctx).unwrap();
            assert_eq!(std::fs::read_to_string(&target).unwrap(), "original");
        });
    }

    #[test]
    fn a_binary_file_is_not_overwritten_because_it_could_not_be_restored() {
        let (db, dir) = fixture("write_binary");
        with_home(&dir, || {
            let ctx = ToolContext::new(&db);
            let target = dir.join("blob.bin");
            std::fs::write(&target, [0xff, 0xfe, 0x00, 0x01]).unwrap();

            let err = WriteFile
                .call(
                    &json!({"path": target.to_str().unwrap(), "content": "text"}),
                    &ctx,
                )
                .unwrap_err();
            assert!(err.contains("cannot be restored"), "{err}");
            assert_eq!(
                std::fs::read(&target).unwrap(),
                vec![0xff, 0xfe, 0x00, 0x01]
            );
        });
    }

    #[test]
    fn a_setting_change_restores_its_previous_value() {
        let (db, _dir) = fixture("setting");
        let ctx = ToolContext::new(&db);
        db.set_setting("agent_name", &json!("HALCY")).unwrap();

        let outcome = SetSetting
            .call(&json!({"key": "agent_name", "value": "ARIA"}), &ctx)
            .unwrap();
        assert_eq!(db.get_setting_string("agent_name", ""), "ARIA");

        SetSetting.undo(&outcome.undo.unwrap(), &ctx).unwrap();
        assert_eq!(db.get_setting_string("agent_name", ""), "HALCY");
    }

    #[test]
    fn undoing_a_setting_that_did_not_exist_clears_it() {
        let (db, _dir) = fixture("setting_new");
        let ctx = ToolContext::new(&db);

        let outcome = SetSetting
            .call(&json!({"key": "color_theme", "value": "amber"}), &ctx)
            .unwrap();
        SetSetting.undo(&outcome.undo.unwrap(), &ctx).unwrap();
        assert_eq!(db.get_setting_string("color_theme", "unset"), "unset");
    }

    #[test]
    fn the_companion_cannot_widen_its_own_permissions() {
        let (db, _dir) = fixture("permissions");
        let ctx = ToolContext::new(&db);
        for key in [
            "tools_enabled",
            "tool_always_allow",
            "command_allowlist",
            "llm_api_key",
        ] {
            let err = SetSetting
                .call(&json!({"key": key, "value": true}), &ctx)
                .unwrap_err();
            assert!(
                err.contains("not a setting"),
                "{key} must be unreachable: {err}"
            );
        }
    }

    #[test]
    fn a_command_not_on_the_allowlist_does_not_run() {
        let (db, _dir) = fixture("cmd_denied");
        let ctx = ToolContext::new(&db);

        let err = RunCommand
            .call(&json!({"program": "echo", "args": ["hi"]}), &ctx)
            .unwrap_err();
        assert!(err.contains("not put any programs"), "{err}");
    }

    #[test]
    fn a_program_cannot_be_smuggled_in_as_a_path() {
        let (db, _dir) = fixture("cmd_path");
        let ctx = ToolContext::new(&db);
        db.set_setting(ALLOWLIST_SETTING, &json!(["echo"])).unwrap();

        let err = RunCommand
            .call(&json!({"program": "/tmp/echo"}), &ctx)
            .unwrap_err();
        assert!(err.contains("name the program"), "{err}");
    }

    #[test]
    fn an_allowed_command_runs_and_reports_its_output() {
        let (db, _dir) = fixture("cmd_runs");
        let ctx = ToolContext::new(&db);
        db.set_setting(ALLOWLIST_SETTING, &json!(["echo"])).unwrap();

        let outcome = RunCommand
            .call(
                &json!({"program": "echo", "args": ["systems", "nominal"]}),
                &ctx,
            )
            .unwrap();
        assert!(
            outcome.result.contains("systems nominal"),
            "{}",
            outcome.result
        );
        assert!(outcome.result.contains("exited 0"), "{}", outcome.result);
        assert!(
            outcome.undo.is_none(),
            "running a program is not reversible and must not claim to be"
        );
    }

    #[test]
    fn arguments_are_not_a_shell_command_line() {
        let (db, _dir) = fixture("cmd_noshell");
        let ctx = ToolContext::new(&db);
        db.set_setting(ALLOWLIST_SETTING, &json!(["echo"])).unwrap();

        // With a shell this would create a file; without one it is just text to echo.
        let outcome = RunCommand
            .call(
                &json!({"program": "echo", "args": ["hi", ">", "/tmp/aether1_should_not_exist"]}),
                &ctx,
            )
            .unwrap();
        assert!(
            outcome.result.contains('>'),
            "the > was passed through as text"
        );
        assert!(!std::path::Path::new("/tmp/aether1_should_not_exist").exists());
    }
}
