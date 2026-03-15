use anyhow::Result;
use portlang_config::{apply_runtime_context, parse_field_with_parent, resolve_parent_config};
use portlang_core::RuntimeContext;
use std::path::PathBuf;

/// Check a field for errors
pub fn check_command(
    field_path: PathBuf,
    parent_field: Option<PathBuf>,
    ctx: RuntimeContext,
) -> Result<()> {
    println!("Checking field: {}", field_path.display());

    let parent = resolve_parent_config(&field_path, parent_field.as_ref())?;
    let field = parse_field_with_parent(&field_path, parent.as_ref())?;

    // Show declared template variables and their status
    if !field.vars.is_empty() {
        println!("  Template variables:");
        let mut var_names: Vec<&String> = field.vars.keys().collect();
        var_names.sort();
        for name in var_names {
            let decl = &field.vars[name];
            let status = if ctx.vars.contains_key(name) {
                format!("supplied ({})", ctx.vars[name])
            } else if let Some(ref default) = decl.default {
                format!("default ({})", default)
            } else {
                "MISSING (required)".to_string()
            };
            let desc = decl
                .description
                .as_deref()
                .map(|d| format!(" — {}", d))
                .unwrap_or_default();
            println!("    {{{{ {} }}}}  {}{}", name, status, desc);
        }
        println!();
    }

    // Apply runtime context to validate templates and catch missing vars
    let field = apply_runtime_context(field, &ctx)?;

    println!("✓ Field '{}' is valid", field.name);
    println!("  Model: {}", field.model.name);

    if let Some(description) = &field.description {
        println!("  Description: {}", description);
    }

    // Show boundary info
    if !field.boundary.allow_write.is_empty() {
        println!("  Write permissions: {:?}", field.boundary.allow_write);
    }

    // Show boundary limits
    if let Some(max_tokens) = field.boundary.max_tokens {
        println!("  Token budget: {}", max_tokens);
    }

    if let Some(max_cost) = &field.boundary.max_cost {
        println!("  Cost budget: {}", max_cost);
    }

    // Show verifiers
    if !field.verifiers.is_empty() {
        println!("  Verifiers: {}", field.verifiers.len());
        for verifier in &field.verifiers {
            println!("    - {} ({:?})", verifier.name, verifier.trigger);
        }
    }

    Ok(())
}
