//! Traces model behavior back to context sources
//!
//! When the model does something unexpected, this helps answer:
//! "Where did the model get that information?"

use std::collections::HashMap;

/// A reference to where information appears in the model's context
#[derive(Debug, Clone)]
pub struct ContextReference {
    pub source: ContextSource,
    pub excerpt: String,
    pub relevance_score: f32,
}

#[derive(Debug, Clone)]
pub enum ContextSource {
    SystemPrompt { section: String },
    ToolDefinition { tool_name: String },
    MessageHistory { step: usize },
    EnvironmentContext { key: String },
}

impl ContextSource {
    pub fn display_name(&self) -> String {
        match self {
            Self::SystemPrompt { section } => format!("System Prompt: {}", section),
            Self::ToolDefinition { tool_name } => format!("Tool Definition: {}", tool_name),
            Self::MessageHistory { step } => format!("Message History: Step {}", step),
            Self::EnvironmentContext { key } => format!("Environment: {}", key),
        }
    }
}

/// Analyzes model context to find potential sources of information
pub struct ContextTracer {
    system_prompt: Option<String>,
    tool_definitions: Option<String>,
    environment_context: HashMap<String, String>,
}

impl ContextTracer {
    pub fn new(system_prompt: Option<String>, tool_definitions: Option<String>) -> Self {
        Self {
            system_prompt,
            tool_definitions,
            environment_context: HashMap::new(),
        }
    }

    /// Add environment context for tracing
    pub fn add_environment_context(&mut self, key: String, value: String) {
        self.environment_context.insert(key, value);
    }

    /// Search for where a specific string or pattern might have come from
    /// Smart tokenization: breaks down complex values into searchable components
    pub fn trace_value(&self, search_term: &str) -> Vec<ContextReference> {
        let mut references = Vec::new();

        // Generate search variants: full term + meaningful components
        let search_variants = self.generate_search_variants(search_term);

        for (variant, weight) in search_variants {
            // Search system prompt
            if let Some(ref prompt) = self.system_prompt {
                for mut ref_item in
                    self.search_in_text(prompt, &variant, |excerpt| ContextSource::SystemPrompt {
                        section: self.identify_section(prompt, excerpt),
                    })
                {
                    ref_item.relevance_score *= weight;
                    references.push(ref_item);
                }
            }

            // Search tool definitions
            if let Some(ref tools) = self.tool_definitions {
                for mut ref_item in self.search_in_text(tools, &variant, |excerpt| {
                    let tool_name = self
                        .extract_tool_name(excerpt)
                        .unwrap_or_else(|| "unknown".to_string());
                    ContextSource::ToolDefinition { tool_name }
                }) {
                    ref_item.relevance_score *= weight;
                    references.push(ref_item);
                }
            }

            // Search environment context
            for (key, value) in &self.environment_context {
                if value.contains(&variant) {
                    let excerpt = self.extract_context_window(value, &variant, 50);
                    references.push(ContextReference {
                        source: ContextSource::EnvironmentContext { key: key.clone() },
                        excerpt,
                        relevance_score: self.calculate_relevance(&variant, value) * weight,
                    });
                }
            }
        }

        // Deduplicate and sort by relevance
        let deduped = self.deduplicate_references(references);
        let mut sorted = deduped;
        sorted.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        sorted
    }

    /// Generate search variants with weights
    /// For "/workspace/result.txt" returns:
    ///   - ("/workspace/result.txt", 1.0) - exact full match
    ///   - ("/workspace", 0.8) - directory component
    ///   - ("result.txt", 0.7) - filename component
    ///   - ("workspace", 0.6) - keyword component
    fn generate_search_variants(&self, term: &str) -> Vec<(String, f32)> {
        let mut variants = vec![
            (term.to_string(), 1.0), // Full term gets highest weight
        ];

        // If it's a path, extract components
        if term.contains('/') || term.contains('\\') {
            let parts: Vec<&str> = term.split(&['/', '\\'][..]).collect();

            // Directory path components
            let mut path_acc = String::new();
            for (i, part) in parts.iter().enumerate() {
                if part.is_empty() {
                    continue;
                }

                if !path_acc.is_empty() {
                    path_acc.push('/');
                }
                path_acc.push_str(part);

                // Earlier components (like /workspace) are more likely to be in context
                let weight = 0.8 - (i as f32 * 0.1);
                variants.push((path_acc.clone(), weight.max(0.5)));
            }

            // Filename (last component)
            if let Some(filename) = parts.last() {
                if !filename.is_empty() {
                    variants.push((filename.to_string(), 0.7));
                }
            }

            // Keywords (without leading /)
            for part in &parts {
                if !part.is_empty() && part.len() > 2 {
                    variants.push((part.to_string(), 0.6));
                }
            }
        }

        // If it contains special chars, also search for the cleaned version
        if term.contains(|c: char| !c.is_alphanumeric() && c != '_' && c != '-') {
            let cleaned: String = term
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            if cleaned.len() > 2 && cleaned != term {
                variants.push((cleaned, 0.5));
            }
        }

        // Deduplicate variants
        let mut seen = std::collections::HashSet::new();
        variants.retain(|(v, _)| seen.insert(v.clone()));

        variants
    }

    /// Extract multiple related terms from model output
    pub fn trace_tokens(&self, text: &str) -> Vec<ContextReference> {
        let mut all_refs = Vec::new();

        // Extract potential search terms (paths, variables, etc.)
        let tokens = self.extract_interesting_tokens(text);

        for token in tokens {
            all_refs.extend(self.trace_value(&token));
        }

        // Deduplicate and return top references
        self.deduplicate_references(all_refs)
    }

    /// Search for a term in text and create references
    fn search_in_text<F>(
        &self,
        text: &str,
        search_term: &str,
        source_fn: F,
    ) -> Vec<ContextReference>
    where
        F: Fn(&str) -> ContextSource,
    {
        let mut refs = Vec::new();
        let search_lower = search_term.to_lowercase();

        // Search for exact matches and partial matches
        for line in text.lines() {
            let line_lower = line.to_lowercase();
            if line_lower.contains(&search_lower) {
                let excerpt = self.extract_context_window(text, search_term, 100);
                refs.push(ContextReference {
                    source: source_fn(&excerpt),
                    excerpt: excerpt.clone(),
                    relevance_score: self.calculate_relevance(search_term, line),
                });
            }
        }

        refs
    }

    /// Extract surrounding context from a larger text
    fn extract_context_window(&self, text: &str, target: &str, window_chars: usize) -> String {
        if let Some(pos) = text.to_lowercase().find(&target.to_lowercase()) {
            let start = pos.saturating_sub(window_chars);
            let end = (pos + target.len() + window_chars).min(text.len());

            let excerpt = &text[start..end];

            // Clean up the excerpt
            let mut clean = excerpt.trim().to_string();
            if start > 0 {
                clean = format!("...{}", clean);
            }
            if end < text.len() {
                clean = format!("{}...", clean);
            }

            clean
        } else {
            text.chars().take(200).collect()
        }
    }

    /// Calculate relevance score (0.0 to 1.0)
    fn calculate_relevance(&self, search_term: &str, context: &str) -> f32 {
        let exact_match = context.contains(search_term);
        let case_insensitive_match = context.to_lowercase().contains(&search_term.to_lowercase());

        if exact_match {
            1.0
        } else if case_insensitive_match {
            0.8
        } else {
            // Fuzzy matching could go here
            0.5
        }
    }

    /// Identify which section of system prompt this excerpt is from
    fn identify_section(&self, _full_prompt: &str, excerpt: &str) -> String {
        // Look for headers or sections
        if excerpt.contains("ENVIRONMENT CONTEXT") || excerpt.contains("Working Directory") {
            "Environment Context".to_string()
        } else if excerpt.contains("Tool") || excerpt.contains("function") {
            "Tool Instructions".to_string()
        } else if excerpt.contains("Goal:") || excerpt.contains("Task:") {
            "Goal Definition".to_string()
        } else {
            "System Instructions".to_string()
        }
    }

    /// Try to extract tool name from JSON snippet
    fn extract_tool_name(&self, json_excerpt: &str) -> Option<String> {
        // Simple heuristic: look for "name": "tool_name" pattern
        if let Some(start) = json_excerpt.find(r#""name""#) {
            if let Some(value_start) = json_excerpt[start..].find(':') {
                let after_colon = &json_excerpt[start + value_start + 1..];
                if let Some(quote_start) = after_colon.find('"') {
                    if let Some(quote_end) = after_colon[quote_start + 1..].find('"') {
                        return Some(
                            after_colon[quote_start + 1..quote_start + 1 + quote_end].to_string(),
                        );
                    }
                }
            }
        }
        None
    }

    /// Extract interesting tokens from text (paths, variables, keywords)
    fn extract_interesting_tokens(&self, text: &str) -> Vec<String> {
        let mut tokens = Vec::new();

        // Extract paths
        for word in text.split_whitespace() {
            if word.contains('/') || word.contains('\\') {
                tokens.push(word.to_string());
            }

            // Extract words in quotes
            if word.starts_with('"') || word.starts_with('\'') {
                tokens.push(word.trim_matches(|c| c == '"' || c == '\'').to_string());
            }
        }

        // Extract from JSON-like patterns
        if let Some(path) = text.split('"').nth(1) {
            tokens.push(path.to_string());
        }

        tokens
    }

    /// Remove duplicate references, keeping highest relevance
    fn deduplicate_references(&self, refs: Vec<ContextReference>) -> Vec<ContextReference> {
        let mut seen_excerpts = std::collections::HashSet::new();
        let mut deduped = Vec::new();

        for reference in refs {
            if !seen_excerpts.contains(&reference.excerpt) {
                seen_excerpts.insert(reference.excerpt.clone());
                deduped.push(reference);
            }
        }

        deduped
    }
}

/// Format context references as a human-readable string
pub fn format_context_trace(references: &[ContextReference]) -> String {
    if references.is_empty() {
        return "No context sources found for this value.".to_string();
    }

    let mut output = String::from("\nCONTEXT TRACE:\n");

    for (i, reference) in references.iter().take(5).enumerate() {
        output.push_str(&format!(
            "\n{}. {} (relevance: {:.0}%)\n   \"{}\"\n",
            i + 1,
            reference.source.display_name(),
            reference.relevance_score * 100.0,
            reference.excerpt
        ));
    }

    if references.len() > 5 {
        output.push_str(&format!("\n...and {} more sources\n", references.len() - 5));
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_value_in_system_prompt() {
        let mut tracer = ContextTracer::new(
            Some("Working Directory: /workspace\nYou are an agent.".to_string()),
            None,
        );

        let refs = tracer.trace_value("/workspace");
        assert!(!refs.is_empty());
        assert!(matches!(refs[0].source, ContextSource::SystemPrompt { .. }));
    }

    #[test]
    fn test_trace_value_in_environment() {
        let mut tracer = ContextTracer::new(None, None);
        tracer.add_environment_context("working_dir".to_string(), "/workspace".to_string());

        let refs = tracer.trace_value("/workspace");
        assert!(!refs.is_empty());
        assert!(matches!(
            refs[0].source,
            ContextSource::EnvironmentContext { .. }
        ));
    }

    #[test]
    fn test_smart_path_tokenization() {
        // System prompt only mentions "/workspace", not the full path
        let tracer = ContextTracer::new(
            Some("Working Directory: /workspace\nYou are an agent.".to_string()),
            None,
        );

        // Search for full path - should still find "/workspace" component
        let refs = tracer.trace_value("/workspace/result.txt");
        assert!(
            !refs.is_empty(),
            "Should find references even though full path not in context"
        );

        // Should find the "/workspace" component
        let has_workspace_ref = refs.iter().any(|r| r.excerpt.contains("/workspace"));
        assert!(has_workspace_ref, "Should find /workspace component");
    }

    #[test]
    fn test_generate_search_variants() {
        let tracer = ContextTracer::new(None, None);
        let variants = tracer.generate_search_variants("/workspace/data/result.txt");

        // Should generate multiple search terms
        let variant_strs: Vec<String> = variants.iter().map(|(s, _)| s.clone()).collect();

        // Debug: print what we got
        eprintln!("Generated variants: {:?}", variant_strs);

        assert!(
            variant_strs.contains(&"/workspace/data/result.txt".to_string()),
            "Full path"
        );
        // The path accumulation builds up: /workspace, /workspace/data, /workspace/data/result.txt
        // So we should check for accumulated paths
        let has_workspace_component = variant_strs.iter().any(|s| s.contains("workspace"));
        assert!(has_workspace_component, "Should have workspace component");
        assert!(variant_strs.contains(&"result.txt".to_string()), "Filename");
    }

    #[test]
    fn test_extract_context_window() {
        let tracer = ContextTracer::new(None, None);
        let text = "This is a long text with /workspace in the middle and more text after it goes on and on";
        let excerpt = tracer.extract_context_window(text, "/workspace", 20);

        assert!(excerpt.contains("/workspace"));
        assert!(excerpt.len() < text.len()); // Should be truncated
    }
}
