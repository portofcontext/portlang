use crate::error::Result;
use crate::types::{TrajectoryQuery, TrajectorySummary};
use portlang_core::{Trajectory, TrajectoryId};

/// Trait for storing and retrieving trajectories
pub trait TrajectoryStore: Send + Sync {
    /// Save a trajectory
    fn save(&self, trajectory: &Trajectory) -> Result<()>;

    /// Load a trajectory by ID
    fn load(&self, id: &TrajectoryId) -> Result<Trajectory>;

    /// List all trajectories for a field
    fn list(&self, field_name: &str) -> Result<Vec<TrajectorySummary>>;

    /// List all trajectories across all fields
    fn list_all(&self) -> Result<Vec<TrajectorySummary>>;

    /// Query trajectories with filters
    fn query(&self, query: &TrajectoryQuery) -> Result<Vec<TrajectorySummary>>;

    /// Delete a trajectory
    fn delete(&self, id: &TrajectoryId) -> Result<()>;
}
