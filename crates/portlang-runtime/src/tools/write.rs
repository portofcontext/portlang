use crate::sandbox::error::{Result, SandboxError};
use crate::tools::handler::ToolHandler;
use async_trait::async_trait;
use serde_json::Value;
use std::fs;
use std::path::Path;

/// Write content to a file
pub fn write_file(root: &Path, file_path: &str, content: &str) -> Result<()> {
    let full_path = root.join(file_path);

    // Ensure parent directory exists
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            SandboxError::ToolError(format!(
                "Failed to create parent directory for '{}': {}",
                file_path, e
            ))
        })?;
    }

    // Ensure path doesn't escape root (check parent since file may not exist)
    let canonical_root = fs::canonicalize(root).map_err(SandboxError::Io)?;

    let parent = full_path
        .parent()
        .ok_or_else(|| SandboxError::PathEscape(format!("Invalid path: {}", file_path)))?;

    let canonical_parent = fs::canonicalize(parent).map_err(SandboxError::Io)?;

    if !canonical_parent.starts_with(&canonical_root) {
        return Err(SandboxError::PathEscape(format!(
            "Path escapes sandbox root: {}",
            file_path
        )));
    }

    // Write file
    fs::write(&full_path, content).map_err(|e| {
        SandboxError::ToolError(format!("Failed to write file '{}': {}", file_path, e))
    })?;

    Ok(())
}

/// Write tool handler
pub struct WriteHandler;

#[async_trait]
impl ToolHandler for WriteHandler {
    async fn execute(&self, root: &Path, input: Value) -> Result<String> {
        let path = input.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
            SandboxError::ToolError("Missing 'path' parameter for write".to_string())
        })?;

        let content = input
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                SandboxError::ToolError("Missing 'content' parameter for write".to_string())
            })?;

        write_file(root, path, content)?;
        Ok(format!("Wrote {} bytes to {}", content.len(), path))
    }

    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Write content to a file"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            },
            "required": ["path", "content"]
        })
    }
}
