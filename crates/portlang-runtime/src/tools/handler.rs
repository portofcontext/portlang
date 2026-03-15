use crate::sandbox::error::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;

/// Handler for executing a specific tool
#[async_trait]
pub trait ToolHandler: Send + Sync {
    /// Execute the tool with given input
    /// Returns tool result as a string
    async fn execute(&self, root: &Path, input: Value) -> Result<String>;

    /// Tool name
    fn name(&self) -> &str;

    /// Tool description (for API)
    fn description(&self) -> &str;

    /// Input schema (JSON Schema)
    fn input_schema(&self) -> Value;

    /// Output schema (JSON Schema), if known
    fn output_schema(&self) -> Option<Value> {
        None
    }
}
