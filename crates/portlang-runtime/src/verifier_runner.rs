use crate::sandbox::Sandbox;
use portlang_core::{
    Action, TrajectoryStep, Verifier, VerifierAlgorithm, VerifierResult, VerifierTrigger,
};
use strsim::normalized_levenshtein;

/// Run verifiers based on trigger conditions.
///
/// `structured_output` is the validated JSON from `output_schema`, if any.
///
/// `tool_context` is `{input: {...}, output: "..."}` for the current tool call,
/// used by `tool_call` verifiers with `on_tool:<name>` triggers.
///
/// `action_history` is all completed steps so far, used by `tool_call` verifiers
/// with `on_stop` triggers to assert a tool was actually called.
pub async fn run_verifiers(
    sandbox: &dyn Sandbox,
    verifiers: &[Verifier],
    action: &Action,
    is_stop: bool,
    structured_output: Option<&serde_json::Value>,
    tool_context: Option<&serde_json::Value>,
    action_history: &[TrajectoryStep],
) -> Vec<VerifierResult> {
    let mut results = Vec::new();

    for verifier in verifiers {
        let should_run = match &verifier.trigger {
            VerifierTrigger::Always => true,
            VerifierTrigger::OnStop => is_stop,
            VerifierTrigger::OnTool(tool_name) => {
                action.tool_name().map(|t| t.as_str()) == Some(tool_name.as_str())
            }
        };

        if should_run {
            let result = run_verifier(
                sandbox,
                verifier,
                structured_output,
                tool_context,
                action_history,
            )
            .await;
            results.push(result);
        }
    }

    results
}

/// Dispatch to the appropriate verifier implementation
async fn run_verifier(
    sandbox: &dyn Sandbox,
    verifier: &Verifier,
    structured_output: Option<&serde_json::Value>,
    tool_context: Option<&serde_json::Value>,
    action_history: &[TrajectoryStep],
) -> VerifierResult {
    match &verifier.algorithm {
        VerifierAlgorithm::Shell { command } => {
            run_shell_verifier(sandbox, verifier, command).await
        }
        VerifierAlgorithm::ToolCall {
            tool,
            field,
            matches,
            not_matches,
        } => run_tool_call_verifier(
            verifier,
            tool.as_deref(),
            field.as_deref(),
            matches.as_deref(),
            not_matches.as_deref(),
            tool_context,
            action_history,
        ),
        VerifierAlgorithm::Levenshtein {
            file,
            expected,
            threshold,
        } => {
            run_levenshtein_verifier(
                sandbox,
                verifier,
                file.as_deref(),
                expected,
                *threshold,
                structured_output,
            )
            .await
        }
        VerifierAlgorithm::Semantic {
            file,
            expected,
            threshold,
            embedding_url,
            embedding_model,
        } => {
            run_semantic_verifier(
                sandbox,
                verifier,
                file.as_deref(),
                expected,
                *threshold,
                embedding_url.as_deref(),
                embedding_model.as_deref(),
                structured_output,
            )
            .await
        }
    }
}

// ---------------------------------------------------------------------------
// Shell verifier
// ---------------------------------------------------------------------------

async fn run_shell_verifier(
    sandbox: &dyn Sandbox,
    verifier: &Verifier,
    command: &str,
) -> VerifierResult {
    match sandbox.run_command(command).await {
        Ok(output) => VerifierResult::with_command(
            verifier.name.clone(),
            output.success,
            command.to_string(),
            output.stdout,
            output.stderr,
            output.exit_code,
        ),
        Err(e) => VerifierResult::with_command(
            verifier.name.clone(),
            false,
            command.to_string(),
            String::new(),
            format!("Failed to run verifier: {}", e),
            -1,
        ),
    }
}

// ---------------------------------------------------------------------------
// Tool call verifier
// ---------------------------------------------------------------------------

fn run_tool_call_verifier(
    verifier: &Verifier,
    tool: Option<&str>,
    field: Option<&str>,
    matches: Option<&str>,
    not_matches: Option<&str>,
    tool_context: Option<&serde_json::Value>,
    action_history: &[TrajectoryStep],
) -> VerifierResult {
    // on_stop mode: scan history to assert a tool was actually called
    if tool_context.is_none() {
        return run_tool_call_history_verifier(
            verifier,
            tool,
            field,
            matches,
            not_matches,
            action_history,
        );
    }

    // on_tool mode: inspect the current tool call's context
    let ctx = tool_context.unwrap();
    check_tool_context(verifier, ctx, field, matches, not_matches)
}

/// on_stop mode: scan action history to assert a matching tool call occurred.
fn run_tool_call_history_verifier(
    verifier: &Verifier,
    tool: Option<&str>,
    field: Option<&str>,
    matches: Option<&str>,
    not_matches: Option<&str>,
    action_history: &[TrajectoryStep],
) -> VerifierResult {
    let required_tool = match tool {
        Some(t) => t,
        None => {
            return VerifierResult::new(
                verifier.name.clone(),
                false,
                String::new(),
                "tool_call verifier with on_stop trigger requires a 'tool' field".to_string(),
                1,
            );
        }
    };

    let candidates: Vec<&TrajectoryStep> = action_history
        .iter()
        .filter(|step| {
            matches!(
                &step.action,
                Action::ToolCall { tool, .. } if tool.as_str() == required_tool
            )
        })
        .collect();

    if candidates.is_empty() {
        return VerifierResult::new(
            verifier.name.clone(),
            false,
            String::new(),
            format!("tool '{}' was never called", required_tool),
            1,
        );
    }

    // If no field constraint, presence alone is sufficient
    if field.is_none() && matches.is_none() && not_matches.is_none() {
        return VerifierResult::new(
            verifier.name.clone(),
            true,
            format!(
                "tool '{}' was called {} time(s)",
                required_tool,
                candidates.len()
            ),
            String::new(),
            0,
        );
    }

    // Check whether any call satisfies the field constraints
    for step in &candidates {
        if let Action::ToolCall { input, .. } = &step.action {
            let ctx = serde_json::json!({ "input": input, "output": step.result });
            let result = check_tool_context(verifier, &ctx, field, matches, not_matches);
            if result.passed {
                return result;
            }
        }
    }

    let field_desc = field.unwrap_or("/input");
    VerifierResult::new(
        verifier.name.clone(),
        false,
        String::new(),
        format!(
            "tool '{}' was called {} time(s) but no call matched the field constraints on '{}'",
            required_tool,
            candidates.len(),
            field_desc
        ),
        1,
    )
}

/// Shared: evaluate field/matches/not_matches against a tool context object.
fn check_tool_context(
    verifier: &Verifier,
    ctx: &serde_json::Value,
    field: Option<&str>,
    matches: Option<&str>,
    not_matches: Option<&str>,
) -> VerifierResult {
    let field_ptr = match field {
        Some(f) => f,
        None => {
            // No field constraint — context presence is enough
            return VerifierResult::new(
                verifier.name.clone(),
                true,
                "tool call observed".to_string(),
                String::new(),
                0,
            );
        }
    };

    let value = match ctx.pointer(field_ptr) {
        Some(v) => v,
        None => {
            return VerifierResult::new(
                verifier.name.clone(),
                false,
                String::new(),
                format!("field '{}' not found in tool context", field_ptr),
                1,
            );
        }
    };

    let text = match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };

    if let Some(pattern) = matches {
        match regex::Regex::new(pattern) {
            Ok(re) => {
                if !re.is_match(&text) {
                    return VerifierResult::new(
                        verifier.name.clone(),
                        false,
                        String::new(),
                        format!(
                            "field '{}' value {:?} does not match /{}/",
                            field_ptr, text, pattern
                        ),
                        1,
                    );
                }
            }
            Err(e) => {
                return VerifierResult::new(
                    verifier.name.clone(),
                    false,
                    String::new(),
                    format!("invalid regex in 'matches': {}", e),
                    1,
                );
            }
        }
    }

    if let Some(pattern) = not_matches {
        match regex::Regex::new(pattern) {
            Ok(re) => {
                if re.is_match(&text) {
                    return VerifierResult::new(
                        verifier.name.clone(),
                        false,
                        String::new(),
                        format!(
                            "field '{}' value {:?} matches forbidden pattern /{}/",
                            field_ptr, text, pattern
                        ),
                        1,
                    );
                }
            }
            Err(e) => {
                return VerifierResult::new(
                    verifier.name.clone(),
                    false,
                    String::new(),
                    format!("invalid regex in 'not_matches': {}", e),
                    1,
                );
            }
        }
    }

    VerifierResult::new(
        verifier.name.clone(),
        true,
        format!("field '{}' = {:?}", field_ptr, text),
        String::new(),
        0,
    )
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve the text content to verify: read from file, or serialize structured output.
/// Returns `Err` if neither is available.
async fn resolve_text_content(
    sandbox: &dyn Sandbox,
    file: Option<&str>,
    structured_output: Option<&serde_json::Value>,
) -> Result<String, String> {
    if let Some(path) = file {
        read_workspace_file(sandbox, path).await
    } else if let Some(output) = structured_output {
        Ok(serde_json::to_string_pretty(output).unwrap_or_else(|_| output.to_string()))
    } else {
        Err("No 'file' specified and no structured output available. Add 'file' or define 'output_schema' in [boundary].".to_string())
    }
}

/// Read a workspace file via the sandbox, returning its contents or an error message.
async fn read_workspace_file(sandbox: &dyn Sandbox, file: &str) -> Result<String, String> {
    let cmd = format!("cat {}", shell_quote(file));
    match sandbox.run_command(&cmd).await {
        Ok(output) if output.success => Ok(output.stdout),
        Ok(output) => Err(format!(
            "Could not read '{}': {}",
            file,
            if output.stderr.is_empty() {
                output.stdout
            } else {
                output.stderr
            }
        )),
        Err(e) => Err(format!("Could not read '{}': {}", file, e)),
    }
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

// ---------------------------------------------------------------------------
// Levenshtein verifier
// ---------------------------------------------------------------------------

async fn run_levenshtein_verifier(
    sandbox: &dyn Sandbox,
    verifier: &Verifier,
    file: Option<&str>,
    expected: &str,
    threshold: f64,
    structured_output: Option<&serde_json::Value>,
) -> VerifierResult {
    let actual = match resolve_text_content(sandbox, file, structured_output).await {
        Ok(content) => content,
        Err(e) => {
            return VerifierResult::new(verifier.name.clone(), false, String::new(), e, 1);
        }
    };

    let score = normalized_levenshtein(actual.trim(), expected.trim());
    let passed = score >= threshold;
    let stdout = format!(
        "Levenshtein similarity: {:.4} (threshold: {:.4})",
        score, threshold
    );
    let stderr = if passed {
        String::new()
    } else {
        format!(
            "Similarity {:.4} is below threshold {:.4}",
            score, threshold
        )
    };

    VerifierResult::new(
        verifier.name.clone(),
        passed,
        stdout,
        stderr,
        if passed { 0 } else { 1 },
    )
}

// ---------------------------------------------------------------------------
// Semantic verifier (embeddings + cosine similarity)
// ---------------------------------------------------------------------------

async fn run_semantic_verifier(
    sandbox: &dyn Sandbox,
    verifier: &Verifier,
    file: Option<&str>,
    expected: &str,
    threshold: f64,
    embedding_url: Option<&str>,
    embedding_model: Option<&str>,
    structured_output: Option<&serde_json::Value>,
) -> VerifierResult {
    let actual = match resolve_text_content(sandbox, file, structured_output).await {
        Ok(content) => content,
        Err(e) => {
            return VerifierResult::new(verifier.name.clone(), false, String::new(), e, 1);
        }
    };

    // No embedding_url → use local fastembed model (no API key required).
    // embedding_url set → use that OpenAI-compatible HTTP endpoint.
    let score_result = if let Some(url) = embedding_url {
        let api_key = std::env::var("EMBEDDING_API_KEY")
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .unwrap_or_default();

        if api_key.is_empty() {
            return VerifierResult::new(
                verifier.name.clone(),
                false,
                String::new(),
                "embedding_url is set but no EMBEDDING_API_KEY or OPENAI_API_KEY found in environment".to_string(),
                1,
            );
        }

        let model = embedding_model.unwrap_or("text-embedding-3-small");
        get_remote_embeddings_score(&actual, expected, url, model, &api_key).await
    } else {
        let model = embedding_model.map(|s| s.to_string());
        get_local_embeddings_score(actual.clone(), expected.to_string(), model).await
    };

    match score_result {
        Ok(score) => {
            let passed = score >= threshold;
            let stdout = format!(
                "Semantic similarity: {:.4} (threshold: {:.4})",
                score, threshold
            );
            let stderr = if passed {
                String::new()
            } else {
                format!(
                    "Similarity {:.4} is below threshold {:.4}",
                    score, threshold
                )
            };
            VerifierResult::new(
                verifier.name.clone(),
                passed,
                stdout,
                stderr,
                if passed { 0 } else { 1 },
            )
        }
        Err(e) => VerifierResult::new(
            verifier.name.clone(),
            false,
            String::new(),
            format!("Semantic verifier error: {}", e),
            1,
        ),
    }
}

/// Embed locally using fastembed (BAAI/bge-small-en-v1.5 by default).
/// Downloads and caches the model from HuggingFace on first use (~67 MB).
/// Supported model names: "bge-small-en-v1.5" (default), "all-minilm-l6-v2", "nomic-embed-text-v1.5".
async fn get_local_embeddings_score(
    actual: String,
    expected: String,
    model_name: Option<String>,
) -> Result<f64, String> {
    tokio::task::spawn_blocking(move || {
        use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

        let embedding_model = match model_name.as_deref() {
            None | Some("bge-small-en-v1.5") => EmbeddingModel::BGESmallENV15,
            Some("all-minilm-l6-v2") => EmbeddingModel::AllMiniLML6V2,
            Some("nomic-embed-text-v1.5") => EmbeddingModel::NomicEmbedTextV15,
            Some(other) => {
                return Err(format!(
                    "Unknown local embedding model '{}'. Supported: bge-small-en-v1.5 (default), all-minilm-l6-v2, nomic-embed-text-v1.5. Set embedding_url to use an external API.",
                    other
                ))
            }
        };

        // Cache models in ~/.cache/portlang/embeddings (or OS equivalent).
        // Falls back to a temp dir rather than the current working directory.
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("portlang")
            .join("embeddings");

        let mut model = TextEmbedding::try_new(
            InitOptions::new(embedding_model).with_cache_dir(cache_dir),
        )
        .map_err(|e| format!("Failed to load embedding model: {}", e))?;

        let embeddings = model
            .embed(vec![actual.trim(), expected.trim()], None)
            .map_err(|e| format!("Embedding failed: {}", e))?;

        if embeddings.len() < 2 {
            return Err(format!(
                "Expected 2 embeddings, got {}",
                embeddings.len()
            ));
        }

        let vec_a: Vec<f64> = embeddings[0].iter().map(|&x| x as f64).collect();
        let vec_b: Vec<f64> = embeddings[1].iter().map(|&x| x as f64).collect();

        Ok(cosine_similarity(&vec_a, &vec_b))
    })
    .await
    .map_err(|e| format!("Embedding task panicked: {}", e))?
}

/// Embed via an OpenAI-compatible HTTP endpoint.
async fn get_remote_embeddings_score(
    actual: &str,
    expected: &str,
    url: &str,
    model: &str,
    api_key: &str,
) -> Result<f64, String> {
    let client = reqwest::Client::new();

    let body = serde_json::json!({
        "model": model,
        "input": [actual.trim(), expected.trim()],
        "encoding_format": "float"
    });

    let response = client
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("HTTP {}: {}", status, text));
    }

    let json: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;

    let data = json["data"]
        .as_array()
        .ok_or("Missing 'data' array in embeddings response")?;

    if data.len() < 2 {
        return Err(format!(
            "Expected 2 embeddings in response, got {}",
            data.len()
        ));
    }

    let vec_a = parse_remote_embedding(&data[0])?;
    let vec_b = parse_remote_embedding(&data[1])?;

    Ok(cosine_similarity(&vec_a, &vec_b))
}

fn parse_remote_embedding(entry: &serde_json::Value) -> Result<Vec<f64>, String> {
    entry["embedding"]
        .as_array()
        .ok_or("Missing 'embedding' field")?
        .iter()
        .map(|v| {
            v.as_f64()
                .ok_or_else(|| "Non-numeric embedding value".to_string())
        })
        .collect()
}

fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let norm_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}
