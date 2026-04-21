//! Read file tool

use std::collections::HashMap;
use std::path::Path;

use crate::tool::{Tool, ToolError, ToolParam, ToolResult, ToolSchema};

pub struct ReadFileTool;

impl ReadFileTool {
    pub fn new() -> Self {
        Self
    }
}

impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file"
    }

    fn schema(&self) -> ToolSchema {
        let mut params = HashMap::new();
        params.insert(
            "path".to_string(),
            ToolParam {
                param_type: "string".to_string(),
                description: Some("Path to the file to read".to_string()),
                default: None,
            },
        );

        ToolSchema {
            name: "read_file".to_string(),
            description: "Read the contents of a file".to_string(),
            parameters: params,
            required: vec!["path".to_string()],
        }
    }

    fn execute(&self, args: serde_json::Value) -> ToolResult<serde_json::Value> {
        let args_obj = args
            .as_object()
            .ok_or_else(|| ToolError::new("Invalid arguments: expected object"))?;

        let path = args_obj
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::new("Missing required parameter: path"))?;

        let path = Path::new(path);

        if !path.exists() {
            return Err(ToolError::new(format!(
                "File not found: {}",
                path.display()
            )));
        }

        let content = std::fs::read_to_string(path)
            .map_err(|e| ToolError::new(format!("Failed to read file: {}", e)))?;

        Ok(serde_json::json!({
            "path": path.to_string_lossy(),
            "content": content,
            "size": content.len(),
        }))
    }
}
