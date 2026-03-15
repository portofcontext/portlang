use crate::sandbox::error::{Result, SandboxError};
use crate::tools::handler::ToolHandler;
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;

/// Python tool handler.
///
/// Executes user-defined Python scripts inside the container. The script is read from
/// the host at startup and embedded into a self-contained wrapper using base64 encoding,
/// so no host paths need to be accessible from within the container.
///
/// Requires `uv` to be available in the container image. When any Python tool is
/// registered, `uv` is automatically added to the container's package list by
/// `prepare_agent_view` in loop_runner.rs.
pub struct PythonToolHandler {
    name: String,
    description: String,
    script_path: PathBuf,
    function_name: String,
    input_schema: Value,
    output_schema: Option<Value>,
    container_id: String,
}

impl PythonToolHandler {
    pub fn new(
        name: String,
        description: String,
        script_path: PathBuf,
        function_name: Option<String>,
        input_schema: Value,
        output_schema: Option<Value>,
        container_id: String,
    ) -> Self {
        Self {
            name,
            description,
            script_path,
            function_name: function_name.unwrap_or_else(|| "execute".to_string()),
            input_schema,
            output_schema,
            container_id,
        }
    }

    /// Extract the PEP 723 inline script metadata block from a script, if present.
    ///
    /// uv reads dependency declarations from the script it is invoked with, not from
    /// scripts that are exec'd inside it. Since the wrapper is what uv runs, any
    /// `# /// script` block in the user's tool file must be forwarded into the wrapper.
    fn extract_pep723_header(script_content: &str) -> String {
        let mut in_block = false;
        let mut lines: Vec<&str> = Vec::new();
        for line in script_content.lines() {
            if line.trim() == "# /// script" {
                in_block = true;
                lines.push(line);
            } else if in_block {
                lines.push(line);
                if line.trim() == "# ///" {
                    break;
                }
            }
        }
        if lines.is_empty() {
            String::new()
        } else {
            lines.join("\n") + "\n"
        }
    }

    /// Generate a self-contained wrapper script that embeds the tool's source via base64.
    ///
    /// Using base64 avoids any escaping issues with arbitrary Python source content.
    /// The wrapper reads JSON from stdin, loads the embedded module, calls the function,
    /// and writes the result as JSON to stdout.
    ///
    /// Any PEP 723 inline script metadata (`# /// script ... # ///`) is forwarded from
    /// the user's script into the wrapper so that `uv run` installs the declared
    /// dependencies before executing.
    fn generate_wrapper(&self, script_content: &str) -> String {
        let encoded = BASE64.encode(script_content.as_bytes());
        let pep723_header = Self::extract_pep723_header(script_content);
        format!(
            r#"#!/usr/bin/env python3
{pep723_header}import sys
import json
import types
import base64

def main():
    script_b64 = "{encoded}"
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

    # Load the tool module from the embedded source
    script_content = base64.b64decode(script_b64).decode("utf-8")
    module = types.ModuleType("tool_module")
    try:
        exec(compile(script_content, "tool_module", "exec"), module.__dict__)
    except Exception as e:
        error = {{
            "error": f"Failed to load tool module: {{e}}",
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
        result = tool_fn(**input_data)

        try:
            # Unwrap Pydantic models (v2: model_dump, v1: dict) before serializing
            if hasattr(result, "model_dump"):
                serializable = result.model_dump()
            elif hasattr(result, "dict") and callable(result.dict):
                serializable = result.dict()
            else:
                serializable = result
            json_result = json.dumps(serializable)
            print(json_result)
        except (TypeError, ValueError) as e:
            error = {{
                "error": f"Tool result is not JSON-serializable: {{e}}",
                "type": "SerializationError",
            }}
            print(json.dumps(error), file=sys.stderr)
            sys.exit(1)

    except Exception as e:
        import traceback
        error = {{
            "error": str(e),
            "type": type(e).__name__,
            "traceback": traceback.format_exc(),
        }}
        print(json.dumps(error), file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()
"#,
            pep723_header = pep723_header,
            encoded = encoded,
            function = self.function_name,
        )
    }

    /// Execute the Python tool inside the container.
    ///
    /// Two container exec calls:
    ///   1. Write the self-contained wrapper to /tmp inside the container.
    ///   2. Run the wrapper with uv, piping JSON input via stdin.
    async fn execute_in_container(&self, input: Value) -> Result<String> {
        // Read the user's script from the host filesystem
        let script_content = std::fs::read_to_string(&self.script_path).map_err(|e| {
            SandboxError::ToolError(format!(
                "Failed to read Python script '{}': {}",
                self.script_path.display(),
                e
            ))
        })?;

        let wrapper = self.generate_wrapper(&script_content);
        let wrapper_path = format!("/tmp/portlang_{}.py", self.name);

        // Step 1: write the self-contained wrapper into the container via stdin
        let mut write_cmd = tokio::process::Command::new("container");
        write_cmd
            .args([
                "exec",
                "-i",
                &self.container_id,
                "sh",
                "-c",
                &format!("cat > {}", wrapper_path),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        let mut write_child = write_cmd.spawn().map_err(|e| {
            SandboxError::CommandError(format!(
                "Failed to write Python wrapper to container: {}",
                e
            ))
        })?;

        if let Some(mut stdin) = write_child.stdin.take() {
            stdin.write_all(wrapper.as_bytes()).await.map_err(|e| {
                SandboxError::CommandError(format!("Failed to pipe wrapper to container: {}", e))
            })?;
            stdin.shutdown().await.map_err(|e| {
                SandboxError::CommandError(format!("Failed to close wrapper pipe: {}", e))
            })?;
        }

        let write_output = write_child.wait_with_output().await.map_err(|e| {
            SandboxError::CommandError(format!("Failed to wait for wrapper write: {}", e))
        })?;

        if !write_output.status.success() {
            return Err(SandboxError::ToolError(format!(
                "Failed to write Python wrapper into container: {}",
                String::from_utf8_lossy(&write_output.stderr)
            )));
        }

        // Step 2: run the wrapper with uv inside the container, /workspace as cwd, JSON on stdin
        let run_cmd_str = format!("cd /workspace && uv run --quiet {}", wrapper_path);
        let mut run_cmd = tokio::process::Command::new("container");
        run_cmd
            .args(["exec", "-i", &self.container_id, "sh", "-c", &run_cmd_str])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut run_child = run_cmd.spawn().map_err(|e| {
            SandboxError::CommandError(format!("Failed to spawn Python tool in container: {}", e))
        })?;

        if let Some(mut stdin) = run_child.stdin.take() {
            let input_json = serde_json::to_string(&input)?;
            stdin.write_all(input_json.as_bytes()).await.map_err(|e| {
                SandboxError::CommandError(format!("Failed to pipe input to Python tool: {}", e))
            })?;
            stdin.shutdown().await.map_err(|e| {
                SandboxError::CommandError(format!("Failed to close input pipe: {}", e))
            })?;
        }

        let output = run_child.wait_with_output().await.map_err(|e| {
            SandboxError::CommandError(format!("Failed to wait for Python tool: {}", e))
        })?;

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
    async fn execute(&self, _root: &Path, input: Value) -> Result<String> {
        self.execute_in_container(input).await
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

    fn output_schema(&self) -> Option<Value> {
        self.output_schema.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_handler() -> PythonToolHandler {
        PythonToolHandler::new(
            "test_tool".to_string(),
            "Test tool".to_string(),
            PathBuf::from("./tools/test.py"),
            None,
            json!({}),
            None,
            "test-container-id".to_string(),
        )
    }

    #[test]
    fn test_output_schema_none_by_default() {
        let handler = make_handler();
        assert_eq!(handler.output_schema(), None);
    }

    #[test]
    fn test_output_schema_stored_and_returned() {
        let schema = json!({"type": "object", "properties": {"result": {"type": "number"}}});
        let handler = PythonToolHandler::new(
            "calc".to_string(),
            "Calculator".to_string(),
            PathBuf::from("./tools/calc.py"),
            None,
            json!({}),
            Some(schema.clone()),
            "container-id".to_string(),
        );
        assert_eq!(handler.output_schema(), Some(schema));
    }

    #[test]
    fn test_pep723_header_forwarded_into_wrapper() {
        let handler = make_handler();
        let script = r#"#!/usr/bin/env python3
# /// script
# dependencies = ["pydantic"]
# ///

from pydantic import BaseModel

def execute(x: int) -> dict:
    return {"x": x}
"#;
        let wrapper = handler.generate_wrapper(script);
        // The wrapper must contain the metadata block so uv installs pydantic
        assert!(
            wrapper.contains("# /// script"),
            "wrapper should forward PEP 723 header"
        );
        assert!(wrapper.contains("pydantic"));
        assert!(wrapper.contains("# ///"));
    }

    #[test]
    fn test_no_pep723_header_is_fine() {
        let handler = make_handler();
        let script = "def execute(**kwargs): return kwargs\n";
        let wrapper = handler.generate_wrapper(script);
        assert!(!wrapper.contains("# /// script"));
    }

    #[test]
    fn test_wrapper_uses_base64_and_reads_stdin() {
        let handler = make_handler();
        let wrapper = handler.generate_wrapper("def execute(**kwargs): return kwargs");
        assert!(wrapper.contains("base64.b64decode"));
        assert!(wrapper.contains("json.load(sys.stdin)"));
        assert!(wrapper.contains("execute"));
    }

    #[test]
    fn test_custom_function_name() {
        let handler = PythonToolHandler::new(
            "test_tool".to_string(),
            "Test tool".to_string(),
            PathBuf::from("./tools/test.py"),
            Some("custom_execute".to_string()),
            json!({}),
            None,
            "test-container-id".to_string(),
        );
        let wrapper = handler.generate_wrapper("def custom_execute(**kwargs): return kwargs");
        assert!(wrapper.contains("custom_execute"));
    }

    #[test]
    fn test_script_with_special_chars_encodes_safely() {
        let handler = make_handler();
        // Script with quotes and backslashes that would break naive string embedding
        let tricky = r#"def execute(**kwargs):
    return {"result": "it's a \"test\"", "path": "C:\\Users\\test"}
"#;
        let wrapper = handler.generate_wrapper(tricky);
        // Verify the base64 round-trip is correct
        let b64_marker = "script_b64 = \"";
        let start = wrapper.find(b64_marker).unwrap() + b64_marker.len();
        let end = wrapper[start..].find('"').unwrap() + start;
        let decoded = BASE64.decode(&wrapper[start..end]).unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), tricky);
    }
}
