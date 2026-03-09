use crate::sandbox::Sandbox;
use std::path::PathBuf;

/// Environment context that provides structured information about the execution environment
pub struct EnvironmentContext {
    pub working_directory: PathBuf,
    pub directory_tree: Option<String>,
    pub custom_context: Option<String>,
}

impl EnvironmentContext {
    /// Discover environment by probing the sandbox
    pub async fn discover(sandbox: &dyn Sandbox, custom: Option<String>) -> Self {
        let working_dir = sandbox.root().to_path_buf();
        let tree = Self::generate_tree(sandbox).await;

        Self {
            working_directory: working_dir,
            directory_tree: tree,
            custom_context: custom,
        }
    }

    /// Generate directory tree view
    async fn generate_tree(sandbox: &dyn Sandbox) -> Option<String> {
        // Try using find command for simplicity
        let result = sandbox
            .run_command("find . -maxdepth 2 -type f -o -type d | sort")
            .await;

        match result {
            Ok(output) if !output.stdout.is_empty() => Some(Self::format_tree(&output.stdout)),
            _ => Some("  (unable to generate tree)".to_string()),
        }
    }

    /// Format find output as tree-like view
    fn format_tree(find_output: &str) -> String {
        let lines: Vec<&str> = find_output.lines().collect();

        if lines.is_empty() {
            return "  (empty directory)".to_string();
        }

        let mut result = String::new();
        for line in lines {
            let path = line.trim_start_matches("./");
            if path.is_empty() || path == "." {
                continue;
            }

            let depth = path.matches('/').count();
            let indent = "  ".repeat(depth);
            let name = path.split('/').last().unwrap_or(path);

            result.push_str(&format!("{}{}\n", indent, name));
        }

        if result.is_empty() {
            "  (empty directory)".to_string()
        } else {
            result
        }
    }

    /// Format as system prompt section
    pub fn format_for_prompt(&self) -> String {
        let mut prompt = String::from("\n=== ENVIRONMENT CONTEXT ===\n\n");

        prompt.push_str("Workspace Files:\n");
        if let Some(tree) = &self.directory_tree {
            prompt.push_str(tree);
        } else {
            prompt.push_str("  (empty workspace)\n");
        }

        if let Some(custom) = &self.custom_context {
            prompt.push_str("\nAdditional Context:\n");
            prompt.push_str(custom);
            prompt.push('\n');
        }

        prompt.push_str("\n=== END ENVIRONMENT ===\n");
        prompt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_tree_empty() {
        let output = "";
        let tree = EnvironmentContext::format_tree(output);
        assert_eq!(tree, "  (empty directory)");
    }

    #[test]
    fn test_format_tree_with_files() {
        let output = "./\n./file1.txt\n./dir1\n./dir1/file2.txt";
        let tree = EnvironmentContext::format_tree(output);
        assert!(tree.contains("file1.txt"));
        assert!(tree.contains("dir1"));
        assert!(tree.contains("file2.txt"));
    }

    #[test]
    fn test_format_tree_ignores_dot() {
        let output = "./\n./file.txt";
        let tree = EnvironmentContext::format_tree(output);
        assert!(!tree.contains("./"));
        assert!(tree.contains("file.txt"));
    }

    #[test]
    fn test_format_for_prompt_basic() {
        let ctx = EnvironmentContext {
            working_directory: PathBuf::from("/workspace"),
            directory_tree: Some("  file.txt\n".to_string()),
            custom_context: None,
        };

        let prompt = ctx.format_for_prompt();
        assert!(prompt.contains("Workspace Files:"));
        assert!(prompt.contains("file.txt"));
        assert!(prompt.contains("=== ENVIRONMENT CONTEXT ==="));
        assert!(prompt.contains("=== END ENVIRONMENT ==="));
    }

    #[test]
    fn test_format_for_prompt_with_custom_context() {
        let ctx = EnvironmentContext {
            working_directory: PathBuf::from("/workspace"),
            directory_tree: Some("  file.txt\n".to_string()),
            custom_context: Some("Python 3.11+ required\n".to_string()),
        };

        let prompt = ctx.format_for_prompt();
        assert!(prompt.contains("Additional Context:"));
        assert!(prompt.contains("Python 3.11+ required"));
    }

    #[test]
    fn test_format_for_prompt_no_tree() {
        let ctx = EnvironmentContext {
            working_directory: PathBuf::from("/workspace"),
            directory_tree: None,
            custom_context: None,
        };

        let prompt = ctx.format_for_prompt();
        assert!(prompt.contains("(empty workspace)"));
    }
}
