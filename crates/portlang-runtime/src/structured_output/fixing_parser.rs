/// Character-by-character JSON fixer.
///
/// Handles the most common LLM output errors:
/// - Trailing commas in objects and arrays
/// - Single-quoted strings
/// - Unquoted object keys
/// - JS-style line (`//`) and block (`/* */`) comments
/// - Unterminated strings, objects, and arrays (closes them at EOF)
pub fn fix_json(input: &str) -> Result<serde_json::Value, String> {
    let fixed = fix_json_str(input)?;
    serde_json::from_str(&fixed).map_err(|e| format!("Still invalid after fixing: {}", e))
}

/// Returns the fixed JSON string without parsing it.
/// Useful for inspection and debugging.
pub fn fix_json_str(input: &str) -> Result<String, String> {
    let chars: Vec<char> = input.trim().chars().collect();
    let len = chars.len();
    let mut out = String::with_capacity(len + 32);
    let mut i = 0;
    let mut stack: Vec<char> = Vec::new(); // '{' or '['

    #[derive(PartialEq)]
    enum State {
        Normal,
        InString(char), // char = opening quote ('"' or '\'')
        LineComment,
        BlockComment,
    }

    let mut state = State::Normal;
    let mut escape_next = false;

    while i < len {
        let c = chars[i];

        if escape_next {
            out.push(c);
            escape_next = false;
            i += 1;
            continue;
        }

        match &state {
            State::InString(q) => {
                if c == '\\' {
                    // In single-quoted strings, \' is an escaped apostrophe.
                    // Emit just the ' (no backslash) since ' needs no escaping in JSON.
                    if *q == '\'' && i + 1 < len && chars[i + 1] == '\'' {
                        out.push('\'');
                        i += 2;
                        continue;
                    }
                    out.push(c);
                    escape_next = true;
                } else if c == *q {
                    out.push('"'); // always close with double-quote
                    state = State::Normal;
                } else if c == '"' && *q == '\'' {
                    // double-quote inside single-quoted string — escape it
                    out.push('\\');
                    out.push('"');
                } else {
                    out.push(c);
                }
            }

            State::LineComment => {
                if c == '\n' {
                    state = State::Normal;
                }
                // consume comment chars
            }

            State::BlockComment => {
                if c == '*' && i + 1 < len && chars[i + 1] == '/' {
                    i += 1; // consume '/'
                    state = State::Normal;
                }
                // consume comment chars
            }

            State::Normal => {
                // Check for comments
                if c == '/' && i + 1 < len {
                    if chars[i + 1] == '/' {
                        state = State::LineComment;
                        i += 2;
                        continue;
                    } else if chars[i + 1] == '*' {
                        state = State::BlockComment;
                        i += 2;
                        continue;
                    }
                }

                // Trailing comma: `,` immediately before `}` or `]`
                if c == ',' {
                    // Look ahead past whitespace
                    let mut j = i + 1;
                    while j < len && chars[j].is_whitespace() {
                        j += 1;
                    }
                    if j < len && (chars[j] == '}' || chars[j] == ']') {
                        // Skip the comma
                        i += 1;
                        continue;
                    }
                }

                // Opening string quotes
                if c == '"' {
                    out.push('"');
                    state = State::InString('"');
                    i += 1;
                    continue;
                }
                if c == '\'' {
                    out.push('"'); // open with double-quote
                    state = State::InString('\'');
                    i += 1;
                    continue;
                }

                // Unquoted object key: after `{` or `,` (with optional whitespace),
                // if we see an identifier character that isn't a JSON value start.
                // We only attempt this when the most-recently-opened container is `{`.
                if c.is_alphabetic() || c == '_' {
                    let in_object = stack.last() == Some(&'{');
                    if in_object {
                        // Check if this looks like an unquoted key (followed by `:`)
                        let key_start = i;
                        let mut j = i;
                        while j < len
                            && (chars[j].is_alphanumeric() || chars[j] == '_' || chars[j] == '-')
                        {
                            j += 1;
                        }
                        // Skip whitespace
                        let mut k = j;
                        while k < len && chars[k].is_whitespace() {
                            k += 1;
                        }
                        if k < len && chars[k] == ':' {
                            // It's an unquoted key — wrap it
                            let key: String = chars[key_start..j].iter().collect();
                            out.push('"');
                            out.push_str(&key);
                            out.push('"');
                            i = j;
                            continue;
                        }
                    }
                }

                // Track container stack
                if c == '{' || c == '[' {
                    stack.push(c);
                } else if c == '}' || c == ']' {
                    let _ = stack.pop();
                }

                out.push(c);
            }
        }

        i += 1;
    }

    // Close any unterminated string
    if let State::InString(_) = state {
        out.push('"');
    }

    // Close any unclosed containers (in reverse order)
    for opener in stack.iter().rev() {
        if *opener == '{' {
            out.push('}');
        } else {
            out.push(']');
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(s: &str) -> serde_json::Value {
        fix_json(s).unwrap_or_else(|_| panic!("fix_json failed for: {}", s))
    }

    // --- Already-valid input passes through unchanged ---

    #[test]
    fn valid_json_unchanged() {
        let v = parse(r#"{"a": 1, "b": true}"#);
        assert_eq!(v["a"], 1);
        assert_eq!(v["b"], true);
    }

    // --- Trailing commas ---

    #[test]
    fn trailing_comma_object() {
        let v = parse(r#"{"a": 1, "b": 2,}"#);
        assert_eq!(v["a"], 1);
        assert_eq!(v["b"], 2);
    }

    #[test]
    fn trailing_comma_array() {
        let v = parse(r#"[1, 2, 3,]"#);
        assert_eq!(v, json!([1, 2, 3]));
    }

    #[test]
    fn trailing_comma_with_whitespace() {
        let v = parse("{\"a\": 1  ,  \n  }");
        assert_eq!(v["a"], 1);
    }

    #[test]
    fn trailing_comma_nested() {
        let v = parse(r#"{"items": [1, 2,], "x": 3,}"#);
        assert_eq!(v["items"], json!([1, 2]));
        assert_eq!(v["x"], 3);
    }

    // --- Single-quoted strings ---

    #[test]
    fn single_quoted_values() {
        let v = parse(r#"{"name": 'Alice'}"#);
        assert_eq!(v["name"], "Alice");
    }

    #[test]
    fn single_quoted_keys_and_values() {
        let v = parse(r#"{'name': 'Alice'}"#);
        assert_eq!(v["name"], "Alice");
    }

    #[test]
    fn single_quoted_with_escaped_apostrophe() {
        let v = parse(r#"{"msg": 'it\'s fine'}"#);
        assert_eq!(v["msg"], "it's fine");
    }

    #[test]
    fn double_quote_inside_single_quoted_escaped() {
        let v = parse(r#"{"msg": 'say "hello"'}"#);
        assert_eq!(v["msg"], r#"say "hello""#);
    }

    // --- Unquoted keys ---

    #[test]
    fn unquoted_simple_key() {
        let v = parse(r#"{name: "Alice"}"#);
        assert_eq!(v["name"], "Alice");
    }

    #[test]
    fn unquoted_key_with_underscore() {
        let v = parse(r#"{first_name: "Alice"}"#);
        assert_eq!(v["first_name"], "Alice");
    }

    #[test]
    fn multiple_unquoted_keys() {
        let v = parse(r#"{a: 1, b: 2}"#);
        assert_eq!(v["a"], 1);
        assert_eq!(v["b"], 2);
    }

    // --- JS comments ---

    #[test]
    fn line_comment_removed() {
        let v = parse("{\"a\": 1 // this is a\n}");
        assert_eq!(v["a"], 1);
    }

    #[test]
    fn block_comment_removed() {
        let v = parse(r#"{"a": /* comment */ 1}"#);
        assert_eq!(v["a"], 1);
    }

    // --- Unterminated containers ---

    #[test]
    fn unterminated_object_closed() {
        let v = parse(r#"{"a": 1"#);
        assert_eq!(v["a"], 1);
    }

    #[test]
    fn unterminated_array_closed() {
        let v = parse(r#"[1, 2"#);
        assert_eq!(v, json!([1, 2]));
    }

    #[test]
    fn unterminated_string_closed() {
        let v = parse(r#"{"a": "hello"#);
        assert_eq!(v["a"], "hello");
    }

    #[test]
    fn nested_unterminated() {
        let v = parse(r#"{"a": {"b": 1"#);
        assert_eq!(v["a"]["b"], 1);
    }

    // --- Combined ---

    #[test]
    fn combined_fixes() {
        let input = "{name: 'Alice', items: [1, 2,], active: true,}";
        let v = parse(input);
        assert_eq!(v["name"], "Alice");
        assert_eq!(v["items"], json!([1, 2]));
        assert_eq!(v["active"], true);
    }
}
