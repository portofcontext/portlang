use anyhow::Result;
use portlang_trajectory::EvalRunStore;

/// List all eval runs
pub fn evals_command(dir_filter: Option<String>, limit: Option<usize>) -> Result<()> {
    let store = EvalRunStore::new()?;
    let mut runs = store.list_all()?;

    if let Some(ref dir) = dir_filter {
        runs.retain(|r| r.eval_dir.contains(dir.as_str()));
    }

    if let Some(lim) = limit {
        runs.truncate(lim);
    }

    if runs.is_empty() {
        println!("No eval runs found.");
        return Ok(());
    }

    println!(
        "{:<26} {:<35} {:<6} {:<8} {:<10}",
        "ID", "Dir", "Tasks", "Passed", "Cost"
    );
    println!("{}", "=".repeat(90));

    for run in &runs {
        println!(
            "{:<26} {:<35} {:<6} {:<8} {:<10}",
            run.id,
            truncate(&run.eval_dir, 34),
            run.task_count,
            format!(
                "{}/{} ({:.0}%)",
                run.passed_count,
                run.task_count,
                run.pass_rate()
            ),
            format!("{}", run.total_cost),
        );
    }

    println!("\nTotal: {} eval run(s)", runs.len());
    Ok(())
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}
