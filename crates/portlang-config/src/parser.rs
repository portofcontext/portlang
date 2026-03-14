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

/// Resolved parent config from a parent field.toml (suite-level template)
#[derive(Debug, Clone)]
pub struct ParentConfig {
    pub model: Option<RawModel>,
    pub tools: Vec<RawTool>,
    pub boundary: Option<RawBoundary>,
    pub code_mode_enabled: Option<bool>,
}

/// Parse a parent field.toml (the suite-level template at the eval directory root).
/// Returns None if the file does not exist.
pub fn parse_parent_config(path: impl AsRef<Path>) -> Result<Option<ParentConfig>> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(None);
    }

    let abs_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| FieldParseError::InvalidField(e.to_string()))?
            .join(path)
    };
    let parent_dir = abs_path.parent().map(|p| p.to_path_buf());

    let content = fs::read_to_string(path)?;
    let raw: RawParentConfig = toml::from_str(&content).map_err(|e| {
        FieldParseError::InvalidField(format!(
            "Failed to parse parent field.toml at {}: {}",
            path.display(),
            e
        ))
    })?;

    // Resolve patch_file paths relative to the parent field.toml's directory so
    // that children inheriting these tools don't re-resolve them from their own dirs.
    let tools = raw
        .tool
        .into_iter()
        .map(|mut t| {
            if let (Some(pf), Some(ref dir)) = (t.patch_file.as_ref(), &parent_dir) {
                let resolved = normalize_path(&dir.join(pf));
                t.patch_file = Some(resolved.to_string_lossy().into_owned());
            }
            t
        })
        .collect();

    Ok(Some(ParentConfig {
        model: raw.model,
        tools,
        boundary: raw.boundary,
        code_mode_enabled: raw.code_mode.map(|cm| cm.enabled),
    }))
}

/// Resolve the parent config for a field path:
/// 1. If `explicit_parent` is provided, use it.
/// 2. Otherwise, auto-detect from `../field.toml` relative to the field file.
pub fn resolve_parent_config(
    field_path: impl AsRef<Path>,
    explicit_parent: Option<impl AsRef<Path>>,
) -> Result<Option<ParentConfig>> {
    if let Some(p) = explicit_parent {
        return parse_parent_config(p);
    }

    // Auto-detect: look for field.toml one directory up
    let field_path = field_path.as_ref();
    let abs = if field_path.is_absolute() {
        field_path.to_path_buf()
    } else {
        std::env::current_dir()?.join(field_path)
    };

    if let Some(parent_dir) = abs.parent().and_then(|d| d.parent()) {
        let candidate = parent_dir.join("field.toml");
        if candidate.exists() {
            return parse_parent_config(candidate);
        }
    }

    Ok(None)
}

/// Parse a field from a TOML file, resolving any "inherit" values from a parent config.
pub fn parse_field_with_parent(
    path: impl AsRef<Path>,
    parent: Option<&ParentConfig>,
) -> Result<Field> {
    let path = path.as_ref();
    let content = fs::read_to_string(path)?;

    let abs_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };

    let config_dir = abs_path.parent().map(|p: &Path| p.to_path_buf());

    let raw: RawField = toml::from_str(&content)?;
    convert_raw_field(raw, config_dir, parent)
}

/// Parse a field from a TOML file
pub fn parse_field_from_file(path: impl AsRef<Path>) -> Result<Field> {
    parse_field_with_parent(path, None)
}

/// Parse a field from a TOML string
pub fn parse_field_from_str(toml_str: &str) -> Result<Field> {
    parse_field_from_str_with_context(toml_str, None)
}

/// Parse a field from a TOML string with config directory context
fn parse_field_from_str_with_context(toml_str: &str, config_dir: Option<PathBuf>) -> Result<Field> {
    let raw: RawField = toml::from_str(toml_str)?;
    convert_raw_field(raw, config_dir, None)
}

/// Parse cost from StringOrNumber
pub fn parse_cost(cost: &StringOrNumber) -> Result<Cost> {
    match cost {
        StringOrNumber::String(s) => {
            // Parse "$X.XX" format
            let s = s.trim();
            let s = s.strip_prefix('$').unwrap_or(s);
            s.parse::<f64>()
                .map(Cost::from_dollars)
                .map_err(|_| FieldParseError::InvalidField(format!("Invalid cost: {}", s)))
        }
        StringOrNumber::Number(n) => Ok(Cost::from_dollars(*n)),
    }
}

/// Convert raw field to validated field, resolving "inherit" values from parent
fn convert_raw_field(
    raw: RawField,
    config_dir: Option<PathBuf>,
    parent: Option<&ParentConfig>,
) -> Result<Field> {
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

    // Resolve model — from field, parent, or error
    let field_name = &raw.name;
    let raw_model = match raw.model {
        None => {
            // Not specified — require parent
            parent
                .and_then(|p| p.model.clone())
                .ok_or_else(|| {
                    FieldParseError::InvalidField(format!(
                        "Field \"{}\" has no [model]. Add [model] to field.toml or to the parent field.toml.",
                        field_name
                    ))
                })?
        }
        Some(InheritOr::Inherit(_)) => {
            // Explicitly inherit from parent
            parent.and_then(|p| p.model.clone()).ok_or_else(|| {
                FieldParseError::InvalidField(format!(
                    "Field \"{}\" uses `model = \"inherit\"` but no parent field.toml was found.",
                    field_name
                ))
            })?
        }
        Some(InheritOr::Value(m)) => m,
    };

    let model = ModelSpec {
        name: raw_model.name,
        temperature: raw_model.temperature,
    };

    // Parse environment with path resolution
    let environment = if let Some(raw_env) = raw.environment {
        let resolved = resolve_path(&raw_env.root);

        // Resolve code_mode_enabled: prioritize environment field, fall back to [code_mode] section for backwards compatibility
        let code_mode_enabled = match raw_env.code_mode_enabled {
            Some(InheritOr::Value(v)) => Some(v),
            Some(InheritOr::Inherit(_)) => parent.and_then(|p| p.code_mode_enabled),
            None => raw
                .code_mode
                .as_ref()
                .and_then(|cm| match cm {
                    InheritOr::Inherit(_) => parent.and_then(|p| p.code_mode_enabled),
                    InheritOr::Value(cm) => Some(cm.enabled),
                })
                .or_else(|| parent.and_then(|p| p.code_mode_enabled)),
        };

        Environment {
            root: resolved.to_string_lossy().to_string(),
            packages: raw_env.packages,
            dockerfile: raw_env.dockerfile,
            image: raw_env.image,
            code_mode_enabled,
        }
    } else {
        // No environment specified, use defaults
        let code_mode_enabled = match raw.code_mode {
            None => parent.and_then(|p| p.code_mode_enabled),
            Some(InheritOr::Inherit(_)) => parent.and_then(|p| p.code_mode_enabled),
            Some(InheritOr::Value(cm)) => Some(cm.enabled),
        };

        Environment {
            code_mode_enabled,
            ..Environment::default()
        }
    };

    // Resolve boundary — field, inherit from parent, or default
    let raw_boundary_opt: Option<RawBoundary> = match raw.boundary {
        None => None,
        Some(InheritOr::Inherit(_)) => parent.and_then(|p| p.boundary.clone()),
        Some(InheritOr::Value(b)) => Some(b),
    };

    // Parse boundary
    let boundary = if let Some(raw_boundary) = raw_boundary_opt {
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

        let max_cost = raw_boundary.max_cost.as_ref().map(parse_cost).transpose()?;

        Boundary {
            allow_write: raw_boundary.allow_write,
            network,
            max_tokens: raw_boundary.max_tokens,
            max_cost,
            max_steps: raw_boundary.max_steps,
            bash: raw_boundary.bash,
            output_schema: raw_boundary.output_schema,
        }
    } else {
        Boundary::default()
    };

    // Parse verifiers
    let verifiers = raw
        .verifier
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

            let algorithm = match raw_verifier.verifier_type.as_str() {
                "shell" | "" => {
                    let command = raw_verifier.command.ok_or_else(|| {
                        FieldParseError::InvalidField(format!(
                            "Verifier '{}': shell verifier requires 'command'",
                            raw_verifier.name
                        ))
                    })?;
                    VerifierAlgorithm::Shell { command }
                }
                "levenshtein" => {
                    let expected = raw_verifier.expected.ok_or_else(|| {
                        FieldParseError::InvalidField(format!(
                            "Verifier '{}': levenshtein verifier requires 'expected'",
                            raw_verifier.name
                        ))
                    })?;
                    VerifierAlgorithm::Levenshtein {
                        file: raw_verifier.file,
                        expected,
                        threshold: raw_verifier.threshold.unwrap_or(1.0),
                    }
                }
                "json" => VerifierAlgorithm::Json {
                    file: raw_verifier.file,
                    schema: raw_verifier.schema,
                },
                "semantic" => {
                    let expected = raw_verifier.expected.ok_or_else(|| {
                        FieldParseError::InvalidField(format!(
                            "Verifier '{}': semantic verifier requires 'expected'",
                            raw_verifier.name
                        ))
                    })?;
                    VerifierAlgorithm::Semantic {
                        file: raw_verifier.file,
                        expected,
                        threshold: raw_verifier.threshold.unwrap_or(0.8),
                        embedding_url: raw_verifier.embedding_url,
                        embedding_model: raw_verifier.embedding_model,
                    }
                }
                other => {
                    return Err(FieldParseError::InvalidField(format!(
                        "Verifier '{}': unknown type '{}'. Must be 'shell', 'levenshtein', 'json', or 'semantic'",
                        raw_verifier.name, other
                    )))
                }
            };

            Ok(Verifier {
                name: raw_verifier.name,
                algorithm,
                trigger,
                description: raw_verifier.description,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    // Resolve tool list — inherit from parent or use field's own [[tool]] entries
    let raw_tools: Vec<RawTool> = if raw.tools.is_some() {
        // `tools = "inherit"` — use parent's tool list
        parent
            .map(|p| p.tools.clone())
            .unwrap_or_default()
            .into_iter()
            .chain(raw.tool)
            .collect()
    } else {
        raw.tool
    };

    // Parse tools
    let mut tools: Vec<Tool> = Vec::new();

    for raw_tool in raw_tools {
        match raw_tool.tool_type.as_str() {
            "python" => {
                // Check if we need to auto-discover from Python file
                let needs_discovery = raw_tool.name.is_none() || raw_tool.input_schema.is_none();

                if needs_discovery {
                    // Must have file path for discovery
                    let file_path_str = raw_tool.file.as_ref().ok_or_else(|| {
                        FieldParseError::InvalidField(
                            "Python tool missing 'file' field for auto-discovery".to_string(),
                        )
                    })?;

                    // Resolve the file path
                    let file_path = resolve_path(file_path_str);

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
                            .extract_function(&file_path, function_name)
                            .map_err(|e| {
                                FieldParseError::InvalidField(format!(
                                    "Failed to extract function '{}' from {}: {}",
                                    function_name,
                                    file_path.display(),
                                    e
                                ))
                            })?;

                        tools.push(Tool {
                            tool_type: "python".to_string(),
                            name: Some(raw_tool.name.unwrap_or(tool_meta.name.clone())),
                            description: Some(
                                raw_tool
                                    .description
                                    .or(tool_meta.description)
                                    .unwrap_or_default(),
                            ),
                            file: Some(file_path.to_string_lossy().to_string()),
                            function: Some(tool_meta.name),
                            input_schema: raw_tool.input_schema.unwrap_or(tool_meta.input_schema),
                            output_schema: raw_tool.output_schema,
                            command: None,
                            args: vec![],
                            env: std::collections::HashMap::new(),
                            url: None,
                            headers: None,
                            transport: None,
                            include_tools: None,
                            exclude_tools: None,
                            patch_file: None,
                        });
                    } else {
                        // Extract all functions from file
                        let tool_metas = extractor.extract_tools(&file_path).map_err(|e| {
                            FieldParseError::InvalidField(format!(
                                "Failed to extract tools from {}: {}",
                                file_path.display(),
                                e
                            ))
                        })?;

                        for tool_meta in tool_metas {
                            tools.push(Tool {
                                tool_type: "python".to_string(),
                                name: Some(tool_meta.name.clone()),
                                description: Some(tool_meta.description.unwrap_or_default()),
                                file: Some(file_path.to_string_lossy().to_string()),
                                function: Some(tool_meta.name),
                                input_schema: tool_meta.input_schema,
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
                            });
                        }
                    }
                } else {
                    // Manually defined - use provided values
                    let resolved_file = raw_tool
                        .file
                        .as_ref()
                        .map(|s| resolve_path(s).to_string_lossy().to_string());

                    tools.push(Tool {
                        tool_type: raw_tool.tool_type,
                        name: Some(raw_tool.name.unwrap_or_default()),
                        description: Some(raw_tool.description.unwrap_or_default()),
                        file: resolved_file,
                        function: raw_tool.function,
                        input_schema: raw_tool.input_schema.unwrap_or(serde_json::json!({})),
                        output_schema: raw_tool.output_schema,
                        command: raw_tool.command,
                        args: raw_tool.args,
                        env: raw_tool.env,
                        url: raw_tool.url,
                        headers: raw_tool.headers,
                        transport: None,
                        include_tools: None,
                        exclude_tools: None,
                        patch_file: None,
                    });
                }
            }
            "shell" => {
                tools.push(Tool {
                    tool_type: raw_tool.tool_type,
                    name: Some(raw_tool.name.ok_or_else(|| {
                        FieldParseError::InvalidField("Shell tool missing 'name' field".to_string())
                    })?),
                    description: Some(raw_tool.description.unwrap_or_default()),
                    file: raw_tool
                        .file
                        .as_ref()
                        .map(|s| resolve_path(s).to_string_lossy().to_string()),
                    function: raw_tool.function,
                    input_schema: raw_tool.input_schema.unwrap_or(serde_json::json!({})),
                    output_schema: raw_tool.output_schema,
                    command: raw_tool.command,
                    args: raw_tool.args,
                    env: raw_tool.env,
                    url: raw_tool.url,
                    headers: raw_tool.headers,
                    transport: None,
                    include_tools: None,
                    exclude_tools: None,
                    patch_file: None,
                });
            }
            "mcp" => {
                // Validate name is non-empty
                let mcp_name = raw_tool.name.ok_or_else(|| {
                    FieldParseError::InvalidField("MCP tool missing 'name' field".to_string())
                })?;

                if mcp_name.trim().is_empty() {
                    return Err(FieldParseError::InvalidField(
                        "MCP tool name cannot be empty".to_string(),
                    ));
                }

                // Determine transport type
                let transport_type = raw_tool.transport.as_deref().unwrap_or("stdio");

                let transport = match transport_type {
                    "stdio" => {
                        // Stdio transport requires command
                        let command = raw_tool.command.ok_or_else(|| {
                            FieldParseError::InvalidField(format!(
                                "MCP tool '{}' with stdio transport requires 'command' field",
                                mcp_name
                            ))
                        })?;

                        if command.trim().is_empty() {
                            return Err(FieldParseError::InvalidField(
                                "MCP tool command cannot be empty".to_string(),
                            ));
                        }

                        McpTransport::Stdio {
                            command,
                            args: raw_tool.args.clone(),
                            env: raw_tool.env.clone(),
                        }
                    }
                    "http" | "sse" => {
                        // HTTP/SSE transport requires url
                        let url = raw_tool.url.ok_or_else(|| {
                            FieldParseError::InvalidField(format!(
                                "MCP tool '{}' with {} transport requires 'url' field",
                                mcp_name, transport_type
                            ))
                        })?;

                        if url.trim().is_empty() {
                            return Err(FieldParseError::InvalidField(
                                "MCP tool url cannot be empty".to_string(),
                            ));
                        }

                        // Expand environment variables in headers
                        let mut headers = raw_tool.headers.clone().unwrap_or_default();
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

                // Validate mutual exclusivity of include_tools and exclude_tools
                if raw_tool.include_tools.is_some() && raw_tool.exclude_tools.is_some() {
                    return Err(FieldParseError::InvalidField(format!(
                        "MCP tool '{}': cannot specify both 'include_tools' and 'exclude_tools'",
                        mcp_name
                    )));
                }

                tools.push(Tool {
                    tool_type: "mcp".to_string(),
                    name: Some(mcp_name),
                    description: raw_tool.description,
                    file: None,
                    function: None,
                    input_schema: raw_tool.input_schema.unwrap_or(serde_json::json!({})),
                    output_schema: raw_tool.output_schema,
                    command: None,
                    args: vec![],
                    env: std::collections::HashMap::new(),
                    url: None,
                    headers: None,
                    transport: Some(transport),
                    include_tools: raw_tool.include_tools,
                    exclude_tools: raw_tool.exclude_tools,
                    patch_file: raw_tool.patch_file,
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

    // Auto-detect packages based on tools
    let mut auto_packages: Vec<String> = Vec::new();
    for tool in &tools {
        match tool.tool_type.as_str() {
            "python" => {
                if !auto_packages.contains(&"python3".to_string()) {
                    auto_packages.push("python3".to_string());
                }
            }
            "mcp" => {
                // Check MCP command for package hints
                if let Some(transport) = &tool.transport {
                    match transport {
                        McpTransport::Stdio { command, .. } => {
                            if command == "npx" || command.ends_with("/npx") {
                                if !auto_packages.contains(&"nodejs".to_string()) {
                                    auto_packages.push("nodejs".to_string());
                                }
                                if !auto_packages.contains(&"npm".to_string()) {
                                    auto_packages.push("npm".to_string());
                                }
                            } else if command == "uvx" || command.ends_with("/uvx") {
                                if !auto_packages.contains(&"python3".to_string()) {
                                    auto_packages.push("python3".to_string());
                                }
                                if !auto_packages.contains(&"uv".to_string()) {
                                    auto_packages.push("uv".to_string());
                                }
                            }
                        }
                        McpTransport::Sse { .. } => {}
                    }
                }
            }
            _ => {}
        }
    }

    // Merge auto-packages into environment packages (environment packages take precedence)
    let mut final_packages = environment.packages.clone();
    for pkg in auto_packages {
        if !final_packages.contains(&pkg) {
            final_packages.push(pkg);
        }
    }

    let environment = Environment {
        root: environment.root,
        packages: final_packages,
        dockerfile: environment.dockerfile,
        image: environment.image,
        code_mode_enabled: environment.code_mode_enabled,
    };

    Ok(Field {
        name: raw.name,
        description: raw.description,
        model,
        prompt: Prompt {
            goal: raw.prompt.goal,
            system: raw.prompt.system,
            re_observation: raw.prompt.re_observation,
        },
        environment,
        boundary,
        tools,
        verifiers,
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

            [model]
            name = "claude-sonnet-4-5"

            [prompt]
            goal = "Do something"
        "#;

        let field = parse_field_from_str(toml).unwrap();
        assert_eq!(field.name, "test-field");
        assert_eq!(field.prompt.goal, "Do something");
        assert_eq!(field.model.name, "claude-sonnet-4-5");
    }

    #[test]
    fn test_parse_cost_string() {
        let toml = r#"
            name = "test-field"

            [model]
            name = "claude-sonnet-4-5"

            [prompt]
            goal = "Do something"

            [boundary]
            max_cost = "$2.50"
        "#;

        let field = parse_field_from_str(toml).unwrap();
        assert_eq!(field.boundary.max_cost.unwrap().to_dollars(), 2.5);
    }

    #[test]
    fn test_parse_cost_number() {
        let toml = r#"
            name = "test-field"

            [model]
            name = "claude-sonnet-4-5"

            [prompt]
            goal = "Do something"

            [boundary]
            max_cost = 2.5
        "#;

        let field = parse_field_from_str(toml).unwrap();
        assert_eq!(field.boundary.max_cost.unwrap().to_dollars(), 2.5);
    }

    #[test]
    fn test_reject_unknown_field() {
        let toml = r#"
            name = "test-field"
            unknown_field = "bad"

            [model]
            name = "claude-sonnet-4-5"

            [prompt]
            goal = "Do something"
        "#;

        let result = parse_field_from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_glob_patterns() {
        let toml = r#"
            name = "test-field"

            [model]
            name = "claude-sonnet-4-5"

            [prompt]
            goal = "Do something"

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

            [model]
            name = "claude-sonnet-4-5"

            [prompt]
            goal = "Test auto-discovery"

            [[tool]]
            type = "python"
            file = "{}"
        "#,
            python_path
        );

        let field = parse_field_from_str(&toml).unwrap();

        // Should discover both functions
        assert_eq!(field.tools.len(), 2);

        // Check first tool (greet)
        assert_eq!(field.tools[0].name.as_deref(), Some("greet"));
        assert_eq!(
            field.tools[0].description.as_deref(),
            Some("Greet someone by name.")
        );
        assert_eq!(field.tools[0].tool_type, "python");
        assert_eq!(
            field.tools[0].input_schema["properties"]["name"]["type"],
            "string"
        );
        assert_eq!(
            field.tools[0].input_schema["required"],
            serde_json::json!(["name"])
        );

        // Check second tool (add)
        assert_eq!(field.tools[1].name.as_deref(), Some("add"));
        assert_eq!(
            field.tools[1].description.as_deref(),
            Some("Add two numbers.")
        );
        assert_eq!(
            field.tools[1].input_schema["properties"]["x"]["type"],
            "integer"
        );
        assert_eq!(
            field.tools[1].input_schema["properties"]["y"]["type"],
            "integer"
        );
        // Only x is required, y has default
        assert_eq!(
            field.tools[1].input_schema["required"],
            serde_json::json!(["x"])
        );
    }

    #[test]
    fn test_parse_prompt_system() {
        let toml = r#"
            name = "test-field"

            [model]
            name = "claude-sonnet-4-5"

            [prompt]
            goal = "Do something"
            system = """
Available Tools:
  - Python 3.11+
  - pytest for testing
"""
        "#;

        let field = parse_field_from_str(toml).unwrap();
        assert!(field.prompt.system.is_some());
        let system = field.prompt.system.unwrap();
        assert!(system.contains("Python 3.11+"));
        assert!(system.contains("pytest"));
    }

    #[test]
    fn test_relative_path_resolution() {
        use tempfile::TempDir;

        // Create temp directory structure
        let temp_dir = TempDir::new().unwrap();
        let field_path = temp_dir.path().join("test_field.toml");

        let toml = r#"
name = "test"

[model]
name = "test"

[prompt]
goal = "test"

[environment]
root = "./workspace"
        "#;

        std::fs::write(&field_path, toml).unwrap();

        let field = parse_field_from_file(&field_path).unwrap();

        // Root should be resolved to temp_dir/workspace
        let expected = temp_dir.path().join("workspace");
        assert_eq!(
            field.environment.root,
            expected.to_string_lossy().to_string()
        );

        // config_dir should be set
        assert_eq!(field.config_dir, Some(temp_dir.path().to_path_buf()));
    }

    #[test]
    fn test_absolute_path_unchanged() {
        let toml = r#"
name = "test"

[model]
name = "test"

[prompt]
goal = "test"

[environment]
root = "/absolute/path/workspace"
        "#;

        let field = parse_field_from_str(toml).unwrap();
        assert_eq!(field.environment.root, "/absolute/path/workspace");

        // config_dir should be None for string parsing
        assert_eq!(field.config_dir, None);
    }

    #[test]
    fn test_python_file_path_resolution() {
        use std::io::Write;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let tools_dir = temp_dir.path().join("tools");
        std::fs::create_dir(&tools_dir).unwrap();

        // Create a simple Python tool
        let file_path = tools_dir.join("calc.py");
        let mut file = std::fs::File::create(&file_path).unwrap();
        file.write_all(
            b"def main(x: int) -> int:
    \"\"\"Test function\"\"\"
    return x * 2
",
        )
        .unwrap();

        let field_path = temp_dir.path().join("field.toml");
        let toml = r#"
name = "test"

[model]
name = "test"

[prompt]
goal = "test"

[environment]
root = "./workspace"

[[tool]]
type = "python"
file = "./tools/calc.py"
function = "main"
        "#;

        std::fs::write(&field_path, toml).unwrap();

        let field = parse_field_from_file(&field_path).unwrap();

        // Should have auto-discovered the Python tool
        assert_eq!(field.tools.len(), 1);
        assert_eq!(field.tools[0].name.as_deref(), Some("main"));

        // File path should be resolved
        let file = field.tools[0].file.as_ref().unwrap();
        let expected_path = temp_dir.path().join("tools").join("calc.py");
        assert_eq!(file, &expected_path.to_string_lossy().to_string());
    }

    #[test]
    fn test_string_parsing_fallback_to_cwd() {
        // When parsing from string (no file path), relative paths should be relative to CWD
        let toml = r#"
name = "test"

[model]
name = "test"

[prompt]
goal = "test"

[environment]
root = "./workspace"
        "#;

        let field = parse_field_from_str(toml).unwrap();

        // Should use relative path as-is (will be resolved from CWD at runtime)
        assert_eq!(field.environment.root, "./workspace");
    }

    #[test]
    fn test_inherit_model_from_parent() {
        let parent = ParentConfig {
            model: Some(RawModel {
                name: "anthropic/claude-sonnet-4-6".to_string(),
                temperature: Some(0.5),
            }),
            tools: vec![],
            boundary: None,
            code_mode_enabled: None,
        };

        let toml = r#"
            name = "child-field"
            model = "inherit"

            [prompt]
            goal = "Do something"
        "#;

        let raw: RawField = toml::from_str(toml).unwrap();
        let field = convert_raw_field(raw, None, Some(&parent)).unwrap();

        assert_eq!(field.model.name, "anthropic/claude-sonnet-4-6");
        assert_eq!(field.model.temperature, Some(0.5));
    }

    #[test]
    fn test_inherit_tools_from_parent() {
        let parent = ParentConfig {
            model: Some(RawModel {
                name: "anthropic/claude-sonnet-4-6".to_string(),
                temperature: None,
            }),
            tools: vec![RawTool {
                tool_type: "shell".to_string(),
                name: Some("echo".to_string()),
                description: Some("Echo tool".to_string()),
                command: Some("echo hello".to_string()),
                input_schema: None,
                output_schema: None,
                file: None,
                function: None,
                args: vec![],
                env: std::collections::HashMap::new(),
                url: None,
                headers: None,
                transport: None,
                include_tools: None,
                exclude_tools: None,
                patch_file: None,
            }],
            boundary: None,
            code_mode_enabled: None,
        };

        let toml = r#"
            name = "child-field"
            model = "inherit"
            tools = "inherit"

            [prompt]
            goal = "Do something"
        "#;

        let raw: RawField = toml::from_str(toml).unwrap();
        let field = convert_raw_field(raw, None, Some(&parent)).unwrap();

        assert_eq!(field.tools.len(), 1);
        assert_eq!(field.tools[0].name.as_deref(), Some("echo"));
    }

    #[test]
    fn test_inherit_model_no_parent_errors() {
        let toml = r#"
            name = "child-field"
            model = "inherit"

            [prompt]
            goal = "Do something"
        "#;

        let raw: RawField = toml::from_str(toml).unwrap();
        let result = convert_raw_field(raw, None, None);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("inherit"),
            "Error should mention inherit: {}",
            err
        );
    }

    #[test]
    fn test_no_model_no_parent_errors() {
        let toml = r#"
            name = "child-field"

            [prompt]
            goal = "Do something"
        "#;

        let raw: RawField = toml::from_str(toml).unwrap();
        let result = convert_raw_field(raw, None, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_code_mode_inherited_from_parent() {
        let parent = ParentConfig {
            model: Some(RawModel {
                name: "anthropic/claude-sonnet-4-6".to_string(),
                temperature: None,
            }),
            tools: vec![],
            boundary: None,
            code_mode_enabled: Some(true),
        };

        let toml = r#"
            name = "child-field"
            model = "inherit"

            [prompt]
            goal = "Do something"
        "#;

        let raw: RawField = toml::from_str(toml).unwrap();
        let field = convert_raw_field(raw, None, Some(&parent)).unwrap();

        assert_eq!(field.environment.code_mode_enabled, Some(true));
    }

    #[test]
    fn test_parse_parent_config_toml() {
        let toml = r#"
            [model]
            name = "anthropic/claude-sonnet-4-6"
            temperature = 1.0

            [code_mode]
            enabled = true

            [[tool]]
            type = "shell"
            name = "greet"
            command = "echo hello"
        "#;

        let raw: RawParentConfig = toml::from_str(toml).unwrap();
        assert_eq!(raw.model.unwrap().name, "anthropic/claude-sonnet-4-6");
        assert_eq!(raw.code_mode.unwrap().enabled, true);
        assert_eq!(raw.tool.len(), 1);
    }
}
