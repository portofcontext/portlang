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

pub mod patch;
pub use patch::{apply_patches, load_patch_map};

/// MCP tool definition discovered from server
#[derive(Debug, Clone)]
pub struct McpToolDefinition {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
    /// Output schema — not reported by most MCP servers; injected via patch files
    pub output_schema: Option<serde_json::Value>,
}
