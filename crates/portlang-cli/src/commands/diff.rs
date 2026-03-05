use anyhow::{Context, Result};
use portlang_trajectory::{diff_trajectories, format_diff, FilesystemStore};

/// Compare two trajectories
pub fn diff_command(trajectory_a: String, trajectory_b: String, format: String) -> Result<()> {
    let store = FilesystemStore::new()?;

    // Load trajectories by filename
    let traj_a = store
        .find_by_filename(&trajectory_a)
        .context(format!("Failed to load trajectory A: {}", trajectory_a))?;
    let traj_b = store
        .find_by_filename(&trajectory_b)
        .context(format!("Failed to load trajectory B: {}", trajectory_b))?;

    // Compute diff
    let diff = diff_trajectories(&traj_a, &traj_b);

    match format.as_str() {
        "json" => {
            // Output as JSON
            let json = serde_json::to_string_pretty(&diff)?;
            println!("{}", json);
        }
        "text" | _ => {
            // Human-readable output
            println!("{}", format_diff(&diff));

            // Summary statistics
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
    }

    Ok(())
}
