use crate::context_window::ContextWindow;
use crate::environment::EnvironmentContext;
use crate::loop_detection::LoopDetector;
use crate::mcp::{McpServerManager, McpToolHandler};
use crate::provider::{ContentBlock, ModelProvider, Tool};
use crate::sandbox::{create_sandbox, BoundaryAnalyzer, ContextTracer};
#[cfg(feature = "code-mode")]
use crate::tools::handler::ToolHandler;
use crate::tools::{
    GlobHandler, PythonToolHandler, ReadHandler, ShellCommandHandler, ToolRegistry, WriteHandler,
};

#[cfg(feature = "code-mode")]
use crate::tools::CodeModeHandler;
use crate::verifier_runner::run_verifiers;
use portlang_core::*;
use std::sync::Arc;
use uuid::Uuid;

/// Generate a unique tool use ID
fn generate_tool_id() -> String {
    format!("toolu_{}", Uuid::new_v4().to_string().replace("-", ""))
}

/// Run a field to completion
pub async fn run_field(field: &Field, provider: &dyn ModelProvider) -> anyhow::Result<Trajectory> {
    tracing::debug!("Starting run_field for field: {}", field.name);

    // Create trajectory
    let mut trajectory = Trajectory::new(field.name.clone());

    // Create tool registry with built-in tools
    let registry = ToolRegistry::new();
    registry.register(Arc::new(ReadHandler));
    registry.register(Arc::new(WriteHandler));
    registry.register(Arc::new(GlobHandler));

    // Wrap registry in Arc early so we can share it with sandbox and continue registering tools
    let registry = Arc::new(registry);

    // Create sandbox FIRST so we can get container ID for MCP and Python tools
    let sandbox = create_sandbox(
        &field.environment,
        &field.boundary,
        registry.clone(),
        &field.container,
    )
    .await?;
    let container_id = sandbox.container_id().map(|s| s.to_string());

    // Create Code Mode handler if enabled
    #[cfg(feature = "code-mode")]
    let mut code_mode_handler = if let Some(ref code_mode_config) = field.code_mode {
        if code_mode_config.enabled {
            Some(CodeModeHandler::new())
        } else {
            None
        }
    } else {
        None
    };

    // Register custom tools from field config
    for custom_tool in &field.custom_tools {
        match custom_tool.tool_type.as_str() {
            "shell" => {
                let handler = ShellCommandHandler::new(
                    custom_tool.name.clone(),
                    custom_tool.description.clone(),
                    custom_tool.command.clone().unwrap_or_default(),
                    custom_tool.input_schema.clone(),
                );
                registry.register(Arc::new(handler));
                tracing::info!("Registered custom shell tool: {}", custom_tool.name);
            }
            "python" => {
                use std::path::PathBuf;

                let script_path = PathBuf::from(custom_tool.script.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Python tool '{}' missing 'script' field", custom_tool.name)
                })?);
                let handler = PythonToolHandler::new(
                    custom_tool.name.clone(),
                    custom_tool.description.clone(),
                    script_path.clone(),
                    custom_tool.function.clone(),
                    custom_tool.input_schema.clone(),
                );
                registry.register(Arc::new(handler));
                tracing::info!("Registered custom Python tool: {}", custom_tool.name);

                // Also register with Code Mode if enabled
                #[cfg(feature = "code-mode")]
                if let Some(ref mut code_mode) = code_mode_handler {
                    let tool_name = custom_tool.name.clone();
                    let tool_description = custom_tool.description.clone();
                    let tool_schema = custom_tool.input_schema.clone();
                    let script = script_path.clone();
                    let function = custom_tool.function.clone();
                    let env_root = match &field.environment {
                        Environment::Local { root } => std::path::PathBuf::from(root),
                    };

                    // Create callback that executes the Python tool
                    let callback: Arc<
                        dyn Fn(
                                Option<serde_json::Value>,
                            ) -> std::pin::Pin<
                                Box<
                                    dyn std::future::Future<
                                            Output = std::result::Result<serde_json::Value, String>,
                                        > + Send,
                                >,
                            > + Send
                            + Sync,
                    > = Arc::new(move |args| {
                        let script = script.clone();
                        let function = function.clone();
                        let root = env_root.clone();
                        let name = tool_name.clone();
                        let description = tool_description.clone();
                        let schema = tool_schema.clone();
                        let args = args.unwrap_or(serde_json::Value::Null);

                        Box::pin(async move {
                            // Execute the Python tool
                            let handler =
                                PythonToolHandler::new(name, description, script, function, schema);

                            let result = handler
                                .execute(&root, args)
                                .await
                                .map_err(|e| e.to_string())?;

                            // Parse the result as JSON
                            serde_json::from_str(&result).map_err(|e| e.to_string())
                        })
                    });

                    code_mode
                        .register_tool(
                            "Tools".to_string(),
                            custom_tool.name.clone(),
                            Some(custom_tool.description.clone()),
                            custom_tool.input_schema.clone(),
                            callback,
                        )
                        .map_err(|e| {
                            anyhow::anyhow!("Failed to register Python tool in Code Mode: {}", e)
                        })?;

                    tracing::info!(
                        "Registered Python tool in Code Mode: Tools.{}",
                        custom_tool.name
                    );
                }
            }
            other => {
                tracing::warn!(
                    "Unknown tool type '{}' for tool '{}', skipping",
                    other,
                    custom_tool.name
                );
            }
        }
    }

    // Initialize MCP servers and register their tools
    let mut mcp_manager = McpServerManager::new();

    if !field.mcp_servers.is_empty() {
        tracing::info!("Initializing {} MCP server(s)", field.mcp_servers.len());

        mcp_manager
            .initialize_servers(
                &field.mcp_servers,
                field.config_dir.clone(),
                container_id.clone(),
            )
            .await?;

        // Discover and register tools from all MCP servers
        let mcp_tools = mcp_manager.discover_tools().await?;

        for (server_name, tool_def) in mcp_tools {
            let client = mcp_manager
                .get_client(&server_name)
                .ok_or_else(|| anyhow::anyhow!("MCP server not found: {}", server_name))?;

            // Register in ToolRegistry
            let handler =
                McpToolHandler::new(server_name.clone(), tool_def.clone(), client.clone());
            registry.register(Arc::new(handler));
            tracing::info!(
                "Registered MCP tool: {} (from server: {})",
                tool_def.name,
                server_name
            );

            // Also register with Code Mode if enabled
            #[cfg(feature = "code-mode")]
            if let Some(ref mut code_mode) = code_mode_handler {
                let tool_name = tool_def.name.clone();
                let tool_description_opt = tool_def.description.clone();
                let tool_schema = tool_def.input_schema.clone();
                let client_for_callback = client.clone();

                // Create callback that executes the MCP tool
                let callback: Arc<
                    dyn Fn(
                            Option<serde_json::Value>,
                        ) -> std::pin::Pin<
                            Box<
                                dyn std::future::Future<
                                        Output = std::result::Result<serde_json::Value, String>,
                                    > + Send,
                            >,
                        > + Send
                        + Sync,
                > = Arc::new(move |args| {
                    let client = client_for_callback.clone();
                    let name = tool_name.clone();
                    let args = args.unwrap_or(serde_json::Value::Null);

                    Box::pin(async move {
                        // Call the MCP tool
                        let client_lock = client.read().await;
                        let result = client_lock
                            .call_tool(&name, args)
                            .await
                            .map_err(|e| e.to_string())?;

                        Ok(result)
                    })
                });

                code_mode
                    .register_tool(
                        "MCP".to_string(),
                        tool_def.name.clone(),
                        tool_description_opt.clone(),
                        tool_schema.clone(),
                        callback,
                    )
                    .map_err(|e| {
                        anyhow::anyhow!("Failed to register MCP tool in Code Mode: {}", e)
                    })?;

                tracing::info!("Registered MCP tool in Code Mode: MCP.{}", tool_def.name);
            }
        }
    }

    // Register Code Mode handler if enabled
    #[cfg(feature = "code-mode")]
    if let Some(code_mode) = code_mode_handler {
        registry.register(Arc::new(code_mode));
        tracing::info!("Code Mode enabled with registered custom tools");
    }

    // Discover environment context
    tracing::debug!("Discovering environment context...");
    let env_context =
        EnvironmentContext::discover(sandbox.as_ref(), field.environment_context.clone()).await;
    tracing::debug!("Environment context generated");

    // Create context window
    let mut context = ContextWindow::new(&field.goal);

    // Create loop detector
    let mut loop_detector = LoopDetector::new();

    // Tool definitions from registry
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
    if field.output_schema.is_some() {
        tools.push(Tool {
            name: "submit_output".to_string(),
            description: Some("Submit your final structured output. Call this when you're ready to finish with your JSON result.".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "output": {
                        "type": "object",
                        "description": "Your structured output matching the required schema"
                    }
                },
                "required": ["output"]
            }),
        });
    }

    // Capture context for trajectory
    let system_prompt_text = build_system_prompt(field, &env_context);
    let tools_json = serde_json::to_string_pretty(&tools).unwrap_or_else(|_| "[]".to_string());
    let env_type = match &field.environment {
        Environment::Local { .. } => "local".to_string(),
    };

    trajectory = trajectory.with_context(
        field.goal.clone(),
        provider.model_name().to_string(),
        system_prompt_text.clone(),
        tools_json.clone(),
        env_type,
    );

    // Store output schema if defined
    if let Some(ref schema) = field.output_schema {
        trajectory.set_output_schema(schema.clone());
    }

    let mut step_number = 0;

    loop {
        step_number += 1;
        tracing::info!("Starting step {}", step_number);

        // Step 1: Execute re-observations
        for re_obs_cmd in &field.re_observation {
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
        let system_prompt = build_system_prompt(field, &env_context);
        let (action, usage) = provider
            .complete(context.messages(), &tools, Some(system_prompt.as_str()))
            .await?;
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
                    // Extract and validate the output
                    if let Some(ref schema) = field.output_schema {
                        match input.get("output") {
                            Some(output_value) => {
                                // Validate against schema
                                if let Err(e) = crate::structured_output::validate_against_schema(
                                    output_value,
                                    schema,
                                ) {
                                    let error_msg = format!("Output validation failed: {}", e);
                                    tracing::error!("{}", error_msg);

                                    // Return error to agent
                                    if let Some(id) = tool_use_id.clone() {
                                        context.append_tool_result(id, error_msg.clone(), true);
                                    }

                                    let step = TrajectoryStep::new(
                                        step_number,
                                        action.clone(),
                                        error_msg.clone(),
                                        false,
                                        cost,
                                        usage.total_tokens,
                                    )
                                    .with_token_breakdown(usage.input_tokens, usage.output_tokens);
                                    trajectory.add_step(step);

                                    continue; // Let agent try again
                                }

                                // Write to output.json
                                let output_json =
                                    serde_json::to_string_pretty(output_value).unwrap();
                                let write_cmd = format!(
                                    "cat > /workspace/output.json << 'EOF'\n{}\nEOF",
                                    output_json
                                );
                                if let Err(e) = sandbox.run_command(&write_cmd).await {
                                    tracing::warn!("Failed to write output.json: {}", e);
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
                                )
                                .await;
                                let all_passed = verifier_results.iter().all(|r| r.passed);

                                if verifier_results.is_empty() || all_passed {
                                    trajectory.finish(RunOutcome::Converged {
                                        message: "Structured output submitted and validated"
                                            .to_string(),
                                    });
                                } else {
                                    if let Some(failed) =
                                        verifier_results.iter().find(|r| !r.passed)
                                    {
                                        trajectory.finish(RunOutcome::VerifierFailed {
                                            verifier: failed.name.clone(),
                                            message: failed.stderr.clone(),
                                        });
                                    }
                                }

                                if !field.mcp_servers.is_empty() {
                                    let _ = mcp_manager.shutdown_all().await;
                                }
                                return Ok(trajectory);
                            }
                            None => {
                                let error_msg =
                                    "submit_output requires an 'output' parameter".to_string();
                                if let Some(id) = tool_use_id.clone() {
                                    context.append_tool_result(id, error_msg.clone(), true);
                                }

                                let step = TrajectoryStep::new(
                                    step_number,
                                    action.clone(),
                                    error_msg,
                                    false,
                                    cost,
                                    usage.total_tokens,
                                )
                                .with_token_breakdown(usage.input_tokens, usage.output_tokens);
                                trajectory.add_step(step);

                                continue;
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

                    // Record result with tool ID if available
                    if let Some(id) = tool_use_id.clone() {
                        context.append_tool_result(id, result.clone(), false);
                    } else {
                        context.append_observation(&result);
                    }
                    result
                }
                Err(e) => {
                    let error_msg = format!("Error executing action: {}", e);

                    // Log tool error
                    if let Action::ToolCall { tool, .. } = &action {
                        tracing::error!("Tool '{}' failed: {}", tool, e);
                    }

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
        let verifier_results =
            run_verifiers(sandbox.as_ref(), &field.verifiers, &action, is_stop).await;

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
        if let Some(max_tokens) = field.context.max_tokens {
            if context.total_tokens() >= max_tokens {
                trajectory.finish(RunOutcome::BudgetExhausted {
                    reason: format!(
                        "Token budget exhausted: {} >= {}",
                        context.total_tokens(),
                        max_tokens
                    ),
                });
                // Cleanup MCP servers
                if !field.mcp_servers.is_empty() {
                    let _ = mcp_manager.shutdown_all().await;
                }
                return Ok(trajectory);
            }
        }

        // Check cost budget
        if let Some(max_cost) = &field.context.max_cost {
            if context.total_cost() >= *max_cost {
                trajectory.finish(RunOutcome::CostLimitExceeded {
                    reason: format!(
                        "Cost limit exceeded: {} >= {}",
                        context.total_cost(),
                        max_cost
                    ),
                });
                // Cleanup MCP servers
                if !field.mcp_servers.is_empty() {
                    let _ = mcp_manager.shutdown_all().await;
                }
                return Ok(trajectory);
            }
        }

        // Check step limit
        if let Some(max_steps) = field.context.max_steps {
            if step_number as u64 >= max_steps {
                trajectory.finish(RunOutcome::BudgetExhausted {
                    reason: format!("Step limit reached: {} >= {}", step_number, max_steps),
                });
                // Cleanup MCP servers
                if !field.mcp_servers.is_empty() {
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

            // Handle structured output BEFORE checking verifiers (verifiers need output.json to exist)
            if let Some(ref schema) = field.output_schema {
                match crate::structured_output::extract_structured_output_from_trajectory(
                    &trajectory.steps,
                ) {
                    Ok(output) => {
                        // Validate against schema
                        if let Err(e) =
                            crate::structured_output::validate_against_schema(&output, schema)
                        {
                            tracing::error!("Output validation failed: {}", e);
                            trajectory.finish(RunOutcome::Error {
                                message: format!("Output validation failed: {}", e),
                            });
                            if !field.mcp_servers.is_empty() {
                                let _ = mcp_manager.shutdown_all().await;
                            }
                            return Ok(trajectory);
                        }

                        // Write to output.json for verifiers to access
                        let output_json = serde_json::to_string_pretty(&output).unwrap();
                        let write_cmd = format!(
                            "cat > /workspace/output.json << 'EOF'\n{}\nEOF",
                            output_json
                        );
                        if let Err(e) = sandbox.run_command(&write_cmd).await {
                            tracing::warn!("Failed to write output.json: {}", e);
                        }

                        // Store in trajectory
                        trajectory.set_structured_output(output);
                    }
                    Err(e) => {
                        tracing::error!("Failed to extract structured output: {}", e);
                        trajectory.finish(RunOutcome::Error {
                            message: format!("Failed to extract structured output: {}", e),
                        });
                        if !field.mcp_servers.is_empty() {
                            let _ = mcp_manager.shutdown_all().await;
                        }
                        return Ok(trajectory);
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
                if !field.mcp_servers.is_empty() {
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
                    if !field.mcp_servers.is_empty() {
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

#[cfg(feature = "code-mode")]
/// Convert snake_case to camelCase for TypeScript function names
fn to_camel_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;

    for c in s.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }

    result
}

/// Build system prompt with environment context
fn build_system_prompt(field: &Field, env_context: &EnvironmentContext) -> String {
    let mut parts = vec![];

    // User's custom system prompt
    if let Some(user_prompt) = &field.context.system_prompt {
        parts.push(user_prompt.clone());
    }

    // Auto-discovered environment
    parts.push(env_context.format_for_prompt());

    // Code Mode API documentation
    #[cfg(feature = "code-mode")]
    if let Some(ref code_mode_config) = field.code_mode {
        if code_mode_config.enabled {
            let mut code_mode_doc = String::from("\n# Code Mode\n\n");
            code_mode_doc.push_str("You have access to a `code_mode` tool that executes TypeScript code in a sandboxed Deno runtime.\n\n");
            code_mode_doc.push_str("**Code Structure:**\n");
            code_mode_doc.push_str("```typescript\n");
            code_mode_doc.push_str("async function run() {\n");
            code_mode_doc.push_str("    // Your code here\n");
            code_mode_doc.push_str("    return result;\n");
            code_mode_doc.push_str("}\n");
            code_mode_doc.push_str("```\n\n");

            // Document available custom tools
            if !field.custom_tools.is_empty() {
                code_mode_doc
                    .push_str("**Available Tools API (callable from within code_mode):**\n\n");
                code_mode_doc.push_str("You can call these tools from your TypeScript code:\n\n");
                for tool in &field.custom_tools {
                    // Convert snake_case to camelCase for TypeScript
                    let camel_name = to_camel_case(&tool.name);
                    code_mode_doc.push_str(&format!(
                        "- `Tools.{}(params)` - {}\n",
                        camel_name, tool.description
                    ));
                    if let Some(props) = tool
                        .input_schema
                        .get("properties")
                        .and_then(|p| p.as_object())
                    {
                        code_mode_doc.push_str("  Parameters: { ");
                        let params: Vec<String> = props.keys().map(|k| k.to_string()).collect();
                        code_mode_doc.push_str(&params.join(", "));
                        code_mode_doc.push_str(" }\n");
                    }
                }
                code_mode_doc.push_str("\n");
            }

            // Document available MCP tools
            if !field.mcp_servers.is_empty() {
                code_mode_doc
                    .push_str("**Available MCP Tools API (callable from within code_mode):**\n\n");
                code_mode_doc.push_str(
                    "Tools from connected MCP servers are available under the `Mcp` namespace (note: capitalized):\n",
                );
                code_mode_doc.push_str(
                    "- Use `Mcp.toolName(params)` to call MCP tools (note: camelCase function names)\n",
                );
                code_mode_doc.push_str("- All MCP tools are async and must be awaited\n");
                code_mode_doc
                    .push_str("- Example: `await Mcp.readFile({ path: \"/file.txt\" })`\n");
                code_mode_doc.push_str(&format!(
                    "- Connected servers: {}\n\n",
                    field
                        .mcp_servers
                        .iter()
                        .map(|s| s.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }

            code_mode_doc.push_str("**Benefits:**\n");
            code_mode_doc
                .push_str("- Dramatically reduces token usage for data-heavy operations\n");
            code_mode_doc
                .push_str("- Chain multiple tool calls without intermediate model invocations\n");
            code_mode_doc.push_str("- Perform complex data transformations in a single step\n");
            code_mode_doc
                .push_str("- Use standard TypeScript/JavaScript for logic and iteration\n");

            parts.push(code_mode_doc);
        }
    }

    // Add structured output requirements if schema is defined
    if let Some(ref schema) = field.output_schema {
        let mut output_doc = String::from("\n# Structured Output\n\n");
        output_doc.push_str("This task requires structured output matching this schema:\n\n");
        output_doc.push_str("```json\n");
        output_doc
            .push_str(&serde_json::to_string_pretty(schema).unwrap_or_else(|_| "{}".to_string()));
        output_doc.push_str("\n```\n\n");
        output_doc.push_str("When you're ready to submit your results, use the `submit_output` tool with your JSON object.\n");
        output_doc.push_str(
            "The system will validate your output and write it to output.json for verification.\n",
        );

        parts.push(output_doc);
    }

    parts.join("\n")
}
