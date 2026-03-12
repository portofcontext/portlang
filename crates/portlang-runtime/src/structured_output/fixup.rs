/// Fixup loop support.
///
/// When `parse_and_coerce` fails, the `FixupTracker` injects a correction
/// message back into the agent's context window, giving it one more chance
/// to produce valid output. The raw malformed output and the schema violation
/// are both included so the model understands exactly what went wrong.
///
/// Max attempts is capped at `MAX_FIXUP_ATTEMPTS` to prevent infinite loops.
use serde_json::Value;

pub const MAX_FIXUP_ATTEMPTS: usize = 2;

/// Tracks how many fixup attempts have been made for the current run.
#[derive(Debug, Default)]
pub struct FixupTracker {
    attempts: usize,
}

impl FixupTracker {
    pub fn new() -> Self {
        Self { attempts: 0 }
    }

    /// Returns `true` if another fixup attempt is allowed.
    pub fn can_attempt(&self) -> bool {
        self.attempts < MAX_FIXUP_ATTEMPTS
    }

    /// Records a fixup attempt and returns the message to inject into context,
    /// or `None` if the limit has been reached.
    pub fn next_message(
        &mut self,
        raw_output: &str,
        schema: &Value,
        error: &str,
    ) -> Option<String> {
        if self.attempts >= MAX_FIXUP_ATTEMPTS {
            return None;
        }
        self.attempts += 1;
        Some(build_fixup_message(
            raw_output,
            schema,
            error,
            self.attempts,
        ))
    }

    pub fn attempts(&self) -> usize {
        self.attempts
    }
}

/// Build the message injected into context after a structured output failure.
///
/// The message includes:
/// - The exact validation error
/// - The raw malformed output from the previous attempt
/// - The required schema
///
/// This mirrors BAML's `@assert` + fixup pattern: feed the broken output back
/// to the model with enough context to self-correct.
pub fn build_fixup_message(
    raw_output: &str,
    schema: &Value,
    error: &str,
    attempt: usize,
) -> String {
    let schema_str = serde_json::to_string_pretty(schema).unwrap_or_else(|_| schema.to_string());
    format!(
        "[STRUCTURED OUTPUT FIXUP — attempt {attempt}]\n\
         Your previous output did not match the required JSON schema.\n\
         \n\
         Error:\n\
         {error}\n\
         \n\
         Your output was:\n\
         {raw_output}\n\
         \n\
         Required schema:\n\
         {schema_str}\n\
         \n\
         Please output ONLY valid JSON that matches the schema. No prose, no markdown fences, no explanation."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- FixupTracker ---

    #[test]
    fn new_tracker_can_attempt() {
        let tracker = FixupTracker::new();
        assert!(tracker.can_attempt());
        assert_eq!(tracker.attempts(), 0);
    }

    #[test]
    fn tracker_allows_up_to_max_attempts() {
        let mut tracker = FixupTracker::new();
        for _ in 0..MAX_FIXUP_ATTEMPTS {
            assert!(tracker.can_attempt());
            let msg = tracker.next_message("bad output", &json!({}), "some error");
            assert!(msg.is_some());
        }
        assert!(!tracker.can_attempt());
        assert!(tracker
            .next_message("bad output", &json!({}), "some error")
            .is_none());
    }

    #[test]
    fn tracker_counts_attempts() {
        let mut tracker = FixupTracker::new();
        tracker.next_message("x", &json!({}), "e");
        assert_eq!(tracker.attempts(), 1);
        tracker.next_message("x", &json!({}), "e");
        assert_eq!(tracker.attempts(), 2);
    }

    // --- build_fixup_message content ---

    #[test]
    fn fixup_message_contains_raw_output() {
        let msg = build_fixup_message(
            r#"{"status": "bad_value"}"#,
            &json!({"type": "object"}),
            "enum validation failed",
            1,
        );
        assert!(msg.contains(r#"{"status": "bad_value"}"#));
    }

    #[test]
    fn fixup_message_contains_error() {
        let msg = build_fixup_message("raw", &json!({}), "enum validation failed", 1);
        assert!(msg.contains("enum validation failed"));
    }

    #[test]
    fn fixup_message_contains_schema() {
        let schema = json!({"type": "object", "required": ["status"]});
        let msg = build_fixup_message("raw", &schema, "error", 1);
        assert!(msg.contains("status"));
        assert!(msg.contains("required"));
    }

    #[test]
    fn fixup_message_contains_attempt_number() {
        let msg1 = build_fixup_message("raw", &json!({}), "err", 1);
        let msg2 = build_fixup_message("raw", &json!({}), "err", 2);
        assert!(msg1.contains("attempt 1"));
        assert!(msg2.contains("attempt 2"));
    }

    #[test]
    fn fixup_message_instructs_json_only() {
        let msg = build_fixup_message("raw", &json!({}), "err", 1);
        assert!(msg.contains("ONLY valid JSON"));
    }

    #[test]
    fn next_message_increments_and_returns_message() {
        let mut tracker = FixupTracker::new();
        let msg = tracker.next_message("bad", &json!({"type": "object"}), "missing field");
        assert!(msg.is_some());
        let text = msg.unwrap();
        assert!(text.contains("attempt 1"));
        assert!(text.contains("missing field"));
    }
}
