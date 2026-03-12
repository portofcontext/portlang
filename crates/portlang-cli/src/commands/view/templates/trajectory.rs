use super::super::common::*;
use portlang_core::{Action, Trajectory};

/// Generate a trajectory viewer HTML page
pub fn generate_trajectory_html(trajectory: &Trajectory) -> String {
    generate_trajectory_html_with_back_link(trajectory, None)
}

/// Generate a trajectory viewer HTML page with optional back link
pub fn generate_trajectory_html_with_back_link(
    trajectory: &Trajectory,
    back_link: Option<&str>,
) -> String {
    let head = render_head(&format!("Trajectory: {}", trajectory.field_name));

    let back_nav = if let Some(link) = back_link {
        format!(
            r#"<div style="margin-bottom: 1rem;">
    <a href="{}" style="color: var(--secondary); text-decoration: none; font-size: 0.875rem;">
        ← Back to Eval Dashboard
    </a>
</div>"#,
            escape_html(link)
        )
    } else {
        String::new()
    };

    let header = render_trajectory_header(trajectory);
    let context_section = render_context_section(trajectory);
    let timeline = render_timeline(trajectory);
    let navigation = render_navigation();
    let steps_container = render_steps_container(&trajectory.steps);
    let script = render_trajectory_script(trajectory);

    let modals = render_modals(trajectory);

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
{}
<body>
<div class="container">
    {}
    {}
    {}
    {}
    {}
    {}
</div>
{}
<script>
{}
</script>
</body>
</html>"#,
        head,
        back_nav,
        header,
        context_section,
        timeline,
        navigation,
        steps_container,
        modals,
        script
    )
}

fn render_trajectory_header(trajectory: &Trajectory) -> String {
    let outcome_class = if trajectory
        .outcome
        .as_ref()
        .map(|o| o.is_success())
        .unwrap_or(false)
    {
        "converged"
    } else {
        "failed"
    };

    let outcome_text = trajectory
        .outcome
        .as_ref()
        .map(|o| o.description())
        .unwrap_or_else(|| "In Progress".to_string());

    let duration = if let Some(ended) = trajectory.ended_at {
        let duration = ended.signed_duration_since(trajectory.started_at);
        format!("{:.1}s", duration.num_milliseconds() as f64 / 1000.0)
    } else {
        "N/A".to_string()
    };

    // Build agent context as modal badges
    let mut context_badges = Vec::new();

    if !trajectory.goal.is_empty() {
        context_badges
            .push(r#"<span class="context-badge" onclick="openModal('goal')">Goal</span>"#);
    }
    if !trajectory.system_prompt.is_empty() {
        context_badges.push(
            r#"<span class="context-badge" onclick="openModal('system')">System Prompt</span>"#,
        );
    }
    if !trajectory.tool_definitions.is_empty() {
        context_badges.push(
            r#"<span class="context-badge" onclick="openModal('tools')">Tool Definitions</span>"#,
        );
    }

    // Add structured output badges if present
    if trajectory.structured_output.is_some() {
        context_badges.push(
            r#"<span class="context-badge" onclick="openModal('output')">Structured Output</span>"#,
        );
    }

    let badges_html = if !context_badges.is_empty() {
        format!(
            r#"<div class="context-badges">{}</div>"#,
            context_badges.join("")
        )
    } else {
        String::new()
    };

    let agent_context_html = format!(
        r#"<div class="info-item" style="grid-column: 1 / -1;">
    <span class="info-label">Model</span>
    <div style="display: flex; align-items: center; gap: 0.75rem; flex-wrap: wrap;">
        <span class="info-value mono">{}</span>
        {}
    </div>
</div>"#,
        escape_html(&trajectory.model_name),
        badges_html
    );

    format!(
        r#"<h1>Trajectory: {}</h1>
<div class="section">
    <div class="header-info">
        <div class="info-item">
            <span class="info-label">Trajectory ID</span>
            <span class="info-value mono">{}</span>
        </div>
        <div class="info-item">
            <span class="info-label">Outcome</span>
            <span class="info-value {}"><span class="status-badge {}">{}</span></span>
        </div>
        <div class="info-item">
            <span class="info-label">Steps</span>
            <span class="info-value">{}</span>
        </div>
        <div class="info-item">
            <span class="info-label">Total Cost</span>
            <span class="info-value">${:.4}</span>
        </div>
        <div class="info-item">
            <span class="info-label">Total Tokens</span>
            <span class="info-value">{}</span>
        </div>
        <div class="info-item">
            <span class="info-label">Duration</span>
            <span class="info-value">{}</span>
        </div>
        {}
    </div>
</div>"#,
        escape_html(&trajectory.field_name),
        escape_html(&trajectory.id.filename()),
        outcome_class,
        outcome_class,
        escape_html(&outcome_text),
        trajectory.step_count(),
        trajectory.total_cost.to_dollars(),
        trajectory.total_tokens,
        duration,
        agent_context_html
    )
}

fn render_modals(trajectory: &Trajectory) -> String {
    let mut modals = Vec::new();

    if !trajectory.goal.is_empty() {
        modals.push(format!(
            r#"<div id="modal-goal" class="modal" onclick="closeModalOnBackdrop(event, 'goal')">
    <div class="modal-content">
        <div class="modal-header">
            <h3>Goal</h3>
            <span class="modal-close" onclick="closeModal('goal')">&times;</span>
        </div>
        <div class="modal-body">
            <pre class="json-content">{}</pre>
        </div>
    </div>
</div>"#,
            escape_html(&trajectory.goal)
        ));
    }

    if !trajectory.system_prompt.is_empty() {
        modals.push(format!(
            r#"<div id="modal-system" class="modal" onclick="closeModalOnBackdrop(event, 'system')">
    <div class="modal-content">
        <div class="modal-header">
            <h3>System Prompt</h3>
            <span class="modal-close" onclick="closeModal('system')">&times;</span>
        </div>
        <div class="modal-body">
            <pre class="json-content">{}</pre>
        </div>
    </div>
</div>"#,
            escape_html(&trajectory.system_prompt)
        ));
    }

    if !trajectory.tool_definitions.is_empty() {
        modals.push(format!(
            r#"<div id="modal-tools" class="modal" onclick="closeModalOnBackdrop(event, 'tools')">
    <div class="modal-content">
        <div class="modal-header">
            <h3>Tool Definitions</h3>
            <span class="modal-close" onclick="closeModal('tools')">&times;</span>
        </div>
        <div class="modal-body">
            <pre class="json-content">{}</pre>
        </div>
    </div>
</div>"#,
            format_json(&trajectory.tool_definitions)
        ));
    }

    if let Some(ref output) = trajectory.structured_output {
        let schema_html = if let Some(ref schema) = trajectory.output_schema {
            format!(
                r#"<div style="margin-bottom: 2rem;">
    <h4 style="margin-top: 0; color: var(--secondary);">Required Schema</h4>
    <pre class="json-content">{}</pre>
</div>"#,
                serde_json::to_string_pretty(schema).unwrap_or_else(|_| "{}".to_string())
            )
        } else {
            String::new()
        };

        modals.push(format!(
            r#"<div id="modal-output" class="modal" onclick="closeModalOnBackdrop(event, 'output')">
    <div class="modal-content">
        <div class="modal-header">
            <h3>Structured Output</h3>
            <span class="modal-close" onclick="closeModal('output')">&times;</span>
        </div>
        <div class="modal-body">
            {}
            <h4 style="margin-top: 0; color: var(--tertiary);">Agent Output</h4>
            <pre class="json-content">{}</pre>
        </div>
    </div>
</div>"#,
            schema_html,
            serde_json::to_string_pretty(output).unwrap_or_else(|_| "{}".to_string())
        ));
    }

    modals.join("\n")
}

fn render_context_section(_trajectory: &Trajectory) -> String {
    // Agent context is now rendered inline with the header
    String::new()
}

fn render_navigation() -> String {
    r#"<div class="navigation">
    <button id="prev-btn" onclick="previousStep()">← Previous</button>
    <div class="nav-info">
        <span id="step-indicator" class="mono">Step <span id="current-step">1</span> of <span id="total-steps">0</span></span>
    </div>
    <button id="next-btn" onclick="nextStep()">Next →</button>
</div>"#
        .to_string()
}

fn render_steps_container(steps: &[portlang_core::TrajectoryStep]) -> String {
    let steps_html: String = steps
        .iter()
        .enumerate()
        .map(|(idx, step)| render_step(idx, step))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<div id="steps-container">
    {}
</div>"#,
        steps_html
    )
}

fn render_step(index: usize, step: &portlang_core::TrajectoryStep) -> String {
    let display = if index == 0 { "block" } else { "none" };
    let action_name = match &step.action {
        Action::ToolCall { tool, .. } => format!("Tool: {}", tool),
        Action::TextOutput { .. } => "Text Output".to_string(),
        Action::Stop => "Stop".to_string(),
    };

    let action_input = match &step.action {
        Action::ToolCall { input, .. } => {
            serde_json::to_string_pretty(input).unwrap_or_else(|_| "{}".to_string())
        }
        Action::TextOutput { text } => text.clone(),
        Action::Stop => String::new(),
    };

    let rejected_badge = if step.rejected {
        r#"<span class="status-badge failed">Rejected by Boundary</span>"#
    } else {
        ""
    };

    let verifier_html = if !step.verifier_results.is_empty() {
        render_verifiers(&step.verifier_results)
    } else {
        String::new()
    };

    // Build token metadata display
    let token_meta = if let (Some(input), Some(output)) = (step.input_tokens, step.output_tokens) {
        format!(
            "{} tokens ({} in · {} out) · ${:.4}",
            step.tokens_used,
            input,
            output,
            step.cost.to_dollars()
        )
    } else {
        format!(
            "{} tokens · ${:.4}",
            step.tokens_used,
            step.cost.to_dollars()
        )
    };

    format!(
        r#"<div class="step-content" id="step-{}" style="display: {}">
    <div class="section">
        <div class="step-header">
            <span class="step-title">Step {}: {}</span>
            <span class="step-meta">{}</span>
        </div>
        {}
        <div class="json-container">
            <div class="json-label">Input</div>
            <pre class="json-content">{}</pre>
        </div>

        <div class="json-container">
            <div class="json-label">Result</div>
            <pre class="json-content">{}</pre>
        </div>

        {}
    </div>
</div>"#,
        index,
        display,
        step.step_number,
        escape_html(&action_name),
        token_meta,
        rejected_badge,
        format_json(&action_input),
        format_json(&step.result),
        verifier_html
    )
}

fn render_verifiers(verifier_results: &[portlang_core::VerifierResult]) -> String {
    let verifiers_html: String = verifier_results
        .iter()
        .map(|vr| {
            let status_class = if vr.passed { "passed" } else { "failed" };
            let status_text = if vr.passed { "Passed" } else { "Failed" };

            let mut details_html = String::new();

            // Always show command if available
            if let Some(ref command) = vr.command {
                details_html.push_str(&format!(
                    r#"<div class="verifier-command">
    <strong>Command:</strong>
    <pre class="command-text">{}</pre>
</div>"#,
                    escape_html(command)
                ));
            }

            // Always show schema for json verifiers
            if let Some(ref schema) = vr.schema {
                let schema_str =
                    serde_json::to_string_pretty(schema).unwrap_or_else(|_| "{}".to_string());
                details_html.push_str(&format!(
                    r#"<div class="verifier-schema">
    <strong>Schema:</strong>
    <pre class="schema-text">{}</pre>
</div>"#,
                    escape_html(&schema_str)
                ));
            }

            // Show output details (especially for failed verifiers)
            if !vr.passed {
                if !vr.stderr.is_empty() {
                    details_html.push_str(&format!(
                        r#"<div class="verifier-output"><strong>Error:</strong><br>{}</div>"#,
                        escape_html(&vr.stderr)
                    ));
                }

                if !vr.stdout.is_empty() {
                    details_html.push_str(&format!(
                        r#"<div class="verifier-output"><strong>Output:</strong><br>{}</div>"#,
                        escape_html(&vr.stdout)
                    ));
                }

                if vr.exit_code != 0 {
                    details_html.push_str(&format!(
                        r#"<div class="verifier-output"><strong>Exit code:</strong> {}</div>"#,
                        vr.exit_code
                    ));
                }
            }

            format!(
                r#"<div class="verifier-item">
    <div class="verifier-header">
        <span class="verifier-name">{}</span>
        <span class="verifier-status {}">{}</span>
    </div>
    {}
</div>"#,
                escape_html(&vr.name),
                status_class,
                status_text,
                details_html
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<div class="verifier-results">
    <div class="json-label">Verifiers</div>
    {}
</div>"#,
        verifiers_html
    )
}

fn render_timeline(trajectory: &Trajectory) -> String {
    let markers: String = trajectory
        .steps
        .iter()
        .enumerate()
        .map(|(i, step)| {
            let current = if i == 0 { " current" } else { "" };

            // Determine if this step had failures
            let has_failure = step.rejected ||
                step.verifier_results.iter().any(|vr| !vr.passed);

            let failed_class = if has_failure { " failed" } else { "" };

            // Get tool name or action type
            let step_label = match &step.action {
                portlang_core::Action::ToolCall { tool, .. } => tool.to_string(),
                portlang_core::Action::TextOutput { .. } => "Text".to_string(),
                portlang_core::Action::Stop => "Stop".to_string(),
            };

            format!(
                r#"<div class="timeline-marker{}{}" id="marker-{}" onclick="goToStep({})" title="Step {}: {}{}"></div>"#,
                current,
                failed_class,
                i,
                i,
                i + 1,
                escape_html(&step_label),
                if has_failure { " (failed)" } else { "" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n            ");

    format!(
        r#"<div class="timeline">
    <div class="timeline-track">
        <div class="timeline-line"></div>
        <div class="timeline-markers">
            {}
        </div>
    </div>
</div>"#,
        markers
    )
}

fn render_trajectory_script(trajectory: &Trajectory) -> String {
    format!(
        r#"
let currentStep = 0;
const totalSteps = {};

function updateNavigation() {{
    document.getElementById('current-step').textContent = currentStep + 1;
    document.getElementById('total-steps').textContent = totalSteps;

    // Update button states
    document.getElementById('prev-btn').disabled = currentStep === 0;
    document.getElementById('next-btn').disabled = currentStep === totalSteps - 1;

    // Update timeline markers
    document.querySelectorAll('.timeline-marker').forEach((marker, idx) => {{
        if (idx === currentStep) {{
            marker.classList.add('current');
        }} else {{
            marker.classList.remove('current');
        }}
    }});
}}

function showStep(stepIndex) {{
    // Hide all steps
    document.querySelectorAll('.step-content').forEach(step => {{
        step.style.display = 'none';
    }});

    // Show target step
    const targetStep = document.getElementById('step-' + stepIndex);
    if (targetStep) {{
        targetStep.style.display = 'block';
    }}

    currentStep = stepIndex;
    updateNavigation();
}}

function nextStep() {{
    if (currentStep < totalSteps - 1) {{
        showStep(currentStep + 1);
    }}
}}

function previousStep() {{
    if (currentStep > 0) {{
        showStep(currentStep - 1);
    }}
}}

function goToStep(stepIndex) {{
    if (stepIndex === -1) {{
        stepIndex = totalSteps - 1;
    }}
    if (stepIndex >= 0 && stepIndex < totalSteps) {{
        showStep(stepIndex);
    }}
}}

// Keyboard navigation
document.addEventListener('keydown', (e) => {{
    if (e.key === 'ArrowRight' || e.key === 'n') {{
        nextStep();
    }} else if (e.key === 'ArrowLeft' || e.key === 'p') {{
        previousStep();
    }} else if (e.key === 'Home') {{
        goToStep(0);
    }} else if (e.key === 'End') {{
        goToStep(-1);
    }}
}});

// Toggle collapsible sections
function toggleSection(element) {{
    element.classList.toggle('collapsed');
    const content = element.nextElementSibling;
    if (content && content.classList.contains('collapsible-content')) {{
        content.classList.toggle('hidden');
    }}
}}

// Modal functions
function openModal(modalId) {{
    const modal = document.getElementById('modal-' + modalId);
    if (modal) {{
        modal.style.display = 'flex';
        document.body.style.overflow = 'hidden';
    }}
}}

function closeModal(modalId) {{
    const modal = document.getElementById('modal-' + modalId);
    if (modal) {{
        modal.style.display = 'none';
        document.body.style.overflow = 'auto';
    }}
}}

function closeModalOnBackdrop(event, modalId) {{
    if (event.target.classList.contains('modal')) {{
        closeModal(modalId);
    }}
}}

// ESC key to close modals
document.addEventListener('keydown', (e) => {{
    if (e.key === 'Escape') {{
        document.querySelectorAll('.modal').forEach(modal => {{
            modal.style.display = 'none';
        }});
        document.body.style.overflow = 'auto';
    }}
}});

// Initialize
updateNavigation();
"#,
        trajectory.step_count()
    )
}
