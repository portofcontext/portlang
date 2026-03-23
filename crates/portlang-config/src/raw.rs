use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Marker type that only deserializes from the string "inherit"
#[derive(Debug, Clone)]
pub struct InheritKeyword;

impl<'de> Deserialize<'de> for InheritKeyword {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        if s == "inherit" {
            Ok(InheritKeyword)
        } else {
            Err(serde::de::Error::custom(format!(
                "expected \"inherit\", got {:?}",
                s
            )))
        }
    }
}

impl Serialize for InheritKeyword {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str("inherit")
    }
}

/// Either the string "inherit" or a concrete value T
#[derive(Debug, Clone, Serialize)]
pub enum InheritOr<T> {
    Inherit(InheritKeyword),
    Value(T),
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for InheritOr<T> {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        use serde::de::{self, value::MapAccessDeserializer, MapAccess, Visitor};
        use std::fmt;
        use std::marker::PhantomData;

        struct InheritOrVisitor<T>(PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for InheritOrVisitor<T> {
            type Value = InheritOr<T>;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, r#""inherit" or a configuration value"#)
            }

            fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Self::Value, E> {
                if v == "inherit" {
                    Ok(InheritOr::Inherit(InheritKeyword))
                } else {
                    Err(E::custom(format!("expected \"inherit\", got {:?}", v)))
                }
            }

            fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<Self::Value, E> {
                self.visit_str(&v)
            }

            fn visit_map<A: MapAccess<'de>>(
                self,
                map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                T::deserialize(MapAccessDeserializer::new(map)).map(InheritOr::Value)
            }

            fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<Self::Value, E> {
                T::deserialize(de::value::BoolDeserializer::new(v)).map(InheritOr::Value)
            }
        }

        deserializer.deserialize_any(InheritOrVisitor(PhantomData))
    }
}

impl<T> InheritOr<T> {
    pub fn is_inherit(&self) -> bool {
        matches!(self, InheritOr::Inherit(_))
    }

    pub fn into_value(self) -> Option<T> {
        match self {
            InheritOr::Value(v) => Some(v),
            InheritOr::Inherit(_) => None,
        }
    }
}

/// Code mode configuration (extended thinking / future settings)
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawCodeMode {
    #[serde(default)]
    pub enabled: bool,
}

/// A single [[skill]] entry in a .field file
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawSkill {
    /// Source string: `owner/repo`, `github:owner/repo`, `clawhub:name`, `./local.md`, etc.
    pub source: String,
}

/// Raw TOML structure for a parent field.toml (suite-level template).
/// Intentionally lenient — ignores unknown fields so a full field.toml
/// can also serve as a parent config without error.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawParentConfig {
    #[serde(default)]
    pub model: Option<RawModel>,
    #[serde(default)]
    pub code_mode: Option<RawCodeMode>,
    #[serde(default)]
    pub tool: Vec<RawTool>,
    #[serde(default)]
    pub skill: Vec<RawSkill>,
    #[serde(default)]
    pub boundary: Option<RawBoundary>,
    #[serde(default)]
    pub environment: Option<RawEnvironment>,
}

/// Declaration of a template variable in [vars]
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawVarDecl {
    /// Whether this variable is required (default: true)
    #[serde(default = "default_true")]
    pub required: bool,

    /// Default value if not supplied at runtime
    #[serde(default)]
    pub default: Option<String>,

    /// Human-readable description
    #[serde(default)]
    pub description: Option<String>,
}

/// Raw TOML field structure (before validation)
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawField {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Either `model = "inherit"` or `[model]\nname = "..."`
    #[serde(default)]
    pub model: Option<InheritOr<RawModel>>,
    pub prompt: RawPrompt,
    #[serde(default)]
    pub environment: Option<RawEnvironment>,
    /// Either `boundary = "inherit"` or `[boundary]\n...`
    #[serde(default)]
    pub boundary: Option<InheritOr<RawBoundary>>,
    /// `[[tool]]` array of tables
    #[serde(default)]
    pub tool: Vec<RawTool>,
    /// `tools = "inherit"` — inherit parent's [[tool]] list
    #[serde(default)]
    pub tools: Option<InheritKeyword>,
    /// `[[skill]]` array of tables
    #[serde(default)]
    pub skill: Vec<RawSkill>,
    /// `skills = "inherit"` — inherit parent's [[skill]] list
    #[serde(default)]
    pub skills: Option<InheritKeyword>,
    /// Either `code_mode = "inherit"` or `[code_mode]\nenabled = true`
    #[serde(default)]
    pub code_mode: Option<InheritOr<RawCodeMode>>,
    #[serde(default)]
    pub verifier: Vec<RawVerifier>,
    /// Template variable declarations: [vars] section
    #[serde(default)]
    pub vars: HashMap<String, RawVarDecl>,
}

fn deserialize_output_schema<'de, D>(deserializer: D) -> Result<Option<serde_json::Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    match Option::<String>::deserialize(deserializer)? {
        None => Ok(None),
        Some(s) => serde_json::from_str(&s)
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawModel {
    pub name: String,
    #[serde(default)]
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawPrompt {
    pub goal: String,
    #[serde(default)]
    pub system: Option<String>,
    #[serde(default)]
    pub re_observation: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawEnvironment {
    #[serde(default = "default_workspace_root")]
    pub root: String,
    #[serde(default)]
    pub packages: Vec<String>,
    #[serde(default)]
    pub dockerfile: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub code_mode_enabled: Option<InheritOr<bool>>,
}

fn default_workspace_root() -> String {
    "./workspace".to_string()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawBoundary {
    #[serde(default)]
    pub allow_write: Vec<String>,
    #[serde(default)]
    pub network: Option<String>,
    #[serde(default)]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub max_cost: Option<StringOrNumber>,
    #[serde(default)]
    pub max_steps: Option<u64>,
    #[serde(default = "default_true")]
    pub bash: bool,
    #[serde(default, deserialize_with = "deserialize_output_schema")]
    pub output_schema: Option<serde_json::Value>,
    #[serde(default)]
    pub collect: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawVerifier {
    pub name: String,
    /// Algorithm type: "shell" (default), "levenshtein", "semantic", "tool_call"
    #[serde(rename = "type", default = "default_verifier_type")]
    pub verifier_type: String,
    // Shell fields
    #[serde(default)]
    pub command: Option<String>,
    // Built-in verifier fields
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub expected: Option<String>,
    #[serde(default)]
    pub threshold: Option<f64>,
    #[serde(default)]
    pub embedding_url: Option<String>,
    #[serde(default)]
    pub embedding_model: Option<String>,
    // tool_call verifier fields
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub field: Option<String>,
    #[serde(default)]
    pub matches: Option<String>,
    #[serde(default)]
    pub not_matches: Option<String>,
    // Common fields
    #[serde(default)]
    pub trigger: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub eval_only: bool,
}

fn default_verifier_type() -> String {
    "shell".to_string()
}

/// Helper type for parsing cost as either string ("$2.00") or number (2.0)
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum StringOrNumber {
    String(String),
    Number(f64),
}

/// Unified tool definition from TOML (covers python, shell, mcp)
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawTool {
    #[serde(rename = "type")]
    pub tool_type: String,

    // Common fields
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_schema")]
    pub input_schema: Option<serde_json::Value>,
    #[serde(default, deserialize_with = "deserialize_optional_schema")]
    pub output_schema: Option<serde_json::Value>,

    // Python fields
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub function: Option<String>,

    // Shell fields
    #[serde(default)]
    pub command: Option<String>,

    // MCP fields
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub transport: Option<String>,
    #[serde(default)]
    pub include_tools: Option<Vec<String>>,
    #[serde(default)]
    pub exclude_tools: Option<Vec<String>>,
    #[serde(default)]
    pub patch_file: Option<String>,
}

fn deserialize_optional_schema<'de, D>(
    deserializer: D,
) -> Result<Option<serde_json::Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum SchemaOrString {
        String(String),
        Value(serde_json::Value),
    }

    match Option::<SchemaOrString>::deserialize(deserializer)? {
        None => Ok(None),
        Some(SchemaOrString::String(s)) => {
            // Parse JSON string
            serde_json::from_str(&s)
                .map(Some)
                .map_err(serde::de::Error::custom)
        }
        Some(SchemaOrString::Value(v)) => Ok(Some(v)),
    }
}
