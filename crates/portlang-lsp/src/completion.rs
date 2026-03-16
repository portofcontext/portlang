use tower_lsp::lsp_types::*;

pub fn completions_at(text: &str, pos: Position) -> Vec<CompletionItem> {
    let section = current_section(text, pos.line as usize);
    match section.as_deref() {
        None | Some("") => top_level_completions(),
        Some("model") => model_completions(),
        Some("prompt") => prompt_completions(),
        Some("environment") => environment_completions(),
        Some("boundary") => boundary_completions(),
        Some("verifier") => verifier_completions(),
        Some("tool") => tool_completions(),
        Some("vars") => vars_completions(),
        _ => vec![],
    }
}

fn current_section(text: &str, cursor_line: usize) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    lines[..=cursor_line.min(lines.len().saturating_sub(1))]
        .iter()
        .rev()
        .find_map(|line| {
            let t = line.trim();
            if t.starts_with("[[") && t.ends_with("]]") {
                Some(t[2..t.len() - 2].trim().to_string())
            } else if t.starts_with('[') && t.ends_with(']') && !t.starts_with("[[") {
                Some(t[1..t.len() - 1].trim().to_string())
            } else {
                None
            }
        })
}

fn snippet(label: &str, detail: &str, insert: &str) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        detail: Some(detail.to_string()),
        kind: Some(CompletionItemKind::FIELD),
        insert_text: Some(insert.to_string()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        ..Default::default()
    }
}

fn keyword(label: &str, detail: &str) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        detail: Some(detail.to_string()),
        kind: Some(CompletionItemKind::VALUE),
        ..Default::default()
    }
}

fn top_level_completions() -> Vec<CompletionItem> {
    vec![
        snippet("name", "Field name (required)", "name = \"$1\""),
        snippet("description", "Human-readable description", "description = \"$1\""),
        snippet("[model]", "Model configuration", "[model]\nname = \"${1:anthropic/claude-sonnet-4.6}\"\ntemperature = ${2:0.5}"),
        snippet("[prompt]", "Prompt configuration", "[prompt]\ngoal = \"$1\""),
        snippet("[environment]", "Environment configuration", "[environment]\nroot = \"${1:./workspace}\""),
        snippet("[boundary]", "Boundary/limits configuration", "[boundary]\nallow_write = [\"${1:output.json}\"]\nmax_steps = ${2:20}\nmax_cost = \"${3:\\$1.00}\""),
        snippet("[[verifier]]", "Add a verifier", "[[verifier]]\nname = \"${1:check}\"\ncommand = \"${2:test -f /workspace/output.json}\"\ntrigger = \"on_stop\"\ndescription = \"${3:Description shown on failure}\""),
        snippet("[[tool]]", "Add a custom tool", "[[tool]]\ntype = \"${1:python}\"\nname = \"${2:my_tool}\"\nfile = \"${3:./tools/my_tool.py}\""),
        snippet("[vars]", "Template variables", "[vars]\n${1:my_var} = { required = ${2:true}, description = \"${3:}\" }"),
        snippet("model = \"inherit\"", "Inherit model from parent", "model = \"inherit\""),
        snippet("boundary = \"inherit\"", "Inherit boundary from parent", "boundary = \"inherit\""),
        snippet("tools = \"inherit\"", "Inherit tools from parent", "tools = \"inherit\""),
    ]
}

fn model_completions() -> Vec<CompletionItem> {
    vec![
        snippet(
            "name",
            "Model name",
            "name = \"${1:anthropic/claude-sonnet-4.6}\"",
        ),
        snippet(
            "temperature",
            "Sampling temperature [0.0-1.0]",
            "temperature = ${1:0.5}",
        ),
        keyword(
            "anthropic/claude-sonnet-4.6",
            "Claude Sonnet 4.6 (Anthropic API)",
        ),
        keyword(
            "anthropic/claude-opus-4.5",
            "Claude Opus 4.5 (Anthropic API)",
        ),
        keyword(
            "anthropic/claude-haiku-4.5-20251001",
            "Claude Haiku 4.5 (Anthropic API)",
        ),
        keyword(
            "anthropic/claude-3.5-sonnet",
            "Claude 3.5 Sonnet (OpenRouter)",
        ),
    ]
}

fn prompt_completions() -> Vec<CompletionItem> {
    vec![
        snippet("goal", "Agent goal (required)", "goal = \"\"\"\n$1\n\"\"\""),
        snippet("system", "System prompt", "system = \"$1\""),
        snippet(
            "re_observation",
            "Commands run before each step",
            "re_observation = [\n  \"$1\",\n]",
        ),
    ]
}

fn environment_completions() -> Vec<CompletionItem> {
    vec![
        snippet(
            "root",
            "Workspace root directory",
            "root = \"${1:./workspace}\"",
        ),
        snippet(
            "packages",
            "apt packages to install",
            "packages = [\"${1:uv}\"]",
        ),
        snippet(
            "dockerfile",
            "Custom Dockerfile path",
            "dockerfile = \"${1:./Dockerfile}\"",
        ),
        snippet(
            "image",
            "Pre-built image tag",
            "image = \"${1:myimage:latest}\"",
        ),
    ]
}

fn boundary_completions() -> Vec<CompletionItem> {
    vec![
        snippet(
            "allow_write",
            "Writable glob patterns",
            "allow_write = [\"${1:output.json}\"]",
        ),
        snippet(
            "network",
            "Network policy",
            "network = \"${1|allow,deny|}\"",
        ),
        snippet(
            "max_tokens",
            "Max context tokens",
            "max_tokens = ${1:100000}",
        ),
        snippet("max_cost", "Max cost ceiling", "max_cost = \"\\$$1\""),
        snippet("max_steps", "Max agent steps", "max_steps = ${1:20}"),
        snippet(
            "bash",
            "Enable/disable bash tool",
            "bash = ${1|true,false|}",
        ),
    ]
}

fn verifier_completions() -> Vec<CompletionItem> {
    vec![
        snippet("name", "Verifier name", "name = \"$1\""),
        snippet(
            "type",
            "Verifier type",
            "type = \"${1|shell,json,levenshtein,semantic,tool_call|}\"",
        ),
        snippet(
            "command",
            "Shell command (exit 0 = pass)",
            "command = \"$1\"",
        ),
        snippet(
            "trigger",
            "When to run",
            "trigger = \"${1|on_stop,always,on_tool:|}\"",
        ),
        snippet("description", "Feedback on failure", "description = \"$1\""),
        snippet("file", "File to check", "file = \"${1:output.json}\""),
        snippet("expected", "Expected content", "expected = \"$1\""),
        snippet(
            "threshold",
            "Similarity threshold [0.0-1.0]",
            "threshold = ${1:0.9}",
        ),
        snippet("schema", "JSON Schema", "schema = '$1'"),
    ]
}

fn tool_completions() -> Vec<CompletionItem> {
    vec![
        snippet("type", "Tool type", "type = \"${1|python,shell,mcp|}\""),
        snippet("name", "Tool name", "name = \"$1\""),
        snippet("description", "Tool description", "description = \"$1\""),
        snippet(
            "file",
            "Python script path",
            "file = \"${1:./tools/my_tool.py}\"",
        ),
        snippet(
            "function",
            "Python function to call",
            "function = \"${1:execute}\"",
        ),
        snippet(
            "command",
            "Shell command template",
            "command = \"${1:echo {input}}\"",
        ),
        snippet(
            "args",
            "MCP server arguments",
            "args = [\"${1:-y}\", \"${2:@scope/package}\"]",
        ),
        snippet(
            "transport",
            "MCP transport",
            "transport = \"${1|stdio,http,sse|}\"",
        ),
        snippet("url", "MCP server URL", "url = \"${1:https://}\""),
        snippet(
            "env",
            "Environment variables",
            "env = { ${1:KEY} = \"${2:\\${ENV_VAR}}\" }",
        ),
    ]
}

fn vars_completions() -> Vec<CompletionItem> {
    vec![
        snippet(
            "variable",
            "Template variable declaration",
            "${1:var_name} = { required = ${2|true,false|}, default = \"${3:}\", description = \"${4:}\" }",
        ),
    ]
}
