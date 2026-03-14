//! MCP tool patch loading and application
//!
//! Patch files let you filter which tools from an MCP server are exposed and
//! override/enrich tool definitions (descriptions, output schemas).
//!
//! # Patch file format
//!
//! A JSON object keyed by tool name, each value being a partial override:
//!
//! ```json
//! {
//!   "list_customers": {
//!     "output_schema": { "type": "array", "items": { ... } }
//!   },
//!   "create_invoice": {
//!     "description": "Create a Stripe invoice",
//!     "output_schema": { "type": "object", "properties": { "id": { "type": "string" } } }
//!   }
//! }
//! ```

use super::McpToolDefinition;
use crate::sandbox::error::{Result, SandboxError};
use portlang_core::{McpPatchMap, Tool};
use std::path::Path;

/// Load a patch map from a JSON file on disk.
///
/// The path is resolved relative to `config_dir` if provided.
/// Returns an empty map if `patch_file` is `None`.
pub fn load_patch_map(patch_file: Option<&str>, config_dir: Option<&Path>) -> Result<McpPatchMap> {
    let Some(path_str) = patch_file else {
        return Ok(McpPatchMap::new());
    };

    let path = match config_dir {
        Some(dir) => dir.join(path_str),
        None => Path::new(path_str).to_path_buf(),
    };

    if !path.exists() {
        return Err(SandboxError::McpToolError(format!(
            "MCP patch file not found: '{}'\nPaths in patch_file are relative to the field.toml directory.",
            path.display()
        )));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| {
        SandboxError::McpToolError(format!(
            "Failed to read MCP patch file '{}': {}",
            path.display(),
            e
        ))
    })?;

    serde_json::from_str(&content).map_err(|e| {
        SandboxError::McpToolError(format!(
            "Invalid JSON in MCP patch file '{}': {}",
            path.display(),
            e
        ))
    })
}

/// Apply filtering and patches to a list of discovered MCP tool definitions.
///
/// Steps applied in order:
/// 1. Filter by `include_tools` (whitelist) or `exclude_tools` (blacklist)
/// 2. Apply per-tool description and output_schema overrides from the patch map
pub fn apply_patches(
    tools: Vec<McpToolDefinition>,
    tool_config: &Tool,
    patch_map: &McpPatchMap,
) -> Vec<McpToolDefinition> {
    tools
        .into_iter()
        .filter(|def| {
            if let Some(ref include) = tool_config.include_tools {
                return include.contains(&def.name);
            }
            if let Some(ref exclude) = tool_config.exclude_tools {
                return !exclude.contains(&def.name);
            }
            true
        })
        .map(|mut def| {
            if let Some(patch) = patch_map.get(&def.name) {
                if let Some(ref desc) = patch.description {
                    def.description = Some(desc.clone());
                }
                if let Some(ref schema) = patch.output_schema {
                    def.output_schema = Some(schema.clone());
                }
            }
            def
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use portlang_core::{McpPatchMap, McpToolPatch, Tool};
    use serde_json::json;

    fn make_tool_config(include: Option<Vec<&str>>, exclude: Option<Vec<&str>>) -> Tool {
        Tool {
            tool_type: "mcp".to_string(),
            name: Some("test".to_string()),
            description: None,
            file: None,
            function: None,
            input_schema: json!({}),
            output_schema: None,
            command: None,
            args: vec![],
            env: Default::default(),
            url: None,
            headers: None,
            transport: None,
            include_tools: include.map(|v| v.into_iter().map(String::from).collect()),
            exclude_tools: exclude.map(|v| v.into_iter().map(String::from).collect()),
            patch_file: None,
        }
    }

    fn make_def(name: &str) -> McpToolDefinition {
        McpToolDefinition {
            name: name.to_string(),
            description: None,
            input_schema: json!({}),
            output_schema: None,
        }
    }

    #[test]
    fn test_include_tools_whitelist() {
        let tools = vec![make_def("a"), make_def("b"), make_def("c")];
        let cfg = make_tool_config(Some(vec!["a", "c"]), None);
        let result = apply_patches(tools, &cfg, &McpPatchMap::new());
        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|t| t.name == "a"));
        assert!(result.iter().any(|t| t.name == "c"));
    }

    #[test]
    fn test_exclude_tools_blacklist() {
        let tools = vec![make_def("a"), make_def("b"), make_def("c")];
        let cfg = make_tool_config(None, Some(vec!["b"]));
        let result = apply_patches(tools, &cfg, &McpPatchMap::new());
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|t| t.name != "b"));
    }

    #[test]
    fn test_no_filter_passes_all() {
        let tools = vec![make_def("a"), make_def("b")];
        let cfg = make_tool_config(None, None);
        let result = apply_patches(tools, &cfg, &McpPatchMap::new());
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_patch_description_override() {
        let tools = vec![make_def("my_tool")];
        let cfg = make_tool_config(None, None);
        let mut patch_map = McpPatchMap::new();
        patch_map.insert(
            "my_tool".to_string(),
            McpToolPatch {
                description: Some("Override description".to_string()),
                output_schema: None,
            },
        );
        let result = apply_patches(tools, &cfg, &patch_map);
        assert_eq!(
            result[0].description.as_deref(),
            Some("Override description")
        );
    }

    #[test]
    fn test_patch_output_schema_injection() {
        let tools = vec![make_def("my_tool")];
        let cfg = make_tool_config(None, None);
        let schema = json!({ "type": "object", "properties": { "id": { "type": "string" } } });
        let mut patch_map = McpPatchMap::new();
        patch_map.insert(
            "my_tool".to_string(),
            McpToolPatch {
                description: None,
                output_schema: Some(schema.clone()),
            },
        );
        let result = apply_patches(tools, &cfg, &patch_map);
        assert_eq!(result[0].output_schema, Some(schema));
    }

    #[test]
    fn test_load_patch_map_none_returns_empty() {
        let result = load_patch_map(None, None).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_load_patch_map_bad_json() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("bad.json");
        std::fs::write(&file, "not valid json").unwrap();
        let result = load_patch_map(Some("bad.json"), Some(dir.path()));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid JSON"));
    }

    #[test]
    fn test_load_patch_map_valid_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("patches.json");
        std::fs::write(
            &file,
            r#"{"my_tool": {"description": "patched", "output_schema": {"type": "string"}}}"#,
        )
        .unwrap();
        let result = load_patch_map(Some("patches.json"), Some(dir.path())).unwrap();
        assert!(result.contains_key("my_tool"));
        assert_eq!(result["my_tool"].description.as_deref(), Some("patched"));
    }
}
