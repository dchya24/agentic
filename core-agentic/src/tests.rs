//! Unit tests for core-agentic

use crate::tool::{ToolCall, ToolSchema};
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
fn test_tool_registry_new() {
    let registry = ToolRegistry::new();
    let _ = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            registry.get("nonexistent").await.is_none()
        });
}
