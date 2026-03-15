pub mod apple_container;
pub mod boundary_analyzer;
pub mod context_tracer;
pub mod dispatch;
pub mod error;
pub mod traits;

pub use apple_container::AppleContainerSandbox;
pub use boundary_analyzer::BoundaryAnalyzer;
pub use context_tracer::{format_context_trace, ContextTracer};
pub use dispatch::DispatchSandbox;
pub use error::*;
pub use traits::*;

use crate::tools::ToolRegistry;
use portlang_core::{Boundary, Environment};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

/// Check if Apple Containerization is available on the system
fn check_apple_container_available() -> bool {
    Command::new("container").arg("--version").output().is_ok()
}

pub async fn create_sandbox(
    environment: &Environment,
    boundary: &Boundary,
    registry: Arc<ToolRegistry>,
) -> Result<Arc<dyn Sandbox>> {
    let root = PathBuf::from(&environment.root);

    // Always use container sandbox - fail if not available
    if !check_apple_container_available() {
        return Err(SandboxError::InitError(
            "Apple Container is not available. Please install it with: portlang init --install"
                .to_string(),
        ));
    }

    Ok(Arc::new(
        AppleContainerSandbox::new(root, boundary.clone(), registry, environment).await?,
    ))
}
