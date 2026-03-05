use crate::error::{ProviderError, Result};
use crate::messages::{MessagesRequest, MessagesResponse};

const API_BASE_URL: &str = "https://api.anthropic.com/v1";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// HTTP client for Anthropic API
pub struct AnthropicClient {
    client: reqwest::Client,
    api_key: String,
}

impl AnthropicClient {
    /// Create a new client with API key from environment
    pub fn from_env() -> Result<Self> {
        let api_key =
            std::env::var("ANTHROPIC_API_KEY").map_err(|_| ProviderError::MissingApiKey)?;

        Self::new(api_key)
    }

    /// Create a new client with explicit API key
    pub fn new(api_key: String) -> Result<Self> {
        let client = reqwest::Client::new();

        Ok(Self { client, api_key })
    }

    /// Send a messages request
    pub async fn messages(&self, request: MessagesRequest) -> Result<MessagesResponse> {
        let url = format!("{}/messages", API_BASE_URL);

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await?;

        let status = response.status();

        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(ProviderError::Api {
                status: status.as_u16(),
                message: error_body,
            });
        }

        let response_body = response.json::<MessagesResponse>().await?;

        Ok(response_body)
    }
}
