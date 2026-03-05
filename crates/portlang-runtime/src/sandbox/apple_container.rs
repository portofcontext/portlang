use super::error::{BoundaryViolation, Result, SandboxError};
use super::traits::{CommandOutput, Sandbox};
use crate::tools::ToolRegistry;
use async_trait::async_trait;
use portlang_core::{Action, Boundary};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use uuid::Uuid;

pub struct AppleContainerSandbox {
    container_id: String,
    #[allow(dead_code)]
    host_workspace: PathBuf,
    boundary: Boundary,
    registry: Arc<ToolRegistry>,
}

impl AppleContainerSandbox {
    pub async fn new(
        host_root: PathBuf,
        boundary: Boundary,
        registry: Arc<ToolRegistry>,
    ) -> Result<Self> {
        // Create workspace on host
        if !host_root.exists() {
            std::fs::create_dir_all(&host_root)
                .map_err(|e| SandboxError::InitError(format!("Create workspace failed: {}", e)))?;
        }

        // Generate unique container name
        let container_name = format!("portlang-{}", Uuid::new_v4());

        // Apple's container CLI (similar to docker but native)
        let output = Command::new("container")
            .args([
                "run",
                "-d",
                "--name",
                &container_name,
                "--workdir",
                "/workspace",
                "--volume",
                &format!("{}:/workspace", host_root.display()),
                "--network",
                "none",
                "python:3.11-alpine", // OCI-compatible image
                "sleep",
                "infinity",
            ])
            .output()
            .map_err(|e| SandboxError::InitError(format!("Container start failed: {}", e)))?;

        if !output.status.success() {
            return Err(SandboxError::InitError(format!(
                "Failed to start container: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        tracing::info!(
            "Started Apple container {} ({})",
            container_name,
            container_id
        );

        Ok(Self {
            container_id,
            host_workspace: host_root,
            boundary,
            registry,
        })
    }

    /// Get the tool registry
    pub fn registry(&self) -> &ToolRegistry {
        &self.registry
    }

    fn is_write_allowed(&self, path: &str) -> bool {
        if self.boundary.allow_write.is_empty() {
            return false;
        }
        for pattern in &self.boundary.allow_write {
            if let Ok(glob_pattern) = glob::Pattern::new(pattern) {
                if glob_pattern.matches(path) {
                    return true;
                }
            }
        }
        false
    }

    async fn exec(&self, cmd: &str) -> Result<CommandOutput> {
        let output = Command::new("container")
            .args(["exec", &self.container_id, "sh", "-c", cmd])
            .output()
            .map_err(|e| SandboxError::CommandError(format!("Container exec failed: {}", e)))?;

        Ok(CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
            success: output.status.success(),
        })
    }
}

#[async_trait]
impl Sandbox for AppleContainerSandbox {
    async fn dispatch(&self, action: &Action) -> Result<String> {
        match action {
            Action::ToolCall { tool, input } => {
                // Note: For built-in tools, we use container-specific implementations
                // Custom tools from registry could be supported here in the future
                match tool.as_str() {
                    "read" => {
                        let path = input
                            .get("path")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| SandboxError::ToolError("Missing path".into()))?;

                        let cmd = format!("cat {}", shell_escape::escape(path.into()));
                        let output = self.exec(&cmd).await?;

                        if output.success {
                            Ok(output.stdout)
                        } else {
                            Err(SandboxError::ToolError(format!(
                                "Read failed: {}",
                                output.stderr
                            )))
                        }
                    }

                    "write" => {
                        let path = input
                            .get("path")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| SandboxError::ToolError("Missing path".into()))?;
                        let content = input
                            .get("content")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| SandboxError::ToolError("Missing content".into()))?;

                        // Create parent directories
                        if let Some(parent) = Path::new(path).parent() {
                            if parent != Path::new("") {
                                let mkdir_cmd = format!(
                                    "mkdir -p {}",
                                    shell_escape::escape(parent.to_str().unwrap().into())
                                );
                                self.exec(&mkdir_cmd).await?;
                            }
                        }

                        // Write file using heredoc
                        let cmd = format!(
                            "cat > {} << 'PORTLANG_EOF'\n{}\nPORTLANG_EOF",
                            shell_escape::escape(path.into()),
                            content
                        );

                        let output = self.exec(&cmd).await?;
                        if output.success {
                            Ok(format!("Wrote {} bytes to {}", content.len(), path))
                        } else {
                            Err(SandboxError::ToolError(format!(
                                "Write failed: {}",
                                output.stderr
                            )))
                        }
                    }

                    "glob" => {
                        let pattern = input
                            .get("pattern")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| SandboxError::ToolError("Missing pattern".into()))?;

                        let cmd = format!("find . -path '{}' -type f 2>/dev/null | sort", pattern);
                        let output = self.exec(&cmd).await?;

                        let files: Vec<String> = output
                            .stdout
                            .lines()
                            .map(|s| s.trim_start_matches("./").to_string())
                            .filter(|s| !s.is_empty())
                            .collect();

                        Ok(serde_json::to_string_pretty(&files)?)
                    }
                    _ => Err(SandboxError::ToolError(format!(
                        "Tool '{}' not supported in container sandbox",
                        tool
                    ))),
                }
            }
            Action::TextOutput { text } => Ok(format!("Agent output: {}", text)),
            Action::Stop => Ok("Agent stopped".to_string()),
        }
    }

    async fn check_boundary(&self, action: &Action) -> std::result::Result<(), BoundaryViolation> {
        if let Action::ToolCall { tool, input } = action {
            if tool.as_str() == "write" {
                let path = input
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| BoundaryViolation::new("Missing path"))?;

                if !self.is_write_allowed(path) {
                    return Err(BoundaryViolation::new(format!(
                        "Write to '{}' not allowed. Allowed patterns: {:?}",
                        path, self.boundary.allow_write
                    )));
                }
            }
        }
        Ok(())
    }

    async fn run_command(&self, cmd: &str) -> Result<CommandOutput> {
        self.exec(cmd).await
    }

    fn root(&self) -> &Path {
        Path::new("/workspace") // Container always sees /workspace
    }
}

impl Drop for AppleContainerSandbox {
    fn drop(&mut self) {
        let _ = Command::new("container")
            .args(["stop", &self.container_id])
            .output();
        tracing::info!("Stopped Apple container {}", self.container_id);
    }
}
