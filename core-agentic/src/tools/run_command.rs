//! Run command tool - executes shell commands

use std::collections::HashMap;
use std::process::Command;

use crate::tool::{Tool, ToolError, ToolParam, ToolResult, ToolSchema};

pub struct RunCommandTool;

impl RunCommandTool {
    pub fn new() -> Self {
        Self
    }
}

impl Tool for RunCommandTool {
    fn name(&self) -> &str {
        "run_command"
    }

    fn description(&self) -> &str {
        "Execute a shell command and return its output"
    }

    fn schema(&self) -> ToolSchema {
        let mut params = HashMap::new();
        params.insert(
            "command".to_string(),
            ToolParam {
                param_type: "string".to_string(),
                description: Some("The command to execute".to_string()),
                default: None,
            },
        );
        params.insert(
            "cwd".to_string(),
            ToolParam {
                param_type: "string".to_string(),
                description: Some("Working directory for command execution".to_string()),
                default: None,
            },
        );

        ToolSchema {
            name: "run_command".to_string(),
            description: "Execute a shell command and return its output".to_string(),
            parameters: params,
            required: vec!["command".to_string()],
        }
    }

    fn execute(&self, args: serde_json::Value) -> ToolResult<serde_json::Value> {
        let args_obj = args
            .as_object()
            .ok_or_else(|| ToolError::new("Invalid arguments: expected object"))?;

        let command = args_obj
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::new("Missing required parameter: command"))?;

        let cwd = args_obj.get("cwd").and_then(|v| v.as_str());

        let output = if cfg!(target_os = "windows") {
            let mut cmd = Command::new("cmd");
            cmd.args(["/C", command]);
            if let Some(dir) = cwd {
                cmd.current_dir(dir);
            }
            cmd.output()
        } else {
            let mut cmd = Command::new("sh");
            cmd.args(["-c", command]);
            if let Some(dir) = cwd {
                cmd.current_dir(dir);
            }
            cmd.output()
        }
        .map_err(|e| ToolError::new(format!("Failed to execute command: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        Ok(serde_json::json!({
            "success": output.status.success(),
            "exit_code": output.status.code().unwrap_or(-1),
            "stdout": stdout,
            "stderr": stderr,
        }))
    }
}
