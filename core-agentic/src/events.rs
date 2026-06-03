//! Event types for agentic runtime

use std::sync::Mutex;

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

    /// Reports progress of a planner-agent plan execution.
    #[serde(rename = "plan_progress")]
    PlanProgress {
        plan_id: String,
        plan_goal: String,
        step_id: String,
        step_description: String,
        step_status: String,
        steps_total: usize,
        steps_completed: usize,
        steps_failed: usize,
        steps_pending: usize,
    },

    /// Emitted when the planner revises a plan after a step failure.
    #[serde(rename = "plan_replanned")]
    PlanReplanned {
        plan_id: String,
        plan_goal: String,
        reason: String,
        steps_carried_over: usize,
        steps_total: usize,
    },
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
    PlanProgress,
    PlanReplanned,
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
            Event::PlanProgress { .. } => EventType::PlanProgress,
            Event::PlanReplanned { .. } => EventType::PlanReplanned,
        }
    }
}

pub struct EventEmitter {
    handlers: Mutex<Vec<Box<dyn Fn(Event) + Send + Sync>>>,
}

impl EventEmitter {
    pub fn new() -> Self {
        Self {
            handlers: Mutex::new(Vec::new()),
        }
    }

    /// Register a handler. Multiple handlers may be registered; they are
    /// called in registration order.
    ///
    /// Takes `&self` because handlers are stored behind a `Mutex`, so the
    /// emitter can sit behind a shared reference (e.g. inside an `Arc` or
    /// as a non-mut field of a longer-lived struct).
    pub fn on<F>(&self, handler: F)
    where
        F: Fn(Event) + Send + Sync + 'static,
    {
        self.handlers.lock().unwrap().push(Box::new(handler));
    }

    pub fn emit(&self, event: Event) {
        // Clone the event for each handler so we don't move it twice.
        let handlers = self.handlers.lock().unwrap();
        for handler in handlers.iter() {
            handler(event.clone());
        }
    }

    /// Drop all registered handlers. Useful between runs in long-lived
    /// orchestrators that get re-subscribed each invocation.
    #[allow(dead_code)]
    pub fn clear(&self) {
        self.handlers.lock().unwrap().clear();
    }
}

impl Default for EventEmitter {
    fn default() -> Self {
        Self::new()
    }
}
