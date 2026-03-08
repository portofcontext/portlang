use serde::{Deserialize, Serialize};

/// Environment configuration for field execution
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Environment {
    /// Local filesystem environment with a root directory
    Local { root: String },
}

/// Container configuration for sandbox customization
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[derive(Default)]
pub struct ContainerConfig {
    /// APT packages to install in the container (e.g., ["nodejs", "npm"])
    #[serde(default)]
    pub packages: Vec<String>,

    /// Path to custom Dockerfile (relative to field.toml)
    #[serde(default)]
    pub dockerfile: Option<String>,

    /// Pre-built Docker image to use instead of default
    #[serde(default)]
    pub image: Option<String>,
}

/// Snapshot configuration (not used in Phase 1, but defined for future)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
#[derive(Default)]
pub enum Snapshot {
    /// No snapshot
    #[default]
    None,
    /// Git-based snapshot
    Git {
        /// Whether to create a new commit
        commit: bool,
    },
}
