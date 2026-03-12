//! Test MCP tool registration with Code Mode

#[cfg(feature = "code-mode")]
#[cfg(test)]
mod tests {
    use portlang_runtime::tools::handler::ToolHandler;
    use portlang_runtime::tools::CodeModeHandler;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;

    type CallbackFn = Arc<
        dyn Fn(
                Option<serde_json::Value>,
            )
                -> Pin<Box<dyn Future<Output = Result<serde_json::Value, String>> + Send>>
            + Send
            + Sync,
    >;

    #[tokio::test]
    async fn test_mcp_namespace_in_code_mode() {
        let mut handler = CodeModeHandler::new();

        // Register a mock MCP tool
        let callback: CallbackFn = Arc::new(|args| {
            Box::pin(async move {
                let path = args
                    .and_then(|v| v.get("path").cloned())
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_default();
                Ok(serde_json::json!({
                    "success": true,
                    "path": path,
                    "content": "test content"
                }))
            })
        });

        handler
            .register_tool(
                "filesystem".to_string(),
                "read_file".to_string(),
                Some("Read a file".to_string()),
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" }
                    },
                    "required": ["path"]
                }),
                callback,
            )
            .unwrap();

        // Execute code that uses the MCP server namespace (filesystem)
        // Function names are camelCase (readFile, not read_file)
        let code = r#"
            async function run() {
                const result = await Filesystem.readFile({ path: "/test.txt" });
                return result;
            }
        "#;

        let input = serde_json::json!({ "code": code });
        let result = handler.execute(std::path::Path::new("."), input).await;

        match result {
            Ok(output) => {
                println!("Code execution succeeded:");
                println!("{}", output);
                let result_value: serde_json::Value = serde_json::from_str(&output).unwrap();
                assert!(result_value.get("success").is_some());
            }
            Err(e) => {
                panic!("Code execution failed: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_tools_namespace_in_code_mode() {
        let mut handler = CodeModeHandler::new();

        // Register a mock Python tool
        let callback: CallbackFn = Arc::new(|_args| {
            Box::pin(async move {
                Ok(serde_json::json!([
                    {"name": "row1"},
                    {"name": "row2"}
                ]))
            })
        });

        handler
            .register_tool(
                "Tools".to_string(),
                "load_csv".to_string(),
                Some("Load CSV file".to_string()),
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "filepath": { "type": "string" }
                    },
                    "required": ["filepath"]
                }),
                callback,
            )
            .unwrap();

        // Execute code that uses the Tools namespace
        // Function names are camelCase (loadCsv, not load_csv)
        let code = r#"
            async function run() {
                const data = await Tools.loadCsv({ filepath: "data.csv" });
                return { count: data.length, data };
            }
        "#;

        let input = serde_json::json!({ "code": code });
        let result = handler.execute(std::path::Path::new("."), input).await;

        match result {
            Ok(output) => {
                println!("Code execution succeeded:");
                println!("{}", output);
                let result_value: serde_json::Value = serde_json::from_str(&output).unwrap();
                assert_eq!(result_value.get("count").and_then(|v| v.as_i64()), Some(2));
            }
            Err(e) => {
                panic!("Code execution failed: {}", e);
            }
        }
    }
}
