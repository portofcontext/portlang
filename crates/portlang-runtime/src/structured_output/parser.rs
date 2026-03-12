/// Multi-stage JSON extractor.
///
/// Stage 1 — Direct:       `serde_json::from_str`
/// Stage 2 — Fenced:       extract from ```json or ``` markdown blocks
/// Stage 3 — Embedded:     scan prose for `{...}` / `[...]` substrings
/// Stage 4 — Fixing:       character-by-character correction via `fixing_parser`
///
/// Returns all candidate `Value`s found. The caller (coercer) picks the best
/// match against the target schema.
use super::fixing_parser::fix_json;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum ParseStage {
    Direct,
    MarkdownFence,
    EmbeddedScan,
    FixingParser,
}

#[derive(Debug, Clone)]
pub struct ParseCandidate {
    pub value: Value,
    pub stage: ParseStage,
}

/// Extract all JSON candidates from `text`, in stage order.
/// Returns the first stage's results if successful, otherwise falls through.
pub fn extract_candidates(text: &str) -> Vec<ParseCandidate> {
    let text = text.trim();

    // Stage 1 — direct parse
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        return vec![ParseCandidate {
            value: v,
            stage: ParseStage::Direct,
        }];
    }

    // Stage 2 — markdown fences
    let fenced = extract_fenced(text);
    if !fenced.is_empty() {
        return fenced;
    }

    // Stage 3 — embedded JSON objects/arrays in prose
    let embedded = extract_embedded(text);
    if !embedded.is_empty() {
        return embedded;
    }

    // Stage 4 — fixing parser on the whole text
    if let Ok(v) = fix_json(text) {
        return vec![ParseCandidate {
            value: v,
            stage: ParseStage::FixingParser,
        }];
    }

    vec![]
}

/// Pick the single best candidate (first found). Returns None if none parsed.
pub fn extract_json(text: &str) -> Option<ParseCandidate> {
    extract_candidates(text).into_iter().next()
}

// ---------------------------------------------------------------------------
// Stage 2: markdown fence extraction
// ---------------------------------------------------------------------------

fn extract_fenced(text: &str) -> Vec<ParseCandidate> {
    let mut candidates = Vec::new();
    let mut search = text;

    while let Some(fence_start) = search.find("```") {
        let after_fence = &search[fence_start + 3..];

        // Skip optional language tag (e.g. "json\n")
        let content_start = after_fence.find('\n').map(|i| i + 1).unwrap_or(0);
        let content = &after_fence[content_start..];

        if let Some(end) = content.find("```") {
            let block = content[..end].trim();
            if let Ok(v) = serde_json::from_str::<Value>(block) {
                candidates.push(ParseCandidate {
                    value: v,
                    stage: ParseStage::MarkdownFence,
                });
            } else if let Ok(v) = fix_json(block) {
                candidates.push(ParseCandidate {
                    value: v,
                    stage: ParseStage::MarkdownFence,
                });
            }
            // Advance past this block
            let consumed = fence_start + 3 + content_start + end + 3;
            if consumed >= search.len() {
                break;
            }
            search = &search[consumed..];
        } else {
            break;
        }
    }

    candidates
}

// ---------------------------------------------------------------------------
// Stage 3: embedded JSON scan
// ---------------------------------------------------------------------------

fn extract_embedded(text: &str) -> Vec<ParseCandidate> {
    let mut candidates = Vec::new();
    let chars: Vec<char> = text.chars().collect();

    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '{' || c == '[' {
            if let Some(end) = find_matching_close(&chars, i) {
                let snippet: String = chars[i..=end].iter().collect();
                if let Ok(v) = serde_json::from_str::<Value>(&snippet) {
                    candidates.push(ParseCandidate {
                        value: v,
                        stage: ParseStage::EmbeddedScan,
                    });
                    i = end + 1;
                    continue;
                }
            }
        }
        i += 1;
    }

    candidates
}

fn find_matching_close(chars: &[char], start: usize) -> Option<usize> {
    let open = chars[start];
    let close = if open == '{' { '}' } else { ']' };
    let mut depth = 0;
    let mut in_string = false;
    let mut escape = false;

    for (i, &c) in chars[start..].iter().enumerate() {
        if escape {
            escape = false;
            continue;
        }
        if c == '\\' && in_string {
            escape = true;
            continue;
        }
        if c == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        if c == open {
            depth += 1;
        }
        if c == close {
            depth -= 1;
            if depth == 0 {
                return Some(start + i);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- Stage 1: Direct ---

    #[test]
    fn stage1_direct_valid_json() {
        let candidates = extract_candidates(r#"{"a": 1}"#);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].stage, ParseStage::Direct);
        assert_eq!(candidates[0].value["a"], 1);
    }

    #[test]
    fn stage1_trims_whitespace() {
        let candidates = extract_candidates("  \n{\"a\": 1}\n  ");
        assert_eq!(candidates[0].stage, ParseStage::Direct);
    }

    #[test]
    fn stage1_array() {
        let candidates = extract_candidates("[1, 2, 3]");
        assert_eq!(candidates[0].stage, ParseStage::Direct);
        assert_eq!(candidates[0].value, json!([1, 2, 3]));
    }

    // --- Stage 2: Markdown fences ---

    #[test]
    fn stage2_json_fence() {
        let text = "Here is the result:\n```json\n{\"status\": \"ok\"}\n```";
        let candidates = extract_candidates(text);
        assert!(!candidates.is_empty());
        assert_eq!(candidates[0].stage, ParseStage::MarkdownFence);
        assert_eq!(candidates[0].value["status"], "ok");
    }

    #[test]
    fn stage2_generic_fence() {
        let text = "Result:\n```\n{\"x\": 42}\n```\nDone.";
        let candidates = extract_candidates(text);
        assert!(!candidates.is_empty());
        assert_eq!(candidates[0].stage, ParseStage::MarkdownFence);
        assert_eq!(candidates[0].value["x"], 42);
    }

    #[test]
    fn stage2_fence_with_prose_around_it() {
        let text = "I computed this:\n```json\n{\"answer\": 42}\n```\nHope that helps!";
        let candidates = extract_candidates(text);
        assert_eq!(candidates[0].stage, ParseStage::MarkdownFence);
        assert_eq!(candidates[0].value["answer"], 42);
    }

    #[test]
    fn stage2_multiple_fences_all_returned() {
        let text = "```json\n{\"a\":1}\n```\nAlso:\n```json\n{\"b\":2}\n```";
        let candidates = extract_candidates(text);
        assert_eq!(candidates.len(), 2);
        assert!(candidates
            .iter()
            .all(|c| c.stage == ParseStage::MarkdownFence));
    }

    // --- Stage 3: Embedded scan ---

    #[test]
    fn stage3_json_embedded_in_prose() {
        let text = "The agent produced {\"result\": \"done\"} as output.";
        let candidates = extract_candidates(text);
        assert!(!candidates.is_empty());
        assert_eq!(candidates[0].stage, ParseStage::EmbeddedScan);
        assert_eq!(candidates[0].value["result"], "done");
    }

    #[test]
    fn stage3_multiple_embedded_objects() {
        let text = "First: {\"a\":1} then {\"b\":2}";
        let candidates = extract_candidates(text);
        assert_eq!(candidates.len(), 2);
    }

    // --- Stage 4: Fixing parser ---

    #[test]
    fn stage4_fixes_trailing_comma() {
        let text = r#"{"a": 1,}"#;
        // Not valid JSON so stage 1 fails; not fenced so stage 2 fails;
        // no clean embedded object so stage 3 fails; fixing parser rescues it
        let candidates = extract_candidates(text);
        assert!(!candidates.is_empty());
        assert_eq!(candidates[0].value["a"], 1);
    }

    #[test]
    fn stage4_fixes_unquoted_keys() {
        let candidates = extract_candidates(r#"{name: "Alice"}"#);
        assert!(!candidates.is_empty());
        assert_eq!(candidates[0].value["name"], "Alice");
    }

    // --- extract_json: picks first candidate ---

    #[test]
    fn extract_json_returns_none_for_garbage() {
        let result = extract_json("this is not json at all!!!");
        assert!(result.is_none());
    }

    #[test]
    fn extract_json_returns_some_for_valid() {
        let result = extract_json(r#"{"ok": true}"#);
        assert!(result.is_some());
        assert_eq!(result.unwrap().value["ok"], true);
    }
}
