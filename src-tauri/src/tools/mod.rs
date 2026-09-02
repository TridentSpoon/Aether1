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
pub mod consent;
mod fs_guard;
mod mutating;
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

    /// One line describing what calling this with `args` would do, addressed to the
    /// operator rather than to the model. This is what an approval card says, so it has to
    /// be concrete: "delete 4 files in ~/build" tells them something, "run write_file"
    /// does not. The default is the tool's name and its arguments, which is honest but
    /// rarely the clearest thing a tool could say about itself.
    fn preview(&self, args: &Value) -> String {
        format!("{} {}", self.name(), args)
    }

    /// Reverses a call, given the `undo` payload its Outcome carried. The default is to
    /// refuse: most things cannot be taken back, and a tool that silently pretends to undo
    /// itself is worse than one that admits it can't.
    fn undo(&self, _undo: &Value, _ctx: &ToolContext) -> Result<String, String> {
        Err(format!("{} cannot be undone", self.name()))
    }
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
            return Err(format!(
                "a tool named {:?} is already registered",
                tool.name()
            ));
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
                    if t.mutating() {
                        " (needs approval)"
                    } else {
                        ""
                    },
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
        // The read-only ones first: nothing here can change the machine, which is what
        // lets them run without asking the operator first.
        for tool in [
            Box::new(builtin::ReadFile) as Box<dyn Tool>,
            Box::new(builtin::ListDir),
            Box::new(builtin::ListProcesses),
            Box::new(builtin::TelemetryDetail),
            Box::new(builtin::SearchMemory),
            // Everything below changes something, so everything below goes through the
            // consent path -- proposed to the operator, never run from a conversation.
            Box::new(mutating::WriteFile),
            Box::new(mutating::SetSetting),
            Box::new(mutating::RunCommand),
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
        let Some(key) = required.as_str() else {
            continue;
        };
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

/// Runs one tool call, or proposes it if it would change the machine.
///
/// This is the single gate: a mutating tool cannot be executed from a conversation, only
/// proposed, and the check lives here rather than in each tool so that adding a tool
/// carelessly cannot bypass it.
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

    validate_args(&tool.parameters(), args)?;

    // A mutating call does not run here. It is written down and left for the operator --
    // unless they have already decided, once, that this tool never needs asking about.
    if tool.mutating() {
        if !consent::is_always_allowed(ctx.db, name) {
            let id = consent::propose(ctx.db, name, args)?;
            return Ok(format!(
                "PROPOSED (id {id}): {} is waiting for the operator to approve it. Do not \
                 assume it happened, and do not propose it again -- tell them what you want \
                 to do and why, then wait.",
                tool.preview(args)
            ));
        }
        return run_now(ctx, tool, name, args, "always-allow");
    }

    run_now(ctx, tool, name, args, "automatic")
}

/// Executes a tool and records the outcome. The log row goes in before the call, so a
/// tool that hangs or crashes the process still leaves evidence it was attempted.
fn run_now(
    ctx: &ToolContext,
    tool: &dyn Tool,
    name: &str,
    args: &Value,
    approved_by: &str,
) -> Result<String, String> {
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
                Some(approved_by),
            ),
            Err(e) => ctx.db.set_action_outcome(
                id,
                ActionStatus::Failed,
                Some(e),
                None,
                Some(approved_by),
            ),
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
        "always_allowed": consent::always_allowed(db),
        "proposal_ttl_minutes": consent::PROPOSAL_TTL_MINUTES,
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
    fn the_shipped_registry_separates_looking_from_changing() {
        let mut changing: Vec<&str> = registry()
            .schemas()
            .iter()
            .filter(|s| s.mutating)
            .map(|s| s.name)
            .collect();
        changing.sort();
        assert_eq!(
            changing,
            vec!["run_command", "set_aether_setting", "write_file"],
            "adding a tool that changes the machine is a deliberate act; update this test \
             along with the reasoning for it"
        );
    }

    #[test]
    fn nothing_is_pre_approved_out_of_the_box() {
        let db = temp_db("nothing_preapproved");
        for schema in registry().schemas() {
            assert!(
                !consent::is_always_allowed(&db, schema.name),
                "{} must start off asking",
                schema.name
            );
        }
        assert!(
            crate::tools::mutating::command_allowlist(&db).is_empty(),
            "the command allowlist starts empty: an allowlist that ships populated is a \
             decision made on someone else's behalf"
        );
    }

    #[test]
    fn every_mutating_tool_describes_itself_for_an_approval_card() {
        // The default preview is the tool name and raw JSON, which is not something an
        // operator can make a decision from. Anything that changes the machine has to do
        // better than the default.
        for tool_name in registry().schemas().iter().filter(|s| s.mutating).map(|s| s.name) {
            let tool = registry().get(tool_name).unwrap();
            let args = json!({"path": "/tmp/x", "content": "hi", "key": "agent_name", "value": "A", "program": "echo"});
            let default = format!("{tool_name} {args}");
            assert_ne!(
                tool.preview(&args),
                default,
                "{tool_name} must override preview() with something an operator can judge"
            );
        }
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
    fn a_mutating_tool_is_proposed_rather_than_run() {
        let db = temp_db("run_mutating");
        let ctx = ToolContext { db: &db };
        let mut registry = Registry::new();
        registry.register(probe("probe_writer", true)).unwrap();

        let reply = run(&registry, &ctx, "probe_writer", &json!({"path": "/tmp"})).unwrap();
        assert!(reply.starts_with("PROPOSED"), "{reply}");

        let waiting = consent::pending(&db, &registry);
        assert_eq!(waiting.len(), 1, "it should be waiting for the operator");
        assert_eq!(waiting[0].status, ActionStatus::Proposed);
        assert!(
            waiting[0].result.is_none(),
            "a proposal has not run, so it has no result"
        );
    }

    #[test]
    fn an_always_allowed_tool_runs_without_asking_and_says_who_let_it() {
        let db = temp_db("run_always_allowed");
        let ctx = ToolContext { db: &db };
        let mut registry = Registry::new();
        registry.register(probe("probe_writer", true)).unwrap();
        consent::set_always_allowed(&db, "probe_writer", true).unwrap();

        let reply = run(&registry, &ctx, "probe_writer", &json!({"path": "/tmp"})).unwrap();
        assert!(!reply.starts_with("PROPOSED"), "{reply}");

        assert!(consent::pending(&db, &registry).is_empty());
        let logged = db.recent_actions(1).unwrap();
        assert_eq!(logged[0].status, ActionStatus::Executed);
        assert_eq!(logged[0].approved_by.as_deref(), Some("always-allow"));
    }

    #[test]
    fn a_read_only_call_records_that_it_needed_no_approval() {
        let db = temp_db("run_automatic");
        let ctx = ToolContext { db: &db };
        let mut registry = Registry::new();
        registry.register(probe("probe_tool", false)).unwrap();

        run(&registry, &ctx, "probe_tool", &json!({"path": "/tmp"})).unwrap();
        assert_eq!(
            db.recent_actions(1).unwrap()[0].approved_by.as_deref(),
            Some("automatic")
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
