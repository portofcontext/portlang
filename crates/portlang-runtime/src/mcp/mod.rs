//! MCP (Model Context Protocol) integration
//!
//! This module provides support for connecting to MCP servers and exposing
//! their tools to the agent runtime.

pub mod client;
pub mod manager;
pub mod tool_handler;

pub use client::McpClient;
pub use manager::McpServerManager;
pub use tool_handler::McpToolHandler;

/// MCP tool definition discovered from server
#[derive(Debug, Clone)]
pub struct McpToolDefinition {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
}
