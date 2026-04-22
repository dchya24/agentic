use std::collections::HashMap;
use std::path::Path;

use crate::tool::{Tool, ToolError, ToolParam, ToolResult, ToolSchema};

pub struct EditFileTool;

impl EditFileTool {
    pub fn new() -> Self {
        Self
    }
}

impl Tool for EditFileTool {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Performs exact string replacements in files. The edit will fail if oldString is not found or found multiple times."
    }

    fn schema(&self) -> ToolSchema {
        let mut params = HashMap::new();
        params.insert(
            "file_path".to_string(),
            ToolParam {
                param_type: "string".to_string(),
                description: Some("The absolute path to the file to modify".to_string()),
                default: None,
            },
        );
        params.insert(
            "old_string".to_string(),
            ToolParam {
                param_type: "string".to_string(),
                description: Some("The text to replace".to_string()),
                default: None,
            },
        );
        params.insert(
            "new_string".to_string(),
            ToolParam {
                param_type: "string".to_string(),
                description: Some("The text to replace it with".to_string()),
                default: None,
            },
        );
        params.insert(
            "replace_all".to_string(),
            ToolParam {
                param_type: "boolean".to_string(),
                description: Some(
                    "Replace all occurrences of old_string (default false)".to_string(),
                ),
                default: Some(serde_json::json!(false)),
            },
        );

        ToolSchema {
            name: "edit_file".to_string(),
            description: "Performs exact string replacements in files".to_string(),
            parameters: params,
            required: vec![
                "file_path".to_string(),
                "old_string".to_string(),
                "new_string".to_string(),
            ],
        }
    }

    fn execute(&self, args: serde_json::Value) -> ToolResult<serde_json::Value> {
        let args_obj = args
            .as_object()
            .ok_or_else(|| ToolError::new("Invalid arguments: expected object"))?;

        let file_path = args_obj
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::new("Missing required parameter: file_path"))?;

        let old_string = args_obj
            .get("old_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::new("Missing required parameter: old_string"))?;

        let new_string = args_obj
            .get("new_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::new("Missing required parameter: new_string"))?;

        let replace_all = args_obj
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if old_string == new_string {
            return Err(ToolError::new(
                "old_string and new_string must be different",
            ));
        }

        let path = Path::new(file_path);
        if !path.exists() {
            return Err(ToolError::new(format!(
                "File not found: {}",
                path.display()
            )));
        }

        let content = std::fs::read_to_string(path)
            .map_err(|e| ToolError::new(format!("Failed to read file: {}", e)))?;

        let count = content.matches(old_string).count();
        if count == 0 {
            return Err(ToolError::new(format!(
                "old_string not found in file: {}",
                path.display()
            )));
        }

        if !replace_all && count > 1 {
            return Err(ToolError::new(format!(
                "Found {} matches for old_string. Use replace_all=true or provide more context to make it unique.",
                count
            )));
        }

        let new_content = if replace_all {
            content.replace(old_string, new_string)
        } else {
            content.replacen(old_string, new_string, 1)
        };

        std::fs::write(path, &new_content)
            .map_err(|e| ToolError::new(format!("Failed to write file: {}", e)))?;

        Ok(serde_json::json!({
            "path": path.to_string_lossy(),
            "success": true,
            "replacements": if replace_all { count } else { 1 },
        }))
    }
}
