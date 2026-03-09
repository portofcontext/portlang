use crate::client::AnthropicClient;
use crate::error::{ProviderError, Result};
use crate::messages::*;
use async_trait::async_trait;
use portlang_core::{Action, Cost, ToolName};

/// Token usage breakdown
#[derive(Debug, Clone, Copy)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

impl TokenUsage {
    pub fn new(input_tokens: u64, output_tokens: u64) -> Self {
        Self {
            input_tokens,
            output_tokens,
            total_tokens: input_tokens + output_tokens,
        }
    }
}

/// Model provider trait for interacting with LLM APIs
#[async_trait]
pub trait ModelProvider: Send + Sync {
    /// Complete a prompt with the model, returning action and token usage
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[Tool],
        system: Option<&str>,
    ) -> Result<(Action, TokenUsage)>;

    /// Get the model name
    fn model_name(&self) -> &str;

    /// Get the cost per input token
    fn cost_per_input_token(&self) -> Cost;

    /// Get the cost per output token
    fn cost_per_output_token(&self) -> Cost;

    /// Calculate total cost for a completion
    fn calculate_cost(&self, usage: &TokenUsage) -> Cost {
        let input_cost = Cost::from_microdollars(
            self.cost_per_input_token().microdollars() * usage.input_tokens,
        );
        let output_cost = Cost::from_microdollars(
            self.cost_per_output_token().microdollars() * usage.output_tokens,
        );
        input_cost + output_cost
    }
}

/// Anthropic provider implementation
pub struct AnthropicProvider {
    client: AnthropicClient,
    model: String,
    temperature: Option<f32>,
    max_tokens: u32,
}

impl AnthropicProvider {
    /// Create a new Anthropic provider from environment
    pub fn from_env(model: &str) -> Result<Self> {
        let client = AnthropicClient::from_env()?;

        Ok(Self {
            client,
            model: map_model_name(model),
            temperature: Some(1.0),
            max_tokens: 4096,
        })
    }

    /// Set temperature
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Set max tokens
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }
}

#[async_trait]
impl ModelProvider for AnthropicProvider {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[Tool],
        system: Option<&str>,
    ) -> Result<(Action, TokenUsage)> {
        let request = MessagesRequest {
            model: self.model.clone(),
            max_tokens: self.max_tokens,
            messages: messages.to_vec(),
            system: system.map(|s| s.to_string()),
            temperature: self.temperature,
            tools: tools.to_vec(),
        };

        let response = self.client.messages(request).await?;

        // Convert response to Action
        let action = response_to_action(&response)?;

        // Create token usage breakdown
        let usage = TokenUsage::new(response.usage.input_tokens, response.usage.output_tokens);

        Ok((action, usage))
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn cost_per_input_token(&self) -> Cost {
        // Sonnet 4.5: $3 per 1M tokens = $0.000003 per token = 3 microdollars per token
        Cost::from_microdollars(3)
    }

    fn cost_per_output_token(&self) -> Cost {
        // Sonnet 4.5: $15 per 1M tokens = $0.000015 per token = 15 microdollars per token
        Cost::from_microdollars(15)
    }
}

/// Map model name to full Anthropic model ID
fn map_model_name(model: &str) -> String {
    match model {
        "claude-sonnet-4-5" => "claude-sonnet-4-5-20250929".to_string(),
        "claude-opus-4-5" => "claude-opus-4-5-20251101".to_string(),
        "claude-haiku-4" => "claude-haiku-4-20250514".to_string(),
        other => other.to_string(),
    }
}

/// Convert MessagesResponse to Action
fn response_to_action(response: &MessagesResponse) -> Result<Action> {
    // Check stop reason
    if let Some(stop_reason) = &response.stop_reason {
        if stop_reason == "end_turn" {
            // Extract text or tool use from content
            for block in &response.content {
                match block {
                    ContentBlock::Text { text } => {
                        if !text.trim().is_empty() {
                            return Ok(Action::text(text.clone()));
                        }
                    }
                    ContentBlock::ToolUse { name, input, .. } => {
                        let tool_name = parse_tool_name(name)?;
                        return Ok(Action::tool_call(tool_name, input.clone()));
                    }
                    _ => {}
                }
            }

            // If we get here, it's a stop without text or tool use
            return Ok(Action::stop());
        } else if stop_reason == "stop_sequence" || stop_reason == "max_tokens" {
            return Ok(Action::stop());
        }
    }

    // Default: extract first meaningful content
    for block in &response.content {
        match block {
            ContentBlock::Text { text } => {
                if !text.trim().is_empty() {
                    return Ok(Action::text(text.clone()));
                }
            }
            ContentBlock::ToolUse { name, input, .. } => {
                let tool_name = parse_tool_name(name)?;
                return Ok(Action::tool_call(tool_name, input.clone()));
            }
            _ => {}
        }
    }

    Err(ProviderError::InvalidResponse(
        "No valid content in response".to_string(),
    ))
}

/// Parse tool name from string - now accepts any tool name
fn parse_tool_name(name: &str) -> Result<ToolName> {
    Ok(ToolName::new(name))
}
