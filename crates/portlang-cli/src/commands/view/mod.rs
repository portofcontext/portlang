pub mod common;
pub mod templates;

use anyhow::{Context, Result};
use portlang_adapt::AdaptationReport;
use portlang_trajectory::{EvalRun, EvalRunStore, FilesystemStore};
use std::path::PathBuf;

/// View a trajectory as HTML
pub fn view_trajectory(trajectory_id: String, auto_open: bool) -> Result<()> {
    let store = FilesystemStore::new()?;

    // Load trajectory by filename
    let trajectory = store
        .find_by_filename(&trajectory_id)
        .context(format!("Failed to load trajectory: {}", trajectory_id))?;

    // Generate HTML
    let html = templates::trajectory::generate_trajectory_html(&trajectory);

    // Write and open
    let filename = format!("{}-trajectory.html", trajectory_id);
    let output_path = common::write_and_open(html, filename, auto_open)?;

    println!("HTML trajectory viewer generated:");
    println!("  {}", output_path.display());

    Ok(())
}

/// View eval results as HTML dashboard — accepts an eval run ID or a directory path.
pub fn view_eval(id_or_dir: String, auto_open: bool) -> Result<()> {
    use portlang_trajectory::TrajectoryStore;

    let eval_run_store = EvalRunStore::new()?;
    let traj_store = FilesystemStore::new()?;

    // Resolve the eval run: by ID if it looks like one, otherwise find latest for dir.
    let eval_run: EvalRun = if EvalRunStore::looks_like_id(&id_or_dir) {
        eval_run_store
            .load(&id_or_dir)
            .context(format!("Eval run not found: {}", id_or_dir))?
    } else {
        eval_run_store
            .find_latest_for_dir(&id_or_dir)?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "No eval runs found for directory: {}\nRun `portlang eval {}` first.",
                    id_or_dir,
                    id_or_dir
                )
            })?
    };

    // Load each trajectory from the eval run manifest (preserving run order)
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

    // Generate HTML dashboard
    let html = templates::eval::generate_eval_html(&eval_dir, &all_trajectories);
    let filename = common::get_output_filename("eval");
    let output_path = common::write_and_open(html, filename.clone(), auto_open)?;

    // Generate individual trajectory HTML files with back links
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

/// View comparison of two trajectories as HTML
pub fn view_diff(trajectory_a_id: String, trajectory_b_id: String, auto_open: bool) -> Result<()> {
    let store = FilesystemStore::new()?;

    // Load both trajectories
    let trajectory_a = store
        .find_by_filename(&trajectory_a_id)
        .context(format!("Failed to load trajectory A: {}", trajectory_a_id))?;

    let trajectory_b = store
        .find_by_filename(&trajectory_b_id)
        .context(format!("Failed to load trajectory B: {}", trajectory_b_id))?;

    // Generate HTML
    let html = templates::diff::generate_diff_html(&trajectory_a, &trajectory_b);

    // Write and open
    let filename = format!("{}-vs-{}-diff.html", trajectory_a_id, trajectory_b_id);
    let output_path = common::write_and_open(html, filename, auto_open)?;

    println!("HTML comparison view generated:");
    println!("  {}", output_path.display());

    Ok(())
}

/// View field adaptation report as HTML
pub fn view_field_report(
    field_name: String,
    converged_only: bool,
    failed_only: bool,
    limit: Option<usize>,
    auto_open: bool,
) -> Result<()> {
    use portlang_trajectory::TrajectoryStore;

    let store = FilesystemStore::new()?;

    // Load trajectories for the field
    let summaries = store.list(&field_name)?;
    let mut trajectories = Vec::new();

    for summary in summaries {
        if let Ok(trajectory) = store.load(&summary.id) {
            trajectories.push(trajectory);
        }
    }

    if trajectories.is_empty() {
        anyhow::bail!("No trajectories found for field: {}", field_name);
    }

    // Apply filters
    if converged_only {
        trajectories.retain(|t| {
            t.outcome
                .as_ref()
                .map(|o: &portlang_core::RunOutcome| o.is_success())
                .unwrap_or(false)
        });
    } else if failed_only {
        trajectories.retain(|t| {
            !t.outcome
                .as_ref()
                .map(|o: &portlang_core::RunOutcome| o.is_success())
                .unwrap_or(true)
        });
    }

    // Apply limit
    if let Some(limit) = limit {
        trajectories.truncate(limit);
    }

    if trajectories.is_empty() {
        anyhow::bail!("No matching trajectories found for field: {}", field_name);
    }

    // Generate adaptation report
    let report = AdaptationReport::from_trajectories(field_name.clone(), &trajectories);

    // Generate HTML
    let html = templates::field::generate_field_report_html(&report);

    // Write and open
    let filename = format!("{}-field-report.html", field_name);
    let output_path = common::write_and_open(html, filename, auto_open)?;

    println!("HTML field report generated:");
    println!("  {}", output_path.display());

    Ok(())
}
