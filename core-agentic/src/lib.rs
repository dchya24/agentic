//! Core Agentic - Rust library for AI agent orchestration
//!
//! Provides multi-step task execution, tool system, LLM provider integration,
//! memory management, and safety features.

#[cfg(test)]
mod tests;

pub mod agent;
pub mod events;
pub mod memory;
pub mod orchestrator;
pub mod providers;
pub mod safety;
pub mod tool;
pub mod tool_registry;
pub mod tools;

// Re-export main types for public API
pub use agent::{Agent, AgentConfig};
pub use events::{Event, EventType};
pub use memory::{Memory, Message, MessageRole};
pub use orchestrator::Orchestrator;
pub use providers::LLMProvider;
pub use safety::{ConfirmationRequest, RiskLevel};
pub use tool::{Tool, ToolError, ToolResult, ToolSchema, ToolCall, ToolResultValue};
pub use tool_registry::ToolRegistry;

// Re-export tool implementations
pub use tools::{
    RunCommandTool, ReadFileTool, WriteFileTool, ListFilesTool,
};

// Re-export provider implementations
pub use providers::openai::OpenAIProvider;

pub type Result<T> = std::result::Result<T, AgenticError>;

#[derive(Debug, thiserror::Error)]
pub enum AgenticError {
    #[error("Provider error: {0}")]
    Provider(String),
    
    #[error("Tool error: {0}")]
    Tool(String),
    
    #[error("Configuration error: {0}")]
    Config(String),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}