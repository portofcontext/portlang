use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use portlang_adapt::{format_report, AdaptationReport};
use portlang_config::{apply_runtime_context, parse_field_with_parent, resolve_parent_config};
use portlang_core::RuntimeContext;
use portlang_provider_anthropic::AnthropicProvider;
use portlang_provider_openrouter::OpenRouterProvider;
use portlang_runner_claudecode::run_field_with_claude_code;
use portlang_runtime::{run_field, ModelProvider};
use portlang_trajectory::{FilesystemStore, TrajectoryStore};
use std::path::PathBuf;

pub async fn run_command(
    field_path: PathBuf,
    parent_field: Option<PathBuf>,
    ctx: RuntimeContext,
    runner: String,
    dry_run: bool,
    runs: usize,
) -> Result<()> {
    if dry_run {
        return run_dry(field_path, parent_field, ctx);
    }
    if runs > 1 {
        return run_multi(field_path, runs, parent_field, ctx, runner).await;
    }
    run_single(field_path, parent_field, ctx, runner).await
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
) -> Result<()> {
    println!("Running field: {}", field_path.display());

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
    println!("Runner: {}", runner);
    println!();

    let progress = ProgressBar::new_spinner();
    progress.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    progress.set_message("Starting...");
    progress.enable_steady_tick(std::time::Duration::from_millis(100));

    let trajectory = match runner.as_str() {
        "claude-code" => {
            println!("Using Claude Code runner");
            run_field_with_claude_code(&field, &ctx).await?
        }
        _ => {
            let provider: Box<dyn ModelProvider> = if field.model.name.contains('/') {
                println!("Using OpenRouter provider");
                let mut p = OpenRouterProvider::from_env(&field.model.name)?;
                if let Some(temp) = field.model.temperature {
                    p = p.with_temperature(temp);
                }
                Box::new(p)
            } else {
                println!("Using Anthropic provider");
                let mut p = AnthropicProvider::from_env(&field.model.name)?;
                if let Some(temp) = field.model.temperature {
                    p = p.with_temperature(temp);
                }
                Box::new(p)
            };
            run_field(&field, provider.as_ref(), &ctx).await?
        }
    };

    progress.finish_and_clear();

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

    let store = FilesystemStore::new()?;
    store.save(&trajectory)?;

    println!("\nTrajectory saved:");
    println!("  Field: {}", trajectory.field_name);
    println!("  ID: {}", trajectory.id.filename());
    println!(
        "  Path: ~/.portlang/trajectories/{}/{}",
        trajectory.field_name,
        trajectory.id.filename()
    );

    if trajectory.outcome.as_ref().unwrap().is_success() {
        println!("\n✓ Field converged successfully!");
    } else {
        println!("\n✗ Field did not converge");
    }

    println!("\nTo view the trajectory:");
    println!("  portlang view trajectory {}", trajectory.id.filename());

    Ok(())
}

async fn run_multi(
    field_path: PathBuf,
    runs: usize,
    parent_field: Option<PathBuf>,
    ctx: RuntimeContext,
    runner: String,
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
            "claude-code" => run_field_with_claude_code(&field, &ctx).await?,
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

        store.save(&trajectory)?;
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
