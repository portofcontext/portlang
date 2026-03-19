use std::collections::HashMap;
use tower_lsp::lsp_types::*;

pub fn hover_at(text: &str, pos: Position) -> Option<Hover> {
    let lines: Vec<&str> = text.lines().collect();
    let cursor_line = pos.line as usize;
    let line = lines.get(cursor_line)?;
    let docs = hover_docs();

    // If hovering on a section header (e.g. [prompt], [[tool]]), show section-level docs
    if let Some(section) = parse_section_header(line) {
        let doc = docs.get(section.as_str())?;
        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: doc.to_string(),
            }),
            range: None,
        });
    }

    let (section, key) = context_at(text, pos)?;
    let lookup_key = match section.as_deref() {
        Some(s) => format!("{}.{}", s, key),
        None => key.clone(),
    };
    let doc = docs
        .get(lookup_key.as_str())
        .or_else(|| docs.get(key.as_str()))?;
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: doc.to_string(),
        }),
        range: None,
    })
}

/// Returns (current_section, key_under_cursor)
fn context_at(text: &str, pos: Position) -> Option<(Option<String>, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let cursor_line = pos.line as usize;

    // Find the key on the cursor line
    let line = lines.get(cursor_line)?;
    let key = key_on_line(line, pos.character as usize)?;

    // Find the most recent section header above cursor
    let section = lines[..=cursor_line]
        .iter()
        .rev()
        .find_map(|l| parse_section_header(l));

    Some((section, key))
}

fn key_on_line(line: &str, _col: usize) -> Option<String> {
    // Return the word at col, stopping at '=' or whitespace
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return None;
    }
    // Extract key part (before '=')
    let key_part = if let Some(eq_pos) = line.find('=') {
        &line[..eq_pos]
    } else {
        line
    };
    let key = key_part.trim().to_string();
    if key.is_empty() {
        None
    } else {
        Some(key)
    }
}

fn parse_section_header(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.starts_with("[[") && trimmed.ends_with("]]") {
        Some(trimmed[2..trimmed.len() - 2].trim().to_string())
    } else if trimmed.starts_with('[') && trimmed.ends_with(']') && !trimmed.starts_with("[[") {
        Some(trimmed[1..trimmed.len() - 1].trim().to_string())
    } else {
        None
    }
}

fn hover_docs() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();

    // Top-level
    m.insert("name", "**name** *(required)*\n\nIdentifier for this field. Used in trajectory storage and `portlang list`.");
    m.insert(
        "description",
        "**description**\n\nHuman-readable summary of what this field does.",
    );

    // [model]
    m.insert("model.name", "**model.name** *(required)*\n\nModel to use.\n\n- Anthropic API: `anthropic/claude-sonnet-4.6`, `anthropic/claude-opus-4.5`\n- OpenRouter: `anthropic/claude-3.5-sonnet`\n\nProvider is auto-detected from your API key.");
    m.insert("model.temperature", "**model.temperature**\n\nSampling temperature `[0.0–1.0]`. Default: `0.5`.\n\nUse `0.0` for deterministic output (recommended for structured JSON tasks).");

    // [prompt]
    m.insert("prompt.goal", "**prompt.goal** *(required)*\n\nThe agent's initial task. Injected into context at step 0.\n\nSupports `{{ variable }}` template interpolation from `[vars]`.");
    m.insert(
        "prompt.system",
        "**prompt.system**\n\nOptional system prompt prepended to all interactions.",
    );
    m.insert("prompt.re_observation", "**prompt.re_observation**\n\nShell commands that run before each agent step to refresh context.\n\nUseful for showing workspace state, test results, etc.\n\n```toml\nre_observation = [\n  \"ls -1\",\n  \"python -m pytest --tb=no -q 2>&1 | tail -5\",\n]\n```");

    // [environment]
    m.insert("environment.root", "**environment.root**\n\nWorking directory mapped to `/workspace` inside the container. Default: `./workspace`.");
    m.insert("environment.packages", "**environment.packages**\n\napt packages to install in the container.\n\nList `\"uv\"` to get pip/uv. Example: `[\"nodejs\", \"npm\", \"uv\"]`");
    m.insert("environment.dockerfile", "**environment.dockerfile**\n\nPath to a custom Dockerfile (relative to `field.toml`). Overrides `packages`.");
    m.insert(
        "environment.image",
        "**environment.image**\n\nPre-built container image tag. Overrides `dockerfile`.",
    );

    // [boundary]
    m.insert("boundary.allow_write", "**boundary.allow_write**\n\nGlob patterns for paths the agent is allowed to write. Default: none (read-only).\n\nExample: `[\"output.json\", \"src/**/*.py\"]`");
    m.insert("boundary.network", "**boundary.network**\n\n`\"allow\"` or `\"deny\"`. Default: `\"allow\"`.\n\nSet to `\"deny\"` for tasks that should not make network requests.");
    m.insert(
        "boundary.max_tokens",
        "**boundary.max_tokens**\n\nHard ceiling on total context tokens. Agent stops if exceeded.",
    );
    m.insert(
        "boundary.max_cost",
        "**boundary.max_cost**\n\nHard ceiling on total cost. Format: `\"$0.50\"` or `0.50`.",
    );
    m.insert("boundary.max_steps", "**boundary.max_steps**\n\nHard ceiling on agent steps. Agent stops after this many tool calls.");
    m.insert(
        "boundary.bash",
        "**boundary.bash**\n\nWhether the `bash` tool is available to the agent. Default: `true`.",
    );

    // [[verifier]]
    m.insert(
        "verifier.name",
        "**verifier.name** *(required)*\n\nIdentifier for this verifier, shown in run output.",
    );
    m.insert("verifier.type", "**verifier.type**\n\nVerifier algorithm. Default: `\"shell\"`.\n\n- `shell` — exit 0 = pass\n- `json` — validates JSON structure against a schema\n- `levenshtein` — fuzzy text match\n- `semantic` — cosine similarity via embeddings\n- `tool_call` — inspects tool call arguments");
    m.insert("verifier.command", "**verifier.command**\n\nShell command for `type = \"shell\"`. Exit 0 = pass, nonzero = fail.");
    m.insert("verifier.trigger", "**verifier.trigger**\n\nWhen to run this verifier.\n\n- `on_stop` *(default)* — run when agent finishes\n- `always` — run after every tool call\n- `on_tool:<tool_name>` — run after a specific tool is called");
    m.insert("verifier.description", "**verifier.description**\n\nFeedback injected into the agent's context window when this verifier fails.");
    m.insert("verifier.file", "**verifier.file**\n\nWorkspace-relative path to the file to check (used by `json`, `levenshtein`, `semantic`).");
    m.insert(
        "verifier.expected",
        "**verifier.expected**\n\nReference string for `levenshtein` and `semantic` verifiers.",
    );
    m.insert("verifier.threshold", "**verifier.threshold**\n\nSimilarity threshold `[0.0–1.0]`. Default: `1.0` (levenshtein), `0.8` (semantic).");
    m.insert("verifier.schema", "**verifier.schema**\n\nJSON Schema string for `type = \"json\"` verifiers.\n\nExample: `'{\"type\": \"object\", \"required\": [\"status\"]}'`");
    m.insert("verifier.eval_only", "**verifier.eval_only**\n\nWhen `true`, this verifier is skipped during `portlang run` and only executes during `portlang eval run`.\n\nUse for ground-truth comparisons (`levenshtein`, `semantic`) where the expected value is known and should not influence the agent or alter the outcome of development runs.");

    // [[tool]]
    m.insert(
        "tool.type",
        "**tool.type** *(required)*\n\nTool kind: `\"python\"`, `\"shell\"`, or `\"mcp\"`.",
    );
    m.insert(
        "tool.name",
        "**tool.name**\n\nTool name exposed to the agent.",
    );
    m.insert("tool.description", "**tool.description**\n\nTool description shown to the agent. Good descriptions improve tool use.");
    m.insert("tool.file", "**tool.file**\n\nPath to the Python script (relative to `field.toml`). Used by `type = \"python\"`.");
    m.insert("tool.function", "**tool.function**\n\nPython function name to call. Omit to expose all functions in the file.");
    m.insert("tool.command", "**tool.command**\n\nShell command template for `type = \"shell\"`. Use `{param}` placeholders.");
    m.insert(
        "tool.args",
        "**tool.args**\n\nCommand arguments for MCP server (`type = \"mcp\"`).",
    );
    m.insert(
        "tool.transport",
        "**tool.transport**\n\nMCP transport: `\"stdio\"`, `\"http\"`, or `\"sse\"`.",
    );
    m.insert(
        "tool.url",
        "**tool.url**\n\nMCP server URL for HTTP/SSE transport.",
    );

    // [vars]
    m.insert("vars", "**[vars]**\n\nTemplate variable declarations. Values injected via `--var name=value` at runtime.\n\n```toml\n[vars]\ncustomer_id = { required = true, description = \"Stripe customer ID\" }\ncurrency = { required = false, default = \"usd\" }\n```");

    // Section-level hover docs
    m.insert("model", "**[model]**\n\nConfigures the language model for this field.\n\n| Field | Required | Description |\n|-------|----------|-------------|\n| `name` | ✓ | Model identifier, e.g. `\"anthropic/claude-sonnet-4.6\"` |\n| `temperature` | | Sampling temperature `0.0–1.0`. Default: `0.5` |\n\nCan also be set to `model = \"inherit\"` to use the parent config's model.");
    m.insert("prompt", "**[prompt]**\n\nDefines the agent's task and context.\n\n| Field | Required | Description |\n|-------|----------|-------------|\n| `goal` | ✓ | Initial task injected at step 0. Supports `{{ variable }}` interpolation |\n| `system` | | System prompt prepended to all interactions |\n| `re_observation` | | Shell commands run before each step to refresh context |");
    m.insert("environment", "**[environment]**\n\nConfigures the container environment the agent runs in.\n\n| Field | Required | Description |\n|-------|----------|-------------|\n| `root` | | Host path mapped to `/workspace`. Default: `./workspace` |\n| `packages` | | apt packages to install, e.g. `[\"nodejs\", \"uv\"]` |\n| `dockerfile` | | Path to a custom Dockerfile. Overrides `packages` |\n| `image` | | Pre-built container image tag. Overrides `dockerfile` |");
    m.insert("boundary", "**[boundary]**\n\nLimits on what the agent can do.\n\n| Field | Required | Description |\n|-------|----------|-------------|\n| `allow_write` | | Glob patterns for writable paths, e.g. `[\"output.json\"]` |\n| `network` | | `\"allow\"` or `\"deny\"`. Default: `\"allow\"` |\n| `max_tokens` | | Hard ceiling on total context tokens |\n| `max_cost` | | Hard ceiling on cost, e.g. `\"$0.50\"` or `0.50` |\n| `max_steps` | | Hard ceiling on agent steps |\n| `bash` | | Whether the agent has bash access. Default: `true` |\n| `output_schema` | | JSON Schema string for structured output validation |\n\nCan also be set to `boundary = \"inherit\"` to use the parent config's boundary.");
    m.insert("tool", "**[[tool]]**\n\nCustom tool exposed to the agent. Repeat the block for multiple tools.\n\n| Field | Required | Description |\n|-------|----------|-------------|\n| `type` | ✓ | `\"python\"`, `\"shell\"`, or `\"mcp\"` |\n| `name` | | Tool name shown to the agent |\n| `description` | | Tool description — good descriptions improve usage |\n| `file` | | Python script path (`type = \"python\"`) |\n| `function` | | Python function name. Omit to expose all functions |\n| `command` | | Shell command template (`type = \"shell\"`) |\n| `args` | | MCP server command arguments (`type = \"mcp\"`) |\n| `url` | | MCP server URL for HTTP/SSE transport |\n| `transport` | | MCP transport: `\"stdio\"`, `\"http\"`, or `\"sse\"` |");
    m.insert("verifier", "**[[verifier]]**\n\nPost-run check that scores agent output. Repeat the block for multiple verifiers.\n\n| Field | Required | Description |\n|-------|----------|-------------|\n| `name` | ✓ | Identifier shown in run output |\n| `type` | | `\"shell\"` (default), `\"json\"`, `\"levenshtein\"`, `\"semantic\"`, `\"tool_call\"` |\n| `command` | | Shell command for `type = \"shell\"`. Exit 0 = pass |\n| `trigger` | | `\"on_stop\"` (default), `\"always\"`, or `\"on_tool:<tool_name>\"` |\n| `description` | | Feedback injected into the agent when this verifier fails |\n| `file` | | Workspace path to the file being checked |\n| `expected` | | Reference string for `levenshtein` / `semantic` |\n| `threshold` | | Similarity threshold `0.0–1.0` |\n| `eval_only` | | Skip during `portlang run`; only runs during `portlang eval run` |");

    m
}
