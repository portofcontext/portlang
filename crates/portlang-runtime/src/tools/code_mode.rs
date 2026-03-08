//! Code Mode tool handler using pctx_code_mode
//!
//! Code Mode allows agents to write TypeScript code that executes in a sandboxed
//! Deno runtime, dramatically reducing token usage for data-heavy operations.

#[cfg(feature = "code-mode")]
use pctx_code_mode::{model::CallbackConfig, CallbackRegistry, CodeMode};

use crate::sandbox::error::{Result, SandboxError};
use crate::tools::handler::ToolHandler;
use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;

#[cfg(feature = "code-mode")]
use std::future::Future;
#[cfg(feature = "code-mode")]
use std::pin::Pin;
#[cfg(feature = "code-mode")]
use std::sync::Arc;

/// Type alias for callback functions
#[cfg(feature = "code-mode")]
pub type CodeModeCallback = Arc<
    dyn Fn(
            Option<Value>,
        ) -> Pin<Box<dyn Future<Output = std::result::Result<Value, String>> + Send>>
        + Send
        + Sync,
>;

/// Code Mode tool handler
///
/// This handler enables agents to write and execute TypeScript code in a sandboxed
/// Deno runtime. Tools are exposed as TypeScript functions that can be called
/// from the generated code.
#[cfg(feature = "code-mode")]
pub struct CodeModeHandler {
    code_mode: CodeMode,
    callback_registry: CallbackRegistry,
}

#[cfg(feature = "code-mode")]
impl CodeModeHandler {
    /// Create a new Code Mode handler
    pub fn new() -> Self {
        Self {
            code_mode: CodeMode::default(),
            callback_registry: CallbackRegistry::default(),
        }
    }

    /// Register a custom tool that can be called from TypeScript code
    ///
    /// # Arguments
    /// * `namespace` - Namespace for the tool (e.g., "Tools", "MCP")
    /// * `name` - Function name
    /// * `description` - Optional description
    /// * `input_schema` - JSON schema for input validation
    /// * `callback` - Async function to execute when the tool is called
    pub fn register_tool(
        &mut self,
        namespace: String,
        name: String,
        description: Option<String>,
        input_schema: Value,
        callback: CodeModeCallback,
    ) -> Result<()> {
        // Register the callback metadata with CodeMode
        let callback_config = CallbackConfig {
            namespace: namespace.clone(),
            name: name.clone(),
            description,
            input_schema: Some(input_schema),
            output_schema: None,
        };

        self.code_mode.add_callback(&callback_config).map_err(|e| {
            SandboxError::ToolError(format!("Failed to add callback config: {}", e))
        })?;

        // Register the callback implementation
        let callback_id = format!("{}.{}", namespace, name);
        self.callback_registry
            .add(&callback_id, callback)
            .map_err(|e| {
                SandboxError::ToolError(format!("Failed to add callback to registry: {}", e))
            })?;

        tracing::debug!("Registered Code Mode tool: {}", callback_id);
        Ok(())
    }

    /// Execute TypeScript code
    ///
    /// Note: Deno runtime is not Send, so we use spawn_blocking
    pub async fn execute_code(&self, code: String) -> Result<Value> {
        // Clone what we need for the blocking task
        let code_mode = self.code_mode.clone();
        let callback_registry = self.callback_registry.clone();

        // Execute in a blocking task since Deno runtime isn't Send
        let result = tokio::task::spawn_blocking(move || {
            // Create a current-thread Tokio runtime (required by Deno/V8)
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| SandboxError::ToolError(format!("Failed to create runtime: {}", e)))?;

            rt.block_on(async move {
                let output = code_mode
                    .execute(&code, Some(callback_registry))
                    .await
                    .map_err(|e| {
                        SandboxError::ToolError(format!("Code Mode execution failed: {}", e))
                    })?;

                if !output.success {
                    return Err(SandboxError::ToolError(format!(
                        "Code Mode execution failed: {}",
                        output.stderr
                    )));
                }

                Ok(output.output.unwrap_or(Value::Null))
            })
        })
        .await
        .map_err(|e| SandboxError::ToolError(format!("Task join error: {}", e)))??;

        Ok(result)
    }
}

#[cfg(feature = "code-mode")]
impl Default for CodeModeHandler {
    fn default() -> Self {
        Self::new()
    }
}

/// Stub implementation when code-mode feature is not enabled
#[cfg(not(feature = "code-mode"))]
pub struct CodeModeHandler;

#[cfg(not(feature = "code-mode"))]
impl CodeModeHandler {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(not(feature = "code-mode"))]
impl Default for CodeModeHandler {
    fn default() -> Self {
        Self::new()
    }
}

/// Tool handler implementation for Code Mode
#[async_trait]
impl ToolHandler for CodeModeHandler {
    fn name(&self) -> &str {
        "code_mode"
    }

    fn description(&self) -> &str {
        "Execute TypeScript code in a sandboxed Deno runtime"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "required": ["code"],
            "properties": {
                "code": {
                    "type": "string",
                    "description": "TypeScript code to execute"
                }
            }
        })
    }

    async fn execute(&self, _root: &Path, input: Value) -> Result<String> {
        #[cfg(feature = "code-mode")]
        {
            let code = input
                .get("code")
                .and_then(|v| v.as_str())
                .ok_or_else(|| SandboxError::ToolError("Missing 'code' field".to_string()))?
                .to_string();

            let result = self.execute_code(code).await?;
            Ok(serde_json::to_string(&result).unwrap_or_else(|_| "null".to_string()))
        }

        #[cfg(not(feature = "code-mode"))]
        {
            let _ = input;
            Err(SandboxError::ToolError(
                "Code Mode is not enabled. Compile with --features code-mode".to_string(),
            ))
        }
    }
}

#[cfg(all(test, feature = "code-mode"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_simple_execution() {
        let handler = CodeModeHandler::new();

        // Code Mode expects an async function run() structure
        let code = r#"
            async function run() {
                const x = 42;
                const y = 137;
                return x + y;
            }
        "#;

        let input = serde_json::json!({ "code": code });
        let result = handler
            .execute(std::path::Path::new("."), input)
            .await
            .unwrap();
        let result_value: Value = serde_json::from_str(&result).unwrap();

        assert_eq!(result_value, serde_json::json!(179));
    }

    #[tokio::test]
    async fn test_tool_registration() {
        let mut handler = CodeModeHandler::new();

        // Register a simple tool
        let callback: CodeModeCallback = Arc::new(|args| {
            Box::pin(async move {
                let value = args
                    .and_then(|v| v.get("value").cloned())
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                Ok(serde_json::json!({ "result": value * 2 }))
            })
        });

        handler
            .register_tool(
                "Test".to_string(),
                "double".to_string(),
                Some("Double a number".to_string()),
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "value": { "type": "number" }
                    },
                    "required": ["value"]
                }),
                callback,
            )
            .unwrap();

        // Execute code that calls the registered tool
        let code = r#"
            async function run() {
                const result = await Test.double({ value: 21 });
                return result;
            }
        "#;

        let input = serde_json::json!({ "code": code });
        let result = handler
            .execute(std::path::Path::new("."), input)
            .await
            .unwrap();
        let result_value: Value = serde_json::from_str(&result).unwrap();

        assert_eq!(result_value, serde_json::json!({ "result": 42 }));
    }
}
