use crate::error::{Result, TrajectoryError};
use crate::store::TrajectoryStore;
use crate::types::{TrajectoryQuery, TrajectorySummary};
use portlang_core::{Trajectory, TrajectoryId};
use std::fs;
use std::path::{Path, PathBuf};

/// Filesystem-based trajectory storage
/// Stores trajectories in ~/.portlang/trajectories/{field_name}/{timestamp}-{suffix}.json
pub struct FilesystemStore {
    base_path: PathBuf,
}

impl FilesystemStore {
    /// Create a new filesystem store with default location (~/.portlang/trajectories)
    pub fn new() -> Result<Self> {
        let home = std::env::var("HOME")
            .map_err(|_| TrajectoryError::Other("HOME environment variable not set".to_string()))?;

        let base_path = PathBuf::from(home).join(".portlang").join("trajectories");

        Ok(Self { base_path })
    }

    /// Create a new filesystem store with a custom base path
    pub fn with_path(base_path: impl AsRef<Path>) -> Self {
        Self {
            base_path: base_path.as_ref().to_path_buf(),
        }
    }

    /// Find a trajectory by filename (searches all field directories)
    pub fn find_by_filename(&self, filename: &str) -> Result<Trajectory> {
        let filename = if filename.ends_with(".json") {
            filename.to_string()
        } else {
            format!("{}.json", filename)
        };

        if !self.base_path.exists() {
            return Err(TrajectoryError::NotFound(format!(
                "Trajectory not found: {}",
                filename
            )));
        }

        // Search through all field directories
        for entry in fs::read_dir(&self.base_path)? {
            let entry = entry?;
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            // Check if this field directory contains the trajectory
            let trajectory_path = path.join(&filename);
            if trajectory_path.exists() {
                let json = fs::read_to_string(&trajectory_path)?;
                let trajectory: Trajectory = serde_json::from_str(&json)?;
                return Ok(trajectory);
            }
        }

        Err(TrajectoryError::NotFound(format!(
            "Trajectory not found: {}",
            filename
        )))
    }

    /// Get the directory path for a field
    fn field_dir(&self, field_name: &str) -> PathBuf {
        self.base_path.join(field_name)
    }

    /// Get the file path for a trajectory
    fn trajectory_path(&self, id: &TrajectoryId) -> PathBuf {
        self.field_dir(&id.field_name).join(id.filename())
    }

    /// Ensure the field directory exists
    fn ensure_field_dir(&self, field_name: &str) -> Result<PathBuf> {
        let dir = self.field_dir(field_name);
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }
}

impl Default for FilesystemStore {
    fn default() -> Self {
        Self::new().expect("Failed to create default filesystem store")
    }
}

impl FilesystemStore {
    /// Save a trajectory with secrets redacted before writing to disk.
    pub fn save_redacted(&self, trajectory: &Trajectory, secrets: &[String]) -> Result<()> {
        if secrets.is_empty() {
            return self.save(trajectory);
        }
        let redactor = crate::redactor::Redactor::new(secrets.to_vec());
        let redacted = redactor.redact_trajectory(trajectory);
        self.save(&redacted)
    }
}

impl TrajectoryStore for FilesystemStore {
    fn save(&self, trajectory: &Trajectory) -> Result<()> {
        self.ensure_field_dir(&trajectory.field_name)?;

        let path = self.trajectory_path(&trajectory.id);
        let json = serde_json::to_string_pretty(trajectory)?;

        fs::write(&path, json)?;

        Ok(())
    }

    fn load(&self, id: &TrajectoryId) -> Result<Trajectory> {
        let path = self.trajectory_path(id);

        if !path.exists() {
            return Err(TrajectoryError::NotFound(format!(
                "Trajectory file not found: {}",
                path.display()
            )));
        }

        let json = fs::read_to_string(&path)?;
        let trajectory: Trajectory = serde_json::from_str(&json)?;

        Ok(trajectory)
    }

    fn list(&self, field_name: &str) -> Result<Vec<TrajectorySummary>> {
        let dir = self.field_dir(field_name);

        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut summaries = Vec::new();

        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }

            // Try to load and extract summary
            if let Ok(json) = fs::read_to_string(&path) {
                if let Ok(trajectory) = serde_json::from_str::<Trajectory>(&json) {
                    summaries.push(TrajectorySummary::from(&trajectory));
                }
            }
        }

        // Sort by start time (newest first)
        summaries.sort_by(|a, b| b.started_at.cmp(&a.started_at));

        Ok(summaries)
    }

    fn delete(&self, id: &TrajectoryId) -> Result<()> {
        let path = self.trajectory_path(id);

        if !path.exists() {
            return Err(TrajectoryError::NotFound(format!(
                "Trajectory file not found: {}",
                path.display()
            )));
        }

        fs::remove_file(&path)?;

        Ok(())
    }

    fn list_all(&self) -> Result<Vec<TrajectorySummary>> {
        let mut summaries = Vec::new();

        if !self.base_path.exists() {
            return Ok(summaries);
        }

        // Iterate through all field directories
        for entry in fs::read_dir(&self.base_path)? {
            let entry = entry?;
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            // Get the field name from the directory
            if let Some(field_name) = path.file_name().and_then(|n| n.to_str()) {
                // List all trajectories for this field
                let field_summaries = self.list(field_name)?;
                summaries.extend(field_summaries);
            }
        }

        // Sort by start time (newest first)
        summaries.sort_by(|a, b| b.started_at.cmp(&a.started_at));

        Ok(summaries)
    }

    fn query(&self, query: &TrajectoryQuery) -> Result<Vec<TrajectorySummary>> {
        // If a specific field is requested, only scan that directory
        // Otherwise scan all fields
        let mut summaries = if let Some(ref field_name) = query.field_name {
            self.list(field_name)?
        } else {
            self.list_all()?
        };

        // Apply filters
        summaries.retain(|s| query.matches(s));

        // Sort by start time (newest first)
        summaries.sort_by(|a, b| b.started_at.cmp(&a.started_at));

        // Apply limit
        if let Some(limit) = query.limit {
            summaries.truncate(limit);
        }

        Ok(summaries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use portlang_core::RunOutcome;
    use tempfile::TempDir;

    #[test]
    fn test_save_redacted_removes_secrets_from_json() {
        use portlang_core::{Action, Cost, TrajectoryStep};

        let temp_dir = TempDir::new().unwrap();
        let store = FilesystemStore::with_path(temp_dir.path());

        let secret = "super-secret-api-key-xyz";
        let mut trajectory = Trajectory::new("test-field".to_string());
        trajectory.goal = format!("goal mentioning {secret}");
        trajectory.add_step(TrajectoryStep::new(
            1,
            Action::stop(),
            format!("result containing {secret}"),
            false,
            Cost::ZERO,
            0,
        ));
        trajectory.finish(RunOutcome::Converged {
            message: "done".to_string(),
        });

        store
            .save_redacted(&trajectory, &[secret.to_string()])
            .unwrap();

        // Scan the entire trajectory directory for any file containing the secret
        let field_dir = temp_dir.path().join("test-field");
        for entry in fs::read_dir(&field_dir).unwrap() {
            let path = entry.unwrap().path();
            let json = fs::read_to_string(&path).unwrap();
            assert!(
                !json.contains(secret),
                "secret should not appear in saved JSON"
            );
            assert!(
                json.contains("[REDACTED]"),
                "redaction marker should be present"
            );
        }
    }

    #[test]
    fn test_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let store = FilesystemStore::with_path(temp_dir.path());

        let mut trajectory = Trajectory::new("test-field".to_string());
        trajectory.finish(RunOutcome::Converged {
            message: "Done".to_string(),
        });

        store.save(&trajectory).unwrap();

        let loaded = store.load(&trajectory.id).unwrap();
        assert_eq!(loaded.field_name, "test-field");
        assert!(loaded.outcome.is_some());
    }

    #[test]
    fn test_list() {
        let temp_dir = TempDir::new().unwrap();
        let store = FilesystemStore::with_path(temp_dir.path());

        let mut traj1 = Trajectory::new("test-field".to_string());
        traj1.finish(RunOutcome::Converged {
            message: "Done 1".to_string(),
        });

        let mut traj2 = Trajectory::new("test-field".to_string());
        traj2.finish(RunOutcome::Converged {
            message: "Done 2".to_string(),
        });

        store.save(&traj1).unwrap();
        store.save(&traj2).unwrap();

        let summaries = store.list("test-field").unwrap();
        assert_eq!(summaries.len(), 2);
    }

    #[test]
    fn test_delete() {
        let temp_dir = TempDir::new().unwrap();
        let store = FilesystemStore::with_path(temp_dir.path());

        let mut trajectory = Trajectory::new("test-field".to_string());
        trajectory.finish(RunOutcome::Converged {
            message: "Done".to_string(),
        });

        store.save(&trajectory).unwrap();
        store.delete(&trajectory.id).unwrap();

        let result = store.load(&trajectory.id);
        assert!(result.is_err());
    }

    #[test]
    fn test_list_all() {
        let temp_dir = TempDir::new().unwrap();
        let store = FilesystemStore::with_path(temp_dir.path());

        let mut traj1 = Trajectory::new("field-a".to_string());
        traj1.finish(RunOutcome::Converged {
            message: "Done".to_string(),
        });

        let mut traj2 = Trajectory::new("field-b".to_string());
        traj2.finish(RunOutcome::BudgetExhausted {
            reason: "Out of tokens".to_string(),
        });

        store.save(&traj1).unwrap();
        store.save(&traj2).unwrap();

        let all_summaries = store.list_all().unwrap();
        assert_eq!(all_summaries.len(), 2);
    }

    #[test]
    fn test_query() {
        use crate::types::TrajectoryQuery;

        let temp_dir = TempDir::new().unwrap();
        let store = FilesystemStore::with_path(temp_dir.path());

        let mut traj1 = Trajectory::new("test-field".to_string());
        traj1.finish(RunOutcome::Converged {
            message: "Done".to_string(),
        });

        let mut traj2 = Trajectory::new("test-field".to_string());
        traj2.finish(RunOutcome::BudgetExhausted {
            reason: "Out of tokens".to_string(),
        });

        let mut traj3 = Trajectory::new("other-field".to_string());
        traj3.finish(RunOutcome::Converged {
            message: "Also done".to_string(),
        });

        store.save(&traj1).unwrap();
        store.save(&traj2).unwrap();
        store.save(&traj3).unwrap();

        // Query for only converged trajectories
        let query = TrajectoryQuery::new().only_converged();
        let results = store.query(&query).unwrap();
        assert_eq!(results.len(), 2);

        // Query for specific field
        let query = TrajectoryQuery::new().field("test-field");
        let results = store.query(&query).unwrap();
        assert_eq!(results.len(), 2);

        // Query with limit
        let query = TrajectoryQuery::new().limit(1);
        let results = store.query(&query).unwrap();
        assert_eq!(results.len(), 1);
    }
}
