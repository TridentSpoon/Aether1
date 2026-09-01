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

// The trait's methods and most of the registry are exercised by this module's tests but
// not yet called by the engine -- the tool loop that calls them is the next step. This
// allow comes off with the first registered tool; until then it is the price of landing
// the machinery before the thing it constrains.
#![allow(dead_code)]

use std::sync::LazyLock;

use serde::Serialize;
use serde_json::{json, Value};

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

    /// Runs the tool. `args` has already been checked against `parameters`. The returned
    /// string goes back to the model as the tool's result, so it should be terse and
    /// factual; errors are returned as Err and reach the model as a failure it can react
    /// to, rather than as a crash.
    fn call(&self, args: &Value) -> Result<Outcome, String>;
}

/// What a tool call produced.
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
    #[allow(dead_code)] // used by the first tools, in the next step
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

    pub fn len(&self) -> usize {
        self.tools.len()
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
    static REGISTRY: LazyLock<Registry> = LazyLock::new(Registry::new);
    &REGISTRY
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
        fn call(&self, args: &Value) -> Result<Outcome, String> {
            Ok(Outcome::text(format!("saw {}", args["path"])))
        }
    }

    fn probe(name: &'static str, mutating: bool) -> Box<dyn Tool> {
        Box::new(Probe { name, mutating })
    }

    #[test]
    fn the_shipped_registry_is_empty_for_now() {
        // Guards the invariant that makes this step safe to land: with nothing registered,
        // the engine can't call anything.
        assert!(registry().is_empty());
    }

    #[test]
    fn tools_are_found_by_name_and_run() {
        let mut registry = Registry::new();
        registry.register(probe("read_file", false)).unwrap();

        let tool = registry.get("read_file").expect("registered tool");
        assert!(!tool.mutating());
        let outcome = tool.call(&json!({"path": "/etc/hostname"})).unwrap();
        assert_eq!(outcome.result, "saw \"/etc/hostname\"");
        assert!(outcome.undo.is_none());

        assert!(registry.get("write_file").is_none());
    }

    #[test]
    fn a_duplicate_name_is_refused_not_shadowed() {
        let mut registry = Registry::new();
        registry.register(probe("read_file", false)).unwrap();
        let err = registry.register(probe("read_file", true)).unwrap_err();
        assert!(err.contains("already registered"), "{err}");
        assert_eq!(registry.len(), 1);
        assert!(!registry.get("read_file").unwrap().mutating());
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
