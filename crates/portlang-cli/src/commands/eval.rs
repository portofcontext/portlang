use anyhow::Result;
use chrono::Utc;
use portlang_config::{apply_runtime_context, parse_field_with_parent, parse_parent_config};
use portlang_core::RuntimeContext;
use portlang_provider_anthropic::AnthropicProvider;
use portlang_provider_openrouter::OpenRouterProvider;
use portlang_runner_claudecode::run_field_with_claude_code;
use portlang_runtime::{run_field, validate_field_config, ModelProvider};
use portlang_trajectory::{EvalRun, EvalRunStore, FilesystemStore, TrajectoryStore};
use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::PathBuf;
use walkdir::WalkDir;

struct TaskResult {
    name: String,
    passed: bool,
    outcome_description: String,
    steps: usize,
}

/// Run all .field files found recursively in `directory` and report aggregate accuracy.
/// If `resume_id` is provided, load that eval run and skip fields that already passed.
pub async fn eval_command(
    directory: PathBuf,
    parent_field: Option<PathBuf>,
    resume_id: Option<String>,
    ctx: RuntimeContext,
    runner: String,
) -> Result<()> {
    // Load parent config: explicit -p flag takes priority, then look for a .field or field.toml at the directory root
    let parent = if let Some(ref explicit) = parent_field {
        let p = parse_parent_config(explicit)?;
        if p.is_some() {
            println!("Using parent config from {}", explicit.display());
        }
        p
    } else {
        // Check for parent.field first (canonical), then field.field, then field.toml (backward compat)
        let parent_path = if directory.join("parent.field").exists() {
            directory.join("parent.field")
        } else if directory.join("field.field").exists() {
            directory.join("field.field")
        } else {
            directory.join("field.toml")
        };
        let p = parse_parent_config(&parent_path)?;
        if p.is_some() {
            println!("Using parent config from {}", parent_path.display());
        }
        p
    };

    // Collect .field paths from subdirectories (and field.toml for backward compat), skipping the root parent file
    let root_parent = {
        let a = directory.join("parent.field").canonicalize().ok();
        let b = directory.join("field.field").canonicalize().ok();
        let c = directory.join("field.toml").canonicalize().ok();
        a.or(b).or(c)
    };
    let mut field_paths: Vec<PathBuf> = WalkDir::new(&directory)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let path = e.path();
            path.extension() == Some(OsStr::new("field")) || e.file_name() == "field.toml"
        })
        .map(|e| e.into_path())
        .filter(|p| {
            // Skip the root-level parent file
            let canonical = p.canonicalize().ok();
            canonical != root_parent
        })
        .collect();

    field_paths.sort();

    if field_paths.is_empty() {
        println!("No .field files found in {}", directory.display());
        return Ok(());
    }

    let store = FilesystemStore::new()?;
    let eval_run_store = EvalRunStore::new()?;
    let eval_started_at = Utc::now();
    let mut eval_run = EvalRun::new(directory.to_string_lossy().to_string(), eval_started_at);

    // If resuming, load previous run and carry over passed trajectories
    let mut skip_fields: HashSet<String> = HashSet::new();
    if let Some(ref resume_id) = resume_id {
        let old_run = eval_run_store
            .load(resume_id)
            .map_err(|e| anyhow::anyhow!("Could not load eval run {}: {}", resume_id, e))?;

        println!("Resuming from eval {}...", resume_id);
        let mut carried = 0usize;
        for traj_id in &old_run.trajectory_ids {
            if let Ok(traj) = store.load(traj_id) {
                let passed = traj
                    .outcome
                    .as_ref()
                    .map(|o| o.is_success())
                    .unwrap_or(false);
                if passed {
                    println!("  ✓  [carried] {}", traj.field_name);
                    skip_fields.insert(traj.field_name.clone());
                    eval_run.trajectory_ids.push(traj.id.clone());
                    eval_run.task_count += 1;
                    eval_run.passed_count += 1;
                    eval_run.total_cost += traj.total_cost;
                    eval_run.total_tokens += traj.total_tokens;
                    carried += 1;
                }
            }
        }
        let remaining = field_paths.len() - carried;
        println!(
            "  {} passed (carried over), {} remaining to run",
            carried, remaining
        );
        println!();
    }

    let total = field_paths.len();
    let mut run_idx = skip_fields.len();
    let mut results: Vec<TaskResult> = Vec::new();

    println!(
        "Evaluating {} field(s) in {}...",
        total - skip_fields.len(),
        directory.display()
    );
    println!();

    // Save initial manifest so partial runs are recoverable
    eval_run_store.save(&eval_run)?;

    // Use a default (no-input) context for eval — --input is not supported per-field in batch runs
    let eval_ctx = RuntimeContext {
        vars: ctx.vars.clone(),
        input: None,
    };

    for path in field_paths.iter() {
        let field = match parse_field_with_parent(path, parent.as_ref()) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("\n✗ Fatal error: Failed to parse field {}", path.display());
                eprintln!("  Error: {}", e);
                eprintln!("\nEvaluation aborted. Fix the field configuration and try again.");
                return Err(e.into());
            }
        };

        let field = match apply_runtime_context(field, &eval_ctx) {
            Ok(f) => f,
            Err(e) => {
                eprintln!(
                    "\n✗ Fatal error: Template variable error in {}",
                    path.display()
                );
                eprintln!("  Error: {}", e);
                eprintln!("\nEvaluation aborted. Supply required variables with --var key=value.");
                return Err(e.into());
            }
        };

        if let Err(e) = validate_field_config(&field) {
            eprintln!(
                "\n✗ Fatal error: Invalid configuration in {}",
                path.display()
            );
            eprintln!("  Error: {}", e);
            eprintln!("\nEvaluation aborted. Fix the field configuration and try again.");
            return Err(e);
        }

        if skip_fields.contains(&field.name) {
            continue;
        }

        run_idx += 1;

        let run_result = match runner.as_str() {
            "claude-code" => run_field_with_claude_code(&field, &eval_ctx).await,
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
                run_field(&field, provider.as_ref(), &eval_ctx).await
            }
        };

        match run_result {
            Ok(trajectory) => {
                store.save(&trajectory)?;

                eval_run.trajectory_ids.push(trajectory.id.clone());
                eval_run.task_count += 1;
                eval_run.total_cost += trajectory.total_cost;
                eval_run.total_tokens += trajectory.total_tokens;

                let outcome = trajectory.outcome.as_ref().unwrap();
                let passed = outcome.is_success();
                if passed {
                    eval_run.passed_count += 1;
                }
                let status = if passed { "✓" } else { "✗" };

                if passed {
                    println!(
                        "  {}  [{}/{}] {}   {} steps  {} tokens  ${:.4}",
                        status,
                        run_idx,
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
                        run_idx,
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
                });
            }
            Err(e) => {
                eprintln!("  ✗  [{}/{}] {}   error: {}", run_idx, total, field.name, e);
                eval_run.task_count += 1;
                results.push(TaskResult {
                    name: field.name.clone(),
                    passed: false,
                    outcome_description: format!("error: {}", e),
                    steps: 0,
                });
            }
        }

        // Save manifest after each task so the run is resumable on failure
        eval_run.finished_at = Utc::now();
        eval_run_store.save(&eval_run)?;
    }

    // Aggregate summary
    let all_results_count = eval_run.task_count;
    let passed_count = eval_run.passed_count;
    let failed_count = all_results_count - passed_count;
    let pass_rate = if all_results_count > 0 {
        passed_count as f64 / all_results_count as f64 * 100.0
    } else {
        0.0
    };

    println!();
    println!("{}", "═".repeat(50));
    println!("Eval Results");
    println!("{}", "═".repeat(50));
    println!("Tasks:   {}", all_results_count);
    println!("Passed:  {}  ({:.1}%)", passed_count, pass_rate);
    println!("Failed:  {}  ({:.1}%)", failed_count, 100.0 - pass_rate);

    if all_results_count > 0 {
        let total_cost = eval_run.total_cost.microdollars();
        let total_tokens = eval_run.total_tokens;
        let total_steps: usize = results.iter().map(|r| r.steps).sum();
        let avg_cost = total_cost as f64 / all_results_count as f64 / 1_000_000.0;
        let avg_tokens = total_tokens as f64 / all_results_count as f64;
        let avg_steps = total_steps as f64 / results.len() as f64;

        println!();
        println!(
            "Cost:    ${:.4} total   ${:.4} avg",
            total_cost as f64 / 1_000_000.0,
            avg_cost
        );
        println!("Tokens:  {} total   {:.0} avg", total_tokens, avg_tokens);
        if !results.is_empty() {
            println!("Steps:   {:.1} avg", avg_steps);
        }
    }

    let failed: Vec<&TaskResult> = results.iter().filter(|r| !r.passed).collect();
    if !failed.is_empty() {
        println!();
        println!("Failed:");
        for r in &failed {
            println!("  ✗  {} — {}", r.name, r.outcome_description);
        }
    }

    let eval_id = eval_run.id.clone();
    println!();
    println!("Eval ID:  {}", eval_id);
    println!("To view the eval results:");
    println!("  portlang eval view {}", eval_id);
    if failed_count > 0 {
        println!("To resume failed tasks:");
        println!(
            "  portlang eval run {} --resume {}",
            directory.display(),
            eval_id
        );
    }

    Ok(())
}
