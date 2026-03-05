use crate::sandbox::error::{Result, SandboxError};
use crate::tools::handler::ToolHandler;
use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;
use std::process::Command;

/// Tool that executes a shell command template
pub struct ShellCommandHandler {
    name: String,
    description: String,
    command_template: String,
    input_schema: Value,
}

impl ShellCommandHandler {
    pub fn new(
        name: String,
        description: String,
        command_template: String,
        input_schema: Value,
    ) -> Self {
        Self {
            name,
            description,
            command_template,
            input_schema,
        }
    }

    fn render_command(&self, input: &Value) -> Result<String> {
        // Simple template substitution: replace {key} with input["key"]
        let mut cmd = self.command_template.clone();

        if let Value::Object(map) = input {
            for (key, value) in map {
                let placeholder = format!("{{{}}}", key);
                let value_str = match value {
                    Value::String(s) => s.clone(),
                    _ => value.to_string(),
                };
                // Basic shell escaping - wrap in single quotes and escape single quotes
                let escaped = value_str.replace('\'', "'\"'\"'");
                cmd = cmd.replace(&placeholder, &format!("'{}'", escaped));
            }
        }

        Ok(cmd)
    }
}

#[async_trait]
impl ToolHandler for ShellCommandHandler {
    async fn execute(&self, root: &Path, input: Value) -> Result<String> {
        let cmd = self.render_command(&input)?;

        let output = Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .current_dir(root)
            .output()
            .map_err(|e| SandboxError::CommandError(format!("Failed to execute command: {}", e)))?;

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
