use super::{
    boundary::Boundary,
    context::ContextPolicy,
    environment::{ContainerConfig, Environment},
    model::ModelSpec,
    verifier::Verifier,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Custom tool configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomTool {
    pub name: String,
    pub description: String,
    pub tool_type: String,
    // Shell tool fields
    pub command: Option<String>,
    // Python tool fields
    pub script: Option<String>,
    pub function: Option<String>,
    pub input_schema: serde_json::Value,
}

/// A field defines a complete agent task configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    /// Name of the field
    pub name: String,

    /// Human-readable description of the field's purpose
    #[serde(default)]
    pub description: Option<String>,

    /// Model configuration
    pub model: ModelSpec,

    /// Environment configuration
    pub environment: Environment,

    /// Boundary policy
    #[serde(default)]
    pub boundary: Boundary,

    /// Context policy (token budget, cost limits)
    #[serde(default)]
    pub context: ContextPolicy,

    /// Verifiers to run during execution
    #[serde(default)]
    pub verifiers: Vec<Verifier>,

    /// Re-observation commands to run before each step
    #[serde(default)]
    pub re_observation: Vec<String>,

    /// Optional custom environment context to append
    #[serde(default)]
    pub environment_context: Option<String>,

    /// Initial prompt/goal for the agent
    pub goal: String,

    /// Custom tools defined in the field
    #[serde(default)]
    pub custom_tools: Vec<CustomTool>,

    /// Enable Code Mode execution (requires code-mode feature)
    #[serde(default)]
    pub code_mode: Option<CodeModeConfig>,

    /// MCP servers to connect to
    #[serde(default)]
    pub mcp_servers: Vec<McpServer>,

    /// Container configuration for sandbox customization
    #[serde(default)]
    pub container: ContainerConfig,

    /// Optional JSON schema for structured output validation
    #[serde(default)]
    pub output_schema: Option<serde_json::Value>,

    /// Directory containing the field.toml file (for path resolution)
    /// None if field was loaded from stdin or string
    #[serde(skip)]
    pub config_dir: Option<PathBuf>,
}

/// Code Mode configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeModeConfig {
    /// Enable code mode
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// MCP server transport type
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum McpTransport {
    /// Stdio transport (local process)
    Stdio {
        command: String,
        args: Vec<String>,
        env: HashMap<String, String>,
    },
    /// SSE/HTTP transport (remote server)
    Sse {
        url: String,
        headers: HashMap<String, String>,
    },
}

/// MCP server configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpServer {
    pub name: String,
    pub transport: McpTransport,
}

impl Field {
    pub fn new(name: String, model: ModelSpec, environment: Environment, goal: String) -> Self {
        Self {
            name,
            description: None,
            model,
            environment,
            boundary: Boundary::default(),
            context: ContextPolicy::default(),
            verifiers: Vec::new(),
            re_observation: Vec::new(),
            environment_context: None,
            goal,
            custom_tools: Vec::new(),
            code_mode: None,
            mcp_servers: Vec::new(),
            container: ContainerConfig::default(),
            output_schema: None,
            config_dir: None,
        }
    }

    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    pub fn with_boundary(mut self, boundary: Boundary) -> Self {
        self.boundary = boundary;
        self
    }

    pub fn with_context(mut self, context: ContextPolicy) -> Self {
        self.context = context;
        self
    }

    pub fn add_verifier(mut self, verifier: Verifier) -> Self {
        self.verifiers.push(verifier);
        self
    }

    pub fn add_re_observation(mut self, command: String) -> Self {
        self.re_observation.push(command);
        self
    }
}
