use serde::{Deserialize, Serialize};

/// Environment configuration for field execution
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Environment {
    /// Root directory for the workspace
    #[serde(default = "default_workspace_root")]
    pub root: String,

    /// APT packages to install in the container
    #[serde(default)]
    pub packages: Vec<String>,

    /// Path to custom Dockerfile (relative to field.toml)
    #[serde(default)]
    pub dockerfile: Option<String>,

    /// Pre-built Docker image to use instead of default
    #[serde(default)]
    pub image: Option<String>,
}

fn default_workspace_root() -> String {
    "./workspace".to_string()
}

impl Default for Environment {
    fn default() -> Self {
        Self {
            root: default_workspace_root(),
            packages: vec![],
            dockerfile: None,
            image: None,
        }
    }
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
