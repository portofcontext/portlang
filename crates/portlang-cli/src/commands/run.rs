use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use portlang_adapt::{format_report, AdaptationReport};
use portlang_config::{apply_runtime_context, parse_field_with_parent, resolve_parent_config};
use portlang_core::RuntimeContext;
use portlang_provider_anthropic::AnthropicProvider;
use portlang_provider_openrouter::OpenRouterProvider;
use portlang_runner_claudecode::{run_field_with_claude_code, with_required_packages};
use portlang_runtime::{run_field, sandbox::create_sandbox, tools::ToolRegistry, ModelProvider};
use portlang_trajectory::FilesystemStore;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;

use crate::output_collector::{
    collect_artifacts, copy_artifacts_to_dir, effective_collect_patterns, CollectedArtifact,
};

/// Machine-readable result emitted to stdout when `--json` is passed.
#[derive(Serialize)]
struct RunResult {
    run_id: String,
    field: String,
    outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    structured_output: Option<serde_json::Value>,
    artifacts: Vec<CollectedArtifact>,
    cost: String,
    tokens: u64,
    steps: usize,
    duration_ms: u64,
    trajectory_id: String,
}

pub async fn run_command(
    field_path: PathBuf,
    parent_field: Option<PathBuf>,
    ctx: RuntimeContext,
    runner: String,
    backend: Option<String>,
    backend_url: Option<String>,
    backend_command: Option<String>,
    dry_run: bool,
    runs: usize,
    auto_reflect: bool,
    output_dir: Option<PathBuf>,
    json_output: bool,
) -> Result<()> {
    if dry_run {
        return run_dry(field_path, parent_field, ctx);
    }
    if runs > 1 {
        return run_multi(
            field_path,
            runs,
            parent_field,
            ctx,
            runner,
            backend,
            backend_url,
            backend_command,
        )
        .await;
    }
    run_single(
        field_path,
        parent_field,
        ctx,
        runner,
        backend,
        backend_url,
        backend_command,
        auto_reflect,
        output_dir,
        json_output,
    )
    .await
}

fn run_dry(field_path: PathBuf, parent_field: Option<PathBuf>, ctx: RuntimeContext) -> Result<()> {
    println!("Checking field: {}", field_path.display());

    let parent = resolve_parent_config(&field_path, parent_field.as_ref())?;
    let field = parse_field_with_parent(&field_path, parent.as_ref())?;

    // Show declared template variables and their status
    if !field.vars.is_empty() {
        println!("  Template variables:");
        let mut var_names: Vec<&String> = field.vars.keys().collect();
        var_names.sort();
        for name in var_names {
            let decl = &field.vars[name];
            let status = if ctx.vars.contains_key(name) {
                format!("supplied ({})", ctx.vars[name])
            } else if let Some(ref default) = decl.default {
                format!("default ({})", default)
            } else {
                "MISSING (required)".to_string()
            };
            let desc = decl
                .description
                .as_deref()
                .map(|d| format!(" — {}", d))
                .unwrap_or_default();
            println!("    {{{{ {} }}}}  {}{}", name, status, desc);
        }
        println!();
    }

    // Apply runtime context to validate templates and catch missing vars
    let field = apply_runtime_context(field, &ctx)?;

    println!("✓ Field '{}' is valid", field.name);
    println!("  Model: {}", field.model.name);

    if let Some(description) = &field.description {
        println!("  Description: {}", description);
    }

    if !field.boundary.allow_write.is_empty() {
        println!("  Write permissions: {:?}", field.boundary.allow_write);
    }

    if let Some(max_tokens) = field.boundary.max_tokens {
        println!("  Token budget: {}", max_tokens);
    }

    if let Some(max_cost) = &field.boundary.max_cost {
        println!("  Cost budget: {}", max_cost);
    }

    if !field.verifiers.is_empty() {
        let run_verifiers: Vec<_> = field.verifiers.iter().filter(|v| !v.eval_only).collect();
        let eval_verifiers: Vec<_> = field.verifiers.iter().filter(|v| v.eval_only).collect();
        println!("  Verifiers: {}", field.verifiers.len());
        for verifier in &run_verifiers {
            println!("    - {} ({:?})", verifier.name, verifier.trigger);
        }
        for verifier in &eval_verifiers {
            println!(
                "    - {} ({:?}) [eval only]",
                verifier.name, verifier.trigger
            );
        }
    }

    Ok(())
}

async fn run_single(
    field_path: PathBuf,
    parent_field: Option<PathBuf>,
    ctx: RuntimeContext,
    runner: String,
    backend: Option<String>,
    backend_url: Option<String>,
    backend_command: Option<String>,
    auto_reflect: bool,
    output_dir: Option<PathBuf>,
    json_output: bool,
) -> Result<()> {
    if !json_output {
        println!("Running field: {}", field_path.display());
    }

    let parent = resolve_parent_config(&field_path, parent_field.as_ref())?;
    let field = parse_field_with_parent(&field_path, parent.as_ref())?;
    let mut field = apply_runtime_context(field, &ctx)?;
    field.verifiers.retain(|v| !v.eval_only);

    if !json_output {
        println!("Field: {}", field.name);
        if let Some(description) = &field.description {
            println!("Description: {}", description);
        }
        println!("Model: {}", field.model.name);
        println!("Goal: {}", field.prompt.goal);
        println!("Runner: {}", runner);
        println!();
    }

    // Determine collect patterns before moving `field` into the runner
    let workspace_root = field.environment.root.clone();
    let collect_patterns =
        effective_collect_patterns(&field.boundary.allow_write, &field.boundary.collect);

    let progress = if !json_output {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} {msg}")
                .unwrap(),
        );
        crate::progress::set_status("");
        pb.enable_steady_tick(std::time::Duration::from_millis(100));
        let pb2 = pb.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                let msg = crate::progress::get_status();
                if !msg.is_empty() {
                    pb2.set_message(msg);
                }
            }
        });
        Some(pb)
    } else {
        None
    };

    let trajectory = match runner.as_str() {
        "claude-code" => {
            if !json_output {
                println!("Using Claude Code runner");
            }
            let env = with_required_packages(
                &field.environment,
                &field.tools,
                field.boundary.output_schema.is_some(),
            );
            let sandbox = create_sandbox(
                &env,
                &field.boundary,
                Arc::new(ToolRegistry::new()),
                backend.as_deref(),
                backend_url.as_deref(),
                backend_command.as_deref(),
            )
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create sandbox: {}", e))?;
            run_field_with_claude_code(&field, &ctx, sandbox).await?
        }
        _ => {
            let provider: Box<dyn ModelProvider> = if field.model.name.contains('/') {
                if !json_output {
                    println!("Using OpenRouter provider");
                }
                let mut p = OpenRouterProvider::from_env(&field.model.name)?;
                if let Some(temp) = field.model.temperature {
                    p = p.with_temperature(temp);
                }
                Box::new(p)
            } else {
                if !json_output {
                    println!("Using Anthropic provider");
                }
                let mut p = AnthropicProvider::from_env(&field.model.name)?;
                if let Some(temp) = field.model.temperature {
                    p = p.with_temperature(temp);
                }
                Box::new(p)
            };
            run_field(&field, provider.as_ref(), &ctx).await?
        }
    };

    if let Some(pb) = progress {
        pb.finish_and_clear();
    }

    // Save trajectory (redacting env var secrets)
    let store = FilesystemStore::new()?;
    store.save_redacted(&trajectory, &field.collect_secret_candidates())?;

    // Collect artifacts from workspace
    let workspace_path = std::path::Path::new(&workspace_root);
    let artifacts = if !collect_patterns.is_empty() {
        match collect_artifacts(workspace_path, &collect_patterns) {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!("Artifact collection failed: {}", e);
                vec![]
            }
        }
    } else {
        vec![]
    };

    // Copy to --output-dir if requested
    if let Some(ref out_dir) = output_dir {
        if !artifacts.is_empty() {
            copy_artifacts_to_dir(&artifacts, workspace_path, out_dir)?;
        } else {
            std::fs::create_dir_all(out_dir)?;
        }
    }

    if json_output {
        let duration_ms = trajectory
            .ended_at
            .map(|e| (e - trajectory.started_at).num_milliseconds() as u64)
            .unwrap_or(0);

        let result = RunResult {
            run_id: trajectory
                .id
                .filename()
                .trim_end_matches(".json")
                .to_string(),
            field: trajectory.field_name.clone(),
            outcome: trajectory
                .outcome
                .as_ref()
                .map(|o| o.description().to_string())
                .unwrap_or_default(),
            structured_output: trajectory.structured_output.clone(),
            artifacts: artifacts
                .into_iter()
                .filter(|a| a.content.is_some())
                .collect(),
            cost: format!("{}", trajectory.total_cost),
            tokens: trajectory.total_tokens,
            steps: trajectory.step_count(),
            duration_ms,
            trajectory_id: trajectory
                .id
                .filename()
                .trim_end_matches(".json")
                .to_string(),
        };
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    // Human-readable output
    println!("\n{}", "=".repeat(60));
    println!("Run completed!");
    println!("{}", "=".repeat(60));

    println!(
        "Outcome: {}",
        trajectory.outcome.as_ref().unwrap().description()
    );
    println!("Steps: {}", trajectory.step_count());
    println!("Total tokens: {}", trajectory.total_tokens);
    println!("Total cost: {}", trajectory.total_cost);
    println!(
        "Duration: {:?}",
        trajectory.ended_at.unwrap() - trajectory.started_at
    );

    println!("\nTrajectory saved:");
    println!("  Field: {}", trajectory.field_name);
    println!("  ID: {}", trajectory.id.filename());
    println!(
        "  Path: ~/.portlang/trajectories/{}/{}",
        trajectory.field_name,
        trajectory.id.filename()
    );

    if !artifacts.is_empty() {
        println!("\nArtifacts ({}):", artifacts.len());
        for a in &artifacts {
            println!("  {}", a.path);
        }
        if let Some(ref out_dir) = output_dir {
            println!("  → copied to {}", out_dir.display());
        } else {
            println!("  (in {})", workspace_root);
        }
    }

    if trajectory.outcome.as_ref().unwrap().is_success() {
        println!("\n✓ Field converged successfully!");
    } else {
        println!("\n✗ Field did not converge");
    }

    println!("\nTo view the trajectory:");
    println!("  portlang view trajectory {}", trajectory.id.filename());

    if auto_reflect {
        println!();
        let traj_id = trajectory
            .id
            .filename()
            .trim_end_matches(".json")
            .to_string();
        crate::commands::reflect::reflect_command(
            Some(trajectory.field_name.clone()),
            Some(traj_id),
            1,
            runner,
        )
        .await?;
    }

    Ok(())
}

async fn run_multi(
    field_path: PathBuf,
    runs: usize,
    parent_field: Option<PathBuf>,
    ctx: RuntimeContext,
    runner: String,
    backend: Option<String>,
    backend_url: Option<String>,
    backend_command: Option<String>,
) -> Result<()> {
    println!("Convergence analysis: {}", field_path.display());
    println!("Runs: {}", runs);
    println!();

    let parent = resolve_parent_config(&field_path, parent_field.as_ref())?;
    let field = parse_field_with_parent(&field_path, parent.as_ref())?;
    let mut field = apply_runtime_context(field, &ctx)?;
    field.verifiers.retain(|v| !v.eval_only);
    println!("Field: {}", field.name);

    if let Some(description) = &field.description {
        println!("Description: {}", description);
    }

    println!("Model: {}", field.model.name);
    println!("Goal: {}", field.prompt.goal);
    println!();

    let store = FilesystemStore::new()?;
    let mut trajectories = Vec::new();

    let overall_progress = ProgressBar::new(runs as u64);
    overall_progress.set_style(
        ProgressStyle::default_bar()
            .template("[{bar:40.cyan/blue}] {pos}/{len} runs ({percent}%) - {msg}")
            .unwrap()
            .progress_chars("=>-"),
    );

    for run_num in 1..=runs {
        overall_progress.set_message(format!("Running trial {}/{}", run_num, runs));

        let trajectory = match runner.as_str() {
            "claude-code" => {
                let env = with_required_packages(
                    &field.environment,
                    &field.tools,
                    field.boundary.output_schema.is_some(),
                );
                let sandbox = create_sandbox(
                    &env,
                    &field.boundary,
                    Arc::new(ToolRegistry::new()),
                    backend.as_deref(),
                    backend_url.as_deref(),
                    backend_command.as_deref(),
                )
                .await
                .map_err(|e| anyhow::anyhow!("Failed to create sandbox: {}", e))?;
                run_field_with_claude_code(&field, &ctx, sandbox).await?
            }
            _ => {
                let provider: Box<dyn ModelProvider> = if field.model.name.contains('/') {
                    let mut p = OpenRouterProvider::from_env(&field.model.name)?;
                    if let Some(temp) = field.model.temperature {
                        p = p.with_temperature(temp);
                    }
                    Box::new(p)
                } else {
                    let mut p = AnthropicProvider::from_env(&field.model.name)?;
                    if let Some(temp) = field.model.temperature {
                        p = p.with_temperature(temp);
                    }
                    Box::new(p)
                };
                run_field(&field, provider.as_ref(), &ctx).await?
            }
        };

        store.save_redacted(&trajectory, &field.collect_secret_candidates())?;
        trajectories.push(trajectory);
        overall_progress.inc(1);
    }

    overall_progress.finish_with_message("All runs completed!");
    println!();

    println!("{}", "=".repeat(60));
    println!("Convergence Report");
    println!("{}", "=".repeat(60));
    println!();

    let report = AdaptationReport::from_trajectories(field.name.clone(), &trajectories);
    println!("{}", format_report(&report));

    println!("Individual Runs:");
    for (idx, traj) in trajectories.iter().enumerate() {
        let outcome = traj.outcome.as_ref().unwrap();
        let status = if outcome.is_success() { "✓" } else { "✗" };
        println!(
            "  {} Run {}: {} ({} steps, {} tokens, ${:.4})",
            status,
            idx + 1,
            outcome.description(),
            traj.step_count(),
            traj.total_tokens,
            traj.total_cost.microdollars() as f64 / 1_000_000.0
        );
    }

    Ok(())
}
