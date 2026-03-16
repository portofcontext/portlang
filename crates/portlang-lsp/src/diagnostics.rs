use portlang_config::raw::{InheritOr, RawField};
use tower_lsp::lsp_types::*;

pub fn diagnostics_for(text: &str) -> Vec<Diagnostic> {
    match toml::from_str::<RawField>(text) {
        Ok(raw) => validate_raw(&raw),
        Err(err) => vec![toml_error_to_diagnostic(text, &err)],
    }
}

fn offset_to_position(text: &str, offset: usize) -> Position {
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, ch) in text.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    Position::new(line, col)
}

fn toml_error_to_diagnostic(text: &str, err: &toml::de::Error) -> Diagnostic {
    let range = if let Some(span) = err.span() {
        Range {
            start: offset_to_position(text, span.start),
            end: offset_to_position(text, span.end),
        }
    } else {
        Range::default()
    };
    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        message: err.message().to_string(),
        source: Some("portlang".to_string()),
        ..Default::default()
    }
}

fn doc_diagnostic(msg: &str) -> Diagnostic {
    Diagnostic {
        range: Range::default(),
        severity: Some(DiagnosticSeverity::ERROR),
        message: msg.to_string(),
        source: Some("portlang".to_string()),
        ..Default::default()
    }
}

fn warn_diagnostic(msg: &str) -> Diagnostic {
    Diagnostic {
        range: Range::default(),
        severity: Some(DiagnosticSeverity::WARNING),
        message: msg.to_string(),
        source: Some("portlang".to_string()),
        ..Default::default()
    }
}

fn validate_raw(raw: &RawField) -> Vec<Diagnostic> {
    let mut diags = vec![];

    // Check model (required unless inherit)
    if raw.model.is_none() {
        diags.push(warn_diagnostic(
            "Missing [model] section. Add `model = \"inherit\"` or a `[model]` block.",
        ));
    }

    // Validate boundary
    if let Some(InheritOr::Value(boundary)) = &raw.boundary {
        for pattern in &boundary.allow_write {
            if glob::Pattern::new(pattern).is_err() {
                diags.push(doc_diagnostic(&format!(
                    "Invalid glob pattern in allow_write: '{}'",
                    pattern
                )));
            }
        }
        if let Some(network) = &boundary.network {
            if network != "allow" && network != "deny" {
                diags.push(doc_diagnostic(&format!(
                    "boundary.network must be 'allow' or 'deny', got '{}'",
                    network
                )));
            }
        }
    }

    // Validate verifier types and required fields
    let valid_verifier_types = ["shell", "json", "levenshtein", "semantic", "tool_call"];
    for v in &raw.verifier {
        if !valid_verifier_types.contains(&v.verifier_type.as_str()) {
            diags.push(doc_diagnostic(&format!(
                "Unknown verifier type '{}'. Valid: shell, json, levenshtein, semantic, tool_call",
                v.verifier_type
            )));
        }
        if v.verifier_type == "shell" && v.command.is_none() {
            diags.push(doc_diagnostic(&format!(
                "Verifier '{}' (type=shell) is missing required field `command`",
                v.name
            )));
        }
        if let Some(trigger) = &v.trigger {
            let is_valid_trigger =
                matches!(trigger.as_str(), "on_stop" | "always") || trigger.starts_with("on_tool:");
            if !is_valid_trigger {
                diags.push(doc_diagnostic(&format!(
                    "Verifier '{}' has unknown trigger '{}'. Valid: on_stop, always, on_tool:<tool_name>",
                    v.name, trigger
                )));
            }
        }
    }

    // Validate tool types
    let valid_tool_types = ["python", "shell", "mcp"];
    for t in &raw.tool {
        if !valid_tool_types.contains(&t.tool_type.as_str()) {
            diags.push(doc_diagnostic(&format!(
                "Unknown tool type '{}'. Valid: python, shell, mcp",
                t.tool_type
            )));
        }
    }

    diags
}
