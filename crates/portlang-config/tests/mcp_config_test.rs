use portlang_config::parse_field_from_str;
use portlang_core::McpTransport;

#[test]
fn test_mcp_tool_config() {
    let toml = r#"
        name = "test-mcp"

        [model]
        name = "anthropic/claude-sonnet-4.5"

        [prompt]
        goal = "Test MCP tool configuration"

        [environment]
        root = "/tmp/test"

        [[tool]]
        type = "mcp"
        name = "filesystem"
        command = "npx"
        args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
    "#;

    let field = parse_field_from_str(toml).unwrap();
    let mcp_tools: Vec<_> = field
        .tools
        .iter()
        .filter(|t| t.tool_type == "mcp")
        .collect();
    assert_eq!(mcp_tools.len(), 1);
    assert_eq!(mcp_tools[0].name.as_deref(), Some("filesystem"));

    match mcp_tools[0].transport.as_ref().unwrap() {
        McpTransport::Stdio { command, args, .. } => {
            assert_eq!(command, "npx");
            assert_eq!(args.len(), 3);
        }
        _ => panic!("Expected Stdio transport"),
    }
}

#[test]
fn test_multiple_mcp_tools() {
    let toml = r#"
        name = "test-mcp"

        [model]
        name = "anthropic/claude-sonnet-4.5"

        [prompt]
        goal = "Test multiple MCP tools"

        [environment]
        root = "/tmp/test"

        [[tool]]
        type = "mcp"
        name = "filesystem"
        command = "npx"
        args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]

        [[tool]]
        type = "mcp"
        name = "github"
        command = "mcp-server-github"
        env = { GITHUB_TOKEN = "${GITHUB_TOKEN}" }
    "#;

    let field = parse_field_from_str(toml).unwrap();
    let mcp_tools: Vec<_> = field
        .tools
        .iter()
        .filter(|t| t.tool_type == "mcp")
        .collect();
    assert_eq!(mcp_tools.len(), 2);
    assert_eq!(mcp_tools[0].name.as_deref(), Some("filesystem"));
    assert_eq!(mcp_tools[1].name.as_deref(), Some("github"));

    match mcp_tools[1].transport.as_ref().unwrap() {
        McpTransport::Stdio { env, .. } => {
            assert_eq!(
                env.get("GITHUB_TOKEN"),
                Some(&"${GITHUB_TOKEN}".to_string())
            );
        }
        _ => panic!("Expected Stdio transport"),
    }
}

#[test]
fn test_mcp_tool_with_env_vars() {
    let toml = r#"
        name = "test-mcp"

        [model]
        name = "anthropic/claude-sonnet-4.5"

        [prompt]
        goal = "Test MCP tool with environment variables"

        [environment]
        root = "/tmp/test"

        [[tool]]
        type = "mcp"
        name = "custom"
        command = "my-server"
        args = ["--port", "8080"]
        env = { API_KEY = "secret", DEBUG = "true" }
    "#;

    let field = parse_field_from_str(toml).unwrap();
    let mcp_tools: Vec<_> = field
        .tools
        .iter()
        .filter(|t| t.tool_type == "mcp")
        .collect();
    assert_eq!(mcp_tools.len(), 1);

    match mcp_tools[0].transport.as_ref().unwrap() {
        McpTransport::Stdio { env, .. } => {
            assert_eq!(env.get("API_KEY"), Some(&"secret".to_string()));
            assert_eq!(env.get("DEBUG"), Some(&"true".to_string()));
        }
        _ => panic!("Expected Stdio transport"),
    }
}

#[test]
fn test_invalid_mcp_transport() {
    let toml = r#"
        name = "test-mcp"

        [model]
        name = "anthropic/claude-sonnet-4.5"

        [prompt]
        goal = "Test invalid transport"

        [environment]
        root = "/tmp/test"

        [[tool]]
        type = "mcp"
        name = "test"
        command = "test"
        transport = "websocket"
    "#;

    let result = parse_field_from_str(toml);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid MCP transport"));
}

#[test]
fn test_empty_mcp_tool_name() {
    let toml = r#"
        name = "test-mcp"

        [model]
        name = "anthropic/claude-sonnet-4.5"

        [prompt]
        goal = "Test empty tool name"

        [environment]
        root = "/tmp/test"

        [[tool]]
        type = "mcp"
        name = ""
        command = "test"
    "#;

    let result = parse_field_from_str(toml);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("MCP tool name cannot be empty"));
}

#[test]
fn test_empty_mcp_tool_command() {
    let toml = r#"
        name = "test-mcp"

        [model]
        name = "anthropic/claude-sonnet-4.5"

        [prompt]
        goal = "Test empty command"

        [environment]
        root = "/tmp/test"

        [[tool]]
        type = "mcp"
        name = "test"
        command = ""
    "#;

    let result = parse_field_from_str(toml);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("MCP tool command cannot be empty"));
}

#[test]
fn test_mcp_tool_sse_transport() {
    let toml = r#"
        name = "test-mcp"

        [model]
        name = "anthropic/claude-sonnet-4.5"

        [prompt]
        goal = "Test SSE/HTTP transport"

        [environment]
        root = "/tmp/test"

        [[tool]]
        type = "mcp"
        name = "remote-server"
        transport = "sse"
        url = "https://api.example.com/mcp"
        headers = { Authorization = "Bearer ${API_TOKEN}" }
    "#;

    let field = parse_field_from_str(toml).unwrap();
    let mcp_tools: Vec<_> = field
        .tools
        .iter()
        .filter(|t| t.tool_type == "mcp")
        .collect();
    assert_eq!(mcp_tools.len(), 1);
    assert_eq!(mcp_tools[0].name.as_deref(), Some("remote-server"));

    match mcp_tools[0].transport.as_ref().unwrap() {
        McpTransport::Sse { url, headers } => {
            assert_eq!(url, "https://api.example.com/mcp");
            assert_eq!(
                headers.get("Authorization"),
                Some(&"Bearer ${API_TOKEN}".to_string())
            );
        }
        _ => panic!("Expected SSE transport"),
    }
}

#[test]
fn test_mcp_tool_missing_url() {
    let toml = r#"
        name = "test-mcp"

        [model]
        name = "anthropic/claude-sonnet-4.5"

        [prompt]
        goal = "Test missing URL for SSE transport"

        [environment]
        root = "/tmp/test"

        [[tool]]
        type = "mcp"
        name = "remote-server"
        transport = "sse"
    "#;

    let result = parse_field_from_str(toml);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("requires 'url' field"));
}
