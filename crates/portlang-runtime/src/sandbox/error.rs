use thiserror::Error;

#[derive(Debug, Error)]
pub enum SandboxError {
    /// Boundary violation
    #[error("Boundary violation: {0}")]
    BoundaryViolation(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Path escape attempt
    #[error("Path escape attempt: {0}")]
    PathEscape(String),

    /// Tool execution error
    #[error("Tool error: {0}")]
    ToolError(String),

    /// Command execution error
    #[error("Command error: {0}")]
    CommandError(String),

    /// Sandbox initialization error
    #[error("Initialization error: {0}")]
    InitError(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// MCP server failed to start
    #[error("MCP server failed to start: {0}")]
    McpServerStartupError(String),

    /// MCP tool execution failed
    #[error("MCP tool execution failed: {0}")]
    McpToolError(String),

    /// MCP server unreachable
    #[error("MCP server unreachable: {0}")]
    McpServerUnreachable(String),

    /// Other error
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, SandboxError>;

/// Specific boundary violation error
#[derive(Debug, Clone)]
pub struct BoundaryViolation {
    pub description: String,
}

impl BoundaryViolation {
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
        }
    }
}

impl std::fmt::Display for BoundaryViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description)
    }
}
