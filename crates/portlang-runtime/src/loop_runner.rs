use crate::context_window::ContextWindow;
use crate::environment::EnvironmentContext;
use crate::loop_detection::LoopDetector;
use crate::mcp::{apply_patches, load_patch_map, McpServerManager, McpToolHandler};
use crate::provider::{ContentBlock, ModelProvider, Tool};
use crate::sandbox::{create_sandbox, BoundaryAnalyzer, ContextTracer};
use crate::tools::{
    BashHandler, GlobHandler, PythonToolHandler, ReadHandler, ShellCommandHandler, ToolRegistry,
    WriteHandler,
};

#[cfg(feature = "code-mode")]
use crate::tools::{CodeModeCallback, CodeModeHandler};

use crate::verifier_runner::run_verifiers;
use portlang_core::*;
use std::sync::Arc;
use uuid::Uuid;

/// Generate a unique tool use ID
fn generate_tool_id() -> String {
    format!("toolu_{}", Uuid::new_v4().to_string().replace("-", ""))
}

/// What the agent sees
pub struct AgentView {
    /// Tools available to the agent
    pub tools: Vec<Tool>,
    /// System prompt
    pub system_prompt: String,
    /// Environment context
    pub env_context: EnvironmentContext,
    /// Sandbox (needed internally for execution)
    pub(crate) sandbox: Arc<dyn crate::sandbox::Sandbox>,
}

/// Validate field configuration without starting any servers or running the agent.
///
/// Catches configuration mistakes (e.g. missing patch files) early so that
/// `portlang eval` can abort before wasting time on MCP connections.
pub fn validate_field_config(field: &Field) -> anyhow::Result<()> {
    for tool in field.tools.iter().filter(|t| t.tool_type == "mcp") {
        if tool.patch_file.is_some() {
            load_patch_map(tool.patch_file.as_deref(), field.config_dir.as_deref())
                .map_err(|e| anyhow::anyhow!("{}", e))?;
        }
    }
    Ok(())
}

/// Prepare what the agent will see for a field
/// This handles all tool registration and prompt building
pub async fn prepare_agent_view(field: &Field) -> anyhow::Result<AgentView> {
    tracing::debug!("Preparing agent view for field: {}", field.name);

    // Create tool registry with built-in tools
    let registry = ToolRegistry::new();
    registry.register(Arc::new(ReadHandler));
    registry.register(Arc::new(WriteHandler));
    registry.register(Arc::new(GlobHandler));

    // Register bash tool if enabled in boundary config
    if field.boundary.bash {
        registry.register(Arc::new(BashHandler::new(
            field.boundary.allow_write.clone(),
        )));
        tracing::info!("Registered built-in bash tool");
    }

    // Wrap registry in Arc early so we can share it with sandbox and continue registering tools
    let registry = Arc::new(registry);

    // Create sandbox FIRST so we can get container ID for MCP, Python, and shell tools.
    // All custom tool execution is sandboxed — the container_id is required.
    // Package inference (uv for Python tools, nodejs/npm for npx MCP, etc.) is handled
    // by the config parser before this point.
    let sandbox = create_sandbox(&field.environment, &field.boundary, registry.clone()).await?;
    let container_id = sandbox
        .container_id()
        .ok_or_else(|| anyhow::anyhow!("Sandbox did not provide a container ID"))?
        .to_string();

    // Register custom tools from field config (python and shell types).
    // Both types execute inside the container.
    for tool in field
        .tools
        .iter()
        .filter(|t| t.tool_type == "python" || t.tool_type == "shell")
    {
        match tool.tool_type.as_str() {
            "shell" => {
                let name = tool.name.clone().unwrap_or_default();
                let description = tool.description.clone().unwrap_or_default();
                let handler = ShellCommandHandler::new(
                    name.clone(),
                    description,
                    tool.command.clone().unwrap_or_default(),
                    tool.input_schema.clone(),
                    container_id.clone(),
                );
                registry.register(Arc::new(handler));
                tracing::info!("Registered custom shell tool: {}", name);
            }
            "python" => {
                use std::path::PathBuf;
                let name = tool.name.clone().unwrap_or_default();
                let description = tool.description.clone().unwrap_or_default();
                let file_path = PathBuf::from(tool.file.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Python tool '{}' missing 'file' field", name)
                })?);
                let handler = PythonToolHandler::new(
                    name.clone(),
                    description,
                    file_path,
                    tool.function.clone(),
                    tool.input_schema.clone(),
                    tool.output_schema.clone(),
                    container_id.clone(),
                );
                registry.register(Arc::new(handler));
                tracing::info!("Registered custom Python tool: {}", name);
            }
            _ => {}
        }
    }

    // Capture built-in tool definitions before MCP tools are registered,
    // so code mode can register them separately under "Tools" namespace.
    #[cfg(feature = "code-mode")]
    let builtin_tool_definitions = registry.tool_definitions();

    // Build McpServer list from mcp-type tools
    let mcp_servers: Vec<McpServer> = field
        .tools
        .iter()
        .filter(|t| t.tool_type == "mcp")
        .filter_map(|t| {
            let name = t.name.clone()?;
            let transport = t.transport.clone()?;
            Some(McpServer { name, transport })
        })
        .collect();

    // Build server name → tool config lookup for patch/filter access
    let server_tool_configs: std::collections::HashMap<String, &portlang_core::Tool> = field
        .tools
        .iter()
        .filter(|t| t.tool_type == "mcp")
        .filter_map(|t| t.name.as_ref().map(|n| (n.clone(), t)))
        .collect();

    // Load patch maps before connecting to MCP servers so a bad patch_file path
    // fails fast without wasting time on MCP connections.
    let mut patch_maps: std::collections::HashMap<String, portlang_core::McpPatchMap> =
        std::collections::HashMap::new();
    for (name, cfg) in &server_tool_configs {
        let pm = load_patch_map(cfg.patch_file.as_deref(), field.config_dir.as_deref())
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        patch_maps.insert(name.clone(), pm);
    }

    // Initialize MCP servers and collect patched tool definitions
    let mut mcp_manager = McpServerManager::new();
    let mut mcp_tools: Vec<(String, crate::mcp::McpToolDefinition)> = Vec::new();

    if !mcp_servers.is_empty() {
        tracing::info!("Initializing {} MCP server(s)", mcp_servers.len());

        mcp_manager
            .initialize_servers(
                &mcp_servers,
                field.config_dir.clone(),
                Some(container_id.clone()),
            )
            .await?;

        // Discover tools, apply filtering and patches, collect for reuse
        let empty_map = portlang_core::McpPatchMap::new();
        let raw_tools = mcp_manager.discover_tools().await?;
        for (server_name, tool_def) in raw_tools {
            let patched = if let Some(cfg) = server_tool_configs.get(&server_name) {
                let pm = patch_maps.get(&server_name).unwrap_or(&empty_map);
                apply_patches(vec![tool_def], cfg, pm)
            } else {
                vec![tool_def]
            };
            for t in patched {
                mcp_tools.push((server_name.clone(), t));
            }
        }

        // Register patched tools in ToolRegistry
        for (server_name, tool_def) in &mcp_tools {
            let client = mcp_manager
                .get_client(server_name)
                .ok_or_else(|| anyhow::anyhow!("MCP server not found: {}", server_name))?;

            let handler =
                McpToolHandler::new(server_name.clone(), tool_def.clone(), client.clone());
            registry.register(Arc::new(handler));
            tracing::info!(
                "Registered MCP tool: {} (from server: {})",
                tool_def.name,
                server_name
            );
        }
    }

    // Discover environment context
    tracing::debug!("Discovering environment context...");
    let env_context =
        EnvironmentContext::discover(sandbox.as_ref(), field.prompt.system.clone()).await;
    tracing::debug!("Environment context generated");

    // Pre-flight: run shell verifiers once against the empty workspace and abort
    // if any command is missing (exit 127 = command not found).
    preflight_verifiers(sandbox.as_ref(), &field.verifiers).await?;

    // Check if code mode is enabled
    let code_mode_enabled = field.environment.code_mode_enabled.unwrap_or(false);

    let (tools, system_prompt) = if code_mode_enabled {
        #[cfg(feature = "code-mode")]
        {
            tracing::info!("Code mode enabled - wrapping tools");

            // Create code mode handler
            let mut code_mode_handler = CodeModeHandler::new();

            // Register built-in and custom tools under "Tools" namespace
            // (captured before MCP tools were registered to avoid including them here)
            for def in builtin_tool_definitions {
                let tool_name = def.name.clone();
                let sandbox_clone = sandbox.clone();
                let callback: CodeModeCallback =
                    Arc::new(move |args: Option<serde_json::Value>| {
                        let sandbox = sandbox_clone.clone();
                        let name = tool_name.clone();
                        Box::pin(async move {
                            let input_value = args.unwrap_or(serde_json::json!({}));
                            match sandbox
                                .dispatch(&Action::ToolCall {
                                    tool: name.into(),
                                    input: input_value,
                                })
                                .await
                            {
                                Ok(result_str) => {
                                    // Try to parse result as JSON, fallback to string
                                    match serde_json::from_str(&result_str) {
                                        Ok(json) => Ok(json),
                                        Err(_) => Ok(serde_json::Value::String(result_str)),
                                    }
                                }
                                Err(e) => Err(format!("Tool error: {}", e)),
                            }
                        })
                    });
                code_mode_handler
                    .register_tool(
                        "Tools".to_string(),
                        def.name.clone(),
                        Some(def.description.clone()),
                        def.input_schema.clone(),
                        def.output_schema.clone(),
                        callback,
                    )
                    .map_err(|e| anyhow::anyhow!("Failed to register tool: {}", e))?;
                tracing::debug!("Registered {} in Tools namespace for code mode", def.name);
            }

            // Register MCP tools under their server name namespaces
            // Uses already-patched mcp_tools (with filtering + output_schema injected)
            for (server_name, tool_def) in &mcp_tools {
                let client = mcp_manager
                    .get_client(server_name)
                    .ok_or_else(|| anyhow::anyhow!("MCP server not found: {}", server_name))?;

                let tool_name = tool_def.name.clone();
                let tool_description = tool_def.description.clone();
                let input_schema = tool_def.input_schema.clone();
                let output_schema = tool_def.output_schema.clone();

                let callback: CodeModeCallback =
                    Arc::new(move |args: Option<serde_json::Value>| {
                        let client = client.clone();
                        let name = tool_name.clone();
                        Box::pin(async move {
                            let input_value = args.unwrap_or(serde_json::json!({}));
                            let client_guard = client.read().await;
                            let result = client_guard
                                .call_tool(&name, input_value)
                                .await
                                .map_err(|e| format!("MCP tool error: {}", e))?;

                            // Unwrap the MCP content envelope: extract text content and parse
                            // as JSON so TypeScript code gets the actual data, not the protocol
                            // wrapper {"content": [{"type": "text", "text": "..."}]}.
                            if let Some(arr) = result.get("content").and_then(|c| c.as_array()) {
                                let text: String = arr
                                    .iter()
                                    .filter_map(|item| {
                                        item.get("text").and_then(|t| t.as_str()).map(String::from)
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                if !text.is_empty() {
                                    return Ok(serde_json::from_str(&text)
                                        .unwrap_or(serde_json::Value::String(text)));
                                }
                            }
                            Ok(result)
                        })
                    });

                code_mode_handler
                    .register_tool(
                        server_name.clone(),
                        tool_def.name.clone(),
                        tool_description,
                        input_schema,
                        output_schema,
                        callback,
                    )
                    .map_err(|e| anyhow::anyhow!("Failed to register MCP tool: {}", e))?;
                tracing::debug!(
                    "Registered {} in {} namespace for code mode",
                    tool_def.name,
                    server_name
                );
            }

            // Get TypeScript definitions for system prompt
            let typescript_defs = code_mode_handler.get_typescript_definitions();

            // Build system prompt with TypeScript catalog
            let mut prompt = build_system_prompt(field, &env_context);
            prompt.push_str("\n\n# Code Mode\n\n");
            prompt.push_str(
                "You have access to a TypeScript execution environment via the `code_mode` tool. ",
            );
            prompt.push_str("Use this to batch multiple tool calls / chain actions.\n\n");

            prompt.push_str("## How to Use Code Mode\n\n");
            prompt.push_str("Your code must define an async `run()` function that will be called automatically:\n\n");
            prompt.push_str("```typescript\n");
            prompt.push_str("async function run() {\n");
            prompt.push_str("    // Call available functions using Namespace.functionName()\n");
            prompt.push_str(
                "    const results = await Namespace.search({ pattern: \"**/*.ts\" });\n",
            );
            prompt.push_str("    const content = await Namespace.read({ path: files[0] });\n");
            prompt.push_str("    return { results, content };\n");
            prompt.push_str("}\n");
            prompt.push_str("```\n\n");

            prompt.push_str("**Key Rules:**\n");
            prompt.push_str("- MUST define `async function run()` - do not call it yourself\n");
            prompt.push_str("- Write the COMPLETE end-to-end logic in a single `run()` function — fetch data, make decisions, and take actions all in one script using sequential `await` calls.\n");
            prompt.push_str(
                "- Only make multiple code_mode calls if the output schemas are not defined or if first one throws a runtime error.\n",
            );
            prompt.push_str("- Do NOT write comments\n");
            prompt.push_str("- Function results are JavaScript objects - access properties directly (do NOT use JSON.parse)\n");
            prompt.push_str("- Only registered SDK functions are available - no fetch(), fs, or other Deno/Node APIs\n");
            prompt.push_str("- Filter/transform large data IN YOUR CODE before returning\n\n");

            prompt.push_str("## Available Functions\n\n");
            prompt.push_str("```typescript\n");
            prompt.push_str(&typescript_defs);
            prompt.push_str("\n```\n");

            // Register the code_mode handler in the registry
            registry.register(Arc::new(code_mode_handler));

            // Return only the code_mode tool using ToolHandler trait
            // Get it from the registry to ensure consistency
            let code_mode_def = registry
                .tool_definitions()
                .into_iter()
                .find(|d| d.name == "code_mode")
                .ok_or_else(|| anyhow::anyhow!("code_mode tool not found in registry"))?;

            let mut tools = vec![Tool {
                name: code_mode_def.name,
                description: Some(code_mode_def.description),
                input_schema: code_mode_def.input_schema,
            }];

            // Add submit_output tool if structured output is required
            if let Some(ref schema) = field.boundary.output_schema {
                tools.push(Tool {
                    name: "submit_output".to_string(),
                    description: Some("Submit your final structured output. Call this when you're ready to finish with your JSON result.".to_string()),
                    input_schema: schema.clone(),
                });
            }

            (tools, prompt)
        }

        #[cfg(not(feature = "code-mode"))]
        {
            anyhow::bail!(
                "Code mode is not enabled in this build. Rebuild with --features code-mode"
            );
        }
    } else {
        // Standard mode - expose all tools directly
        let tool_definitions = registry.tool_definitions();
        let mut tools: Vec<Tool> = tool_definitions
            .into_iter()
            .map(|def| Tool {
                name: def.name,
                description: Some(def.description),
                input_schema: def.input_schema,
            })
            .collect();

        // Add submit_output tool if structured output is required
        if let Some(ref schema) = field.boundary.output_schema {
            tools.push(Tool {
                name: "submit_output".to_string(),
                description: Some("Submit your final structured output. Call this when you're ready to finish with your JSON result.".to_string()),
                input_schema: schema.clone(),
            });
        }

        let system_prompt = build_system_prompt(field, &env_context);

        (tools, system_prompt)
    };

    Ok(AgentView {
        tools,
        system_prompt,
        env_context,
        sandbox,
    })
}

/// Run a field configuration
pub async fn run_field(field: &Field, provider: &dyn ModelProvider) -> anyhow::Result<Trajectory> {
    // Prepare what the agent sees
    let agent_view = prepare_agent_view(field).await?;
    let AgentView {
        tools,
        system_prompt: system_prompt_text,
        env_context: _,
        sandbox,
    } = agent_view;

    // Build McpServer list for cleanup later
    let mcp_servers: Vec<McpServer> = field
        .tools
        .iter()
        .filter(|t| t.tool_type == "mcp")
        .filter_map(|t| {
            let name = t.name.clone()?;
            let transport = t.transport.clone()?;
            Some(McpServer { name, transport })
        })
        .collect();

    // Create MCP manager for cleanup
    let mut mcp_manager = McpServerManager::new();

    // Create context window
    let mut context = ContextWindow::new(&field.prompt.goal);

    // Create loop detector
    let mut loop_detector = LoopDetector::new();

    // Create fixup tracker for structured output correction
    let mut fixup_tracker = crate::structured_output::FixupTracker::new();

    // Create trajectory
    let tools_json = serde_json::to_string_pretty(&tools).unwrap_or_else(|_| "[]".to_string());
    let env_type = "local".to_string();
    let mut trajectory = Trajectory::new(field.name.clone());
    trajectory = trajectory.with_context(
        field.prompt.goal.clone(),
        provider.model_name().to_string(),
        system_prompt_text.clone(),
        tools_json.clone(),
        env_type,
    );

    // Store output schema if defined
    if let Some(ref schema) = field.boundary.output_schema {
        trajectory.set_output_schema(schema.clone());
    }

    let mut step_number = 0;

    // Set up Ctrl+C handler once for the duration of the run
    let mut ctrl_c = std::pin::pin!(tokio::signal::ctrl_c());

    loop {
        step_number += 1;
        tracing::info!("Starting step {}", step_number);

        // Step 1: Execute re-observations
        for re_obs_cmd in &field.prompt.re_observation {
            if let Ok(output) = sandbox.run_command(re_obs_cmd).await {
                context.append_observation(format!(
                    "Re-observation ({}):\n{}\n{}",
                    re_obs_cmd,
                    output.stdout,
                    if !output.stderr.is_empty() {
                        format!("stderr: {}", output.stderr)
                    } else {
                        String::new()
                    }
                ));
            }
        }

        // Step 2: Invoke policy with current context
        tracing::info!("Calling model API...");
        let (action, usage) = tokio::select! {
            result = provider.complete(
                context.messages(),
                &tools,
                Some(system_prompt_text.as_str()),
            ) => result?,
            _ = &mut ctrl_c => {
                tracing::info!("Ctrl+C received — saving partial trajectory ({} steps)", step_number - 1);
                trajectory.finish(RunOutcome::Interrupted { steps_completed: step_number - 1 });
                if !mcp_servers.is_empty() {
                    let _ = mcp_manager.shutdown_all().await;
                }
                return Ok(trajectory);
            }
        };
        tracing::info!("Model API returned, processing response...");

        // Calculate cost with proper input/output token breakdown
        let cost = provider.calculate_cost(&usage);
        context.add_tokens_and_cost(usage.total_tokens, cost);

        // NEW: Record the assistant's response so it can see its own actions
        let tool_use_id = match &action {
            Action::ToolCall { tool, input } => {
                let id = generate_tool_id();
                let block = ContentBlock::ToolUse {
                    id: id.clone(),
                    name: tool.to_string(),
                    input: input.clone(),
                };
                context.append_response(vec![block]);
                Some(id)
            }
            Action::TextOutput { text } => {
                let block = ContentBlock::Text { text: text.clone() };
                context.append_response(vec![block]);
                None
            }
            Action::Stop => {
                // Stop doesn't need recording as it's implicit
                None
            }
        };

        // Step 2.5: Check for loops
        let loop_check = loop_detector.detect_loop(&action);
        if let Some(loop_message) = loop_check {
            tracing::warn!("Loop detected: {}", loop_message);

            let rejection_msg = format!("REJECTED: {}", loop_message);

            // Record the loop rejection as a tool result if we have a tool ID
            if let Some(id) = tool_use_id {
                context.append_tool_result(id, rejection_msg.clone(), true);
            } else {
                context.append_rejection(&loop_message);
            }

            let step = TrajectoryStep::new(
                step_number,
                action.clone(),
                rejection_msg,
                true,
                cost,
                usage.total_tokens,
            )
            .with_token_breakdown(usage.input_tokens, usage.output_tokens);
            trajectory.add_step(step);

            // Don't record this in loop detector since it was rejected
            continue;
        }

        // Record action in loop detector (only if not rejected)
        loop_detector.record(&action);

        // Step 3: Check action against boundary
        let boundary_check = sandbox.check_boundary(&action).await;
        let rejected = boundary_check.is_err();

        let result = if rejected {
            let mut violation = boundary_check.unwrap_err();

            // Trace where the model might have gotten this information
            if let Some(ref attempted_value) = violation.attempted_value {
                let mut tracer =
                    ContextTracer::new(Some(system_prompt_text.clone()), Some(tools_json.clone()));

                // Add environment context
                tracer.add_environment_context(
                    "working_directory".to_string(),
                    "/workspace".to_string(),
                );

                // Analyze the violation with full context
                let analyzer = BoundaryAnalyzer::new(tracer);
                violation =
                    analyzer.analyze_write_violation(attempted_value, &violation.allowed_patterns);
            }

            let rejection_msg = violation.full_message();

            // Record the rejection as a tool result if we have a tool ID
            if let Some(id) = tool_use_id.clone() {
                context.append_tool_result(id, rejection_msg.clone(), true);
            } else {
                context.append_rejection(&rejection_msg);
            }
            rejection_msg
        } else {
            // Special handling for submit_output tool
            if let Action::ToolCall { tool, input, .. } = &action {
                if tool.as_str() == "submit_output" {
                    if let Some(ref schema) = field.boundary.output_schema {
                        match crate::structured_output::coerce(input, schema) {
                            Err(e) => {
                                // Coercion failed — inject fixup message and let agent retry
                                let raw = serde_json::to_string_pretty(input).unwrap_or_default();
                                let feedback = match fixup_tracker.next_message(&raw, schema, &e) {
                                    Some(msg) => msg,
                                    None => format!(
                                        "Output validation failed after {} fixup attempts. {}",
                                        crate::structured_output::MAX_FIXUP_ATTEMPTS,
                                        e
                                    ),
                                };
                                tracing::error!("submit_output coercion failed: {}", e);
                                if let Some(id) = tool_use_id.clone() {
                                    context.append_tool_result(id, feedback.clone(), true);
                                }
                                let step = TrajectoryStep::new(
                                    step_number,
                                    action.clone(),
                                    feedback,
                                    false,
                                    cost,
                                    usage.total_tokens,
                                )
                                .with_token_breakdown(usage.input_tokens, usage.output_tokens);
                                trajectory.add_step(step);
                                continue;
                            }
                            Ok(coerced) => {
                                let output_value = &coerced.value;
                                if !coerced.corrections.is_empty() {
                                    tracing::info!(
                                        "submit_output: applied {} correction(s), score={}",
                                        coerced.corrections.len(),
                                        coerced.score
                                    );
                                }

                                // Store in trajectory
                                trajectory.set_structured_output(output_value.clone());

                                // Return success to agent and trigger termination
                                let success_msg =
                                    "Output submitted successfully. Validation passed.".to_string();
                                if let Some(id) = tool_use_id.clone() {
                                    context.append_tool_result(id, success_msg.clone(), false);
                                }

                                let step = TrajectoryStep::new(
                                    step_number,
                                    action.clone(),
                                    success_msg.clone(),
                                    false,
                                    cost,
                                    usage.total_tokens,
                                )
                                .with_token_breakdown(usage.input_tokens, usage.output_tokens);
                                trajectory.add_step(step);

                                // Run verifiers
                                let verifier_results = run_verifiers(
                                    sandbox.as_ref(),
                                    &field.verifiers,
                                    &action,
                                    true,
                                    Some(output_value),
                                )
                                .await;
                                let all_passed = verifier_results.iter().all(|r| r.passed);

                                if verifier_results.is_empty() || all_passed {
                                    trajectory.finish(RunOutcome::Converged {
                                        message: "Structured output submitted and validated"
                                            .to_string(),
                                    });
                                } else if let Some(failed) =
                                    verifier_results.iter().find(|r| !r.passed)
                                {
                                    trajectory.finish(RunOutcome::VerifierFailed {
                                        verifier: failed.name.clone(),
                                        message: failed.stderr.clone(),
                                    });
                                }

                                if !mcp_servers.is_empty() {
                                    let _ = mcp_manager.shutdown_all().await;
                                }
                                return Ok(trajectory);
                            }
                        }
                    }
                }
            }

            // Step 4: Dispatch action to sandbox
            match sandbox.dispatch(&action).await {
                Ok(result) => {
                    // Log tool result
                    if let Action::ToolCall { tool, .. } = &action {
                        let truncated_result = if result.len() > 200 {
                            format!("{}... ({} chars total)", &result[..200], result.len())
                        } else {
                            result.clone()
                        };
                        tracing::info!("Tool '{}' returned: {}", tool, truncated_result);
                    }

                    // Reset consecutive error streak on success
                    loop_detector.record_success();

                    // Record result with tool ID if available
                    if let Some(id) = tool_use_id.clone() {
                        context.append_tool_result(id, result.clone(), false);
                    } else {
                        context.append_observation(&result);
                    }
                    result
                }
                Err(e) => {
                    let base_msg = format!("Error executing action: {}", e);

                    // Log tool error
                    if let Action::ToolCall { tool, .. } = &action {
                        tracing::error!("Tool '{}' failed: {}", tool, e);
                    }

                    // Augment message if consecutive error threshold is reached
                    let error_msg = match loop_detector.record_error() {
                        Some(warning) => format!("{}\n\n{}", base_msg, warning),
                        None => base_msg,
                    };

                    // Record error with tool ID if available
                    if let Some(id) = tool_use_id.clone() {
                        context.append_tool_result(id, error_msg.clone(), true);
                    } else {
                        context.append_observation(&error_msg);
                    }
                    error_msg
                }
            }
        };

        // Step 5: Run triggered verifiers
        let is_stop = action.is_stop();
        let verifier_results = run_verifiers(
            sandbox.as_ref(),
            &field.verifiers,
            &action,
            is_stop,
            trajectory.structured_output.as_ref(),
        )
        .await;

        // Add verifier results to context
        context.append_verifier_results(&verifier_results);

        // Create trajectory step
        let step = TrajectoryStep::new(
            step_number,
            action.clone(),
            result,
            rejected,
            cost,
            usage.total_tokens,
        )
        .with_token_breakdown(usage.input_tokens, usage.output_tokens)
        .with_verifier_results(verifier_results.clone());

        trajectory.add_step(step);

        // Log step completion
        tracing::info!(
            "Step {} complete: {:?}, input: {}, output: {}, total: {}, cost: {}",
            step_number,
            action,
            usage.input_tokens,
            usage.output_tokens,
            usage.total_tokens,
            cost
        );

        // Step 6: Check token budget
        // Use input_tokens from the current API call — this reflects the actual context window
        // size (the full conversation history sent to the model), not a cumulative sum.
        if let Some(max_tokens) = field.boundary.max_tokens {
            if usage.input_tokens >= max_tokens {
                trajectory.finish(RunOutcome::BudgetExhausted {
                    reason: format!(
                        "Token budget exhausted: context window {} >= {}",
                        usage.input_tokens, max_tokens
                    ),
                });
                // Cleanup MCP servers
                if !mcp_servers.is_empty() {
                    let _ = mcp_manager.shutdown_all().await;
                }
                return Ok(trajectory);
            }
        }

        // Check cost budget
        if let Some(max_cost) = &field.boundary.max_cost {
            if context.total_cost() >= *max_cost {
                trajectory.finish(RunOutcome::CostLimitExceeded {
                    reason: format!(
                        "Cost limit exceeded: {} >= {}",
                        context.total_cost(),
                        max_cost
                    ),
                });
                // Cleanup MCP servers
                if !mcp_servers.is_empty() {
                    let _ = mcp_manager.shutdown_all().await;
                }
                return Ok(trajectory);
            }
        }

        // Check step limit
        if let Some(max_steps) = field.boundary.max_steps {
            if step_number as u64 >= max_steps {
                trajectory.finish(RunOutcome::BudgetExhausted {
                    reason: format!("Step limit reached: {} >= {}", step_number, max_steps),
                });
                // Cleanup MCP servers
                if !mcp_servers.is_empty() {
                    let _ = mcp_manager.shutdown_all().await;
                }
                return Ok(trajectory);
            }
        }

        // Step 8: Check termination conditions
        if is_stop {
            // Get final message from action
            let final_message = match &action {
                Action::TextOutput { text } => text.clone(),
                _ => "Agent stopped".to_string(),
            };

            // Handle structured output BEFORE checking verifiers
            if let Some(ref schema) = field.boundary.output_schema {
                // Find the most recent text output to parse
                let raw_text = trajectory
                    .steps
                    .iter()
                    .rev()
                    .find_map(|s| {
                        if let portlang_core::Action::TextOutput { text } = &s.action {
                            Some(text.clone())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_default();

                match crate::structured_output::parse_and_coerce(&raw_text, schema) {
                    Ok(coerced) => {
                        if !coerced.corrections.is_empty() {
                            tracing::info!(
                                "Structured output: applied {} correction(s), score={}",
                                coerced.corrections.len(),
                                coerced.score
                            );
                        }

                        trajectory.set_structured_output(coerced.value);
                    }
                    Err(e) => {
                        // Inject fixup message and let agent try again (up to MAX_FIXUP_ATTEMPTS)
                        match fixup_tracker.next_message(&raw_text, schema, &e.to_string()) {
                            Some(fixup_msg) => {
                                tracing::warn!(
                                    "Structured output parse failed, injecting fixup (attempt {}): {}",
                                    fixup_tracker.attempts(),
                                    e
                                );
                                context.append_observation(fixup_msg);
                                continue; // restart loop — agent sees fixup message
                            }
                            None => {
                                tracing::error!(
                                    "Structured output failed after all fixup attempts: {}",
                                    e
                                );
                                trajectory.finish(RunOutcome::Error {
                                    message: format!(
                                        "Failed to produce valid structured output after {} attempts: {}",
                                        crate::structured_output::MAX_FIXUP_ATTEMPTS,
                                        e
                                    ),
                                });
                                if !mcp_servers.is_empty() {
                                    let _ = mcp_manager.shutdown_all().await;
                                }
                                return Ok(trajectory);
                            }
                        }
                    }
                }
            }

            // Now check if all verifiers passed
            let all_passed = verifier_results.iter().all(|r| r.passed);

            if verifier_results.is_empty() || all_passed {
                trajectory.finish(RunOutcome::Converged {
                    message: final_message,
                });
                // Cleanup MCP servers
                if !mcp_servers.is_empty() {
                    let _ = mcp_manager.shutdown_all().await;
                }
                return Ok(trajectory);
            } else {
                // Verifier failed - find first failure
                if let Some(failed) = verifier_results.iter().find(|r| !r.passed) {
                    trajectory.finish(RunOutcome::VerifierFailed {
                        verifier: failed.name.clone(),
                        message: failed.stderr.clone(),
                    });
                    // Cleanup MCP servers
                    if !mcp_servers.is_empty() {
                        let _ = mcp_manager.shutdown_all().await;
                    }
                    return Ok(trajectory);
                }
            }
        }

        // Check for boundary violations that should terminate
        if rejected {
            // For now, we let the agent try again after rejection
            // Could add a max_rejections limit here
        }
    }
}

/// Run each shell verifier once against the empty workspace. If any command exits with
/// code 127 (command not found), return an error immediately so the run aborts before
/// spending any model budget. Other non-zero exits are ignored — the workspace is empty
/// so failures like "file not found" are expected and harmless.
async fn preflight_verifiers(
    sandbox: &dyn crate::sandbox::Sandbox,
    verifiers: &[Verifier],
) -> anyhow::Result<()> {
    for verifier in verifiers {
        if let VerifierAlgorithm::Shell { command } = &verifier.algorithm {
            if let Ok(output) = sandbox.run_command(command).await {
                if output.exit_code == 127 {
                    anyhow::bail!(
                        "Verifier '{}' cannot run — command not found (exit 127).\n  Command: {}\n  stderr: {}\nInstall the required binary on your host before running this field.",
                        verifier.name,
                        command,
                        output.stderr.trim()
                    );
                }
            }
        }
    }
    Ok(())
}

/// Build system prompt with environment context
fn build_system_prompt(field: &Field, env_context: &EnvironmentContext) -> String {
    let mut parts = vec![];

    // User's custom system prompt
    if let Some(user_prompt) = &field.prompt.system {
        parts.push(user_prompt.clone());
    }

    // Auto-discovered environment
    parts.push(env_context.format_for_prompt());

    // Add structured output requirements if schema is defined
    if let Some(ref schema) = field.boundary.output_schema {
        let mut output_doc = String::from("\n# Structured Output\n\n");
        output_doc.push_str("This task requires structured output matching this schema:\n\n");
        output_doc.push_str("```json\n");
        output_doc
            .push_str(&serde_json::to_string_pretty(schema).unwrap_or_else(|_| "{}".to_string()));
        output_doc.push_str("\n```\n\n");
        output_doc.push_str("When you're ready to submit your results, call the `submit_output` tool passing your JSON fields directly as the tool arguments.\n");
        output_doc
            .push_str("The system will validate your output and store it in the trajectory.\n");

        parts.push(output_doc);
    }

    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::dispatch::DispatchSandbox;
    use crate::tools::ToolRegistry;
    use portlang_core::{Boundary, Verifier, VerifierAlgorithm, VerifierTrigger};
    use std::sync::Arc;

    fn shell_verifier(name: &str, command: &str) -> Verifier {
        Verifier {
            name: name.to_string(),
            algorithm: VerifierAlgorithm::Shell {
                command: command.to_string(),
            },
            trigger: VerifierTrigger::OnStop,
            description: None,
        }
    }

    fn make_sandbox() -> DispatchSandbox {
        let tmp = std::env::temp_dir();
        let registry = Arc::new(ToolRegistry::new());
        DispatchSandbox::new(tmp, Boundary::default(), registry)
    }

    #[tokio::test]
    async fn preflight_passes_for_known_command() {
        let sandbox = make_sandbox();
        let verifiers = vec![shell_verifier("check-echo", "echo hello")];
        let result = preflight_verifiers(&sandbox, &verifiers).await;
        assert!(result.is_ok(), "echo should always be found: {:?}", result);
    }

    #[tokio::test]
    async fn preflight_catches_missing_binary() {
        let sandbox = make_sandbox();
        let verifiers = vec![shell_verifier(
            "uses-missing-binary",
            "portlang_test_nonexistent_binary_xyz --version",
        )];
        let result = preflight_verifiers(&sandbox, &verifiers).await;
        assert!(result.is_err(), "should fail when binary is missing");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("uses-missing-binary"),
            "error should name the verifier: {}",
            msg
        );
        assert!(
            msg.contains("127"),
            "error should mention exit code 127: {}",
            msg
        );
    }
}
