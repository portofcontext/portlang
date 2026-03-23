use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A file artifact collected from the workspace after a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectedArtifact {
    /// Path relative to the workspace root (e.g. `"report.md"`, `"results/summary.json"`)
    pub path: String,
    /// File size in bytes
    pub size: u64,
    /// File contents as a UTF-8 string, or `null` if the file is binary or over the size limit
    pub content: Option<String>,
}

/// Walk `workspace_root` and collect files matching any of the given glob patterns.
///
/// Patterns are interpreted relative to `workspace_root`. For example a pattern
/// `"results/*.json"` matches `{workspace_root}/results/foo.json`.
///
/// If `patterns` is empty, an empty Vec is returned.
pub fn collect_artifacts(
    workspace_root: &Path,
    patterns: &[String],
) -> Result<Vec<CollectedArtifact>> {
    if patterns.is_empty() {
        return Ok(vec![]);
    }

    // Resolve each glob pattern against the workspace root, collect unique paths
    let mut matched_paths: Vec<PathBuf> = Vec::new();
    for pattern in patterns {
        let abs_pattern = workspace_root.join(pattern);
        let pattern_str = abs_pattern.to_string_lossy();
        for entry in glob::glob(&pattern_str)
            .map_err(|e| anyhow::anyhow!("Invalid collect pattern {:?}: {}", pattern, e))?
        {
            match entry {
                Ok(path) if path.is_file() => {
                    if !matched_paths.contains(&path) {
                        matched_paths.push(path);
                    }
                }
                Ok(_) => {} // skip directories
                Err(e) => tracing::warn!("Glob error for pattern {:?}: {}", pattern, e),
            }
        }
    }

    matched_paths.sort();

    let mut artifacts = Vec::new();

    for abs_path in &matched_paths {
        let relative = abs_path
            .strip_prefix(workspace_root)
            .unwrap_or(abs_path)
            .to_string_lossy()
            .into_owned();

        let size = abs_path.metadata().map(|m| m.len()).unwrap_or(0);
        let content = std::fs::read_to_string(abs_path).ok(); // None for binary files

        artifacts.push(CollectedArtifact {
            path: relative,
            size,
            content,
        });
    }

    Ok(artifacts)
}

/// Copy collected artifacts into `output_dir`, preserving relative paths.
///
/// Creates `output_dir` and any subdirectories as needed. If `artifacts` is
/// empty, `output_dir` is still created (as an empty directory).
pub fn copy_artifacts_to_dir(
    artifacts: &[CollectedArtifact],
    workspace_root: &Path,
    output_dir: &Path,
) -> Result<()> {
    std::fs::create_dir_all(output_dir)?;

    for artifact in artifacts {
        let src = workspace_root.join(&artifact.path);
        let dst = output_dir.join(&artifact.path);

        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::copy(&src, &dst)
            .map_err(|e| anyhow::anyhow!("Failed to copy artifact {:?} → {:?}: {}", src, dst, e))?;
    }

    Ok(())
}

/// Resolve the effective collect patterns for a field.
///
/// Returns `boundary.collect` when set explicitly; falls back to `boundary.allow_write`
/// so existing fields that never declared `collect` still have their outputs delivered.
pub fn effective_collect_patterns(
    allow_write: &[String],
    collect: &Option<Vec<String>>,
) -> Vec<String> {
    match collect {
        Some(patterns) => patterns.clone(),
        None => allow_write.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_patterns_falls_back_to_allow_write() {
        let allow_write = vec!["report.md".to_string(), "*.json".to_string()];
        let result = effective_collect_patterns(&allow_write, &None);
        assert_eq!(result, allow_write);
    }

    #[test]
    fn effective_patterns_uses_explicit_collect() {
        let allow_write = vec!["*.md".to_string(), "scratch/*.tmp".to_string()];
        let collect = Some(vec!["*.md".to_string()]);
        let result = effective_collect_patterns(&allow_write, &collect);
        assert_eq!(result, vec!["*.md".to_string()]);
    }

    #[test]
    fn effective_patterns_empty_collect_collects_nothing() {
        let allow_write = vec!["*.md".to_string()];
        let collect = Some(vec![]);
        let result = effective_collect_patterns(&allow_write, &collect);
        assert!(result.is_empty());
    }
}
