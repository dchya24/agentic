//! Unit tests for core-agentic

use crate::tool::{Tool, ToolCall, ToolSchema};
use crate::tools::RunCommandTool;
use crate::{Memory, Message, MessageRole, RiskLevel, ToolRegistry};

#[test]
fn test_tool_call_new() {
    let call = ToolCall::new("echo", serde_json::json!({"text": "hello"}));
    assert_eq!(call.tool_name, "echo");
    assert_eq!(call.arguments["text"], "hello");
    assert!(!call.id.is_empty());
}

#[test]
fn test_tool_schema_new() {
    let schema = ToolSchema::new("echo", "Echoes back the input");
    assert_eq!(schema.name, "echo");
    assert_eq!(schema.description, "Echoes back the input");
    assert!(schema.parameters.is_empty());
}

#[test]
fn test_tool_schema_with_param() {
    let schema =
        ToolSchema::new("echo", "Echoes back the input").with_param("text", "string", true);

    assert!(schema.parameters.contains_key("text"));
    assert!(schema.required.contains(&"text".to_string()));
}

#[test]
fn test_memory_new() {
    let memory = Memory::new(1000);
    assert_eq!(memory.max_tokens, 1000);
}

#[test]
fn test_memory_add_message() {
    let mut memory = Memory::new(1000);
    memory.add_message(Message::user("Hello"));
    memory.add_message(Message::assistant("Hi there"));

    let context = memory.get_context(10);
    assert_eq!(context.len(), 2);
}

#[test]
fn test_message_user() {
    let msg = Message::user("test content");
    assert!(matches!(msg.role, MessageRole::User));
    assert_eq!(msg.content, "test content");
}

#[test]
fn test_message_assistant() {
    let msg = Message::assistant("response");
    assert!(matches!(msg.role, MessageRole::Assistant));
    assert_eq!(msg.content, "response");
}

#[test]
fn test_message_tool() {
    let msg = Message::tool("my_tool", "call_id", "output");
    assert!(matches!(msg.role, MessageRole::Tool { .. }));
}

#[test]
fn test_risk_level_confirmation() {
    let low = RiskLevel::Low;
    let medium = RiskLevel::Medium;
    let high = RiskLevel::High;
    let critical = RiskLevel::Critical;

    assert!(!low.requires_confirmation());
    assert!(medium.requires_confirmation());
    assert!(high.requires_confirmation());
    assert!(critical.requires_confirmation());
}

#[test]
fn test_run_command_tool_schema() {
    let tool = RunCommandTool::new();
    assert_eq!(tool.name(), "run_command");
    assert_eq!(
        tool.description(),
        "Execute a shell command and return its output"
    );

    let schema = tool.schema();
    assert_eq!(schema.name, "run_command");
    assert!(schema.parameters.contains_key("command"));
    assert!(schema.required.contains(&"command".to_string()));
}

#[test]
fn test_run_command_tool_execute() {
    let tool = RunCommandTool::new();
    let result = tool.execute(serde_json::json!({"command": "echo hello"}));

    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.get("success").unwrap().as_bool().unwrap());
    assert!(output
        .get("stdout")
        .unwrap()
        .as_str()
        .unwrap()
        .contains("hello"));
}

#[test]
fn test_run_command_tool_missing_command() {
    let tool = RunCommandTool::new();
    let result = tool.execute(serde_json::json!({}));

    assert!(result.is_err());
}

#[test]
fn test_tool_registry_register() {
    let registry = ToolRegistry::new();
    let tool = RunCommandTool::new();

    registry.register(Box::new(tool));

    assert!(registry.has_tool("run_command"));
    assert!(!registry.has_tool("nonexistent"));
}

#[test]
fn test_tool_registry_list() {
    let registry = ToolRegistry::new();
    let tool = RunCommandTool::new();

    registry.register(Box::new(tool));

    let tools = registry.list();
    assert!(!tools.is_empty());
}

#[test]
fn test_memory_clear() {
    let mut memory = Memory::new(1000);
    memory.add_message(Message::user("Hello"));
    memory.add_message(Message::assistant("Hi there"));

    memory.clear();

    let context = memory.get_context(10);
    assert!(context.is_empty());
}

#[test]
fn test_memory_context_limit() {
    let mut memory = Memory::new(100);
    for i in 0..10 {
        memory.add_message(Message::user(&format!("message {}", i)));
    }

    let context = memory.get_context(3);
    assert_eq!(context.len(), 3);
}

#[test]
fn test_mcp_server_config_stdio() {
    use crate::mcp::types::McpServerConfig;
    let config = McpServerConfig::stdio(
        "npx",
        vec![
            "-y".into(),
            "@modelcontextprotocol/server-filesystem".into(),
            "/tmp".into(),
        ],
    );
    assert!(config.is_stdio());
    assert!(!config.is_http());
    assert_eq!(config.command.as_deref(), Some("npx"));
    assert_eq!(config.args.as_ref().unwrap().len(), 3);
}

#[test]
fn test_mcp_server_config_http() {
    use crate::mcp::types::McpServerConfig;
    let config = McpServerConfig::http("http://localhost:3001/mcp");
    assert!(config.is_http());
    assert!(!config.is_stdio());
    assert_eq!(config.url.as_deref(), Some("http://localhost:3001/mcp"));
}

#[test]
fn test_json_rpc_request_serialization() {
    use crate::mcp::types::JsonRpcRequest;
    let req = JsonRpcRequest::new(1, "initialize", Some(serde_json::json!({"test": true})));
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("\"jsonrpc\":\"2.0\""));
    assert!(json.contains("\"id\":1"));
    assert!(json.contains("\"method\":\"initialize\""));
}

#[test]
fn test_json_rpc_response_deserialization() {
    use crate::mcp::types::JsonRpcResponse;
    let json =
        r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{}}}"#;
    let resp: JsonRpcResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.id, 1);
    assert!(resp.result.is_some());
    assert!(resp.error.is_none());
}

#[test]
fn test_json_rpc_error_response() {
    use crate::mcp::types::JsonRpcResponse;
    let json = r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32600,"message":"Invalid Request"}}"#;
    let resp: JsonRpcResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.id, 2);
    assert!(resp.error.is_some());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32600);
    assert_eq!(err.message, "Invalid Request");
}

#[test]
fn test_mcp_tool_schema_parsing() {
    use crate::mcp::types::ToolsListResult;
    let json = r#"{"tools":[{"name":"read_file","description":"Read a file","inputSchema":{"type":"object","properties":{"path":{"type":"string","description":"File path"}},"required":["path"]}}]}"#;
    let result: ToolsListResult = serde_json::from_str(json).unwrap();
    assert_eq!(result.tools.len(), 1);
    assert_eq!(result.tools[0].name, "read_file");
    assert_eq!(result.tools[0].description.as_deref(), Some("Read a file"));
    assert!(result.tools[0].input_schema.is_some());
}

#[test]
fn test_mcp_server_config_serialization_roundtrip() {
    use crate::mcp::types::McpServerConfig;
    let config = McpServerConfig::stdio("my-server", vec!["--arg1".into()]);
    let json = serde_json::to_string(&config).unwrap();
    let parsed: McpServerConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.command, config.command);
    assert_eq!(parsed.args, config.args);
}

#[test]
fn test_mcp_http_config_roundtrip() {
    use crate::mcp::types::McpServerConfig;
    let mut config = McpServerConfig::http("http://localhost:8080/mcp");
    let mut headers = std::collections::HashMap::new();
    headers.insert("Authorization".into(), "Bearer test-token".into());
    config.headers = Some(headers);

    let json = serde_json::to_string(&config).unwrap();
    let parsed: McpServerConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.url, config.url);
    assert!(parsed.headers.is_some());
    assert_eq!(
        parsed.headers.unwrap().get("Authorization").unwrap(),
        "Bearer test-token"
    );
}

#[test]
fn test_tool_call_result_parsing() {
    use crate::mcp::types::ToolCallResult;
    let json = r#"{"content":[{"type":"text","text":"file contents here"}],"is_error":false}"#;
    let result: ToolCallResult = serde_json::from_str(json).unwrap();
    assert_eq!(result.content.len(), 1);
    assert_eq!(result.content[0].content_type, "text");
    assert_eq!(
        result.content[0].text.as_deref(),
        Some("file contents here")
    );
    assert_eq!(result.is_error, Some(false));
}
