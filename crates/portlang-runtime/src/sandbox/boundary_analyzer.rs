//! Analyzes boundary violations and traces them back to context sources

use super::context_tracer::{format_context_trace, ContextTracer};
use super::error::BoundaryViolation;

/// Analyzes boundary violations using context tracing
pub struct BoundaryAnalyzer {
    tracer: ContextTracer,
}

impl BoundaryAnalyzer {
    pub fn new(tracer: ContextTracer) -> Self {
        Self { tracer }
    }

    /// Analyze a write path violation and trace where the model got the information
    pub fn analyze_write_violation(
        &self,
        attempted_path: &str,
        allowed_patterns: &[String],
    ) -> BoundaryViolation {
        // Trace where the model might have gotten this path
        let references = self.tracer.trace_value(attempted_path);

        let context_trace = if !references.is_empty() {
            Some(format_context_trace(&references))
        } else {
            None
        };

        BoundaryViolation::write_not_allowed(
            attempted_path.to_string(),
            allowed_patterns.to_vec(),
            context_trace,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_with_system_prompt_context() {
        let tracer = ContextTracer::new(
            Some("Working Directory: /workspace\nWrite files to this directory.".to_string()),
            None,
        );
        let analyzer = BoundaryAnalyzer::new(tracer);

        let violation =
            analyzer.analyze_write_violation("/workspace/result.txt", &["result.txt".to_string()]);

        assert!(violation.context_trace.is_some());
        let trace = violation.context_trace.unwrap();
        assert!(trace.contains("System Prompt"));
        assert!(trace.contains("/workspace"));
    }

    #[test]
    fn test_analyze_without_context() {
        let tracer = ContextTracer::new(None, None);
        let analyzer = BoundaryAnalyzer::new(tracer);

        let violation = analyzer.analyze_write_violation("random.txt", &["output.txt".to_string()]);

        // Should still create violation even without context trace
        assert_eq!(violation.attempted_value, Some("random.txt".to_string()));
    }
}
