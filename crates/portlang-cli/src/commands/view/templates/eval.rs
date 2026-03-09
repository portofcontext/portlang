use super::super::common::*;
use portlang_core::Trajectory;
use std::path::PathBuf;

/// Generate an eval dashboard HTML page
pub fn generate_eval_html(eval_dir: &PathBuf, trajectories: &[Trajectory]) -> String {
    let head = render_head("Eval Dashboard");

    let header = render_eval_header(eval_dir, trajectories);
    let summary = render_eval_summary(trajectories);
    let trajectories_table = render_trajectories_table(trajectories);

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
{}
<body>
<div class="container">
    {}
    {}
    {}
</div>
</body>
</html>"#,
        head, header, summary, trajectories_table
    )
}

fn render_eval_header(eval_dir: &PathBuf, trajectories: &[Trajectory]) -> String {
    let date = if let Some(first) = trajectories.first() {
        first.started_at.format("%Y-%m-%d %H:%M").to_string()
    } else {
        "Unknown".to_string()
    };

    format!(
        r#"<h1>Eval Results: {}</h1>
<p style="color: var(--secondary); margin-bottom: 1rem;">
    Date: {} · Tasks: {}
</p>"#,
        escape_html(&eval_dir.display().to_string()),
        date,
        trajectories.len()
    )
}

fn render_eval_summary(trajectories: &[Trajectory]) -> String {
    if trajectories.is_empty() {
        return r#"<div class="section"><p>No trajectories found.</p></div>"#.to_string();
    }

    let passed_count = trajectories
        .iter()
        .filter(|t| t.outcome.as_ref().map(|o| o.is_success()).unwrap_or(false))
        .count();
    let total = trajectories.len();
    let pass_rate = (passed_count as f64 / total as f64) * 100.0;

    let total_cost: f64 = trajectories.iter().map(|t| t.total_cost.to_dollars()).sum();
    let avg_tokens = trajectories.iter().map(|t| t.total_tokens).sum::<u64>() as f64 / total as f64;

    format!(
        r#"<div class="section">
    <h2>Summary</h2>
    <div class="summary-grid">
        <div class="summary-card">
            <div class="summary-label">Passed</div>
            <div class="summary-value converged">{}/{} ({:.1}%)</div>
        </div>
        <div class="summary-card">
            <div class="summary-label">Total Cost</div>
            <div class="summary-value">${:.2}</div>
        </div>
        <div class="summary-card">
            <div class="summary-label">Avg Tokens</div>
            <div class="summary-value">{:.0}</div>
        </div>
    </div>
</div>"#,
        passed_count, total, pass_rate, total_cost, avg_tokens
    )
}

fn render_trajectories_table(trajectories: &[Trajectory]) -> String {
    let rows: String = trajectories
        .iter()
        .map(|t| {
            let outcome_class = if t.outcome.as_ref().map(|o| o.is_success()).unwrap_or(false) {
                "converged"
            } else {
                "failed"
            };

            let outcome_text = t
                .outcome
                .as_ref()
                .map(|o| {
                    if o.is_success() {
                        "Converged".to_string()
                    } else {
                        o.description()
                    }
                })
                .unwrap_or_else(|| "In Progress".to_string());

            let status_emoji = if t.outcome.as_ref().map(|o| o.is_success()).unwrap_or(false) {
                "✓"
            } else {
                "✗"
            };

            format!(
                r#"<tr>
    <td><span class="{}">{}</span></td>
    <td><span class="mono">{}</span></td>
    <td>{}</td>
    <td class="mono">{}</td>
    <td class="mono">${:.4}</td>
    <td class="mono">{}</td>
    <td><a href="{}-trajectory.html">View</a></td>
</tr>"#,
                outcome_class,
                status_emoji,
                escape_html(&t.field_name),
                escape_html(&outcome_text),
                t.step_count(),
                t.total_cost.to_dollars(),
                t.total_tokens,
                t.id.filename().trim_end_matches(".json")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<div class="section">
    <h2>Trajectories</h2>
    <div class="table-container">
        <table>
            <thead>
                <tr>
                    <th>Status</th>
                    <th>Field Name</th>
                    <th>Outcome</th>
                    <th>Steps</th>
                    <th>Cost</th>
                    <th>Tokens</th>
                    <th>Actions</th>
                </tr>
            </thead>
            <tbody>
                {}
            </tbody>
        </table>
    </div>
</div>"#,
        rows
    )
}
