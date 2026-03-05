use serde::{Deserialize, Serialize};

/// Environment configuration for field execution
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Environment {
    /// Local filesystem environment with a root directory
    Local { root: String },
}

/// Snapshot configuration (not used in Phase 1, but defined for future)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Snapshot {
    /// No snapshot
    None,
    /// Git-based snapshot
    Git {
        /// Whether to create a new commit
        commit: bool,
    },
}

impl Default for Snapshot {
    fn default() -> Self {
        Snapshot::None
    }
}
