use crate::sandbox::Sandbox;
use portlang_core::{Action, Verifier, VerifierResult, VerifierTrigger};

/// Run verifiers based on trigger conditions
pub async fn run_verifiers(
    sandbox: &dyn Sandbox,
    verifiers: &[Verifier],
    action: &Action,
    is_stop: bool,
) -> Vec<VerifierResult> {
    let mut results = Vec::new();

    for verifier in verifiers {
        let should_run = match verifier.trigger {
            VerifierTrigger::Always => true,
            VerifierTrigger::OnStop => is_stop,
            VerifierTrigger::OnWrite => action.tool_name().map(|t| t.as_str()) == Some("write"),
        };

        if should_run {
            let result = run_verifier(sandbox, verifier).await;
            results.push(result);
        }
    }

    results
}

/// Run a single verifier
async fn run_verifier(sandbox: &dyn Sandbox, verifier: &Verifier) -> VerifierResult {
    match sandbox.run_command(&verifier.command).await {
        Ok(output) => VerifierResult::new(
            verifier.name.clone(),
            output.success,
            output.stdout,
            output.stderr,
            output.exit_code,
        ),
        Err(e) => VerifierResult::new(
            verifier.name.clone(),
            false,
            String::new(),
            format!("Failed to run verifier: {}", e),
            -1,
        ),
    }
}
