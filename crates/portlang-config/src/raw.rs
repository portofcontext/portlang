use serde::{Deserialize, Serialize};

/// Raw TOML field structure (before validation)
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawField {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub model: RawModel,
    #[serde(default)]
    pub environment: Option<RawEnvironment>,
    #[serde(default)]
    pub boundary: Option<RawBoundary>,
    #[serde(default)]
    pub context: Option<RawContext>,
    #[serde(default)]
    pub verifiers: Vec<RawVerifier>,
    #[serde(default)]
    pub re_observation: Vec<String>,
    #[serde(default)]
    pub environment_context: Option<String>,
    pub goal: String,
    #[serde(default)]
    pub tool: Vec<RawCustomTool>,
    #[serde(default)]
    pub code_mode: Option<RawCodeMode>,
    #[serde(default)]
    pub mcp_server: Vec<RawMcpServer>,
    #[serde(default)]
    pub container: Option<RawContainerConfig>,
    #[serde(default, deserialize_with = "deserialize_output_schema")]
    pub output_schema: Option<serde_json::Value>,
}

fn deserialize_output_schema<'de, D>(deserializer: D) -> Result<Option<serde_json::Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum SchemaOrString {
        String(String),
        Value(serde_json::Value),
    }

    match Option::<SchemaOrString>::deserialize(deserializer)? {
        None => Ok(None),
        Some(SchemaOrString::String(s)) => {
            // Parse JSON string
            serde_json::from_str(&s)
                .map(Some)
                .map_err(serde::de::Error::custom)
        }
        Some(SchemaOrString::Value(v)) => Ok(Some(v)),
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawCodeMode {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawContainerConfig {
    #[serde(default)]
    pub packages: Vec<String>,
    #[serde(default)]
    pub dockerfile: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawModel {
    pub name: String,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct RawEnvironment {
    #[serde(default = "default_workspace_root")]
    pub root: String,
}

fn default_workspace_root() -> String {
    "./workspace".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawBoundary {
    #[serde(default)]
    pub allow_write: Vec<String>,
    #[serde(default)]
    pub network: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawContext {
    #[serde(default)]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub max_cost: Option<StringOrNumber>,
    #[serde(default)]
    pub max_steps: Option<u64>,
    #[serde(default)]
    pub system_prompt: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawVerifier {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub trigger: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// Helper type for parsing cost as either string ("$2.00") or number (2.0)
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum StringOrNumber {
    String(String),
    Number(f64),
}

/// Custom tool definition from TOML
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawCustomTool {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub tool_type: String,
    // Shell tool fields
    #[serde(default)]
    pub command: Option<String>,
    // Python tool fields
    #[serde(default)]
    pub script: Option<String>,
    #[serde(default)]
    pub function: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_schema")]
    pub input_schema: Option<serde_json::Value>,
}

fn deserialize_optional_schema<'de, D>(
    deserializer: D,
) -> Result<Option<serde_json::Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum SchemaOrString {
        String(String),
        Value(serde_json::Value),
    }

    match Option::<SchemaOrString>::deserialize(deserializer)? {
        None => Ok(None),
        Some(SchemaOrString::String(s)) => {
            // Parse JSON string
            serde_json::from_str(&s)
                .map(Some)
                .map_err(serde::de::Error::custom)
        }
        Some(SchemaOrString::Value(v)) => Ok(Some(v)),
    }
}

/// MCP server configuration from TOML
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawMcpServer {
    pub name: String,

    // Stdio transport fields
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,

    // HTTP/SSE transport fields
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: Option<std::collections::HashMap<String, String>>,

    // Transport type: "stdio" or "http"/"sse"
    #[serde(default)]
    pub transport: Option<String>,
}
