use anyhow::Result;
use portlang_config::{parse_field_with_parent, parse_parent_config};
use portlang_provider_anthropic::AnthropicProvider;
use portlang_provider_openrouter::OpenRouterProvider;
use portlang_runtime::{run_field, ModelProvider};
use portlang_trajectory::{FilesystemStore, TrajectoryStore};
use std::path::PathBuf;
use walkdir::WalkDir;

struct TaskResult {
    name: String,
    passed: bool,
    outcome_description: String,
    steps: usize,
    tokens: u64,
    cost_microdollars: u64,
}

/// Run all field.toml files found recursively in `directory` and report aggregate accuracy.
pub async fn eval_command(directory: PathBuf, parent_field: Option<PathBuf>) -> Result<()> {
    // Load parent config: explicit -p flag takes priority, then directory/field.toml
    let parent = if let Some(ref explicit) = parent_field {
        let p = parse_parent_config(explicit)?;
        if p.is_some() {
            println!("Using parent config from {}", explicit.display());
        }
        p
    } else {
        let parent_path = directory.join("field.toml");
        let p = parse_parent_config(&parent_path)?;
        if p.is_some() {
            println!("Using parent config from {}", parent_path.display());
        }
        p
    };

    // Collect field.toml paths from subdirectories only (skip the root field.toml)
    let root_field_toml = directory.join("field.toml").canonicalize().ok();
    let mut field_paths: Vec<PathBuf> = WalkDir::new(&directory)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name() == "field.toml")
        .map(|e| e.into_path())
        .filter(|p| {
            // Skip the root-level field.toml (parent template)
            let canonical = p.canonicalize().ok();
            canonical != root_field_toml
        })
        .collect();

    field_paths.sort();

    if field_paths.is_empty() {
        println!("No field.toml files found in {}", directory.display());
        return Ok(());
    }

    println!(
        "Evaluating {} field(s) in {}...",
        field_paths.len(),
        directory.display()
    );
    println!();

    let store = FilesystemStore::new()?;
    let total = field_paths.len();
    let mut results: Vec<TaskResult> = Vec::with_capacity(total);

    for (idx, path) in field_paths.iter().enumerate() {
        let field = match parse_field_with_parent(path, parent.as_ref()) {
            Ok(f) => f,
            Err(e) => {
                eprintln!(
                    "\n✗ Fatal error: Failed to parse field [{}/{}] {}",
                    idx + 1,
                    total,
                    path.display()
                );
                eprintln!("  Error: {}", e);
                eprintln!("\nEvaluation aborted. Fix the field configuration and try again.");
                return Err(e.into());
            }
        };

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

        let trajectory = run_field(&field, provider.as_ref()).await?;
        store.save(&trajectory)?;

        let outcome = trajectory.outcome.as_ref().unwrap();
        let passed = outcome.is_success();
        let status = if passed { "✓" } else { "✗" };

        if passed {
            println!(
                "  {}  [{}/{}] {}   {} steps  {}  tokens  ${:.4}",
                status,
                idx + 1,
                total,
                field.name,
                trajectory.step_count(),
                trajectory.total_tokens,
                trajectory.total_cost.microdollars() as f64 / 1_000_000.0
            );
        } else {
            println!(
                "  {}  [{}/{}] {}   {}   {} tokens  ${:.4}",
                status,
                idx + 1,
                total,
                field.name,
                outcome.description(),
                trajectory.total_tokens,
                trajectory.total_cost.microdollars() as f64 / 1_000_000.0
            );
        }

        results.push(TaskResult {
            name: field.name.clone(),
            passed,
            outcome_description: outcome.description(),
            steps: trajectory.step_count(),
            tokens: trajectory.total_tokens,
            cost_microdollars: trajectory.total_cost.microdollars(),
        });
    }

    // Aggregate summary
    println!();
    println!("{}", "═".repeat(50));
    println!("Eval Results");
    println!("{}", "═".repeat(50));

    let passed_count = results.iter().filter(|r| r.passed).count();
    let failed_count = results.len() - passed_count;
    let pass_rate = passed_count as f64 / results.len() as f64 * 100.0;

    println!("Tasks:   {}", results.len());
    println!("Passed:  {}  ({:.1}%)", passed_count, pass_rate);
    println!("Failed:  {}  ({:.1}%)", failed_count, 100.0 - pass_rate);

    if !results.is_empty() {
        let total_cost: u64 = results.iter().map(|r| r.cost_microdollars).sum();
        let total_tokens: u64 = results.iter().map(|r| r.tokens).sum();
        let total_steps: usize = results.iter().map(|r| r.steps).sum();
        let avg_cost = total_cost as f64 / results.len() as f64 / 1_000_000.0;
        let avg_tokens = total_tokens as f64 / results.len() as f64;
        let avg_steps = total_steps as f64 / results.len() as f64;

        println!();
        println!(
            "Cost:    ${:.4} total   ${:.4} avg",
            total_cost as f64 / 1_000_000.0,
            avg_cost
        );
        println!("Tokens:  {} total   {:.0} avg", total_tokens, avg_tokens);
        println!("Steps:   {:.1} avg", avg_steps);
    }

    let failed: Vec<&TaskResult> = results.iter().filter(|r| !r.passed).collect();
    if !failed.is_empty() {
        println!();
        println!("Failed:");
        for r in &failed {
            println!("  ✗  {} — {}", r.name, r.outcome_description);
        }
    }

    println!();
    println!("To view the eval results:");
    println!("  portlang view eval {}", directory.display());

    Ok(())
}
