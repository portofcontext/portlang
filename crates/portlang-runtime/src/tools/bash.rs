use crate::sandbox::error::{Result, SandboxError};
use crate::tools::handler::ToolHandler;
use async_trait::async_trait;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;
use walkdir::WalkDir;

/// Check if a relative path matches any of the allow_write glob patterns
fn is_write_allowed(rel_path: &str, allow_write: &[String]) -> bool {
    if allow_write.is_empty() {
        return false;
    }
    for pattern in allow_write {
        if let Ok(p) = glob::Pattern::new(pattern) {
            if p.matches(rel_path) {
                return true;
            }
        }
    }
    false
}

/// Snapshot the workspace: map of absolute path -> modification time
fn snapshot_workspace(root: &Path) -> std::io::Result<Vec<(PathBuf, SystemTime)>> {
    let mut files = vec![];
    for entry in WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        if let Some(mtime) = entry.metadata().ok().and_then(|m| m.modified().ok()) {
            files.push((entry.path().to_path_buf(), mtime));
        }
    }
    Ok(files)
}

/// Built-in bash tool handler for local (non-container) execution.
/// Runs shell commands via `sh -c` in the workspace root and enforces
/// allow_write patterns by reverting any unauthorized file writes post-execution.
pub struct BashHandler {
    allow_write: Vec<String>,
}

impl BashHandler {
    pub fn new(allow_write: Vec<String>) -> Self {
        Self { allow_write }
    }
}

#[async_trait]
impl ToolHandler for BashHandler {
    async fn execute(&self, root: &Path, input: Value) -> Result<String> {
        let command = input
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| SandboxError::ToolError("Missing 'command' parameter".into()))?;

        // Create a marker file to track writes
        let marker_path = root.join(".portlang_bash_marker");
        fs::File::create(&marker_path).map_err(|_| {
            SandboxError::ToolError(format!(
                "Cannot run bash in workspace '{}': directory does not exist",
                root.display()
            ))
        })?;
        let marker_mtime = fs::metadata(&marker_path)
            .and_then(|m| m.modified())
            .map_err(|e| SandboxError::ToolError(format!("Failed to read marker mtime: {}", e)))?;

        // Execute command
        let output = Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(root)
            .output()
            .map_err(|e| SandboxError::CommandError(format!("Failed to execute command: {}", e)))?;

        // Find files modified/created after marker
        let mut violations: Vec<String> = vec![];
        if let Ok(files) = snapshot_workspace(root) {
            for (abs_path, mtime) in files {
                if abs_path == marker_path {
                    continue;
                }
                if mtime >= marker_mtime {
                    let rel = abs_path
                        .strip_prefix(root)
                        .ok()
                        .and_then(|p| p.to_str())
                        .unwrap_or("");
                    if !is_write_allowed(rel, &self.allow_write) {
                        let _ = fs::remove_file(&abs_path);
                        violations.push(rel.to_string());
                    }
                }
            }
        }

        // Cleanup marker
        let _ = fs::remove_file(&marker_path);

        // Format output
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

        let mut result = stdout;
        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str("stderr: ");
            result.push_str(&stderr);
        }
        if exit_code != 0 {
            result.push_str(&format!("\nExit code: {}", exit_code));
        }
        if !violations.is_empty() {
            result.push_str(
                "\n\nBoundary violations — the following files were removed (not in allow_write):\n",
            );
            for v in &violations {
                result.push_str(&format!("  - {}\n", v));
            }
        }
        if result.trim().is_empty() {
            result = "(no output)".to_string();
        }

        Ok(result)
    }

    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Execute a shell command in the workspace. Use this to run programs, make network requests, process data, and perform any computation."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to execute"
                }
            },
            "required": ["command"]
        })
    }
}
