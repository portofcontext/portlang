use super::cost::Cost;
use serde::{Deserialize, Serialize};

/// Boundary policy defining what actions are allowed
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Boundary {
    /// Allowed write patterns (glob patterns)
    #[serde(default)]
    pub allow_write: Vec<String>,

    /// Network policy
    #[serde(default)]
    pub network: NetworkPolicy,

    /// Maximum tokens allowed in the context window
    #[serde(default)]
    pub max_tokens: Option<u64>,

    /// Maximum cost budget for this run
    #[serde(default)]
    pub max_cost: Option<Cost>,

    /// Maximum number of agent steps before termination
    #[serde(default)]
    pub max_steps: Option<u64>,

    /// Enable built-in bash tool for arbitrary shell command execution.
    /// File writes are enforced against allow_write patterns post-execution.
    /// Defaults to true — set to false to restrict the agent to filesystem tools only.
    #[serde(default = "default_true")]
    pub bash: bool,

    /// Optional JSON schema for structured output validation.
    #[serde(default)]
    pub output_schema: Option<serde_json::Value>,
}

fn default_true() -> bool {
    true
}

impl Default for Boundary {
    fn default() -> Self {
        Self {
            allow_write: vec![],
            network: NetworkPolicy::Deny,
            max_tokens: None,
            max_cost: None,
            max_steps: None,
            bash: true,
            output_schema: None,
        }
    }
}

/// Network access policy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum NetworkPolicy {
    /// Allow all network access
    Allow,
    /// Deny all network access
    #[default]
    Deny,
}
