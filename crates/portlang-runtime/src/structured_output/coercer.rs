/// Schema-aware value coercer with scoring.
///
/// Given a parsed JSON value and a JSON Schema, attempts to coerce the value
/// toward schema compliance using minimum-cost transformations.
///
/// Inspired by BAML's Schema-Aligned Parsing (SAP). Each correction carries a
/// cost; the total score reflects how much the LLM's output deviated from the
/// expected schema. Lower score = better match.
///
/// Score table:
///   0  — ExtraFieldIgnored       (free: strip unknown fields)
///   1  — SingleToArray           (scalar wrapped in array)
///   1  — NumberCoercion          (float↔int, string→number)
///   1  — BoolCoercion            ("true"/"false" string → bool)
///   1  — OptionalDefaultedNull   (absent optional field → null)
///   2  — EnumCaseInsensitive     (wrong-case enum value)
///   3  — EnumSubstringMatch      (enum value is substring of candidate)
/// 110  — NullForRequired         (required field missing — last resort)
use serde_json::{Map, Value};

pub const SCORE_EXTRA_FIELD: u32 = 0;
pub const SCORE_SINGLE_TO_ARRAY: u32 = 1;
pub const SCORE_NUMBER_COERCION: u32 = 1;
pub const SCORE_BOOL_COERCION: u32 = 1;
pub const SCORE_OPTIONAL_DEFAULT: u32 = 1;
pub const SCORE_ENUM_CASE: u32 = 2;
pub const SCORE_ENUM_SUBSTRING: u32 = 3;
pub const SCORE_NULL_REQUIRED: u32 = 110;

#[derive(Debug, Clone, PartialEq)]
pub enum Correction {
    ExtraFieldIgnored(String),
    SingleToArray,
    NumberCoercion { from: String, to: String },
    BoolCoercion(String),
    OptionalDefaultedNull(String),
    EnumCaseInsensitive { from: String, to: String },
    EnumSubstringMatch { from: String, to: String },
    NullForRequired(String),
}

impl Correction {
    pub fn score(&self) -> u32 {
        match self {
            Correction::ExtraFieldIgnored(_) => SCORE_EXTRA_FIELD,
            Correction::SingleToArray => SCORE_SINGLE_TO_ARRAY,
            Correction::NumberCoercion { .. } => SCORE_NUMBER_COERCION,
            Correction::BoolCoercion(_) => SCORE_BOOL_COERCION,
            Correction::OptionalDefaultedNull(_) => SCORE_OPTIONAL_DEFAULT,
            Correction::EnumCaseInsensitive { .. } => SCORE_ENUM_CASE,
            Correction::EnumSubstringMatch { .. } => SCORE_ENUM_SUBSTRING,
            Correction::NullForRequired(_) => SCORE_NULL_REQUIRED,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CoercedValue {
    pub value: Value,
    pub score: u32,
    pub corrections: Vec<Correction>,
}

impl CoercedValue {
    fn perfect(value: Value) -> Self {
        Self {
            value,
            score: 0,
            corrections: vec![],
        }
    }

    fn with(value: Value, corrections: Vec<Correction>) -> Self {
        let score = corrections.iter().map(|c| c.score()).sum();
        Self {
            value,
            score,
            corrections,
        }
    }
}

/// Attempt to coerce `value` to match `schema`.
/// Returns `Ok(CoercedValue)` on success (possibly with corrections),
/// or `Err(String)` if the value cannot be reconciled with the schema.
pub fn coerce(value: &Value, schema: &Value) -> Result<CoercedValue, String> {
    coerce_inner(value, schema, "<root>")
}

fn coerce_inner(value: &Value, schema: &Value, path: &str) -> Result<CoercedValue, String> {
    // Handle oneOf / anyOf — try each branch, pick lowest score
    if let Some(branches) = schema.get("oneOf").or_else(|| schema.get("anyOf")) {
        if let Some(arr) = branches.as_array() {
            return coerce_union(value, arr, path);
        }
    }

    // Handle allOf — must satisfy all branches
    if let Some(arr) = schema.get("allOf").and_then(|v| v.as_array()) {
        return coerce_all_of(value, arr, path);
    }

    let schema_type = schema.get("type").and_then(|v| v.as_str());

    match schema_type {
        Some("object") => coerce_object(value, schema, path),
        Some("array") => coerce_array(value, schema, path),
        Some("string") => coerce_string(value, schema, path),
        Some("integer") | Some("number") => {
            coerce_number(value, schema, schema_type.unwrap(), path)
        }
        Some("boolean") => coerce_bool(value, path),
        Some("null") => coerce_null(value, path),
        // No type constraint — accept as-is
        None => Ok(CoercedValue::perfect(value.clone())),
        Some(t) => Err(format!("{}: unsupported schema type '{}'", path, t)),
    }
}

// ---------------------------------------------------------------------------
// Object coercion
// ---------------------------------------------------------------------------

fn coerce_object(value: &Value, schema: &Value, path: &str) -> Result<CoercedValue, String> {
    let obj = match value.as_object() {
        Some(o) => o,
        None => {
            return Err(format!(
                "{}: expected object, got {}",
                path,
                value_type(value)
            ))
        }
    };

    let properties = schema
        .get("properties")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();

    let required: Vec<String> = schema
        .get("required")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let additional_properties_allowed = schema
        .get("additionalProperties")
        .map(|v| v.as_bool().unwrap_or(true))
        .unwrap_or(true);

    let mut result = Map::new();
    let mut all_corrections = Vec::new();

    // Coerce each declared property
    for (key, prop_schema) in &properties {
        let field_path = format!("{}.{}", path, key);
        if let Some(field_value) = obj.get(key) {
            let coerced = coerce_inner(field_value, prop_schema, &field_path)?;
            all_corrections.extend(coerced.corrections);
            result.insert(key.clone(), coerced.value);
        } else {
            // Field absent — check if optional
            let is_required = required.contains(key);
            if is_required {
                all_corrections.push(Correction::NullForRequired(key.clone()));
                result.insert(key.clone(), Value::Null);
            } else {
                all_corrections.push(Correction::OptionalDefaultedNull(key.clone()));
                result.insert(key.clone(), Value::Null);
            }
        }
    }

    // Handle extra fields
    for key in obj.keys() {
        if !properties.contains_key(key) {
            if additional_properties_allowed || properties.is_empty() {
                // Pass through extra fields when additionalProperties is not explicitly false
                // or when there are no declared properties (schema is loose)
                result.insert(key.clone(), obj[key].clone());
            } else {
                all_corrections.push(Correction::ExtraFieldIgnored(key.clone()));
            }
        }
    }

    Ok(CoercedValue::with(Value::Object(result), all_corrections))
}

// ---------------------------------------------------------------------------
// Array coercion
// ---------------------------------------------------------------------------

fn coerce_array(value: &Value, schema: &Value, path: &str) -> Result<CoercedValue, String> {
    let item_schema = schema.get("items");

    // If value is not an array, wrap it (SingleToArray)
    if !value.is_array() {
        let wrapped = Value::Array(vec![value.clone()]);
        let mut corrections = vec![Correction::SingleToArray];

        if let Some(item_sch) = item_schema {
            let coerced_item = coerce_inner(value, item_sch, &format!("{}[0]", path))?;
            corrections.extend(coerced_item.corrections);
            let final_array = Value::Array(vec![coerced_item.value]);
            return Ok(CoercedValue::with(final_array, corrections));
        }

        return Ok(CoercedValue::with(wrapped, corrections));
    }

    let arr = value.as_array().unwrap();
    let mut result = Vec::new();
    let mut all_corrections = Vec::new();

    for (idx, item) in arr.iter().enumerate() {
        let item_path = format!("{}[{}]", path, idx);
        if let Some(item_sch) = item_schema {
            let coerced = coerce_inner(item, item_sch, &item_path)?;
            all_corrections.extend(coerced.corrections);
            result.push(coerced.value);
        } else {
            result.push(item.clone());
        }
    }

    Ok(CoercedValue::with(Value::Array(result), all_corrections))
}

// ---------------------------------------------------------------------------
// String / enum coercion
// ---------------------------------------------------------------------------

fn coerce_string(value: &Value, schema: &Value, path: &str) -> Result<CoercedValue, String> {
    let enum_values: Option<Vec<String>> =
        schema.get("enum").and_then(|v| v.as_array()).map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        });

    // Convert value to string candidate
    let candidate = match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => {
            return Err(format!(
                "{}: cannot coerce {} to string",
                path,
                value_type(value)
            ))
        }
    };

    if let Some(variants) = enum_values {
        match_enum(&candidate, &variants, path)
    } else {
        Ok(CoercedValue::perfect(Value::String(candidate)))
    }
}

fn match_enum(candidate: &str, variants: &[String], path: &str) -> Result<CoercedValue, String> {
    // Exact match
    if variants.contains(&candidate.to_string()) {
        return Ok(CoercedValue::perfect(Value::String(candidate.to_string())));
    }

    // Case-insensitive match
    let lower = candidate.to_lowercase();
    for v in variants {
        if v.to_lowercase() == lower {
            return Ok(CoercedValue::with(
                Value::String(v.clone()),
                vec![Correction::EnumCaseInsensitive {
                    from: candidate.to_string(),
                    to: v.clone(),
                }],
            ));
        }
    }

    // Substring: candidate contains a variant
    for v in variants {
        if lower.contains(&v.to_lowercase()) {
            return Ok(CoercedValue::with(
                Value::String(v.clone()),
                vec![Correction::EnumSubstringMatch {
                    from: candidate.to_string(),
                    to: v.clone(),
                }],
            ));
        }
    }

    // Substring: a variant contains the candidate
    for v in variants {
        if v.to_lowercase().contains(&lower) {
            return Ok(CoercedValue::with(
                Value::String(v.clone()),
                vec![Correction::EnumSubstringMatch {
                    from: candidate.to_string(),
                    to: v.clone(),
                }],
            ));
        }
    }

    Err(format!(
        "{}: '{}' does not match any enum variant: {:?}",
        path, candidate, variants
    ))
}

// ---------------------------------------------------------------------------
// Number coercion
// ---------------------------------------------------------------------------

fn coerce_number(
    value: &Value,
    _schema: &Value,
    schema_type: &str,
    path: &str,
) -> Result<CoercedValue, String> {
    match value {
        Value::Number(n) => {
            if schema_type == "integer" {
                if let Some(i) = n.as_i64() {
                    return Ok(CoercedValue::perfect(Value::Number(i.into())));
                }
                // Float → int
                if let Some(f) = n.as_f64() {
                    let i = f as i64;
                    return Ok(CoercedValue::with(
                        Value::Number(i.into()),
                        vec![Correction::NumberCoercion {
                            from: f.to_string(),
                            to: i.to_string(),
                        }],
                    ));
                }
            }
            Ok(CoercedValue::perfect(value.clone()))
        }
        Value::String(s) => {
            if let Ok(n) = s.parse::<i64>() {
                Ok(CoercedValue::with(
                    Value::Number(n.into()),
                    vec![Correction::NumberCoercion {
                        from: s.clone(),
                        to: n.to_string(),
                    }],
                ))
            } else if let Ok(f) = s.parse::<f64>() {
                let n = serde_json::Number::from_f64(f)
                    .ok_or_else(|| format!("{}: cannot represent {} as JSON number", path, f))?;
                Ok(CoercedValue::with(
                    Value::Number(n),
                    vec![Correction::NumberCoercion {
                        from: s.clone(),
                        to: f.to_string(),
                    }],
                ))
            } else {
                Err(format!("{}: cannot coerce '{}' to number", path, s))
            }
        }
        _ => Err(format!(
            "{}: cannot coerce {} to number",
            path,
            value_type(value)
        )),
    }
}

// ---------------------------------------------------------------------------
// Bool coercion
// ---------------------------------------------------------------------------

fn coerce_bool(value: &Value, path: &str) -> Result<CoercedValue, String> {
    match value {
        Value::Bool(_) => Ok(CoercedValue::perfect(value.clone())),
        Value::String(s) => match s.to_lowercase().as_str() {
            "true" | "yes" | "1" => Ok(CoercedValue::with(
                Value::Bool(true),
                vec![Correction::BoolCoercion(s.clone())],
            )),
            "false" | "no" | "0" => Ok(CoercedValue::with(
                Value::Bool(false),
                vec![Correction::BoolCoercion(s.clone())],
            )),
            _ => Err(format!("{}: cannot coerce '{}' to bool", path, s)),
        },
        Value::Number(n) => {
            if n.as_i64() == Some(1) {
                Ok(CoercedValue::with(
                    Value::Bool(true),
                    vec![Correction::BoolCoercion("1".into())],
                ))
            } else if n.as_i64() == Some(0) {
                Ok(CoercedValue::with(
                    Value::Bool(false),
                    vec![Correction::BoolCoercion("0".into())],
                ))
            } else {
                Err(format!("{}: cannot coerce {} to bool", path, n))
            }
        }
        _ => Err(format!(
            "{}: cannot coerce {} to bool",
            path,
            value_type(value)
        )),
    }
}

// ---------------------------------------------------------------------------
// Null coercion
// ---------------------------------------------------------------------------

fn coerce_null(value: &Value, path: &str) -> Result<CoercedValue, String> {
    if value.is_null() {
        Ok(CoercedValue::perfect(Value::Null))
    } else {
        Err(format!(
            "{}: expected null, got {}",
            path,
            value_type(value)
        ))
    }
}

// ---------------------------------------------------------------------------
// Union coercion (oneOf / anyOf)
// ---------------------------------------------------------------------------

fn coerce_union(value: &Value, branches: &[Value], path: &str) -> Result<CoercedValue, String> {
    let mut best: Option<CoercedValue> = None;

    for branch in branches {
        if let Ok(coerced) = coerce_inner(value, branch, path) {
            // Short-circuit on perfect match
            if coerced.score == 0 {
                return Ok(coerced);
            }
            let is_better = best.as_ref().is_none_or(|b| coerced.score < b.score);
            if is_better {
                best = Some(coerced);
            }
        }
    }

    best.ok_or_else(|| format!("{}: value matches no branch of union schema", path))
}

// ---------------------------------------------------------------------------
// allOf coercion
// ---------------------------------------------------------------------------

fn coerce_all_of(value: &Value, branches: &[Value], path: &str) -> Result<CoercedValue, String> {
    let mut current = CoercedValue::perfect(value.clone());
    for branch in branches {
        let coerced = coerce_inner(&current.value, branch, path)?;
        current.score += coerced.score;
        current.corrections.extend(coerced.corrections);
        current.value = coerced.value;
    }
    Ok(current)
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn value_type(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema(s: serde_json::Value) -> Value {
        s
    }

    // --- Perfect matches (score 0) ---

    #[test]
    fn perfect_match_score_zero() {
        let v = json!({"name": "Alice", "age": 30});
        let s = schema(json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer"}
            },
            "required": ["name", "age"]
        }));
        let result = coerce(&v, &s).unwrap();
        assert_eq!(result.score, 0);
        assert!(result.corrections.is_empty());
    }

    // --- Array coercion ---

    #[test]
    fn single_value_wrapped_in_array() {
        let v = json!("hello");
        let s = schema(json!({"type": "array", "items": {"type": "string"}}));
        let result = coerce(&v, &s).unwrap();
        assert_eq!(result.value, json!(["hello"]));
        assert_eq!(result.score, SCORE_SINGLE_TO_ARRAY);
        assert!(result.corrections.contains(&Correction::SingleToArray));
    }

    #[test]
    fn array_items_coerced() {
        let v = json!(["1", "2", "3"]);
        let s = schema(json!({"type": "array", "items": {"type": "integer"}}));
        let result = coerce(&v, &s).unwrap();
        assert_eq!(result.value, json!([1, 2, 3]));
        assert_eq!(result.corrections.len(), 3);
    }

    // --- Number coercion ---

    #[test]
    fn float_to_int() {
        let v = json!(3.7);
        let s = schema(json!({"type": "integer"}));
        let result = coerce(&v, &s).unwrap();
        assert_eq!(result.value, json!(3));
        assert_eq!(result.score, SCORE_NUMBER_COERCION);
    }

    #[test]
    fn string_number_to_int() {
        let v = json!("42");
        let s = schema(json!({"type": "integer"}));
        let result = coerce(&v, &s).unwrap();
        assert_eq!(result.value, json!(42));
    }

    // --- Bool coercion ---

    #[test]
    fn string_true_to_bool() {
        let v = json!("true");
        let s = schema(json!({"type": "boolean"}));
        let result = coerce(&v, &s).unwrap();
        assert_eq!(result.value, json!(true));
        assert_eq!(result.score, SCORE_BOOL_COERCION);
    }

    #[test]
    fn string_false_to_bool() {
        let v = json!("false");
        let s = schema(json!({"type": "boolean"}));
        let result = coerce(&v, &s).unwrap();
        assert_eq!(result.value, json!(false));
    }

    // --- Enum matching ---

    #[test]
    fn enum_exact_match_score_zero() {
        let v = json!("success");
        let s = schema(json!({"type": "string", "enum": ["success", "failure"]}));
        let result = coerce(&v, &s).unwrap();
        assert_eq!(result.value, json!("success"));
        assert_eq!(result.score, 0);
    }

    #[test]
    fn enum_case_insensitive() {
        let v = json!("SUCCESS");
        let s = schema(json!({"type": "string", "enum": ["success", "failure"]}));
        let result = coerce(&v, &s).unwrap();
        assert_eq!(result.value, json!("success"));
        assert_eq!(result.score, SCORE_ENUM_CASE);
        assert!(matches!(
            &result.corrections[0],
            Correction::EnumCaseInsensitive { from, to } if from == "SUCCESS" && to == "success"
        ));
    }

    #[test]
    fn enum_substring_match() {
        let v = json!("it was a success overall");
        let s = schema(json!({"type": "string", "enum": ["success", "failure"]}));
        let result = coerce(&v, &s).unwrap();
        assert_eq!(result.value, json!("success"));
        assert_eq!(result.score, SCORE_ENUM_SUBSTRING);
    }

    #[test]
    fn enum_no_match_fails() {
        let v = json!("unknown");
        let s = schema(json!({"type": "string", "enum": ["success", "failure"]}));
        assert!(coerce(&v, &s).is_err());
    }

    // --- Object: missing fields ---

    #[test]
    fn missing_optional_field_defaulted_null() {
        let v = json!({"name": "Alice"});
        let s = schema(json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer"}
            },
            "required": ["name"]
        }));
        let result = coerce(&v, &s).unwrap();
        assert_eq!(result.value["name"], "Alice");
        assert_eq!(result.value["age"], Value::Null);
        assert_eq!(result.score, SCORE_OPTIONAL_DEFAULT);
    }

    #[test]
    fn missing_required_field_gets_null_with_high_score() {
        let v = json!({});
        let s = schema(json!({
            "type": "object",
            "required": ["name"],
            "properties": {"name": {"type": "string"}}
        }));
        let result = coerce(&v, &s).unwrap();
        assert_eq!(result.value["name"], Value::Null);
        assert_eq!(result.score, SCORE_NULL_REQUIRED);
    }

    // --- Object: extra fields ---

    #[test]
    fn extra_fields_stripped_when_additional_false() {
        let v = json!({"name": "Alice", "unexpected": "x"});
        let s = schema(json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {"name": {"type": "string"}},
            "required": ["name"]
        }));
        let result = coerce(&v, &s).unwrap();
        assert!(!result.value.as_object().unwrap().contains_key("unexpected"));
        assert!(result
            .corrections
            .contains(&Correction::ExtraFieldIgnored("unexpected".into())));
        assert_eq!(result.score, SCORE_EXTRA_FIELD); // free
    }

    // --- Union selection ---

    #[test]
    fn union_picks_lowest_score() {
        // value matches "string" perfectly but "integer" only via coercion
        let v = json!("hello");
        let s = schema(json!({
            "oneOf": [
                {"type": "integer"},
                {"type": "string"}
            ]
        }));
        let result = coerce(&v, &s).unwrap();
        assert_eq!(result.value, json!("hello"));
        assert_eq!(result.score, 0);
    }

    #[test]
    fn union_fails_if_no_branch_matches() {
        let v = json!({"nested": true});
        let s = schema(json!({
            "oneOf": [
                {"type": "string"},
                {"type": "integer"}
            ]
        }));
        assert!(coerce(&v, &s).is_err());
    }

    // --- Nested object ---

    #[test]
    fn nested_object_coercion() {
        let v = json!({"user": {"age": "25"}});
        let s = schema(json!({
            "type": "object",
            "properties": {
                "user": {
                    "type": "object",
                    "properties": {"age": {"type": "integer"}},
                    "required": ["age"]
                }
            },
            "required": ["user"]
        }));
        let result = coerce(&v, &s).unwrap();
        assert_eq!(result.value["user"]["age"], 25);
        assert_eq!(result.score, SCORE_NUMBER_COERCION);
    }

    // --- Type mismatch fails cleanly ---

    #[test]
    fn object_where_string_expected_fails() {
        let v = json!({"x": 1});
        let s = schema(json!({"type": "string"}));
        assert!(coerce(&v, &s).is_err());
    }
}
