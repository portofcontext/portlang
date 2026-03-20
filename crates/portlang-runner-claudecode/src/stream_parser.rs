/// Parser for Claude Code's `--output-format stream-json` JSONL output.
///
/// Claude Code emits one JSON object per line. The events we care about:
///
/// ```json
/// {"type":"assistant","message":{"content":[
///   {"type":"text","text":"..."},
///   {"type":"tool_use","id":"toolu_...","name":"Bash","input":{"command":"ls"}}
/// ],"usage":{"input_tokens":N,"output_tokens":N},...}}
///
/// {"type":"user","message":{"content":[
///   {"type":"tool_result","tool_use_id":"toolu_...","content":"...output..."}
/// ]}}
///
/// {"type":"result","subtype":"success","cost_usd":0.05,...}
/// ```
use portlang_core::{Action, Cost, TrajectoryStep};
use serde_json::Value;

/// State accumulated while parsing the stream, flushed into TrajectoryStep on
/// tool_result or stop.
pub struct StreamAccumulator {
    /// Pending tool calls waiting for their results (id, name, input)
    pending: Vec<(String, String, Value)>,
    /// Steps completed so far
    pub steps: Vec<TrajectoryStep>,
    /// Cumulative input tokens from usage events
    pub input_tokens: u64,
    /// Cumulative output tokens from usage events
    pub output_tokens: u64,
    /// Total cost in dollars from result event
    pub cost_usd: f64,
    /// Final result subtype ("success", "error_max_turns", etc.)
    pub result_subtype: Option<String>,
}

impl StreamAccumulator {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            steps: Vec::new(),
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: 0.0,
            result_subtype: None,
        }
    }

    /// Process one JSONL line. Returns `true` if the stream is done (result event seen).
    pub fn process_line(&mut self, line: &str) -> bool {
        let event: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                tracing::debug!("claude-code stream: non-JSON line ({}): {}", e, line);
                return false;
            }
        };

        let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match event_type {
            "assistant" => self.handle_assistant(&event),
            "user" => self.handle_user(&event),
            "result" => {
                self.handle_result(&event);
                return true;
            }
            other => {
                tracing::debug!("claude-code stream: ignoring event type '{}'", other);
            }
        }

        false
    }

    fn handle_assistant(&mut self, event: &Value) {
        let message = match event.get("message") {
            Some(m) => m,
            None => return,
        };

        // Accumulate token usage from each assistant turn so budget-killed runs
        // still report non-zero tokens (result event is never received on kill).
        if let Some(usage) = message.get("usage") {
            self.input_tokens = self.input_tokens.max(
                usage
                    .get("input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
            );
            self.output_tokens += usage
                .get("output_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
        }

        let content = match message.get("content").and_then(|v| v.as_array()) {
            Some(c) => c,
            None => return,
        };

        for block in content {
            let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");

            match block_type {
                "text" => {
                    let text = block
                        .get("text")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if !text.trim().is_empty() {
                        let step_number = self.steps.len() + 1;
                        let step = TrajectoryStep::new(
                            step_number,
                            Action::text(text),
                            String::new(),
                            false,
                            Cost::ZERO,
                            0,
                        );
                        self.steps.push(step);
                    }
                }
                "tool_use" => {
                    let id = block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = block
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let input = block
                        .get("input")
                        .cloned()
                        .unwrap_or(Value::Object(Default::default()));

                    let input_preview = serde_json::to_string(&input).unwrap_or_default();
                    let preview = if input_preview.len() > 120 {
                        format!("{}…", &input_preview[..120])
                    } else {
                        input_preview
                    };
                    tracing::info!("→ tool_use  {} {}", name, preview);

                    self.pending.push((id, name, input));
                }
                _ => {}
            }
        }
    }

    fn handle_user(&mut self, event: &Value) {
        let message = match event.get("message") {
            Some(m) => m,
            None => return,
        };

        let content = match message.get("content").and_then(|v| v.as_array()) {
            Some(c) => c,
            None => return,
        };

        for block in content {
            let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");

            if block_type == "tool_result" {
                let tool_use_id = block
                    .get("tool_use_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                // Extract text content from tool result
                let result_text = extract_tool_result_text(block);

                // Match to pending tool call
                if let Some(pos) = self
                    .pending
                    .iter()
                    .position(|(id, _, _)| id == &tool_use_id)
                {
                    let (_, name, input) = self.pending.remove(pos);
                    let result_preview = if result_text.len() > 120 {
                        format!("{}…", &result_text[..120])
                    } else {
                        result_text.clone()
                    };
                    tracing::info!("← tool_result {} {}", name, result_preview);
                    let step_number = self.steps.len() + 1;
                    let step = TrajectoryStep::new(
                        step_number,
                        Action::ToolCall {
                            tool: name.into(),
                            input,
                        },
                        result_text,
                        false,
                        Cost::ZERO,
                        0,
                    );
                    self.steps.push(step);
                } else {
                    tracing::debug!(
                        "claude-code stream: tool_result for unknown id '{}'",
                        tool_use_id
                    );
                }
            }
        }
    }

    fn handle_result(&mut self, event: &Value) {
        self.result_subtype = event
            .get("subtype")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Cost: prefer total_cost_usd, fall back to cost_usd
        self.cost_usd = event
            .get("total_cost_usd")
            .or_else(|| event.get("cost_usd"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        // Token counts from result.usage
        if let Some(usage) = event.get("usage") {
            // Input = direct input + cache reads + cache creation
            let direct = usage
                .get("input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let cache_read = usage
                .get("cache_read_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let cache_create = usage
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            self.input_tokens = direct + cache_read + cache_create;
            self.output_tokens = usage
                .get("output_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
        }

        tracing::info!(
            "result  subtype={} cost=${:.4} tokens={}",
            self.result_subtype.as_deref().unwrap_or("?"),
            self.cost_usd,
            self.total_tokens()
        );

        // Flush any remaining pending tool calls (without results)
        for (_, name, input) in self.pending.drain(..) {
            let step_number = self.steps.len() + 1;
            let step = TrajectoryStep::new(
                step_number,
                Action::ToolCall {
                    tool: name.into(),
                    input,
                },
                "(no result captured)".to_string(),
                false,
                Cost::ZERO,
                0,
            );
            self.steps.push(step);
        }
    }

    /// Total tokens seen so far (input + output, best effort)
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }

    /// True if the result event indicated success
    pub fn is_success(&self) -> bool {
        matches!(self.result_subtype.as_deref(), Some("success"))
    }
}

fn extract_tool_result_text(block: &Value) -> String {
    // content can be a string or array of content blocks
    match block.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|item| {
                if item.get("type").and_then(|v| v.as_str()) == Some("text") {
                    item.get("text")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}
