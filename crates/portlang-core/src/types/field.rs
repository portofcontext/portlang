use super::{boundary::Boundary, environment::Environment, model::ModelSpec, verifier::Verifier};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Prompt configuration — required section
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prompt {
    /// The agent's goal / initial prompt
    pub goal: String,

    /// Optional system prompt (replaces environment_context + context.system_prompt)
    #[serde(default)]
    pub system: Option<String>,

    /// Re-observation commands to run before each step
    #[serde(default)]
    pub re_observation: Vec<String>,
}

/// Unified tool configuration (python, shell, or mcp)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tool {
    /// Tool type: "python", "shell", or "mcp"
    pub tool_type: String,

    /// Tool name
    #[serde(default)]
    pub name: Option<String>,

    /// Tool description
    #[serde(default)]
    pub description: Option<String>,

    // Python tool fields
    /// Path to the Python script file (renamed from "script")
    #[serde(default)]
    pub file: Option<String>,

    /// Python function name to call
    #[serde(default)]
    pub function: Option<String>,

    /// Input JSON schema
    #[serde(default)]
    pub input_schema: serde_json::Value,

    /// Output JSON schema (optional)
    #[serde(default)]
    pub output_schema: Option<serde_json::Value>,

    // Shell tool fields
    /// Shell command template
    #[serde(default)]
    pub command: Option<String>,

    // MCP tool fields
    /// MCP command args
    #[serde(default)]
    pub args: Vec<String>,

    /// MCP environment variables
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// MCP server URL (for HTTP/SSE transport)
    #[serde(default)]
    pub url: Option<String>,

    /// MCP HTTP headers
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,

    /// MCP transport: "stdio", "http", or "sse"
    #[serde(default)]
    pub transport: Option<McpTransport>,

    /// Whitelist: only expose these tool names from the MCP server
    #[serde(default)]
    pub include_tools: Option<Vec<String>>,

    /// Blacklist: exclude these tool names from the MCP server
    #[serde(default)]
    pub exclude_tools: Option<Vec<String>>,

    /// Path to a JSON patch file (relative to field.toml directory) with per-tool patches
    #[serde(default)]
    pub patch_file: Option<String>,
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

/// MCP server configuration (kept for runtime use by McpServerManager)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpServer {
    pub name: String,
    pub transport: McpTransport,
}

/// Per-tool patch — overrides/extends what the MCP server reports
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpToolPatch {
    /// Override the tool description from the MCP server
    #[serde(default)]
    pub description: Option<String>,
    /// Inject an output schema (most MCP servers omit this; patches supply it for code mode)
    #[serde(default)]
    pub output_schema: Option<serde_json::Value>,
}

/// Map of MCP tool name → patch (the patch file format)
pub type McpPatchMap = HashMap<String, McpToolPatch>;

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

    /// Prompt configuration (goal, system, re_observation)
    pub prompt: Prompt,

    /// Environment configuration
    #[serde(default)]
    pub environment: Environment,

    /// Boundary policy
    #[serde(default)]
    pub boundary: Boundary,

    /// Unified tools list (python, shell, mcp)
    #[serde(default)]
    pub tools: Vec<Tool>,

    /// Verifiers to run during execution
    #[serde(default)]
    pub verifiers: Vec<Verifier>,

    /// Directory containing the field.toml file (for path resolution)
    /// None if field was loaded from stdin or string
    #[serde(skip)]
    pub config_dir: Option<PathBuf>,
}

impl Field {
    pub fn new(name: String, model: ModelSpec, environment: Environment, goal: String) -> Self {
        Self {
            name,
            description: None,
            model,
            prompt: Prompt {
                goal,
                system: None,
                re_observation: Vec::new(),
            },
            environment,
            boundary: Boundary::default(),
            tools: Vec::new(),
            verifiers: Vec::new(),
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

    pub fn add_verifier(mut self, verifier: Verifier) -> Self {
        self.verifiers.push(verifier);
        self
    }

    pub fn add_re_observation(mut self, command: String) -> Self {
        self.prompt.re_observation.push(command);
        self
    }
}
