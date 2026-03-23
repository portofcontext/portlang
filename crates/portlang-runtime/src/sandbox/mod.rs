pub mod boundary_analyzer;
pub mod container_backend;
pub mod container_sandbox;
pub mod context_tracer;
pub mod dispatch;
pub mod error;
pub mod traits;

pub use boundary_analyzer::BoundaryAnalyzer;
pub use container_backend::{
    AppleContainerBackend, ContainerBackend, DockerBackend, PodmanBackend,
};
pub use container_sandbox::ContainerSandbox;
pub use context_tracer::{format_context_trace, ContextTracer};
pub use dispatch::DispatchSandbox;
pub use error::*;
pub use traits::*;

use crate::tools::ToolRegistry;
use portlang_core::{Boundary, Environment};
use std::path::PathBuf;
use std::sync::Arc;

/// Create the appropriate sandbox for the current environment.
///
/// Backend selection: `PORTLANG_CONTAINER_BACKEND` env var overrides auto-detection.
/// Valid values: `apple-container`, `podman`, `docker`.
///
/// Auto-detection priority (when env var is unset):
/// 1. Apple Container — preferred on macOS when available
/// 2. Podman
/// 3. Docker
///
/// Returns an error if no supported container backend is found.
pub async fn create_sandbox(
    environment: &Environment,
    boundary: &Boundary,
    registry: Arc<ToolRegistry>,
) -> Result<Arc<dyn Sandbox>> {
    let root = PathBuf::from(&environment.root);

    let backend: Box<dyn ContainerBackend> = match std::env::var("PORTLANG_CONTAINER_BACKEND")
        .as_deref()
    {
        Ok("apple-container") => {
            tracing::info!("Using Apple Container backend (forced via PORTLANG_CONTAINER_BACKEND)");
            Box::new(AppleContainerBackend)
        }
        Ok("podman") => {
            tracing::info!("Using Podman backend (forced via PORTLANG_CONTAINER_BACKEND)");
            Box::new(PodmanBackend)
        }
        Ok("docker") => {
            tracing::info!("Using Docker backend (forced via PORTLANG_CONTAINER_BACKEND)");
            Box::new(DockerBackend)
        }
        Ok(other) => {
            return Err(SandboxError::InitError(format!(
                    "Unknown PORTLANG_CONTAINER_BACKEND '{other}'. Valid values: apple-container, podman, docker."
                )));
        }
        Err(_) => {
            if AppleContainerBackend::is_available() {
                tracing::info!("Using Apple Container backend");
                Box::new(AppleContainerBackend)
            } else if PodmanBackend::is_available() {
                tracing::info!("Using Podman backend");
                Box::new(PodmanBackend)
            } else if DockerBackend::is_available() {
                tracing::info!("Using Docker backend");
                Box::new(DockerBackend)
            } else {
                return Err(SandboxError::InitError(
                        "No container backend found. Install Apple Container (macOS), Podman, or Docker.".to_string(),
                    ));
            }
        }
    };

    Ok(Arc::new(
        ContainerSandbox::new(root, boundary.clone(), registry, environment, backend).await?,
    ))
}
