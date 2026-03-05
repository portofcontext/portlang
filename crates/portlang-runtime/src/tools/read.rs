use crate::sandbox::error::{Result, SandboxError};
use crate::tools::handler::ToolHandler;
use async_trait::async_trait;
use serde_json::Value;
use std::fs;
use std::path::Path;

/// Read a file from the filesystem
pub fn read_file(root: &Path, file_path: &str) -> Result<String> {
    let full_path = root.join(file_path);

    // Ensure path doesn't escape root
    let canonical_root = fs::canonicalize(root).map_err(|e| SandboxError::Io(e))?;

    let canonical_path = match full_path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            // If file doesn't exist, canonicalize the parent and append filename
            let parent = full_path
                .parent()
                .ok_or_else(|| SandboxError::PathEscape(format!("Invalid path: {}", file_path)))?;

            let canonical_parent = fs::canonicalize(parent).map_err(|e| SandboxError::Io(e))?;

            let filename = full_path
                .file_name()
                .ok_or_else(|| SandboxError::PathEscape(format!("Invalid path: {}", file_path)))?;

            canonical_parent.join(filename)
        }
    };

    if !canonical_path.starts_with(&canonical_root) {
        return Err(SandboxError::PathEscape(format!(
            "Path escapes sandbox root: {}",
            file_path
        )));
    }

    // Read file
    let content = fs::read_to_string(&canonical_path).map_err(|e| {
        SandboxError::ToolError(format!("Failed to read file '{}': {}", file_path, e))
    })?;

    Ok(content)
}

/// Read tool handler
pub struct ReadHandler;

#[async_trait]
impl ToolHandler for ReadHandler {
    async fn execute(&self, root: &Path, input: Value) -> Result<String> {
        let path = input.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
            SandboxError::ToolError("Missing 'path' parameter for read".to_string())
        })?;

        read_file(root, path)
    }

    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Read the contents of a file"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read"
                }
            },
            "required": ["path"]
        })
    }
}
