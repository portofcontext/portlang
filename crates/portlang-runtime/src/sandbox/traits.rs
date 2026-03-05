use super::error::{BoundaryViolation, Result};
use async_trait::async_trait;
use portlang_core::Action;

/// Output from running a shell command
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub success: bool,
}

/// Sandbox trait for executing actions with boundary enforcement
#[async_trait]
pub trait Sandbox: Send + Sync {
    /// Dispatch an action for execution
    async fn dispatch(&self, action: &Action) -> Result<String>;

    /// Check if an action violates the boundary
    async fn check_boundary(&self, action: &Action) -> std::result::Result<(), BoundaryViolation>;

    /// Run a shell command (for verifiers and re-observation)
    async fn run_command(&self, cmd: &str) -> Result<CommandOutput>;

    /// Get the sandbox root directory
    fn root(&self) -> &std::path::Path;
}
