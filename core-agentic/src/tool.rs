//! Tool trait and related types

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type ToolResult<T> = std::result::Result<T, ToolError>;

#[derive(Debug, thiserror::Error)]
#[error("Tool error: {0}")]
pub struct ToolError(pub String);

impl ToolError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: HashMap<String, ToolParam>,
    pub required: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolParam {
    pub param_type: String,
    pub description: Option<String>,
    pub default: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
}

impl ToolCall {
    pub fn new(tool_name: impl Into<String>, arguments: serde_json::Value) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            tool_name: tool_name.into(),
            arguments,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultValue {
    pub tool_call_id: String,
    pub output: serde_json::Value,
    pub error: Option<String>,
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> ToolSchema;
    fn execute(&self, args: serde_json::Value) -> ToolResult<serde_json::Value>;

    /// Whether this tool only reads state (no filesystem writes, no shell
    /// commands, no network mutations). Read-only tools may be executed
    /// concurrently with other read-only tools by the orchestrator.
    ///
    /// Defaults to `false` (assume mutating). Override in tools that are
    /// known to be safe for parallel execution.
    fn is_read_only(&self) -> bool {
        false
    }

    /// Stream progressive output to `on_progress` as the tool runs.
    ///
    /// Default: run [`Self::execute`] atomically and ignore the callback.
    /// Tools that produce long-running output (e.g. run_command) override
    /// this to report deltas live; non-streaming tools are untouched.
    fn execute_streaming(
        &self,
        args: serde_json::Value,
        _on_progress: &dyn Fn(&str),
    ) -> ToolResult<serde_json::Value> {
        self.execute(args)
    }
}

impl ToolSchema {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters: HashMap::new(),
            required: vec![],
        }
    }

    pub fn with_param(
        self,
        name: impl Into<String>,
        param_type: impl Into<String>,
        required: bool,
    ) -> Self {
        let name = name.into();
        let mut params = self.parameters;
        params.insert(
            name.clone(),
            ToolParam {
                param_type: param_type.into(),
                description: None,
                default: None,
            },
        );
        let mut required_list = self.required;
        if required {
            required_list.push(name);
        }
        Self {
            name: self.name,
            description: self.description,
            parameters: params,
            required: required_list,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    // Tool yang hanya mengimplementasikan `execute` (bukan meng-override
    // `execute_streaming`) tetap berfungsi: default mengembalikan hasil yang
    // sama dan tidak pernah memanggil on_progress.
    #[test]
    fn execute_streaming_defaults_to_execute() {
        struct Basic;
        impl Tool for Basic {
            fn name(&self) -> &str {
                "basic"
            }
            fn description(&self) -> &str {
                ""
            }
            fn schema(&self) -> ToolSchema {
                ToolSchema::new("basic", "")
            }
            fn execute(&self, _: serde_json::Value) -> ToolResult<serde_json::Value> {
                Ok(serde_json::json!({ "ok": 1 }))
            }
        }

        let tool = Basic;
        let callbacks = Arc::new(AtomicUsize::new(0));
        let c = callbacks.clone();
        // Closure harus Fn (bukan FnMut) supaya bisa di-coerce ke
        // `&dyn Fn(&str)`.
        let result = tool
            .execute_streaming(serde_json::json!({}), &move |_| {
                c.fetch_add(1, Ordering::SeqCst);
            })
            .unwrap();
        assert_eq!(result, serde_json::json!({ "ok": 1 }));
        assert_eq!(
            callbacks.load(Ordering::SeqCst),
            0,
            "fallback must not invoke on_progress"
        );
    }
}
