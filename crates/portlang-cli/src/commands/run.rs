use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use portlang_config::{apply_runtime_context, parse_field_with_parent, resolve_parent_config};
use portlang_core::RuntimeContext;
use portlang_provider_anthropic::AnthropicProvider;
use portlang_provider_openrouter::OpenRouterProvider;
use portlang_runner_claudecode::run_field_with_claude_code;
use portlang_runtime::{run_field, ModelProvider};
use portlang_trajectory::{FilesystemStore, TrajectoryStore};
use std::path::PathBuf;

/// Run a field
pub async fn run_command(
    field_path: PathBuf,
    parent_field: Option<PathBuf>,
    ctx: RuntimeContext,
    runner: String,
) -> Result<()> {
    println!("Running field: {}", field_path.display());

    let parent = resolve_parent_config(&field_path, parent_field.as_ref())?;
    let field = parse_field_with_parent(&field_path, parent.as_ref())?;
    let field = apply_runtime_context(field, &ctx)?;
    println!("Field: {}", field.name);

    if let Some(description) = &field.description {
        println!("Description: {}", description);
    }

    println!("Model: {}", field.model.name);
    println!("Goal: {}", field.prompt.goal);
    println!("Runner: {}", runner);
    println!();

    // Create progress indicator
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
            // Native loop — create provider based on model name
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

    // Print results
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

    // Save trajectory
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

    // Show outcome status
    if trajectory.outcome.as_ref().unwrap().is_success() {
        println!("\n✓ Field converged successfully!");
    } else {
        println!("\n✗ Field did not converge");
    }

    // Show command to view the trajectory
    println!("\nTo view the trajectory:");
    println!("  portlang view trajectory {}", trajectory.id.filename());

    Ok(())
}
