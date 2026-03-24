use super::container_backend::ContainerBackend;
use super::error::{BoundaryViolation, Result, SandboxError};
use super::traits::{CommandOutput, Sandbox, ScriptHandle};
use crate::tools::ToolRegistry;
use async_trait::async_trait;
use portlang_core::{Action, Boundary, Environment};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

/// Sandbox implementation backed by any [`ContainerBackend`].
///
/// All image-building logic (package installation, Dockerfile caching, etc.)
/// lives here. The backend is only responsible for the four primitive CLI
/// operations: build, run, exec, stop.
pub struct ContainerSandbox {
    container_id: String,
    host_workspace: PathBuf,
    boundary: Boundary,
    registry: Arc<ToolRegistry>,
    backend: Box<dyn ContainerBackend>,
}

impl ContainerSandbox {
    pub async fn new(
        host_root: PathBuf,
        boundary: Boundary,
        registry: Arc<ToolRegistry>,
        environment: &Environment,
        backend: Box<dyn ContainerBackend>,
    ) -> Result<Self> {
        if !host_root.exists() {
            std::fs::create_dir_all(&host_root)
                .map_err(|e| SandboxError::InitError(format!("Create workspace failed: {e}")))?;
        }

        let container_name = format!("portlang-{}", Uuid::new_v4());
        let image = Self::determine_image(backend.as_ref(), environment, &container_name).await?;

        let container_id = backend
            .run(&container_name, &image, &host_root)
            .await
            .map_err(|e| SandboxError::InitError(format!("Failed to start container: {e}")))?;

        tracing::info!(
            "Started {} container {} ({})",
            backend.name(),
            container_name,
            container_id
        );

        Ok(Self {
            container_id,
            host_workspace: host_root,
            boundary,
            registry,
            backend,
        })
    }

    /// The name of the container backend in use (e.g. "apple-container", "docker").
    pub fn backend_name(&self) -> &str {
        self.backend.name()
    }

    // -----------------------------------------------------------------------
    // Image resolution
    // -----------------------------------------------------------------------

    /// Resolve the image to use, in priority order:
    /// custom image > dockerfile > packages > default debian slim.
    async fn determine_image(
        backend: &dyn ContainerBackend,
        environment: &Environment,
        container_name: &str,
    ) -> Result<String> {
        if let Some(ref image) = environment.image {
            tracing::info!("Using custom image: {}", image);
            return Ok(image.clone());
        }

        if let Some(ref dockerfile_path) = environment.dockerfile {
            if environment.packages.is_empty() {
                return Self::build_from_dockerfile(backend, dockerfile_path, container_name).await;
            } else {
                return Self::build_from_dockerfile_with_extras(
                    backend,
                    dockerfile_path,
                    &environment.packages,
                    container_name,
                )
                .await;
            }
        }

        if !environment.packages.is_empty() {
            tracing::info!("Building image with packages: {:?}", environment.packages);
            return Self::build_with_packages(backend, &environment.packages, container_name).await;
        }

        tracing::info!("Using default debian:bookworm-slim image");
        Ok("debian:bookworm-slim".to_string())
    }

    /// Build an image from a Dockerfile. Uses a content-hash tag so each unique
    /// Dockerfile is only built once across all runs.
    async fn build_from_dockerfile(
        backend: &dyn ContainerBackend,
        dockerfile_path: &str,
        _tag: &str,
    ) -> Result<String> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let content = std::fs::read_to_string(dockerfile_path).map_err(|e| {
            SandboxError::InitError(format!(
                "Failed to read Dockerfile '{}': {}",
                dockerfile_path, e
            ))
        })?;

        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        let image_tag = format!("portlang-dockerfile-{:016x}", hasher.finish());

        let marker = std::env::temp_dir().join(format!("{}.built", image_tag));
        if marker.exists() {
            tracing::info!("Reusing cached image: {}", image_tag);
            return Ok(image_tag);
        }

        backend
            .build(&content, &image_tag)
            .await
            .map_err(|e| SandboxError::InitError(format!("Dockerfile build failed: {e}")))?;

        let _ = std::fs::write(&marker, "");
        Ok(image_tag)
    }

    /// Build a composite image: user's Dockerfile as the base, with extra packages
    /// layered on top. Used by the claude-code runner when a custom Dockerfile is
    /// provided alongside packages like `claude-code` or `uv`.
    async fn build_from_dockerfile_with_extras(
        backend: &dyn ContainerBackend,
        dockerfile_path: &str,
        packages: &[String],
        container_name: &str,
    ) -> Result<String> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let base_image =
            Self::build_from_dockerfile(backend, dockerfile_path, container_name).await?;

        let mut hasher = DefaultHasher::new();
        base_image.hash(&mut hasher);
        packages.hash(&mut hasher);
        let composite_tag = format!("portlang-composite-{:016x}", hasher.finish());

        let marker = std::env::temp_dir().join(format!("{}.built", composite_tag));
        if marker.exists() {
            tracing::info!("Reusing cached composite image: {}", composite_tag);
            return Ok(composite_tag);
        }

        let dockerfile_content = Self::build_dockerfile_content(&base_image, packages);

        tracing::info!(
            "Building composite image (Dockerfile + packages {:?}): {}",
            packages,
            composite_tag
        );
        backend
            .build(&dockerfile_content, &composite_tag)
            .await
            .map_err(|e| SandboxError::InitError(format!("Composite image build failed: {e}")))?;

        let _ = std::fs::write(&marker, "");
        Ok(composite_tag)
    }

    /// Build an image from the default debian base with the requested packages installed.
    async fn build_with_packages(
        backend: &dyn ContainerBackend,
        packages: &[String],
        tag: &str,
    ) -> Result<String> {
        let dockerfile_content = Self::build_dockerfile_content("debian:bookworm-slim", packages);

        backend
            .build(&dockerfile_content, tag)
            .await
            .map_err(|e| SandboxError::InitError(format!("Image build failed: {e}")))?;

        Ok(tag.to_string())
    }

    /// Generate a Dockerfile that starts FROM `base_image` and layers on `packages`.
    /// `uv` and `claude-code` are installed via their own standalone installers;
    /// everything else goes through apt.
    fn build_dockerfile_content(base_image: &str, packages: &[String]) -> String {
        let has_uv = packages.iter().any(|p| p == "uv");
        let has_claude_code = packages.iter().any(|p| p == "claude-code");

        let mut apt_packages: Vec<&str> = packages
            .iter()
            .filter(|p| p.as_str() != "uv" && p.as_str() != "claude-code")
            .map(|s| s.as_str())
            .collect();

        if has_uv || has_claude_code {
            if !apt_packages.contains(&"curl") {
                apt_packages.push("curl");
            }
            if !apt_packages.contains(&"ca-certificates") {
                apt_packages.push("ca-certificates");
            }
        }

        let mut lines = vec![format!("FROM {}", base_image)];

        if !apt_packages.is_empty() {
            lines.push(format!(
                "RUN apt-get update && apt-get install -y {} && rm -rf /var/lib/apt/lists/*",
                apt_packages.join(" ")
            ));
        }

        // uv bundles its own Python runtime — no python3 apt package required.
        if has_uv {
            lines.push(
                "RUN curl -LsSf https://astral.sh/uv/install.sh | env HOME=/root sh".to_string(),
            );
            lines.push(r#"ENV PATH="/root/.local/bin:$PATH""#.to_string());
        }

        if has_claude_code {
            lines.push("RUN curl -fsSL https://claude.ai/install.sh | bash".to_string());
            lines.push(r#"ENV PATH="/root/.local/bin:$PATH""#.to_string());
        }

        lines.join("\n") + "\n"
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn normalize_path(path: &str) -> &str {
        path.strip_prefix("/workspace/")
            .or_else(|| path.strip_prefix("workspace/"))
            .unwrap_or(path)
    }

    fn is_write_allowed(&self, path: &str) -> bool {
        if self.boundary.allow_write.is_empty() {
            return false;
        }
        let normalized = Self::normalize_path(path);
        self.boundary.allow_write.iter().any(|pattern| {
            glob::Pattern::new(pattern)
                .map(|p| p.matches(normalized) || p.matches(path))
                .unwrap_or(false)
        })
    }

    async fn exec(&self, cmd: &str) -> Result<CommandOutput> {
        self.backend
            .exec(&self.container_id, cmd)
            .await
            .map_err(|e| SandboxError::CommandError(format!("Container exec failed: {e}")))
    }
}

#[async_trait]
impl Sandbox for ContainerSandbox {
    fn backend_name(&self) -> &str {
        self.backend.name()
    }

    async fn dispatch(&self, action: &Action) -> Result<String> {
        match action {
            Action::ToolCall { tool, input } => match tool.as_str() {
                "read" => {
                    let path = input
                        .get("path")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| SandboxError::ToolError("Missing path".into()))?;
                    let output = self
                        .exec(&format!("cat {}", shell_escape::escape(path.into())))
                        .await?;
                    if output.success {
                        Ok(output.stdout)
                    } else {
                        Err(SandboxError::ToolError(format!(
                            "Read failed: {}",
                            output.stderr
                        )))
                    }
                }

                "write" => {
                    let path = input
                        .get("path")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| SandboxError::ToolError("Missing path".into()))?;
                    let content = input
                        .get("content")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| SandboxError::ToolError("Missing content".into()))?;

                    if let Some(parent) = Path::new(path).parent() {
                        if parent != Path::new("") {
                            self.exec(&format!(
                                "mkdir -p {}",
                                shell_escape::escape(parent.to_str().unwrap().into())
                            ))
                            .await?;
                        }
                    }

                    let cmd = format!(
                        "cat > {} << 'PORTLANG_EOF'\n{}\nPORTLANG_EOF",
                        shell_escape::escape(path.into()),
                        content
                    );
                    let output = self.exec(&cmd).await?;
                    if output.success {
                        Ok(format!("Wrote {} bytes to {}", content.len(), path))
                    } else {
                        Err(SandboxError::ToolError(format!(
                            "Write failed: {}",
                            output.stderr
                        )))
                    }
                }

                "glob" => {
                    let pattern = input
                        .get("pattern")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| SandboxError::ToolError("Missing pattern".into()))?;
                    let output = self
                        .exec(&format!(
                            "find . -path '{}' -type f 2>/dev/null | sort",
                            pattern
                        ))
                        .await?;
                    let files: Vec<String> = output
                        .stdout
                        .lines()
                        .map(|s| s.trim_start_matches("./").to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    Ok(serde_json::to_string_pretty(&files)?)
                }

                "bash" => {
                    let command =
                        input
                            .get("command")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| {
                                SandboxError::ToolError("Missing 'command' parameter".into())
                            })?;

                    self.exec("touch /tmp/portlang_bash_marker").await?;
                    let output = self.exec(command).await?;
                    let find_output = self
                        .exec("find /workspace -newer /tmp/portlang_bash_marker -type f | sort")
                        .await?;

                    let mut violations: Vec<String> = vec![];
                    for file in find_output.stdout.lines().filter(|l| !l.is_empty()) {
                        let rel_path = file.strip_prefix("/workspace/").unwrap_or(file);
                        if !self.is_write_allowed(rel_path) {
                            let _ = self
                                .exec(&format!("rm -f {}", shell_escape::escape(file.into())))
                                .await;
                            violations.push(rel_path.to_string());
                        }
                    }
                    let _ = self.exec("rm -f /tmp/portlang_bash_marker").await;

                    let mut result = output.stdout.clone();
                    if !output.stderr.is_empty() {
                        if !result.is_empty() {
                            result.push('\n');
                        }
                        result.push_str("stderr: ");
                        result.push_str(&output.stderr);
                    }
                    if output.exit_code != 0 {
                        result.push_str(&format!("\nExit code: {}", output.exit_code));
                    }
                    if !violations.is_empty() {
                        result.push_str("\n\nBoundary violations — the following files were removed (not in allow_write):\n");
                        for v in &violations {
                            result.push_str(&format!("  - {}\n", v));
                        }
                    }
                    if result.trim().is_empty() {
                        result = "(no output)".to_string();
                    }
                    Ok(result)
                }

                _ => {
                    self.registry
                        .execute(tool.as_str(), &self.host_workspace, input.clone())
                        .await
                }
            },
            Action::TextOutput { text } => Ok(format!("Agent output: {}", text)),
            Action::Stop => Ok("Agent stopped".to_string()),
        }
    }

    async fn check_boundary(&self, action: &Action) -> std::result::Result<(), BoundaryViolation> {
        if let Action::ToolCall { tool, input } = action {
            if tool.as_str() == "write" {
                let path = input
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| BoundaryViolation::new("Missing path"))?;

                if !self.is_write_allowed(path) {
                    return Err(BoundaryViolation::write_not_allowed(
                        path.to_string(),
                        self.boundary.allow_write.clone(),
                        None,
                    ));
                }
            }
        }
        Ok(())
    }

    async fn run_command(&self, cmd: &str) -> Result<CommandOutput> {
        self.exec(cmd).await
    }

    fn root(&self) -> &Path {
        Path::new("/workspace")
    }

    fn container_id(&self) -> Option<&str> {
        Some(&self.container_id)
    }

    async fn exec_script_streaming(&self, script_content: &str) -> Result<ScriptHandle> {
        self.backend
            .exec_streaming(&self.container_id, script_content, &self.host_workspace)
            .await
            .map_err(|e| SandboxError::CommandError(e))
    }
}

impl Drop for ContainerSandbox {
    fn drop(&mut self) {
        self.backend.stop(&self.container_id);
        tracing::info!(
            "Stopped {} container {}",
            self.backend.name(),
            self.container_id
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::traits::CommandOutput;
    use super::*;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};

    struct MockBackend {
        calls: Arc<Mutex<Vec<(String, String)>>>,
    }

    impl MockBackend {
        fn new() -> (Self, Arc<Mutex<Vec<(String, String)>>>) {
            let calls = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    calls: calls.clone(),
                },
                calls,
            )
        }
    }

    #[async_trait]
    impl ContainerBackend for MockBackend {
        fn name(&self) -> &str {
            "mock"
        }
        fn cli(&self) -> &str {
            ""
        }
        async fn build(
            &self,
            dockerfile_content: &str,
            tag: &str,
        ) -> std::result::Result<(), String> {
            self.calls
                .lock()
                .unwrap()
                .push((dockerfile_content.to_string(), tag.to_string()));
            Ok(())
        }
        async fn run(
            &self,
            _name: &str,
            _image: &str,
            _workspace: &std::path::Path,
        ) -> std::result::Result<String, String> {
            Ok("mock-container-id".to_string())
        }
        async fn exec(&self, _id: &str, _cmd: &str) -> std::result::Result<CommandOutput, String> {
            Ok(CommandOutput {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
                success: true,
            })
        }
        fn stop(&self, _id: &str) {}
    }

    /// `build_from_dockerfile` must read the Dockerfile from disk and pass its
    /// *content* to `backend.build()`, not the file path.
    #[tokio::test]
    async fn build_from_dockerfile_passes_content_not_path() {
        // Use a nanosecond timestamp in the content so the hash is unique per
        // test run and we never accidentally hit a stale marker file.
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let expected_content = format!("FROM debian:bookworm-slim\n# test-run={unique}\n");

        let dir = tempfile::tempdir().unwrap();
        let dockerfile_path = dir.path().join("Dockerfile");
        std::fs::write(&dockerfile_path, &expected_content).unwrap();

        let (mock, calls) = MockBackend::new();
        ContainerSandbox::build_from_dockerfile(
            &mock,
            dockerfile_path.to_str().unwrap(),
            "ignored-tag",
        )
        .await
        .expect("build_from_dockerfile should succeed");

        let calls = calls.lock().unwrap();
        assert_eq!(
            calls.len(),
            1,
            "backend.build should be called exactly once"
        );

        let (received_content, _tag) = &calls[0];
        assert_eq!(
            received_content, &expected_content,
            "backend received file content, not the path"
        );
        assert!(
            !received_content.contains(dockerfile_path.to_str().unwrap()),
            "content must not contain the file path"
        );
    }
}
