use crate::provider::{ContentBlock, Message};
use portlang_core::{Cost, VerifierResult};

/// Context window manages the conversation history
pub struct ContextWindow {
    messages: Vec<Message>,
    total_tokens: u64,
    total_cost: Cost,
}

impl ContextWindow {
    pub fn new(goal: &str) -> Self {
        Self {
            messages: vec![Message::user(goal)],
            total_tokens: 0,
            total_cost: Cost::ZERO,
        }
    }

    /// Append an observation (user message with tool results)
    pub fn append_observation(&mut self, text: impl Into<String>) {
        self.messages.push(Message::user(text));
    }

    /// Append tool results
    pub fn append_tool_result(&mut self, tool_use_id: String, result: String, is_error: bool) {
        let block = ContentBlock::ToolResult {
            tool_use_id,
            content: result,
            is_error: if is_error { Some(true) } else { None },
        };
        self.messages.push(Message::user_blocks(vec![block]));
    }

    /// Append an agent response
    pub fn append_response(&mut self, blocks: Vec<ContentBlock>) {
        self.messages.push(Message::assistant_blocks(blocks));
    }

    /// Append a rejection message
    pub fn append_rejection(&mut self, reason: impl Into<String>) {
        self.messages
            .push(Message::user(format!("REJECTED: {}", reason.into())));
    }

    /// Append verifier results
    pub fn append_verifier_results(&mut self, results: &[VerifierResult]) {
        if results.is_empty() {
            return;
        }

        let mut message = String::from("Verifier results:\n");
        for result in results {
            message.push_str(&format!(
                "\n[{}] {}: {}\n",
                if result.passed { "PASS" } else { "FAIL" },
                result.name,
                if result.passed { "OK" } else { &result.stderr }
            ));
        }

        self.messages.push(Message::user(message));
    }

    /// Update token and cost tracking
    pub fn add_tokens_and_cost(&mut self, tokens: u64, cost: Cost) {
        self.total_tokens += tokens;
        self.total_cost += cost;
    }

    /// Get all messages
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// Get total tokens used
    pub fn total_tokens(&self) -> u64 {
        self.total_tokens
    }

    /// Get total cost
    pub fn total_cost(&self) -> Cost {
        self.total_cost
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_proper_tool_use_flow() {
        let mut ctx = ContextWindow::new("test goal");

        // Record tool call (assistant message)
        let block = ContentBlock::ToolUse {
            id: "toolu_123".to_string(),
            name: "read".to_string(),
            input: json!({"path": "test.txt"}),
        };
        ctx.append_response(vec![block]);

        // Record result (user message)
        ctx.append_tool_result("toolu_123".to_string(), "file content".to_string(), false);

        // Verify message structure
        let messages = ctx.messages();
        assert_eq!(
            messages.len(),
            3,
            "Should have 3 messages: goal, assistant, user"
        );

        // Verify roles
        assert_eq!(
            messages[0].role, "user",
            "First message should be user (goal)"
        );
        assert_eq!(
            messages[1].role, "assistant",
            "Second message should be assistant (tool use)"
        );
        assert_eq!(
            messages[2].role, "user",
            "Third message should be user (tool result)"
        );
    }

    #[test]
    fn test_text_output_flow() {
        let mut ctx = ContextWindow::new("test goal");

        // Record text output (assistant message)
        let block = ContentBlock::Text {
            text: "Hello, world!".to_string(),
        };
        ctx.append_response(vec![block]);

        // Verify message structure
        let messages = ctx.messages();
        assert_eq!(messages.len(), 2, "Should have 2 messages: goal, assistant");

        // Verify roles
        assert_eq!(
            messages[0].role, "user",
            "First message should be user (goal)"
        );
        assert_eq!(
            messages[1].role, "assistant",
            "Second message should be assistant (text)"
        );
    }

    #[test]
    fn test_error_handling_with_tool_id() {
        let mut ctx = ContextWindow::new("test goal");

        // Record tool call
        let block = ContentBlock::ToolUse {
            id: "toolu_456".to_string(),
            name: "write".to_string(),
            input: json!({"path": "test.txt", "content": "data"}),
        };
        ctx.append_response(vec![block]);

        // Record error result
        ctx.append_tool_result(
            "toolu_456".to_string(),
            "Permission denied".to_string(),
            true,
        );

        // Verify message structure
        let messages = ctx.messages();
        assert_eq!(messages.len(), 3, "Should have 3 messages");
        assert_eq!(
            messages[1].role, "assistant",
            "Tool call should be assistant message"
        );
        assert_eq!(
            messages[2].role, "user",
            "Error result should be user message"
        );
    }
}
