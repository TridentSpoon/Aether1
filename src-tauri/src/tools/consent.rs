// Consent: how a tool that changes the machine gets permission to run.
//
// The rule is that a mutating tool never executes as a side effect of a conversation. It
// is *proposed* -- written down, shown to the operator, and left waiting -- and only runs
// when they say so. Three things follow from taking that seriously:
//
//   1. The queue lives in the database, not in memory. A proposal outlives a restart, so
//      closing the app with a card on screen doesn't silently drop the request, and the
//      log shows what was asked for even if nobody ever answered.
//   2. Proposals expire. An approval is consent to do something *now*; a card answered an
//      hour later is answering a question about a machine that has moved on.
//   3. Approval is recorded with who gave it -- the operator, or an earlier decision to
//      always allow this tool. "It was approved" and "it was approved by a rule you set
//      last week" are different facts and the log keeps them apart.

use serde_json::Value;

use super::{validate_args, Registry, ToolContext};
use crate::llm::{ActionRecord, ActionStatus, MemoryDb};

/// How long a proposal stays answerable.
pub const PROPOSAL_TTL_MINUTES: u32 = 15;

/// Setting holding the tools the operator has chosen to stop being asked about.
const ALWAYS_ALLOW_SETTING: &str = "tool_always_allow";

/// Tools the operator has pre-approved.
pub fn always_allowed(db: &MemoryDb) -> Vec<String> {
    db.get_setting(ALWAYS_ALLOW_SETTING)
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
        .unwrap_or_default()
}

pub fn is_always_allowed(db: &MemoryDb, tool: &str) -> bool {
    always_allowed(db).iter().any(|t| t == tool)
}

/// Adds or removes a tool from the pre-approved list.
pub fn set_always_allowed(db: &MemoryDb, tool: &str, allowed: bool) -> Result<(), String> {
    let mut list = always_allowed(db);
    list.retain(|t| t != tool);
    if allowed {
        list.push(tool.to_string());
    }
    list.sort();
    db.set_setting(ALWAYS_ALLOW_SETTING, &serde_json::json!(list))
        .map_err(|e| e.to_string())
}

/// Records a mutating call as waiting for the operator, and returns its id.
pub fn propose(db: &MemoryDb, tool: &str, args: &Value) -> Result<i64, String> {
    db.log_action(tool, args, true, ActionStatus::Proposed)
        .map_err(|e| format!("could not record the proposed action: {e}"))
}

/// Everything currently waiting, stale proposals having been retired first, each carrying
/// the tool's own description of what it would do -- an approval card is worth nothing if
/// it only says which function is about to be called.
pub fn pending(db: &MemoryDb, registry: &Registry) -> Vec<ActionRecord> {
    let _ = db.expire_stale_proposals(PROPOSAL_TTL_MINUTES);
    let mut waiting = db.pending_actions().unwrap_or_default();
    for action in &mut waiting {
        action.preview = registry
            .get(&action.tool)
            .map(|tool| tool.preview(&action.args));
    }
    waiting
}

/// Runs a proposal the operator has approved.
///
/// Everything is re-checked at approval time rather than trusted from when it was
/// proposed: the tool still has to exist, still has to be registered, and the arguments
/// still have to validate. The row is what carries the request, and a row is exactly the
/// kind of thing that can be edited between being written and being run.
pub fn approve(
    registry: &Registry,
    ctx: &ToolContext,
    id: i64,
    approved_by: &str,
) -> Result<String, String> {
    let _ = ctx.db.expire_stale_proposals(PROPOSAL_TTL_MINUTES);

    let record = ctx
        .db
        .get_action(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no action {id}"))?;

    if record.status != ActionStatus::Proposed {
        return Err(format!(
            "action {id} is already {}, so there is nothing to approve",
            record.status.as_str()
        ));
    }

    let Some(tool) = registry.get(&record.tool) else {
        let message = format!("{} is no longer available", record.tool);
        let _ = ctx.db.set_action_outcome(
            id,
            ActionStatus::Failed,
            Some(&message),
            None,
            Some(approved_by),
        );
        return Err(message);
    };

    if let Err(message) = validate_args(&tool.parameters(), &record.args) {
        let _ = ctx.db.set_action_outcome(
            id,
            ActionStatus::Failed,
            Some(&message),
            None,
            Some(approved_by),
        );
        return Err(message);
    }

    let outcome = tool.call(&record.args, ctx);
    let _ = match &outcome {
        Ok(o) => ctx.db.set_action_outcome(
            id,
            ActionStatus::Executed,
            Some(&o.result),
            o.undo.as_ref(),
            Some(approved_by),
        ),
        Err(e) => {
            ctx.db
                .set_action_outcome(id, ActionStatus::Failed, Some(e), None, Some(approved_by))
        }
    };

    outcome.map(|o| o.result)
}

/// Declines a proposal. Recorded rather than deleted: what the companion asked to do and
/// was told not to do is part of the history worth keeping.
pub fn reject(db: &MemoryDb, id: i64) -> Result<(), String> {
    let record = db
        .get_action(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no action {id}"))?;

    if record.status != ActionStatus::Proposed {
        return Err(format!("action {id} is already {}", record.status.as_str()));
    }

    db.set_action_outcome(
        id,
        ActionStatus::Rejected,
        Some("declined by the operator"),
        None,
        Some("operator"),
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{Outcome, Tool};
    use serde_json::json;

    struct Writer;

    impl Tool for Writer {
        fn name(&self) -> &'static str {
            "probe_writer"
        }
        fn description(&self) -> &'static str {
            "a mutating test tool"
        }
        fn parameters(&self) -> Value {
            json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"]
            })
        }
        fn mutating(&self) -> bool {
            true
        }
        fn call(&self, args: &Value, _ctx: &ToolContext) -> Result<Outcome, String> {
            Ok(Outcome::reversible(
                format!("wrote {}", args["path"]),
                json!({"restore": "previous"}),
            ))
        }
    }

    fn fixture(name: &str) -> (MemoryDb, Registry) {
        let path =
            std::env::temp_dir().join(format!("aether1_consent_{name}_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = MemoryDb::open(path).unwrap();
        let mut registry = Registry::new();
        registry.register(Box::new(Writer)).unwrap();
        (db, registry)
    }

    #[test]
    fn a_proposal_waits_and_runs_nothing_until_approved() {
        let (db, registry) = fixture("waits");
        let ctx = ToolContext { db: &db };

        let id = propose(&db, "probe_writer", &json!({"path": "/tmp/x"})).unwrap();
        let waiting = pending(&db, &registry);
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].id, id);
        assert_eq!(waiting[0].status, ActionStatus::Proposed);
        assert!(waiting[0].result.is_none(), "nothing has run");
        assert!(waiting[0].approved_by.is_none());

        let result = approve(&registry, &ctx, id, "operator").unwrap();
        assert_eq!(result, "wrote \"/tmp/x\"");

        let done = db.get_action(id).unwrap().unwrap();
        assert_eq!(done.status, ActionStatus::Executed);
        assert_eq!(done.approved_by.as_deref(), Some("operator"));
        assert!(done.undo.is_some(), "the undo payload is kept for step 7");
        assert!(pending(&db, &registry).is_empty());
    }

    #[test]
    fn a_rejection_is_recorded_rather_than_forgotten() {
        let (db, registry) = fixture("rejects");
        let id = propose(&db, "probe_writer", &json!({"path": "/tmp/x"})).unwrap();

        reject(&db, id).unwrap();
        let record = db.get_action(id).unwrap().unwrap();
        assert_eq!(record.status, ActionStatus::Rejected);
        assert_eq!(record.approved_by.as_deref(), Some("operator"));
        assert!(pending(&db, &registry).is_empty());
    }

    #[test]
    fn an_action_cannot_be_approved_twice() {
        let (db, registry) = fixture("twice");
        let ctx = ToolContext { db: &db };
        let id = propose(&db, "probe_writer", &json!({"path": "/tmp/x"})).unwrap();

        approve(&registry, &ctx, id, "operator").unwrap();
        let err = approve(&registry, &ctx, id, "operator").unwrap_err();
        assert!(err.contains("already executed"), "{err}");
    }

    #[test]
    fn a_rejected_action_cannot_then_be_approved() {
        let (db, registry) = fixture("rejected_then_approved");
        let ctx = ToolContext { db: &db };
        let id = propose(&db, "probe_writer", &json!({"path": "/tmp/x"})).unwrap();

        reject(&db, id).unwrap();
        let err = approve(&registry, &ctx, id, "operator").unwrap_err();
        assert!(err.contains("already rejected"), "{err}");
    }

    #[test]
    fn arguments_are_revalidated_at_approval_time() {
        // The row is the request, and a row can be edited between being written and being
        // run. Trusting the proposal would make the log the security boundary.
        let (db, registry) = fixture("revalidate");
        let ctx = ToolContext { db: &db };
        let id = propose(&db, "probe_writer", &json!({"path": 7})).unwrap();

        let err = approve(&registry, &ctx, id, "operator").unwrap_err();
        assert!(err.contains("should be a string"), "{err}");
        assert_eq!(
            db.get_action(id).unwrap().unwrap().status,
            ActionStatus::Failed
        );
    }

    #[test]
    fn a_proposal_for_a_tool_that_no_longer_exists_fails_cleanly() {
        let (db, _registry) = fixture("gone");
        let ctx = ToolContext { db: &db };
        let empty = Registry::new();
        let id = propose(&db, "probe_writer", &json!({"path": "/tmp/x"})).unwrap();

        let err = approve(&empty, &ctx, id, "operator").unwrap_err();
        assert!(err.contains("no longer available"), "{err}");
    }

    #[test]
    fn stale_proposals_expire_instead_of_waiting_forever() {
        let (db, registry) = fixture("stale");
        let ctx = ToolContext { db: &db };
        let id = propose(&db, "probe_writer", &json!({"path": "/tmp/x"})).unwrap();

        // Backdate it past the TTL.
        db.set_setting("unused", &json!(1)).unwrap();
        rusqlite::Connection::open(
            std::env::temp_dir().join(format!("aether1_consent_stale_{}.db", std::process::id())),
        )
        .unwrap()
        .execute(
            "UPDATE action_log SET ts = datetime('now', '-60 minutes') WHERE id = ?1",
            [id],
        )
        .unwrap();

        assert!(
            pending(&db, &registry).is_empty(),
            "an old proposal is not still waiting"
        );
        let err = approve(&registry, &ctx, id, "operator").unwrap_err();
        assert!(err.contains("already rejected"), "{err}");
    }

    #[test]
    fn the_always_allow_list_round_trips() {
        let (db, _registry) = fixture("always_allow");
        assert!(!is_always_allowed(&db, "probe_writer"));

        set_always_allowed(&db, "probe_writer", true).unwrap();
        assert!(is_always_allowed(&db, "probe_writer"));
        assert_eq!(always_allowed(&db), vec!["probe_writer".to_string()]);

        // Idempotent: allowing twice doesn't duplicate the entry.
        set_always_allowed(&db, "probe_writer", true).unwrap();
        assert_eq!(always_allowed(&db).len(), 1);

        set_always_allowed(&db, "probe_writer", false).unwrap();
        assert!(!is_always_allowed(&db, "probe_writer"));
    }
}
