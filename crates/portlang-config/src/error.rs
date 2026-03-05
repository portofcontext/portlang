use thiserror::Error;

/// Error types for field parsing
#[derive(Debug, Error)]
pub enum FieldParseError {
    /// TOML syntax error
    #[error("TOML parse error: {0}")]
    TomlSyntax(#[from] toml::de::Error),

    /// Invalid field format
    #[error("Invalid field: {0}")]
    InvalidField(String),

    /// Missing required field
    #[error("Missing required field: {0}")]
    MissingField(String),

    /// Invalid cost format
    #[error("Invalid cost format: {0}")]
    InvalidCost(String),

    /// Invalid glob pattern
    #[error("Invalid glob pattern '{pattern}': {error}")]
    InvalidGlob { pattern: String, error: String },

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Other error
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, FieldParseError>;
