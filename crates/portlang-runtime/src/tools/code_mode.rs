//! Code Mode tool handler using pctx_code_mode
//!
//! Code Mode allows agents to write TypeScript code that executes in a sandboxed
//! Deno runtime, dramatically reducing token usage for data-heavy operations.

#[cfg(feature = "code-mode")]
use pctx_code_mode::{
    config::ToolDisclosure,
    model::{CallbackConfig, FunctionId, GetFunctionDetailsInput},
    registry::PctxRegistry,
    CodeMode,
};

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
    callback_registry: PctxRegistry,
}

#[cfg(feature = "code-mode")]
impl CodeModeHandler {
    /// Create a new Code Mode handler
    pub fn new() -> Self {
        Self {
            code_mode: CodeMode::default(),
            callback_registry: PctxRegistry::default(),
        }
    }

    /// Register a custom tool that can be called from TypeScript code
    ///
    /// # Arguments
    /// * `namespace` - Namespace for the tool (e.g., "Tools", "MCP")
    /// * `name` - Function name
    /// * `description` - Optional description
    /// * `input_schema` - JSON schema for input validation
    /// * `output_schema` - Optional JSON schema for return type (generates TypeScript return types in code mode)
    /// * `callback` - Async function to execute when the tool is called
    pub fn register_tool(
        &mut self,
        namespace: String,
        name: String,
        description: Option<String>,
        input_schema: Value,
        output_schema: Option<Value>,
        callback: CodeModeCallback,
    ) -> Result<()> {
        // Register the callback metadata with CodeMode
        let callback_config = CallbackConfig {
            namespace: Some(namespace.clone()),
            name: name.clone(),
            description,
            input_schema: Some(input_schema),
            output_schema,
        };

        self.code_mode.add_callback(&callback_config).map_err(|e| {
            SandboxError::ToolError(format!("Failed to add callback config: {}", e))
        })?;

        // Register the callback implementation using the callback_id method
        let callback_id = callback_config.id();
        self.callback_registry
            .add_callback(&callback_id, callback)
            .map_err(|e| {
                SandboxError::ToolError(format!("Failed to add callback to registry: {}", e))
            })?;

        tracing::debug!("Registered Code Mode tool: {}", callback_id);
        Ok(())
    }

    /// Get the TypeScript type definitions for all registered tools
    pub fn get_typescript_definitions(&self) -> String {
        let all_fns: Vec<FunctionId> = self
            .code_mode
            .tool_sets()
            .iter()
            .flat_map(|ts| {
                ts.tools.iter().map(|t| FunctionId {
                    mod_name: ts.pascal_namespace(),
                    fn_name: t.fn_name.clone(),
                })
            })
            .collect();

        let input = GetFunctionDetailsInput { functions: all_fns };
        self.code_mode.get_function_details(input).code
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
                    .execute_typescript(&code, ToolDisclosure::Catalog, Some(callback_registry))
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
        "Execute a complete TypeScript script end-to-end. Chain all discovery, decisions, and actions in one run() function."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "required": ["code"],
            "properties": {
                "code": {
                    "type": "string",
                    "description": "Complete async TypeScript script. Define async function run() and chain all operations — fetching data, making decisions, taking actions — using sequential await calls."
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
                None, // no output schema for this test tool
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

    #[tokio::test]
    async fn test_load_patch_map_and_apply() {
        use crate::mcp::{apply_patches, load_patch_map, McpToolDefinition};

        // Write a temp patch file and load it
        let patch_json = serde_json::json!({
            "list_products": {
                "output_schema": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["id", "name"],
                        "properties": {
                            "id": { "type": "string" },
                            "name": { "type": "string" }
                        }
                    }
                }
            }
        });

        let dir = tempfile::tempdir().unwrap();
        let patch_path = dir.path().join("test.patches.json");
        std::fs::write(&patch_path, serde_json::to_string(&patch_json).unwrap()).unwrap();

        let patch_map = load_patch_map(Some("test.patches.json"), Some(dir.path())).unwrap();

        println!(
            "Loaded patch_map keys: {:?}",
            patch_map.keys().collect::<Vec<_>>()
        );
        assert!(
            patch_map.contains_key("list_products"),
            "patch_map should contain list_products"
        );
        assert!(
            patch_map["list_products"].output_schema.is_some(),
            "output_schema should be Some"
        );

        // Apply to a tool that matches
        let tool_def = McpToolDefinition {
            name: "list_products".to_string(),
            description: None,
            input_schema: serde_json::json!({ "type": "object" }),
            output_schema: None,
        };

        let tool_config = portlang_core::Tool {
            tool_type: "mcp".to_string(),
            name: Some("stripe".to_string()),
            description: None,
            file: None,
            function: None,
            input_schema: serde_json::Value::Null,
            output_schema: None,
            command: None,
            args: vec![],
            env: std::collections::HashMap::new(),
            url: None,
            headers: None,
            transport: None,
            include_tools: None,
            exclude_tools: None,
            patch_file: None,
        };

        let patched = apply_patches(vec![tool_def], &tool_config, &patch_map);
        println!(
            "Patched tool output_schema: {:?}",
            patched[0].output_schema.is_some()
        );
        assert!(
            patched[0].output_schema.is_some(),
            "apply_patches should inject output_schema from patch_map"
        );
    }

    #[tokio::test]
    async fn test_patch_apply_then_register() {
        use crate::mcp::{apply_patches, McpToolDefinition};
        use portlang_core::{McpPatchMap, McpToolPatch, Tool};

        // Simulate a discovered MCP tool with no output_schema
        let tool_def = McpToolDefinition {
            name: "list_products".to_string(),
            description: None,
            input_schema: serde_json::json!({ "type": "object", "properties": {} }),
            output_schema: None,
        };

        // Build a patch map as if loaded from a patch file
        let mut patch_map = McpPatchMap::new();
        patch_map.insert(
            "list_products".to_string(),
            McpToolPatch {
                description: None,
                output_schema: Some(serde_json::json!({
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["id", "name"],
                        "properties": {
                            "id": { "type": "string" },
                            "name": { "type": "string" }
                        }
                    }
                })),
            },
        );

        let tool_config = Tool {
            tool_type: "mcp".to_string(),
            name: Some("stripe".to_string()),
            description: None,
            file: None,
            function: None,
            input_schema: serde_json::Value::Null,
            output_schema: None,
            command: None,
            args: vec![],
            env: std::collections::HashMap::new(),
            url: None,
            headers: None,
            transport: None,
            include_tools: None,
            exclude_tools: None,
            patch_file: None,
        };
        let patched = apply_patches(vec![tool_def], &tool_config, &patch_map);

        assert_eq!(patched.len(), 1);
        let patched_tool = &patched[0];
        assert!(
            patched_tool.output_schema.is_some(),
            "apply_patches should inject output_schema"
        );

        // Now register it and check the TypeScript
        let mut handler = CodeModeHandler::new();
        let callback: CodeModeCallback =
            Arc::new(|_| Box::pin(async move { Ok(serde_json::json!([])) }));

        handler
            .register_tool(
                "Stripe".to_string(),
                patched_tool.name.clone(),
                patched_tool.description.clone(),
                patched_tool.input_schema.clone(),
                patched_tool.output_schema.clone(),
                callback,
            )
            .unwrap();

        let defs = handler.get_typescript_definitions();
        println!("Patch→Register TypeScript:\n{}", defs);
        assert!(
            !defs.contains("Promise<any>"),
            "Should have typed return after patch. Got:\n{}",
            defs
        );
    }

    #[tokio::test]
    async fn test_output_schema_none_gives_any() {
        let mut handler = CodeModeHandler::new();

        let callback: CodeModeCallback =
            Arc::new(|_| Box::pin(async move { Ok(serde_json::json!([])) }));

        handler
            .register_tool(
                "Stripe".to_string(),
                "list_products".to_string(),
                None,
                serde_json::json!({ "type": "object", "properties": {} }),
                None, // no output schema
                callback,
            )
            .unwrap();

        let defs = handler.get_typescript_definitions();
        println!("No-schema TypeScript:\n{}", defs);
        assert!(
            defs.contains("Promise<any>"),
            "Expected Promise<any> when no output schema"
        );
    }

    #[tokio::test]
    async fn test_output_schema_generates_typed_return() {
        let mut handler = CodeModeHandler::new();

        let callback: CodeModeCallback = Arc::new(|_| {
            Box::pin(async move { Ok(serde_json::json!([{"id": "prod_123", "name": "Widget"}])) })
        });

        handler
            .register_tool(
                "Stripe".to_string(),
                "list_products".to_string(),
                None,
                serde_json::json!({ "type": "object", "properties": {} }),
                Some(serde_json::json!({
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["id", "name"],
                        "properties": {
                            "id": { "type": "string" },
                            "name": { "type": "string" }
                        }
                    }
                })),
                callback,
            )
            .unwrap();

        let defs = handler.get_typescript_definitions();
        println!("Generated TypeScript:\n{}", defs);

        // Should NOT return `any` — should have a typed array return
        assert!(
            !defs.contains("Promise<any>"),
            "Expected typed return, got Promise<any>. Full defs:\n{}",
            defs
        );
        assert!(
            defs.contains("listProducts"),
            "Expected listProducts function"
        );
    }
}
