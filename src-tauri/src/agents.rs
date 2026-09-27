// Agent Browser: discovery and management of AI agents
//
// Provides the agent catalogue, selection state persistence, and filtering logic
// for the Agent Browser UI. Built-in agents cover common use cases: code review,
// architecture planning, quality analysis, and context optimization.

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
    Collector,
    Reviewer,
    Planner,
}

impl std::fmt::Display for AgentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentType::Collector => write!(f, "Collector"),
            AgentType::Reviewer => write!(f, "Reviewer"),
            AgentType::Planner => write!(f, "Planner"),
        }
    }
}

/// Built-in agent catalogue for MVP
pub fn built_in_agents() -> Vec<Agent> {
    vec![
        Agent {
            id: "claude-opus-reviewer".to_string(),
            name: "Claude-Opus Code Reviewer".to_string(),
            description: "Deep code review specialist with architectural insights. Analyzes design patterns, performance bottlenecks, and security vulnerabilities with detailed feedback.".to_string(),
            agent_type: AgentType::Reviewer,
            model: "claude-opus".to_string(),
            capabilities: vec!["code-review".to_string(), "testing".to_string()],
            installed: false,
            selected: false,
        },
        Agent {
            id: "gpt4o-architect".to_string(),
            name: "GPT-4o Architect".to_string(),
            description: "System design and architecture specialist. Plans scalable solutions, evaluates technology choices, and guides long-term architectural decisions.".to_string(),
            agent_type: AgentType::Planner,
            model: "gpt-4o".to_string(),
            capabilities: vec!["architecture".to_string(), "documentation".to_string()],
            installed: false,
            selected: false,
        },
        Agent {
            id: "claude-sonnet-qa".to_string(),
            name: "Claude-Sonnet Quality Analyst".to_string(),
            description: "Quality assurance and performance specialist. Tests edge cases, validates performance requirements, and identifies regressions before production.".to_string(),
            agent_type: AgentType::Reviewer,
            model: "claude-sonnet".to_string(),
            capabilities: vec!["testing".to_string(), "performance".to_string()],
            installed: false,
            selected: false,
        },
        Agent {
            id: "haiku-context-optimizer".to_string(),
            name: "Haiku Context Optimizer".to_string(),
            description: "Context and knowledge management. Summarizes discussions, maintains context windows, and identifies key information to preserve across sessions.".to_string(),
            agent_type: AgentType::Collector,
            model: "claude-haiku".to_string(),
            capabilities: vec!["summarization".to_string(), "context".to_string()],
            installed: false,
            selected: false,
        },
    ]
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
    let mut capabilities: Vec<String> = agents
        .iter()
        .flat_map(|a| a.capabilities.clone())
        .collect();
    capabilities.sort();
    capabilities.dedup();
    capabilities
}

/// Returns all agent types
#[allow(dead_code)]
pub fn all_types() -> Vec<AgentType> {
    vec![AgentType::Collector, AgentType::Reviewer, AgentType::Planner]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_built_in_agents_count() {
        assert_eq!(built_in_agents().len(), 4);
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
