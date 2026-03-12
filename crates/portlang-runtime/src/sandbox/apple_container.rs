use super::error::{BoundaryViolation, Result, SandboxError};
use super::traits::{CommandOutput, Sandbox};
use crate::tools::ToolRegistry;
use async_trait::async_trait;
use portlang_core::{Action, Boundary, Environment};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use uuid::Uuid;

pub struct AppleContainerSandbox {
    container_id: String,
    #[allow(dead_code)]
    host_workspace: PathBuf,
    boundary: Boundary,
    registry: Arc<ToolRegistry>,
}

impl AppleContainerSandbox {
    /// Determine which container image to use based on configuration
    async fn determine_image(environment: &Environment, container_name: &str) -> Result<String> {
        // Priority: custom image > dockerfile > packages > default

        if let Some(ref image) = environment.image {
            // Use pre-built image
            tracing::info!("Using custom image: {}", image);
            return Ok(image.clone());
        }

        if let Some(ref dockerfile_path) = environment.dockerfile {
            // Build from custom Dockerfile
            tracing::info!("Building image from Dockerfile: {}", dockerfile_path);
            return Self::build_from_dockerfile(dockerfile_path, container_name).await;
        }

        if !environment.packages.is_empty() {
            // Build image with required packages
            tracing::info!("Building image with packages: {:?}", environment.packages);
            return Self::build_with_packages(&environment.packages, container_name).await;
        }

        // Default: minimal debian image with nothing extra installed
        tracing::info!("Using default debian:bookworm-slim image");
        Ok("debian:bookworm-slim".to_string())
    }

    /// Build container image from custom Dockerfile
    async fn build_from_dockerfile(dockerfile_path: &str, tag: &str) -> Result<String> {
        let output = Command::new("container")
            .args(["build", "-f", dockerfile_path, "-t", tag, "."])
            .output()
            .map_err(|e| {
                SandboxError::InitError(format!("Failed to build from Dockerfile: {}", e))
            })?;

        if !output.status.success() {
            return Err(SandboxError::InitError(format!(
                "Dockerfile build failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(tag.to_string())
    }

    /// Build container image with additional packages
    async fn build_with_packages(packages: &[String], tag: &str) -> Result<String> {
        // Separate apt packages from pip packages (uv is not in apt)
        let apt_packages: Vec<&str> = packages
            .iter()
            .filter(|p| p.as_str() != "uv")
            .map(|s| s.as_str())
            .collect();
        let has_uv = packages.iter().any(|p| p == "uv");

        let mut dockerfile_lines = vec!["FROM debian:bookworm-slim".to_string()];

        if !apt_packages.is_empty() {
            dockerfile_lines.push(format!(
                "RUN apt-get update && apt-get install -y {} && rm -rf /var/lib/apt/lists/*",
                apt_packages.join(" ")
            ));
        }

        // uv is installed via pip (requires python3 to already be in apt_packages)
        if has_uv {
            dockerfile_lines
                .push("RUN python3 -m pip install uv --break-system-packages".to_string());
        }

        let dockerfile_content = dockerfile_lines.join("\n") + "\n";

        // Write to temp file
        let temp_dockerfile = std::env::temp_dir().join(format!("Dockerfile.{}", tag));
        std::fs::write(&temp_dockerfile, dockerfile_content).map_err(|e| {
            SandboxError::InitError(format!("Failed to write temp Dockerfile: {}", e))
        })?;

        // Build image
        let output = Command::new("container")
            .args([
                "build",
                "-f",
                temp_dockerfile.to_str().unwrap(),
                "-t",
                tag,
                std::env::temp_dir().to_str().unwrap(),
            ])
            .output()
            .map_err(|e| SandboxError::InitError(format!("Failed to build image: {}", e)))?;

        // Clean up temp file
        let _ = std::fs::remove_file(&temp_dockerfile);

        if !output.status.success() {
            return Err(SandboxError::InitError(format!(
                "Image build failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(tag.to_string())
    }

    pub async fn new(
        host_root: PathBuf,
        boundary: Boundary,
        registry: Arc<ToolRegistry>,
        environment: &Environment,
    ) -> Result<Self> {
        // Create workspace on host
        if !host_root.exists() {
            std::fs::create_dir_all(&host_root)
                .map_err(|e| SandboxError::InitError(format!("Create workspace failed: {}", e)))?;
        }

        // Generate unique container name
        let container_name = format!("portlang-{}", Uuid::new_v4());

        // Determine which image to use
        let image = Self::determine_image(environment, &container_name).await?;

        // Apple's container CLI (similar to docker but native)
        // Network configuration: Allow network access by default for package downloads
        // Only disable network if explicitly set to Deny in boundary
        let mut cmd = Command::new("container");
        cmd.args([
            "run",
            "-d",
            "--name",
            &container_name,
            "--workdir",
            "/workspace",
            "--volume",
            &format!("{}:/workspace", host_root.display()),
        ]);

        // Add network flag only if denying (use default network otherwise)
        // FOR NOW WE JUST ALLOW ALL NETWORK TRAFFIC IN THE CONTAINER
        // IF USERS REQUEST IT, CREATE A NEW NETWORK POLICY VARIABLE IN THE CONTAINER SECTION
        // if matches!(boundary.network, portlang_core::NetworkPolicy::Deny) {
        //     cmd.args(["--network", "none"]);
        // }

        cmd.args([&image, "sleep", "infinity"]);

        let output = cmd
            .output()
            .map_err(|e| SandboxError::InitError(format!("Container start failed: {}", e)))?;

        if !output.status.success() {
            return Err(SandboxError::InitError(format!(
                "Failed to start container: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        tracing::info!(
            "Started Apple container {} ({})",
            container_name,
            container_id
        );

        Ok(Self {
            container_id,
            host_workspace: host_root,
            boundary,
            registry,
        })
    }

    /// Get the tool registry
    pub fn registry(&self) -> &ToolRegistry {
        &self.registry
    }

    /// Get the container ID
    pub fn container_id(&self) -> &str {
        &self.container_id
    }

    /// Normalize a path by stripping /workspace/ prefix
    ///
    /// The agent sees "Working Directory: /workspace" and uses absolute paths like "/workspace/file.txt",
    /// but custom tools run on the host where files are at relative paths like "file.txt".
    fn normalize_path(path: &str) -> String {
        path.strip_prefix("/workspace/")
            .or_else(|| path.strip_prefix("workspace/"))
            .unwrap_or(path)
            .to_string()
    }

    /// Recursively normalize paths in a JSON value
    ///
    /// Traverses the JSON structure and normalizes any string values that look like paths
    /// (start with /workspace/ or workspace/)
    fn normalize_paths_in_value(value: &Value) -> Value {
        match value {
            Value::String(s) => {
                // Normalize if it looks like a workspace path
                if s.starts_with("/workspace/") || s.starts_with("workspace/") {
                    Value::String(Self::normalize_path(s))
                } else {
                    value.clone()
                }
            }
            Value::Object(map) => {
                let mut new_map = serde_json::Map::new();
                for (k, v) in map {
                    new_map.insert(k.clone(), Self::normalize_paths_in_value(v));
                }
                Value::Object(new_map)
            }
            Value::Array(arr) => {
                Value::Array(arr.iter().map(Self::normalize_paths_in_value).collect())
            }
            _ => value.clone(),
        }
    }

    fn is_write_allowed(&self, path: &str) -> bool {
        if self.boundary.allow_write.is_empty() {
            return false;
        }

        // Normalize path to match relative patterns
        let normalized_path = Self::normalize_path(path);

        for pattern in &self.boundary.allow_write {
            if let Ok(glob_pattern) = glob::Pattern::new(pattern) {
                // Try matching both the normalized path and original path
                if glob_pattern.matches(&normalized_path) || glob_pattern.matches(path) {
                    return true;
                }
            }
        }
        false
    }

    async fn exec(&self, cmd: &str) -> Result<CommandOutput> {
        let output = Command::new("container")
            .args(["exec", &self.container_id, "sh", "-c", cmd])
            .output()
            .map_err(|e| SandboxError::CommandError(format!("Container exec failed: {}", e)))?;

        Ok(CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
            success: output.status.success(),
        })
    }
}

#[async_trait]
impl Sandbox for AppleContainerSandbox {
    async fn dispatch(&self, action: &Action) -> Result<String> {
        match action {
            Action::ToolCall { tool, input } => {
                // Note: For built-in tools, we use container-specific implementations
                // Custom tools from registry could be supported here in the future
                match tool.as_str() {
                    "read" => {
                        let path = input
                            .get("path")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| SandboxError::ToolError("Missing path".into()))?;

                        let cmd = format!("cat {}", shell_escape::escape(path.into()));
                        let output = self.exec(&cmd).await?;

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

                        // Create parent directories
                        if let Some(parent) = Path::new(path).parent() {
                            if parent != Path::new("") {
                                let mkdir_cmd = format!(
                                    "mkdir -p {}",
                                    shell_escape::escape(parent.to_str().unwrap().into())
                                );
                                self.exec(&mkdir_cmd).await?;
                            }
                        }

                        // Write file using heredoc
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

                        let cmd = format!("find . -path '{}' -type f 2>/dev/null | sort", pattern);
                        let output = self.exec(&cmd).await?;

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

                        // Create a timestamp marker in the container
                        self.exec("touch /tmp/portlang_bash_marker").await?;

                        // Execute command inside the container
                        let output = self.exec(command).await?;

                        // Find files modified/created after the marker
                        let find_output = self
                            .exec("find /workspace -newer /tmp/portlang_bash_marker -type f | sort")
                            .await?;

                        // Enforce allow_write: remove violating files
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

                        // Cleanup marker
                        let _ = self.exec("rm -f /tmp/portlang_bash_marker").await;

                        // Format result
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

                    // For all other tools, delegate to the registry
                    // This includes: custom tools (Python, shell), code_mode, MCP tools, etc.
                    // These run on the host, so we need to normalize /workspace/ paths to relative paths
                    _ => {
                        let normalized_input = Self::normalize_paths_in_value(input);
                        self.registry
                            .execute(tool.as_str(), &self.host_workspace, normalized_input)
                            .await
                    }
                }
            }
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
                    // Return simple violation - context tracing happens in loop_runner
                    return Err(BoundaryViolation::write_not_allowed(
                        path.to_string(),
                        self.boundary.allow_write.clone(),
                        None, // Context trace added later by loop_runner
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
        Path::new("/workspace") // Container always sees /workspace
    }

    fn container_id(&self) -> Option<&str> {
        Some(&self.container_id)
    }
}

impl Drop for AppleContainerSandbox {
    fn drop(&mut self) {
        let _ = Command::new("container")
            .args(["stop", &self.container_id])
            .output();
        tracing::info!("Stopped Apple container {}", self.container_id);
    }
}
