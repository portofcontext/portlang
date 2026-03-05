use chrono::{DateTime, Utc};
use portlang_core::{Cost, RunOutcome, TrajectoryId};
use serde::{Deserialize, Serialize};

/// Summary information about a trajectory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectorySummary {
    /// Trajectory ID
    pub id: TrajectoryId,

    /// Field name
    pub field_name: String,

    /// Number of steps
    pub step_count: usize,

    /// Total cost
    pub total_cost: Cost,

    /// Total tokens used
    pub total_tokens: u64,

    /// Start time
    pub started_at: DateTime<Utc>,

    /// End time
    pub ended_at: Option<DateTime<Utc>>,

    /// Final outcome
    pub outcome: Option<RunOutcome>,
}

impl From<&portlang_core::Trajectory> for TrajectorySummary {
    fn from(trajectory: &portlang_core::Trajectory) -> Self {
        Self {
            id: trajectory.id.clone(),
            field_name: trajectory.field_name.clone(),
            step_count: trajectory.step_count(),
            total_cost: trajectory.total_cost,
            total_tokens: trajectory.total_tokens,
            started_at: trajectory.started_at,
            ended_at: trajectory.ended_at,
            outcome: trajectory.outcome.clone(),
        }
    }
}

/// Query filters for searching trajectories
#[derive(Debug, Clone, Default)]
pub struct TrajectoryQuery {
    /// Filter by field name (exact match)
    pub field_name: Option<String>,

    /// Filter by outcome type
    pub outcome_filter: OutcomeFilter,

    /// Filter by minimum cost
    pub min_cost: Option<Cost>,

    /// Filter by maximum cost
    pub max_cost: Option<Cost>,

    /// Filter by minimum tokens
    pub min_tokens: Option<u64>,

    /// Filter by maximum tokens
    pub max_tokens: Option<u64>,

    /// Filter by date range (trajectories started after this time)
    pub started_after: Option<DateTime<Utc>>,

    /// Filter by date range (trajectories started before this time)
    pub started_before: Option<DateTime<Utc>>,

    /// Maximum number of results to return
    pub limit: Option<usize>,
}

/// Filter for trajectory outcomes
#[derive(Debug, Clone, Default)]
pub enum OutcomeFilter {
    /// No filter - return all outcomes
    #[default]
    Any,

    /// Only converged trajectories
    Converged,

    /// Only failed trajectories (any non-converged outcome)
    Failed,

    /// Only specific outcome type
    Specific(RunOutcome),
}

impl TrajectoryQuery {
    /// Create a new empty query (matches all trajectories)
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by field name
    pub fn field(mut self, field_name: impl Into<String>) -> Self {
        self.field_name = Some(field_name.into());
        self
    }

    /// Filter for only converged trajectories
    pub fn only_converged(mut self) -> Self {
        self.outcome_filter = OutcomeFilter::Converged;
        self
    }

    /// Filter for only failed trajectories
    pub fn only_failed(mut self) -> Self {
        self.outcome_filter = OutcomeFilter::Failed;
        self
    }

    /// Filter by cost range
    pub fn cost_range(mut self, min: Cost, max: Cost) -> Self {
        self.min_cost = Some(min);
        self.max_cost = Some(max);
        self
    }

    /// Filter by token range
    pub fn token_range(mut self, min: u64, max: u64) -> Self {
        self.min_tokens = Some(min);
        self.max_tokens = Some(max);
        self
    }

    /// Filter by date range
    pub fn date_range(mut self, after: DateTime<Utc>, before: DateTime<Utc>) -> Self {
        self.started_after = Some(after);
        self.started_before = Some(before);
        self
    }

    /// Limit the number of results
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Check if a summary matches this query
    pub fn matches(&self, summary: &TrajectorySummary) -> bool {
        // Field name filter
        if let Some(ref field) = self.field_name {
            if summary.field_name != *field {
                return false;
            }
        }

        // Outcome filter
        if let Some(ref outcome) = summary.outcome {
            match &self.outcome_filter {
                OutcomeFilter::Any => {}
                OutcomeFilter::Converged => {
                    if !outcome.is_success() {
                        return false;
                    }
                }
                OutcomeFilter::Failed => {
                    if outcome.is_success() {
                        return false;
                    }
                }
                OutcomeFilter::Specific(ref expected) => {
                    if !outcome.matches_type(expected) {
                        return false;
                    }
                }
            }
        }

        // Cost filters
        if let Some(min) = self.min_cost {
            if summary.total_cost < min {
                return false;
            }
        }
        if let Some(max) = self.max_cost {
            if summary.total_cost > max {
                return false;
            }
        }

        // Token filters
        if let Some(min) = self.min_tokens {
            if summary.total_tokens < min {
                return false;
            }
        }
        if let Some(max) = self.max_tokens {
            if summary.total_tokens > max {
                return false;
            }
        }

        // Date filters
        if let Some(after) = self.started_after {
            if summary.started_at < after {
                return false;
            }
        }
        if let Some(before) = self.started_before {
            if summary.started_at > before {
                return false;
            }
        }

        true
    }
}
