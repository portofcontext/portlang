//! Test to inspect what prompts and tools the agent sees with Code Mode enabled
//! This test loads a real field.toml and prints what the agent would see

#[cfg(feature = "code-mode")]
#[cfg(test)]
mod tests {
    use portlang_config::parse_field_from_file;
    use portlang_runtime::prepare_agent_view;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_code_mode_prompt_inspection() {
        // Load the inspection field
        let field_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/field.toml");

        if !field_path.exists() {
            println!("\nField file not found at: {}", field_path.display());
            println!("Skipping test");
            return;
        }

        let field = parse_field_from_file(&field_path).unwrap();

        println!("\n{}", "=".repeat(80));
        println!("CODE MODE PROMPT INSPECTION TEST");
        println!("{}\n", "=".repeat(80));
        println!("Field: {}", field.name);
        println!(
            "Code mode enabled: {:?}",
            field.environment.code_mode_enabled
        );
        println!("{}\n", "=".repeat(80));

        // Prepare agent view - this sets up all tools and generates the prompt
        let agent_view = prepare_agent_view(&field).await.unwrap();

        println!("\n{}", "=".repeat(80));
        println!("TOOLS AVAILABLE TO AGENT");
        println!("{}\n", "=".repeat(80));
        println!("Number of tools: {}", agent_view.tools.len());
        for tool in &agent_view.tools {
            println!("\nTool: {}", tool.name);
            if let Some(desc) = &tool.description {
                println!("  Description: {}", desc);
            }
            println!(
                "  Schema: {}",
                serde_json::to_string_pretty(&tool.input_schema).unwrap()
            );
        }

        println!("\n{}", "=".repeat(80));
        println!("SYSTEM PROMPT");
        println!("{}\n", "=".repeat(80));
        println!("{}", agent_view.system_prompt);
        println!("\n{}", "=".repeat(80));

        // Verify code mode is enabled
        assert_eq!(field.environment.code_mode_enabled, Some(true));

        // Verify we only have one tool (code_mode)
        assert_eq!(agent_view.tools.len(), 1);
        assert_eq!(agent_view.tools[0].name, "code_mode");

        // Verify system prompt includes code mode instructions
        assert!(agent_view.system_prompt.contains("# Code Mode"));
        assert!(agent_view.system_prompt.contains("async function run()"));
        assert!(agent_view.system_prompt.contains("namespace Tools"));
    }
}
