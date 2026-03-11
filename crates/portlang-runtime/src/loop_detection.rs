use portlang_core::Action;
use std::collections::VecDeque;

/// Detects when the agent is stuck in a loop
pub struct LoopDetector {
    history: VecDeque<ActionSignature>,
    max_history: usize,
    repeat_threshold: usize,
    consecutive_errors: usize,
    error_threshold: usize,
}

/// Simplified signature of an action for loop detection
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ActionSignature {
    Read { path: String },
    Write { path: String },
    Glob { pattern: String },
    CustomTool { name: String, input_hash: u64 },
    Text,
    Stop,
}

impl LoopDetector {
    /// Create a new loop detector
    pub fn new() -> Self {
        Self {
            history: VecDeque::new(),
            max_history: 10,
            repeat_threshold: 3,
            consecutive_errors: 0,
            error_threshold: 3,
        }
    }

    /// Record that the most recent tool call returned an error.
    /// Returns a warning message if the error threshold is reached.
    pub fn record_error(&mut self) -> Option<String> {
        self.consecutive_errors += 1;
        if self.consecutive_errors >= self.error_threshold {
            Some(format!(
                "Warning: {} consecutive tool calls have failed with errors. \
                Something in the environment may be broken or unavailable. \
                Review the errors above and consider a fundamentally different approach, \
                or stop if the task cannot be completed.",
                self.consecutive_errors
            ))
        } else {
            None
        }
    }

    /// Record that the most recent tool call succeeded. Resets the error streak.
    pub fn record_success(&mut self) {
        self.consecutive_errors = 0;
    }

    /// Record an action
    pub fn record(&mut self, action: &Action) {
        let signature = Self::action_signature(action);

        // Add to history
        self.history.push_back(signature);

        // Trim history to max size
        if self.history.len() > self.max_history {
            self.history.pop_front();
        }
    }

    /// Check if the agent is stuck in a loop
    /// Returns Some(message) if a loop is detected, None otherwise
    pub fn detect_loop(&self, action: &Action) -> Option<String> {
        let signature = Self::action_signature(action);

        // Count how many times this exact action appears in recent history
        let count = self.history.iter().filter(|s| *s == &signature).count();

        if count >= self.repeat_threshold {
            return Some(self.loop_message(&signature, count));
        }

        // Detect read loops (reading the same file multiple times)
        if let ActionSignature::Read { path } = &signature {
            if let Some(recent_reads) = self.detect_repeated_reads(path) {
                return Some(format!(
                    "Loop detected: You have read '{}' {} times in the last {} steps. \
                    If this file doesn't exist, you need to WRITE it first, not keep reading it. \
                    If you're trying to understand the file, one read is enough. \
                    Please take a different action.",
                    path,
                    recent_reads,
                    self.history.len()
                ));
            }
        }

        None
    }

    /// Convert an action to a signature for comparison
    fn action_signature(action: &Action) -> ActionSignature {
        match action {
            Action::ToolCall { tool, input } => match tool.as_str() {
                "read" => {
                    let path = input
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    ActionSignature::Read { path }
                }
                "write" => {
                    let path = input
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    ActionSignature::Write { path }
                }
                "glob" => {
                    let pattern = input
                        .get("pattern")
                        .and_then(|v| v.as_str())
                        .unwrap_or("*")
                        .to_string();
                    ActionSignature::Glob { pattern }
                }
                other => {
                    // For custom tools, hash the input to detect exact matches
                    use std::collections::hash_map::DefaultHasher;
                    use std::hash::{Hash, Hasher};
                    let mut hasher = DefaultHasher::new();
                    input.to_string().hash(&mut hasher);
                    ActionSignature::CustomTool {
                        name: other.to_string(),
                        input_hash: hasher.finish(),
                    }
                }
            },
            Action::TextOutput { .. } => ActionSignature::Text,
            Action::Stop => ActionSignature::Stop,
        }
    }

    /// Detect if we're repeatedly reading the same file
    fn detect_repeated_reads(&self, path: &str) -> Option<usize> {
        let count = self
            .history
            .iter()
            .filter(|s| matches!(s, ActionSignature::Read { path: p } if p == path))
            .count();

        if count >= self.repeat_threshold {
            Some(count)
        } else {
            None
        }
    }

    /// Generate a helpful message about the detected loop
    fn loop_message(&self, signature: &ActionSignature, count: usize) -> String {
        match signature {
            ActionSignature::Read { path } => {
                format!(
                    "Loop detected: You have tried to read '{}' {} times. \
                    If the file doesn't exist, you need to WRITE it first. \
                    If you need to check what files exist, use the Glob tool. \
                    Please take a different action to make progress.",
                    path, count
                )
            }
            ActionSignature::Write { path } => {
                format!(
                    "Loop detected: You have tried to write '{}' {} times. \
                    The file should already exist from a previous write. \
                    Check the re-observation output to see the current state. \
                    Please take a different action to make progress.",
                    path, count
                )
            }
            ActionSignature::Glob { pattern } => {
                format!(
                    "Loop detected: You have used glob pattern '{}' {} times. \
                    You already know what files exist. \
                    Please take a different action to make progress.",
                    pattern, count
                )
            }
            ActionSignature::Text => {
                format!(
                    "Loop detected: You have produced text output {} times. \
                    Please take a concrete action (read, write, glob, or stop) to make progress.",
                    count
                )
            }
            ActionSignature::Stop => {
                "Loop detected: Multiple stop attempts. This shouldn't happen.".to_string()
            }
            ActionSignature::CustomTool { name, .. } => {
                format!(
                    "Loop detected: You have used tool '{}' {} times with the same inputs. \
                    Please take a different action to make progress.",
                    name, count
                )
            }
        }
    }
}

impl Default for LoopDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use portlang_core::ToolName;
    use serde_json::json;

    #[test]
    fn test_detect_read_loop() {
        let mut detector = LoopDetector::new();
        let action = Action::tool_call(ToolName::new("read"), json!({ "path": "test.txt" }));

        // First two times should be fine
        detector.record(&action);
        assert!(detector.detect_loop(&action).is_none());

        detector.record(&action);
        assert!(detector.detect_loop(&action).is_none());

        // Third time should trigger
        detector.record(&action);
        let result = detector.detect_loop(&action);
        assert!(result.is_some());
        assert!(result.unwrap().contains("Loop detected"));
    }

    #[test]
    fn test_different_files_no_loop() {
        let mut detector = LoopDetector::new();

        let action1 = Action::tool_call(ToolName::new("read"), json!({ "path": "file1.txt" }));
        let action2 = Action::tool_call(ToolName::new("read"), json!({ "path": "file2.txt" }));
        let action3 = Action::tool_call(ToolName::new("read"), json!({ "path": "file3.txt" }));

        detector.record(&action1);
        detector.record(&action2);
        detector.record(&action3);

        // Reading different files shouldn't trigger loop detection
        assert!(detector.detect_loop(&action1).is_none());
    }

    #[test]
    fn test_write_after_read_no_loop() {
        let mut detector = LoopDetector::new();

        let read = Action::tool_call(ToolName::new("read"), json!({ "path": "test.txt" }));
        let write = Action::tool_call(
            ToolName::new("write"),
            json!({ "path": "test.txt", "content": "x" }),
        );

        detector.record(&read);
        detector.record(&write);

        // Different action types on same file is fine
        assert!(detector.detect_loop(&read).is_none());
        assert!(detector.detect_loop(&write).is_none());
    }
}
