//! MCP server manager for handling multiple MCP server instances

use super::{McpClient, McpToolDefinition};
use crate::sandbox::error::Result;
use portlang_core::McpServer;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Manager for multiple MCP server instances
pub struct McpServerManager {
    /// Map of server name to client instance (shared for use across tool handlers)
    clients: HashMap<String, Arc<RwLock<McpClient>>>,
}

impl McpServerManager {
    /// Create a new MCP server manager
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
        }
    }

    /// Initialize all MCP servers in parallel
    ///
    /// # Arguments
    /// * `servers` - List of MCP server configurations
    /// * `working_dir` - Working directory for server processes (typically the field's directory)
    /// * `container_id` - Optional container ID to run servers inside a container
    pub async fn initialize_servers(
        &mut self,
        servers: &[McpServer],
        working_dir: Option<std::path::PathBuf>,
        container_id: Option<String>,
    ) -> Result<()> {
        if servers.is_empty() {
            return Ok(());
        }

        tracing::info!("Initializing {} MCP server(s)", servers.len());

        // Spawn all servers in parallel
        let mut tasks = Vec::new();
        for server in servers {
            let server_clone = server.clone();
            let working_dir_clone = working_dir.clone();
            let container_id_clone = container_id.clone();
            let task = tokio::spawn(async move {
                McpClient::connect(
                    server_clone.name.clone(),
                    &server_clone.transport,
                    working_dir_clone.as_deref(),
                    container_id_clone.as_deref(),
                )
                .await
            });
            tasks.push((server.name.clone(), task));
        }

        // Wait for all servers to initialize
        for (name, task) in tasks {
            match task.await {
                Ok(Ok(client)) => {
                    tracing::info!("MCP server '{}' ready", name);
                    self.clients.insert(name, Arc::new(RwLock::new(client)));
                }
                Ok(Err(e)) => {
                    tracing::error!("Failed to initialize MCP server '{}': {}", name, e);
                    return Err(e);
                }
                Err(e) => {
                    tracing::error!("Task error initializing MCP server '{}': {}", name, e);
                    return Err(crate::sandbox::error::SandboxError::McpServerStartupError(
                        format!("Task join error: {}", e),
                    ));
                }
            }
        }

        Ok(())
    }

    /// Discover all tools from all initialized MCP servers
    ///
    /// Returns a list of (server_name, tool_definition) tuples
    pub async fn discover_tools(&self) -> Result<Vec<(String, McpToolDefinition)>> {
        let mut all_tools = Vec::new();

        for (server_name, client) in &self.clients {
            let client_lock = client.read().await;
            let tools = client_lock.list_tools().await?;
            for tool in tools {
                all_tools.push((server_name.clone(), tool));
            }
        }

        tracing::info!(
            "Discovered {} total tools from {} MCP server(s)",
            all_tools.len(),
            self.clients.len()
        );

        Ok(all_tools)
    }

    /// Get a shared client reference by server name
    pub fn get_client(&self, server_name: &str) -> Option<Arc<RwLock<McpClient>>> {
        self.clients.get(server_name).cloned()
    }

    /// Shutdown all MCP servers
    pub async fn shutdown_all(&mut self) -> Result<()> {
        tracing::info!("Shutting down {} MCP server(s)", self.clients.len());

        for (name, client) in self.clients.drain() {
            let mut client_lock = client.write().await;
            if let Err(e) = client_lock.shutdown().await {
                tracing::warn!("Error shutting down MCP server '{}': {}", name, e);
            }
        }

        Ok(())
    }
}

impl Default for McpServerManager {
    fn default() -> Self {
        Self::new()
    }
}
