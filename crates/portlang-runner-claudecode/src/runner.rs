use crate::settings::build_all_mcp_config;
use crate::stream_parser::StreamAccumulator;
use anyhow::{Context, Result};
use portlang_core::{
    Action, Cost, Environment, Field, InputSource, RunOutcome, RuntimeContext, Skill, Trajectory,
    TrajectoryStep, VerifierAlgorithm, VerifierTrigger,
};
use portlang_runtime::{run_verifiers, sandbox::Sandbox};
use portlang_skills::{
    build_skill_metadata_block, detect_skill_invocations, write_skills_to_workspace, SkillResolver,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};

/// Run a field using Claude Code CLI as the agent loop.
///
/// Sets up the container sandbox exactly as the native runner does, then
/// execs `claude --print "<goal>" --output-format stream-json` inside the
/// container. Verifiers and trajectory recording remain portlang's
/// responsibility.
///
/// Requires `ANTHROPIC_API_KEY` to be set in the host environment.
pub async fn run_field_with_claude_code(
    field: &Field,
    ctx: &RuntimeContext,
    sandbox: Arc<dyn Sandbox>,
) -> Result<Trajectory> {
    // --- 1. Ensure workspace directory exists and stage any input ---
    let workspace_str = &field.environment.root;
    let workspace = PathBuf::from(workspace_str);
    tokio::fs::create_dir_all(&workspace)
        .await
        .context("Failed to create workspace directory")?;

    if let Some(ref input) = ctx.input {
        stage_input(&workspace, input).await?;
    }

    // --- 2. Sandbox is injected by the caller ---
    // The caller is responsible for building the environment with with_required_packages()
    // and creating the sandbox before passing it here.

    // --- 2b. Resolve skills ---
    let mut skills = field.skills.clone();
    if !skills.is_empty() {
        let resolver = SkillResolver::default();
        if let Err(e) = resolver.resolve_all(&mut skills).await {
            tracing::warn!("Skill resolution encountered errors: {}", e);
        }
    }

    // --- 3. Prepare trajectory skeleton ---
    let mut trajectory = Trajectory::new(field.name.clone()).with_context(
        field.prompt.goal.clone(),
        format!("claude-code/{}", field.model.name),
        field.prompt.system.clone().unwrap_or_default(),
        "claude-code (native tools)".to_string(),
        sandbox.backend_name().to_string(),
    );

    // --- 4. Write helper files into the workspace (visible at /workspace/ in container) ---
    // Writing via the mounted host directory avoids any shell-escaping issues.
    let goal =
        build_goal_with_output_schema(&field.prompt.goal, field.boundary.output_schema.as_ref());
    write_workspace_file(&workspace, ".portlang_cc_goal.txt", &goal)?;

    // --- 4a. Write skill files into workspace ---
    // Each resolved skill is written to .portlang_skills/<slug>/SKILL.md so Claude
    // can read them on-demand via bash (Anthropic progressive disclosure pattern).
    write_skills_to_workspace(&skills, &workspace).await?;

    // Also register skills in ~/.claude/skills/ so Claude Code's native $slug
    // invocation shorthand works in the goal text.
    let registered_skills = register_skills_with_claude_code(&skills).await;

    // --- 4b. Build system prompt with skill metadata ---
    // Following Anthropic's Agent Skills architecture:
    // - Only skill name + description goes in the system prompt (~100 tokens/skill)
    // - Full SKILL.md content lives on the workspace filesystem, read on-demand
    const BASELINE_SYSTEM: &str = "\
To create a new file use Bash (e.g. `echo ... > file` or `tee file`). \
The Write tool requires the file to already exist and to have been read first.";

    let skill_metadata = build_skill_metadata_block(&skills);
    let system_text = match (&field.prompt.system, skill_metadata.is_empty()) {
        (Some(s), false) => format!("{}\n\n{}\n\n{}", s, skill_metadata, BASELINE_SYSTEM),
        (Some(s), true) => format!("{}\n\n{}", s, BASELINE_SYSTEM),
        (None, false) => format!("{}\n\n{}", skill_metadata, BASELINE_SYSTEM),
        (None, true) => BASELINE_SYSTEM.to_string(),
    };
    write_workspace_file(&workspace, ".portlang_cc_system.txt", &system_text)?;
    let has_system = true;

    // Write MCP config for all tool types:
    // - MCP tools: passed through directly
    // - Shell tools: generate Python MCP stdio server scripts in the workspace
    // - Python tools: generate base64-embedded Python MCP stdio server scripts
    // - submit_output: generated when output_schema is defined
    let (has_mcp, generated_tool_files) = match build_all_mcp_config(
        &field.tools,
        &workspace,
        field.boundary.output_schema.as_ref(),
    )? {
        Some((config, files)) => {
            let json = serde_json::to_string_pretty(&config)?;
            write_workspace_file(&workspace, ".portlang_cc_mcp.json", &json)?;
            (true, files)
        }
        None => (false, vec![]),
    };

    // --- 5. Build runner script ---
    // Auth priority: CLAUDE_CODE_OAUTH_TOKEN env var > ANTHROPIC_API_KEY env var >
    // ~/.claude/.credentials.json (Claude Code's own credential store, written by `claude setup-token`)
    let (auth_env_var, auth_value) = if let Ok(token) = std::env::var("CLAUDE_CODE_OAUTH_TOKEN") {
        ("CLAUDE_CODE_OAUTH_TOKEN".to_string(), token)
    } else if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        ("ANTHROPIC_API_KEY".to_string(), key)
    } else if let Some(token) = read_claude_oauth_token() {
        ("CLAUDE_CODE_OAUTH_TOKEN".to_string(), token)
    } else {
        anyhow::bail!(
            "No credentials found. Run `claude setup-token` or set \
             CLAUDE_CODE_OAUTH_TOKEN / ANTHROPIC_API_KEY to use --runner claude-code"
        );
    };

    let model = normalize_model_name(&field.model.name);

    // Generate hook scripts for always/on_tool Shell verifiers (Phase 2).
    // Each script is written to the workspace and referenced from settings.json hooks.
    let generated_hook_files = generate_hook_scripts(&field.verifiers, &field.tools, &workspace)?;

    // Generate write boundary hook (Phase 3).
    // Enforces allow_write patterns on Claude Code's Write/Edit tool calls via PostToolUse hook.
    let boundary_hook_file = generate_write_boundary_hook(&field.boundary.allow_write, &workspace)?;

    // Write claude settings.json into the workspace so the runner script can install it at ~/.claude/
    // This pre-approves all tools and wires up PostToolUse hooks for always/on_tool Shell verifiers.
    // Uses settings.json instead of --dangerously-skip-permissions (which Claude blocks as root).
    let settings_json = claude_settings_json(
        &field.tools,
        &field.verifiers,
        &field.boundary.allow_write,
        field.boundary.output_schema.is_some(),
    );
    write_workspace_file(&workspace, ".portlang_cc_settings.json", &settings_json)?;

    let script = build_runner_script(&auth_env_var, &auth_value, &model, has_system, has_mcp);

    // --- 6. Spawn exec and stream JSONL output ---
    // The sandbox stages the script and returns live stdout/stderr streams.
    // Local backends write the script to the bind-mounted host workspace;
    // remote backends (e.g. Modal) inject it directly into the container.
    let mut exec_handle = sandbox
        .exec_script_streaming(&script)
        .await
        .context("Failed to start claude-code in sandbox")?;

    let mut lines = BufReader::new(exec_handle.stdout).lines();

    // Drain stderr concurrently to prevent pipe-buffer deadlock.
    // If stderr is not consumed while the subprocess writes to it, the OS pipe
    // buffer (~64 KB) fills up, the subprocess blocks on the write, stops
    // producing stdout, and our lines loop waits forever. This is especially
    // likely when npx downloads packages on first run (lots of npm output).
    // Log each line in real-time at debug level so it's visible with RUST_LOG=debug.
    let stderr = exec_handle.stderr;
    let stderr_task: tokio::task::JoinHandle<String> = tokio::spawn(async move {
        let mut collected = String::new();
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::debug!(target: "portlang_runner_claudecode::stderr", "{}", line);
            collected.push_str(&line);
            collected.push('\n');
        }
        collected
    });

    let mut acc = StreamAccumulator::new();
    let max_steps = field.boundary.max_steps.unwrap_or(u64::MAX);
    let max_cost_usd = field
        .boundary
        .max_cost
        .map(|c| c.microdollars() as f64 / 1_000_000.0)
        .unwrap_or(f64::MAX);
    let max_tokens = field.boundary.max_tokens.unwrap_or(u64::MAX);

    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }

        let done = acc.process_line(&line);

        // If the stream ended naturally, stop before checking budgets.
        // Token/cost counts only arrive on the final result event (same event that
        // sets done=true), so checking limits after that event would incorrectly
        // classify a successful run as BudgetExhausted.
        if done {
            break;
        }

        // Count only tool_call steps toward max_steps — text output steps are model
        // commentary, not agent actions, and shouldn't consume the step budget.
        let tool_steps = acc
            .steps
            .iter()
            .filter(|s| matches!(s.action, Action::ToolCall { .. }))
            .count() as u64;

        if tool_steps >= max_steps {
            let _ = exec_handle.exec.kill().await;
            trajectory.finish(RunOutcome::BudgetExhausted {
                reason: format!("Step limit {} exceeded", max_steps),
            });
            break;
        }
        if acc.cost_usd >= max_cost_usd {
            let _ = exec_handle.exec.kill().await;
            trajectory.finish(RunOutcome::BudgetExhausted {
                reason: format!(
                    "Cost limit ${:.4} exceeded (current: ${:.4})",
                    max_cost_usd, acc.cost_usd
                ),
            });
            break;
        }
        if acc.total_tokens() >= max_tokens {
            let _ = exec_handle.exec.kill().await;
            trajectory.finish(RunOutcome::BudgetExhausted {
                reason: format!(
                    "Token limit {} exceeded (current: {})",
                    max_tokens,
                    acc.total_tokens()
                ),
            });
            break;
        }
    }

    // Collect stderr and wait for process to exit
    let exit_code = exec_handle.exec.wait().await.ok().flatten();
    let stderr_output = stderr_task.await.unwrap_or_default();
    if !stderr_output.is_empty() {
        tracing::warn!("claude-code stderr:\n{}", stderr_output);
    }
    if acc.steps.is_empty() {
        tracing::warn!(
            "claude-code produced no output (exit code: {:?}). \
             Check ANTHROPIC_API_KEY/CLAUDE_CODE_OAUTH_TOKEN and that 'claude' is in PATH.",
            exit_code
        );
    }

    // --- 7. Attach cost/token info to final step ---
    let run_cost = Cost::from_dollars(acc.cost_usd);
    let total_tokens = acc.total_tokens();

    // Distribute cost+tokens to the last step (rough attribution)
    if let Some(last) = acc.steps.last_mut() {
        last.cost = run_cost;
        last.tokens_used = total_tokens;
        last.input_tokens = Some(acc.input_tokens);
        last.output_tokens = Some(acc.output_tokens);
    }

    // Normalize the submit_output tool name: Claude Code reports it as
    // mcp__submit_output__submit_output but verifiers expect submit_output.
    for step in &mut acc.steps {
        if let Action::ToolCall { tool, .. } = &mut step.action {
            if tool.as_str() == "mcp__submit_output__submit_output" {
                *tool = "submit_output".into();
            }
        }
    }

    // Extract structured output from the submit_output tool call input.
    // The agent passes its structured fields as tool arguments; we capture them
    // here so trajectory.structured_output is populated for the caller.
    for step in &acc.steps {
        if let Action::ToolCall { tool, input } = &step.action {
            if tool.as_str() == "submit_output" {
                trajectory.set_structured_output(input.clone());
                break;
            }
        }
    }

    // Move steps into trajectory
    let steps = std::mem::take(&mut acc.steps);
    for step in steps {
        trajectory.add_step(step);
    }

    // --- 8. Run on-stop verifiers ---
    let verifier_results = run_verifiers(
        sandbox.as_ref(),
        &field.verifiers,
        &Action::Stop,
        true,
        None,
        None,
        &trajectory.steps,
    )
    .await;

    // Attach verifier results to a synthetic final step
    if !verifier_results.is_empty() {
        let step_number = trajectory.steps.len() + 1;
        let mut final_step = TrajectoryStep::new(
            step_number,
            Action::Stop,
            "claude-code completed".to_string(),
            false,
            Cost::ZERO,
            0,
        );
        final_step.verifier_results = verifier_results.clone();
        trajectory.add_step(final_step);
    }

    // --- 9. Determine outcome (if not already set by budget check) ---
    if trajectory.outcome.is_none() {
        let failed: Vec<_> = verifier_results.iter().filter(|r| !r.passed).collect();
        if let Some(v) = failed.first() {
            trajectory.finish(RunOutcome::VerifierFailed {
                verifier: v.name.clone(),
                message: v.stderr.clone(),
            });
        } else if acc.is_success() || trajectory.outcome.is_none() {
            trajectory.finish(RunOutcome::Converged {
                message: "Claude Code completed successfully".to_string(),
            });
        }
    }

    // --- 9b. Populate trajectory with skill data ---
    if !skills.is_empty() {
        let skills_available: Vec<String> = skills.iter().map(|s| s.slug.clone()).collect();
        let skills_invoked =
            detect_skill_invocations(&trajectory.steps, &field.prompt.goal, &skills);
        trajectory.set_skills(skills_available, skills_invoked);
    }

    // --- 10. Cleanup temp files ---
    for name in &[
        ".portlang_cc_goal.txt",
        ".portlang_cc_system.txt",
        ".portlang_cc_mcp.json",
        ".portlang_cc_runner.sh",
        ".portlang_cc_settings.json",
        ".portlang_mcp_submit_output.py",
    ] {
        let _ = std::fs::remove_file(workspace.join(name));
    }
    for name in &generated_tool_files {
        let _ = std::fs::remove_file(workspace.join(name));
    }
    for name in &generated_hook_files {
        let _ = std::fs::remove_file(workspace.join(name));
    }
    if let Some(ref name) = boundary_hook_file {
        let _ = std::fs::remove_file(workspace.join(name));
    }
    // Cleanup skill files from workspace
    let _ = tokio::fs::remove_dir_all(workspace.join(".portlang_skills")).await;
    // Cleanup registered Claude Code skills
    cleanup_registered_skills(&registered_skills).await;

    Ok(trajectory)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Append structured output instructions to the goal when `output_schema` is defined,
/// mirroring the native runner's context builder.
fn build_goal_with_output_schema(goal: &str, output_schema: Option<&serde_json::Value>) -> String {
    let Some(schema) = output_schema else {
        return goal.to_string();
    };
    let schema_pretty = serde_json::to_string_pretty(schema).unwrap_or_else(|_| "{}".to_string());
    format!(
        "{}\n\n# Structured Output\n\nThis task requires structured output matching this schema:\n\n```json\n{}\n```\n\nWhen you're ready to submit your results, use ToolSearch with query `select:mcp__submit_output__submit_output` to fetch the tool, then call it passing your JSON fields directly as the tool arguments.\n",
        goal, schema_pretty
    )
}

/// Returns a cloned Environment with required packages injected:
/// - "claude-code" always (the Claude Code CLI)
/// - "uv" when the field has python tools or an output_schema (submit_output uses uv run)
///
/// Skipped when a custom image or Dockerfile is provided (we assume they
/// already have the necessary tools).
pub fn with_required_packages(
    env: &Environment,
    tools: &[portlang_core::Tool],
    has_output_schema: bool,
) -> Environment {
    // Pre-built images are assumed to already contain everything they need.
    // Custom Dockerfiles are NOT skipped — the sandbox will build a composite image
    // (user Dockerfile base + packages) so claude-code is always available.
    if env.image.is_some() {
        return env.clone();
    }
    let mut cloned = env.clone();
    if !cloned.packages.iter().any(|p| p == "claude-code") {
        cloned.packages.push("claude-code".to_string());
    }
    let has_python_tools = tools.iter().any(|t| t.tool_type == "python");
    if (has_python_tools || has_output_schema) && !cloned.packages.iter().any(|p| p == "uv") {
        cloned.packages.push("uv".to_string());
    }
    cloned
}

/// Write a string to a file inside the host-side workspace directory.
fn write_workspace_file(workspace: &Path, name: &str, content: &str) -> Result<()> {
    std::fs::write(workspace.join(name), content)
        .with_context(|| format!("Failed to write {}", name))
}

/// Stage input data into the workspace before agent starts.
async fn stage_input(workspace: &Path, input: &InputSource) -> Result<()> {
    match input {
        InputSource::File(src) => {
            let filename = src
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "portlang_input".to_string());
            tokio::fs::copy(src, workspace.join(&filename))
                .await
                .with_context(|| format!("Failed to copy input file '{}'", src.display()))?;
        }
        InputSource::Inline(content) => {
            tokio::fs::write(workspace.join("portlang_input.json"), content)
                .await
                .context("Failed to write inline input")?;
        }
    }
    Ok(())
}

/// Normalize a portlang model name for the Claude Code CLI.
///
/// Portlang fields use OpenRouter-style names like `anthropic/claude-sonnet-4.6`.
/// Claude Code CLI expects names like `claude-sonnet-4-6` (no provider prefix, dots → dashes).
fn normalize_model_name(model: &str) -> String {
    let without_prefix = model.find('/').map(|i| &model[i + 1..]).unwrap_or(model);
    without_prefix.replace('.', "-")
}

/// Try to read the OAuth token from Claude Code's credential store (~/.claude/.credentials.json).
///
/// Claude Code writes this file when the user runs `claude setup-token`. Reading it
/// here means users don't need to manually export CLAUDE_CODE_OAUTH_TOKEN.
fn read_claude_oauth_token() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let path = std::path::Path::new(&home)
        .join(".claude")
        .join(".credentials.json");
    let content = std::fs::read_to_string(&path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    json.get("claudeAiOauth")?
        .get("accessToken")?
        .as_str()
        .map(|s| s.to_string())
}

// Static body of the write boundary hook (patterns injected at generation time).
const BOUNDARY_HOOK_BODY: &str = r#"
def is_allowed(path):
    if path.startswith('./'):
        path = path[2:]
    for prefix in ['/workspace/', 'workspace/']:
        if path.startswith(prefix):
            path = path[len(prefix):]
            break
    return any(fnmatch.fnmatch(path, pat) for pat in ALLOW_PATTERNS)

try:
    data = json.load(sys.stdin)
    path = data.get('tool_input', {}).get('path', '')
    if path and not is_allowed(path):
        full = path if os.path.isabs(path) else os.path.join('/workspace', path)
        try:
            os.remove(full)
        except OSError:
            pass
        sys.stderr.write("Boundary violation: write to '{}' not permitted. allow_write: {}\n".format(path, ALLOW_PATTERNS))
        sys.exit(1)
except Exception:
    pass
"#;

/// Generate the write boundary hook script when `allow_write` patterns are declared.
///
/// Enforces `boundary.allow_write` on Claude Code's Write and Edit tool calls via a
/// PostToolUse hook. When the written path doesn't match any pattern the hook deletes
/// the file and exits non-zero, causing Claude Code to surface the violation.
///
/// Only generated when `allow_write` is non-empty. Fields without `allow_write` keep
/// unrestricted write access (matching the intent of not specifying a boundary).
fn generate_write_boundary_hook(
    allow_write: &[String],
    workspace: &Path,
) -> Result<Option<String>> {
    if allow_write.is_empty() {
        return Ok(None);
    }
    let patterns_json = serde_json::to_string(allow_write).unwrap_or_else(|_| "[]".to_string());
    let script = format!(
        "#!/usr/bin/env python3\nimport json, sys, os, fnmatch\n\nALLOW_PATTERNS = {}\n{}",
        patterns_json, BOUNDARY_HOOK_BODY
    );
    let filename = ".portlang_boundary_write.py".to_string();
    std::fs::write(workspace.join(&filename), &script)
        .context("Failed to write boundary hook script")?;
    Ok(Some(filename))
}

/// Sanitize a name for use in filenames and identifiers (replaces non-alphanumeric with `_`).
fn sanitize_hook_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Map a portlang tool name (lowercase) to the Claude Code PostToolUse matcher string.
///
/// Built-in portlang tools map to Claude Code's PascalCase tool names.
/// Custom shell/python/mcp tools map to their MCP tool names (`mcp__server__tool`).
fn tool_name_to_matcher(tool_name: &str, tools: &[portlang_core::Tool]) -> String {
    match tool_name {
        "write" => return "Write".to_string(),
        "read" => return "Read".to_string(),
        "bash" => return "Bash".to_string(),
        "edit" => return "Edit".to_string(),
        "glob" => return "Glob".to_string(),
        "webfetch" => return "WebFetch".to_string(),
        "websearch" => return "WebSearch".to_string(),
        "todowrite" => return "TodoWrite".to_string(),
        "todoread" => return "TodoRead".to_string(),
        "notebookread" => return "NotebookRead".to_string(),
        "notebookedit" => return "NotebookEdit".to_string(),
        _ => {}
    }
    // Check custom tools for MCP name mapping
    for tool in tools {
        if tool.name.as_deref() == Some(tool_name) {
            let sanitized = sanitize_hook_name(tool_name);
            return match tool.tool_type.as_str() {
                "shell" => format!("mcp__{}__{}", sanitized, sanitized),
                "python" => {
                    let func = tool.function.as_deref().unwrap_or("execute");
                    format!("mcp__{}__{}", sanitized, func)
                }
                "mcp" => format!("mcp__{}", sanitized),
                _ => tool_name.to_string(),
            };
        }
    }
    // Fallback: capitalize first letter (best-effort for unknown tool names)
    let mut chars = tool_name.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Generate shell hook scripts for all always/on_tool Shell verifiers.
///
/// Each script is written to the workspace as `.portlang_hook_<name>.sh` and
/// referenced from the `hooks.PostToolUse` section of settings.json.
/// Non-zero exit code from a hook causes Claude Code to surface the error to the agent.
fn generate_hook_scripts(
    verifiers: &[portlang_core::Verifier],
    tools: &[portlang_core::Tool],
    workspace: &Path,
) -> Result<Vec<String>> {
    let _ = tools; // reserved for future matcher validation
    let mut files = Vec::new();
    for verifier in verifiers {
        let is_hook_trigger = matches!(
            verifier.trigger,
            VerifierTrigger::Always | VerifierTrigger::OnTool(_)
        );
        if !is_hook_trigger {
            continue;
        }
        let command = match &verifier.algorithm {
            VerifierAlgorithm::Shell { command } => command,
            _ => continue, // ToolCall/Levenshtein/Semantic can't run as shell hooks
        };
        let sanitized = sanitize_hook_name(&verifier.name);
        let filename = format!(".portlang_hook_{}.sh", sanitized);
        let script = format!("#!/bin/sh\ncd /workspace\n{}\n", command);
        std::fs::write(workspace.join(&filename), &script)
            .with_context(|| format!("Failed to write hook script '{}'", verifier.name))?;
        files.push(filename);
    }
    Ok(files)
}

/// Generate the Claude Code settings.json content that pre-approves all tools and
/// wires up PostToolUse hooks for always/on_tool Shell verifiers.
///
/// Written into /workspace and copied to ~/.claude/settings.json inside the container
/// before claude runs. This pre-approves all built-in tools and any MCP tools defined
/// in the field, replacing the need for --dangerously-skip-permissions (which Claude
/// blocks when running as root).
///
/// For shell/python tools, the generated MCP server registers a single tool with the
/// same name as the server, so the permission is `mcp__<name>__<name>`.
/// For MCP tools (whose sub-tool names are unknown at generation time), we allow
/// the entire server namespace with `mcp__<server>`.
fn claude_settings_json(
    tools: &[portlang_core::Tool],
    verifiers: &[portlang_core::Verifier],
    allow_write: &[String],
    has_output_schema: bool,
) -> String {
    let mut mcp_perms: Vec<String> = Vec::new();
    for tool in tools {
        if let Some(ref name) = tool.name {
            let sanitized = sanitize_hook_name(name);
            let perm = match tool.tool_type.as_str() {
                "shell" => format!("mcp__{}__{}", sanitized, sanitized),
                "python" => {
                    let func = tool.function.as_deref().unwrap_or("execute");
                    format!("mcp__{}__{}", sanitized, func)
                }
                "mcp" => format!("mcp__{}", sanitized),
                _ => continue,
            };
            mcp_perms.push(format!("      \"{}\"", perm));
        }
    }

    if has_output_schema {
        mcp_perms.push("      \"mcp__submit_output__submit_output\"".to_string());
    }

    let mcp_block = if mcp_perms.is_empty() {
        String::new()
    } else {
        format!(",\n{}", mcp_perms.join(",\n"))
    };

    // Build PostToolUse hooks for always/on_tool Shell verifiers
    let mut hook_entries: Vec<String> = Vec::new();
    for verifier in verifiers {
        let is_hook_trigger = matches!(
            verifier.trigger,
            VerifierTrigger::Always | VerifierTrigger::OnTool(_)
        );
        if !is_hook_trigger {
            continue;
        }
        if !matches!(verifier.algorithm, VerifierAlgorithm::Shell { .. }) {
            continue;
        }
        let sanitized = sanitize_hook_name(&verifier.name);
        let hook_cmd = format!("sh /workspace/.portlang_hook_{}.sh", sanitized);
        let entry = match &verifier.trigger {
            VerifierTrigger::Always => {
                format!(
                    "      {{\"hooks\": [{{\"type\": \"command\", \"command\": \"{}\"}}]}}",
                    hook_cmd
                )
            }
            VerifierTrigger::OnTool(tool_name) => {
                let matcher = tool_name_to_matcher(tool_name, tools);
                format!(
                    "      {{\"matcher\": \"{}\", \"hooks\": [{{\"type\": \"command\", \"command\": \"{}\"}}]}}",
                    matcher, hook_cmd
                )
            }
            _ => continue,
        };
        hook_entries.push(entry);
    }

    // Boundary enforcement hook: fires on Write and Edit to enforce allow_write patterns.
    if !allow_write.is_empty() {
        hook_entries.push(
            "      {\"matcher\": \"Write|Edit\", \"hooks\": [{\"type\": \"command\", \"command\": \"python3 /workspace/.portlang_boundary_write.py\"}]}".to_string()
        );
    }

    let hooks_block = if hook_entries.is_empty() {
        String::new()
    } else {
        format!(
            ",\n  \"hooks\": {{\n    \"PostToolUse\": [\n{}\n    ]\n  }}",
            hook_entries.join(",\n")
        )
    };

    format!(
        r#"{{
  "permissions": {{
    "allow": [
      "Bash(*)",
      "Edit(*)",
      "Write(*)",
      "Read(*)",
      "Glob(*)",
      "WebFetch(*)",
      "WebSearch(*)",
      "TodoWrite(*)",
      "TodoRead(*)",
      "NotebookRead(*)",
      "NotebookEdit(*)"{}
    ],
    "deny": ["AskUserQuestion"]
  }}{}
}}"#,
        mcp_block, hooks_block
    )
}

/// Build the shell script that runs inside the container.
///
/// Goal and system prompt are read from files written to /workspace to avoid
/// any shell-escaping issues with arbitrary user content.
fn build_runner_script(
    auth_env_var: &str,
    auth_value: &str,
    model: &str,
    has_system: bool,
    has_mcp: bool,
) -> String {
    // Escape single quotes in the auth value for embedding in a single-quoted shell string.
    let escaped_value = auth_value.replace('\'', "'\\''");

    let mut script = format!(
        "#!/bin/sh\n\
         export {}='{}'\n\
         export HOME=/root\n\
         export PATH=\"/root/.local/bin:$PATH\"\n\
         mkdir -p /root/.claude\n\
         cp /workspace/.portlang_cc_settings.json /root/.claude/settings.json\n\
         GOAL=$(cat /workspace/.portlang_cc_goal.txt)\n",
        auth_env_var, escaped_value
    );

    if has_system {
        script.push_str("SYSTEM=$(cat /workspace/.portlang_cc_system.txt)\n");
    }

    // Build claude invocation.
    // --verbose is required for stream-json output in --print mode.
    // Permissions are pre-approved via ~/.claude/settings.json (see claude_settings_json()).
    // --dangerously-skip-permissions is NOT used because Claude blocks it when running as root.
    let mut claude_cmd = format!(
        "claude --print \"$GOAL\" \
         --output-format stream-json \
         --verbose \
         --model {}",
        model
    );

    if has_system {
        claude_cmd.push_str(" --system-prompt \"$SYSTEM\"");
    }

    if has_mcp {
        claude_cmd.push_str(" --mcp-config /workspace/.portlang_cc_mcp.json");
    }

    script.push_str(&claude_cmd);
    script.push('\n');
    script
}

// ---------------------------------------------------------------------------
// Skills helpers (claude-code specific — shared helpers live in portlang-skills)
// ---------------------------------------------------------------------------

/// Register skills in `~/.claude/skills/<slug>/SKILL.md` so Claude Code's
/// native `$slug` invocation shorthand works in goal/prompt text.
/// Returns the list of slugs that were successfully registered for later cleanup.
async fn register_skills_with_claude_code(skills: &[Skill]) -> Vec<String> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let cc_skills_dir = home.join(".claude").join("skills");
    let mut registered = Vec::new();

    for skill in skills {
        let Some(ref content) = skill.content else {
            continue;
        };
        let skill_dir = cc_skills_dir.join(&skill.slug);
        if let Err(e) = tokio::fs::create_dir_all(&skill_dir).await {
            tracing::warn!("Failed to create CC skill dir for {}: {}", skill.slug, e);
            continue;
        }
        let skill_path = skill_dir.join("SKILL.md");
        if let Err(e) = tokio::fs::write(&skill_path, content).await {
            tracing::warn!("Failed to write CC skill for {}: {}", skill.slug, e);
            continue;
        }
        registered.push(skill.slug.clone());
    }
    registered
}

/// Remove skills that were temporarily registered in `~/.claude/skills/`.
async fn cleanup_registered_skills(slugs: &[String]) {
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let cc_skills_dir = home.join(".claude").join("skills");
    for slug in slugs {
        let skill_dir = cc_skills_dir.join(slug);
        if let Err(e) = tokio::fs::remove_dir_all(&skill_dir).await {
            tracing::debug!("Failed to clean up CC skill {}: {}", slug, e);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use portlang_core::{Cost, SkillSourceKind};
    use portlang_skills::{
        build_skill_metadata_block, detect_skill_invocations, extract_skill_description,
        write_skills_to_workspace,
    };
    use std::path::PathBuf;

    // --- helpers ---

    fn make_skill(slug: &str, content: Option<&str>) -> Skill {
        Skill {
            source: slug.to_string(),
            kind: SkillSourceKind::Local {
                path: PathBuf::from(format!("/fake/{}.md", slug)),
            },
            slug: slug.to_string(),
            content: content.map(|s| s.to_string()),
            resources: Vec::new(),
        }
    }

    fn text_step(text: &str) -> TrajectoryStep {
        TrajectoryStep::new(
            1,
            Action::TextOutput {
                text: text.to_string(),
            },
            String::new(),
            false,
            Cost::ZERO,
            0,
        )
    }

    fn tool_step(tool: &str, input: serde_json::Value) -> TrajectoryStep {
        TrajectoryStep::new(
            2,
            Action::ToolCall {
                tool: tool.to_string().into(),
                input,
            },
            String::new(),
            false,
            Cost::ZERO,
            0,
        )
    }

    // --- detect_skill_invocations ---

    #[test]
    fn detect_dollar_slug_in_goal() {
        let skills = vec![make_skill("my-skill", Some("content"))];
        let result = detect_skill_invocations(&[], "$my-skill do the thing", &skills);
        assert_eq!(result, vec!["my-skill"]);
    }

    #[test]
    fn detect_cat_pattern_in_goal() {
        let skills = vec![make_skill("my-skill", Some("content"))];
        let result =
            detect_skill_invocations(&[], "cat .portlang_skills/my-skill/SKILL.md", &skills);
        assert_eq!(result, vec!["my-skill"]);
    }

    #[test]
    fn detect_dollar_slug_in_text_step() {
        let skills = vec![make_skill("analyze", Some("content"))];
        let steps = vec![text_step("I'll use $analyze to check this")];
        let result = detect_skill_invocations(&steps, "do the task", &skills);
        assert_eq!(result, vec!["analyze"]);
    }

    #[test]
    fn detect_cat_pattern_in_tool_call_input() {
        let skills = vec![make_skill("refactor", Some("content"))];
        let input = serde_json::json!({"command": "cat .portlang_skills/refactor/SKILL.md"});
        let steps = vec![tool_step("Bash", input)];
        let result = detect_skill_invocations(&steps, "refactor the code", &skills);
        assert_eq!(result, vec!["refactor"]);
    }

    #[test]
    fn no_false_positive_partial_slug_match() {
        // "skill" should not match "$my-skill"
        let skills = vec![make_skill("my-skill", Some("content"))];
        let steps = vec![text_step("I have skill in coding")];
        let result = detect_skill_invocations(&steps, "show your skill", &skills);
        assert!(result.is_empty());
    }

    #[test]
    fn no_false_positive_wrong_slug() {
        let skills = vec![make_skill("skill-a", Some("c"))];
        let steps = vec![text_step("I'll use $skill-b here")];
        let result = detect_skill_invocations(&steps, "$skill-b run", &skills);
        // skill-b is not in the declared skills list → not detected
        assert!(result.is_empty());
    }

    #[test]
    fn detect_multiple_skills_only_matching_ones() {
        let skills = vec![
            make_skill("alpha", Some("c1")),
            make_skill("beta", Some("c2")),
            make_skill("gamma", Some("c3")),
        ];
        let steps = vec![text_step("using $alpha here"), text_step("and $gamma too")];
        let mut result = detect_skill_invocations(&steps, "goal", &skills);
        result.sort();
        assert_eq!(result, vec!["alpha", "gamma"]);
    }

    #[test]
    fn unresolved_skill_still_detected_via_dollar_slug() {
        // Detection scans goal/steps by slug regardless of whether content is Some
        let skills = vec![make_skill("raw", None)];
        let result = detect_skill_invocations(&[], "$raw", &skills);
        assert_eq!(result, vec!["raw"]);
    }

    // --- extract_skill_description ---

    #[test]
    fn extract_description_from_frontmatter() {
        let content = "---\nname: My Skill\ndescription: Does useful things\n---\n# Body";
        assert_eq!(
            extract_skill_description(content),
            Some("Does useful things".to_string())
        );
    }

    #[test]
    fn extract_description_quoted_value() {
        let content = "---\ndescription: \"Handles refactoring tasks\"\n---\n";
        assert_eq!(
            extract_skill_description(content),
            Some("Handles refactoring tasks".to_string())
        );
    }

    #[test]
    fn extract_description_missing_returns_none() {
        let content = "---\nname: My Skill\nversion: 1.0\n---\n# Body";
        assert_eq!(extract_skill_description(content), None);
    }

    #[test]
    fn extract_description_no_frontmatter_returns_none() {
        let content = "# Just a markdown file\nNo frontmatter here.";
        assert_eq!(extract_skill_description(content), None);
    }

    #[test]
    fn extract_description_empty_content_returns_none() {
        assert_eq!(extract_skill_description(""), None);
    }

    // --- build_skill_metadata_block ---

    #[test]
    fn metadata_block_empty_when_no_skills() {
        assert_eq!(build_skill_metadata_block(&[]), "");
    }

    #[test]
    fn metadata_block_empty_when_all_unresolved() {
        let skills = vec![make_skill("foo", None), make_skill("bar", None)];
        assert_eq!(build_skill_metadata_block(&skills), "");
    }

    #[test]
    fn metadata_block_contains_slug_and_path() {
        let content = "---\ndescription: Analyzes code quality\n---\n# Body";
        let skills = vec![make_skill("quality-check", Some(content))];
        let block = build_skill_metadata_block(&skills);
        assert!(block.contains("quality-check"));
        assert!(block.contains("/workspace/.portlang_skills/quality-check/SKILL.md"));
        assert!(block.contains("Analyzes code quality"));
    }

    #[test]
    fn metadata_block_omits_skill_without_description() {
        // Spec: a skill with no description field must be omitted from the catalog
        // (description is essential — without it the agent cannot know when to activate).
        let skills = vec![make_skill(
            "no-desc-skill",
            Some("# Just content, no frontmatter"),
        )];
        let block = build_skill_metadata_block(&skills);
        assert!(
            block.is_empty(),
            "skill lacking a description must not appear in catalog"
        );
    }

    #[test]
    fn metadata_block_skips_unresolved_but_includes_resolved() {
        let skills = vec![
            make_skill("resolved", Some("---\ndescription: Works great\n---\n")),
            make_skill("unresolved", None),
        ];
        let block = build_skill_metadata_block(&skills);
        assert!(block.contains("resolved"));
        assert!(!block.contains("unresolved"));
    }

    // --- write_skills_to_workspace ---

    #[tokio::test]
    async fn write_skills_creates_skill_md_files() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path();

        let skills = vec![
            make_skill("skill-a", Some("# Skill A content")),
            make_skill("skill-b", Some("# Skill B content")),
        ];

        write_skills_to_workspace(&skills, workspace).await.unwrap();

        let path_a = workspace.join(".portlang_skills/skill-a/SKILL.md");
        let path_b = workspace.join(".portlang_skills/skill-b/SKILL.md");
        assert!(path_a.exists(), "SKILL.md for skill-a should exist");
        assert!(path_b.exists(), "SKILL.md for skill-b should exist");
        assert_eq!(
            std::fs::read_to_string(&path_a).unwrap(),
            "# Skill A content"
        );
        assert_eq!(
            std::fs::read_to_string(&path_b).unwrap(),
            "# Skill B content"
        );
    }

    #[tokio::test]
    async fn write_skills_skips_unresolved() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path();

        let skills = vec![
            make_skill("present", Some("content")),
            make_skill("absent", None),
        ];

        write_skills_to_workspace(&skills, workspace).await.unwrap();

        assert!(workspace.join(".portlang_skills/present/SKILL.md").exists());
        assert!(!workspace.join(".portlang_skills/absent").exists());
    }

    #[tokio::test]
    async fn write_skills_no_op_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path();

        write_skills_to_workspace(&[], workspace).await.unwrap();

        // No .portlang_skills directory should be created
        assert!(!workspace.join(".portlang_skills").exists());
    }
}
