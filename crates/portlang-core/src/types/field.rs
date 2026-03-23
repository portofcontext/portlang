use super::{
    boundary::Boundary, environment::Environment, model::ModelSpec, skill::Skill,
    verifier::Verifier,
};
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

/// Declaration of a template variable in [vars]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct VarDecl {
    /// Whether this variable is required (default: true)
    #[serde(default = "default_true")]
    pub required: bool,

    /// Default value if not supplied at runtime
    #[serde(default)]
    pub default: Option<String>,

    /// Human-readable description shown in `portlang check`
    #[serde(default)]
    pub description: Option<String>,
}

fn default_true() -> bool {
    true
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

    /// Skills to load into the agent's context
    #[serde(default)]
    pub skills: Vec<Skill>,

    /// Verifiers to run during execution
    #[serde(default)]
    pub verifiers: Vec<Verifier>,

    /// Template variable declarations (from [vars] section)
    #[serde(default)]
    pub vars: HashMap<String, VarDecl>,

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
            skills: Vec::new(),
            verifiers: Vec::new(),
            vars: HashMap::new(),
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

    /// Collect all env var values from tool configuration for secret redaction.
    /// Returns raw values plus any `${VAR}` expansions from the host environment.
    /// Longest values are first so replacements don't partially match shorter substrings.
    pub fn collect_secret_candidates(&self) -> Vec<String> {
        let mut secrets = Vec::new();

        for tool in &self.tools {
            for v in tool.env.values() {
                push_value_and_expansion(v, &mut secrets);
            }
            if let Some(ref headers) = tool.headers {
                for v in headers.values() {
                    push_value_and_expansion(v, &mut secrets);
                }
            }
            match &tool.transport {
                Some(McpTransport::Stdio { env, .. }) => {
                    for v in env.values() {
                        push_value_and_expansion(v, &mut secrets);
                    }
                }
                Some(McpTransport::Sse { headers, .. }) => {
                    for v in headers.values() {
                        push_value_and_expansion(v, &mut secrets);
                    }
                }
                None => {}
            }
        }

        secrets.retain(|s| !s.is_empty());
        secrets.sort_by(|a, b| b.len().cmp(&a.len()));
        secrets.dedup();
        secrets
    }
}

fn push_value_and_expansion(value: &str, secrets: &mut Vec<String>) {
    if value.is_empty() {
        return;
    }
    secrets.push(value.to_string());
    // Also expand ${VAR} references so the actual secret value is redacted too
    if let Some(var_name) = value.strip_prefix("${").and_then(|s| s.strip_suffix("}")) {
        if let Ok(expanded) = std::env::var(var_name) {
            if !expanded.is_empty() {
                secrets.push(expanded);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::environment::Environment;

    fn minimal_field() -> Field {
        Field::new(
            "test".to_string(),
            ModelSpec {
                name: "claude-sonnet-4-5".to_string(),
                temperature: None,
            },
            Environment::default(),
            "goal".to_string(),
        )
    }

    fn mcp_tool_with_env(env: HashMap<String, String>) -> Tool {
        Tool {
            tool_type: "mcp".to_string(),
            name: None,
            description: None,
            file: None,
            function: None,
            input_schema: serde_json::Value::Null,
            output_schema: None,
            command: None,
            args: vec![],
            env,
            url: None,
            headers: None,
            transport: None,
            include_tools: None,
            exclude_tools: None,
            patch_file: None,
        }
    }

    #[test]
    fn test_collects_tool_env_values() {
        let mut field = minimal_field();
        field.tools.push(mcp_tool_with_env(HashMap::from([(
            "API_KEY".to_string(),
            "literal-secret-value".to_string(),
        )])));

        let secrets = field.collect_secret_candidates();
        assert!(secrets.contains(&"literal-secret-value".to_string()));
    }

    #[test]
    fn test_collects_tool_headers() {
        let mut field = minimal_field();
        let mut tool = mcp_tool_with_env(HashMap::new());
        tool.headers = Some(HashMap::from([(
            "Authorization".to_string(),
            "Bearer sk-abc123".to_string(),
        )]));
        field.tools.push(tool);

        let secrets = field.collect_secret_candidates();
        assert!(secrets.contains(&"Bearer sk-abc123".to_string()));
    }

    #[test]
    fn test_collects_stdio_transport_env() {
        let mut field = minimal_field();
        let mut tool = mcp_tool_with_env(HashMap::new());
        tool.transport = Some(McpTransport::Stdio {
            command: "npx".to_string(),
            args: vec![],
            env: HashMap::from([("SECRET".to_string(), "transport-secret-val".to_string())]),
        });
        field.tools.push(tool);

        let secrets = field.collect_secret_candidates();
        assert!(secrets.contains(&"transport-secret-val".to_string()));
    }

    #[test]
    fn test_collects_sse_transport_headers() {
        let mut field = minimal_field();
        let mut tool = mcp_tool_with_env(HashMap::new());
        tool.transport = Some(McpTransport::Sse {
            url: "https://example.com/mcp".to_string(),
            headers: HashMap::from([("x-api-key".to_string(), "sse-header-secret".to_string())]),
        });
        field.tools.push(tool);

        let secrets = field.collect_secret_candidates();
        assert!(secrets.contains(&"sse-header-secret".to_string()));
    }

    #[test]
    fn test_expands_var_references() {
        unsafe { std::env::set_var("_PORTLANG_TEST_SECRET", "expanded-secret-value") };

        let mut field = minimal_field();
        field.tools.push(mcp_tool_with_env(HashMap::from([(
            "KEY".to_string(),
            "${_PORTLANG_TEST_SECRET}".to_string(),
        )])));

        let secrets = field.collect_secret_candidates();
        // Both the raw template and the expanded value should be present
        assert!(secrets.contains(&"${_PORTLANG_TEST_SECRET}".to_string()));
        assert!(secrets.contains(&"expanded-secret-value".to_string()));
    }

    #[test]
    fn test_empty_values_excluded() {
        let mut field = minimal_field();
        field.tools.push(mcp_tool_with_env(HashMap::from([(
            "EMPTY".to_string(),
            "".to_string(),
        )])));

        let secrets = field.collect_secret_candidates();
        assert!(secrets.is_empty());
    }

    #[test]
    fn test_sorted_longest_first() {
        let mut field = minimal_field();
        field.tools.push(mcp_tool_with_env(HashMap::from([
            ("SHORT".to_string(), "abc".to_string()),
            ("LONG".to_string(), "abcdefghij".to_string()),
        ])));

        let secrets = field.collect_secret_candidates();
        // Longest should come first to prevent partial matches during replacement
        assert!(secrets[0].len() >= secrets[secrets.len() - 1].len());
    }

    #[test]
    fn test_no_tools_returns_empty() {
        let field = minimal_field();
        assert!(field.collect_secret_candidates().is_empty());
    }
}
