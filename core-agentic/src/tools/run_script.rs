//! Run script tool — execute multi-line scripts with optional timeout.
//!
//! Unlike `run_command` which runs a single command, this tool writes
//! a script to a temp file and executes it, supporting multi-line scripts.

use std::collections::HashMap;
use std::io::Write as IoWrite;
use std::process::Command;

use crate::tool::{Tool, ToolError, ToolParam, ToolResult, ToolSchema};

pub struct RunScriptTool;

impl Default for RunScriptTool {
    fn default() -> Self {
        Self::new()
    }
}

impl RunScriptTool {
    pub fn new() -> Self {
        Self
    }
}

impl Tool for RunScriptTool {
    fn name(&self) -> &str {
        "run_script"
    }

    fn description(&self) -> &str {
        "Execute a multi-line script (bash/sh) and return its output. Supports timeout."
    }

    fn schema(&self) -> ToolSchema {
        let mut params = HashMap::new();
        params.insert(
            "script".to_string(),
            ToolParam {
                param_type: "string".to_string(),
                description: Some("The script content to execute".to_string()),
                default: None,
            },
        );
        params.insert(
            "cwd".to_string(),
            ToolParam {
                param_type: "string".to_string(),
                description: Some("Working directory for script execution".to_string()),
                default: None,
            },
        );
        params.insert(
            "timeout".to_string(),
            ToolParam {
                param_type: "number".to_string(),
                description: Some("Timeout in seconds (default 30)".to_string()),
                default: Some(serde_json::json!(30)),
            },
        );
        params.insert(
            "interpreter".to_string(),
            ToolParam {
                param_type: "string".to_string(),
                description: Some(
                    "Interpreter to use (default 'sh'). Use 'bash', 'python3', etc.".to_string(),
                ),
                default: Some(serde_json::json!("sh")),
            },
        );

        ToolSchema {
            name: "run_script".to_string(),
            description: "Execute a multi-line script and return its output.".to_string(),
            parameters: params,
            required: vec!["script".to_string()],
        }
    }

    fn execute(&self, args: serde_json::Value) -> ToolResult<serde_json::Value> {
        let args_obj = args
            .as_object()
            .ok_or_else(|| ToolError::new("Invalid arguments: expected object"))?;

        let script = args_obj
            .get("script")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::new("Missing required parameter: script"))?;

        let cwd = args_obj.get("cwd").and_then(|v| v.as_str());
        let timeout_secs = args_obj
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(30);
        let interpreter = args_obj
            .get("interpreter")
            .and_then(|v| v.as_str())
            .unwrap_or("sh");

        // Write script to temp file
        let suffix = match interpreter {
            "python3" | "python" => ".py",
            "node" => ".js",
            _ => ".sh",
        };

        let temp_dir = std::env::temp_dir().join("core-agentic-scripts");
        std::fs::create_dir_all(&temp_dir)
            .map_err(|e| ToolError::new(format!("Failed to create temp dir: {}", e)))?;

        let script_id = uuid::Uuid::new_v4().to_string();
        let script_path = temp_dir.join(format!("{}{}", script_id, suffix));

        let mut file = std::fs::File::create(&script_path)
            .map_err(|e| ToolError::new(format!("Failed to create script file: {}", e)))?;
        file.write_all(script.as_bytes())
            .map_err(|e| ToolError::new(format!("Failed to write script: {}", e)))?;
        drop(file);

        // Execute
        let result = self.run_interpreter(interpreter, &script_path, cwd, timeout_secs);

        // Cleanup
        let _ = std::fs::remove_file(&script_path);

        result
    }
}

impl RunScriptTool {
    fn run_interpreter(
        &self,
        interpreter: &str,
        script_path: &std::path::Path,
        cwd: Option<&str>,
        _timeout_secs: u64,
    ) -> ToolResult<serde_json::Value> {
        let mut cmd = Command::new(interpreter);
        cmd.arg(script_path);

        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }

        let output = cmd
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .map_err(|e| ToolError::new(format!("Failed to execute script: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        // Truncate output if too large (64KB limit)
        let max_output = 65536;
        let stdout_truncated = if stdout.len() > max_output {
            format!(
                "{}... (truncated, {} bytes total)",
                &stdout[..max_output],
                stdout.len()
            )
        } else {
            stdout
        };
        let stderr_truncated = if stderr.len() > max_output {
            format!(
                "{}... (truncated, {} bytes total)",
                &stderr[..max_output],
                stderr.len()
            )
        } else {
            stderr
        };

        Ok(serde_json::json!({
            "success": output.status.success(),
            "exit_code": output.status.code().unwrap_or(-1),
            "stdout": stdout_truncated,
            "stderr": stderr_truncated,
            "interpreter": interpreter,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_script_schema() {
        let tool = RunScriptTool::new();
        let schema = tool.schema();
        assert_eq!(schema.name, "run_script");
        assert!(schema.parameters.contains_key("script"));
        assert!(schema.required.contains(&"script".to_string()));
    }

    #[test]
    fn test_run_script_missing_script() {
        let tool = RunScriptTool::new();
        let result = tool.execute(serde_json::json!({}));
        assert!(result.is_err());
    }

    #[test]
    fn test_run_script_echo() {
        let tool = RunScriptTool::new();
        let result = tool.execute(serde_json::json!({
            "script": "echo hello\necho world",
            "interpreter": "sh",
        }));
        assert!(result.is_ok());
        let output = result.unwrap();
        assert_eq!(output["success"], true);
        assert!(output["stdout"].as_str().unwrap().contains("hello"));
        assert!(output["stdout"].as_str().unwrap().contains("world"));
    }

    #[test]
    fn test_run_script_multiline() {
        let tool = RunScriptTool::new();
        let result = tool.execute(serde_json::json!({
            "script": "for i in 1 2 3; do echo \"line $i\"; done",
        }));
        assert!(result.is_ok());
        let output = result.unwrap();
        assert_eq!(output["success"], true);
        let stdout = output["stdout"].as_str().unwrap();
        assert!(stdout.contains("line 1"));
        assert!(stdout.contains("line 2"));
        assert!(stdout.contains("line 3"));
    }

    #[test]
    fn test_run_script_failure() {
        let tool = RunScriptTool::new();
        let result = tool.execute(serde_json::json!({
            "script": "exit 42",
        }));
        assert!(result.is_ok());
        let output = result.unwrap();
        assert_eq!(output["success"], false);
        assert_eq!(output["exit_code"], 42);
    }
}
