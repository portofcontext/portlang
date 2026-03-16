use crate::sandbox::error::{Result, SandboxError};
use crate::tools::handler::ToolHandler;
use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;
use std::process::Command;

/// Tool that executes a shell command template inside the container.
pub struct ShellCommandHandler {
    name: String,
    description: String,
    command_template: String,
    input_schema: Value,
    container_id: String,
}

impl ShellCommandHandler {
    pub fn new(
        name: String,
        description: String,
        command_template: String,
        input_schema: Value,
        container_id: String,
    ) -> Self {
        Self {
            name,
            description,
            command_template,
            input_schema,
            container_id,
        }
    }

    fn render_command(&self, input: &Value) -> Result<String> {
        // Step 1: substitute {key} placeholders from tool input
        let mut cmd = self.command_template.clone();

        if let Value::Object(map) = input {
            for (key, value) in map {
                let placeholder = format!("{{{}}}", key);
                let value_str = match value {
                    Value::String(s) => s.clone(),
                    _ => value.to_string(),
                };
                cmd = cmd.replace(&placeholder, &value_str);
            }
        }

        // Step 2: expand ${ENV_VAR} references from the host environment
        let mut result = String::new();
        let mut chars = cmd.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '$' && chars.peek() == Some(&'{') {
                chars.next(); // consume '{'
                let mut var_name = String::new();
                for nc in chars.by_ref() {
                    if nc == '}' {
                        break;
                    }
                    var_name.push(nc);
                }
                let val = std::env::var(&var_name).unwrap_or_default();
                result.push_str(&val);
            } else {
                result.push(c);
            }
        }

        Ok(result)
    }
}

#[async_trait]
impl ToolHandler for ShellCommandHandler {
    async fn execute(&self, _root: &Path, input: Value) -> Result<String> {
        let cmd = self.render_command(&input)?;

        // All shell tools run inside the container with /workspace as the working directory
        let output = Command::new("container")
            .args(["exec", &self.container_id, "sh", "-c", &cmd])
            .output()
            .map_err(|e| {
                SandboxError::CommandError(format!("Container exec failed for shell tool: {}", e))
            })?;

        if !output.status.success() {
            return Err(SandboxError::ToolError(format!(
                "Command failed with exit code {}: {}",
                output.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
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
