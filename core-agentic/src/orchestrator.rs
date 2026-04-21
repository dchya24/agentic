//! Orchestrator - Core agent loop

use std::process::Command as ProcessCommand;
use std::sync::{Arc, Mutex};

use crate::events::EventEmitter;
use crate::memory::{Memory, Message, MessageRole};
use crate::providers::{
    ChatMessageRequest, ChatRequest, ChatResponse, LLMProvider, ToolDefinition, ToolFunction,
};
use crate::safety::{ConfirmationRequest, Safety};
use crate::tool::{ToolCall, ToolResultValue};
use crate::tool_registry::ToolRegistry;
use crate::AgenticError;

fn builtin_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            tool_type: "function".into(),
            function: ToolFunction {
                name: "run_command".into(),
                description: "Execute a shell command and return its output. Use this to run any system command like ls, dir, cat, etc.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The shell command to execute"
                        }
                    },
                    "required": ["command"]
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".into(),
            function: ToolFunction {
                name: "read_file".into(),
                description: "Read the contents of a file".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file to read"
                        }
                    },
                    "required": ["path"]
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".into(),
            function: ToolFunction {
                name: "list_files".into(),
                description: "List files and directories at the given path".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Directory path to list. Defaults to current directory."
                        }
                    }
                }),
            },
        },
    ]
}

fn execute_builtin(name: &str, args: &serde_json::Value) -> String {
    match name {
        "run_command" => {
            let cmd = args["command"].as_str().unwrap_or("");
            let output = if cfg!(target_os = "windows") {
                ProcessCommand::new("cmd").args(["/C", cmd]).output()
            } else {
                ProcessCommand::new("sh").args(["-c", cmd]).output()
            };
            match output {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                    if stderr.is_empty() {
                        stdout
                    } else {
                        format!("{}\n{}", stdout, stderr)
                    }
                }
                Err(e) => format!("Command failed: {}", e),
            }
        }
        "read_file" => {
            let path = args["path"].as_str().unwrap_or(".");
            match std::fs::read_to_string(path) {
                Ok(content) => content,
                Err(e) => format!("Failed to read file: {}", e),
            }
        }
        "list_files" => {
            let path = args["path"].as_str().unwrap_or(".");
            match std::fs::read_dir(path) {
                Ok(entries) => {
                    let names: Vec<String> = entries
                        .filter_map(|e| e.ok())
                        .map(|e| e.file_name().to_string_lossy().to_string())
                        .collect();
                    if names.is_empty() {
                        "(empty directory)".into()
                    } else {
                        names.join("\n")
                    }
                }
                Err(e) => format!("Failed to list directory: {}", e),
            }
        }
        _ => format!("Unknown tool: {}", name),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrchestratorState {
    Idle,
    Planning,
    Executing,
    Completed,
}

pub struct Orchestrator {
    provider: Arc<dyn LLMProvider>,
    tools: ToolRegistry,
    memory: Mutex<Memory>,
    safety: Safety,
    state: Mutex<OrchestratorState>,
    events: EventEmitter,
    confirmation_handler:
        Mutex<Option<Box<dyn Fn(crate::safety::ConfirmationRequest) -> bool + Send + Sync>>>,
}

impl Orchestrator {
    pub fn new(provider: Arc<dyn LLMProvider>, tools: ToolRegistry) -> Self {
        Self {
            provider,
            tools,
            memory: Mutex::new(Memory::new(128000)),
            safety: Safety::new(),
            state: Mutex::new(OrchestratorState::Idle),
            events: EventEmitter::new(),
            confirmation_handler: Mutex::new(None),
        }
    }

    pub fn set_confirmation_handler<F>(&mut self, handler: F)
    where
        F: Fn(ConfirmationRequest) -> bool + Send + Sync + 'static,
    {
        let mut h = self.confirmation_handler.lock().unwrap();
        *h = Some(Box::new(handler));
    }

    fn should_confirm(&self, tool_name: &str, args: &serde_json::Value) -> bool {
        let action = tool_name;
        let target = args
            .get("command")
            .or(args.get("path"))
            .and_then(|v| v.as_str());
        self.safety.needs_confirmation(action, target)
    }

    fn require_confirmation(&self, request: ConfirmationRequest) -> bool {
        let handler = self.confirmation_handler.lock().unwrap();
        if let Some(ref h) = *handler {
            h(request)
        } else {
            false
        }
    }

    pub fn run(&self, input: &str) -> Result<String, AgenticError> {
        {
            let mut state = self.state.lock().unwrap();
            *state = OrchestratorState::Planning;
        }

        self.memory
            .lock()
            .unwrap()
            .add_message(Message::user(input));

        let tools = builtin_tool_definitions();
        let mut last_output = String::new();

        loop {
            let context = self.memory.lock().unwrap().get_context(20);
            let messages: Vec<ChatMessageRequest> = context
                .iter()
                .map(|m| ChatMessageRequest {
                    role: match &m.role {
                        MessageRole::User => "user".to_string(),
                        MessageRole::Assistant => "assistant".to_string(),
                        MessageRole::System => "system".to_string(),
                        MessageRole::Tool { .. } => "tool".to_string(),
                    },
                    content: m.content.clone(),
                    tool_call_id: None,
                    tool_calls: vec![],
                })
                .collect();

            let request = ChatRequest::new("glm-4.7", messages).with_tools(tools.clone());

            let response = self
                .provider
                .chat(request)
                .map_err(|e| AgenticError::Provider(e.to_string()))?;

            last_output = response.message.content.clone().unwrap_or_default();

            // If model wants to call tools
            if !response.message.tool_calls.is_empty() {
                // Add assistant message with tool calls to memory
                let assistant_content = last_output.clone();
                self.memory
                    .lock()
                    .unwrap()
                    .add_message(Message::assistant(&assistant_content));

                for tc in &response.message.tool_calls {
                    let args: serde_json::Value = serde_json::from_str(&tc.function.arguments)
                        .unwrap_or(serde_json::json!({}));

                    println!("  [{}] {}", tc.function.name, args);

                    if self.should_confirm(&tc.function.name, &args) {
                        let request = self
                            .safety
                            .create_request(&tc.function.name, &format!("{:?}", args));
                        if !self.require_confirmation(request) {
                            println!("  -> [SKIPPED - Confirmation denied]");
                            self.memory.lock().unwrap().add_message(Message::tool(
                                tc.id.clone(),
                                tc.function.name.clone(),
                                "Skipped: Confirmation denied".to_string(),
                            ));
                            continue;
                        }
                    }

                    let result = execute_builtin(&tc.function.name, &args);

                    println!(
                        "  -> {}",
                        if result.len() > 200 {
                            format!("{}...", &result[..200])
                        } else {
                            result.clone()
                        }
                    );

                    self.memory.lock().unwrap().add_message(Message::tool(
                        tc.id.clone(),
                        tc.function.name.clone(),
                        result,
                    ));
                }
            } else {
                // No tool calls - model is done, return content
                self.memory
                    .lock()
                    .unwrap()
                    .add_message(Message::assistant(&last_output));

                {
                    let mut state = self.state.lock().unwrap();
                    *state = OrchestratorState::Completed;
                }
                return Ok(last_output);
            }
        }
    }

    pub fn get_state(&self) -> OrchestratorState {
        *self.state.lock().unwrap()
    }

    pub fn clear_memory(&self) {
        self.memory.lock().unwrap().clear();
    }
}
