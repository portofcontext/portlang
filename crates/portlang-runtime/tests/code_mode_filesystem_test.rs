//! Tests that code mode filesystem tools (read, write, glob) resolve paths
//! relative to the workspace root, not a hardcoded "/workspace" path.

#[cfg(feature = "code-mode")]
#[cfg(test)]
mod tests {
    use portlang_runtime::tools::{
        handler::ToolHandler, CodeModeHandler, GlobHandler, ReadHandler, WriteHandler,
    };
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use tempfile::TempDir;

    type CallbackFn = Arc<
        dyn Fn(
                Option<serde_json::Value>,
            )
                -> Pin<Box<dyn Future<Output = Result<serde_json::Value, String>> + Send>>
            + Send
            + Sync,
    >;

    /// Build a CodeModeHandler with read/write/glob wired to `root`, mirroring
    /// what loop_runner.rs does when code_mode_enabled = true.
    fn build_handler(root: std::path::PathBuf) -> CodeModeHandler {
        let mut handler = CodeModeHandler::new();

        // Register read
        {
            let root = root.clone();
            let read_handler = Arc::new(ReadHandler);
            let callback: CallbackFn = Arc::new(move |args| {
                let root = root.clone();
                let h = read_handler.clone();
                let input = args.unwrap_or(serde_json::json!({}));
                Box::pin(async move {
                    h.execute(&root, input)
                        .await
                        .map(|s| serde_json::Value::String(s))
                        .map_err(|e| format!("Tool error: {}", e))
                })
            });
            handler
                .register_tool(
                    "Tools".to_string(),
                    "read".to_string(),
                    Some("Read the contents of a file".to_string()),
                    ReadHandler.input_schema(),
                    None,
                    callback,
                )
                .unwrap();
        }

        // Register write
        {
            let root = root.clone();
            let write_handler = Arc::new(WriteHandler);
            let callback: CallbackFn = Arc::new(move |args| {
                let root = root.clone();
                let h = write_handler.clone();
                let input = args.unwrap_or(serde_json::json!({}));
                Box::pin(async move {
                    h.execute(&root, input)
                        .await
                        .map(|s| serde_json::Value::String(s))
                        .map_err(|e| format!("Tool error: {}", e))
                })
            });
            handler
                .register_tool(
                    "Tools".to_string(),
                    "write".to_string(),
                    Some("Write content to a file".to_string()),
                    WriteHandler.input_schema(),
                    None,
                    callback,
                )
                .unwrap();
        }

        // Register glob
        {
            let root = root.clone();
            let glob_handler = Arc::new(GlobHandler);
            let callback: CallbackFn = Arc::new(move |args| {
                let root = root.clone();
                let h = glob_handler.clone();
                let input = args.unwrap_or(serde_json::json!({}));
                Box::pin(async move {
                    h.execute(&root, input)
                        .await
                        .map(|s| {
                            serde_json::from_str::<serde_json::Value>(&s)
                                .unwrap_or(serde_json::Value::String(s))
                        })
                        .map_err(|e| format!("Tool error: {}", e))
                })
            });
            handler
                .register_tool(
                    "Tools".to_string(),
                    "glob".to_string(),
                    Some("Find files matching a glob pattern".to_string()),
                    GlobHandler.input_schema(),
                    None,
                    callback,
                )
                .unwrap();
        }

        handler
    }

    #[tokio::test]
    async fn test_read_uses_workspace_root() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();

        // Write a file directly into the temp dir (not /workspace)
        std::fs::write(root.join("hello.txt"), "hello from temp dir").unwrap();

        let handler = build_handler(root.clone());

        let code = r#"
            async function run() {
                const content = await Tools.read({ path: "hello.txt" });
                return { content };
            }
        "#;

        let result = handler
            .execute(&root, serde_json::json!({ "code": code }))
            .await
            .expect("code execution failed");

        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(
            value["content"].as_str(),
            Some("hello from temp dir"),
            "read should return contents from workspace root, not /workspace"
        );
    }

    #[tokio::test]
    async fn test_write_uses_workspace_root() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();

        let handler = build_handler(root.clone());

        let code = r#"
            async function run() {
                await Tools.write({ path: "output.txt", content: "written by code mode" });
                return { ok: true };
            }
        "#;

        let result = handler
            .execute(&root, serde_json::json!({ "code": code }))
            .await
            .expect("code execution failed");

        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(
            value["ok"].as_bool().unwrap_or(false),
            "write should succeed"
        );

        let written = std::fs::read_to_string(root.join("output.txt")).unwrap();
        assert_eq!(
            written, "written by code mode",
            "file should be written to workspace root, not /workspace"
        );
    }

    #[tokio::test]
    async fn test_glob_uses_workspace_root() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();

        // Write a couple of files
        std::fs::write(root.join("a.json"), "{}").unwrap();
        std::fs::write(root.join("b.json"), "{}").unwrap();

        let handler = build_handler(root.clone());

        let code = r#"
            async function run() {
                const files = await Tools.glob({ pattern: "*.json" });
                return { count: files.length };
            }
        "#;

        let result = handler
            .execute(&root, serde_json::json!({ "code": code }))
            .await
            .expect("code execution failed");

        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(
            value["count"].as_i64(),
            Some(2),
            "glob should find files in workspace root, not /workspace"
        );
    }
}
