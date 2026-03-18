/// Generate Claude Code MCP config from field tool definitions.
///
/// Claude Code accepts `--mcp-config <path>` pointing to a JSON file with:
/// ```json
/// {
///   "mcpServers": {
///     "server-name": {
///       "command": "npx",
///       "args": ["-y", "@stripe/mcp"],
///       "env": {"STRIPE_SECRET_KEY": "..."}
///     }
///   }
/// }
/// ```
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use portlang_core::Tool;
use serde_json::{json, Value};
use std::path::Path;

/// Build the combined mcpServers JSON for all tools in the field:
/// - MCP tools → passed through as-is
/// - Shell tools → generate a Python MCP stdio server script in the workspace
/// - Python tools → generate a Python MCP stdio server script (base64-embedded) in the workspace
///
/// Returns None if the field has no tools at all.
/// Also returns the list of generated filenames so the caller can clean them up.
pub fn build_all_mcp_config(
    tools: &[Tool],
    workspace: &Path,
) -> Result<Option<(Value, Vec<String>)>> {
    if tools.is_empty() {
        return Ok(None);
    }

    let mut servers = serde_json::Map::new();
    let mut generated_files: Vec<String> = Vec::new();

    // MCP tools: pass through directly
    for tool in tools.iter().filter(|t| t.tool_type == "mcp") {
        let name = tool
            .name
            .clone()
            .unwrap_or_else(|| "mcp-server".to_string());

        let server = if let Some(ref url) = tool.url {
            let transport = tool.transport.as_ref().map(|_| "sse").unwrap_or("sse");
            let mut obj = serde_json::Map::new();
            obj.insert("url".to_string(), json!(url));
            obj.insert("transport".to_string(), json!(transport));
            if let Some(ref headers) = tool.headers {
                obj.insert("headers".to_string(), json!(headers));
            }
            Value::Object(obj)
        } else {
            let command = tool.command.clone().unwrap_or_default();
            let mut obj = serde_json::Map::new();
            obj.insert("command".to_string(), json!(command));
            obj.insert("args".to_string(), json!(tool.args));
            if !tool.env.is_empty() {
                obj.insert("env".to_string(), json!(tool.env));
            }
            Value::Object(obj)
        };

        servers.insert(name, server);
    }

    // Shell tools: generate Python MCP stdio server scripts
    for tool in tools.iter().filter(|t| t.tool_type == "shell") {
        let name = tool
            .name
            .clone()
            .unwrap_or_else(|| "shell-tool".to_string());
        let filename = format!(".portlang_mcp_{}.py", sanitize_name(&name));

        let script = generate_shell_mcp_script(tool);
        std::fs::write(workspace.join(&filename), &script).with_context(|| {
            format!(
                "Failed to write MCP server script for shell tool '{}'",
                name
            )
        })?;
        generated_files.push(filename.clone());

        let server = json!({
            "command": "python3",
            "args": [format!("/workspace/{}", filename)]
        });
        servers.insert(name, server);
    }

    // Python tools: generate base64-embedded Python MCP stdio server scripts
    for tool in tools.iter().filter(|t| t.tool_type == "python") {
        let name = tool
            .name
            .clone()
            .unwrap_or_else(|| "python-tool".to_string());
        let filename = format!(".portlang_mcp_{}.py", sanitize_name(&name));

        let script = generate_python_mcp_script(tool)
            .with_context(|| format!("Failed to generate MCP server for python tool '{}'", name))?;
        std::fs::write(workspace.join(&filename), &script).with_context(|| {
            format!(
                "Failed to write MCP server script for python tool '{}'",
                name
            )
        })?;
        generated_files.push(filename.clone());

        let server = json!({
            "command": "uv",
            "args": ["run", format!("/workspace/{}", filename)]
        });
        servers.insert(name, server);
    }

    if servers.is_empty() {
        return Ok(None);
    }

    Ok(Some((
        json!({ "mcpServers": Value::Object(servers) }),
        generated_files,
    )))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Expand `${VAR}` references in a shell command template using the host environment.
/// Same logic as `ShellCommandHandler::render_command` in portlang-runtime.
fn expand_env_vars(cmd: &str) -> String {
    let mut result = String::new();
    let mut chars = cmd.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'{') {
            chars.next(); // consume '{'
            let mut var_name = String::new();
            for nc in chars.by_ref() {
                if nc == '}' {
                    break;
                }
                var_name.push(nc);
            }
            let val = std::env::var(&var_name).unwrap_or_default();
            result.push_str(&val);
        } else {
            result.push(c);
        }
    }
    result
}

/// Extract PEP 723 inline script metadata block from a Python script.
/// Same logic as `PythonToolHandler::extract_pep723_header` in portlang-runtime.
fn extract_pep723_header(content: &str) -> String {
    let mut in_block = false;
    let mut lines: Vec<&str> = Vec::new();
    for line in content.lines() {
        if line.trim() == "# /// script" {
            in_block = true;
            lines.push(line);
        } else if in_block {
            lines.push(line);
            if line.trim() == "# ///" {
                break;
            }
        }
    }
    if lines.is_empty() {
        String::new()
    } else {
        lines.join("\n") + "\n"
    }
}

/// Generate a Python MCP stdio server script for a shell tool.
///
/// `${VAR}` env references in the command template are expanded at generation
/// time from the host environment (same as the native ShellCommandHandler).
/// `{param}` placeholders are substituted at call time by the MCP server.
fn generate_shell_mcp_script(tool: &Tool) -> String {
    let name = tool.name.clone().unwrap_or_default();
    let description = tool.description.clone().unwrap_or_default();
    let schema_json =
        serde_json::to_string(&tool.input_schema).unwrap_or_else(|_| "{}".to_string());
    let command_expanded = expand_env_vars(tool.command.as_deref().unwrap_or(""));
    // Escape backslashes and double-quotes so the value embeds safely in a Python double-quoted string.
    let command_escaped = command_expanded.replace('\\', "\\\\").replace('"', "\\\"");
    let desc_json = serde_json::to_string(&description).unwrap_or_else(|_| "\"\"".to_string());

    format!(
        r#"#!/usr/bin/env python3
"""MCP stdio server for portlang shell tool: {name}"""
import json, sys, subprocess

TOOL_NAME = "{name}"
TOOL_DESCRIPTION = {desc_json}
TOOL_SCHEMA = {schema_json}
COMMAND_TEMPLATE = "{command_escaped}"


def call_tool(arguments):
    cmd = COMMAND_TEMPLATE
    for k, v in arguments.items():
        cmd = cmd.replace("{{" + k + "}}", str(v))
    r = subprocess.run(cmd, shell=True, capture_output=True, text=True, cwd="/workspace")
    output = r.stdout
    if r.returncode != 0 and r.stderr:
        output += "\nstderr: " + r.stderr
    return output or "(no output)"


for raw in sys.stdin:
    raw = raw.strip()
    if not raw:
        continue
    try:
        msg = json.loads(raw)
    except Exception:
        continue
    method = msg.get("method", "")
    mid = msg.get("id")
    if method == "initialize":
        r = {{"jsonrpc": "2.0", "id": mid, "result": {{"protocolVersion": "2024-11-05", "capabilities": {{"tools": {{}}}}, "serverInfo": {{"name": TOOL_NAME, "version": "1.0.0"}}}}}}
    elif method in ("notifications/initialized", "notifications/cancelled"):
        continue
    elif method == "tools/list":
        r = {{"jsonrpc": "2.0", "id": mid, "result": {{"tools": [{{"name": TOOL_NAME, "description": TOOL_DESCRIPTION, "inputSchema": TOOL_SCHEMA}}]}}}}
    elif method == "tools/call":
        try:
            args = msg.get("params", {{}}).get("arguments", {{}})
            out = call_tool(args)
            r = {{"jsonrpc": "2.0", "id": mid, "result": {{"content": [{{"type": "text", "text": out}}]}}}}
        except Exception as e:
            r = {{"jsonrpc": "2.0", "id": mid, "result": {{"content": [{{"type": "text", "text": f"Error: {{e}}"}}], "isError": True}}}}
    elif mid is not None:
        r = {{"jsonrpc": "2.0", "id": mid, "error": {{"code": -32601, "message": f"Unknown method: {{method}}"}}}}
    else:
        continue
    if mid is not None:
        print(json.dumps(r), flush=True)
"#,
        name = name,
        desc_json = desc_json,
        schema_json = schema_json,
        command_escaped = command_escaped,
    )
}

/// Generate a Python MCP stdio server script for a python tool.
///
/// The user's script is embedded as base64 (same technique as the native
/// PythonToolHandler) so the MCP server is fully self-contained.  Any PEP 723
/// inline script metadata is forwarded so `uv run` installs declared dependencies.
fn generate_python_mcp_script(tool: &Tool) -> Result<String> {
    let name = tool.name.clone().unwrap_or_default();
    let description = tool.description.clone().unwrap_or_default();
    let schema_json =
        serde_json::to_string(&tool.input_schema).unwrap_or_else(|_| "{}".to_string());
    let function = tool
        .function
        .clone()
        .unwrap_or_else(|| "execute".to_string());

    let file_path = tool
        .file
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Python tool '{}' missing 'file' field", name))?;

    let script_content = std::fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read Python tool script '{}'", file_path))?;

    let pep723_header = extract_pep723_header(&script_content);
    let encoded = BASE64.encode(script_content.as_bytes());
    let desc_json = serde_json::to_string(&description).unwrap_or_else(|_| "\"\"".to_string());

    Ok(format!(
        r#"#!/usr/bin/env python3
{pep723_header}"""MCP stdio server for portlang python tool: {name}"""
import json, sys, base64, types

TOOL_NAME = "{name}"
TOOL_DESCRIPTION = {desc_json}
TOOL_SCHEMA = {schema_json}
FUNCTION_NAME = "{function}"
SCRIPT_B64 = "{encoded}"

_module = None


def load_module():
    script = base64.b64decode(SCRIPT_B64).decode("utf-8")
    mod = types.ModuleType("tool_module")
    exec(compile(script, "tool_module", "exec"), mod.__dict__)
    return mod


def call_tool(arguments):
    global _module
    if _module is None:
        _module = load_module()
    fn = getattr(_module, FUNCTION_NAME)
    result = fn(**arguments)
    if hasattr(result, "model_dump"):
        result = result.model_dump()
    elif hasattr(result, "dict") and callable(result.dict):
        result = result.dict()
    return json.dumps(result)


for raw in sys.stdin:
    raw = raw.strip()
    if not raw:
        continue
    try:
        msg = json.loads(raw)
    except Exception:
        continue
    method = msg.get("method", "")
    mid = msg.get("id")
    if method == "initialize":
        r = {{"jsonrpc": "2.0", "id": mid, "result": {{"protocolVersion": "2024-11-05", "capabilities": {{"tools": {{}}}}, "serverInfo": {{"name": TOOL_NAME, "version": "1.0.0"}}}}}}
    elif method in ("notifications/initialized", "notifications/cancelled"):
        continue
    elif method == "tools/list":
        r = {{"jsonrpc": "2.0", "id": mid, "result": {{"tools": [{{"name": TOOL_NAME, "description": TOOL_DESCRIPTION, "inputSchema": TOOL_SCHEMA}}]}}}}
    elif method == "tools/call":
        try:
            args = msg.get("params", {{}}).get("arguments", {{}})
            out = call_tool(args)
            r = {{"jsonrpc": "2.0", "id": mid, "result": {{"content": [{{"type": "text", "text": out}}]}}}}
        except Exception as e:
            import traceback
            r = {{"jsonrpc": "2.0", "id": mid, "result": {{"content": [{{"type": "text", "text": f"Error: {{e}}\n{{traceback.format_exc()}}"}}], "isError": True}}}}
    elif mid is not None:
        r = {{"jsonrpc": "2.0", "id": mid, "error": {{"code": -32601, "message": f"Unknown method: {{method}}"}}}}
    else:
        continue
    if mid is not None:
        print(json.dumps(r), flush=True)
"#,
        pep723_header = pep723_header,
        name = name,
        desc_json = desc_json,
        schema_json = schema_json,
        function = function,
        encoded = encoded,
    ))
}
