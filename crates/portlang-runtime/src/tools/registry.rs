use super::handler::ToolHandler;
use crate::sandbox::error::{Result, SandboxError};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Registry of available tool handlers
pub struct ToolRegistry {
    handlers: HashMap<String, Arc<dyn ToolHandler>>,
}

impl ToolRegistry {
    /// Create a new empty tool registry
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register a tool handler
    pub fn register(&mut self, handler: Arc<dyn ToolHandler>) {
        self.handlers.insert(handler.name().to_string(), handler);
    }

    /// Execute a tool by name
    pub async fn execute(&self, name: &str, root: &Path, input: Value) -> Result<String> {
        let handler = self.handlers.get(name).ok_or_else(|| {
            SandboxError::ToolError(format!(
                "Unknown tool: '{}'. Available tools: {:?}",
                name,
                self.handlers.keys().collect::<Vec<_>>()
            ))
        })?;

        handler.execute(root, input).await
    }

    /// Get tool definitions for API
    pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.handlers
            .values()
            .map(|h| ToolDefinition {
                name: h.name().to_string(),
                description: h.description().to_string(),
                input_schema: h.input_schema(),
            })
            .collect()
    }

    /// Check if tool exists
    pub fn has_tool(&self, name: &str) -> bool {
        self.handlers.contains_key(name)
    }

    /// Get list of tool names
    pub fn tool_names(&self) -> Vec<String> {
        self.handlers.keys().cloned().collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Tool definition for API
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}
