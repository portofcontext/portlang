use portlang_config::parse_field_from_str;
use portlang_core::McpTransport;

#[test]
fn test_mcp_server_config() {
    let toml = r#"
        name = "test-mcp"
        goal = "Test MCP server configuration"

        [model]
        name = "anthropic/claude-sonnet-4.5"

        [environment]
        type = "local"
        root = "/tmp/test"

        [[mcp_server]]
        name = "filesystem"
        command = "npx"
        args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
    "#;

    let field = parse_field_from_str(toml).unwrap();
    assert_eq!(field.mcp_servers.len(), 1);
    assert_eq!(field.mcp_servers[0].name, "filesystem");

    match &field.mcp_servers[0].transport {
        McpTransport::Stdio { command, args, .. } => {
            assert_eq!(command, "npx");
            assert_eq!(args.len(), 3);
        }
        _ => panic!("Expected Stdio transport"),
    }
}

#[test]
fn test_multiple_mcp_servers() {
    let toml = r#"
        name = "test-mcp"
        goal = "Test multiple MCP servers"

        [model]
        name = "anthropic/claude-sonnet-4.5"

        [environment]
        type = "local"
        root = "/tmp/test"

        [[mcp_server]]
        name = "filesystem"
        command = "npx"
        args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]

        [[mcp_server]]
        name = "github"
        command = "mcp-server-github"
        env = { GITHUB_TOKEN = "${GITHUB_TOKEN}" }
    "#;

    let field = parse_field_from_str(toml).unwrap();
    assert_eq!(field.mcp_servers.len(), 2);
    assert_eq!(field.mcp_servers[0].name, "filesystem");
    assert_eq!(field.mcp_servers[1].name, "github");

    match &field.mcp_servers[1].transport {
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
fn test_mcp_server_with_env_vars() {
    let toml = r#"
        name = "test-mcp"
        goal = "Test MCP server with environment variables"

        [model]
        name = "anthropic/claude-sonnet-4.5"

        [environment]
        type = "local"
        root = "/tmp/test"

        [[mcp_server]]
        name = "custom"
        command = "my-server"
        args = ["--port", "8080"]
        env = { API_KEY = "secret", DEBUG = "true" }
    "#;

    let field = parse_field_from_str(toml).unwrap();
    assert_eq!(field.mcp_servers.len(), 1);

    match &field.mcp_servers[0].transport {
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
        goal = "Test invalid transport"

        [model]
        name = "anthropic/claude-sonnet-4.5"

        [environment]
        type = "local"
        root = "/tmp/test"

        [[mcp_server]]
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
fn test_empty_mcp_server_name() {
    let toml = r#"
        name = "test-mcp"
        goal = "Test empty server name"

        [model]
        name = "anthropic/claude-sonnet-4.5"

        [environment]
        type = "local"
        root = "/tmp/test"

        [[mcp_server]]
        name = ""
        command = "test"
    "#;

    let result = parse_field_from_str(toml);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("MCP server name cannot be empty"));
}

#[test]
fn test_empty_mcp_server_command() {
    let toml = r#"
        name = "test-mcp"
        goal = "Test empty command"

        [model]
        name = "anthropic/claude-sonnet-4.5"

        [environment]
        type = "local"
        root = "/tmp/test"

        [[mcp_server]]
        name = "test"
        command = ""
    "#;

    let result = parse_field_from_str(toml);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("MCP server command cannot be empty"));
}

#[test]
fn test_mcp_server_sse_transport() {
    let toml = r#"
        name = "test-mcp"
        goal = "Test SSE/HTTP transport"

        [model]
        name = "anthropic/claude-sonnet-4.5"

        [environment]
        type = "local"
        root = "/tmp/test"

        [[mcp_server]]
        name = "remote-server"
        transport = "sse"
        url = "https://api.example.com/mcp"
        headers = { Authorization = "Bearer ${API_TOKEN}" }
    "#;

    let field = parse_field_from_str(toml).unwrap();
    assert_eq!(field.mcp_servers.len(), 1);
    assert_eq!(field.mcp_servers[0].name, "remote-server");

    match &field.mcp_servers[0].transport {
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
fn test_mcp_server_missing_url() {
    let toml = r#"
        name = "test-mcp"
        goal = "Test missing URL for SSE transport"

        [model]
        name = "anthropic/claude-sonnet-4.5"

        [environment]
        type = "local"
        root = "/tmp/test"

        [[mcp_server]]
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
