pub mod common;
pub mod templates;

use anyhow::{Context, Result};
use portlang_adapt::{format_report, AdaptationReport};
use portlang_trajectory::{
    diff_trajectories, format_diff, format_step, format_summary, EvalRun, EvalRunStore,
    FilesystemStore, ReplaySession, TrajectoryQuery, TrajectoryStore,
};
use std::io::{self, Write};
use std::path::PathBuf;

/// View a trajectory. Format: "html" (default), "text" (interactive replay), or "json".
pub fn view_trajectory(trajectory_id: String, format: &str, auto_open: bool) -> Result<()> {
    let store = FilesystemStore::new()?;
    let trajectory = store
        .find_by_filename(&trajectory_id)
        .context(format!("Failed to load trajectory: {}", trajectory_id))?;

    match format {
        "json" => {
            let json = serde_json::to_string_pretty(&trajectory)?;
            println!("{}", json);
        }
        "text" => {
            interactive_replay(trajectory)?;
        }
        _ => {
            let html = templates::trajectory::generate_trajectory_html(&trajectory);
            let filename = format!("{}-trajectory.html", trajectory_id);
            let output_path = common::write_and_open(html, filename, auto_open)?;
            println!("HTML trajectory viewer generated:");
            println!("  {}", output_path.display());
        }
    }

    Ok(())
}

/// View eval results as HTML dashboard — accepts an eval run ID or a directory path.
pub fn view_eval(id_or_dir: String, auto_open: bool) -> Result<()> {
    let eval_run_store = EvalRunStore::new()?;
    let traj_store = FilesystemStore::new()?;

    let eval_run: EvalRun = if EvalRunStore::looks_like_id(&id_or_dir) {
        eval_run_store
            .load(&id_or_dir)
            .context(format!("Eval run not found: {}", id_or_dir))?
    } else {
        eval_run_store
            .find_latest_for_dir(&id_or_dir)?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "No eval runs found for directory: {}\nRun `portlang eval run {}` first.",
                    id_or_dir,
                    id_or_dir
                )
            })?
    };

    let mut all_trajectories = Vec::new();
    for traj_id in &eval_run.trajectory_ids {
        if let Ok(trajectory) = traj_store.load(traj_id) {
            all_trajectories.push(trajectory);
        }
    }

    if all_trajectories.is_empty() {
        anyhow::bail!("No trajectories found for eval run: {}", eval_run.id);
    }

    let eval_dir = PathBuf::from(&eval_run.eval_dir);

    let html = templates::eval::generate_eval_html(&eval_dir, &all_trajectories);
    let filename = common::get_output_filename("eval");
    let output_path = common::write_and_open(html, filename.clone(), auto_open)?;

    println!("Generating trajectory viewers...");
    let output_dir = common::get_output_dir()?;
    for trajectory in &all_trajectories {
        let traj_html = templates::trajectory::generate_trajectory_html_with_back_link(
            trajectory,
            Some(&filename),
        );
        let traj_filename = format!(
            "{}-trajectory.html",
            trajectory.id.filename().trim_end_matches(".json")
        );
        let traj_path = output_dir.join(&traj_filename);
        std::fs::write(&traj_path, traj_html)?;
    }

    println!("HTML eval dashboard generated (eval run {}):", eval_run.id);
    println!("  {}", output_path.display());
    println!("  Generated {} trajectory viewers", all_trajectories.len());

    Ok(())
}

/// Compare two trajectories. Format: "html" (default), "text", or "json".
pub fn view_diff(
    trajectory_a_id: String,
    trajectory_b_id: String,
    format: &str,
    auto_open: bool,
) -> Result<()> {
    let store = FilesystemStore::new()?;

    let traj_a = store
        .find_by_filename(&trajectory_a_id)
        .context(format!("Failed to load trajectory A: {}", trajectory_a_id))?;
    let traj_b = store
        .find_by_filename(&trajectory_b_id)
        .context(format!("Failed to load trajectory B: {}", trajectory_b_id))?;

    let diff = diff_trajectories(&traj_a, &traj_b);

    match format {
        "json" => {
            let json = serde_json::to_string_pretty(&diff)?;
            println!("{}", json);
        }
        "text" => {
            println!("{}", format_diff(&diff));
            println!("\n=== Summary ===");
            println!(
                "Trajectory A: {} ({} steps)",
                traj_a.field_name,
                traj_a.step_count()
            );
            println!(
                "Trajectory B: {} ({} steps)",
                traj_b.field_name,
                traj_b.step_count()
            );
            if let Some(point) = diff.divergence_point {
                println!("Diverged at step: {}", point);
            } else {
                println!("Trajectories are structurally identical");
            }
        }
        _ => {
            let html = templates::diff::generate_diff_html(&traj_a, &traj_b);
            let filename = format!("{}-vs-{}-diff.html", trajectory_a_id, trajectory_b_id);
            let output_path = common::write_and_open(html, filename, auto_open)?;
            println!("HTML comparison view generated:");
            println!("  {}", output_path.display());
        }
    }

    Ok(())
}

/// View field adaptation report. Format: "html" (default) or "text".
pub fn view_field_report(
    field_name: String,
    converged_only: bool,
    failed_only: bool,
    limit: Option<usize>,
    format: &str,
    auto_open: bool,
) -> Result<()> {
    let store = FilesystemStore::new()?;

    let mut query = TrajectoryQuery::new().field(&field_name);

    if converged_only && !failed_only {
        query = query.only_converged();
    } else if failed_only && !converged_only {
        query = query.only_failed();
    }

    if let Some(limit) = limit {
        query = query.limit(limit);
    }

    let summaries = store.query(&query)?;

    if summaries.is_empty() {
        println!("No trajectories found matching the criteria.");
        return Ok(());
    }

    let mut trajectories = Vec::new();
    for summary in &summaries {
        let traj = store.load(&summary.id)?;
        trajectories.push(traj);
    }

    if trajectories.is_empty() {
        anyhow::bail!("No matching trajectories found for field: {}", field_name);
    }

    let report = AdaptationReport::from_trajectories(field_name.clone(), &trajectories);

    match format {
        "text" => {
            println!("Generating adaptation report for field: {}", field_name);
            println!();
            println!("Found {} trajectories", trajectories.len());
            println!();
            println!("{}", format_report(&report));
        }
        _ => {
            let html = templates::field::generate_field_report_html(&report);
            let filename = format!("{}-field-report.html", field_name);
            let output_path = common::write_and_open(html, filename, auto_open)?;
            println!("HTML field report generated:");
            println!("  {}", output_path.display());
        }
    }

    Ok(())
}

fn interactive_replay(trajectory: portlang_core::Trajectory) -> Result<()> {
    let mut session = ReplaySession::new(trajectory);

    println!("{}", format_summary(session.trajectory()));
    println!();

    loop {
        if let Some(step) = session.current() {
            println!("{}", format_step(step));
        } else if session.is_at_end() {
            println!("=== End of Trajectory ===");
            if let Some(outcome) = &session.trajectory().outcome {
                println!("Final outcome: {}", outcome.description());
            }
        }

        println!();

        let mut options = Vec::new();
        if !session.is_at_start() {
            options.push("[p]rev");
        }
        if !session.is_at_end() {
            options.push("[n]ext");
        }
        options.push("[g]oto");
        options.push("[s]ummary");
        options.push("[q]uit");

        println!(
            "Step {}/{} - {}",
            session.current_step_number(),
            session.total_steps(),
            options.join("  ")
        );

        print!("> ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim().to_lowercase();

        match input.as_str() {
            "n" | "next" => {
                if session.next_step().is_none() && session.is_at_end() {
                    println!("Already at the end.");
                }
            }
            "p" | "prev" => {
                if session.prev().is_none() && session.is_at_start() {
                    println!("Already at the start.");
                }
            }
            "g" | "goto" => {
                print!("Go to step (0-{}): ", session.total_steps());
                io::stdout().flush()?;
                let mut step_input = String::new();
                io::stdin().read_line(&mut step_input)?;
                if let Ok(step_num) = step_input.trim().parse::<usize>() {
                    if session.goto(step_num).is_none() {
                        println!("Invalid step number.");
                    }
                } else {
                    println!("Invalid input.");
                }
            }
            "s" | "summary" => {
                println!("{}", format_summary(session.trajectory()));
            }
            "q" | "quit" => {
                break;
            }
            "" => {
                if session.next_step().is_none() && session.is_at_end() {
                    println!("Already at the end.");
                }
            }
            _ => {
                println!("Unknown command: {}", input);
            }
        }

        println!();
    }

    Ok(())
}
