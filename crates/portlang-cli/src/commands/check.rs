use anyhow::Result;
use portlang_config::{parse_field_with_parent, resolve_parent_config};
use std::path::PathBuf;

/// Check a field for errors
pub fn check_command(field_path: PathBuf, parent_field: Option<PathBuf>) -> Result<()> {
    println!("Checking field: {}", field_path.display());

    let parent = resolve_parent_config(&field_path, parent_field.as_ref())?;
    let field = parse_field_with_parent(&field_path, parent.as_ref())?;

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
