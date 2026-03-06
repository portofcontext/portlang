use crate::error::{FieldParseError, Result};
use crate::raw::*;
use crate::validation::*;
use portlang_core::*;
use std::fs;
use std::path::Path;

/// Parse a field from a TOML file
pub fn parse_field_from_file(path: impl AsRef<Path>) -> Result<Field> {
    let content = fs::read_to_string(path)?;
    parse_field_from_str(&content)
}

/// Parse a field from a TOML string
pub fn parse_field_from_str(toml_str: &str) -> Result<Field> {
    let raw: RawField = toml::from_str(toml_str)?;
    convert_raw_field(raw)
}

/// Convert raw field to validated field
fn convert_raw_field(raw: RawField) -> Result<Field> {
    // Parse model
    let model = ModelSpec {
        name: raw.model.name,
        temperature: raw.model.temperature,
        max_tokens: raw.model.max_tokens,
    };

    // Parse environment
    let environment = match raw.environment {
        RawEnvironment::Local { root } => Environment::Local { root },
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
                    let script_path = raw_tool.script.as_ref().ok_or_else(|| {
                        FieldParseError::InvalidField(
                            "Python tool missing 'script' field for auto-discovery".to_string(),
                        )
                    })?;

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
                            .extract_function(Path::new(script_path), function_name)
                            .map_err(|e| {
                                FieldParseError::InvalidField(format!(
                                    "Failed to extract function '{}' from {}: {}",
                                    function_name, script_path, e
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
                            script: Some(script_path.clone()),
                            function: Some(tool_meta.name),
                            input_schema: raw_tool.input_schema.unwrap_or(tool_meta.input_schema),
                        });
                    } else {
                        // Extract all functions from file
                        let tools =
                            extractor
                                .extract_tools(Path::new(script_path))
                                .map_err(|e| {
                                    FieldParseError::InvalidField(format!(
                                        "Failed to extract tools from {}: {}",
                                        script_path, e
                                    ))
                                })?;

                        for tool_meta in tools {
                            custom_tools.push(CustomTool {
                                name: tool_meta.name.clone(),
                                description: tool_meta.description.unwrap_or_default(),
                                tool_type: "python".to_string(),
                                command: None,
                                script: Some(script_path.clone()),
                                function: Some(tool_meta.name),
                                input_schema: tool_meta.input_schema,
                            });
                        }
                    }
                } else {
                    // Manually defined - use provided values
                    custom_tools.push(CustomTool {
                        name: raw_tool.name.unwrap_or_default(),
                        description: raw_tool.description.unwrap_or_default(),
                        tool_type: raw_tool.tool_type,
                        command: raw_tool.command,
                        script: raw_tool.script,
                        function: raw_tool.function,
                        input_schema: raw_tool.input_schema.unwrap_or(serde_json::json!({})),
                    });
                }
            }
            "shell" => {
                // Shell tools require manual definition
                custom_tools.push(CustomTool {
                    name: raw_tool.name.ok_or_else(|| {
                        FieldParseError::InvalidField("Shell tool missing 'name' field".to_string())
                    })?,
                    description: raw_tool.description.unwrap_or_default(),
                    tool_type: raw_tool.tool_type,
                    command: raw_tool.command,
                    script: raw_tool.script,
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
}
