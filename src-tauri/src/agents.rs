// Agent Browser: task-focused AETHER CODE profiles.

use serde::{Deserialize, Serialize};

/// Represents a single AI agent available for discovery and selection
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub description: String,
    pub agent_type: AgentType,
    pub model: String,
    pub capabilities: Vec<String>,
    pub installed: bool,
    pub selected: bool,
}

/// Classification of agent purpose
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum AgentType {
    Builder,
    Collector,
    Reviewer,
    Planner,
}

impl std::fmt::Display for AgentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentType::Builder => write!(f, "Builder"),
            AgentType::Collector => write!(f, "Collector"),
            AgentType::Reviewer => write!(f, "Reviewer"),
            AgentType::Planner => write!(f, "Planner"),
        }
    }
}

/// The bundled profiles all run through AETHER CODE's configured coding model. They are
/// prompt profiles, not claims that Claude/GPT accounts or separate model processes exist.
pub fn built_in_agents() -> Vec<Agent> {
    vec![
        Agent {
            id: "code-builder".to_string(),
            name: "Code Builder".to_string(),
            description: "Implements requested changes in the selected workspace, follows AGENTS.md, and verifies the result when AETHER CODE's Edit and Run permissions allow it.".to_string(),
            agent_type: AgentType::Builder,
            model: "active AETHER CODE model".to_string(),
            capabilities: vec!["implementation".to_string(), "refactoring".to_string(), "testing".to_string()],
            installed: true,
            selected: false,
        },
        Agent {
            id: "code-reviewer".to_string(),
            name: "Code Reviewer".to_string(),
            description: "Reviews the selected repository or changes for correctness, security, regressions, and maintainability. Reports actionable findings with file and line references.".to_string(),
            agent_type: AgentType::Reviewer,
            model: "active AETHER CODE model".to_string(),
            capabilities: vec!["code-review".to_string(), "security".to_string(), "regressions".to_string()],
            installed: true,
            selected: false,
        },
        Agent {
            id: "implementation-planner".to_string(),
            name: "Implementation Planner".to_string(),
            description: "Maps the repository structure and turns a requested change into a small, ordered implementation plan with risks and verification steps.".to_string(),
            agent_type: AgentType::Planner,
            model: "active AETHER CODE model".to_string(),
            capabilities: vec!["architecture".to_string(), "implementation".to_string()],
            installed: true,
            selected: false,
        },
        Agent {
            id: "test-analyst".to_string(),
            name: "Test Analyst".to_string(),
            description: "Traces behavior and edge cases, checks existing test coverage, and recommends or runs focused checks when AETHER CODE's run permission is enabled.".to_string(),
            agent_type: AgentType::Reviewer,
            model: "active AETHER CODE model".to_string(),
            capabilities: vec!["testing".to_string(), "edge-cases".to_string()],
            installed: true,
            selected: false,
        },
        Agent {
            id: "repository-guide".to_string(),
            name: "Repository Guide".to_string(),
            description: "Explores the codebase and explains its architecture, key files, data flow, and conventions with references to the source.".to_string(),
            agent_type: AgentType::Collector,
            model: "active AETHER CODE model".to_string(),
            capabilities: vec!["architecture".to_string(), "documentation".to_string()],
            installed: true,
            selected: false,
        },
    ]
}

/// The profile instructions are applied by the backend; a browser client cannot invent
/// arbitrary system capabilities by submitting its own profile text.
pub fn instructions(id: &str) -> Option<&'static str> {
    match id {
        "code-reviewer" => Some("Act as a senior code reviewer and perform your own analysis. Inspect the relevant repository files and diffs before judging. Prioritize concrete bugs, security issues, data loss, and regressions. Report findings first, highest severity first, with file paths and line numbers. If you find no issues, say so and mention any remaining uncertainty. Do not edit files unless the task explicitly asks you to implement a fix."),
        "code-builder" => Some("Act as a coding agent implementing the operator's requested change. First inspect the workspace AGENTS.md, then read the relevant code and callers; inspect git status and diff when Run is enabled. Make the requested minimal code changes with edit_file or create_file; do not stop at a plan when implementation was requested. Preserve unrelated work and never commit, push, reset, clean, or rewrite history. Run focused checks only if the operator enabled Run; if Edit or Run is unavailable, explain which permission is missing and continue with the capability that is available. Finish with a concise summary of changed files, checks actually run, and anything still unverified."),
        "implementation-planner" => Some("Act as a software architect planning this change. Inspect the relevant code and repository conventions. Give a concise ordered plan, name the files and interfaces involved, call out compatibility risks, and define how success can be verified. Do not edit files."),
        "test-analyst" => Some("Act as a test and reliability analyst. Trace the requested behavior through the code, inspect existing tests, and identify edge cases or regressions. Run focused checks only when the operator has enabled run permission. Report concrete findings and missing coverage with file references. Do not edit files unless explicitly asked."),
        "repository-guide" => Some("Act as a repository guide. Inspect the code rather than guessing. Explain the relevant architecture, entry points, data flow, and conventions with file references, then answer the task directly. Do not edit files."),
        _ => None,
    }
}

/// Retrieves all agents, with selection state persisted and merged
pub fn get_agents(selected_agent_ids: Option<Vec<String>>) -> Vec<Agent> {
    let mut agents = built_in_agents();

    // Apply persisted selection state
    if let Some(ids) = selected_agent_ids {
        let selected_set: std::collections::HashSet<_> = ids.into_iter().collect();
        for agent in &mut agents {
            agent.selected = selected_set.contains(&agent.id);
        }
    }

    agents
}

/// Filters agents by type
#[allow(dead_code)]
pub fn filter_by_type(agents: &[Agent], agent_type: &AgentType) -> Vec<Agent> {
    agents
        .iter()
        .filter(|a| a.agent_type == *agent_type)
        .cloned()
        .collect()
}

/// Filters agents by capability
#[allow(dead_code)]
pub fn filter_by_capability(agents: &[Agent], capability: &str) -> Vec<Agent> {
    agents
        .iter()
        .filter(|a| a.capabilities.iter().any(|c| c == capability))
        .cloned()
        .collect()
}

/// Filters agents by search query (name and description)
#[allow(dead_code)]
pub fn search_agents(agents: &[Agent], query: &str) -> Vec<Agent> {
    let query_lower = query.to_lowercase();
    agents
        .iter()
        .filter(|a| {
            a.name.to_lowercase().contains(&query_lower)
                || a.description.to_lowercase().contains(&query_lower)
        })
        .cloned()
        .collect()
}

/// Applies multiple filters in sequence
#[allow(dead_code)]
pub fn apply_filters(
    agents: &[Agent],
    type_filter: Option<&AgentType>,
    capability_filter: Option<&str>,
    search_query: Option<&str>,
) -> Vec<Agent> {
    let mut result = agents.to_vec();

    if let Some(agent_type) = type_filter {
        result = filter_by_type(&result, agent_type);
    }

    if let Some(capability) = capability_filter {
        result = filter_by_capability(&result, capability);
    }

    if let Some(query) = search_query {
        result = search_agents(&result, query);
    }

    result
}

/// Returns all unique capabilities across all agents
#[allow(dead_code)]
pub fn all_capabilities() -> Vec<String> {
    let agents = built_in_agents();
    let mut capabilities: Vec<String> =
        agents.iter().flat_map(|a| a.capabilities.clone()).collect();
    capabilities.sort();
    capabilities.dedup();
    capabilities
}

/// Returns all agent types
#[allow(dead_code)]
pub fn all_types() -> Vec<AgentType> {
    vec![
        AgentType::Builder,
        AgentType::Collector,
        AgentType::Reviewer,
        AgentType::Planner,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_built_in_agents_count() {
        assert_eq!(built_in_agents().len(), 5);
    }

    #[test]
    fn every_browser_profile_has_a_backend_instruction_set() {
        for agent in built_in_agents() {
            let instructions = instructions(&agent.id)
                .unwrap_or_else(|| panic!("{} has no executable profile", agent.id));
            assert!(!instructions.is_empty());
            assert_eq!(agent.model, "active AETHER CODE model");
        }
    }

    #[test]
    fn unknown_profiles_are_not_executable() {
        assert!(instructions("arbitrary-prompt").is_none());
    }

    #[test]
    fn test_filter_by_type() {
        let agents = built_in_agents();
        let reviewers = filter_by_type(&agents, &AgentType::Reviewer);
        assert_eq!(reviewers.len(), 2);
    }

    #[test]
    fn test_search_agents() {
        let agents = built_in_agents();
        let results = search_agents(&agents, "code");
        assert!(!results.is_empty());
        assert!(results.iter().any(|a| a.name.contains("Reviewer")));
    }

    #[test]
    fn test_all_capabilities() {
        let caps = all_capabilities();
        assert!(caps.contains(&"code-review".to_string()));
        assert!(caps.contains(&"testing".to_string()));
    }
}
