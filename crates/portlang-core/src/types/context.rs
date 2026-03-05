use super::cost::Cost;
use serde::{Deserialize, Serialize};

/// Context policy defining token budget and management
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextPolicy {
    /// Maximum tokens allowed in the context window
    /// Budget may be slightly exceeded before termination
    #[serde(default)]
    pub max_tokens: Option<u64>,

    /// Maximum cost budget for this run
    #[serde(default)]
    pub max_cost: Option<Cost>,

    /// Maximum number of agent steps before termination
    /// Prevents infinite loops
    #[serde(default)]
    pub max_steps: Option<u64>,

    /// System prompt to prepend to all interactions
    #[serde(default)]
    pub system_prompt: Option<String>,
}

impl Default for ContextPolicy {
    fn default() -> Self {
        Self {
            max_tokens: None,
            max_cost: None,
            max_steps: None,
            system_prompt: None,
        }
    }
}

impl ContextPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_max_tokens(mut self, max_tokens: u64) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub fn with_max_cost(mut self, max_cost: Cost) -> Self {
        self.max_cost = Some(max_cost);
        self
    }

    pub fn with_max_steps(mut self, max_steps: u64) -> Self {
        self.max_steps = Some(max_steps);
        self
    }

    pub fn with_system_prompt(mut self, prompt: String) -> Self {
        self.system_prompt = Some(prompt);
        self
    }
}
