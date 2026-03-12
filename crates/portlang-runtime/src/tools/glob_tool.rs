use crate::sandbox::error::{Result, SandboxError};
use crate::tools::handler::ToolHandler;
use async_trait::async_trait;
use glob::glob;
use serde_json::Value;
use std::path::Path;

/// Find files matching a glob pattern
pub fn glob_files(root: &Path, pattern: &str) -> Result<Vec<String>> {
    // Construct full pattern relative to root
    let full_pattern = root.join(pattern);
    let pattern_str = full_pattern
        .to_str()
        .ok_or_else(|| SandboxError::ToolError("Invalid pattern path".to_string()))?;

    // Execute glob
    let paths = glob(pattern_str).map_err(|e| {
        SandboxError::ToolError(format!("Invalid glob pattern '{}': {}", pattern, e))
    })?;

    let mut results = Vec::new();
    let canonical_root = std::fs::canonicalize(root).map_err(SandboxError::Io)?;

    for path in paths {
        let path = path.map_err(|e| SandboxError::ToolError(format!("Glob error: {}", e)))?;

        // Ensure path is within root
        if let Ok(canonical_path) = std::fs::canonicalize(&path) {
            if canonical_path.starts_with(&canonical_root) {
                // Make path relative to root
                if let Ok(relative) = canonical_path.strip_prefix(&canonical_root) {
                    results.push(relative.to_string_lossy().to_string());
                }
            }
        }
    }

    // Sort for consistent ordering
    results.sort();

    Ok(results)
}

/// Glob tool handler
pub struct GlobHandler;

#[async_trait]
impl ToolHandler for GlobHandler {
    async fn execute(&self, root: &Path, input: Value) -> Result<String> {
        let pattern = input
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                SandboxError::ToolError("Missing 'pattern' parameter for glob".to_string())
            })?;

        let files = glob_files(root, pattern)?;
        Ok(serde_json::to_string_pretty(&files)?)
    }

    fn name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Find files matching a glob pattern"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to match files (e.g., '*.txt', 'src/**/*.rs')"
                }
            },
            "required": ["pattern"]
        })
    }
}
