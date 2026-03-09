use anyhow::{anyhow, Context, Result};
use portlang_core::Action;
use serde_json::Value;

/// Extract structured output from an agent's final output
/// Looks at the stop action first, then falls back to the most recent text output
pub fn extract_structured_output_from_trajectory(
    steps: &[portlang_core::TrajectoryStep],
) -> Result<Value> {
    // First try the last action (the stop)
    if let Some(last_step) = steps.last() {
        if let Action::TextOutput { text } = &last_step.action {
            if let Ok(value) = extract_json_from_text(text) {
                return Ok(value);
            }
        }
    }

    // Fall back to searching backwards for the most recent text output with JSON
    for step in steps.iter().rev() {
        if let Action::TextOutput { text } = &step.action {
            if let Ok(value) = extract_json_from_text(text) {
                return Ok(value);
            }
        }
    }

    Err(anyhow!(
        "No valid JSON found in agent output. Agent must output JSON matching the schema."
    ))
}

/// Extract structured output from an agent's stop action (deprecated, use extract_structured_output_from_trajectory)
pub fn extract_structured_output(action: &Action) -> Result<Value> {
    match action {
        Action::TextOutput { text } => {
            // Try to find JSON in the text output
            extract_json_from_text(text)
        }
        Action::Stop => Err(anyhow!("Agent stopped without producing output")),
        Action::ToolCall { .. } => Err(anyhow!("Expected text output, got tool call")),
    }
}

/// Extract JSON from text, handling markdown code blocks
fn extract_json_from_text(text: &str) -> Result<Value> {
    let text = text.trim();

    // Try direct JSON parse first
    if let Ok(value) = serde_json::from_str::<Value>(text) {
        return Ok(value);
    }

    // Try to extract from markdown code block
    if let Some(json_str) = extract_from_code_block(text, "json") {
        if let Ok(value) = serde_json::from_str::<Value>(&json_str) {
            return Ok(value);
        }
    }

    // Try to extract from any code block
    if let Some(json_str) = extract_from_any_code_block(text) {
        if let Ok(value) = serde_json::from_str::<Value>(&json_str) {
            return Ok(value);
        }
    }

    Err(anyhow!("Could not parse JSON from agent output"))
}

/// Extract content from markdown code block with specific language
fn extract_from_code_block(text: &str, lang: &str) -> Option<String> {
    let start_marker = format!("```{}", lang);
    let end_marker = "```";

    let start_idx = text.find(&start_marker)? + start_marker.len();
    let remaining = &text[start_idx..];
    let end_idx = remaining.find(end_marker)?;

    Some(remaining[..end_idx].trim().to_string())
}

/// Extract content from any markdown code block
fn extract_from_any_code_block(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut in_block = false;
    let mut content = String::new();

    for line in lines {
        if line.trim().starts_with("```") {
            if in_block {
                // End of block, try to parse what we have
                return Some(content.trim().to_string());
            } else {
                // Start of block
                in_block = true;
                content.clear();
            }
        } else if in_block {
            content.push_str(line);
            content.push('\n');
        }
    }

    None
}

/// Validate JSON value against JSON Schema
pub fn validate_against_schema(value: &Value, schema: &Value) -> Result<()> {
    // Use jsonschema crate for validation
    // We need to Box::leak to satisfy the 'static lifetime requirement
    // This is safe because schemas are small and this only happens once per run
    let schema_static: &'static Value = Box::leak(Box::new(schema.clone()));

    let compiled_schema =
        jsonschema::JSONSchema::compile(schema_static).context("Failed to compile JSON schema")?;

    let validation_result = compiled_schema.validate(value);

    if let Err(errors) = validation_result {
        let error_messages: Vec<String> = errors.map(|e| e.to_string()).collect();
        return Err(anyhow!(
            "Output validation failed:\n{}",
            error_messages.join("\n")
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_direct_json() {
        let text = r#"{"status": "success", "value": 42}"#;
        let result = extract_json_from_text(text).unwrap();
        assert_eq!(result["status"], "success");
        assert_eq!(result["value"], 42);
    }

    #[test]
    fn test_extract_from_json_code_block() {
        let text = r#"
Here's the output:
```json
{
  "status": "success",
  "value": 42
}
```
"#;
        let result = extract_json_from_text(text).unwrap();
        assert_eq!(result["status"], "success");
        assert_eq!(result["value"], 42);
    }

    #[test]
    fn test_extract_from_generic_code_block() {
        let text = r#"
Here's the output:
```
{
  "status": "success",
  "value": 42
}
```
"#;
        let result = extract_json_from_text(text).unwrap();
        assert_eq!(result["status"], "success");
        assert_eq!(result["value"], 42);
    }

    #[test]
    fn test_validate_schema_success() {
        let value = json!({
            "status": "success",
            "changes": ["file1.py", "file2.py"]
        });

        let schema = json!({
            "type": "object",
            "required": ["status", "changes"],
            "properties": {
                "status": {
                    "type": "string",
                    "enum": ["success", "failure"]
                },
                "changes": {
                    "type": "array",
                    "items": { "type": "string" }
                }
            }
        });

        assert!(validate_against_schema(&value, &schema).is_ok());
    }

    #[test]
    fn test_validate_schema_failure() {
        let value = json!({
            "status": "invalid",
            "changes": ["file1.py"]
        });

        let schema = json!({
            "type": "object",
            "required": ["status", "changes"],
            "properties": {
                "status": {
                    "type": "string",
                    "enum": ["success", "failure"]
                },
                "changes": {
                    "type": "array",
                    "items": { "type": "string" }
                }
            }
        });

        assert!(validate_against_schema(&value, &schema).is_err());
    }
}
