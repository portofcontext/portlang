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
}

impl Default for Boundary {
    fn default() -> Self {
        Self {
            allow_write: vec![],
            network: NetworkPolicy::Deny,
            max_tokens: None,
            max_cost: None,
            max_steps: None,
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
