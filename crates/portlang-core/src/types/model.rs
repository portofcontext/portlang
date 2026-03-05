use serde::{Deserialize, Serialize};

/// Model specification
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelSpec {
    /// Model identifier (e.g., "claude-sonnet-4-5")
    pub name: String,

    /// Temperature for sampling (0.0 to 1.0)
    #[serde(default = "default_temperature")]
    pub temperature: Option<f32>,

    /// Maximum tokens to generate
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

fn default_temperature() -> Option<f32> {
    Some(1.0)
}

impl ModelSpec {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            temperature: Some(1.0),
            max_tokens: None,
        }
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }
}
