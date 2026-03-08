use super::handler::ToolHandler;
use crate::sandbox::error::{Result, SandboxError};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

/// Registry of available tool handlers
pub struct ToolRegistry {
    handlers: RwLock<HashMap<String, Arc<dyn ToolHandler>>>,
}

impl ToolRegistry {
    /// Create a new empty tool registry
    pub fn new() -> Self {
        Self {
            handlers: RwLock::new(HashMap::new()),
        }
    }

    /// Register a tool handler
    pub fn register(&self, handler: Arc<dyn ToolHandler>) {
        let mut handlers = self.handlers.write().unwrap();
        handlers.insert(handler.name().to_string(), handler);
    }

    /// Execute a tool by name
    pub async fn execute(&self, name: &str, root: &Path, input: Value) -> Result<String> {
        let handler_clone = {
            let handlers = self.handlers.read().unwrap();
            let handler = handlers.get(name).ok_or_else(|| {
                SandboxError::ToolError(format!(
                    "Unknown tool: '{}'. Available tools: {:?}",
                    name,
                    handlers.keys().collect::<Vec<_>>()
                ))
            })?;
            handler.clone()
        }; // Lock is dropped here

        handler_clone.execute(root, input).await
    }

    /// Get tool definitions for API
    pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let handlers = self.handlers.read().unwrap();
        handlers
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
        let handlers = self.handlers.read().unwrap();
        handlers.contains_key(name)
    }

    /// Get list of tool names
    pub fn tool_names(&self) -> Vec<String> {
        let handlers = self.handlers.read().unwrap();
        handlers.keys().cloned().collect()
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
