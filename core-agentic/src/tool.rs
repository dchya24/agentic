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
