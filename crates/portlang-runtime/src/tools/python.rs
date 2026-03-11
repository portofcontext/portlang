use crate::sandbox::error::{Result, SandboxError};
use crate::tools::handler::ToolHandler;
use async_trait::async_trait;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;

/// Configuration for a Python tool
#[derive(Debug, Clone)]
pub enum PythonToolConfig {
    /// PEP 723 inline dependencies - uv handles everything
    Inline,

    /// Workspace mode - shared dependencies (future)
    #[allow(dead_code)]
    Workspace { workspace_root: PathBuf },

    /// Tool-specific project (future)
    #[allow(dead_code)]
    Project { project_path: PathBuf },
}

/// Python tool handler using uv
pub struct PythonToolHandler {
    name: String,
    description: String,
    script_path: PathBuf,
    function_name: String,
    input_schema: Value,
    config: PythonToolConfig,
}

impl PythonToolHandler {
    /// Create a new Python tool handler
    pub fn new(
        name: String,
        description: String,
        script_path: PathBuf,
        function_name: Option<String>,
        input_schema: Value,
    ) -> Self {
        Self {
            name,
            description,
            script_path,
            function_name: function_name.unwrap_or_else(|| "execute".to_string()),
            input_schema,
            config: PythonToolConfig::Inline, // Start with inline mode only
        }
    }

    /// Generate the wrapper script that loads and executes the tool
    fn generate_wrapper(&self) -> String {
        format!(
            r#"#!/usr/bin/env python3
import sys
import json
import importlib.util
from pathlib import Path

def main():
    script_path = Path("{script}")
    function_name = "{function}"

    # Read JSON input from stdin
    try:
        input_data = json.load(sys.stdin)
    except json.JSONDecodeError as e:
        error = {{
            "error": f"Failed to parse input JSON: {{e}}",
            "type": "JSONDecodeError",
        }}
        print(json.dumps(error), file=sys.stderr)
        sys.exit(1)

    # Import the tool module
    try:
        spec = importlib.util.spec_from_file_location("tool_module", script_path)
        if spec is None or spec.loader is None:
            raise ImportError(f"Could not load module from {{script_path}}")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
    except Exception as e:
        error = {{
            "error": f"Failed to import tool module: {{e}}",
            "type": type(e).__name__,
        }}
        print(json.dumps(error), file=sys.stderr)
        sys.exit(1)

    # Get the function
    if not hasattr(module, function_name):
        error = {{
            "error": f"Module does not have function '{{function_name}}'",
            "type": "AttributeError",
            "available": [name for name in dir(module) if not name.startswith("_")],
        }}
        print(json.dumps(error), file=sys.stderr)
        sys.exit(1)

    tool_fn = getattr(module, function_name)

    # Execute the tool function
    try:
        # Unpack input_data as keyword arguments to support typed parameters
        result = tool_fn(**input_data)

        # Ensure result is JSON-serializable
        try:
            json_result = json.dumps(result)
            print(json_result)
        except (TypeError, ValueError) as e:
            error = {{
                "error": f"Tool result is not JSON-serializable: {{e}}",
                "type": "SerializationError",
            }}
            print(json.dumps(error), file=sys.stderr)
            sys.exit(1)

    except Exception as e:
        error = {{
            "error": str(e),
            "type": type(e).__name__,
        }}

        # Include traceback for debugging
        import traceback
        error["traceback"] = traceback.format_exc()

        print(json.dumps(error), file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()
"#,
            script = self.script_path.display(),
            function = self.function_name
        )
    }

    /// Execute the Python tool in inline mode
    async fn execute_inline(&self, root: &Path, input: Value) -> Result<String> {
        // Generate wrapper script
        let wrapper = self.generate_wrapper();

        // Write wrapper to temp file
        let temp_dir = std::env::temp_dir();
        let wrapper_path = temp_dir.join(format!("portlang_python_wrapper_{}.py", self.name));
        std::fs::write(&wrapper_path, wrapper).map_err(|e| {
            SandboxError::ToolError(format!("Failed to write wrapper script: {}", e))
        })?;

        // Execute with uv run
        let mut cmd = tokio::process::Command::new("uv");
        cmd.args(["run", "--quiet"])
            .arg(&wrapper_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(root);

        let mut child = cmd.spawn().map_err(|e| {
            SandboxError::CommandError(format!("Failed to spawn uv process: {}", e))
        })?;

        // Send input as JSON to stdin
        if let Some(mut stdin) = child.stdin.take() {
            let input_json = serde_json::to_string(&input)?;
            stdin.write_all(input_json.as_bytes()).await.map_err(|e| {
                SandboxError::CommandError(format!("Failed to write to stdin: {}", e))
            })?;
            stdin
                .shutdown()
                .await
                .map_err(|e| SandboxError::CommandError(format!("Failed to close stdin: {}", e)))?;
        }

        // Wait for completion and collect output
        let output = child.wait_with_output().await.map_err(|e| {
            SandboxError::CommandError(format!("Failed to wait for process: {}", e))
        })?;

        // Clean up wrapper file
        let _ = std::fs::remove_file(&wrapper_path);

        // Check if execution was successful
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SandboxError::ToolError(format!(
                "Python tool '{}' failed:\n{}",
                self.name, stderr
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

#[async_trait]
impl ToolHandler for PythonToolHandler {
    async fn execute(&self, root: &Path, input: Value) -> Result<String> {
        match &self.config {
            PythonToolConfig::Inline => self.execute_inline(root, input).await,
            _ => Err(SandboxError::ToolError(
                "Only inline mode is currently supported".to_string(),
            )),
        }
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.input_schema.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_wrapper_generation() {
        let handler = PythonToolHandler::new(
            "test_tool".to_string(),
            "Test tool".to_string(),
            PathBuf::from("./tools/test.py"),
            None,
            json!({}),
        );

        let wrapper = handler.generate_wrapper();
        assert!(wrapper.contains("./tools/test.py"));
        assert!(wrapper.contains("execute"));
        assert!(wrapper.contains("json.load(sys.stdin)"));
    }

    #[test]
    fn test_custom_function_name() {
        let handler = PythonToolHandler::new(
            "test_tool".to_string(),
            "Test tool".to_string(),
            PathBuf::from("./tools/test.py"),
            Some("custom_execute".to_string()),
            json!({}),
        );

        let wrapper = handler.generate_wrapper();
        assert!(wrapper.contains("custom_execute"));
    }
}
