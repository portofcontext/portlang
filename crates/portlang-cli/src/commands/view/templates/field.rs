use super::super::common::*;
use portlang_adapt::AdaptationReport;

/// Generate a field report HTML page
pub fn generate_field_report_html(report: &AdaptationReport) -> String {
    let head = render_head(&format!("Field Report: {}", report.field_name));

    let header = render_field_header(report);
    let distributions = render_distributions(report);
    let tool_usage = render_tool_usage(report);
    let verifier_signals = render_verifier_signals(report);

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
{}
<body>
<div class="container">
    {}
    {}
    {}
    {}
</div>
</body>
</html>"#,
        head, header, distributions, tool_usage, verifier_signals
    )
}

fn render_field_header(report: &AdaptationReport) -> String {
    let convergence_class = if report.convergence_rate >= 0.8 {
        "converged"
    } else {
        "failed"
    };

    let converged_count = (report.convergence_rate * report.run_count as f64).round() as usize;
    let failed_count = report.run_count - converged_count;

    format!(
        r#"<h1>Field Report: {}</h1>
<div class="section">
    <div class="header-info">
        <div class="info-item">
            <span class="info-label">Runs Analyzed</span>
            <span class="info-value">{}</span>
        </div>
        <div class="info-item">
            <span class="info-label">Convergence Rate</span>
            <span class="info-value {}"><span class="status-badge {}">{:.1}%</span></span>
        </div>
        <div class="info-item">
            <span class="info-label">Converged</span>
            <span class="info-value converged">{}</span>
        </div>
        <div class="info-item">
            <span class="info-label">Failed</span>
            <span class="info-value failed">{}</span>
        </div>
    </div>
</div>"#,
        escape_html(&report.field_name),
        report.run_count,
        convergence_class,
        convergence_class,
        report.convergence_rate * 100.0,
        converged_count,
        failed_count
    )
}

fn render_distributions(report: &AdaptationReport) -> String {
    format!(
        r#"<div class="section">
    <h2>Resource Usage Distributions</h2>

    <h3>Token Distribution</h3>
    <div class="summary-grid">
        <div class="summary-card">
            <div class="summary-label">Min</div>
            <div class="summary-value">{:.0}</div>
        </div>
        <div class="summary-card">
            <div class="summary-label">Median</div>
            <div class="summary-value">{:.0}</div>
        </div>
        <div class="summary-card">
            <div class="summary-label">Mean</div>
            <div class="summary-value">{:.0}</div>
        </div>
        <div class="summary-card">
            <div class="summary-label">P90</div>
            <div class="summary-value">{:.0}</div>
        </div>
        <div class="summary-card">
            <div class="summary-label">P99</div>
            <div class="summary-value">{:.0}</div>
        </div>
        <div class="summary-card">
            <div class="summary-label">Max</div>
            <div class="summary-value">{:.0}</div>
        </div>
    </div>

    <h3>Cost Distribution</h3>
    <div class="summary-grid">
        <div class="summary-card">
            <div class="summary-label">Min</div>
            <div class="summary-value">${:.4}</div>
        </div>
        <div class="summary-card">
            <div class="summary-label">Median</div>
            <div class="summary-value">${:.4}</div>
        </div>
        <div class="summary-card">
            <div class="summary-label">Mean</div>
            <div class="summary-value">${:.4}</div>
        </div>
        <div class="summary-card">
            <div class="summary-label">P90</div>
            <div class="summary-value">${:.4}</div>
        </div>
        <div class="summary-card">
            <div class="summary-label">P99</div>
            <div class="summary-value">${:.4}</div>
        </div>
        <div class="summary-card">
            <div class="summary-label">Max</div>
            <div class="summary-value">${:.4}</div>
        </div>
    </div>

    <h3>Step Distribution</h3>
    <div class="summary-grid">
        <div class="summary-card">
            <div class="summary-label">Min</div>
            <div class="summary-value">{:.0}</div>
        </div>
        <div class="summary-card">
            <div class="summary-label">Median</div>
            <div class="summary-value">{:.0}</div>
        </div>
        <div class="summary-card">
            <div class="summary-label">Mean</div>
            <div class="summary-value">{:.1}</div>
        </div>
        <div class="summary-card">
            <div class="summary-label">Max</div>
            <div class="summary-value">{:.0}</div>
        </div>
    </div>
</div>"#,
        report.token_distribution.min,
        report.token_distribution.median,
        report.token_distribution.mean,
        report.token_distribution.p90,
        report.token_distribution.p99,
        report.token_distribution.max,
        report.cost_distribution.min / 1_000_000.0,
        report.cost_distribution.median / 1_000_000.0,
        report.cost_distribution.mean / 1_000_000.0,
        report.cost_distribution.p90 / 1_000_000.0,
        report.cost_distribution.p99 / 1_000_000.0,
        report.cost_distribution.max / 1_000_000.0,
        report.step_distribution.min,
        report.step_distribution.median,
        report.step_distribution.mean,
        report.step_distribution.max
    )
}

fn render_tool_usage(report: &AdaptationReport) -> String {
    if report.tool_usage.is_empty() {
        return String::new();
    }

    let rows: String = report
        .tool_usage
        .iter()
        .map(|tool| {
            let usage_rate = (tool.runs_used_in as f64 / report.run_count as f64) * 100.0;
            let convergence_delta =
                (tool.convergence_when_used - tool.convergence_when_not_used) * 100.0;
            let delta_class = if convergence_delta > 0.0 {
                "converged"
            } else {
                "failed"
            };

            format!(
                r#"<tr>
    <td class="mono">{}</td>
    <td class="mono">{}</td>
    <td class="mono">{}/{} ({:.1}%)</td>
    <td class="mono">{:.1}%</td>
    <td class="mono">{:.1}%</td>
    <td class="mono {}">
        {} {:.1}pp
    </td>
</tr>"#,
                escape_html(&tool.tool),
                tool.invocation_count,
                tool.runs_used_in,
                report.run_count,
                usage_rate,
                tool.convergence_when_used * 100.0,
                tool.convergence_when_not_used * 100.0,
                delta_class,
                if convergence_delta > 0.0 { "+" } else { "" },
                convergence_delta
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<div class="section">
    <h2>Tool Usage Analysis</h2>
    <p style="color: var(--secondary); margin-bottom: 1rem;">
        Shows how tool usage correlates with convergence success.
        A positive delta suggests the tool helps convergence.
    </p>
    <div class="table-container">
        <table>
            <thead>
                <tr>
                    <th>Tool</th>
                    <th>Total Invocations</th>
                    <th>Used In Runs</th>
                    <th>Conv. When Used</th>
                    <th>Conv. When NOT Used</th>
                    <th>Delta</th>
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

fn render_verifier_signals(report: &AdaptationReport) -> String {
    if report.verifier_signals.is_empty() {
        return String::new();
    }

    let rows: String = report
        .verifier_signals
        .iter()
        .map(|verifier| {
            // Signal quality assessment
            let signal_quality = assess_signal_quality(
                verifier.pass_rate,
                verifier.pass_rate_in_converged,
                verifier.pass_rate_in_failed,
            );

            format!(
                r#"<tr>
    <td class="mono">{}</td>
    <td class="mono">{}</td>
    <td class="mono">{:.1}%</td>
    <td class="mono">{:.1}%</td>
    <td class="mono">{:.1}%</td>
    <td>{}</td>
</tr>"#,
                escape_html(&verifier.verifier),
                verifier.invocation_count,
                verifier.pass_rate * 100.0,
                verifier.pass_rate_in_converged * 100.0,
                verifier.pass_rate_in_failed * 100.0,
                signal_quality
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<div class="section">
    <h2>Verifier Signal Quality</h2>
    <p style="color: var(--secondary); margin-bottom: 1rem;">
        Analyzes how well verifiers differentiate between converged and failed runs.
        Clear signals show different pass rates in converged vs. failed runs.
    </p>
    <div class="table-container">
        <table>
            <thead>
                <tr>
                    <th>Verifier</th>
                    <th>Invocations</th>
                    <th>Overall Pass Rate</th>
                    <th>Pass Rate (Converged)</th>
                    <th>Pass Rate (Failed)</th>
                    <th>Signal Quality</th>
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

fn assess_signal_quality(
    overall_pass_rate: f64,
    pass_rate_converged: f64,
    pass_rate_failed: f64,
) -> String {
    let diff = (pass_rate_converged - pass_rate_failed).abs();

    if diff >= 0.5 {
        "Clear (high differentiation)".to_string()
    } else if diff >= 0.2 {
        "Moderate (some differentiation)".to_string()
    } else if overall_pass_rate >= 0.95 || overall_pass_rate <= 0.05 {
        "Weak (always passes/fails)".to_string()
    } else {
        "Low (little differentiation)".to_string()
    }
}
