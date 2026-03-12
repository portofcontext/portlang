use portlang_provider_anthropic::{ProviderError, Result};
use serde::{Deserialize, Serialize};

const OPENROUTER_API_BASE: &str = "https://openrouter.ai/api/v1";

/// OpenRouter API client
pub struct OpenRouterClient {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl OpenRouterClient {
    /// Create a new client with API key from environment
    pub fn from_env() -> Result<Self> {
        let api_key =
            std::env::var("OPENROUTER_API_KEY").map_err(|_| ProviderError::MissingApiKey)?;

        let base_url =
            std::env::var("OPENROUTER_HOST").unwrap_or_else(|_| OPENROUTER_API_BASE.to_string());

        Self::new(api_key, base_url)
    }

    /// Create a new client with explicit API key and base URL
    pub fn new(api_key: String, base_url: String) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        Ok(Self {
            client,
            api_key,
            base_url,
        })
    }

    /// Send a chat completion request
    pub async fn chat_completion(&self, request: ChatRequest) -> Result<ChatResponse> {
        let url = format!("{}/chat/completions", self.base_url);

        tracing::debug!("Sending request to OpenRouter: model={}", request.model);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("HTTP-Referer", "https://github.com/portofcontext/portlang")
            .header("X-Title", "portlang")
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                tracing::error!("OpenRouter request failed: {}", e);
                ProviderError::Other(format!("HTTP request failed: {}", e))
            })?;

        let status = response.status();
        tracing::debug!("OpenRouter response status: {}", status);

        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            tracing::error!("OpenRouter API error: {}", error_body);
            return Err(ProviderError::Api {
                status: status.as_u16(),
                message: error_body,
            });
        }

        let response_body = response.json::<ChatResponse>().await.map_err(|e| {
            tracing::error!("Failed to parse OpenRouter response: {}", e);
            ProviderError::Other(format!("Failed to parse response: {}", e))
        })?;

        tracing::debug!("OpenRouter request completed successfully");
        Ok(response_body)
    }
}

/// OpenAI-compatible chat request format
#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

/// OpenAI-compatible chat response format
#[derive(Debug, Clone, Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Choice {
    pub message: ResponseMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResponseMessage {
    pub role: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}
