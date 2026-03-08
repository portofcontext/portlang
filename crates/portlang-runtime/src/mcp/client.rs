//! MCP client implementation

use super::McpToolDefinition;
use crate::sandbox::error::{Result, SandboxError};
use portlang_core::McpTransport;
use rmcp::{
    model::CallToolRequestParams,
    service::RunningService,
    transport::{
        streamable_http_client::StreamableHttpClientTransportConfig, ConfigureCommandExt,
        StreamableHttpClientTransport, TokioChildProcess,
    },
    RoleClient, ServiceExt,
};
use serde_json::Value;
use tokio::process::Command;

/// MCP client for communicating with an MCP server
pub struct McpClient {
    /// Server name for identification
    server_name: String,
    /// Running service instance (wrapped in Option to allow taking ownership for shutdown)
    service: Option<RunningService<RoleClient, ()>>,
}

impl McpClient {
    /// Create and connect to a new MCP server
    ///
    /// # Arguments
    /// * `server_name` - Name identifier for this server
    /// * `transport` - Transport configuration (Stdio or SSE)
    /// * `working_dir` - Working directory for the server process (stdio only)
    /// * `container_id` - Optional container ID to run stdio servers inside a container
    pub async fn connect(
        server_name: String,
        transport: &McpTransport,
        working_dir: Option<&std::path::Path>,
        container_id: Option<&str>,
    ) -> Result<Self> {
        match transport {
            McpTransport::Stdio { command, args, env } => {
                tracing::debug!(
                    "Starting MCP server '{}' via stdio: {} {:?} (cwd: {:?}, container: {:?})",
                    server_name,
                    command,
                    args,
                    working_dir,
                    container_id
                );

                // Create and configure the command
                let cmd = if let Some(container) = container_id {
                    // Run inside container via `container exec`
                    let mut base_cmd = Command::new("container");
                    base_cmd.arg("exec");
                    base_cmd.arg("-i"); // Keep stdin open for interactive communication

                    // Set environment variables via -e flags
                    for (key, value) in env {
                        let expanded_value = shellexpand::env(value)
                            .map_err(|e| {
                                tracing::error!("Failed to expand env var {}: {}", key, e);
                                e
                            })
                            .unwrap_or_else(|_| std::borrow::Cow::Borrowed(value));
                        base_cmd
                            .arg("-e")
                            .arg(format!("{}={}", key, expanded_value));
                    }

                    base_cmd.arg(container);
                    base_cmd.arg(command);
                    base_cmd.args(args);

                    // Explicitly configure stdio for proper pipe handling
                    base_cmd.stdin(std::process::Stdio::piped());
                    base_cmd.stdout(std::process::Stdio::piped());
                    base_cmd.stderr(std::process::Stdio::piped());

                    base_cmd
                } else {
                    // Run directly on host
                    Command::new(command).configure(move |c| {
                        c.args(args);

                        // Set working directory if provided
                        if let Some(dir) = working_dir {
                            c.current_dir(dir);
                        }

                        // Set environment variables
                        for (key, value) in env {
                            let expanded_value = shellexpand::env(value)
                                .map_err(|e| {
                                    tracing::error!("Failed to expand env var {}: {}", key, e);
                                    e
                                })
                                .unwrap_or_else(|_| std::borrow::Cow::Borrowed(value));
                            c.env(key, expanded_value.as_ref());
                        }
                    })
                };

                // Create stdio transport
                let transport = TokioChildProcess::new(cmd).map_err(|e| {
                    SandboxError::McpServerStartupError(format!(
                        "Failed to create stdio transport for '{}': {}",
                        server_name, e
                    ))
                })?;

                // Initialize service with timeout
                let service =
                    tokio::time::timeout(std::time::Duration::from_secs(30), ().serve(transport))
                        .await
                        .map_err(|_| {
                            SandboxError::McpServerStartupError(format!(
                                "MCP server '{}' initialization timed out after 30s",
                                server_name
                            ))
                        })?
                        .map_err(|e| {
                            SandboxError::McpServerStartupError(format!(
                                "MCP server '{}' initialization failed: {}",
                                server_name, e
                            ))
                        })?;

                if let Some(server_info) = service.peer_info() {
                    tracing::info!(
                        "MCP server '{}' initialized: {} v{}",
                        server_name,
                        server_info.server_info.name,
                        server_info.server_info.version
                    );
                } else {
                    tracing::info!("MCP server '{}' initialized (no server info)", server_name);
                }

                Ok(Self {
                    server_name,
                    service: Some(service),
                })
            }
            McpTransport::Sse { url, headers } => {
                tracing::info!(
                    "Connecting to remote MCP server '{}' at {}",
                    server_name,
                    url
                );

                // Extract Authorization header if present
                let mut auth_value = None;
                let mut other_headers = reqwest::header::HeaderMap::new();

                for (key, value) in headers {
                    if key.eq_ignore_ascii_case("authorization") {
                        // Extract the Bearer token (remove "Bearer " prefix if present)
                        let token = if value.starts_with("Bearer ") {
                            value.strip_prefix("Bearer ").unwrap_or(value)
                        } else {
                            value
                        };
                        auth_value = Some(token.to_string());
                    } else {
                        // Collect other headers for the reqwest client
                        let header_name = reqwest::header::HeaderName::from_bytes(key.as_bytes())
                            .map_err(|e| {
                            SandboxError::McpServerStartupError(format!(
                                "Invalid header name '{}': {}",
                                key, e
                            ))
                        })?;
                        let header_value =
                            reqwest::header::HeaderValue::from_str(value).map_err(|e| {
                                SandboxError::McpServerStartupError(format!(
                                    "Invalid header value for '{}': {}",
                                    key, e
                                ))
                            })?;
                        other_headers.insert(header_name, header_value);
                    }
                }

                // Create HTTP client with non-auth headers
                let client = reqwest::Client::builder()
                    .default_headers(other_headers)
                    .build()
                    .map_err(|e| {
                        SandboxError::McpServerStartupError(format!(
                            "Failed to create HTTP client: {}",
                            e
                        ))
                    })?;

                // Create transport config with auth header
                let mut config = StreamableHttpClientTransportConfig::with_uri(url.clone());
                if let Some(auth) = auth_value {
                    config = config.auth_header(auth);
                }

                // Create SSE transport
                let transport = StreamableHttpClientTransport::with_client(client, config);

                // Initialize service with timeout
                let service =
                    tokio::time::timeout(std::time::Duration::from_secs(30), ().serve(transport))
                        .await
                        .map_err(|_| {
                            SandboxError::McpServerStartupError(format!(
                                "MCP server '{}' initialization timed out after 30s",
                                server_name
                            ))
                        })?
                        .map_err(|e| {
                            SandboxError::McpServerStartupError(format!(
                                "MCP server '{}' initialization failed: {}",
                                server_name, e
                            ))
                        })?;

                if let Some(server_info) = service.peer_info() {
                    tracing::info!(
                        "Remote MCP server '{}' connected: {} v{}",
                        server_name,
                        server_info.server_info.name,
                        server_info.server_info.version
                    );
                } else {
                    tracing::info!(
                        "Remote MCP server '{}' connected (no server info)",
                        server_name
                    );
                }

                Ok(Self {
                    server_name,
                    service: Some(service),
                })
            }
        }
    }

    /// List all available tools from the MCP server
    pub async fn list_tools(&self) -> Result<Vec<McpToolDefinition>> {
        let service = self
            .service
            .as_ref()
            .ok_or_else(|| SandboxError::McpServerUnreachable(self.server_name.clone()))?;

        let tools_result = service.list_tools(Default::default()).await.map_err(|e| {
            SandboxError::McpToolError(format!(
                "Failed to list tools from MCP server '{}': {}",
                self.server_name, e
            ))
        })?;

        let tool_definitions = tools_result
            .tools
            .into_iter()
            .map(|tool| McpToolDefinition {
                name: tool.name.to_string(),
                description: tool.description.map(|d| d.to_string()),
                // input_schema is already a JsonObject (Map<String, Value>), convert to Value
                input_schema: serde_json::Value::Object(tool.input_schema.as_ref().clone()),
            })
            .collect::<Vec<_>>();

        tracing::debug!(
            "Discovered {} tools from MCP server '{}'",
            tool_definitions.len(),
            self.server_name
        );

        Ok(tool_definitions)
    }

    /// Call a tool on the MCP server
    ///
    /// # Arguments
    /// * `tool_name` - Name of the tool to call
    /// * `arguments` - JSON arguments to pass to the tool
    pub async fn call_tool(&self, tool_name: &str, arguments: Value) -> Result<Value> {
        tracing::debug!(
            "Calling MCP tool '{}' on server '{}'",
            tool_name,
            self.server_name
        );

        let service = self
            .service
            .as_ref()
            .ok_or_else(|| SandboxError::McpServerUnreachable(self.server_name.clone()))?;

        // Convert arguments to object if not already
        let args_obj = arguments.as_object().cloned();

        // Build the request params
        let params = CallToolRequestParams {
            meta: None,
            name: std::borrow::Cow::Owned(tool_name.to_string()),
            arguments: args_obj,
            task: None,
        };

        // Execute tool with timeout
        let call_result = tokio::time::timeout(
            std::time::Duration::from_secs(120),
            service.call_tool(params),
        )
        .await
        .map_err(|_| {
            SandboxError::McpToolError(format!(
                "MCP tool '{}' execution timed out after 120s",
                tool_name
            ))
        })?
        .map_err(|e| {
            SandboxError::McpToolError(format!("MCP tool '{}' execution failed: {}", tool_name, e))
        })?;

        // Convert result to JSON value
        let result_json =
            serde_json::to_value(&call_result).map_err(SandboxError::Serialization)?;

        Ok(result_json)
    }

    /// Shutdown the MCP server gracefully
    pub async fn shutdown(&mut self) -> Result<()> {
        tracing::debug!("Shutting down MCP server '{}'", self.server_name);

        // Take ownership of the service and cancel it
        if let Some(service) = self.service.take() {
            if let Err(e) = service.cancel().await {
                tracing::warn!("Error cancelling MCP server '{}': {}", self.server_name, e);
            }
        }

        tracing::debug!("MCP server '{}' shut down", self.server_name);
        Ok(())
    }
}
