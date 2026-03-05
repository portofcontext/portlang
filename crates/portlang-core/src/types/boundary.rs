use serde::{Deserialize, Serialize};

/// Boundary policy defining what actions are allowed
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Boundary {
    /// Allowed write patterns (glob patterns)
    #[serde(default)]
    pub allow_write: Vec<String>,

    /// Network policy
    #[serde(default)]
    pub network: NetworkPolicy,
}

impl Default for Boundary {
    fn default() -> Self {
        Self {
            allow_write: vec![],
            network: NetworkPolicy::Deny,
        }
    }
}

/// Network access policy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkPolicy {
    /// Allow all network access
    Allow,
    /// Deny all network access
    Deny,
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        NetworkPolicy::Deny
    }
}
