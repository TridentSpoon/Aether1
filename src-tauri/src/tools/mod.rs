// The tool layer: what the companion is allowed to do to the machine, and the record of
// what it did.
//
// Nothing is registered yet -- this step lands the registry, the declaration shape, and the
// action log so that the pieces that depend on them can be built against something real.
// The order matters: the log has to exist before the first tool, and the consent path (a
// later step) before the first *mutating* tool. A tool that ships before the machinery
// that constrains it is a tool that runs unconstrained.
//
// Every tool declares whether it mutates anything. Non-mutating tools run freely;
// mutating ones are proposed and wait for the operator's approval. That flag is the
// entire basis of the safety model, so it is part of the tool's declaration rather than
// something the caller decides per call.

mod builtin;
mod fs_guard;
pub mod protocol;

use std::sync::LazyLock;

use serde::Serialize;
use serde_json::{json, Value};

use crate::llm::{ActionStatus, MemoryDb};

/// One thing the companion can do. Implementors are stateless and shared across threads:
/// a tool holds no per-call state, so the registry can be built once and consulted from
/// wherever a turn happens to run.
pub trait Tool: Send + Sync {
    /// The name the model calls it by. Lowercase snake_case, stable -- it appears in the
    /// prompt, in the action log, and in the operator's approval history.
    fn name(&self) -> &'static str;

    /// What it does, addressed to the model. One or two sentences, concrete about
    /// what the tool will and won't touch.
    fn description(&self) -> &'static str;

    /// JSON Schema for the arguments, as an object schema with `properties` and
    /// `required`. Rendered into the prompt for the text protocol and sent verbatim as a
    /// tool definition to providers that take one.
    fn parameters(&self) -> Value;

    /// True if calling this changes anything outside Aether1's own reading of the world:
    /// a file, a setting, a process, a service. Read-only inspection is false.
    fn mutating(&self) -> bool;

    /// Runs the tool. `args` has been checked against `parameters` for required keys and
    /// their primitive types -- anything beyond that a tool validates itself. The returned
    /// string goes back to the model as the tool's result, so it should be terse and
    /// factual; errors are returned as Err and reach the model as a failure it can react
    /// to, rather than as a crash.
    fn call(&self, args: &Value, ctx: &ToolContext) -> Result<Outcome, String>;
}

/// What a tool can reach besides its own arguments. Tools are stateless and shared, so
/// anything per-machine or per-operator arrives here rather than being held by the tool.
pub struct ToolContext<'a> {
    pub db: &'a MemoryDb,
}

/// What a tool call produced.
#[derive(Debug)]
pub struct Outcome {
    /// The result text handed back to the model.
    pub result: String,
    /// What would be needed to reverse this call, if it can be reversed -- the previous
    /// contents of a file, the setting that was replaced. None for anything one-way or
    /// read-only. Stored with the action so undo doesn't have to reconstruct it later.
    pub undo: Option<Value>,
}

impl Outcome {
    /// A result with nothing to undo: every read-only tool, and any mutation that can't
    /// be taken back.
    pub fn text(result: impl Into<String>) -> Outcome {
        Outcome {
            result: result.into(),
            undo: None,
        }
    }

    #[allow(dead_code)] // used by the first mutating tools, once the consent path exists
    pub fn reversible(result: impl Into<String>, undo: Value) -> Outcome {
        Outcome {
            result: result.into(),
            undo: Some(undo),
        }
    }
}

/// A tool's declaration, in the shape both the prompt renderer and the provider tool-call
/// formats need.
#[derive(Serialize)]
pub struct ToolSchema {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
    pub mutating: bool,
}

/// The set of tools available to a turn.
#[derive(Default)]
pub struct Registry {
    tools: Vec<Box<dyn Tool>>,
}

impl Registry {
    pub fn new() -> Registry {
        Registry { tools: Vec::new() }
    }

    /// Adds a tool. Names are unique: a second tool claiming a name already taken is
    /// rejected rather than silently shadowing the first, since the model addresses tools
    /// by name and would have no way to tell which one it reached.
    pub fn register(&mut self, tool: Box<dyn Tool>) -> Result<(), String> {
        if self.get(tool.name()).is_some() {
            return Err(format!("a tool named {:?} is already registered", tool.name()));
        }
        self.tools.push(tool);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools
            .iter()
            .find(|t| t.name() == name)
            .map(|t| t.as_ref())
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn schemas(&self) -> Vec<ToolSchema> {
        self.tools
            .iter()
            .map(|t| ToolSchema {
                name: t.name(),
                description: t.description(),
                input_schema: t.parameters(),
                mutating: t.mutating(),
            })
            .collect()
    }

    /// The catalog as the model sees it in a system prompt, for providers without a native
    /// tool-call format. Empty when nothing is registered, so the caller can leave the
    /// tool section out of the prompt entirely rather than announcing an empty toolbox.
    pub fn prompt_catalog(&self) -> String {
        if self.is_empty() {
            return String::new();
        }
        self.tools
            .iter()
            .map(|t| {
                format!(
                    "- {}{}: {}\n  arguments: {}",
                    t.name(),
                    if t.mutating() { " (needs approval)" } else { "" },
                    t.description(),
                    t.parameters()
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// The tools the app runs with. Empty until the first ones land -- with no tools
/// registered the engine behaves exactly as it did before this module existed, which is
/// what makes it safe to land the machinery ahead of anything that uses it.
///
/// Built once: tools are stateless and shared, so there is no reason for a turn to
/// assemble its own copy.
pub fn registry() -> &'static Registry {
    static REGISTRY: LazyLock<Registry> = LazyLock::new(|| {
        let mut registry = Registry::new();
        // Read-only, every one of them. Nothing here can change the machine, which is what
        // lets them run without asking the operator first.
        for tool in [
            Box::new(builtin::ReadFile) as Box<dyn Tool>,
            Box::new(builtin::ListDir),
            Box::new(builtin::ListProcesses),
            Box::new(builtin::TelemetryDetail),
            Box::new(builtin::SearchMemory),
        ] {
            registry
                .register(tool)
                .expect("the built-in tool names are distinct");
        }
        registry
    });
    &REGISTRY
}

/// Checks `args` against a tool's schema: every required key present, and each present
/// key of the declared primitive type. Deliberately not a full JSON Schema validator --
/// the schemas here are flat objects of strings and numbers, and a wrong argument is a
/// message back to the model rather than a safety boundary. The safety boundaries live
/// inside the tools (see fs_guard).
fn validate_args(schema: &Value, args: &Value) -> Result<(), String> {
    if !args.is_object() {
        return Err("arguments must be a JSON object".to_string());
    }
    for required in schema
        .get("required")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let Some(key) = required.as_str() else { continue };
        if args.get(key).is_none() {
            return Err(format!("missing required argument {key:?}"));
        }
    }

    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return Ok(());
    };
    for (key, spec) in properties {
        let Some(value) = args.get(key) else { continue };
        let Some(expected) = spec.get("type").and_then(Value::as_str) else {
            continue;
        };
        let ok = match expected {
            "string" => value.is_string(),
            "integer" => value.is_i64() || value.is_u64(),
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            "array" => value.is_array(),
            "object" => value.is_object(),
            _ => true,
        };
        if !ok {
            return Err(format!(
                "argument {key:?} should be a {expected}, got {value}"
            ));
        }
    }
    Ok(())
}

/// Runs one tool call and records it.
///
/// The row goes in before the call, so a tool that hangs or crashes the process still
/// leaves evidence that it was attempted. A mutating tool is refused here rather than
/// executed: the consent path that would approve it doesn't exist yet, and "there are no
/// mutating tools registered" is not something this function should have to trust.
pub fn run(
    registry: &Registry,
    ctx: &ToolContext,
    name: &str,
    args: &Value,
) -> Result<String, String> {
    let Some(tool) = registry.get(name) else {
        return Err(format!(
            "no such tool {name:?}; available: {}",
            registry
                .schemas()
                .iter()
                .map(|s| s.name)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    };

    if tool.mutating() {
        return Err(format!(
            "{name} changes the system and Aether1 cannot run it yet -- approval for \
             mutating actions is not implemented"
        ));
    }

    validate_args(&tool.parameters(), args)?;

    let id = ctx
        .db
        .log_action(name, args, tool.mutating(), ActionStatus::Proposed)
        .ok();

    let outcome = tool.call(args, ctx);
    if let Some(id) = id {
        let _ = match &outcome {
            Ok(o) => ctx.db.set_action_outcome(
                id,
                ActionStatus::Executed,
                Some(&o.result),
                o.undo.as_ref(),
            ),
            Err(e) => ctx
                .db
                .set_action_outcome(id, ActionStatus::Failed, Some(e), None),
        };
    }

    outcome.map(|o| o.result)
}

/// Whether the operator has turned the tool layer on. Off by default: a companion that can
/// act on the machine is a decision to make deliberately, not a default to discover.
pub fn tools_enabled(db: &crate::llm::MemoryDb) -> bool {
    db.get_setting_bool("tools_enabled", false)
}

/// The catalog and its on/off state, for the settings UI and for anything that wants to
/// know what the companion can currently do. Shared by the Tauri command and the HTTP
/// route so both report the same thing.
pub fn catalog(db: &crate::llm::MemoryDb, registry: &Registry) -> Value {
    json!({
        "enabled": tools_enabled(db),
        "tools": registry.schemas(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Probe {
        name: &'static str,
        mutating: bool,
    }

    impl Tool for Probe {
        fn name(&self) -> &'static str {
            self.name
        }
        fn description(&self) -> &'static str {
            "a test tool"
        }
        fn parameters(&self) -> Value {
            json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"],
            })
        }
        fn mutating(&self) -> bool {
            self.mutating
        }
        fn call(&self, args: &Value, _ctx: &ToolContext) -> Result<Outcome, String> {
            Ok(Outcome::text(format!("saw {}", args["path"])))
        }
    }

    fn probe(name: &'static str, mutating: bool) -> Box<dyn Tool> {
        Box::new(Probe { name, mutating })
    }

    fn temp_db(name: &str) -> MemoryDb {
        let path =
            std::env::temp_dir().join(format!("aether1_registry_{name}_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        MemoryDb::open(path).unwrap()
    }

    #[test]
    fn the_shipped_registry_is_read_only() {
        // The invariant that lets these tools run without asking: nothing registered can
        // change the machine until the consent path exists.
        let registry = registry();
        assert!(!registry.is_empty());
        assert!(registry.schemas().iter().all(|s| !s.mutating));
    }

    #[test]
    fn tools_are_found_by_name_and_run() {
        let db = temp_db("found_by_name");
        let ctx = ToolContext { db: &db };
        let mut registry = Registry::new();
        registry.register(probe("probe_tool", false)).unwrap();

        let tool = registry.get("probe_tool").expect("registered tool");
        assert!(!tool.mutating());
        let outcome = tool.call(&json!({"path": "/etc/hostname"}), &ctx).unwrap();
        assert_eq!(outcome.result, "saw \"/etc/hostname\"");
        assert!(outcome.undo.is_none());

        assert!(registry.get("write_file").is_none());
    }

    #[test]
    fn running_a_tool_logs_it_with_its_outcome() {
        let db = temp_db("run_logs");
        let ctx = ToolContext { db: &db };
        let mut registry = Registry::new();
        registry.register(probe("probe_tool", false)).unwrap();

        let result = run(&registry, &ctx, "probe_tool", &json!({"path": "/tmp"})).unwrap();
        assert!(result.contains("/tmp"));

        let logged = db.recent_actions(1).unwrap();
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].tool, "probe_tool");
        assert_eq!(logged[0].status, ActionStatus::Executed);
        assert_eq!(logged[0].result.as_deref(), Some(result.as_str()));
    }

    #[test]
    fn a_mutating_tool_is_refused_until_approval_exists() {
        let db = temp_db("run_mutating");
        let ctx = ToolContext { db: &db };
        let mut registry = Registry::new();
        registry.register(probe("probe_writer", true)).unwrap();

        let err = run(&registry, &ctx, "probe_writer", &json!({"path": "/tmp"})).unwrap_err();
        assert!(err.contains("cannot run it yet"), "{err}");
        assert!(
            db.recent_actions(5).unwrap().is_empty(),
            "a refused call must not be logged as having happened"
        );
    }

    #[test]
    fn an_unknown_tool_reports_what_does_exist() {
        let db = temp_db("run_unknown");
        let ctx = ToolContext { db: &db };
        let mut registry = Registry::new();
        registry.register(probe("probe_tool", false)).unwrap();

        let err = run(&registry, &ctx, "nonexistent", &json!({})).unwrap_err();
        assert!(err.contains("no such tool"), "{err}");
        assert!(err.contains("probe_tool"), "{err}");
    }

    #[test]
    fn bad_arguments_are_rejected_before_the_tool_runs() {
        let db = temp_db("run_badargs");
        let ctx = ToolContext { db: &db };
        let mut registry = Registry::new();
        registry.register(probe("probe_tool", false)).unwrap();

        let missing = run(&registry, &ctx, "probe_tool", &json!({})).unwrap_err();
        assert!(missing.contains("missing required argument"), "{missing}");

        let wrong_type = run(&registry, &ctx, "probe_tool", &json!({"path": 7})).unwrap_err();
        assert!(wrong_type.contains("should be a string"), "{wrong_type}");

        assert!(
            db.recent_actions(5).unwrap().is_empty(),
            "a call rejected on its arguments never ran, so it is not in the log"
        );
    }

    #[test]
    fn a_duplicate_name_is_refused_not_shadowed() {
        let mut registry = Registry::new();
        registry.register(probe("probe_tool", false)).unwrap();
        let err = registry.register(probe("probe_tool", true)).unwrap_err();
        assert!(err.contains("already registered"), "{err}");
        assert_eq!(registry.schemas().len(), 1);
        assert!(!registry.get("probe_tool").unwrap().mutating());
    }

    #[test]
    fn schemas_carry_the_mutating_flag() {
        let mut registry = Registry::new();
        registry.register(probe("read_file", false)).unwrap();
        registry.register(probe("write_file", true)).unwrap();

        let schemas = serde_json::to_value(registry.schemas()).unwrap();
        assert_eq!(schemas[0]["name"], "read_file");
        assert_eq!(schemas[0]["mutating"], json!(false));
        assert_eq!(schemas[1]["mutating"], json!(true));
        assert_eq!(schemas[0]["input_schema"]["required"], json!(["path"]));
    }

    #[test]
    fn the_prompt_catalog_is_empty_when_nothing_is_registered() {
        assert_eq!(Registry::new().prompt_catalog(), "");

        let mut registry = Registry::new();
        registry.register(probe("write_file", true)).unwrap();
        let catalog = registry.prompt_catalog();
        assert!(catalog.contains("write_file"));
        assert!(
            catalog.contains("needs approval"),
            "a mutating tool must be marked as such in the prompt: {catalog}"
        );
    }
}
