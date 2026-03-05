use portlang_core::{Action, Trajectory, TrajectoryStep};
use serde::{Deserialize, Serialize};

/// Result of comparing two trajectories
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryDiff {
    /// The two trajectories being compared
    pub trajectory_a: String,
    pub trajectory_b: String,

    /// Aligned steps (pairs of steps from A and B at the same index)
    pub aligned_steps: Vec<AlignedStep>,

    /// The first step where trajectories diverged (if any)
    pub divergence_point: Option<usize>,

    /// Reason for divergence (if any)
    pub divergence_reason: Option<DivergenceReason>,
}

/// A pair of aligned steps from two trajectories
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlignedStep {
    /// Both trajectories have a step at this index
    Both {
        step_a: StepSummary,
        step_b: StepSummary,
        matches: bool,
    },

    /// Only trajectory A has a step at this index
    OnlyA { step_a: StepSummary },

    /// Only trajectory B has a step at this index
    OnlyB { step_b: StepSummary },
}

/// Summary of a trajectory step for comparison
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepSummary {
    pub step_number: usize,
    pub action_type: String,
    pub action_target: Option<String>,
    pub rejected: bool,
    pub verifier_results: Vec<(String, bool)>,
}

impl From<&TrajectoryStep> for StepSummary {
    fn from(step: &TrajectoryStep) -> Self {
        let (action_type, action_target) = match &step.action {
            Action::ToolCall { tool, input } => {
                let tool_name = tool.as_str();

                let target = if let Some(path) = input.get("path") {
                    path.as_str().map(|s| s.to_string())
                } else if let Some(pattern) = input.get("pattern") {
                    pattern.as_str().map(|s| s.to_string())
                } else {
                    None
                };

                (format!("tool:{}", tool_name), target)
            }
            Action::TextOutput { .. } => ("text".to_string(), None),
            Action::Stop => ("stop".to_string(), None),
        };

        let verifier_results = step
            .verifier_results
            .iter()
            .map(|vr| (vr.name.clone(), vr.passed))
            .collect();

        Self {
            step_number: step.step_number,
            action_type,
            action_target,
            rejected: step.rejected,
            verifier_results,
        }
    }
}

/// Reason why two trajectories diverged
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DivergenceReason {
    /// Different action types (e.g. read vs write)
    DifferentActionType { a: String, b: String },

    /// Same action type but different targets (e.g. different files)
    DifferentTarget { a: String, b: String },

    /// One action was rejected, the other wasn't
    DifferentRejection,

    /// Different verifier outcomes
    DifferentVerifierOutcome { verifier: String },

    /// Different number of steps
    StepCountMismatch { a: usize, b: usize },
}

/// Compare two trajectories and identify divergence
pub fn diff_trajectories(a: &Trajectory, b: &Trajectory) -> TrajectoryDiff {
    let mut aligned_steps = Vec::new();
    let mut divergence_point = None;
    let mut divergence_reason = None;

    let max_steps = a.steps.len().max(b.steps.len());

    for i in 0..max_steps {
        let step_a = a.steps.get(i);
        let step_b = b.steps.get(i);

        match (step_a, step_b) {
            (Some(sa), Some(sb)) => {
                let summary_a = StepSummary::from(sa);
                let summary_b = StepSummary::from(sb);

                // Check if steps match
                let (matches, reason) = compare_steps(&summary_a, &summary_b);

                aligned_steps.push(AlignedStep::Both {
                    step_a: summary_a,
                    step_b: summary_b,
                    matches,
                });

                // Record first divergence
                if !matches && divergence_point.is_none() {
                    divergence_point = Some(i);
                    divergence_reason = reason;
                }
            }
            (Some(sa), None) => {
                let summary_a = StepSummary::from(sa);
                aligned_steps.push(AlignedStep::OnlyA { step_a: summary_a });

                if divergence_point.is_none() {
                    divergence_point = Some(i);
                    divergence_reason = Some(DivergenceReason::StepCountMismatch {
                        a: a.steps.len(),
                        b: b.steps.len(),
                    });
                }
            }
            (None, Some(sb)) => {
                let summary_b = StepSummary::from(sb);
                aligned_steps.push(AlignedStep::OnlyB { step_b: summary_b });

                if divergence_point.is_none() {
                    divergence_point = Some(i);
                    divergence_reason = Some(DivergenceReason::StepCountMismatch {
                        a: a.steps.len(),
                        b: b.steps.len(),
                    });
                }
            }
            (None, None) => break,
        }
    }

    TrajectoryDiff {
        trajectory_a: a.id.filename(),
        trajectory_b: b.id.filename(),
        aligned_steps,
        divergence_point,
        divergence_reason,
    }
}

/// Compare two step summaries
fn compare_steps(a: &StepSummary, b: &StepSummary) -> (bool, Option<DivergenceReason>) {
    // Check action type
    if a.action_type != b.action_type {
        return (
            false,
            Some(DivergenceReason::DifferentActionType {
                a: a.action_type.clone(),
                b: b.action_type.clone(),
            }),
        );
    }

    // Check target (if present)
    if a.action_target != b.action_target {
        return (
            false,
            Some(DivergenceReason::DifferentTarget {
                a: a.action_target.clone().unwrap_or_default(),
                b: b.action_target.clone().unwrap_or_default(),
            }),
        );
    }

    // Check rejection status
    if a.rejected != b.rejected {
        return (false, Some(DivergenceReason::DifferentRejection));
    }

    // Check verifier results
    for (name_a, passed_a) in &a.verifier_results {
        if let Some((_, passed_b)) = b.verifier_results.iter().find(|(n, _)| n == name_a) {
            if passed_a != passed_b {
                return (
                    false,
                    Some(DivergenceReason::DifferentVerifierOutcome {
                        verifier: name_a.clone(),
                    }),
                );
            }
        }
    }

    (true, None)
}

/// Format a diff for display
pub fn format_diff(diff: &TrajectoryDiff) -> String {
    let mut output = String::new();

    output.push_str(&format!("Comparing:\n"));
    output.push_str(&format!("  A: {}\n", diff.trajectory_a));
    output.push_str(&format!("  B: {}\n", diff.trajectory_b));
    output.push('\n');

    if let Some(point) = diff.divergence_point {
        output.push_str(&format!("Divergence at step {}\n", point));
        if let Some(ref reason) = diff.divergence_reason {
            output.push_str(&format!("Reason: {}\n", format_divergence_reason(reason)));
        }
        output.push('\n');
    } else {
        output.push_str("Trajectories are identical\n\n");
    }

    output.push_str("Step-by-step comparison:\n");
    for (i, aligned) in diff.aligned_steps.iter().enumerate() {
        match aligned {
            AlignedStep::Both {
                step_a,
                step_b,
                matches,
            } => {
                let marker = if *matches { "=" } else { "≠" };
                output.push_str(&format!(
                    "  {} Step {}: {} {:?} {} {} {:?}\n",
                    marker,
                    i,
                    step_a.action_type,
                    step_a.action_target,
                    if *matches { "==" } else { "!=" },
                    step_b.action_type,
                    step_b.action_target
                ));
            }
            AlignedStep::OnlyA { step_a } => {
                output.push_str(&format!(
                    "  < Step {}: {} {:?} (only in A)\n",
                    i, step_a.action_type, step_a.action_target
                ));
            }
            AlignedStep::OnlyB { step_b } => {
                output.push_str(&format!(
                    "  > Step {}: {} {:?} (only in B)\n",
                    i, step_b.action_type, step_b.action_target
                ));
            }
        }
    }

    output
}

fn format_divergence_reason(reason: &DivergenceReason) -> String {
    match reason {
        DivergenceReason::DifferentActionType { a, b } => {
            format!("Different action types: {} vs {}", a, b)
        }
        DivergenceReason::DifferentTarget { a, b } => {
            format!("Different targets: {} vs {}", a, b)
        }
        DivergenceReason::DifferentRejection => {
            "One action was rejected, the other wasn't".to_string()
        }
        DivergenceReason::DifferentVerifierOutcome { verifier } => {
            format!("Different verifier outcome for: {}", verifier)
        }
        DivergenceReason::StepCountMismatch { a, b } => {
            format!("Different number of steps: {} vs {}", a, b)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use portlang_core::{Cost, RunOutcome};

    #[test]
    fn test_identical_trajectories() {
        let traj_a = create_test_trajectory("field-a", 3);
        let traj_b = create_test_trajectory("field-b", 3);

        let diff = diff_trajectories(&traj_a, &traj_b);

        assert_eq!(diff.divergence_point, None);
        assert_eq!(diff.aligned_steps.len(), 3);
    }

    #[test]
    fn test_different_step_count() {
        let traj_a = create_test_trajectory("field-a", 3);
        let traj_b = create_test_trajectory("field-b", 5);

        let diff = diff_trajectories(&traj_a, &traj_b);

        assert_eq!(diff.divergence_point, Some(3));
        assert_eq!(diff.aligned_steps.len(), 5);
    }

    #[test]
    fn test_different_action_type() {
        let mut traj_a = Trajectory::new("test-a".to_string());
        let mut traj_b = Trajectory::new("test-b".to_string());

        // Same first step
        traj_a.add_step(TrajectoryStep::new(
            1,
            Action::TextOutput {
                text: "Hello".to_string(),
            },
            "Result".to_string(),
            false,
            Cost::from_microdollars(0),
            100,
        ));

        traj_b.add_step(TrajectoryStep::new(
            1,
            Action::TextOutput {
                text: "Hello".to_string(),
            },
            "Result".to_string(),
            false,
            Cost::from_microdollars(0),
            100,
        ));

        // Different second step
        traj_a.add_step(TrajectoryStep::new(
            2,
            Action::TextOutput {
                text: "Text".to_string(),
            },
            "Result".to_string(),
            false,
            Cost::from_microdollars(0),
            100,
        ));

        traj_b.add_step(TrajectoryStep::new(
            2,
            Action::Stop,
            "Stopped".to_string(),
            false,
            Cost::from_microdollars(0),
            100,
        ));

        let diff = diff_trajectories(&traj_a, &traj_b);

        assert_eq!(diff.divergence_point, Some(1)); // Second step (0-indexed)
        assert!(matches!(
            diff.divergence_reason,
            Some(DivergenceReason::DifferentActionType { .. })
        ));
    }

    #[test]
    fn test_format_diff() {
        let traj_a = create_test_trajectory("field-a", 2);
        let traj_b = create_test_trajectory("field-b", 3);

        let diff = diff_trajectories(&traj_a, &traj_b);
        let output = format_diff(&diff);

        assert!(output.contains("Comparing"));
        assert!(output.contains("Divergence at step"));
    }

    // Helper to create a test trajectory with N steps
    fn create_test_trajectory(name: &str, steps: usize) -> Trajectory {
        let mut traj = Trajectory::new(name.to_string());

        for i in 0..steps {
            traj.add_step(TrajectoryStep::new(
                i + 1,
                Action::TextOutput {
                    text: format!("Step {}", i + 1),
                },
                "Result".to_string(),
                false,
                Cost::from_microdollars(0),
                100,
            ));
        }

        traj.finish(RunOutcome::Converged {
            message: "Done".to_string(),
        });

        traj
    }
}
