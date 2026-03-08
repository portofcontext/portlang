use crate::error::{FieldParseError, Result};
use crate::raw::*;
use crate::validation::*;
use portlang_core::*;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Normalize a path by removing "." and ".." components
fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {
                // Skip "." components
            }
            Component::ParentDir => {
                // Handle ".." by removing last component if possible
                if !components.is_empty() {
                    components.pop();
                }
            }
            comp => {
                components.push(comp);
            }
        }
    }
    components.iter().collect()
}

/// Parse a field from a TOML file
pub fn parse_field_from_file(path: impl AsRef<Path>) -> Result<Field> {
    let path = path.as_ref();
    let content = fs::read_to_string(path)?;

    // Convert to absolute path to ensure consistent resolution
    let abs_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };

    // Capture directory containing field.toml
    let config_dir = abs_path.parent().map(|p: &Path| p.to_path_buf());

    parse_field_from_str_with_context(&content, config_dir)
}

/// Parse a field from a TOML string
pub fn parse_field_from_str(toml_str: &str) -> Result<Field> {
    parse_field_from_str_with_context(toml_str, None)
}

/// Parse a field from a TOML string with config directory context
fn parse_field_from_str_with_context(toml_str: &str, config_dir: Option<PathBuf>) -> Result<Field> {
    let raw: RawField = toml::from_str(toml_str)?;
    convert_raw_field(raw, config_dir)
}

/// Convert raw field to validated field
fn convert_raw_field(raw: RawField, config_dir: Option<PathBuf>) -> Result<Field> {
    // Helper function to resolve relative paths
    let resolve_path = |path_str: &str| -> PathBuf {
        let path = Path::new(path_str);
        if path.is_absolute() {
            // Absolute paths pass through unchanged
            path.to_path_buf()
        } else if let Some(ref base) = config_dir {
            // Relative paths resolve from field.toml directory
            normalize_path(&base.join(path))
        } else {
            // No config_dir (stdin/string): fallback to CWD
            path.to_path_buf()
        }
    };

    // Parse model
    let model = ModelSpec {
        name: raw.model.name,
        temperature: raw.model.temperature,
        max_tokens: raw.model.max_tokens,
    };

    // Parse environment with path resolution
    let environment = match raw.environment {
        RawEnvironment::Local { root } => {
            let resolved = resolve_path(&root);
            Environment::Local {
                root: resolved.to_string_lossy().to_string(),
            }
        }
    };

    // Parse boundary
    let boundary = if let Some(raw_boundary) = raw.boundary {
        // Validate glob patterns
        validate_glob_patterns(&raw_boundary.allow_write)?;

        let network = match raw_boundary.network.as_deref() {
            Some("allow") => NetworkPolicy::Allow,
            Some("deny") | None => NetworkPolicy::Deny,
            Some(other) => {
                return Err(FieldParseError::InvalidField(format!(
                    "Invalid network policy: {}. Must be 'allow' or 'deny'",
                    other
                )))
            }
        };

        Boundary {
            allow_write: raw_boundary.allow_write,
            network,
        }
    } else {
        Boundary::default()
    };

    // Parse context
    let context = if let Some(raw_context) = raw.context {
        let max_cost = raw_context.max_cost.as_ref().map(parse_cost).transpose()?;

        ContextPolicy {
            max_tokens: raw_context.max_tokens,
            max_cost,
            max_steps: raw_context.max_steps,
            system_prompt: raw_context.system_prompt,
        }
    } else {
        ContextPolicy::default()
    };

    // Parse verifiers
    let verifiers = raw
        .verifiers
        .into_iter()
        .map(|raw_verifier| {
            let trigger = match raw_verifier.trigger.as_deref() {
                Some("always") => VerifierTrigger::Always,
                Some("on_stop") | None => VerifierTrigger::OnStop,
                Some("on_write") => VerifierTrigger::OnWrite,
                Some(other) => {
                    return Err(FieldParseError::InvalidField(format!(
                        "Invalid verifier trigger: {}. Must be 'always', 'on_stop', or 'on_write'",
                        other
                    )))
                }
            };

            Ok(Verifier {
                name: raw_verifier.name,
                command: raw_verifier.command,
                trigger,
                description: raw_verifier.description,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    // Parse custom tools
    let mut custom_tools = Vec::new();

    for raw_tool in raw.tool {
        match raw_tool.tool_type.as_str() {
            "python" => {
                // Check if we need to auto-discover from Python file
                let needs_discovery = raw_tool.name.is_none() || raw_tool.input_schema.is_none();

                if needs_discovery {
                    // Must have script path for discovery
                    let script_path_str = raw_tool.script.as_ref().ok_or_else(|| {
                        FieldParseError::InvalidField(
                            "Python tool missing 'script' field for auto-discovery".to_string(),
                        )
                    })?;

                    // Resolve the script path
                    let script_path = resolve_path(script_path_str);

                    // Extract tool metadata from Python file
                    use crate::python_extractor::PythonToolExtractor;
                    let mut extractor = PythonToolExtractor::new().map_err(|e| {
                        FieldParseError::InvalidField(format!(
                            "Failed to initialize Python extractor: {}",
                            e
                        ))
                    })?;

                    // If function name specified, extract only that function
                    // Otherwise, extract all functions
                    if let Some(ref function_name) = raw_tool.function {
                        let tool_meta = extractor
                            .extract_function(&script_path, function_name)
                            .map_err(|e| {
                                FieldParseError::InvalidField(format!(
                                    "Failed to extract function '{}' from {}: {}",
                                    function_name,
                                    script_path.display(),
                                    e
                                ))
                            })?;

                        custom_tools.push(CustomTool {
                            name: raw_tool.name.unwrap_or(tool_meta.name.clone()),
                            description: raw_tool
                                .description
                                .or(tool_meta.description)
                                .unwrap_or_default(),
                            tool_type: "python".to_string(),
                            command: None,
                            script: Some(script_path.to_string_lossy().to_string()),
                            function: Some(tool_meta.name),
                            input_schema: raw_tool.input_schema.unwrap_or(tool_meta.input_schema),
                        });
                    } else {
                        // Extract all functions from file
                        let tools = extractor.extract_tools(&script_path).map_err(|e| {
                            FieldParseError::InvalidField(format!(
                                "Failed to extract tools from {}: {}",
                                script_path.display(),
                                e
                            ))
                        })?;

                        for tool_meta in tools {
                            custom_tools.push(CustomTool {
                                name: tool_meta.name.clone(),
                                description: tool_meta.description.unwrap_or_default(),
                                tool_type: "python".to_string(),
                                command: None,
                                script: Some(script_path.to_string_lossy().to_string()),
                                function: Some(tool_meta.name),
                                input_schema: tool_meta.input_schema,
                            });
                        }
                    }
                } else {
                    // Manually defined - use provided values
                    let resolved_script = raw_tool
                        .script
                        .as_ref()
                        .map(|s| resolve_path(s).to_string_lossy().to_string());

                    custom_tools.push(CustomTool {
                        name: raw_tool.name.unwrap_or_default(),
                        description: raw_tool.description.unwrap_or_default(),
                        tool_type: raw_tool.tool_type,
                        command: raw_tool.command,
                        script: resolved_script,
                        function: raw_tool.function,
                        input_schema: raw_tool.input_schema.unwrap_or(serde_json::json!({})),
                    });
                }
            }
            "shell" => {
                // Shell tools require manual definition
                let resolved_script = raw_tool
                    .script
                    .as_ref()
                    .map(|s| resolve_path(s).to_string_lossy().to_string());

                custom_tools.push(CustomTool {
                    name: raw_tool.name.ok_or_else(|| {
                        FieldParseError::InvalidField("Shell tool missing 'name' field".to_string())
                    })?,
                    description: raw_tool.description.unwrap_or_default(),
                    tool_type: raw_tool.tool_type,
                    command: raw_tool.command,
                    script: resolved_script,
                    function: raw_tool.function,
                    input_schema: raw_tool.input_schema.unwrap_or(serde_json::json!({})),
                });
            }
            _ => {
                return Err(FieldParseError::InvalidField(format!(
                    "Unknown tool type: {}",
                    raw_tool.tool_type
                )));
            }
        }
    }

    // Parse code mode config
    let code_mode = raw.code_mode.map(|raw_code_mode| CodeModeConfig {
        enabled: raw_code_mode.enabled,
    });

    // Parse MCP servers
    let mcp_servers = raw
        .mcp_server
        .into_iter()
        .map(|raw_mcp| {
            // Validate name is non-empty
            if raw_mcp.name.trim().is_empty() {
                return Err(FieldParseError::InvalidField(
                    "MCP server name cannot be empty".to_string(),
                ));
            }

            // Determine transport type
            let transport_type = raw_mcp.transport.as_deref().unwrap_or("stdio");

            let transport = match transport_type {
                "stdio" => {
                    // Stdio transport requires command
                    let command = raw_mcp.command.ok_or_else(|| {
                        FieldParseError::InvalidField(format!(
                            "MCP server '{}' with stdio transport requires 'command' field",
                            raw_mcp.name
                        ))
                    })?;

                    if command.trim().is_empty() {
                        return Err(FieldParseError::InvalidField(
                            "MCP server command cannot be empty".to_string(),
                        ));
                    }

                    McpTransport::Stdio {
                        command,
                        args: raw_mcp.args,
                        env: raw_mcp.env,
                    }
                }
                "http" | "sse" => {
                    // HTTP/SSE transport requires url
                    let url = raw_mcp.url.ok_or_else(|| {
                        FieldParseError::InvalidField(format!(
                            "MCP server '{}' with {} transport requires 'url' field",
                            raw_mcp.name, transport_type
                        ))
                    })?;

                    if url.trim().is_empty() {
                        return Err(FieldParseError::InvalidField(
                            "MCP server url cannot be empty".to_string(),
                        ));
                    }

                    // Expand environment variables in headers
                    let mut headers = raw_mcp.headers.unwrap_or_default();
                    for (_, value) in headers.iter_mut() {
                        if let Ok(expanded) = shellexpand::env(value) {
                            *value = expanded.to_string();
                        }
                    }

                    McpTransport::Sse { url, headers }
                }
                other => {
                    return Err(FieldParseError::InvalidField(format!(
                        "Invalid MCP transport: '{}'. Supported: 'stdio', 'http', 'sse'",
                        other
                    )))
                }
            };

            Ok(McpServer {
                name: raw_mcp.name,
                transport,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    // Parse container config
    let container = raw
        .container
        .map(|raw_container| ContainerConfig {
            packages: raw_container.packages,
            dockerfile: raw_container.dockerfile,
            image: raw_container.image,
        })
        .unwrap_or_default();

    Ok(Field {
        name: raw.name,
        description: raw.description,
        model,
        environment,
        boundary,
        context,
        verifiers,
        re_observation: raw.re_observation,
        environment_context: raw.environment_context,
        goal: raw.goal,
        custom_tools,
        code_mode,
        mcp_servers,
        container,
        config_dir,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_field() {
        let toml = r#"
            name = "test-field"
            goal = "Do something"

            [model]
            name = "claude-sonnet-4-5"

            [environment]
            type = "local"
            root = "/tmp/test"
        "#;

        let field = parse_field_from_str(toml).unwrap();
        assert_eq!(field.name, "test-field");
        assert_eq!(field.goal, "Do something");
        assert_eq!(field.model.name, "claude-sonnet-4-5");
    }

    #[test]
    fn test_parse_cost_string() {
        let toml = r#"
            name = "test-field"
            goal = "Do something"

            [model]
            name = "claude-sonnet-4-5"

            [environment]
            type = "local"
            root = "/tmp/test"

            [context]
            max_cost = "$2.50"
        "#;

        let field = parse_field_from_str(toml).unwrap();
        assert_eq!(field.context.max_cost.unwrap().to_dollars(), 2.5);
    }

    #[test]
    fn test_parse_cost_number() {
        let toml = r#"
            name = "test-field"
            goal = "Do something"

            [model]
            name = "claude-sonnet-4-5"

            [environment]
            type = "local"
            root = "/tmp/test"

            [context]
            max_cost = 2.5
        "#;

        let field = parse_field_from_str(toml).unwrap();
        assert_eq!(field.context.max_cost.unwrap().to_dollars(), 2.5);
    }

    #[test]
    fn test_reject_unknown_field() {
        let toml = r#"
            name = "test-field"
            goal = "Do something"
            unknown_field = "bad"

            [model]
            name = "claude-sonnet-4-5"

            [environment]
            type = "local"
            root = "/tmp/test"
        "#;

        let result = parse_field_from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_glob_patterns() {
        let toml = r#"
            name = "test-field"
            goal = "Do something"

            [model]
            name = "claude-sonnet-4-5"

            [environment]
            type = "local"
            root = "/tmp/test"

            [boundary]
            allow_write = ["*.txt", "src/**/*.rs"]
        "#;

        let field = parse_field_from_str(toml).unwrap();
        assert_eq!(field.boundary.allow_write.len(), 2);
    }

    #[test]
    fn test_python_auto_discovery() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        // Create a temp Python file
        let mut python_file = NamedTempFile::new().unwrap();
        python_file
            .write_all(
                b"
def greet(name: str) -> str:
    \"\"\"Greet someone by name.\"\"\"
    return f'Hello, {name}!'

def add(x: int, y: int = 0) -> int:
    \"\"\"Add two numbers.\"\"\"
    return x + y
",
            )
            .unwrap();

        let python_path = python_file.path().to_string_lossy().to_string();

        let toml = format!(
            r#"
            name = "test-field"
            goal = "Test auto-discovery"

            [model]
            name = "claude-sonnet-4-5"

            [environment]
            type = "local"
            root = "/tmp/test"

            [[tool]]
            type = "python"
            script = "{}"
        "#,
            python_path
        );

        let field = parse_field_from_str(&toml).unwrap();

        // Should discover both functions
        assert_eq!(field.custom_tools.len(), 2);

        // Check first tool (greet)
        assert_eq!(field.custom_tools[0].name, "greet");
        assert_eq!(field.custom_tools[0].description, "Greet someone by name.");
        assert_eq!(field.custom_tools[0].tool_type, "python");
        assert_eq!(
            field.custom_tools[0].input_schema["properties"]["name"]["type"],
            "string"
        );
        assert_eq!(
            field.custom_tools[0].input_schema["required"],
            serde_json::json!(["name"])
        );

        // Check second tool (add)
        assert_eq!(field.custom_tools[1].name, "add");
        assert_eq!(field.custom_tools[1].description, "Add two numbers.");
        assert_eq!(
            field.custom_tools[1].input_schema["properties"]["x"]["type"],
            "integer"
        );
        assert_eq!(
            field.custom_tools[1].input_schema["properties"]["y"]["type"],
            "integer"
        );
        // Only x is required, y has default
        assert_eq!(
            field.custom_tools[1].input_schema["required"],
            serde_json::json!(["x"])
        );
    }

    #[test]
    fn test_parse_environment_context() {
        let toml = r#"
            name = "test-field"
            goal = "Do something"

            environment_context = """
Available Tools:
  - Python 3.11+
  - pytest for testing
"""

            [model]
            name = "claude-sonnet-4-5"

            [environment]
            type = "local"
            root = "/tmp/test"
        "#;

        let field = parse_field_from_str(toml).unwrap();
        assert!(field.environment_context.is_some());
        let context = field.environment_context.unwrap();
        assert!(context.contains("Python 3.11+"));
        assert!(context.contains("pytest"));
    }

    #[test]
    fn test_relative_path_resolution() {
        use tempfile::TempDir;

        // Create temp directory structure
        let temp_dir = TempDir::new().unwrap();
        let field_path = temp_dir.path().join("test_field.toml");

        let toml = r#"
name = "test"
goal = "test"

[model]
name = "test"

[environment]
type = "local"
root = "./workspace"
        "#;

        std::fs::write(&field_path, toml).unwrap();

        let field = parse_field_from_file(&field_path).unwrap();

        // Root should be resolved to temp_dir/workspace
        let Environment::Local { root } = &field.environment;
        let expected = temp_dir.path().join("workspace");
        assert_eq!(root, &expected.to_string_lossy().to_string());

        // config_dir should be set
        assert_eq!(field.config_dir, Some(temp_dir.path().to_path_buf()));
    }

    #[test]
    fn test_absolute_path_unchanged() {
        let toml = r#"
name = "test"
goal = "test"

[model]
name = "test"

[environment]
type = "local"
root = "/absolute/path/workspace"
        "#;

        let field = parse_field_from_str(toml).unwrap();

        let Environment::Local { root } = &field.environment;
        assert_eq!(root, "/absolute/path/workspace");

        // config_dir should be None for string parsing
        assert_eq!(field.config_dir, None);
    }

    #[test]
    fn test_python_script_path_resolution() {
        use std::io::Write;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let tools_dir = temp_dir.path().join("tools");
        std::fs::create_dir(&tools_dir).unwrap();

        // Create a simple Python tool
        let script_path = tools_dir.join("calc.py");
        let mut script_file = std::fs::File::create(&script_path).unwrap();
        script_file
            .write_all(
                b"def main(x: int) -> int:
    \"\"\"Test function\"\"\"
    return x * 2
",
            )
            .unwrap();

        let field_path = temp_dir.path().join("field.toml");
        let toml = r#"
name = "test"
goal = "test"

[model]
name = "test"

[environment]
type = "local"
root = "./workspace"

[[tool]]
type = "python"
script = "./tools/calc.py"
function = "main"
        "#;

        std::fs::write(&field_path, toml).unwrap();

        let field = parse_field_from_file(&field_path).unwrap();

        // Should have auto-discovered the Python tool
        assert_eq!(field.custom_tools.len(), 1);
        assert_eq!(field.custom_tools[0].name, "main");

        // Script path should be resolved
        let script = field.custom_tools[0].script.as_ref().unwrap();
        let expected_path = temp_dir.path().join("tools").join("calc.py");
        assert_eq!(script, &expected_path.to_string_lossy().to_string());
    }

    #[test]
    fn test_string_parsing_fallback_to_cwd() {
        // When parsing from string (no file path), relative paths should be relative to CWD
        let toml = r#"
name = "test"
goal = "test"

[model]
name = "test"

[environment]
type = "local"
root = "./workspace"
        "#;

        let field = parse_field_from_str(toml).unwrap();

        // Should use relative path as-is (will be resolved from CWD at runtime)
        let Environment::Local { root } = &field.environment;
        assert_eq!(root, "./workspace");
    }
}
