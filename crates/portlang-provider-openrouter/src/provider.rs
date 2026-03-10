use crate::client::{ChatRequest, OpenRouterClient};
use async_trait::async_trait;
use portlang_core::{Action, Cost, ToolName};
use portlang_provider_anthropic::{
    ContentBlock, Message, MessageContent, ModelProvider, TokenUsage, Tool,
};
use portlang_provider_anthropic::{ProviderError, Result};

/// OpenRouter provider implementation
pub struct OpenRouterProvider {
    client: OpenRouterClient,
    model: String,
    temperature: Option<f32>,
    max_tokens: u32,
}

impl OpenRouterProvider {
    /// Create a new OpenRouter provider from environment
    pub fn from_env(model: &str) -> Result<Self> {
        let client = OpenRouterClient::from_env()?;

        Ok(Self {
            client,
            model: model.to_string(),
            temperature: Some(0.5),
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

    /// Convert our Message format to OpenAI format
    fn convert_messages(messages: &[Message]) -> Vec<serde_json::Value> {
        let mut result = Vec::new();

        for msg in messages {
            match &msg.content {
                MessageContent::Text(text) => {
                    result.push(serde_json::json!({
                        "role": msg.role,
                        "content": text
                    }));
                }
                MessageContent::Blocks(blocks) => {
                    // Check if this is an assistant message with tool use
                    if msg.role == "assistant" {
                        let tool_uses: Vec<&ContentBlock> = blocks
                            .iter()
                            .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
                            .collect();

                        if !tool_uses.is_empty() {
                            // Assistant message with tool calls - convert to OpenAI format
                            let tool_calls: Vec<serde_json::Value> = tool_uses
                                .iter()
                                .filter_map(|block| match block {
                                    ContentBlock::ToolUse { id, name, input } => Some(serde_json::json!({
                                        "id": id,
                                        "type": "function",
                                        "function": {
                                            "name": name,
                                            "arguments": serde_json::to_string(input).unwrap_or_default()
                                        }
                                    })),
                                    _ => None,
                                })
                                .collect();

                            // Check for any text content
                            let text_content = blocks.iter().find_map(|b| match b {
                                ContentBlock::Text { text } => Some(text.clone()),
                                _ => None,
                            });

                            result.push(serde_json::json!({
                                "role": "assistant",
                                "content": text_content,
                                "tool_calls": tool_calls
                            }));
                            continue;
                        }

                        // Assistant message with just text
                        if let Some(text) = blocks.iter().find_map(|b| match b {
                            ContentBlock::Text { text } => Some(text.clone()),
                            _ => None,
                        }) {
                            result.push(serde_json::json!({
                                "role": "assistant",
                                "content": text
                            }));
                            continue;
                        }
                    }

                    // Handle user messages with tool results
                    for block in blocks {
                        match block {
                            ContentBlock::ToolResult {
                                tool_use_id,
                                content,
                                ..
                            } => {
                                // Tool results become separate "tool" role messages
                                result.push(serde_json::json!({
                                    "role": "tool",
                                    "tool_call_id": tool_use_id,
                                    "content": content
                                }));
                            }
                            ContentBlock::Text { text } => {
                                result.push(serde_json::json!({
                                    "role": msg.role,
                                    "content": text
                                }));
                            }
                            ContentBlock::ToolUse { .. } => {
                                // Tool use in user message - shouldn't happen, skip
                            }
                        }
                    }
                }
            }
        }

        result
    }

    /// Convert our Tool format to OpenAI format
    fn convert_tools(tools: &[Tool]) -> Vec<serde_json::Value> {
        tools
            .iter()
            .map(|tool| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.input_schema
                    }
                })
            })
            .collect()
    }
}

#[async_trait]
impl ModelProvider for OpenRouterProvider {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[Tool],
        system: Option<&str>,
    ) -> Result<(Action, TokenUsage)> {
        // Convert messages
        let mut openai_messages = Vec::new();

        // Add system message if provided
        if let Some(system_text) = system {
            openai_messages.push(serde_json::json!({
                "role": "system",
                "content": system_text
            }));
        }

        // Add user/assistant messages
        openai_messages.extend(Self::convert_messages(messages));

        let openai_tools = if tools.is_empty() {
            None
        } else {
            Some(Self::convert_tools(tools))
        };

        let request = ChatRequest {
            model: self.model.clone(),
            messages: openai_messages,
            tools: openai_tools,
            temperature: self.temperature,
            max_tokens: Some(self.max_tokens),
        };

        let response = self.client.chat_completion(request).await?;

        // Extract first choice
        let choice = response
            .choices
            .first()
            .ok_or_else(|| ProviderError::InvalidResponse("No choices in response".to_string()))?;

        // Convert to Action
        let action = if !choice.message.tool_calls.is_empty() {
            // Tool call
            let tool_call = &choice.message.tool_calls[0];
            let tool_name = parse_tool_name(&tool_call.function.name)?;

            let input: serde_json::Value = serde_json::from_str(&tool_call.function.arguments)
                .map_err(|e| {
                    ProviderError::InvalidResponse(format!("Failed to parse tool arguments: {}", e))
                })?;

            Action::tool_call(tool_name, input)
        } else if let Some(content) = &choice.message.content {
            if content.trim().is_empty()
                || choice.finish_reason.as_deref() == Some("stop")
                || choice.finish_reason.as_deref() == Some("end_turn")
            {
                Action::stop()
            } else {
                Action::text(content.clone())
            }
        } else {
            Action::stop()
        };

        // Create token usage breakdown from OpenRouter response
        let usage = TokenUsage::new(
            response.usage.prompt_tokens,
            response.usage.completion_tokens,
        );

        Ok((action, usage))
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn cost_per_input_token(&self) -> Cost {
        // OpenRouter pricing varies by model
        // For Anthropic models, use similar pricing
        // This is a rough estimate - ideally we'd get pricing from OpenRouter API
        if self.model.starts_with("anthropic/claude-sonnet") {
            Cost::from_microdollars(3) // $3 per 1M tokens
        } else {
            Cost::from_microdollars(2) // Generic fallback
        }
    }

    fn cost_per_output_token(&self) -> Cost {
        if self.model.starts_with("anthropic/claude-sonnet") {
            Cost::from_microdollars(15) // $15 per 1M tokens
        } else {
            Cost::from_microdollars(10) // Generic fallback
        }
    }
}

/// Parse tool name from string - now accepts any tool name
fn parse_tool_name(name: &str) -> Result<ToolName> {
    Ok(ToolName::new(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_convert_tool_use_message() {
        // Create an assistant message with a tool use (our new format)
        let messages = vec![
            Message {
                role: "user".to_string(),
                content: MessageContent::Text("Create a file".to_string()),
            },
            Message {
                role: "assistant".to_string(),
                content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                    id: "toolu_123".to_string(),
                    name: "write".to_string(),
                    input: json!({"path": "test.txt", "content": "hello"}),
                }]),
            },
        ];

        let converted = OpenRouterProvider::convert_messages(&messages);

        assert_eq!(converted.len(), 2);

        // Check user message
        assert_eq!(converted[0]["role"], "user");
        assert_eq!(converted[0]["content"], "Create a file");

        // Check assistant message with tool call
        assert_eq!(converted[1]["role"], "assistant");
        assert!(converted[1]["tool_calls"].is_array());

        let tool_calls = converted[1]["tool_calls"].as_array().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0]["id"], "toolu_123");
        assert_eq!(tool_calls[0]["type"], "function");
        assert_eq!(tool_calls[0]["function"]["name"], "write");
    }

    #[test]
    fn test_convert_tool_result_message() {
        // Create a user message with a tool result
        let messages = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "toolu_123".to_string(),
                content: "File created successfully".to_string(),
                is_error: None,
            }]),
        }];

        let converted = OpenRouterProvider::convert_messages(&messages);

        assert_eq!(converted.len(), 1);

        // Check that it becomes a "tool" role message
        assert_eq!(converted[0]["role"], "tool");
        assert_eq!(converted[0]["tool_call_id"], "toolu_123");
        assert_eq!(converted[0]["content"], "File created successfully");
    }

    #[test]
    fn test_convert_full_conversation() {
        // Simulate a full conversation with tool use and results
        let messages = vec![
            Message {
                role: "user".to_string(),
                content: MessageContent::Text("Read a file".to_string()),
            },
            Message {
                role: "assistant".to_string(),
                content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                    id: "toolu_456".to_string(),
                    name: "read".to_string(),
                    input: json!({"path": "test.txt"}),
                }]),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "toolu_456".to_string(),
                    content: "file contents".to_string(),
                    is_error: None,
                }]),
            },
        ];

        let converted = OpenRouterProvider::convert_messages(&messages);

        assert_eq!(converted.len(), 3);
        assert_eq!(converted[0]["role"], "user");
        assert_eq!(converted[1]["role"], "assistant");
        assert_eq!(converted[1]["tool_calls"].as_array().unwrap().len(), 1);
        assert_eq!(converted[2]["role"], "tool");
        assert_eq!(converted[2]["tool_call_id"], "toolu_456");
    }
}
