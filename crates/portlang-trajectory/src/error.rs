use thiserror::Error;

#[derive(Debug, Error)]
pub enum TrajectoryError {
    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Trajectory not found
    #[error("Trajectory not found: {0}")]
    NotFound(String),

    /// Invalid trajectory ID
    #[error("Invalid trajectory ID: {0}")]
    InvalidId(String),

    /// Other error
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, TrajectoryError>;
