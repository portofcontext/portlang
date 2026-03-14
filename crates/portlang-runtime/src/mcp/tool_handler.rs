//! MCP tool handler that bridges MCP tools to the ToolHandler trait

use super::{McpClient, McpToolDefinition};
use crate::sandbox::error::Result;
use crate::tools::handler::ToolHandler;
use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Tool handler for MCP tools
///
/// Each instance represents one tool from an MCP server, making it appear
/// as a native tool in the agent's tool registry.
pub struct McpToolHandler {
    server_name: String,
    tool_name: String,
    description: String,
    input_schema: Value,
    /// Output schema injected via patch files (stored for future ToolHandler trait extension)
    #[allow(dead_code)]
    output_schema: Option<Value>,
    client: Arc<RwLock<McpClient>>,
}

impl McpToolHandler {
    /// Create a new MCP tool handler
    pub fn new(
        server_name: String,
        tool_def: McpToolDefinition,
        client: Arc<RwLock<McpClient>>,
    ) -> Self {
        Self {
            server_name,
            tool_name: tool_def.name,
            description: tool_def.description.unwrap_or_default(),
            input_schema: tool_def.input_schema,
            output_schema: tool_def.output_schema,
            client,
        }
    }
}

#[async_trait]
impl ToolHandler for McpToolHandler {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.input_schema.clone()
    }

    async fn execute(&self, _root: &Path, input: Value) -> Result<String> {
        tracing::debug!(
            "Executing MCP tool '{}' from server '{}'",
            self.tool_name,
            self.server_name
        );

        // Get read lock on client
        let client = self.client.read().await;

        // Call the tool
        let result = client.call_tool(&self.tool_name, input).await?;

        // Convert MCP result to string
        // MCP tools return a CallToolResult with a `content` field that contains an array of content items
        // Each content item can be text, image, resource, etc.
        // For now, we'll extract text content and format it as a string
        if let Some(content) = result.get("content") {
            if let Some(arr) = content.as_array() {
                let mut text_parts = Vec::new();
                for item in arr {
                    if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                        text_parts.push(text.to_string());
                    } else if let Some(item_type) = item.get("type").and_then(|v| v.as_str()) {
                        // For non-text content, include a placeholder
                        text_parts.push(format!("[{} content]", item_type));
                    }
                }
                if !text_parts.is_empty() {
                    return Ok(text_parts.join("\n"));
                }
            }
        }

        // Fallback: return the entire result as JSON
        Ok(serde_json::to_string_pretty(&result)?)
    }
}
