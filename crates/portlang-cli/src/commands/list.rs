use anyhow::Result;
use portlang_trajectory::{FilesystemStore, TrajectoryQuery, TrajectoryStore};

/// List trajectories command
pub fn list_command(
    field_name: Option<String>,
    converged: bool,
    failed: bool,
    limit: Option<usize>,
) -> Result<()> {
    let store = FilesystemStore::new()?;

    // Build query
    let mut query = TrajectoryQuery::new();

    if let Some(field) = field_name {
        query = query.field(field);
    }

    if converged {
        query = query.only_converged();
    } else if failed {
        query = query.only_failed();
    }

    if let Some(lim) = limit {
        query = query.limit(lim);
    }

    // Execute query
    let summaries = store.query(&query)?;

    if summaries.is_empty() {
        println!("No trajectories found.");
        return Ok(());
    }

    // Print header
    println!(
        "{:<40} {:<20} {:<8} {:<12} {:<12} {:<15}",
        "ID", "Field", "Steps", "Tokens", "Cost", "Outcome"
    );
    println!("{}", "=".repeat(110));

    // Print each trajectory
    for summary in &summaries {
        let outcome_str = if let Some(ref outcome) = summary.outcome {
            if outcome.is_success() {
                "✓ Converged"
            } else {
                "✗ Failed"
            }
        } else {
            "Running"
        };

        println!(
            "{:<40} {:<20} {:<8} {:<12} {:<12} {:<15}",
            summary.id.filename(),
            summary.field_name,
            summary.step_count,
            summary.total_tokens,
            format!("{}", summary.total_cost),
            outcome_str
        );
    }

    println!("\nTotal: {} trajectories", summaries.len());

    Ok(())
}
