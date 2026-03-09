use super::super::common::*;
use portlang_core::{Action, Trajectory, TrajectoryStep};

/// Generate a comparison view HTML page
pub fn generate_diff_html(trajectory_a: &Trajectory, trajectory_b: &Trajectory) -> String {
    let head = render_head(&format!(
        "Comparing: {} vs {}",
        trajectory_a.field_name, trajectory_b.field_name
    ));

    let header = render_diff_header(trajectory_a, trajectory_b);
    let summary = render_diff_summary(trajectory_a, trajectory_b);
    let comparison = render_step_comparison(trajectory_a, trajectory_b);

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
        head, header, summary, comparison
    )
}

fn render_diff_header(trajectory_a: &Trajectory, trajectory_b: &Trajectory) -> String {
    format!(
        r#"<h1>Trajectory Comparison</h1>
<div class="section">
    <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 2rem;">
        <div>
            <h3>Trajectory A</h3>
            <div class="info-item">
                <span class="info-label">Field</span>
                <span class="info-value mono">{}</span>
            </div>
            <div class="info-item">
                <span class="info-label">ID</span>
                <span class="info-value mono">{}</span>
            </div>
            <div class="info-item">
                <span class="info-label">Outcome</span>
                <span class="info-value {}">{}</span>
            </div>
            <div class="info-item">
                <span class="info-label">Steps</span>
                <span class="info-value">{}</span>
            </div>
            <div class="info-item">
                <span class="info-label">Cost</span>
                <span class="info-value">${:.4}</span>
            </div>
            <div class="info-item">
                <span class="info-label">Tokens</span>
                <span class="info-value">{}</span>
            </div>
        </div>
        <div>
            <h3>Trajectory B</h3>
            <div class="info-item">
                <span class="info-label">Field</span>
                <span class="info-value mono">{}</span>
            </div>
            <div class="info-item">
                <span class="info-label">ID</span>
                <span class="info-value mono">{}</span>
            </div>
            <div class="info-item">
                <span class="info-label">Outcome</span>
                <span class="info-value {}">{}</span>
            </div>
            <div class="info-item">
                <span class="info-label">Steps</span>
                <span class="info-value">{}</span>
            </div>
            <div class="info-item">
                <span class="info-label">Cost</span>
                <span class="info-value">${:.4}</span>
            </div>
            <div class="info-item">
                <span class="info-label">Tokens</span>
                <span class="info-value">{}</span>
            </div>
        </div>
    </div>
</div>"#,
        escape_html(&trajectory_a.field_name),
        escape_html(&trajectory_a.id.filename()),
        if trajectory_a
            .outcome
            .as_ref()
            .map(|o| o.is_success())
            .unwrap_or(false)
        {
            "converged"
        } else {
            "failed"
        },
        trajectory_a
            .outcome
            .as_ref()
            .map(|o| o.description())
            .unwrap_or_else(|| "In Progress".to_string()),
        trajectory_a.step_count(),
        trajectory_a.total_cost.to_dollars(),
        trajectory_a.total_tokens,
        escape_html(&trajectory_b.field_name),
        escape_html(&trajectory_b.id.filename()),
        if trajectory_b
            .outcome
            .as_ref()
            .map(|o| o.is_success())
            .unwrap_or(false)
        {
            "converged"
        } else {
            "failed"
        },
        trajectory_b
            .outcome
            .as_ref()
            .map(|o| o.description())
            .unwrap_or_else(|| "In Progress".to_string()),
        trajectory_b.step_count(),
        trajectory_b.total_cost.to_dollars(),
        trajectory_b.total_tokens
    )
}

fn render_diff_summary(trajectory_a: &Trajectory, trajectory_b: &Trajectory) -> String {
    let cost_diff = trajectory_b.total_cost.to_dollars() - trajectory_a.total_cost.to_dollars();
    let tokens_diff = trajectory_b.total_tokens as i64 - trajectory_a.total_tokens as i64;
    let steps_diff = trajectory_b.step_count() as i64 - trajectory_a.step_count() as i64;

    let divergence_point = find_divergence_point(trajectory_a, trajectory_b);
    let divergence_text = if let Some(step) = divergence_point {
        format!("Step {}", step)
    } else {
        "No divergence".to_string()
    };

    format!(
        r#"<div class="section">
    <h2>Comparison Summary</h2>
    <div class="summary-grid">
        <div class="summary-card">
            <div class="summary-label">Cost Difference</div>
            <div class="summary-value">{:+.4}</div>
        </div>
        <div class="summary-card">
            <div class="summary-label">Token Difference</div>
            <div class="summary-value">{:+}</div>
        </div>
        <div class="summary-card">
            <div class="summary-label">Step Difference</div>
            <div class="summary-value">{:+}</div>
        </div>
        <div class="summary-card">
            <div class="summary-label">First Divergence</div>
            <div class="summary-value">{}</div>
        </div>
    </div>
</div>"#,
        cost_diff, tokens_diff, steps_diff, divergence_text
    )
}

fn find_divergence_point(trajectory_a: &Trajectory, trajectory_b: &Trajectory) -> Option<usize> {
    let max_steps = trajectory_a.step_count().min(trajectory_b.step_count());

    for i in 0..max_steps {
        let step_a = &trajectory_a.steps[i];
        let step_b = &trajectory_b.steps[i];

        if !steps_match(step_a, step_b) {
            return Some(i + 1); // 1-indexed
        }
    }

    // If one trajectory is longer than the other, divergence is at the end
    if trajectory_a.step_count() != trajectory_b.step_count() {
        Some(max_steps + 1)
    } else {
        None
    }
}

fn steps_match(step_a: &TrajectoryStep, step_b: &TrajectoryStep) -> bool {
    match (&step_a.action, &step_b.action) {
        (
            Action::ToolCall {
                tool: tool_a,
                input: input_a,
            },
            Action::ToolCall {
                tool: tool_b,
                input: input_b,
            },
        ) => tool_a == tool_b && input_a == input_b,
        (Action::TextOutput { text: text_a }, Action::TextOutput { text: text_b }) => {
            text_a == text_b
        }
        (Action::Stop, Action::Stop) => true,
        _ => false,
    }
}

fn render_step_comparison(trajectory_a: &Trajectory, trajectory_b: &Trajectory) -> String {
    let max_steps = trajectory_a.step_count().max(trajectory_b.step_count());
    let mut rows = String::new();

    for i in 0..max_steps {
        let step_a_opt = trajectory_a.steps.get(i);
        let step_b_opt = trajectory_b.steps.get(i);

        let (step_num, aligned) = match (step_a_opt, step_b_opt) {
            (Some(a), Some(b)) => {
                let is_aligned = steps_match(a, b);
                (i + 1, is_aligned)
            }
            _ => (i + 1, false),
        };

        let alignment_badge = if aligned {
            r#"<span class="status-badge converged">Aligned</span>"#
        } else {
            r#"<span class="status-badge failed">Divergent</span>"#
        };

        let step_a_content = if let Some(step) = step_a_opt {
            render_step_summary(step)
        } else {
            r#"<em style="color: var(--secondary);">No step</em>"#.to_string()
        };

        let step_b_content = if let Some(step) = step_b_opt {
            render_step_summary(step)
        } else {
            r#"<em style="color: var(--secondary);">No step</em>"#.to_string()
        };

        rows.push_str(&format!(
            r#"<tr>
    <td class="mono">{}</td>
    <td>{}</td>
    <td>{}</td>
    <td>{}</td>
</tr>"#,
            step_num, step_a_content, step_b_content, alignment_badge
        ));
    }

    format!(
        r#"<div class="section">
    <h2>Step-by-Step Comparison</h2>
    <div class="table-container">
        <table>
            <thead>
                <tr>
                    <th>Step</th>
                    <th>Trajectory A</th>
                    <th>Trajectory B</th>
                    <th>Alignment</th>
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

fn render_step_summary(step: &TrajectoryStep) -> String {
    let action_name = match &step.action {
        Action::ToolCall { tool, .. } => format!("Tool: {}", tool),
        Action::TextOutput { text } => {
            let preview = if text.len() > 50 {
                format!("{}...", &text[..50])
            } else {
                text.clone()
            };
            format!("Text: {}", escape_html(&preview))
        }
        Action::Stop => "Stop".to_string(),
    };

    format!(
        r#"<div>
    <div class="mono" style="font-weight: 600; margin-bottom: 0.25rem;">{}</div>
    <div style="font-size: 0.75rem; color: var(--secondary);">
        {} tokens · ${:.4}
    </div>
</div>"#,
        escape_html(&action_name),
        step.tokens_used,
        step.cost.to_dollars()
    )
}
