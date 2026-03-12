use crate::error::{Result, TrajectoryError};
use chrono::{DateTime, Utc};
use portlang_core::{Cost, TrajectoryId};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// A completed eval run — a snapshot of running all fields in an eval directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalRun {
    /// Unique ID: "YYYYMMDD-HHMMSS-xxxxxxxx"
    pub id: String,

    /// The eval directory as provided by the user (informational)
    pub eval_dir: String,

    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,

    /// IDs of each trajectory produced by this run, in order
    pub trajectory_ids: Vec<TrajectoryId>,

    // Denormalized summary for fast listing
    pub task_count: usize,
    pub passed_count: usize,
    pub total_cost: Cost,
    pub total_tokens: u64,
}

impl EvalRun {
    pub fn new(eval_dir: String, started_at: DateTime<Utc>) -> Self {
        let timestamp = Utc::now();
        let suffix = format!("{:08x}", rand::random::<u32>());
        let id = format!("{}-{}", timestamp.format("%Y%m%d-%H%M%S"), suffix);

        Self {
            id,
            eval_dir,
            started_at,
            finished_at: timestamp,
            trajectory_ids: Vec::new(),
            task_count: 0,
            passed_count: 0,
            total_cost: Cost::ZERO,
            total_tokens: 0,
        }
    }

    pub fn filename(&self) -> String {
        format!("{}.json", self.id)
    }

    pub fn pass_rate(&self) -> f64 {
        if self.task_count == 0 {
            return 0.0;
        }
        self.passed_count as f64 / self.task_count as f64 * 100.0
    }
}

/// Filesystem store for eval runs at ~/.portlang/evals/
pub struct EvalRunStore {
    base_path: PathBuf,
}

impl EvalRunStore {
    pub fn new() -> Result<Self> {
        let home = std::env::var("HOME")
            .map_err(|_| TrajectoryError::Other("HOME environment variable not set".to_string()))?;
        let base_path = PathBuf::from(home).join(".portlang").join("evals");
        Ok(Self { base_path })
    }

    pub fn save(&self, run: &EvalRun) -> Result<()> {
        fs::create_dir_all(&self.base_path)?;
        let path = self.base_path.join(run.filename());
        let json = serde_json::to_string_pretty(run)?;
        fs::write(&path, json)?;
        Ok(())
    }

    pub fn load(&self, id: &str) -> Result<EvalRun> {
        let filename = if id.ends_with(".json") {
            id.to_string()
        } else {
            format!("{}.json", id)
        };
        let path = self.base_path.join(&filename);
        if !path.exists() {
            return Err(TrajectoryError::NotFound(format!(
                "Eval run not found: {}",
                id
            )));
        }
        let json = fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&json)?)
    }

    /// List all eval runs, newest first.
    pub fn list_all(&self) -> Result<Vec<EvalRun>> {
        if !self.base_path.exists() {
            return Ok(Vec::new());
        }
        let mut runs = Vec::new();
        for entry in fs::read_dir(&self.base_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            if let Ok(json) = fs::read_to_string(&path) {
                if let Ok(run) = serde_json::from_str::<EvalRun>(&json) {
                    runs.push(run);
                }
            }
        }
        runs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        Ok(runs)
    }

    /// Find the most recent eval run for a given eval directory.
    pub fn find_latest_for_dir(&self, eval_dir: &str) -> Result<Option<EvalRun>> {
        let canonical = canonicalize_eval_dir(eval_dir);
        let mut runs = self.list_all()?;
        runs.retain(|r| canonicalize_eval_dir(&r.eval_dir) == canonical);
        Ok(runs.into_iter().next())
    }

    /// Check whether a string looks like an eval run ID (YYYYMMDD-HHMMSS-xxxxxxxx).
    pub fn looks_like_id(s: &str) -> bool {
        // "20260312-150634-abc123ef" → len 24, two hyphens splitting timestamp + suffix
        let parts: Vec<&str> = s.splitn(3, '-').collect();
        parts.len() == 3
            && parts[0].len() == 8
            && parts[1].len() == 6
            && parts[0].chars().all(|c| c.is_ascii_digit())
            && parts[1].chars().all(|c| c.is_ascii_digit())
    }
}

/// Normalize an eval dir path for comparison (strip trailing slashes, strip ./ prefix).
fn canonicalize_eval_dir(s: &str) -> String {
    let s = s.trim_end_matches('/');
    let s = s.strip_prefix("./").unwrap_or(s);
    // Also try to resolve to an absolute path for robust matching
    if let Ok(abs) = Path::new(s).canonicalize() {
        return abs.to_string_lossy().to_string();
    }
    s.to_string()
}
