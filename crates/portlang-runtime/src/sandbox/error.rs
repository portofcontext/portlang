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

/// Type of boundary violation
#[derive(Debug, Clone)]
pub enum ViolationType {
    WriteNotAllowed,
    NetworkDenied,
    PathEscape,
    Other(String),
}

/// Specific boundary violation error with structured data
#[derive(Debug, Clone)]
pub struct BoundaryViolation {
    pub violation_type: ViolationType,
    pub attempted_value: Option<String>,
    pub allowed_patterns: Vec<String>,
    pub context_trace: Option<String>,
    pub description: String,
}

impl BoundaryViolation {
    /// Create a simple violation with just a description
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            violation_type: ViolationType::Other("unknown".to_string()),
            attempted_value: None,
            allowed_patterns: Vec::new(),
            context_trace: None,
            description: description.into(),
        }
    }

    /// Create a write violation with context trace
    pub fn write_not_allowed(
        path: String,
        allowed_patterns: Vec<String>,
        context_trace: Option<String>,
    ) -> Self {
        let description = format!(
            "Write to '{}' not allowed. Allowed patterns: {:?}",
            path, allowed_patterns
        );

        Self {
            violation_type: ViolationType::WriteNotAllowed,
            attempted_value: Some(path),
            allowed_patterns,
            context_trace,
            description,
        }
    }

    /// Get the full message including context trace
    pub fn full_message(&self) -> String {
        let mut msg = format!("REJECTED: {}", self.description);

        if let Some(ref trace) = self.context_trace {
            msg.push_str("\n");
            msg.push_str(trace);
        }

        msg
    }
}

impl std::fmt::Display for BoundaryViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.full_message())
    }
}
