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

/// Run a field N times and measure convergence reliability.
///
/// Reports convergence rate, token/cost distributions, tool usage patterns,
/// and verifier signal quality across the repeated runs of the same field.
pub async fn converge_command(
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
    let field = apply_runtime_context(field, &ctx)?;
    println!("Field: {}", field.name);

    if let Some(description) = &field.description {
        println!("Description: {}", description);
    }

    println!("Model: {}", field.model.name);
    println!("Goal: {}", field.prompt.goal);
    println!();

    // Create trajectory store
    let store = FilesystemStore::new()?;

    // Collect trajectories from all runs
    let mut trajectories = Vec::new();

    // Progress bar for all runs
    let overall_progress = ProgressBar::new(runs as u64);
    overall_progress.set_style(
        ProgressStyle::default_bar()
            .template("[{bar:40.cyan/blue}] {pos}/{len} runs ({percent}%) - {msg}")
            .unwrap()
            .progress_chars("=>-"),
    );

    for run_num in 1..=runs {
        overall_progress.set_message(format!("Running trial {}/{}", run_num, runs));

        // Run the field with the selected runner
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

        // Save trajectory
        store.save(&trajectory)?;

        // Collect for analysis
        trajectories.push(trajectory);

        overall_progress.inc(1);
    }

    overall_progress.finish_with_message("All runs completed!");
    println!();

    // Generate adaptation report
    println!("{}", "=".repeat(60));
    println!("Convergence Report");
    println!("{}", "=".repeat(60));
    println!();

    let report = AdaptationReport::from_trajectories(field.name.clone(), &trajectories);

    // Print the report
    println!("{}", format_report(&report));

    // Print individual run outcomes
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
