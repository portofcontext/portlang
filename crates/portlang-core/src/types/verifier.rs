use serde::{Deserialize, Serialize};

/// The algorithm a verifier uses to evaluate agent output
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum VerifierAlgorithm {
    /// Shell command: exit code 0 = pass (default)
    Shell { command: String },
    /// Normalized Levenshtein edit distance against a reference string
    Levenshtein {
        /// Workspace-relative path to the file containing actual output.
        /// When omitted, the run's structured output (output_schema) is used.
        #[serde(default)]
        file: Option<String>,
        /// Reference string to compare against
        expected: String,
        /// Minimum normalized similarity [0.0, 1.0] required to pass (default: 1.0)
        #[serde(default = "default_levenshtein_threshold")]
        threshold: f64,
    },
    /// Tool call inspection.
    ///
    /// With `on_tool:<name>` trigger: evaluates the current tool call's
    /// `{input: {...}, output: "..."}` context using a JSON pointer + regex.
    ///
    /// With `on_stop` trigger: scans the full action history to assert a tool
    /// was actually called (catches hallucination). `tool` names the tool to
    /// look for; `field`/`matches`/`not_matches` optionally constrain it.
    ToolCall {
        /// Tool name to require in history (only used with `on_stop`).
        #[serde(default)]
        tool: Option<String>,
        /// JSON pointer (RFC 6901) into `{input: {...}, output: "..."}`,
        /// e.g. `/input/path` or `/output`
        #[serde(default)]
        field: Option<String>,
        /// Regex the field value must match to pass
        #[serde(default)]
        matches: Option<String>,
        /// Regex the field value must NOT match to pass
        #[serde(default)]
        not_matches: Option<String>,
    },
    /// Cosine similarity of embeddings from an OpenAI-compatible embeddings API
    Semantic {
        /// Workspace-relative path to the file containing actual output.
        /// When omitted, the run's structured output (output_schema) is used.
        #[serde(default)]
        file: Option<String>,
        /// Reference string to embed and compare against
        expected: String,
        /// Minimum cosine similarity [0.0, 1.0] required to pass (default: 0.8)
        #[serde(default = "default_semantic_threshold")]
        threshold: f64,
        /// Embeddings endpoint URL (default: https://api.openai.com/v1/embeddings)
        #[serde(default)]
        embedding_url: Option<String>,
        /// Embeddings model name (default: text-embedding-3-small)
        #[serde(default)]
        embedding_model: Option<String>,
    },
}

fn default_levenshtein_threshold() -> f64 {
    1.0
}

fn default_semantic_threshold() -> f64 {
    0.8
}

/// Verifier configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Verifier {
    /// Name of the verifier
    pub name: String,

    /// Algorithm used to evaluate the output
    #[serde(flatten)]
    pub algorithm: VerifierAlgorithm,

    /// When to trigger this verifier
    #[serde(default)]
    pub trigger: VerifierTrigger,

    /// Human-readable description of what this verifier checks
    #[serde(default)]
    pub description: Option<String>,
}

/// When to trigger a verifier
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum VerifierTrigger {
    /// Run after every action
    Always,
    /// Run only when agent stops
    #[default]
    OnStop,
    /// Run after any call to a specific tool (e.g. `on_tool:bash`)
    OnTool(String),
}

impl serde::Serialize for VerifierTrigger {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            VerifierTrigger::Always => s.serialize_str("always"),
            VerifierTrigger::OnStop => s.serialize_str("on_stop"),
            VerifierTrigger::OnTool(name) => s.serialize_str(&format!("on_tool:{}", name)),
        }
    }
}

impl<'de> serde::Deserialize<'de> for VerifierTrigger {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        match raw.as_str() {
            "always" => Ok(VerifierTrigger::Always),
            "on_stop" => Ok(VerifierTrigger::OnStop),
            s if s.starts_with("on_tool:") => Ok(VerifierTrigger::OnTool(s[8..].to_string())),
            _ => Err(serde::de::Error::custom(format!(
                "unknown trigger '{}'. Must be 'always', 'on_stop', or 'on_tool:<tool_name>'",
                raw
            ))),
        }
    }
}

/// Result of running a verifier
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerifierResult {
    /// Name of the verifier that ran
    pub name: String,

    /// Whether the verifier passed
    pub passed: bool,

    /// The command that was executed (shell verifiers only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    /// Standard output from the verifier
    pub stdout: String,

    /// Standard error from the verifier
    pub stderr: String,

    /// Exit code (shell verifiers) or 0/1 for built-in verifiers
    pub exit_code: i32,

    /// The JSON Schema used for validation (json verifiers only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_json::Value>,
}

impl VerifierResult {
    pub fn new(name: String, passed: bool, stdout: String, stderr: String, exit_code: i32) -> Self {
        Self {
            name,
            passed,
            command: None,
            stdout,
            stderr,
            exit_code,
            schema: None,
        }
    }

    pub fn with_command(
        name: String,
        passed: bool,
        command: String,
        stdout: String,
        stderr: String,
        exit_code: i32,
    ) -> Self {
        Self {
            name,
            passed,
            command: Some(command),
            stdout,
            stderr,
            exit_code,
            schema: None,
        }
    }

    pub fn with_schema(mut self, schema: serde_json::Value) -> Self {
        self.schema = Some(schema);
        self
    }
}
