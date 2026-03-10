use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Raw TOML field structure (before validation)
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawField {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub model: RawModel,
    pub prompt: RawPrompt,
    #[serde(default)]
    pub environment: Option<RawEnvironment>,
    #[serde(default)]
    pub boundary: Option<RawBoundary>,
    #[serde(default)]
    pub tool: Vec<RawTool>,
    #[serde(default)]
    pub verifier: Vec<RawVerifier>,
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
pub struct RawModel {
    pub name: String,
    #[serde(default)]
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawPrompt {
    pub goal: String,
    #[serde(default)]
    pub system: Option<String>,
    #[serde(default)]
    pub re_observation: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawEnvironment {
    #[serde(default = "default_workspace_root")]
    pub root: String,
    #[serde(default)]
    pub packages: Vec<String>,
    #[serde(default)]
    pub dockerfile: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
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
    #[serde(default)]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub max_cost: Option<StringOrNumber>,
    #[serde(default)]
    pub max_steps: Option<u64>,
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

/// Unified tool definition from TOML (covers python, shell, mcp)
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawTool {
    #[serde(rename = "type")]
    pub tool_type: String,

    // Common fields
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_schema")]
    pub input_schema: Option<serde_json::Value>,
    #[serde(default, deserialize_with = "deserialize_optional_schema")]
    pub output_schema: Option<serde_json::Value>,

    // Python fields
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub function: Option<String>,

    // Shell fields
    #[serde(default)]
    pub command: Option<String>,

    // MCP fields
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub transport: Option<String>,
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
