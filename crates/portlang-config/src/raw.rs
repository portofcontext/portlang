use serde::{Deserialize, Serialize};

/// Raw TOML field structure (before validation)
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawField {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub model: RawModel,
    pub environment: RawEnvironment,
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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum RawEnvironment {
    Local { root: String },
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
    pub name: String,
    pub description: String,
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
    pub input_schema: serde_json::Value,
}
