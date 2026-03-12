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
    /// JSON structure validation, optionally against a JSON Schema
    Json {
        /// Workspace-relative path to the file to validate.
        /// When omitted, the run's structured output (output_schema) is used.
        #[serde(default)]
        file: Option<String>,
        /// Optional JSON Schema (as a JSON value) to validate structure
        #[serde(default)]
        schema: Option<serde_json::Value>,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum VerifierTrigger {
    /// Run after every action
    Always,
    /// Run only when agent stops
    #[default]
    OnStop,
    /// Run after specific tool calls
    OnWrite,
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
