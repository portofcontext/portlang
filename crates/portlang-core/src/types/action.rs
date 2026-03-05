use super::tool::ToolName;
use serde::{Deserialize, Serialize};

/// Actions that can be taken by the agent
/// Matches Anthropic's message response format
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    ToolCall {
        tool: ToolName,
        #[serde(flatten)]
        input: serde_json::Value,
    },
    TextOutput {
        text: String,
    },
    Stop,
}

impl Action {
    /// Create a tool call action
    pub fn tool_call(tool: ToolName, input: serde_json::Value) -> Self {
        Action::ToolCall { tool, input }
    }

    /// Create a text output action
    pub fn text(text: impl Into<String>) -> Self {
        Action::TextOutput { text: text.into() }
    }

    /// Create a stop action
    pub fn stop() -> Self {
        Action::Stop
    }

    /// Check if this is a stop action
    pub fn is_stop(&self) -> bool {
        matches!(self, Action::Stop)
    }

    /// Check if this is a tool call
    pub fn is_tool_call(&self) -> bool {
        matches!(self, Action::ToolCall { .. })
    }

    /// Get the tool name if this is a tool call
    pub fn tool_name(&self) -> Option<&ToolName> {
        match self {
            Action::ToolCall { tool, .. } => Some(tool),
            _ => None,
        }
    }
}
