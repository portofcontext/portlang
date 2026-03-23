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

/// Resolved parent config from a parent field file (suite-level template)
#[derive(Debug, Clone)]
pub struct ParentConfig {
    pub model: Option<RawModel>,
    pub tools: Vec<RawTool>,
    pub skills: Vec<RawSkill>,
    pub boundary: Option<RawBoundary>,
    pub code_mode_enabled: Option<bool>,
    /// Resolved absolute path to the parent's dockerfile, if any
    pub dockerfile: Option<String>,
}

/// Parse a parent field file (the suite-level template at the eval directory root).
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
            "Failed to parse parent field at {}: {}",
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

    // Resolve the parent's dockerfile path relative to the parent dir
    let dockerfile = raw
        .environment
        .as_ref()
        .and_then(|e| e.dockerfile.as_ref())
        .map(|df| {
            if let Some(ref dir) = parent_dir {
                normalize_path(&dir.join(df)).to_string_lossy().into_owned()
            } else {
                df.clone()
            }
        });

    Ok(Some(ParentConfig {
        model: raw.model,
        tools,
        skills: raw.skill,
        boundary: raw.boundary,
        code_mode_enabled: raw.code_mode.map(|cm| cm.enabled),
        dockerfile,
    }))
}

/// Resolve the parent config for a field path:
/// 1. If `explicit_parent` is provided, use it.
/// 2. Otherwise, auto-detect from a `.field` file (or `field.toml`) one directory up.
pub fn resolve_parent_config(
    field_path: impl AsRef<Path>,
    explicit_parent: Option<impl AsRef<Path>>,
) -> Result<Option<ParentConfig>> {
    if let Some(p) = explicit_parent {
        return parse_parent_config(p);
    }

    // Auto-detect: look for a .field or field.toml one directory up
    let field_path = field_path.as_ref();
    let abs = if field_path.is_absolute() {
        field_path.to_path_buf()
    } else {
        std::env::current_dir()?.join(field_path)
    };

    if let Some(parent_dir) = abs.parent().and_then(|d| d.parent()) {
        // Prefer parent.field (canonical name), then field.field, then field.toml
        let candidate_parent = parent_dir.join("parent.field");
        if candidate_parent.exists() {
            return parse_parent_config(candidate_parent);
        }
        let candidate_field = parent_dir.join("field.field");
        if candidate_field.exists() {
            return parse_parent_config(candidate_field);
        }
        let candidate_toml = parent_dir.join("field.toml");
        if candidate_toml.exists() {
            return parse_parent_config(candidate_toml);
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

        let dockerfile = match raw_env.dockerfile.as_deref() {
            Some("inherit") => parent.and_then(|p| p.dockerfile.clone()),
            Some(path) => Some(resolve_path(path).to_string_lossy().into_owned()),
            None => None,
        };

        Environment {
            root: resolved.to_string_lossy().to_string(),
            packages: raw_env.packages,
            dockerfile,
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
                Some(s) if s.starts_with("on_tool:") => {
                    VerifierTrigger::OnTool(s[8..].to_string())
                }
                Some(other) => {
                    return Err(FieldParseError::InvalidField(format!(
                        "Invalid verifier trigger: {}. Must be 'always', 'on_stop', or 'on_tool:<tool_name>'",
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
                "tool_call" => {
                    VerifierAlgorithm::ToolCall {
                        tool: raw_verifier.tool,
                        field: raw_verifier.field,
                        matches: raw_verifier.matches,
                        not_matches: raw_verifier.not_matches,
                    }
                }
                other => {
                    return Err(FieldParseError::InvalidField(format!(
                        "Verifier '{}': unknown type '{}'. Must be 'shell', 'levenshtein', 'semantic', or 'tool_call'",
                        raw_verifier.name, other
                    )))
                }
            };

            Ok(Verifier {
                name: raw_verifier.name,
                algorithm,
                trigger,
                description: raw_verifier.description,
                eval_only: raw_verifier.eval_only,
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

                        // Default name: explicit > function name.
                        let default_name = tool_meta.name.clone();
                        tools.push(Tool {
                            tool_type: "python".to_string(),
                            name: Some(raw_tool.name.unwrap_or(default_name)),
                            description: Some(
                                raw_tool
                                    .description
                                    .or(tool_meta.description)
                                    .unwrap_or_default(),
                            ),
                            file: Some(file_path.to_string_lossy().to_string()),
                            function: Some(tool_meta.name),
                            input_schema: raw_tool.input_schema.unwrap_or(tool_meta.input_schema),
                            output_schema: raw_tool.output_schema.or(tool_meta.output_schema),
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
                                output_schema: tool_meta.output_schema,
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

    // Auto-detect required container packages based on tool types.
    // All package inference lives here so there's one place to look.
    let mut auto_packages: Vec<String> = Vec::new();
    let needs_uv = |pkgs: &Vec<String>| !pkgs.contains(&"uv".to_string());
    for tool in &tools {
        match tool.tool_type.as_str() {
            // Python tools execute via uv, which bundles its own Python runtime.
            "python" => {
                if needs_uv(&auto_packages) {
                    auto_packages.push("uv".to_string());
                }
            }
            "mcp" => {
                if let Some(McpTransport::Stdio { command, .. }) = &tool.transport {
                    if command == "npx" || command.ends_with("/npx") {
                        if !auto_packages.contains(&"nodejs".to_string()) {
                            auto_packages.push("nodejs".to_string());
                        }
                        if !auto_packages.contains(&"npm".to_string()) {
                            auto_packages.push("npm".to_string());
                        }
                    } else if (command == "uvx" || command.ends_with("/uvx"))
                        && needs_uv(&auto_packages)
                    {
                        auto_packages.push("uv".to_string());
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

    // Parse [vars] declarations
    let vars: std::collections::HashMap<String, VarDecl> = raw
        .vars
        .into_iter()
        .map(|(k, v)| {
            (
                k,
                VarDecl {
                    required: v.required,
                    default: v.default,
                    description: v.description,
                },
            )
        })
        .collect();

    // Resolve skill list — inherit from parent or use field's own [[skill]] entries
    let raw_skills: Vec<RawSkill> = if raw.skills.is_some() {
        parent
            .map(|p| p.skills.clone())
            .unwrap_or_default()
            .into_iter()
            .chain(raw.skill)
            .collect()
    } else {
        raw.skill
    };

    let config_dir_for_skills = config_dir
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let skills: Vec<Skill> = raw_skills
        .into_iter()
        .map(|rs| {
            let (kind, slug) =
                portlang_core::skill::parse_skill_source(&rs.source, &config_dir_for_skills)
                    .map_err(FieldParseError::InvalidField)?;
            Ok(Skill {
                source: rs.source,
                kind,
                slug,
                content: None,
                resources: Vec::new(),
            })
        })
        .collect::<Result<Vec<_>>>()?;

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
        skills,
        verifiers,
        vars,
        config_dir,
    })
}

/// Interpolate `{{ var_name }}` placeholders in a string using the provided variable map.
/// Returns an error if an unresolved placeholder is found.
fn interpolate(s: &str, vars: &std::collections::HashMap<String, String>) -> Result<String> {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        // Look for "{{"
        if i + 1 < len && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            // Find the closing "}}"
            let start = i + 2;
            let mut j = start;
            loop {
                if j + 1 >= len {
                    // No closing "}}"; treat as literal
                    result.push_str(&s[i..]);
                    return Ok(result);
                }
                if bytes[j] == b'}' && bytes[j + 1] == b'}' {
                    break;
                }
                j += 1;
            }
            let var_name = s[start..j].trim();
            match vars.get(var_name) {
                Some(val) => result.push_str(val),
                None => {
                    return Err(FieldParseError::InvalidField(format!(
                        "Template variable '{{{{ {} }}}}' is not defined. Supply it with --var {}=<value>",
                        var_name, var_name
                    )))
                }
            }
            i = j + 2; // skip past "}}"
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    Ok(result)
}

/// Apply runtime context to a field: validate vars, apply defaults, interpolate templates.
/// Call this after `parse_field_with_parent` and before running the field.
pub fn apply_runtime_context(mut field: Field, ctx: &RuntimeContext) -> Result<Field> {
    // Build effective vars: declared defaults first, then runtime overrides
    let mut effective: std::collections::HashMap<String, String> = field
        .vars
        .iter()
        .filter_map(|(k, decl)| decl.default.as_ref().map(|d| (k.clone(), d.clone())))
        .collect();
    for (k, v) in &ctx.vars {
        effective.insert(k.clone(), v.clone());
    }

    // Validate: collect all missing required vars
    let mut missing: Vec<String> = Vec::new();
    for (name, decl) in &field.vars {
        if decl.required && !effective.contains_key(name) {
            missing.push(name.clone());
        }
    }
    if !missing.is_empty() {
        missing.sort();
        return Err(FieldParseError::InvalidField(format!(
            "Missing required template variable(s): {}. Supply with --var key=value.",
            missing.join(", ")
        )));
    }

    // If no vars to substitute, return early
    if effective.is_empty() {
        return Ok(field);
    }

    // Interpolate all string fields
    field.prompt.goal = interpolate(&field.prompt.goal, &effective)?;
    if let Some(ref s) = field.prompt.system.clone() {
        field.prompt.system = Some(interpolate(s, &effective)?);
    }
    let re_obs: Result<Vec<String>> = field
        .prompt
        .re_observation
        .iter()
        .map(|cmd| interpolate(cmd, &effective))
        .collect();
    field.prompt.re_observation = re_obs?;

    // Interpolate environment root
    field.environment.root = interpolate(&field.environment.root, &effective)?;

    // Interpolate tool descriptions, file paths, and shell tool commands
    for tool in &mut field.tools {
        if let Some(ref d) = tool.description.clone() {
            tool.description = Some(interpolate(d, &effective)?);
        }
        if let Some(ref f) = tool.file.clone() {
            tool.file = Some(interpolate(f, &effective)?);
        }
        if tool.tool_type == "shell" {
            if let Some(ref cmd) = tool.command.clone() {
                tool.command = Some(interpolate(cmd, &effective)?);
            }
        }
    }

    // Interpolate shell verifier commands
    for verifier in &mut field.verifiers {
        if let VerifierAlgorithm::Shell { ref mut command } = verifier.algorithm {
            *command = interpolate(command, &effective)?;
        }
    }

    Ok(field)
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
        // output_schema extracted from return type hints
        assert_eq!(
            field.tools[0].output_schema,
            Some(serde_json::json!({"type": "string"}))
        );
        assert_eq!(
            field.tools[1].output_schema,
            Some(serde_json::json!({"type": "integer"}))
        );
    }

    #[test]
    fn test_python_specific_function_output_schema() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut python_file = NamedTempFile::new().unwrap();
        python_file
            .write_all(
                b"
def calculate(x: int, y: int) -> float:
    \"\"\"Calculate something.\"\"\"
    return float(x + y)
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
            goal = "Test specific function output schema"

            [[tool]]
            type = "python"
            file = "{}"
            function = "calculate"
        "#,
            python_path
        );

        let field = parse_field_from_str(&toml).unwrap();
        assert_eq!(field.tools.len(), 1);
        assert_eq!(field.tools[0].name.as_deref(), Some("calculate"));
        // output_schema extracted from return type -> float
        assert_eq!(
            field.tools[0].output_schema,
            Some(serde_json::json!({"type": "number"}))
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
            skills: vec![],
            boundary: None,
            code_mode_enabled: None,
            dockerfile: None,
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
            skills: vec![],
            boundary: None,
            code_mode_enabled: None,
            dockerfile: None,
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
            skills: vec![],
            boundary: None,
            code_mode_enabled: Some(true),
            dockerfile: None,
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
        assert!(raw.code_mode.unwrap().enabled);
        assert_eq!(raw.tool.len(), 1);
    }

    #[test]
    fn test_eval_only_verifier_parsed_and_filtered() {
        let toml = r#"
            name = "test-field"

            [model]
            name = "anthropic/claude-sonnet-4-6"

            [prompt]
            goal = "Do something"

            [[verifier]]
            name = "always-runs"
            command = "true"

            [[verifier]]
            name = "eval-grade"
            type = "levenshtein"
            expected = "hello"
            eval_only = true
        "#;

        let field = parse_field_from_str(toml).unwrap();

        // Both verifiers are present on the parsed field
        assert_eq!(field.verifiers.len(), 2);
        assert!(!field.verifiers[0].eval_only);
        assert!(field.verifiers[1].eval_only);

        // Simulating what `portlang run` does before passing to run_field()
        let run_verifiers: Vec<_> = field
            .verifiers
            .iter()
            .filter(|v| !v.eval_only)
            .cloned()
            .collect();
        assert_eq!(run_verifiers.len(), 1);
        assert_eq!(run_verifiers[0].name, "always-runs");

        // Simulating what `portlang eval run` does — no filtering, all verifiers present
        let eval_verifiers: Vec<_> = field.verifiers.iter().cloned().collect();
        assert_eq!(eval_verifiers.len(), 2);
        assert_eq!(eval_verifiers[1].name, "eval-grade");
    }

    // -----------------------------------------------------------------------
    // Skills parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_field_with_no_skills_has_empty_vec() {
        let toml = r#"
            name = "no-skills"
            [model]
            name = "anthropic/claude-sonnet-4-6"
            [prompt]
            goal = "Do something"
        "#;
        let field = parse_field_from_str(toml).unwrap();
        assert!(field.skills.is_empty());
    }

    #[test]
    fn test_single_github_shorthand_skill() {
        let toml = r#"
            name = "test"
            [model]
            name = "anthropic/claude-sonnet-4-6"
            [prompt]
            goal = "Do something"
            [[skill]]
            source = "owner/my-skill"
        "#;
        let field = parse_field_from_str(toml).unwrap();
        assert_eq!(field.skills.len(), 1);
        let skill = &field.skills[0];
        assert_eq!(skill.source, "owner/my-skill");
        assert_eq!(skill.slug, "my-skill");
        assert!(
            skill.content.is_none(),
            "content should not be resolved at parse time"
        );
        assert!(
            matches!(
                &skill.kind,
                portlang_core::SkillSourceKind::GitHub { owner, repo, .. }
                if owner == "owner" && repo == "my-skill"
            ),
            "expected GitHub kind, got {:?}",
            skill.kind
        );
    }

    #[test]
    fn test_multiple_skills_different_sources() {
        let toml = r#"
            name = "multi-skill"
            [model]
            name = "anthropic/claude-sonnet-4-6"
            [prompt]
            goal = "Do something"
            [[skill]]
            source = "owner/skill-one"
            [[skill]]
            source = "clawhub:my-formatter"
            [[skill]]
            source = "https://example.com"
        "#;
        let field = parse_field_from_str(toml).unwrap();
        assert_eq!(field.skills.len(), 3);
        assert_eq!(field.skills[0].slug, "skill-one");
        assert_eq!(field.skills[1].slug, "my-formatter");
        assert!(matches!(
            field.skills[1].kind,
            portlang_core::SkillSourceKind::ClawHub { .. }
        ));
        assert!(matches!(
            field.skills[2].kind,
            portlang_core::SkillSourceKind::WellKnown { .. }
        ));
    }

    #[test]
    fn test_skills_inherit_from_parent() {
        let parent = ParentConfig {
            model: Some(RawModel {
                name: "anthropic/claude-sonnet-4-6".to_string(),
                temperature: None,
            }),
            tools: vec![],
            skills: vec![
                RawSkill {
                    source: "parent/skill-a".to_string(),
                },
                RawSkill {
                    source: "clawhub:shared-skill".to_string(),
                },
            ],
            boundary: None,
            code_mode_enabled: None,
            dockerfile: None,
        };

        let toml = r#"
            name = "child"
            model = "inherit"
            skills = "inherit"
            [prompt]
            goal = "Do something"
        "#;

        let raw: RawField = toml::from_str(toml).unwrap();
        let field = convert_raw_field(raw, None, Some(&parent)).unwrap();

        assert_eq!(
            field.skills.len(),
            2,
            "child should inherit both parent skills"
        );
        assert_eq!(field.skills[0].slug, "skill-a");
        assert_eq!(field.skills[1].slug, "shared-skill");
    }

    #[test]
    fn test_skills_inherit_appends_to_child_skills() {
        // When `skills = "inherit"`, parent skills are prepended; child [[skill]] entries follow
        let parent = ParentConfig {
            model: Some(RawModel {
                name: "anthropic/claude-sonnet-4-6".to_string(),
                temperature: None,
            }),
            tools: vec![],
            skills: vec![RawSkill {
                source: "parent/base-skill".to_string(),
            }],
            boundary: None,
            code_mode_enabled: None,
            dockerfile: None,
        };

        let toml = r#"
            name = "child"
            model = "inherit"
            skills = "inherit"
            [prompt]
            goal = "Do something"
            [[skill]]
            source = "child/extra-skill"
        "#;

        let raw: RawField = toml::from_str(toml).unwrap();
        let field = convert_raw_field(raw, None, Some(&parent)).unwrap();

        assert_eq!(field.skills.len(), 2);
        assert_eq!(
            field.skills[0].slug, "base-skill",
            "parent skill should come first"
        );
        assert_eq!(
            field.skills[1].slug, "extra-skill",
            "child skill should follow"
        );
    }

    #[test]
    fn test_skills_without_inherit_ignores_parent() {
        let parent = ParentConfig {
            model: Some(RawModel {
                name: "anthropic/claude-sonnet-4-6".to_string(),
                temperature: None,
            }),
            tools: vec![],
            skills: vec![RawSkill {
                source: "parent/should-not-appear".to_string(),
            }],
            boundary: None,
            code_mode_enabled: None,
            dockerfile: None,
        };

        let toml = r#"
            name = "child"
            model = "inherit"
            [prompt]
            goal = "Do something"
            [[skill]]
            source = "child/own-skill"
        "#;

        let raw: RawField = toml::from_str(toml).unwrap();
        let field = convert_raw_field(raw, None, Some(&parent)).unwrap();

        assert_eq!(
            field.skills.len(),
            1,
            "should not pick up parent skills without inherit"
        );
        assert_eq!(field.skills[0].slug, "own-skill");
    }

    #[test]
    fn test_local_skill_path_resolved_from_config_dir() {
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let skill_file = dir.path().join("my-skill.md");
        std::fs::write(&skill_file, "# My Skill\nDo stuff.").unwrap();

        let field_path = dir.path().join("test.field");
        let toml = r#"
            name = "local-skill-test"
            [model]
            name = "anthropic/claude-sonnet-4-6"
            [prompt]
            goal = "test"
            [[skill]]
            source = "./my-skill.md"
            "#
        .to_string();
        std::fs::write(&field_path, &toml).unwrap();

        let field = parse_field_with_parent(&field_path, None).unwrap();
        assert_eq!(field.skills.len(), 1);
        let skill = &field.skills[0];
        assert_eq!(skill.slug, "my-skill");
        match &skill.kind {
            portlang_core::SkillSourceKind::Local { path } => {
                assert!(path.is_absolute());
                assert_eq!(path, &skill_file);
            }
            other => panic!("expected Local, got {:?}", other),
        }
    }

    #[test]
    fn test_invalid_skill_source_fails_parse() {
        let toml = r#"
            name = "bad-skill"
            [model]
            name = "anthropic/claude-sonnet-4-6"
            [prompt]
            goal = "test"
            [[skill]]
            source = "not-a-valid-source"
        "#;
        // "not-a-valid-source" has no slash — owner/repo requires at least one slash
        let result = parse_field_from_str(toml);
        assert!(
            result.is_err(),
            "a skill with no slash should fail to parse"
        );
    }
}
