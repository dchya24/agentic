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
pub mod attachments;
pub mod capabilities;
pub mod file_tracker;
pub mod memory_file;
pub mod skills;

// Re-export main types for public API
pub use agent::{Agent, AgentConfig};
pub use config::{AgentLoopConfig, BreakpointStrategy, CacheConfig, Config, ModelConfig, ModelOutput, OutputConfig, PlannerLoopConfig, ProviderConfig, SafetyConfig, SkillsConfig};
pub use events::{Event, EventType};
pub use memory::{Memory, Message, MessageRole, SessionInfo, MemoryConfig, ContextWindow, SummarizedContext, MessageMetadata};
pub use orchestrator::Orchestrator;
pub use providers::LLMProvider;
pub use safety::{ConfirmationRequest, RiskLevel, RiskScore, SafetyDecision, AuditEntry, AuditDecision, RateLimit, PermissionMode, UrlPolicy};
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
    skills_system_section,
    DEFAULT_SYSTEM_PROMPT, PROJECT_INSTRUCTION_FILES,
};
pub use file_tracker::{FileTracker, Freshness};
pub use attachments::{
    Attachment, AttachmentError, AttachmentKind, AttachmentLimits, AttachmentSource,
    ALLOWED_IMAGE_MIME, DEFAULT_MAX_BYTES,
};
pub use capabilities::ModelCapabilities;
pub use memory_file::{
    append_project_memory, append_user_memory, assemble_memory_section, find_project_memory,
    load_project_memory, load_user_memory, user_memory_path, PROJECT_MEMORY_FILE,
};

// Re-export skill system types
pub use skills::{
    Skill, SkillMetadata, SkillIndex, SkillLoader,
    SkillTool,
    set_skill_loader, clear_skill_loader,
    resolve_skill, list_skills, activate_skill, deactivate_skill, active_skill,
    DiscoveryConfig, discover_skills,
};

// Re-export tool implementations
pub use tools::{
    EditFileTool, FetchTool, GlobTool, GrepTool, ListFilesTool, ReadFileTool, RunCommandTool,
    WriteFileTool, SearchFilesTool, RunScriptTool, UpdateMemoryTool, SpawnSubagentTool,
    WebSearchTool, ApplyPatchTool, RunTestsTool, GitStatusTool, GitDiffTool,
    // Interactive tools
    QuestionTool, QuestionPrompt, QuestionAnswer, QuestionHandler,
    set_question_handler, clear_question_handler,
    TodowriteTool, TodoItem, TodoStatus, TodoPriority, TodoChangeHandler,
    set_todo_change_handler, clear_todo_change_handler, current_todos, clear_todos,
};

// Re-export provider implementations
pub use providers::openai::OpenAIProvider;
pub use providers::anthropic::AnthropicProvider;
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