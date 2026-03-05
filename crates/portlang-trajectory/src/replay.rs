use portlang_core::{Trajectory, TrajectoryStep};

/// A replay session for stepping through a trajectory
pub struct ReplaySession {
    trajectory: Trajectory,
    current_step: usize,
}

impl ReplaySession {
    /// Create a new replay session
    pub fn new(trajectory: Trajectory) -> Self {
        Self {
            trajectory,
            current_step: 0,
        }
    }

    /// Get the trajectory being replayed
    pub fn trajectory(&self) -> &Trajectory {
        &self.trajectory
    }

    /// Get the current step number (0-indexed)
    pub fn current_step_number(&self) -> usize {
        self.current_step
    }

    /// Get the total number of steps
    pub fn total_steps(&self) -> usize {
        self.trajectory.step_count()
    }

    /// Check if we're at the beginning
    pub fn is_at_start(&self) -> bool {
        self.current_step == 0
    }

    /// Check if we're at the end
    pub fn is_at_end(&self) -> bool {
        self.current_step >= self.trajectory.step_count()
    }

    /// Get the current step
    pub fn current(&self) -> Option<&TrajectoryStep> {
        if self.current_step < self.trajectory.step_count() {
            self.trajectory.steps.get(self.current_step)
        } else {
            None
        }
    }

    /// Move to the next step
    pub fn next(&mut self) -> Option<&TrajectoryStep> {
        if self.current_step < self.trajectory.step_count() {
            self.current_step += 1;
        }
        self.current()
    }

    /// Move to the previous step
    pub fn prev(&mut self) -> Option<&TrajectoryStep> {
        if self.current_step > 0 {
            self.current_step -= 1;
        }
        self.current()
    }

    /// Go to a specific step
    pub fn goto(&mut self, step: usize) -> Option<&TrajectoryStep> {
        if step <= self.trajectory.step_count() {
            self.current_step = step;
        }
        self.current()
    }

    /// Reset to the beginning
    pub fn reset(&mut self) {
        self.current_step = 0;
    }

    /// Get a step by index
    pub fn get_step(&self, index: usize) -> Option<&TrajectoryStep> {
        self.trajectory.steps.get(index)
    }

    /// Get all steps
    pub fn steps(&self) -> &[TrajectoryStep] {
        &self.trajectory.steps
    }
}

/// Format a replay session for display
pub fn format_session(session: &ReplaySession) -> String {
    let mut output = String::new();

    output.push_str(&format!("Field: {}\n", session.trajectory().field_name));
    output.push_str(&format!("ID: {}\n", session.trajectory().id.filename()));
    output.push_str(&format!(
        "Steps: {}/{}\n",
        session.current_step_number(),
        session.total_steps()
    ));
    output.push_str(&format!(
        "Total Cost: {}\n",
        session.trajectory().total_cost
    ));
    output.push_str(&format!(
        "Total Tokens: {}\n",
        session.trajectory().total_tokens
    ));

    if let Some(outcome) = &session.trajectory().outcome {
        output.push_str(&format!("Outcome: {}\n", outcome.description()));
    }

    output.push('\n');

    if let Some(step) = session.current() {
        output.push_str(&format_step(step));
    } else {
        output.push_str("(No current step)\n");
    }

    output
}

/// Format a single trajectory step for display
pub fn format_step(step: &TrajectoryStep) -> String {
    let mut output = String::new();

    output.push_str(&format!("=== Step {} ===\n", step.step_number));
    output.push_str(&format!("Action: {:?}\n", step.action));
    output.push_str(&format!("Result: {}\n", step.result));

    if step.rejected {
        output.push_str("Status: REJECTED\n");
    }

    if !step.verifier_results.is_empty() {
        output.push_str("\nVerifiers:\n");
        for vr in &step.verifier_results {
            output.push_str(&format!(
                "  - {}: {}\n",
                vr.name,
                if vr.passed { "PASS" } else { "FAIL" }
            ));
            if !vr.stderr.is_empty() {
                output.push_str(&format!("    stderr: {}\n", vr.stderr));
            }
        }
    }

    output.push_str(&format!("\nTokens: {}\n", step.tokens_used));
    output.push_str(&format!("Cost: {}\n", step.cost));

    output
}

/// Format a summary of all steps
pub fn format_summary(trajectory: &Trajectory) -> String {
    let mut output = String::new();

    output.push_str(&format!("Field: {}\n", trajectory.field_name));
    output.push_str(&format!("ID: {}\n", trajectory.id.filename()));
    output.push_str(&format!("Steps: {}\n", trajectory.step_count()));
    output.push_str(&format!("Total Cost: {}\n", trajectory.total_cost));
    output.push_str(&format!("Total Tokens: {}\n", trajectory.total_tokens));

    if let Some(outcome) = &trajectory.outcome {
        output.push_str(&format!("Outcome: {}\n", outcome.description()));
    }

    output.push_str("\n--- Steps ---\n");

    for step in &trajectory.steps {
        output.push_str(&format!(
            "{}: {:?} -> {}\n",
            step.step_number,
            step.action,
            if step.rejected { "REJECTED" } else { "OK" }
        ));
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use portlang_core::{Action, Cost, RunOutcome};

    #[test]
    fn test_replay_navigation() {
        let mut trajectory = Trajectory::new("test-field".to_string());

        // Add some steps
        let step1 = TrajectoryStep::new(
            1,
            Action::TextOutput {
                text: "Step 1".to_string(),
            },
            "Result 1".to_string(),
            false,
            Cost::from_microdollars(0),
            100,
        );

        let step2 = TrajectoryStep::new(
            2,
            Action::TextOutput {
                text: "Step 2".to_string(),
            },
            "Result 2".to_string(),
            false,
            Cost::from_microdollars(0),
            100,
        );

        trajectory.add_step(step1);
        trajectory.add_step(step2);
        trajectory.finish(RunOutcome::Converged {
            message: "Done".to_string(),
        });

        let mut session = ReplaySession::new(trajectory);

        // Start at step 0
        assert_eq!(session.current_step_number(), 0);
        assert!(session.is_at_start());

        // Move to next
        session.next();
        assert_eq!(session.current_step_number(), 1);

        // Move to next again
        session.next();
        assert_eq!(session.current_step_number(), 2);
        assert!(session.is_at_end());

        // Move back
        session.prev();
        assert_eq!(session.current_step_number(), 1);

        // Go to specific step
        session.goto(0);
        assert_eq!(session.current_step_number(), 0);

        // Reset
        session.reset();
        assert_eq!(session.current_step_number(), 0);
    }

    #[test]
    fn test_format_functions() {
        let mut trajectory = Trajectory::new("test-field".to_string());

        let step = TrajectoryStep::new(
            1,
            Action::TextOutput {
                text: "Test".to_string(),
            },
            "Result".to_string(),
            false,
            Cost::from_microdollars(0),
            100,
        );

        trajectory.add_step(step);
        trajectory.finish(RunOutcome::Converged {
            message: "Done".to_string(),
        });

        // Test format functions don't panic
        let session = ReplaySession::new(trajectory.clone());
        let _output = format_session(&session);
        let _summary = format_summary(&trajectory);
        let _step_output = format_step(&trajectory.steps[0]);
    }
}
