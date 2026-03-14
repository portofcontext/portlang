//! Structured output pipeline.
//!
//! Replaces binary pass/fail JSON validation with a multi-stage parse +
//! schema-aware coercion pipeline inspired by BAML's Schema-Aligned Parsing.
//!
//! # Pipeline
//!
//! ```text
//! raw text
//!   → parser   (4 stages: direct → fenced → embedded → fixing)
//!   → coercer  (schema-aligned correction with scoring)
//!   → best candidate selected (lowest correction score)
//!   → on failure: fixup message injected into context (up to 2 attempts)
//! ```

pub mod coercer;
pub mod fixing_parser;
pub mod fixup;
pub mod parser;

pub use coercer::{coerce, CoercedValue, Correction};
pub use fixup::{build_fixup_message, FixupTracker, MAX_FIXUP_ATTEMPTS};
pub use parser::{extract_candidates, extract_json, ParseCandidate, ParseStage};

use anyhow::{anyhow, Result};
use serde_json::Value;

/// Parse `text` and coerce the result toward `schema`, returning the best
/// matching `CoercedValue`.
///
/// Tries all parse candidates and all coercion branches, returning the one
/// with the lowest correction score. Returns an error only when no candidate
/// can be reconciled with the schema at all.
pub fn parse_and_coerce(text: &str, schema: &Value) -> Result<CoercedValue> {
    let candidates = extract_candidates(text);
    if candidates.is_empty() {
        return Err(anyhow!(
            "Could not extract any JSON from agent output.\n\nRaw output:\n{}",
            text
        ));
    }

    let mut best: Option<(CoercedValue, &str)> = None;
    let mut last_error = String::new();

    for candidate in &candidates {
        match coerce(&candidate.value, schema) {
            Ok(coerced) => {
                let is_better = best.as_ref().is_none_or(|(b, _)| coerced.score < b.score);
                if is_better {
                    let stage_name = match candidate.stage {
                        ParseStage::Direct => "direct",
                        ParseStage::MarkdownFence => "markdown_fence",
                        ParseStage::EmbeddedScan => "embedded_scan",
                        ParseStage::FixingParser => "fixing_parser",
                    };
                    best = Some((coerced, stage_name));
                }
            }
            Err(e) => {
                last_error = e;
            }
        }
    }

    best.map(|(coerced, _)| coerced)
        .ok_or_else(|| anyhow!("No candidate matched schema: {}", last_error))
}

/// Validate a `Value` against a JSON Schema strictly (no coercion).
/// Used for the final hard validation after coercion.
pub fn validate_against_schema(value: &Value, schema: &Value) -> Result<()> {
    let compiled = jsonschema::validator_for(schema)
        .map_err(|e| anyhow!("Failed to compile JSON schema: {}", e))?;
    let messages: Vec<String> = compiled.iter_errors(value).map(|e| e.to_string()).collect();
    if !messages.is_empty() {
        return Err(anyhow!(
            "Schema validation failed:\n{}",
            messages.join("\n")
        ));
    }
    Ok(())
}

/// Extract raw text from trajectory steps (backward compat with old module).
pub fn extract_structured_output_from_trajectory(
    steps: &[portlang_core::TrajectoryStep],
) -> Result<Value> {
    use portlang_core::Action;

    for step in steps.iter().rev() {
        if let Action::TextOutput { text } = &step.action {
            if let Some(candidate) = extract_json(text) {
                return Ok(candidate.value);
            }
        }
    }

    Err(anyhow!(
        "No valid JSON found in agent output. Agent must output JSON matching the schema."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- parse_and_coerce integration ---

    #[test]
    fn direct_valid_json_no_corrections() {
        let schema = json!({
            "type": "object",
            "required": ["status"],
            "properties": {"status": {"type": "string", "enum": ["ok", "error"]}}
        });
        let result = parse_and_coerce(r#"{"status": "ok"}"#, &schema).unwrap();
        assert_eq!(result.value["status"], "ok");
        assert_eq!(result.score, 0);
    }

    #[test]
    fn json_in_markdown_fence_coerced() {
        let schema =
            json!({"type": "object", "properties": {"x": {"type": "integer"}}, "required": ["x"]});
        let text = "Here you go:\n```json\n{\"x\": 42}\n```";
        let result = parse_and_coerce(text, &schema).unwrap();
        assert_eq!(result.value["x"], 42);
    }

    #[test]
    fn broken_json_fixed_and_coerced() {
        let schema = json!({"type": "object", "properties": {"name": {"type": "string"}}, "required": ["name"]});
        let result = parse_and_coerce(r#"{name: "Alice"}"#, &schema).unwrap();
        assert_eq!(result.value["name"], "Alice");
    }

    #[test]
    fn enum_case_corrected_in_pipeline() {
        let schema = json!({"type": "object", "required": ["status"],
            "properties": {"status": {"type": "string", "enum": ["success", "failure"]}}});
        let result = parse_and_coerce(r#"{"status": "SUCCESS"}"#, &schema).unwrap();
        assert_eq!(result.value["status"], "success");
        assert!(result.score > 0);
    }

    #[test]
    fn pure_garbage_returns_error() {
        let schema = json!({"type": "object"});
        assert!(parse_and_coerce("no json here at all", &schema).is_err());
    }

    // --- validate_against_schema ---

    #[test]
    fn validate_passes_for_valid() {
        let v = json!({"status": "ok"});
        let s = json!({"type": "object", "required": ["status"],
            "properties": {"status": {"type": "string"}}});
        assert!(validate_against_schema(&v, &s).is_ok());
    }

    #[test]
    fn validate_fails_for_invalid() {
        let v = json!({"status": 123});
        let s = json!({"type": "object", "required": ["status"],
            "properties": {"status": {"type": "string"}}});
        assert!(validate_against_schema(&v, &s).is_err());
    }
}
