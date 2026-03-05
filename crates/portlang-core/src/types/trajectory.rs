use super::{action::Action, cost::Cost, verifier::VerifierResult};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Unique identifier for a trajectory
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrajectoryId {
    /// Name of the field that generated this trajectory
    pub field_name: String,

    /// Timestamp when the trajectory was created
    pub timestamp: DateTime<Utc>,

    /// Random suffix for uniqueness
    pub random_suffix: String,
}

impl TrajectoryId {
    pub fn new(field_name: String) -> Self {
        use chrono::Utc;
        let timestamp = Utc::now();
        let random_suffix = format!("{:08x}", rand::random::<u32>());

        Self {
            field_name,
            timestamp,
            random_suffix,
        }
    }

    /// Get the filename for this trajectory
    pub fn filename(&self) -> String {
        format!(
            "{}-{}.json",
            self.timestamp.format("%Y%m%d-%H%M%S"),
            self.random_suffix
        )
    }

    /// Get the directory path for this trajectory
    pub fn directory(&self) -> String {
        self.field_name.clone()
    }
}

/// A complete trajectory of field execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trajectory {
    /// Unique identifier
    pub id: TrajectoryId,

    /// Field name
    pub field_name: String,

    /// Steps in the trajectory
    pub steps: Vec<TrajectoryStep>,

    /// Total cost incurred
    pub total_cost: Cost,

    /// Total tokens used
    pub total_tokens: u64,

    /// Start time
    pub started_at: DateTime<Utc>,

    /// End time (None if still running)
    pub ended_at: Option<DateTime<Utc>>,

    /// Final outcome
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<crate::outcome::RunOutcome>,
}

impl Trajectory {
    pub fn new(field_name: String) -> Self {
        let id = TrajectoryId::new(field_name.clone());

        Self {
            id,
            field_name,
            steps: Vec::new(),
            total_cost: Cost::ZERO,
            total_tokens: 0,
            started_at: Utc::now(),
            ended_at: None,
            outcome: None,
        }
    }

    pub fn add_step(&mut self, step: TrajectoryStep) {
        self.total_cost += step.cost;
        self.total_tokens += step.tokens_used;
        self.steps.push(step);
    }

    pub fn finish(&mut self, outcome: crate::outcome::RunOutcome) {
        self.ended_at = Some(Utc::now());
        self.outcome = Some(outcome);
    }

    pub fn step_count(&self) -> usize {
        self.steps.len()
    }
}

/// A single step in a trajectory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryStep {
    /// Step number (1-indexed)
    pub step_number: usize,

    /// The action taken by the agent
    pub action: Action,

    /// Result of executing the action
    pub result: String,

    /// Whether the action was rejected by boundary check
    pub rejected: bool,

    /// Verifier results for this step
    #[serde(default)]
    pub verifier_results: Vec<VerifierResult>,

    /// Cost of this step
    pub cost: Cost,

    /// Tokens used in this step
    pub tokens_used: u64,

    /// Timestamp of this step
    pub timestamp: DateTime<Utc>,
}

impl TrajectoryStep {
    pub fn new(
        step_number: usize,
        action: Action,
        result: String,
        rejected: bool,
        cost: Cost,
        tokens_used: u64,
    ) -> Self {
        Self {
            step_number,
            action,
            result,
            rejected,
            verifier_results: Vec::new(),
            cost,
            tokens_used,
            timestamp: Utc::now(),
        }
    }

    pub fn with_verifier_results(mut self, results: Vec<VerifierResult>) -> Self {
        self.verifier_results = results;
        self
    }
}
