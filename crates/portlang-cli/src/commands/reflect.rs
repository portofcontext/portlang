use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use portlang_config::{apply_runtime_context, parse_field_from_str};
use portlang_core::RuntimeContext;
use portlang_provider_anthropic::AnthropicProvider;
use portlang_provider_openrouter::OpenRouterProvider;
use portlang_runner_claudecode::{run_field_with_claude_code, with_required_packages};
use portlang_runtime::{run_field, sandbox::create_sandbox, tools::ToolRegistry, ModelProvider};
use std::collections::HashMap;
use std::sync::Arc;

const REFLECT_FIELD: &str = include_str!("../reflect_tools/reflect.field");
const LIST_TRAJECTORIES_PY: &str = include_str!("../reflect_tools/list_trajectories.py");
const LOAD_TRAJECTORY_PY: &str = include_str!("../reflect_tools/load_trajectory.py");

pub async fn reflect_command(
    field: Option<String>,
    trajectory_id: Option<String>,
    trajectories: usize,
    runner: String,
) -> Result<()> {
    let home = dirs::home_dir().context("cannot determine home directory")?;
    let trajectories_dir = home.join(".portlang").join("trajectories");

    // ── Resolve field name ────────────────────────────────────────────────────
    let field_name = if let Some(f) = field {
        f
    } else if let Some(ref tid) = trajectory_id {
        // Search all field directories for one containing this trajectory ID
        let mut found: Option<String> = None;
        if trajectories_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&trajectories_dir) {
                for entry in entries.flatten() {
                    if entry.path().is_dir() {
                        let candidate = entry.path().join(format!("{tid}.json"));
                        if candidate.exists() {
                            if let Some(name) = entry.file_name().to_str() {
                                found = Some(name.to_string());
                                break;
                            }
                        }
                    }
                }
            }
        }
        found.with_context(|| format!("trajectory \"{}\" not found in any field", tid))?
    } else {
        anyhow::bail!("--field is required when --trajectory-id is not specified");
    };

    // ── Validate field name exists ────────────────────────────────────────────
    {
        let field_dir = trajectories_dir.join(&field_name);

        if !field_dir.exists() {
            // Collect all known field names
            let mut known: Vec<String> = Vec::new();
            if trajectories_dir.exists() {
                if let Ok(entries) = std::fs::read_dir(&trajectories_dir) {
                    for entry in entries.flatten() {
                        if entry.path().is_dir() {
                            if let Some(name) = entry.file_name().to_str() {
                                known.push(name.to_string());
                            }
                        }
                    }
                }
            }
            known.sort();

            let mut msg = format!("no trajectories found for field \"{}\"", field_name);
            if !known.is_empty() {
                let mut scored: Vec<(&String, usize)> = known
                    .iter()
                    .map(|k| (k, token_similarity(&field_name, k)))
                    .collect();
                scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
                let suggestions: Vec<&str> = scored
                    .iter()
                    .filter(|(_, s)| *s > 0)
                    .take(3)
                    .map(|(n, _)| n.as_str())
                    .collect();
                if !suggestions.is_empty() {
                    msg.push_str("\n\nSimilar field names:\n");
                    for s in &suggestions {
                        msg.push_str(&format!("  {}\n", s));
                    }
                } else {
                    msg.push_str("\n\nAvailable fields:\n");
                    for k in known.iter().take(10) {
                        msg.push_str(&format!("  {}\n", k));
                    }
                }
            } else {
                msg.push_str("\n\nNo fields have trajectories yet.");
            }
            anyhow::bail!("{}", msg);
        }
    }

    // ── Stage built-in Python tools to ~/.portlang/builtin/reflect/tools/ ───
    let portlang_root = home.join(".portlang");
    let tools_dir = portlang_root.join("builtin/reflect/tools");
    std::fs::create_dir_all(&tools_dir)?;
    std::fs::write(tools_dir.join("list_trajectories.py"), LIST_TRAJECTORIES_PY)?;
    std::fs::write(tools_dir.join("load_trajectory.py"), LOAD_TRAJECTORY_PY)?;

    // ── Build goal ───────────────────────────────────────────────────────────
    let goal = if let Some(ref tid) = trajectory_id {
        format!(
            r#"Analyze the specific trajectory "{tid}" for field "{field_name}".

Use only the load_trajectory tool — do not use Bash, Read, or Glob.

1. Call load_trajectory with field_name="{field_name}" and trajectory_id="{tid}".

2. Examine the trajectory:
   - Which steps have the highest input_tokens? (context growth hotspots)
   - Are any steps rejected=true? What tool was being called?
   - Which steps have result_truncated=true? How large is result_length? (over-fetching)
   - Are any verifier_results passed=false? What does the stderr say?
   - Are there repeated tool calls with similar tool_params? (stuck behavior)
   - Are text_output steps necessary, or are they pure narration waste?

3. Submit your findings: a natural language analysis paragraph covering what you found,
   with specific data points woven in. Then a short prioritized list of actionable
   recommendations for improving the field."#
        )
    } else {
        format!(
            r#"Analyze the {trajectories} most recent trajectories for the portlang field "{field_name}".

Use only the list_trajectories and load_trajectory tools — do not use Bash, Read, or Glob.

1. Call list_trajectories to get an overview of recent runs (outcome, tokens, cost, verifiers).

2. For each trajectory, call load_trajectory and examine:
   - Which steps have the highest input_tokens? (context growth hotspots)
   - Are any steps rejected=true? What tool was being called?
   - Which steps have result_truncated=true? How large is result_length? (over-fetching)
   - Are any verifier_results passed=false? What does the stderr say?
   - Are there repeated tool calls with similar tool_params? (stuck behavior)
   - Are text_output steps necessary, or are they pure narration waste?

3. Look for patterns across all trajectories, not just individual runs.

4. Submit your findings: a natural language analysis paragraph (or two) covering what you
   found, with specific data points woven in where they matter. Then a short prioritized
   list of actionable recommendations for improving the field."#
        )
    };

    // ── Parse the embedded field, substituting vars ──────────────────────────
    // portlang_root must be substituted before parse_field_from_str because the
    // parser reads tool files immediately to extract their schema.
    let field_toml = REFLECT_FIELD.replace("{{ portlang_root }}", &portlang_root.to_string_lossy());
    let mut vars = HashMap::new();
    vars.insert("goal".to_string(), goal);
    let ctx = RuntimeContext { vars, input: None };
    let field_config = apply_runtime_context(parse_field_from_str(&field_toml)?, &ctx)?;

    // ── Spinner ──────────────────────────────────────────────────────────────
    let label = trajectory_id.as_deref().unwrap_or(&field_name).to_string();
    println!("Reflecting on: {label}");
    println!();

    crate::progress::set_status("");
    let progress = ProgressBar::new_spinner();
    progress.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .unwrap(),
    );
    progress.enable_steady_tick(std::time::Duration::from_millis(100));
    let pb = progress.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
            let msg = crate::progress::get_status();
            if !msg.is_empty() {
                pb.set_message(msg);
            }
        }
    });

    // ── Run ──────────────────────────────────────────────────────────────────
    let trajectory = match runner.as_str() {
        "claude-code" => {
            let env = with_required_packages(
                &field_config.environment,
                &field_config.tools,
                field_config.boundary.output_schema.is_some(),
            );
            let sandbox = create_sandbox(
                &env,
                &field_config.boundary,
                Arc::new(ToolRegistry::new()),
                None,
                None,
                None,
            )
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create sandbox: {}", e))?;
            run_field_with_claude_code(&field_config, &ctx, sandbox).await?
        }
        _ => {
            let provider: Box<dyn ModelProvider> = if field_config.model.name.contains('/') {
                let mut p = OpenRouterProvider::from_env(&field_config.model.name)?;
                if let Some(temp) = field_config.model.temperature {
                    p = p.with_temperature(temp);
                }
                Box::new(p)
            } else {
                let mut p = AnthropicProvider::from_env(&field_config.model.name)?;
                if let Some(temp) = field_config.model.temperature {
                    p = p.with_temperature(temp);
                }
                Box::new(p)
            };
            run_field(&field_config, provider.as_ref(), &ctx).await?
        }
    };

    progress.finish_and_clear();

    // ── Parse structured output ───────────────────────────────────────────────
    let output = trajectory
        .structured_output
        .as_ref()
        .context("no structured output — the agent did not call submit_output")?;

    let analysis = output["analysis"].as_str().unwrap_or("").trim().to_string();
    let recommendations = output["recommendations"]
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or(&[]);

    // ── Display ──────────────────────────────────────────────────────────────
    let width = term_width();
    let divider = "─".repeat(width.min(72));
    let heavy = "━".repeat(width.min(72));

    println!("{heavy}");
    println!(" Reflection: {label}");
    if trajectory_id.is_none() {
        let n = trajectories;
        let cost = trajectory.total_cost;
        println!(" {n} trajectories  ·  ${:.4}", cost.to_dollars());
    }
    println!("{heavy}");
    println!();

    println!(" Analysis");
    println!(" {divider}");
    for line in wrap_text(&analysis, width.saturating_sub(3)) {
        println!("  {line}");
    }
    println!();

    if !recommendations.is_empty() {
        println!(" Recommendations");
        println!(" {divider}");
        for rec in recommendations {
            let priority = rec["priority"].as_str().unwrap_or("medium");
            let suggestion = rec["suggestion"].as_str().unwrap_or("").trim();
            let rationale = rec["rationale"].as_str().unwrap_or("").trim();
            let (bullet, color) = priority_style(priority);
            println!();
            println!("  {}{} {}{}", color, bullet, suggestion, RESET);
            for line in wrap_text(rationale, width.saturating_sub(5)) {
                println!("    {line}");
            }
        }
        println!();
    }

    println!("{heavy}");
    println!();

    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn priority_style(p: &str) -> (&'static str, &'static str) {
    match p.to_lowercase().as_str() {
        "high" => ("● HIGH  ", RED),
        "medium" => ("○ MED   ", YELLOW),
        _ => ("◦ LOW   ", DIM),
    }
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width < 10 {
        return text.lines().map(|l| l.to_string()).collect();
    }
    let mut lines = Vec::new();
    for paragraph in text.split("\n\n") {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        let words: Vec<&str> = paragraph.split_whitespace().collect();
        let mut current = String::new();
        for word in words {
            if current.is_empty() {
                current.push_str(word);
            } else if current.len() + 1 + word.len() <= width {
                current.push(' ');
                current.push_str(word);
            } else {
                lines.push(current.clone());
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    lines
}

fn term_width() -> usize {
    if let Some((w, _)) = term_size::dimensions() {
        w
    } else {
        80
    }
}

const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

fn token_similarity(a: &str, b: &str) -> usize {
    let split = |s: &str| -> std::collections::HashSet<String> {
        s.split(['-', '_']).map(|t| t.to_lowercase()).collect()
    };
    let a_tokens = split(a);
    let b_tokens = split(b);
    a_tokens.intersection(&b_tokens).count()
}

mod term_size {
    pub fn dimensions() -> Option<(usize, usize)> {
        std::env::var("COLUMNS")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .map(|w| (w, 24))
    }
}
