//! Core Agentic - Rust library for AI agent orchestration
//!
//! Provides multi-step task execution, tool system, LLM provider integration,
//! memory management, and safety features.

#[cfg(test)]
mod tests;

pub mod config;
pub mod agent;
pub mod diff_util;
pub mod events;
pub mod memory;
pub mod orchestrator;
pub mod providers;
pub mod safety;
pub mod tool;
pub mod tool_registry;
pub mod mcp;
pub mod tools;
pub mod planner;
pub mod prompts;
pub mod file_tracker;
pub mod memory_file;

// Re-export main types for public API
pub use agent::{Agent, AgentConfig};
pub use config::{AgentLoopConfig, Config, ModelConfig, ModelOutput, OutputConfig, ProviderConfig, SafetyConfig};
pub use events::{Event, EventType};
pub use memory::{Memory, Message, MessageRole, SessionInfo, MemoryConfig, ContextWindow, SummarizedContext, MessageMetadata};
pub use orchestrator::Orchestrator;
pub use providers::LLMProvider;
pub use safety::{ConfirmationRequest, RiskLevel, RiskScore, SafetyDecision, AuditEntry, AuditDecision, RateLimit, PermissionMode};
pub use tool::{Tool, ToolError, ToolResult, ToolSchema, ToolCall, ToolResultValue};
pub use mcp::{
    McpClient, McpServerConfig, McpToolAdapter,
    AsyncMcpClient, AsyncMcpToolAdapter, ReconnectConfig,
    AsyncMcpTransport, AsyncStdioTransport, AsyncHttpTransport, AsyncSseTransport,
};
pub use planner::{PlannerAgent, PlannerConfig, Plan, Step, PlanStatus, StepStatus, PlanResult};
pub use tool_registry::ToolRegistry;
pub use prompts::{
    assemble_system_prompt, find_project_instructions, load_project_instructions,
    DEFAULT_SYSTEM_PROMPT, PROJECT_INSTRUCTION_FILES,
};
pub use file_tracker::{FileTracker, Freshness};
pub use memory_file::{
    append_project_memory, append_user_memory, assemble_memory_section, find_project_memory,
    load_project_memory, load_user_memory, user_memory_path, PROJECT_MEMORY_FILE,
};

// Re-export tool implementations
pub use tools::{
    EditFileTool, FetchTool, GlobTool, GrepTool, ListFilesTool, ReadFileTool, RunCommandTool,
    WriteFileTool, SearchFilesTool, RunScriptTool, UpdateMemoryTool, SpawnSubagentTool,
    WebSearchTool,
};

// Re-export provider implementations
pub use providers::openai::OpenAIProvider;
pub use providers::anthropic::AnthropicProvider;
pub use providers::zai::ZaiProvider;
pub use providers::failover::FailoverProvider;
pub use providers::{ModelInfo, ModelCapability};

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

    #[error("Cancelled by user")]
    Cancelled,
}