use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use portlang_config::parse_field_from_file;
use portlang_provider_anthropic::AnthropicProvider;
use portlang_provider_openrouter::OpenRouterProvider;
use portlang_runtime::{run_field, ModelProvider};
use portlang_trajectory::{FilesystemStore, TrajectoryStore};
use std::path::PathBuf;

/// Run a field
pub async fn run_command(field_path: PathBuf) -> Result<()> {
    println!("Running field: {}", field_path.display());

    // Parse the field
    let field = parse_field_from_file(&field_path)?;
    println!("Field: {}", field.name);

    if let Some(description) = &field.description {
        println!("Description: {}", description);
    }

    println!("Model: {}", field.model.name);
    println!("Goal: {}", field.goal);
    println!();

    // Create provider based on model name
    let provider: Box<dyn ModelProvider> = if field.model.name.contains('/') {
        // OpenRouter format (provider/model)
        println!("Using OpenRouter provider");
        let mut p = OpenRouterProvider::from_env(&field.model.name)?;
        if let Some(temp) = field.model.temperature {
            p = p.with_temperature(temp);
        }
        if let Some(max_tokens) = field.model.max_tokens {
            p = p.with_max_tokens(max_tokens);
        }
        Box::new(p)
    } else {
        // Anthropic direct
        println!("Using Anthropic provider");
        let mut p = AnthropicProvider::from_env(&field.model.name)?;
        if let Some(temp) = field.model.temperature {
            p = p.with_temperature(temp);
        }
        if let Some(max_tokens) = field.model.max_tokens {
            p = p.with_max_tokens(max_tokens);
        }
        Box::new(p)
    };

    // Create progress indicator
    let progress = ProgressBar::new_spinner();
    progress.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    progress.set_message("Starting...");

    // Run the field
    progress.enable_steady_tick(std::time::Duration::from_millis(100));

    let trajectory = run_field(&field, provider.as_ref()).await?;

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

    Ok(())
}
