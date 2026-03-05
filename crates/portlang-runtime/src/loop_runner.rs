use crate::context_window::ContextWindow;
use crate::environment::EnvironmentContext;
use crate::loop_detection::LoopDetector;
use crate::provider::{ContentBlock, ModelProvider, Tool};
use crate::sandbox::create_sandbox;
use crate::tools::{
    GlobHandler, PythonToolHandler, ReadHandler, ShellCommandHandler, ToolRegistry, WriteHandler,
};
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
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(ReadHandler));
    registry.register(Arc::new(WriteHandler));
    registry.register(Arc::new(GlobHandler));

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
                    script_path,
                    custom_tool.function.clone(),
                    custom_tool.input_schema.clone(),
                );
                registry.register(Arc::new(handler));
                tracing::info!("Registered custom Python tool: {}", custom_tool.name);
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

    let registry = Arc::new(registry);

    // Create sandbox with registry
    let sandbox = create_sandbox(&field.environment, &field.boundary, registry.clone()).await?;

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
    let tools: Vec<Tool> = tool_definitions
        .into_iter()
        .map(|def| Tool {
            name: def.name,
            description: Some(def.description),
            input_schema: def.input_schema,
        })
        .collect();

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
        let (action, tokens) = provider
            .complete(context.messages(), &tools, system_prompt.as_deref())
            .await?;
        tracing::info!("Model API returned, processing response...");

        // Calculate cost
        let cost = provider.calculate_cost(tokens, 0); // Note: provider already includes both input and output
        context.add_tokens_and_cost(tokens, cost);

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
                tokens,
            );
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
            let violation = boundary_check.unwrap_err();
            let rejection_msg = format!("REJECTED: {}", violation.description);

            // Record the rejection as a tool result if we have a tool ID
            if let Some(id) = tool_use_id.clone() {
                context.append_tool_result(id, rejection_msg.clone(), true);
            } else {
                context.append_rejection(&violation.description);
            }
            rejection_msg
        } else {
            // Step 4: Dispatch action to sandbox
            match sandbox.dispatch(&action).await {
                Ok(result) => {
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
        let step = TrajectoryStep::new(step_number, action.clone(), result, rejected, cost, tokens)
            .with_verifier_results(verifier_results.clone());

        trajectory.add_step(step);

        // Log step completion
        tracing::info!(
            "Step {} complete: {:?}, tokens: {}, cost: {}",
            step_number,
            action,
            tokens,
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
                return Ok(trajectory);
            }
        }

        // Check step limit
        if let Some(max_steps) = field.context.max_steps {
            if step_number as u64 >= max_steps {
                trajectory.finish(RunOutcome::BudgetExhausted {
                    reason: format!("Step limit reached: {} >= {}", step_number, max_steps),
                });
                return Ok(trajectory);
            }
        }

        // Step 8: Check termination conditions
        if is_stop {
            // Check if all verifiers passed
            let all_passed = verifier_results.iter().all(|r| r.passed);

            if verifier_results.is_empty() || all_passed {
                // Get final message from action
                let final_message = match &action {
                    Action::TextOutput { text } => text.clone(),
                    _ => "Agent stopped".to_string(),
                };

                trajectory.finish(RunOutcome::Converged {
                    message: final_message,
                });
                return Ok(trajectory);
            } else {
                // Verifier failed - find first failure
                if let Some(failed) = verifier_results.iter().find(|r| !r.passed) {
                    trajectory.finish(RunOutcome::VerifierFailed {
                        verifier: failed.name.clone(),
                        message: failed.stderr.clone(),
                    });
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

/// Build system prompt with environment context
fn build_system_prompt(field: &Field, env_context: &EnvironmentContext) -> Option<String> {
    let mut parts = vec![];

    // User's custom system prompt
    if let Some(user_prompt) = &field.context.system_prompt {
        parts.push(user_prompt.clone());
    }

    // Auto-discovered environment
    parts.push(env_context.format_for_prompt());

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}
