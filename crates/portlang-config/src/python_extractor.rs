use anyhow::{anyhow, Result};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::Path;
use tree_sitter::{Node, Parser};

use crate::ty_resolver_hybrid::{is_custom_class, TyResolverHybrid};

/// Metadata extracted from a Python function
#[derive(Debug, Clone, PartialEq)]
pub struct PythonToolMetadata {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
}

/// Parsed docstring information
#[derive(Debug, Default)]
struct ParsedDocstring {
    summary: String,
    params: HashMap<String, String>,
    #[allow(dead_code)] // Reserved for future use
    returns: Option<String>,
}

/// Python tool metadata extractor using tree-sitter + ty semantic analysis
pub struct PythonToolExtractor {
    parser: Parser,
    ty_resolver: Option<TyResolverHybrid>,
}

impl PythonToolExtractor {
    /// Create a new extractor
    pub fn new() -> Result<Self> {
        let mut parser = Parser::new();
        let language = tree_sitter_python::LANGUAGE.into();
        parser
            .set_language(&language)
            .map_err(|e| anyhow!("Failed to set language: {}", e))?;
        Ok(Self {
            parser,
            ty_resolver: None,
        })
    }

    /// Extract tools from source code with full semantic type resolution
    ///
    /// This uses ty's semantic analysis engine with vendored typeshed stubs to:
    /// - Resolve cross-file imports
    /// - Expand type aliases
    /// - Handle forward references
    /// - Support advanced typing features (Annotated, TypeVar, Generic)
    pub fn extract_tools_from_source(&mut self, source: &str) -> Result<Vec<PythonToolMetadata>> {
        // Create TyResolverHybrid with full semantic analysis
        let resolver = TyResolverHybrid::new(Path::new("<source>"), source)?;
        self.ty_resolver = Some(resolver);

        // Parse the source
        let tree = self
            .parser
            .parse(source, None)
            .ok_or_else(|| anyhow!("Failed to parse Python source"))?;

        let mut tools = Vec::new();
        let root = tree.root_node();

        // Find all function definitions at module level
        let mut cursor = root.walk();
        for node in root.children(&mut cursor) {
            if node.kind() == "function_definition" {
                if let Some(tool) = self.extract_function_metadata(&node, source)? {
                    tools.push(tool);
                }
            }
        }

        Ok(tools)
    }

    /// Extract all tool definitions from a Python file
    pub fn extract_tools(&mut self, path: &Path) -> Result<Vec<PythonToolMetadata>> {
        let source = std::fs::read_to_string(path)
            .map_err(|e| anyhow!("Failed to read file {}: {}", path.display(), e))?;

        let tree = self
            .parser
            .parse(&source, None)
            .ok_or_else(|| anyhow!("Failed to parse Python file"))?;

        let mut tools = Vec::new();
        let root = tree.root_node();

        // Find all function definitions at module level
        let mut cursor = root.walk();
        for node in root.children(&mut cursor) {
            if node.kind() == "function_definition" {
                if let Some(tool) = self.extract_function_metadata(&node, &source)? {
                    tools.push(tool);
                }
            }
        }

        Ok(tools)
    }

    /// Extract a specific function by name
    pub fn extract_function(
        &mut self,
        path: &Path,
        function_name: &str,
    ) -> Result<PythonToolMetadata> {
        let tools = self.extract_tools(path)?;
        tools
            .into_iter()
            .find(|t| t.name == function_name)
            .ok_or_else(|| {
                anyhow!(
                    "Function '{}' not found in {}",
                    function_name,
                    path.display()
                )
            })
    }

    /// Extract metadata from a single function definition
    fn extract_function_metadata(
        &mut self,
        node: &Node,
        source: &str,
    ) -> Result<Option<PythonToolMetadata>> {
        // Get function name
        let name_node = node
            .child_by_field_name("name")
            .ok_or_else(|| anyhow!("Function missing name"))?;
        let name = &source[name_node.byte_range()];

        // Skip private functions (start with _)
        if name.starts_with('_') {
            return Ok(None);
        }

        // Get docstring
        let docstring = self.extract_docstring(node, source);

        // Get parameters with type hints
        let params = node
            .child_by_field_name("parameters")
            .ok_or_else(|| anyhow!("Function missing parameters"))?;
        let input_schema = self.extract_input_schema(&params, source, &docstring)?;

        // Get return type hint
        let output_schema = node
            .child_by_field_name("return_type")
            .and_then(|rt| self.extract_output_schema(&rt, source).ok());

        Ok(Some(PythonToolMetadata {
            name: name.to_string(),
            description: if docstring.summary.is_empty() {
                None
            } else {
                Some(docstring.summary)
            },
            input_schema,
            output_schema,
        }))
    }

    /// Extract docstring and parse parameter descriptions
    fn extract_docstring(&self, func_node: &Node, source: &str) -> ParsedDocstring {
        let body = match func_node.child_by_field_name("body") {
            Some(b) => b,
            None => return ParsedDocstring::default(),
        };

        let first_stmt = match body.named_child(0) {
            Some(s) => s,
            None => return ParsedDocstring::default(),
        };

        if first_stmt.kind() == "expression_statement" {
            if let Some(expr) = first_stmt.named_child(0) {
                if expr.kind() == "string" {
                    let docstring_raw = &source[expr.byte_range()];
                    // Remove quotes (handles ''', """, ', ")
                    let docstring = docstring_raw
                        .trim_start_matches("\"\"\"")
                        .trim_start_matches("'''")
                        .trim_start_matches('"')
                        .trim_start_matches('\'')
                        .trim_end_matches("\"\"\"")
                        .trim_end_matches("'''")
                        .trim_end_matches('"')
                        .trim_end_matches('\'');
                    return parse_docstring(docstring);
                }
            }
        }

        ParsedDocstring::default()
    }

    /// Convert Python parameters to JSON Schema
    fn extract_input_schema(
        &mut self,
        params: &Node,
        source: &str,
        docstring: &ParsedDocstring,
    ) -> Result<Value> {
        let mut properties = Map::new();
        let mut required = Vec::new();

        let mut cursor = params.walk();
        for param in params.named_children(&mut cursor) {
            // Handle different parameter types
            let (param_name, type_hint, has_default) = match param.kind() {
                "typed_parameter" | "default_parameter" | "typed_default_parameter" => {
                    // Try both "name" and get first named child (identifier)
                    let name_node = param.child_by_field_name("name").or_else(|| {
                        // Fallback: first child is usually the identifier
                        param.named_child(0).filter(|n| n.kind() == "identifier")
                    });

                    let name = name_node.map(|n| &source[n.byte_range()]).ok_or_else(|| {
                        anyhow!(
                            "Parameter missing name (kind: {}, text: {})",
                            param.kind(),
                            &source[param.byte_range()]
                        )
                    })?;

                    let type_hint = param
                        .child_by_field_name("type")
                        .or_else(|| {
                            // Fallback: look for type annotation node
                            // In default_parameter, structure is often: identifier, type, =, value
                            param.named_children(&mut param.walk()).find(|n| {
                                // Look for type nodes (not identifier, not value)
                                ![
                                    "identifier",
                                    ":",
                                    "=",
                                    "integer",
                                    "float",
                                    "string",
                                    "true",
                                    "false",
                                    "none",
                                ]
                                .contains(&n.kind())
                                    && n.start_position().column
                                        > param
                                            .named_child(0)
                                            .map(|c| c.start_position().column)
                                            .unwrap_or(0)
                            })
                        })
                        .map(|t| &source[t.byte_range()]);

                    let has_default = param.kind() == "default_parameter"
                        || param.kind() == "typed_default_parameter"
                        || param.child_by_field_name("value").is_some()
                        || param
                            .named_children(&mut param.walk())
                            .any(|n| n.kind() == "=");

                    (name, type_hint, has_default)
                }
                "identifier" => {
                    let name = &source[param.byte_range()];
                    (name, None, false)
                }
                _ => continue,
            };

            // Skip self/cls parameters
            if param_name == "self" || param_name == "cls" {
                continue;
            }

            // Convert Python type to JSON Schema
            let mut json_type = self.python_type_to_json_schema(type_hint)?;

            // Add description from docstring if available
            if let Some(param_desc) = docstring.params.get(param_name) {
                if let Some(obj) = json_type.as_object_mut() {
                    obj.insert("description".to_string(), json!(param_desc));
                }
            }

            properties.insert(param_name.to_string(), json_type);

            if !has_default {
                required.push(param_name.to_string());
            }
        }

        Ok(json!({
            "type": "object",
            "properties": properties,
            "required": required,
        }))
    }

    /// Convert Python type hint to JSON Schema type
    fn python_type_to_json_schema(&mut self, type_hint: Option<&str>) -> Result<Value> {
        let type_hint = match type_hint {
            Some(t) => t.trim(),
            None => return Ok(json!({})), // No type hint - accept anything
        };

        // Try TyResolver first for custom classes (Pydantic models, TypedDict, etc.)
        if is_custom_class(type_hint) {
            if let Some(resolver) = &mut self.ty_resolver {
                if let Ok(schema) = resolver.resolve_type(type_hint) {
                    return Ok(schema);
                }
                // If resolution fails, fall through to generic object
            }
        }

        // Handle primitive types
        match type_hint {
            "str" => return Ok(json!({"type": "string"})),
            "int" => return Ok(json!({"type": "integer"})),
            "float" => return Ok(json!({"type": "number"})),
            "bool" => return Ok(json!({"type": "boolean"})),
            "dict" | "Dict" => return Ok(json!({"type": "object"})),
            "list" | "List" => return Ok(json!({"type": "array"})),
            "Any" => return Ok(json!({})),
            "None" => return Ok(json!({"type": "null"})),
            _ => {}
        }

        // Handle List[T]
        if let Some(inner) = self.extract_generic_param(type_hint, "List") {
            let items = self.python_type_to_json_schema(Some(inner))?;
            return Ok(json!({"type": "array", "items": items}));
        }

        // Handle list[T] (lowercase)
        if let Some(inner) = self.extract_generic_param(type_hint, "list") {
            let items = self.python_type_to_json_schema(Some(inner))?;
            return Ok(json!({"type": "array", "items": items}));
        }

        // Handle Dict[K, V]
        if let Some(params) = self.extract_generic_params(type_hint, "Dict") {
            if params.len() == 2 {
                let value_type = self.python_type_to_json_schema(Some(&params[1]))?;
                return Ok(json!({
                    "type": "object",
                    "additionalProperties": value_type
                }));
            }
            return Ok(json!({"type": "object"}));
        }

        // Handle dict[K, V] (lowercase)
        if let Some(params) = self.extract_generic_params(type_hint, "dict") {
            if params.len() == 2 {
                let value_type = self.python_type_to_json_schema(Some(&params[1]))?;
                return Ok(json!({
                    "type": "object",
                    "additionalProperties": value_type
                }));
            }
            return Ok(json!({"type": "object"}));
        }

        // Handle Optional[T]
        if let Some(inner) = self.extract_generic_param(type_hint, "Optional") {
            let mut schema = self.python_type_to_json_schema(Some(inner))?;
            // Make it nullable
            if let Some(obj) = schema.as_object_mut() {
                if let Some(type_val) = obj.get("type").cloned() {
                    obj.insert("type".to_string(), json!([type_val, "null"]));
                }
            }
            return Ok(schema);
        }

        // Handle Union[T1, T2, ...]
        if let Some(params) = self.extract_generic_params(type_hint, "Union") {
            let mut any_of = Vec::new();
            for param in params {
                any_of.push(self.python_type_to_json_schema(Some(&param))?);
            }
            return Ok(json!({"anyOf": any_of}));
        }

        // Handle Literal["value1", "value2"]
        if let Some(values) = self.extract_literal_values(type_hint) {
            return Ok(json!({"enum": values}));
        }

        // Handle TypedDict, Pydantic models, dataclass - treat as object
        // We can't introspect these without running Python, so just use object
        if type_hint.contains("TypedDict") || type_hint.contains("BaseModel") {
            return Ok(json!({"type": "object"}));
        }

        // If type hint looks like a class name (starts with uppercase letter),
        // treat it as a custom class/object type
        if type_hint.chars().next().map_or(false, |c| c.is_uppercase()) {
            return Ok(json!({"type": "object"}));
        }

        // Unknown type - accept anything
        Ok(json!({}))
    }

    fn extract_output_schema(&mut self, return_type: &Node, source: &str) -> Result<Value> {
        let type_str = &source[return_type.byte_range()];
        self.python_type_to_json_schema(Some(type_str))
    }

    /// Extract single generic parameter: List[str] -> "str"
    fn extract_generic_param<'a>(&self, type_hint: &'a str, generic_name: &str) -> Option<&'a str> {
        let prefix = format!("{}[", generic_name);
        type_hint
            .strip_prefix(&prefix)
            .and_then(|s| s.strip_suffix(']'))
            .map(|s| s.trim())
    }

    /// Extract multiple generic parameters: Dict[str, int] -> ["str", "int"]
    fn extract_generic_params(&self, type_hint: &str, generic_name: &str) -> Option<Vec<String>> {
        let prefix = format!("{}[", generic_name);
        let inner = type_hint.strip_prefix(&prefix)?.strip_suffix(']')?;

        // Split by comma, but respect nested brackets
        let params = self.split_generic_params(inner);
        Some(params)
    }

    /// Split generic parameters respecting nested brackets
    fn split_generic_params(&self, params: &str) -> Vec<String> {
        let mut result = Vec::new();
        let mut current = String::new();
        let mut bracket_depth = 0;

        for ch in params.chars() {
            match ch {
                '[' => {
                    bracket_depth += 1;
                    current.push(ch);
                }
                ']' => {
                    bracket_depth -= 1;
                    current.push(ch);
                }
                ',' if bracket_depth == 0 => {
                    result.push(current.trim().to_string());
                    current.clear();
                }
                _ => current.push(ch),
            }
        }

        if !current.is_empty() {
            result.push(current.trim().to_string());
        }

        result
    }

    /// Extract Literal values: Literal["a", "b"] -> ["a", "b"]
    fn extract_literal_values(&self, type_hint: &str) -> Option<Vec<String>> {
        let inner = type_hint.strip_prefix("Literal[")?.strip_suffix(']')?;

        // Split by comma and remove quotes
        let values = inner
            .split(',')
            .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
            .collect();

        Some(values)
    }
}

/// Parse Google-style or NumPy-style docstrings
fn parse_docstring(docstring: &str) -> ParsedDocstring {
    let mut summary = String::new();
    let mut params = HashMap::new();
    let mut returns = None;

    let lines: Vec<&str> = docstring.lines().collect();
    let mut i = 0;

    // Extract summary (first paragraph)
    while i < lines.len() {
        let line = lines[i].trim();
        if line.is_empty() {
            break;
        }
        if !summary.is_empty() {
            summary.push(' ');
        }
        summary.push_str(line);
        i += 1;
    }

    // Parse Args/Arguments section
    while i < lines.len() {
        let line = lines[i].trim();

        if line.starts_with("Args:")
            || line.starts_with("Arguments:")
            || line.starts_with("Parameters:")
        {
            i += 1;
            while i < lines.len() {
                let param_line = lines[i].trim();

                // Check for next section
                if param_line.starts_with("Returns:")
                    || param_line.starts_with("Raises:")
                    || param_line.starts_with("Yields:")
                {
                    break;
                }

                // Parse "param_name: description" or "param_name (type): description"
                if let Some((name_part, desc)) = param_line.split_once(':') {
                    let name = name_part
                        .split_whitespace()
                        .next()
                        .unwrap_or(name_part)
                        .trim();
                    if !name.is_empty() {
                        params.insert(name.to_string(), desc.trim().to_string());
                    }
                }

                i += 1;
            }
            continue;
        }

        if line.starts_with("Returns:") || line.starts_with("Return:") {
            i += 1;
            let mut return_desc = String::new();
            while i < lines.len() {
                let return_line = lines[i].trim();
                if return_line.starts_with("Raises:") || return_line.starts_with("Args:") {
                    break;
                }
                if !return_desc.is_empty() {
                    return_desc.push(' ');
                }
                return_desc.push_str(return_line);
                i += 1;
            }
            returns = Some(return_desc);
            continue;
        }

        i += 1;
    }

    ParsedDocstring {
        summary,
        params,
        returns,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_temp_python_file(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file
    }

    #[test]
    fn test_simple_function() {
        let code = r#"
def greet(name: str) -> str:
    """Greet a person by name."""
    return f"Hello, {name}!"
"#;
        let file = create_temp_python_file(code);
        let mut extractor = PythonToolExtractor::new().unwrap();
        let tools = extractor.extract_tools(file.path()).unwrap();

        assert_eq!(tools.len(), 1);
        let tool = &tools[0];
        assert_eq!(tool.name, "greet");
        assert_eq!(
            tool.description,
            Some("Greet a person by name.".to_string())
        );
        assert_eq!(
            tool.input_schema,
            json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string"}
                },
                "required": ["name"]
            })
        );
        assert_eq!(tool.output_schema, Some(json!({"type": "string"})));
    }

    #[test]
    fn test_function_with_default_params() {
        let code = r#"
def calculate(x: int, y: int = 10, multiplier: float = 1.0) -> float:
    """Calculate a value."""
    return (x + y) * multiplier
"#;
        let file = create_temp_python_file(code);
        let mut extractor = PythonToolExtractor::new().unwrap();
        let tools = extractor.extract_tools(file.path()).unwrap();

        assert_eq!(tools.len(), 1);
        let tool = &tools[0];
        assert_eq!(
            tool.input_schema["required"],
            json!(["x"]) // Only x is required
        );
        assert_eq!(tool.input_schema["properties"]["y"]["type"], "integer");
        assert_eq!(
            tool.input_schema["properties"]["multiplier"]["type"],
            "number"
        );
    }

    #[test]
    fn test_list_type() {
        let code = r#"
from typing import List

def process_items(items: List[str]) -> List[int]:
    """Process a list of items."""
    return [len(item) for item in items]
"#;
        let file = create_temp_python_file(code);
        let mut extractor = PythonToolExtractor::new().unwrap();
        let tools = extractor.extract_tools(file.path()).unwrap();

        let tool = &tools[0];
        assert_eq!(
            tool.input_schema["properties"]["items"],
            json!({
                "type": "array",
                "items": {"type": "string"}
            })
        );
        assert_eq!(
            tool.output_schema,
            Some(json!({
                "type": "array",
                "items": {"type": "integer"}
            }))
        );
    }

    #[test]
    fn test_dict_type() {
        let code = r#"
from typing import Dict

def get_config(overrides: Dict[str, int]) -> Dict[str, str]:
    """Get configuration."""
    return {}
"#;
        let file = create_temp_python_file(code);
        let mut extractor = PythonToolExtractor::new().unwrap();
        let tools = extractor.extract_tools(file.path()).unwrap();

        let tool = &tools[0];
        assert_eq!(
            tool.input_schema["properties"]["overrides"],
            json!({
                "type": "object",
                "additionalProperties": {"type": "integer"}
            })
        );
        assert_eq!(
            tool.output_schema,
            Some(json!({
                "type": "object",
                "additionalProperties": {"type": "string"}
            }))
        );
    }

    #[test]
    fn test_optional_type() {
        let code = r#"
from typing import Optional

def find_user(user_id: int) -> Optional[str]:
    """Find a user by ID."""
    return None
"#;
        let file = create_temp_python_file(code);
        let mut extractor = PythonToolExtractor::new().unwrap();
        let tools = extractor.extract_tools(file.path()).unwrap();

        let tool = &tools[0];
        assert_eq!(
            tool.output_schema,
            Some(json!({
                "type": ["string", "null"]
            }))
        );
    }

    #[test]
    fn test_union_type() {
        let code = r#"
from typing import Union

def process(data: Union[str, int, float]) -> Union[bool, str]:
    """Process data."""
    return True
"#;
        let file = create_temp_python_file(code);
        let mut extractor = PythonToolExtractor::new().unwrap();
        let tools = extractor.extract_tools(file.path()).unwrap();

        let tool = &tools[0];
        assert_eq!(
            tool.input_schema["properties"]["data"],
            json!({
                "anyOf": [
                    {"type": "string"},
                    {"type": "integer"},
                    {"type": "number"}
                ]
            })
        );
    }

    #[test]
    fn test_literal_type() {
        let code = r#"
from typing import Literal

def set_mode(mode: Literal["fast", "slow", "medium"]) -> str:
    """Set processing mode."""
    return mode
"#;
        let file = create_temp_python_file(code);
        let mut extractor = PythonToolExtractor::new().unwrap();
        let tools = extractor.extract_tools(file.path()).unwrap();

        let tool = &tools[0];
        assert_eq!(
            tool.input_schema["properties"]["mode"],
            json!({
                "enum": ["fast", "slow", "medium"]
            })
        );
    }

    #[test]
    fn test_nested_generics() {
        let code = r#"
from typing import List, Dict

def complex_data(items: List[Dict[str, int]]) -> Dict[str, List[float]]:
    """Handle complex nested data."""
    return {}
"#;
        let file = create_temp_python_file(code);
        let mut extractor = PythonToolExtractor::new().unwrap();
        let tools = extractor.extract_tools(file.path()).unwrap();

        let tool = &tools[0];
        assert_eq!(
            tool.input_schema["properties"]["items"],
            json!({
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": {"type": "integer"}
                }
            })
        );
    }

    #[test]
    fn test_google_style_docstring() {
        let code = r#"
def process_order(order_id: int, priority: str) -> bool:
    """Process an order from the queue.

    Args:
        order_id: The unique identifier for the order
        priority: Priority level (high, medium, low)

    Returns:
        True if order was processed successfully
    """
    return True
"#;
        let file = create_temp_python_file(code);
        let mut extractor = PythonToolExtractor::new().unwrap();
        let tools = extractor.extract_tools(file.path()).unwrap();

        let tool = &tools[0];
        assert_eq!(
            tool.description,
            Some("Process an order from the queue.".to_string())
        );
        assert_eq!(
            tool.input_schema["properties"]["order_id"]["description"],
            "The unique identifier for the order"
        );
        assert_eq!(
            tool.input_schema["properties"]["priority"]["description"],
            "Priority level (high, medium, low)"
        );
    }

    #[test]
    fn test_no_type_hints() {
        let code = r#"
def legacy_function(x, y):
    """A function without type hints."""
    return x + y
"#;
        let file = create_temp_python_file(code);
        let mut extractor = PythonToolExtractor::new().unwrap();
        let tools = extractor.extract_tools(file.path()).unwrap();

        let tool = &tools[0];
        assert_eq!(tool.name, "legacy_function");
        // Should accept any type for parameters without hints
        assert_eq!(tool.input_schema["properties"]["x"], json!({}));
        assert_eq!(tool.input_schema["properties"]["y"], json!({}));
        assert_eq!(tool.output_schema, None);
    }

    #[test]
    fn test_skip_private_functions() {
        let code = r#"
def public_function() -> str:
    """Public function."""
    return "public"

def _private_function() -> str:
    """Private function."""
    return "private"

def __dunder_function__() -> str:
    """Dunder function."""
    return "dunder"
"#;
        let file = create_temp_python_file(code);
        let mut extractor = PythonToolExtractor::new().unwrap();
        let tools = extractor.extract_tools(file.path()).unwrap();

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "public_function");
    }

    #[test]
    fn test_multiple_functions() {
        let code = r#"
def function_a(x: int) -> int:
    """First function."""
    return x

def function_b(y: str) -> str:
    """Second function."""
    return y

def function_c(z: bool) -> bool:
    """Third function."""
    return z
"#;
        let file = create_temp_python_file(code);
        let mut extractor = PythonToolExtractor::new().unwrap();
        let tools = extractor.extract_tools(file.path()).unwrap();

        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0].name, "function_a");
        assert_eq!(tools[1].name, "function_b");
        assert_eq!(tools[2].name, "function_c");
    }

    #[test]
    fn test_lowercase_generics() {
        let code = r#"
def process(items: list[str]) -> dict[str, int]:
    """Process items (Python 3.9+ syntax)."""
    return {}
"#;
        let file = create_temp_python_file(code);
        let mut extractor = PythonToolExtractor::new().unwrap();
        let tools = extractor.extract_tools(file.path()).unwrap();

        let tool = &tools[0];
        assert_eq!(
            tool.input_schema["properties"]["items"],
            json!({
                "type": "array",
                "items": {"type": "string"}
            })
        );
        assert_eq!(
            tool.output_schema,
            Some(json!({
                "type": "object",
                "additionalProperties": {"type": "integer"}
            }))
        );
    }

    #[test]
    fn test_any_type() {
        let code = r#"
from typing import Any

def flexible(data: Any) -> Any:
    """Accepts and returns any type."""
    return data
"#;
        let file = create_temp_python_file(code);
        let mut extractor = PythonToolExtractor::new().unwrap();
        let tools = extractor.extract_tools(file.path()).unwrap();

        let tool = &tools[0];
        assert_eq!(tool.input_schema["properties"]["data"], json!({}));
        assert_eq!(tool.output_schema, Some(json!({})));
    }

    #[test]
    fn test_pydantic_model() {
        let code = r#"
from pydantic import BaseModel

class UserModel(BaseModel):
    name: str
    age: int

def create_user(user: UserModel) -> UserModel:
    """Create a user from Pydantic model."""
    return user
"#;
        let file = create_temp_python_file(code);
        let mut extractor = PythonToolExtractor::new().unwrap();
        let tools = extractor.extract_tools(file.path()).unwrap();

        let tool = &tools[0];
        // Pydantic models are treated as generic objects
        assert_eq!(
            tool.input_schema["properties"]["user"],
            json!({"type": "object"})
        );
    }

    #[test]
    fn test_typed_dict() {
        let code = r#"
from typing import TypedDict

class UserDict(TypedDict):
    name: str
    age: int

def process_user(user: UserDict) -> UserDict:
    """Process a TypedDict user."""
    return user
"#;
        let file = create_temp_python_file(code);
        let mut extractor = PythonToolExtractor::new().unwrap();
        let tools = extractor.extract_tools(file.path()).unwrap();

        let tool = &tools[0];
        // TypedDicts are treated as generic objects
        assert_eq!(
            tool.input_schema["properties"]["user"],
            json!({"type": "object"})
        );
    }

    #[test]
    fn test_extract_specific_function() {
        let code = r#"
def function_a() -> str:
    return "a"

def function_b() -> str:
    return "b"

def function_c() -> str:
    return "c"
"#;
        let file = create_temp_python_file(code);
        let mut extractor = PythonToolExtractor::new().unwrap();
        let tool = extractor
            .extract_function(file.path(), "function_b")
            .unwrap();

        assert_eq!(tool.name, "function_b");
    }

    #[test]
    fn test_extract_specific_function_not_found() {
        let code = r#"
def function_a() -> str:
    return "a"
"#;
        let file = create_temp_python_file(code);
        let mut extractor = PythonToolExtractor::new().unwrap();
        let result = extractor.extract_function(file.path(), "function_b");

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }
}
