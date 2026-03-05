use anyhow::Result;
use portlang_adapt::{format_report, AdaptationReport};
use portlang_trajectory::{FilesystemStore, TrajectoryQuery, TrajectoryStore};

/// Generate an adaptation report from existing trajectories
pub fn report_command(
    field_name: String,
    converged: bool,
    failed: bool,
    limit: Option<usize>,
) -> Result<()> {
    println!("Generating adaptation report for field: {}", field_name);
    println!();

    let store = FilesystemStore::new()?;

    // Build query
    let mut query = TrajectoryQuery::new().field(&field_name);

    // Apply outcome filter
    if converged && !failed {
        query = query.only_converged();
    } else if failed && !converged {
        query = query.only_failed();
    }

    // Apply limit
    if let Some(limit) = limit {
        query = query.limit(limit);
    }

    // Load matching trajectories
    let summaries = store.query(&query)?;

    if summaries.is_empty() {
        println!("No trajectories found matching the criteria.");
        return Ok(());
    }

    println!("Found {} trajectories matching criteria", summaries.len());
    println!("Loading full trajectory data...");
    println!();

    // Load full trajectories for detailed analysis
    let mut trajectories = Vec::new();
    for summary in &summaries {
        let traj = store.load(&summary.id)?;
        trajectories.push(traj);
    }

    // Generate adaptation report
    let report = AdaptationReport::from_trajectories(field_name, &trajectories);

    // Print the report
    println!("{}", format_report(&report));

    Ok(())
}
