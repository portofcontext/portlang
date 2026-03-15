use std::collections::HashMap;
use std::path::PathBuf;

/// Runtime-supplied overrides passed to run/converge/eval/check commands.
/// Carries template variable bindings and optional input data.
#[derive(Debug, Clone, Default)]
pub struct RuntimeContext {
    /// Template variable bindings: variable name → value string.
    /// Populated from --var key=value flags or --vars file.json.
    pub vars: HashMap<String, String>,

    /// Optional input data to stage into the workspace before the agent starts.
    pub input: Option<InputSource>,
}

/// Source of input data to inject into the agent workspace
#[derive(Debug, Clone)]
pub enum InputSource {
    /// Path to a file to copy into the workspace root
    File(PathBuf),
    /// Inline string (JSON or other content) to write to portlang_input.json
    Inline(String),
}
