//! Event types for agentic runtime

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Event {
    #[serde(rename = "thought")]
    Thought { content: String },

    #[serde(rename = "tool_call")]
    ToolCall {
        tool_name: String,
        arguments: serde_json::Value,
    },

    #[serde(rename = "tool_output")]
    ToolOutput {
        tool_name: String,
        output: serde_json::Value,
    },

    #[serde(rename = "confirmation_request")]
    ConfirmationRequest {
        action: String,
        description: String,
        risk_level: String,
    },

    #[serde(rename = "error")]
    Error { message: String },

    #[serde(rename = "completed")]
    Completed { result: String },

    #[serde(rename = "system")]
    System { message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    Thought,
    ToolCall,
    ToolOutput,
    ConfirmationRequest,
    Error,
    Completed,
    System,
}

impl Event {
    pub fn event_type(&self) -> EventType {
        match self {
            Event::Thought { .. } => EventType::Thought,
            Event::ToolCall { .. } => EventType::ToolCall,
            Event::ToolOutput { .. } => EventType::ToolOutput,
            Event::ConfirmationRequest { .. } => EventType::ConfirmationRequest,
            Event::Error { .. } => EventType::Error,
            Event::Completed { .. } => EventType::Completed,
            Event::System { .. } => EventType::System,
        }
    }
}

pub struct EventEmitter {
    handlers: Vec<Box<dyn Fn(Event) + Send + Sync>>,
}

impl EventEmitter {
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    pub fn on<F>(&mut self, handler: F)
    where
        F: Fn(Event) + Send + Sync + 'static,
    {
        self.handlers.push(Box::new(handler));
    }

    pub fn emit(&self, event: Event) {
        for handler in &self.handlers {
            handler(event.clone());
        }
    }
}

impl Default for EventEmitter {
    fn default() -> Self {
        Self::new()
    }
}
