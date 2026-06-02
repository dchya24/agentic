//! `todowrite` tool — create and manage a structured task list for the
//! current coding session.
//!
//! The agent uses this to track progress and organize complex multi-step
//! tasks. Each todo has content, status, and priority. The tool receives
//! the **full** updated list on every call (replaces the previous state).
//!
//! Why this exists:
//! - Gives the agent a structured way to break down work and track it.
//! - The TUI/CLI can render the todo list as a visual progress indicator.
//! - Survives context compaction: the todo list is kept outside the
//!   message history, so important task context isn't lost when older
//!   messages get summarized away.
//!
//! Storage: in-memory, session-scoped. The list is lost when the process
//! exits (by design — todos are ephemeral). The user can persist them
//! via `update_memory` if they want cross-session continuity.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::tool::{Tool, ToolError, ToolParam, ToolResult, ToolSchema};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Status of a single todo item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TodoStatus {
    /// Not yet started.
    #[serde(rename = "pending")]
    Pending,
    /// Currently being worked on.
    #[serde(rename = "in_progress")]
    InProgress,
    /// Finished successfully.
    #[serde(rename = "completed")]
    Completed,
    /// Cancelled or no longer relevant.
    #[serde(rename = "cancelled")]
    Cancelled,
}

impl TodoStatus {
    pub fn as_str(&self) -> &str {
        match self {
            TodoStatus::Pending => "pending",
            TodoStatus::InProgress => "in_progress",
            TodoStatus::Completed => "completed",
            TodoStatus::Cancelled => "cancelled",
        }
    }

    /// Parse from string, case-insensitive.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "pending" => Some(TodoStatus::Pending),
            "in_progress" | "inprogress" | "in-progress" | "active" => Some(TodoStatus::InProgress),
            "completed" | "done" | "finished" => Some(TodoStatus::Completed),
            "cancelled" | "canceled" | "skipped" => Some(TodoStatus::Cancelled),
            _ => None,
        }
    }
}

impl Default for TodoStatus {
    fn default() -> Self {
        TodoStatus::Pending
    }
}

/// Priority level for a todo item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TodoPriority {
    Low = 0,
    Medium = 1,
    High = 2,
}

impl TodoPriority {
    pub fn as_str(&self) -> &str {
        match self {
            TodoPriority::Low => "low",
            TodoPriority::Medium => "medium",
            TodoPriority::High => "high",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "low" | "l" => Some(TodoPriority::Low),
            "medium" | "med" | "m" | "normal" => Some(TodoPriority::Medium),
            "high" | "h" | "important" | "critical" => Some(TodoPriority::High),
            _ => None,
        }
    }
}

impl Default for TodoPriority {
    fn default() -> Self {
        TodoPriority::Medium
    }
}

/// A single todo item.
///
/// Uses custom serde logic so `status` and `priority` accept flexible
/// aliases (e.g. "done" → Completed, "active" → InProgress).
#[derive(Debug, Clone, Serialize)]
pub struct TodoItem {
    /// Description of the task.
    pub content: String,
    /// Current status.
    #[serde(default)]
    pub status: TodoStatus,
    /// Priority level.
    #[serde(default)]
    pub priority: TodoPriority,
}

impl<'de> serde::Deserialize<'de> for TodoItem {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            content: String,
            status: Option<String>,
            priority: Option<String>,
        }
        let raw = Raw::deserialize(de)?;
        let status = raw.status
            .as_deref()
            .and_then(TodoStatus::parse)
            .unwrap_or_default();
        let priority = raw.priority
            .as_deref()
            .and_then(TodoPriority::parse)
            .unwrap_or_default();
        Ok(TodoItem { content: raw.content, status, priority })
    }
}

/// Global session-scoped todo list. Replaced wholesale on every call.
pub(crate) static TODO_LIST: std::sync::LazyLock<Mutex<Vec<TodoItem>>> =
    std::sync::LazyLock::new(|| Mutex::new(Vec::new()));

/// Callback type for notifying UI about todo list changes.
/// Called after every successful `todowrite` call with the new list.
pub trait TodoChangeHandler: Send + Sync {
    fn on_change(&self, todos: &[TodoItem]);
}

/// Global handler for todo changes (optional, for UI rendering).
pub(crate) static TODO_CHANGE_HANDLER:
    std::sync::LazyLock<Mutex<Option<Box<dyn TodoChangeHandler>>>> =
    std::sync::LazyLock::new(|| Mutex::new(None));

/// Register a todo change handler.
pub fn set_todo_change_handler(handler: Box<dyn TodoChangeHandler>) {
    let mut slot = TODO_CHANGE_HANDLER.lock().unwrap();
    *slot = Some(handler);
}

/// Clear the registered handler.
pub fn clear_todo_change_handler() {
    let mut slot = TODO_CHANGE_HANDLER.lock().unwrap();
    *slot = None;
}

/// Get a snapshot of the current todo list.
pub fn current_todos() -> Vec<TodoItem> {
    TODO_LIST.lock().unwrap().clone()
}

/// Clear the todo list.
pub fn clear_todos() {
    TODO_LIST.lock().unwrap().clear();
}

// ---------------------------------------------------------------------------
// Tool implementation
// ---------------------------------------------------------------------------

pub struct TodowriteTool;

impl TodowriteTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TodowriteTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for TodowriteTool {
    fn name(&self) -> &str {
        "todowrite"
    }

    fn description(&self) -> &str {
        "Create or update the session task list. Each call replaces the entire \
         list — send the full updated array every time. Each todo has content \
         (description), status (pending / in_progress / completed / cancelled), \
         and priority (low / medium / high). Use this to plan complex tasks, \
         track progress, and organize multi-step work."
    }

    fn schema(&self) -> ToolSchema {
        let mut params = HashMap::new();
        params.insert(
            "todos".to_string(),
            ToolParam {
                param_type: "array".to_string(),
                description: Some(
                    "The full updated todo list. Each entry has: \
                     content (string, required) — task description, \
                     status (string, optional) — one of: pending, in_progress, completed, cancelled. \
                     priority (string, optional) — one of: low, medium, high."
                        .to_string(),
                ),
                default: None,
            },
        );

        ToolSchema {
            name: "todowrite".to_string(),
            description: "Create and manage a structured task list for the session.".to_string(),
            parameters: params,
            required: vec!["todos".to_string()],
        }
    }

    fn execute(&self, args: serde_json::Value) -> ToolResult<serde_json::Value> {
        let args_obj = args
            .as_object()
            .ok_or_else(|| ToolError::new("Invalid arguments: expected object"))?;

        let todos_val = args_obj
            .get("todos")
            .ok_or_else(|| ToolError::new("Missing required parameter: todos"))?;

        let todos: Vec<TodoItem> = serde_json::from_value(todos_val.clone())
            .map_err(|e| ToolError::new(format!("Invalid todos format: {}", e)))?;

        // Validate: each item must have non-empty content.
        for (i, item) in todos.iter().enumerate() {
            if item.content.trim().is_empty() {
                return Err(ToolError::new(format!(
                    "Todo at index {} has empty content",
                    i
                )));
            }
        }

        // Cap to prevent abuse.
        const MAX_TODOS: usize = 50;
        if todos.len() > MAX_TODOS {
            return Err(ToolError::new(format!(
                "Too many todos: {} (max {})",
                todos.len(),
                MAX_TODOS
            )));
        }

        // Compute summary before storing.
        let total = todos.len();
        let completed = todos
            .iter()
            .filter(|t| t.status == TodoStatus::Completed)
            .count();
        let in_progress = todos
            .iter()
            .filter(|t| t.status == TodoStatus::InProgress)
            .count();
        let pending = todos
            .iter()
            .filter(|t| t.status == TodoStatus::Pending)
            .count();

        // Replace the global list.
        {
            let mut list = TODO_LIST.lock().unwrap();
            *list = todos;
        }

        // Notify UI handler if registered.
        {
            let list = TODO_LIST.lock().unwrap();
            let handler = TODO_CHANGE_HANDLER.lock().unwrap();
            if let Some(ref h) = *handler {
                h.on_change(&list);
            }
        }

        let progress_pct = if total > 0 {
            ((completed as f64 / total as f64) * 100.0) as u32
        } else {
            0
        };

        Ok(serde_json::json!({
            "total": total,
            "completed": completed,
            "in_progress": in_progress,
            "pending": pending,
            "progress_pct": progress_pct,
        }))
    }

    /// `todowrite` is read-only from the filesystem perspective but
    /// modifies session state. Return `false` so it doesn't get
    /// batched with other tools in parallel execution (order matters
    /// for task tracking).
    fn is_read_only(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_content() {
        let tool = TodowriteTool::new();
        let err = tool
            .execute(serde_json::json!({"todos": [{"content": "  "}]}))
            .expect_err("should reject empty content");
        assert!(err.to_string().contains("empty content"));
    }

    #[test]
    fn rejects_too_many_todos() {
        clear_todos();
        let tool = TodowriteTool::new();
        let todos: Vec<serde_json::Value> = (0..51)
            .map(|i| serde_json::json!({"content": format!("task {}", i)}))
            .collect();
        let err = tool
            .execute(serde_json::json!({"todos": todos}))
            .expect_err("should reject too many");
        assert!(err.to_string().contains("Too many"));
        clear_todos();
    }

    #[test]
    fn stores_and_returns_summary() {
        clear_todos();

        let tool = TodowriteTool::new();
        let result = tool
            .execute(serde_json::json!({
                "todos": [
                    {"content": "Set up project", "status": "completed", "priority": "high"},
                    {"content": "Write tests", "status": "in_progress", "priority": "medium"},
                    {"content": "Add docs", "status": "pending", "priority": "low"}
                ]
            }))
            .expect("should succeed");

        assert_eq!(result["total"], 3);
        assert_eq!(result["completed"], 1);
        assert_eq!(result["in_progress"], 1);
        assert_eq!(result["pending"], 1);
        assert_eq!(result["progress_pct"], 33);

        // Verify global state was updated.
        let todos = current_todos();
        assert_eq!(todos.len(), 3);
        assert_eq!(todos[0].status, TodoStatus::Completed);
        assert_eq!(todos[0].priority, TodoPriority::High);
        assert_eq!(todos[1].status, TodoStatus::InProgress);
        assert_eq!(todos[2].status, TodoStatus::Pending);

        clear_todos();
    }

    #[test]
    fn replaces_previous_list() {
        clear_todos();

        let tool = TodowriteTool::new();
        let r = tool.execute(serde_json::json!({
            "todos": [{"content": "First task"}]
        }));
        assert!(r.is_ok(), "first write: {:?}", r);
        assert_eq!(current_todos().len(), 1);

        tool.execute(serde_json::json!({
            "todos": [
                {"content": "Task A"},
                {"content": "Task B"},
            ]
        }))
        .unwrap();
        assert_eq!(current_todos().len(), 2);
        assert_eq!(current_todos()[0].content, "Task A");

        clear_todos();
    }

    #[test]
    fn empty_array_clears_list() {
        clear_todos();

        let tool = TodowriteTool::new();
        tool.execute(serde_json::json!({
            "todos": [{"content": "task"}]
        }))
        .unwrap();
        assert_eq!(current_todos().len(), 1);

        tool.execute(serde_json::json!({"todos": []})).unwrap();
        assert!(current_todos().is_empty());
    }

    #[test]
    fn calls_change_handler() {
        clear_todos();

        struct TestHandler;
        static mut CALLED: bool = false;
        impl TodoChangeHandler for TestHandler {
            fn on_change(&self, todos: &[TodoItem]) {
                unsafe { CALLED = true };
                assert_eq!(todos.len(), 1);
                assert_eq!(todos[0].content, "tracked task");
            }
        }

        set_todo_change_handler(Box::new(TestHandler));

        let tool = TodowriteTool::new();
        tool.execute(serde_json::json!({
            "todos": [{"content": "tracked task"}]
        }))
        .unwrap();

        assert!(unsafe { CALLED }, "change handler should have been called");

        clear_todo_change_handler();
        clear_todos();
    }

    #[test]
    fn todo_status_parse() {
        assert_eq!(TodoStatus::parse("pending"), Some(TodoStatus::Pending));
        assert_eq!(TodoStatus::parse("in_progress"), Some(TodoStatus::InProgress));
        assert_eq!(TodoStatus::parse("in-progress"), Some(TodoStatus::InProgress));
        assert_eq!(TodoStatus::parse("completed"), Some(TodoStatus::Completed));
        assert_eq!(TodoStatus::parse("done"), Some(TodoStatus::Completed));
        assert_eq!(TodoStatus::parse("cancelled"), Some(TodoStatus::Cancelled));
        assert_eq!(TodoStatus::parse("UNKNOWN"), None);
    }

    #[test]
    fn todo_priority_parse() {
        assert_eq!(TodoPriority::parse("low"), Some(TodoPriority::Low));
        assert_eq!(TodoPriority::parse("medium"), Some(TodoPriority::Medium));
        assert_eq!(TodoPriority::parse("high"), Some(TodoPriority::High));
        assert_eq!(TodoPriority::parse("critical"), Some(TodoPriority::High));
        assert_eq!(TodoPriority::parse("bogus"), None);
    }

    #[test]
    fn todo_status_default() {
        assert_eq!(TodoStatus::default(), TodoStatus::Pending);
    }

    #[test]
    fn todo_priority_default() {
        assert_eq!(TodoPriority::default(), TodoPriority::Medium);
    }

    #[test]
    fn progress_pct_100_when_all_done() {
        let tool = TodowriteTool::new();
        let result = tool
            .execute(serde_json::json!({
                "todos": [
                    {"content": "A", "status": "completed"},
                    {"content": "B", "status": "completed"}
                ]
            }))
            .unwrap();
        assert_eq!(result["progress_pct"], 100);
    }

    #[test]
    fn progress_pct_0_when_none_done() {
        let tool = TodowriteTool::new();
        let result = tool
            .execute(serde_json::json!({
                "todos": [
                    {"content": "A", "status": "pending"},
                    {"content": "B", "status": "in_progress"}
                ]
            }))
            .unwrap();
        assert_eq!(result["progress_pct"], 0);
    }

    #[test]
    fn defaults_optional_fields_in_item() {
        let item: TodoItem =
            serde_json::from_value(serde_json::json!({"content": "test"})).unwrap();
        assert_eq!(item.status, TodoStatus::Pending);
        assert_eq!(item.priority, TodoPriority::Medium);
    }
}
