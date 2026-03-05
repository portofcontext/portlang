use crate::statistics::{convergence_rate, Distribution};
use portlang_core::{Action, ToolName, Trajectory};
use portlang_trajectory::TrajectorySummary;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Adaptation report summarizing patterns across multiple trajectory runs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdaptationReport {
    /// Field name
    pub field_name: String,

    /// Total number of runs analyzed
    pub run_count: usize,

    /// Convergence rate (fraction of runs that converged)
    pub convergence_rate: f64,

    /// Tool usage patterns
    pub tool_usage: Vec<ToolUsageReport>,

    /// Token usage distribution
    pub token_distribution: Distribution,

    /// Cost distribution
    pub cost_distribution: Distribution,

    /// Step count distribution
    pub step_distribution: Distribution,

    /// Verifier signal quality
    pub verifier_signals: Vec<VerifierSignalReport>,
}

/// Tool usage analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUsageReport {
    /// Tool name
    pub tool: String,

    /// Number of times invoked across all runs
    pub invocation_count: usize,

    /// Number of runs that used this tool
    pub runs_used_in: usize,

    /// Convergence rate when this tool was used
    pub convergence_when_used: f64,

    /// Convergence rate when this tool was NOT used
    pub convergence_when_not_used: f64,
}

/// Verifier signal quality analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifierSignalReport {
    /// Verifier name
    pub verifier: String,

    /// Number of times this verifier was invoked
    pub invocation_count: usize,

    /// Pass rate across all invocations
    pub pass_rate: f64,

    /// How often this verifier passed in converged runs
    pub pass_rate_in_converged: f64,

    /// How often this verifier passed in failed runs
    pub pass_rate_in_failed: f64,
}

impl AdaptationReport {
    /// Generate a report from trajectory summaries
    pub fn from_summaries(field_name: String, summaries: &[TrajectorySummary]) -> Self {
        if summaries.is_empty() {
            return Self::empty(field_name);
        }

        let run_count = summaries.len();

        // Calculate convergence rate
        let converged_count = summaries
            .iter()
            .filter(|s| s.outcome.as_ref().map(|o| o.is_success()).unwrap_or(false))
            .count();
        let conv_rate = convergence_rate(converged_count, run_count);

        // Calculate distributions
        let token_values: Vec<f64> = summaries.iter().map(|s| s.total_tokens as f64).collect();
        let token_distribution = Distribution::from_values(token_values);

        let cost_values: Vec<f64> = summaries
            .iter()
            .map(|s| s.total_cost.microdollars() as f64)
            .collect();
        let cost_distribution = Distribution::from_values(cost_values);

        let step_values: Vec<f64> = summaries.iter().map(|s| s.step_count as f64).collect();
        let step_distribution = Distribution::from_values(step_values);

        // Note: Tool usage and verifier signals require full trajectory data
        // For now, return empty vectors (would need to load full trajectories)
        let tool_usage = Vec::new();
        let verifier_signals = Vec::new();

        Self {
            field_name,
            run_count,
            convergence_rate: conv_rate,
            tool_usage,
            token_distribution,
            cost_distribution,
            step_distribution,
            verifier_signals,
        }
    }

    /// Generate a detailed report from full trajectories
    pub fn from_trajectories(field_name: String, trajectories: &[Trajectory]) -> Self {
        if trajectories.is_empty() {
            return Self::empty(field_name);
        }

        let run_count = trajectories.len();

        // Calculate convergence rate
        let converged_count = trajectories
            .iter()
            .filter(|t| t.outcome.as_ref().map(|o| o.is_success()).unwrap_or(false))
            .count();
        let conv_rate = convergence_rate(converged_count, run_count);

        // Calculate distributions
        let token_values: Vec<f64> = trajectories.iter().map(|t| t.total_tokens as f64).collect();
        let token_distribution = Distribution::from_values(token_values);

        let cost_values: Vec<f64> = trajectories
            .iter()
            .map(|t| t.total_cost.microdollars() as f64)
            .collect();
        let cost_distribution = Distribution::from_values(cost_values);

        let step_values: Vec<f64> = trajectories.iter().map(|t| t.step_count() as f64).collect();
        let step_distribution = Distribution::from_values(step_values);

        // Analyze tool usage
        let tool_usage = analyze_tool_usage(trajectories);

        // Analyze verifier signals
        let verifier_signals = analyze_verifier_signals(trajectories);

        Self {
            field_name,
            run_count,
            convergence_rate: conv_rate,
            tool_usage,
            token_distribution,
            cost_distribution,
            step_distribution,
            verifier_signals,
        }
    }

    /// Create an empty report
    fn empty(field_name: String) -> Self {
        Self {
            field_name,
            run_count: 0,
            convergence_rate: 0.0,
            tool_usage: Vec::new(),
            token_distribution: Distribution::from_values(vec![]),
            cost_distribution: Distribution::from_values(vec![]),
            step_distribution: Distribution::from_values(vec![]),
            verifier_signals: Vec::new(),
        }
    }
}

/// Analyze tool usage patterns across trajectories
fn analyze_tool_usage(trajectories: &[Trajectory]) -> Vec<ToolUsageReport> {
    let mut tool_stats: HashMap<String, (usize, Vec<bool>)> = HashMap::new();

    // Collect tool usage data
    for traj in trajectories {
        let converged = traj
            .outcome
            .as_ref()
            .map(|o| o.is_success())
            .unwrap_or(false);

        let mut tools_used_in_run = std::collections::HashSet::new();

        for step in &traj.steps {
            if let Action::ToolCall { tool, .. } = &step.action {
                let tool_name = tool_name_str(tool);
                tools_used_in_run.insert(tool_name.to_string());

                let entry = tool_stats
                    .entry(tool_name.to_string())
                    .or_insert((0, Vec::new()));
                entry.0 += 1; // invocation count
            }
        }

        // Record which tools were used in this run
        for tool_name in &tools_used_in_run {
            if let Some(entry) = tool_stats.get_mut(tool_name) {
                entry.1.push(converged);
            }
        }
    }

    // Calculate statistics
    let mut reports = Vec::new();
    for (tool, (invocation_count, run_outcomes)) in tool_stats {
        let runs_used_in = run_outcomes.len();
        let converged_when_used = run_outcomes.iter().filter(|&&c| c).count();
        let convergence_when_used = if runs_used_in > 0 {
            converged_when_used as f64 / runs_used_in as f64
        } else {
            0.0
        };

        // Calculate convergence when NOT used
        let runs_not_used = trajectories.len() - runs_used_in;
        let converged_when_not_used = trajectories
            .iter()
            .filter(|t| {
                !t.steps.iter().any(|s| {
                    matches!(&s.action, Action::ToolCall { tool: t_tool, .. } if tool_name_str(t_tool) == tool)
                })
            })
            .filter(|t| t.outcome.as_ref().map(|o| o.is_success()).unwrap_or(false))
            .count();

        let convergence_when_not_used = if runs_not_used > 0 {
            converged_when_not_used as f64 / runs_not_used as f64
        } else {
            0.0
        };

        reports.push(ToolUsageReport {
            tool,
            invocation_count,
            runs_used_in,
            convergence_when_used,
            convergence_when_not_used,
        });
    }

    reports.sort_by(|a, b| b.invocation_count.cmp(&a.invocation_count));
    reports
}

/// Analyze verifier signal quality
fn analyze_verifier_signals(trajectories: &[Trajectory]) -> Vec<VerifierSignalReport> {
    let mut verifier_stats: HashMap<String, VerifierStats> = HashMap::new();

    for traj in trajectories {
        let converged = traj
            .outcome
            .as_ref()
            .map(|o| o.is_success())
            .unwrap_or(false);

        for step in &traj.steps {
            for vr in &step.verifier_results {
                let stats = verifier_stats
                    .entry(vr.name.clone())
                    .or_insert_with(VerifierStats::new);

                stats.invocations += 1;
                if vr.passed {
                    stats.passes += 1;
                }

                if converged {
                    stats.invocations_in_converged += 1;
                    if vr.passed {
                        stats.passes_in_converged += 1;
                    }
                } else {
                    stats.invocations_in_failed += 1;
                    if vr.passed {
                        stats.passes_in_failed += 1;
                    }
                }
            }
        }
    }

    verifier_stats
        .into_iter()
        .map(|(verifier, stats)| VerifierSignalReport {
            verifier,
            invocation_count: stats.invocations,
            pass_rate: if stats.invocations > 0 {
                stats.passes as f64 / stats.invocations as f64
            } else {
                0.0
            },
            pass_rate_in_converged: if stats.invocations_in_converged > 0 {
                stats.passes_in_converged as f64 / stats.invocations_in_converged as f64
            } else {
                0.0
            },
            pass_rate_in_failed: if stats.invocations_in_failed > 0 {
                stats.passes_in_failed as f64 / stats.invocations_in_failed as f64
            } else {
                0.0
            },
        })
        .collect()
}

struct VerifierStats {
    invocations: usize,
    passes: usize,
    invocations_in_converged: usize,
    passes_in_converged: usize,
    invocations_in_failed: usize,
    passes_in_failed: usize,
}

impl VerifierStats {
    fn new() -> Self {
        Self {
            invocations: 0,
            passes: 0,
            invocations_in_converged: 0,
            passes_in_converged: 0,
            invocations_in_failed: 0,
            passes_in_failed: 0,
        }
    }
}

fn tool_name_str(tool: &ToolName) -> &str {
    tool.as_str()
}

/// Format an adaptation report for display
pub fn format_report(report: &AdaptationReport) -> String {
    let mut output = String::new();

    output.push_str(&format!(
        "=== Adaptation Report: {} ===\n",
        report.field_name
    ));
    output.push_str(&format!("Runs analyzed: {}\n", report.run_count));
    output.push_str(&format!(
        "Convergence rate: {:.1}%\n",
        report.convergence_rate * 100.0
    ));
    output.push('\n');

    // Token distribution
    output.push_str("Token Usage:\n");
    output.push_str(&format!("  Mean: {:.0}\n", report.token_distribution.mean));
    output.push_str(&format!(
        "  Median: {:.0}\n",
        report.token_distribution.median
    ));
    output.push_str(&format!("  P90: {:.0}\n", report.token_distribution.p90));
    output.push_str(&format!("  P99: {:.0}\n", report.token_distribution.p99));
    output.push('\n');

    // Cost distribution
    output.push_str("Cost:\n");
    output.push_str(&format!(
        "  Mean: ${:.4}\n",
        report.cost_distribution.mean / 1_000_000.0
    ));
    output.push_str(&format!(
        "  Median: ${:.4}\n",
        report.cost_distribution.median / 1_000_000.0
    ));
    output.push_str(&format!(
        "  P90: ${:.4}\n",
        report.cost_distribution.p90 / 1_000_000.0
    ));
    output.push('\n');

    // Step distribution
    output.push_str("Steps:\n");
    output.push_str(&format!("  Mean: {:.1}\n", report.step_distribution.mean));
    output.push_str(&format!(
        "  Median: {:.0}\n",
        report.step_distribution.median
    ));
    output.push_str(&format!("  Max: {:.0}\n", report.step_distribution.max));
    output.push('\n');

    // Tool usage
    if !report.tool_usage.is_empty() {
        output.push_str("Tool Usage:\n");
        for tool in &report.tool_usage {
            output.push_str(&format!("  {}:\n", tool.tool));
            output.push_str(&format!("    Invocations: {}\n", tool.invocation_count));
            output.push_str(&format!(
                "    Used in: {}/{} runs\n",
                tool.runs_used_in, report.run_count
            ));
            output.push_str(&format!(
                "    Convergence when used: {:.1}%\n",
                tool.convergence_when_used * 100.0
            ));
            output.push_str(&format!(
                "    Convergence when NOT used: {:.1}%\n",
                tool.convergence_when_not_used * 100.0
            ));
        }
        output.push('\n');
    }

    // Verifier signals
    if !report.verifier_signals.is_empty() {
        output.push_str("Verifier Signals:\n");
        for verifier in &report.verifier_signals {
            output.push_str(&format!("  {}:\n", verifier.verifier));
            output.push_str(&format!("    Invocations: {}\n", verifier.invocation_count));
            output.push_str(&format!(
                "    Pass rate: {:.1}%\n",
                verifier.pass_rate * 100.0
            ));
            output.push_str(&format!(
                "    Pass rate in converged: {:.1}%\n",
                verifier.pass_rate_in_converged * 100.0
            ));
            output.push_str(&format!(
                "    Pass rate in failed: {:.1}%\n",
                verifier.pass_rate_in_failed * 100.0
            ));
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use portlang_core::{Cost, RunOutcome};

    #[test]
    fn test_from_summaries() {
        let summaries = vec![
            TrajectorySummary {
                id: portlang_core::TrajectoryId::new("field".to_string()),
                field_name: "test-field".to_string(),
                step_count: 10,
                total_cost: Cost::from_microdollars(1000),
                total_tokens: 500,
                started_at: chrono::Utc::now(),
                ended_at: Some(chrono::Utc::now()),
                outcome: Some(RunOutcome::Converged {
                    message: "Done".to_string(),
                }),
            },
            TrajectorySummary {
                id: portlang_core::TrajectoryId::new("field".to_string()),
                field_name: "test-field".to_string(),
                step_count: 20,
                total_cost: Cost::from_microdollars(2000),
                total_tokens: 1000,
                started_at: chrono::Utc::now(),
                ended_at: Some(chrono::Utc::now()),
                outcome: Some(RunOutcome::BudgetExhausted {
                    reason: "Out of tokens".to_string(),
                }),
            },
        ];

        let report = AdaptationReport::from_summaries("test-field".to_string(), &summaries);

        assert_eq!(report.field_name, "test-field");
        assert_eq!(report.run_count, 2);
        assert_eq!(report.convergence_rate, 0.5);
        assert_eq!(report.token_distribution.count, 2);
    }
}
