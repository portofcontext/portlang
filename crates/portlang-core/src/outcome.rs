use serde::{Deserialize, Serialize};

/// Final outcome of a field run
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunOutcome {
    /// Agent stopped successfully (convergence)
    Converged {
        /// Final message from the agent
        message: String,
    },

    /// Token budget was exhausted
    BudgetExhausted {
        /// Reason for budget exhaustion
        reason: String,
    },

    /// Cost limit was exceeded
    CostLimitExceeded {
        /// Reason for cost limit being exceeded
        reason: String,
    },

    /// Boundary violation occurred
    BoundaryViolation {
        /// Description of the violation
        violation: String,
    },

    /// Verifier failed and blocked convergence
    VerifierFailed {
        /// Name of the failed verifier
        verifier: String,
        /// Error message
        message: String,
    },

    /// Error occurred during execution
    Error {
        /// Error message
        message: String,
    },
}

impl RunOutcome {
    pub fn is_success(&self) -> bool {
        matches!(self, RunOutcome::Converged { .. })
    }

    pub fn description(&self) -> String {
        match self {
            RunOutcome::Converged { message } => {
                format!("Converged: {}", message)
            }
            RunOutcome::BudgetExhausted { reason } => {
                format!("Budget exhausted: {}", reason)
            }
            RunOutcome::CostLimitExceeded { reason } => {
                format!("Cost limit exceeded: {}", reason)
            }
            RunOutcome::BoundaryViolation { violation } => {
                format!("Boundary violation: {}", violation)
            }
            RunOutcome::VerifierFailed { verifier, message } => {
                format!("Verifier '{}' failed: {}", verifier, message)
            }
            RunOutcome::Error { message } => {
                format!("Error: {}", message)
            }
        }
    }

    /// Check if this outcome is the same type as another (ignoring field values)
    pub fn matches_type(&self, other: &RunOutcome) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}
