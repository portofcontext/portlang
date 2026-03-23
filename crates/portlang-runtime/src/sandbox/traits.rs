use super::error::{BoundaryViolation, Result};
use async_trait::async_trait;
use portlang_core::Action;
use tokio::io::AsyncRead;

/// Output from running a shell command
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub success: bool,
}

/// Handle returned by [`Sandbox::exec_script_streaming`].
///
/// Implementors wrap whatever process or remote call drives the script so that
/// the runner can kill or wait for it without knowing the backend details.
#[async_trait]
pub trait ScriptExecHandle: Send {
    /// Kill the running script.
    async fn kill(&mut self) -> std::io::Result<()>;
    /// Wait for the script to finish and return its exit code.
    async fn wait(&mut self) -> std::io::Result<Option<i32>>;
}

/// Standard process-backed [`ScriptExecHandle`] wrapping a `tokio::process::Child`.
///
/// Used by local container backends (Apple Container, Podman, Docker) and
/// `DispatchSandbox`. Remote backends return their own implementations.
pub struct ChildHandle(pub tokio::process::Child);

#[async_trait]
impl ScriptExecHandle for ChildHandle {
    async fn kill(&mut self) -> std::io::Result<()> {
        self.0.kill().await
    }
    async fn wait(&mut self) -> std::io::Result<Option<i32>> {
        self.0.wait().await.map(|s| s.code())
    }
}

/// Streams and control handle returned by [`Sandbox::exec_script_streaming`].
pub struct ScriptHandle {
    /// Live stdout from the running script (JSONL from claude).
    pub stdout: Box<dyn AsyncRead + Unpin + Send>,
    /// Live stderr from the running script.
    pub stderr: Box<dyn AsyncRead + Unpin + Send>,
    /// Backend-specific kill/wait handle.
    pub exec: Box<dyn ScriptExecHandle>,
}

/// Sandbox trait for executing actions with boundary enforcement
#[async_trait]
pub trait Sandbox: Send + Sync {
    /// Name of the container backend in use (e.g. "apple-container", "docker").
    fn backend_name(&self) -> &str;

    /// Dispatch an action for execution
    async fn dispatch(&self, action: &Action) -> Result<String>;

    /// Check if an action violates the boundary
    async fn check_boundary(&self, action: &Action) -> std::result::Result<(), BoundaryViolation>;

    /// Run a shell command (for verifiers and re-observation)
    async fn run_command(&self, cmd: &str) -> Result<CommandOutput>;

    /// Get the sandbox root directory
    fn root(&self) -> &std::path::Path;

    /// Get the container ID (for running external processes inside the container)
    fn container_id(&self) -> Option<&str>;

    /// Stage `script_content` into the sandbox and execute it, returning live
    /// stdout/stderr streams and a handle for kill/wait.
    ///
    /// The sandbox owns script staging: local backends write to the host
    /// workspace (visible via bind-mount), remote backends inject directly into
    /// the container.  The runner never touches the host filesystem for this.
    async fn exec_script_streaming(&self, script_content: &str) -> Result<ScriptHandle>;
}
