use super::error::{BoundaryViolation, Result, SandboxError};
use super::traits::{CommandOutput, Sandbox};
use crate::tools::ToolRegistry;
use async_trait::async_trait;
use portlang_core::{Action, Boundary};
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
    async fn determine_image(
        config: &portlang_core::ContainerConfig,
        container_name: &str,
    ) -> Result<String> {
        // Priority: custom image > dockerfile > packages > default

        if let Some(ref image) = config.image {
            // Use pre-built image
            tracing::info!("Using custom image: {}", image);
            return Ok(image.clone());
        }

        if let Some(ref dockerfile_path) = config.dockerfile {
            // Build from custom Dockerfile
            tracing::info!("Building image from Dockerfile: {}", dockerfile_path);
            return Self::build_from_dockerfile(dockerfile_path, container_name).await;
        }

        if !config.packages.is_empty() {
            // Build image with additional packages
            tracing::info!("Building image with packages: {:?}", config.packages);
            return Self::build_with_packages(&config.packages, container_name).await;
        }

        // Default: Build image with Python and Node.js (for MCP servers and Python tools)
        tracing::info!("Building default image with Python 3 and Node.js LTS");
        let default_packages = vec!["nodejs".to_string(), "npm".to_string()];
        Self::build_with_packages(&default_packages, container_name).await
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
        // Check if Node.js is being installed to also pre-install MCP packages
        let has_nodejs = packages
            .iter()
            .any(|p| p.contains("nodejs") || p.contains("node"));

        // Create a temporary Dockerfile
        let mut dockerfile_content = format!(
            r#"FROM python:3-slim
RUN apt-get update && apt-get install -y {} && rm -rf /var/lib/apt/lists/*
"#,
            packages.join(" ")
        );

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
        container_config: &portlang_core::ContainerConfig,
    ) -> Result<Self> {
        // Create workspace on host
        if !host_root.exists() {
            std::fs::create_dir_all(&host_root)
                .map_err(|e| SandboxError::InitError(format!("Create workspace failed: {}", e)))?;
        }

        // Generate unique container name
        let container_name = format!("portlang-{}", Uuid::new_v4());

        // Determine which image to use
        let image = Self::determine_image(container_config, &container_name).await?;

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

    fn is_write_allowed(&self, path: &str) -> bool {
        if self.boundary.allow_write.is_empty() {
            return false;
        }
        for pattern in &self.boundary.allow_write {
            if let Ok(glob_pattern) = glob::Pattern::new(pattern) {
                if glob_pattern.matches(path) {
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

                    // For all other tools, delegate to the registry
                    // This includes: custom tools, code_mode, MCP tools, etc.
                    _ => {
                        self.registry
                            .execute(tool.as_str(), &self.host_workspace, input.clone())
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
                    return Err(BoundaryViolation::new(format!(
                        "Write to '{}' not allowed. Allowed patterns: {:?}",
                        path, self.boundary.allow_write
                    )));
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
