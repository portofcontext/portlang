use serde::{Deserialize, Serialize};

/// Verifier configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verifier {
    /// Name of the verifier
    pub name: String,

    /// Shell command to run for verification
    pub command: String,

    /// When to trigger this verifier
    #[serde(default)]
    pub trigger: VerifierTrigger,

    /// Human-readable description of what this verifier checks
    #[serde(default)]
    pub description: Option<String>,
}

/// When to trigger a verifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VerifierTrigger {
    /// Run after every action
    Always,
    /// Run only when agent stops
    OnStop,
    /// Run after specific tool calls
    OnWrite,
}

impl Default for VerifierTrigger {
    fn default() -> Self {
        VerifierTrigger::OnStop
    }
}

/// Result of running a verifier
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifierResult {
    /// Name of the verifier that ran
    pub name: String,

    /// Whether the verifier passed (exit code 0)
    pub passed: bool,

    /// Standard output from the verifier
    pub stdout: String,

    /// Standard error from the verifier
    pub stderr: String,

    /// Exit code
    pub exit_code: i32,
}

impl VerifierResult {
    pub fn new(name: String, passed: bool, stdout: String, stderr: String, exit_code: i32) -> Self {
        Self {
            name,
            passed,
            stdout,
            stderr,
            exit_code,
        }
    }
}
