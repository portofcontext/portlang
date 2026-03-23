use super::traits::CommandOutput;
use std::path::Path;
use std::process::Command;

/// Abstracts over container runtimes that share a Docker-compatible CLI interface.
///
/// Implementations wrap a CLI binary (e.g. `container`, `docker`) and translate
/// the four lifecycle operations portlang needs into the right invocations.
/// Adding a new backend means implementing this trait — the rest of the sandbox
/// machinery in [`ContainerSandbox`] stays unchanged.
pub trait ContainerBackend: Send + Sync {
    /// Short name used in logs and trajectory metadata (e.g. "apple-container", "docker").
    fn name(&self) -> &str;

    /// The CLI binary used to exec into containers (e.g. "container", "podman", "docker").
    fn cli(&self) -> &str;

    /// Build an image from a Dockerfile.
    fn build(&self, dockerfile_path: &str, tag: &str, context: &str) -> Result<(), String>;

    /// Start a detached container with the workspace bind-mounted at `/workspace`.
    /// Returns the container identifier to use for subsequent `exec` and `stop` calls.
    fn run(
        &self,
        container_name: &str,
        image: &str,
        host_workspace: &Path,
    ) -> Result<String, String>;

    /// Execute a shell command inside a running container.
    fn exec(&self, container_id: &str, cmd: &str) -> Result<CommandOutput, String>;

    /// Stop a running container. Failures are silently ignored (best-effort cleanup).
    fn stop(&self, container_id: &str);
}

// ---------------------------------------------------------------------------
// Apple Container backend
// ---------------------------------------------------------------------------

/// Container backend using Apple's native `container` CLI (macOS only).
pub struct AppleContainerBackend;

impl AppleContainerBackend {
    /// Returns `true` if the `container` binary is present and responsive.
    pub fn is_available() -> bool {
        Command::new("container")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

impl ContainerBackend for AppleContainerBackend {
    fn name(&self) -> &str {
        "apple-container"
    }

    fn cli(&self) -> &str {
        "container"
    }

    fn build(&self, dockerfile_path: &str, tag: &str, context: &str) -> Result<(), String> {
        let output = Command::new("container")
            .args(["build", "-f", dockerfile_path, "-t", tag, context])
            .output()
            .map_err(|e| format!("container build failed: {e}"))?;

        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).into_owned())
        }
    }

    fn run(
        &self,
        container_name: &str,
        image: &str,
        host_workspace: &Path,
    ) -> Result<String, String> {
        let volume = format!("{}:/workspace", host_workspace.display());
        let output = Command::new("container")
            .args([
                "run",
                "-d",
                "--name",
                container_name,
                "--workdir",
                "/workspace",
                "--volume",
                &volume,
                image,
                "sleep",
                "infinity",
            ])
            .output()
            .map_err(|e| format!("container run failed: {e}"))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).into_owned())
        }
    }

    fn exec(&self, container_id: &str, cmd: &str) -> Result<CommandOutput, String> {
        let output = Command::new("container")
            .args(["exec", container_id, "sh", "-c", cmd])
            .output()
            .map_err(|e| format!("container exec failed: {e}"))?;

        Ok(CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            exit_code: output.status.code().unwrap_or(-1),
            success: output.status.success(),
        })
    }

    fn stop(&self, container_id: &str) {
        let _ = Command::new("container")
            .args(["stop", container_id])
            .output();
    }
}

// ---------------------------------------------------------------------------
// Podman backend
// ---------------------------------------------------------------------------

/// Container backend using the `podman` CLI (Docker-compatible interface).
pub struct PodmanBackend;

impl PodmanBackend {
    /// Returns `true` if the `podman` binary is present and responsive.
    pub fn is_available() -> bool {
        Command::new("podman")
            .arg("info")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

impl ContainerBackend for PodmanBackend {
    fn name(&self) -> &str {
        "podman"
    }

    fn cli(&self) -> &str {
        "podman"
    }

    fn build(&self, dockerfile_path: &str, tag: &str, context: &str) -> Result<(), String> {
        let output = Command::new("podman")
            .args(["build", "-f", dockerfile_path, "-t", tag, context])
            .output()
            .map_err(|e| format!("podman build failed: {e}"))?;

        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).into_owned())
        }
    }

    fn run(
        &self,
        container_name: &str,
        image: &str,
        host_workspace: &Path,
    ) -> Result<String, String> {
        let volume = format!("{}:/workspace", host_workspace.display());
        let output = Command::new("podman")
            .args([
                "run",
                "-d",
                "--name",
                container_name,
                "--workdir",
                "/workspace",
                "--volume",
                &volume,
                image,
                "sleep",
                "infinity",
            ])
            .output()
            .map_err(|e| format!("podman run failed: {e}"))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).into_owned())
        }
    }

    fn exec(&self, container_id: &str, cmd: &str) -> Result<CommandOutput, String> {
        let output = Command::new("podman")
            .args(["exec", container_id, "sh", "-c", cmd])
            .output()
            .map_err(|e| format!("podman exec failed: {e}"))?;

        Ok(CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            exit_code: output.status.code().unwrap_or(-1),
            success: output.status.success(),
        })
    }

    fn stop(&self, container_id: &str) {
        let _ = Command::new("podman").args(["stop", container_id]).output();
    }
}

// ---------------------------------------------------------------------------
// Docker backend
// ---------------------------------------------------------------------------

/// Container backend using the standard `docker` CLI.
pub struct DockerBackend;

impl DockerBackend {
    /// Returns `true` if the Docker daemon is reachable (`docker info` succeeds).
    pub fn is_available() -> bool {
        Command::new("docker")
            .arg("info")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

impl ContainerBackend for DockerBackend {
    fn name(&self) -> &str {
        "docker"
    }

    fn cli(&self) -> &str {
        "docker"
    }

    fn build(&self, dockerfile_path: &str, tag: &str, context: &str) -> Result<(), String> {
        let output = Command::new("docker")
            .args(["build", "-f", dockerfile_path, "-t", tag, context])
            .output()
            .map_err(|e| format!("docker build failed: {e}"))?;

        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).into_owned())
        }
    }

    fn run(
        &self,
        container_name: &str,
        image: &str,
        host_workspace: &Path,
    ) -> Result<String, String> {
        let volume = format!("{}:/workspace", host_workspace.display());
        let output = Command::new("docker")
            .args([
                "run",
                "-d",
                "--name",
                container_name,
                "--workdir",
                "/workspace",
                "--volume",
                &volume,
                image,
                "sleep",
                "infinity",
            ])
            .output()
            .map_err(|e| format!("docker run failed: {e}"))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).into_owned())
        }
    }

    fn exec(&self, container_id: &str, cmd: &str) -> Result<CommandOutput, String> {
        let output = Command::new("docker")
            .args(["exec", container_id, "sh", "-c", cmd])
            .output()
            .map_err(|e| format!("docker exec failed: {e}"))?;

        Ok(CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            exit_code: output.status.code().unwrap_or(-1),
            success: output.status.success(),
        })
    }

    fn stop(&self, container_id: &str) {
        let _ = Command::new("docker").args(["stop", container_id]).output();
    }
}
