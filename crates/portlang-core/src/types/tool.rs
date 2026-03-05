use serde::{Deserialize, Serialize};
use std::fmt;

/// Tool name wrapper - allows any string as a tool name
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(transparent)]
pub struct ToolName(String);

impl ToolName {
    /// Create a new tool name
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Get the tool name as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ToolName {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for ToolName {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl fmt::Display for ToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
