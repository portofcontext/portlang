use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProviderError {
    /// HTTP request failed
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// API error response
    #[error("API error: {status} - {message}")]
    Api { status: u16, message: String },

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Invalid response format
    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    /// Missing API key
    #[error("Missing ANTHROPIC_API_KEY environment variable")]
    MissingApiKey,

    /// Other error
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, ProviderError>;
