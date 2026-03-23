use super::error::{BoundaryViolation, Result, SandboxError};
use super::traits::{CommandOutput, Sandbox, ScriptExecHandle, ScriptHandle};
use crate::tools::ToolRegistry;
use async_trait::async_trait;
use portlang_core::{Action, Boundary};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

/// Wraps a `tokio::process::Child` to implement [`ScriptExecHandle`].
struct ChildHandle(tokio::process::Child);

#[async_trait]
impl ScriptExecHandle for ChildHandle {
    async fn kill(&mut self) -> std::io::Result<()> {
        self.0.kill().await
    }
    async fn wait(&mut self) -> std::io::Result<Option<i32>> {
        self.0.wait().await.map(|s| s.code())
    }
}

/// Dispatch sandbox - executes actions on the actual filesystem with boundary checks
pub struct DispatchSandbox {
    root: PathBuf,
    boundary: Boundary,
    registry: Arc<ToolRegistry>,
}

impl DispatchSandbox {
    /// Create a new dispatch sandbox
    pub fn new(root: PathBuf, boundary: Boundary, registry: Arc<ToolRegistry>) -> Self {
        Self {
            root,
            boundary,
            registry,
        }
    }

    /// Check if a write path is allowed by boundary patterns
    fn is_write_allowed(&self, path: &str) -> bool {
        if self.boundary.allow_write.is_empty() {
            // If no patterns specified, deny all writes
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

    /// Get the tool registry
    pub fn registry(&self) -> &ToolRegistry {
        &self.registry
    }
}

#[async_trait]
impl Sandbox for DispatchSandbox {
    fn backend_name(&self) -> &str {
        "dispatch"
    }

    async fn dispatch(&self, action: &Action) -> Result<String> {
        match action {
            Action::ToolCall { tool, input } => {
                // Use registry to execute tool
                self.registry
                    .execute(tool.as_str(), &self.root, input.clone())
                    .await
            }
            Action::TextOutput { text } => Ok(format!("Agent output: {}", text)),
            Action::Stop => Ok("Agent stopped".to_string()),
        }
    }

    async fn check_boundary(&self, action: &Action) -> std::result::Result<(), BoundaryViolation> {
        match action {
            Action::ToolCall { tool, input } => {
                // Only check write boundary for write tool
                if tool.as_str() == "write" {
                    let path = input.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                        BoundaryViolation::new("Missing 'path' parameter for write")
                    })?;

                    if !self.is_write_allowed(path) {
                        return Err(BoundaryViolation::new(format!(
                            "Write to '{}' is not allowed by boundary policy. Allowed patterns: {:?}",
                            path, self.boundary.allow_write
                        )));
                    }
                }
                // All other tools are allowed (within root)
            }
            Action::TextOutput { .. } | Action::Stop => {
                // Text output and stop are always allowed
            }
        }

        Ok(())
    }

    async fn run_command(&self, cmd: &str) -> Result<CommandOutput> {
        let output = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(&self.root)
            .output()
            .map_err(|e| SandboxError::CommandError(format!("Failed to execute command: {}", e)))?;

        Ok(CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
            success: output.status.success(),
        })
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn container_id(&self) -> Option<&str> {
        None // DispatchSandbox doesn't use containers
    }

    async fn exec_script_streaming(&self, script_content: &str) -> Result<ScriptHandle> {
        let script_path = self.root.join(".portlang_cc_runner.sh");
        tokio::fs::write(&script_path, script_content)
            .await
            .map_err(|e| {
                SandboxError::CommandError(format!("Failed to write runner script: {e}"))
            })?;

        let mut child = tokio::process::Command::new("sh")
            .arg(&script_path)
            .current_dir(&self.root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| SandboxError::CommandError(format!("Failed to spawn script: {e}")))?;

        let stdout = Box::new(child.stdout.take().expect("stdout piped"));
        let stderr = Box::new(child.stderr.take().expect("stderr piped"));

        Ok(ScriptHandle {
            stdout,
            stderr,
            exec: Box::new(ChildHandle(child)),
        })
    }
}
