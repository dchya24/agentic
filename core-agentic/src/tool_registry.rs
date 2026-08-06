//! Tool registry for dynamic tool registration and execution

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::providers::{ToolDefinition, ToolFunction};
use crate::tool::{Tool, ToolCall, ToolError, ToolResultValue};

pub struct ToolRegistry {
    /// `RwLock` lets multiple read-only tool calls proceed concurrently.
    /// Tool::execute takes &self, so no exclusive lock is needed during
    /// execution. We only acquire the write lock for register/unregister.
    tools: Arc<RwLock<HashMap<String, Box<dyn Tool + Send + Sync>>>>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn register(&self, tool: Box<dyn Tool + Send + Sync>) {
        let mut tools = self.tools.write().unwrap();
        tools.insert(tool.name().to_string(), tool);
    }

    pub fn unregister(&self, name: &str) -> Option<Box<dyn Tool + Send + Sync>> {
        let mut tools = self.tools.write().unwrap();
        tools.remove(name)
    }

    pub fn list(&self) -> Vec<crate::tool::ToolSchema> {
        let tools = self.tools.read().unwrap();
        tools.values().map(|t| t.schema()).collect()
    }

    pub fn execute(&self, call: ToolCall) -> Result<ToolResultValue, ToolError> {
        let tools = self.tools.read().unwrap();

        let tool = tools
            .get(&call.tool_name)
            .ok_or_else(|| ToolError::new(format!("Tool not found: {}", call.tool_name)))?;

        let result = tool
            .execute(call.arguments)
            .map_err(|e| ToolError::new(e.to_string()))?;

        Ok(ToolResultValue {
            tool_call_id: call.id,
            output: result,
            error: None,
        })
    }

    pub fn has_tool(&self, name: &str) -> bool {
        let tools = self.tools.read().unwrap();
        tools.contains_key(name)
    }

    /// Returns whether the named tool advertises itself as read-only.
    /// Unknown tools are treated as mutating (safer default).
    pub fn is_read_only(&self, name: &str) -> bool {
        let tools = self.tools.read().unwrap();
        tools.get(name).map(|t| t.is_read_only()).unwrap_or(false)
    }

    pub fn tool_names(&self) -> Vec<String> {
        let tools = self.tools.read().unwrap();
        tools.keys().cloned().collect()
    }

    pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let tools = self.tools.read().unwrap();
        tools
            .values()
            .map(|t| {
                let schema = t.schema();
                let mut properties = serde_json::Map::new();
                for (name, param) in &schema.parameters {
                    let mut prop = serde_json::json!({
                        "type": param.param_type,
                    });
                    if let Some(desc) = &param.description {
                        prop["description"] = serde_json::json!(desc);
                    }
                    properties.insert(name.clone(), prop);
                }
                ToolDefinition {
                    tool_type: "function".into(),
                    function: ToolFunction {
                        name: schema.name.clone(),
                        description: schema.description.clone(),
                        parameters: serde_json::json!({
                            "type": "object",
                            "properties": properties,
                            "required": schema.required,
                        }),
                    },
                }
            })
            .collect()
    }

    pub fn register_mcp_server(
        &self,
        config: &crate::mcp::types::McpServerConfig,
    ) -> Result<(), String> {
        let tools = crate::mcp::tool_adapter::mcp_tools_from_config(config)?;
        for tool in tools {
            self.register(tool);
        }
        Ok(())
    }

    pub fn execute_by_name(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolError> {
        let tools = self.tools.read().unwrap();
        let tool = tools
            .get(name)
            .ok_or_else(|| ToolError::new(format!("Tool not found: {}", name)))?;
        tool.execute(args.clone())
    }

    /// Execute a tool by name, streaming progress deltas through
    /// `on_progress`. For tools without a streaming override this routes
    /// to the default (atomic) execution and never calls `on_progress`.
    pub fn execute_streaming_by_name(
        &self,
        name: &str,
        args: &serde_json::Value,
        on_progress: &dyn Fn(&str),
    ) -> Result<serde_json::Value, ToolError> {
        let tools = self.tools.read().unwrap();
        let tool = tools
            .get(name)
            .ok_or_else(|| ToolError::new(format!("Tool not found: {}", name)))?;
        tool.execute_streaming(args.clone(), on_progress)
    }
}

impl Clone for ToolRegistry {
    fn clone(&self) -> Self {
        Self {
            tools: self.tools.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{ToolError, ToolSchema};

    struct ReadTool;
    impl Tool for ReadTool {
        fn name(&self) -> &str {
            "reader"
        }
        fn description(&self) -> &str {
            ""
        }
        fn schema(&self) -> ToolSchema {
            ToolSchema::new("reader", "")
        }
        fn execute(&self, _: serde_json::Value) -> Result<serde_json::Value, ToolError> {
            Ok(serde_json::json!({"ok": true}))
        }
        fn is_read_only(&self) -> bool {
            true
        }
    }

    struct WriteTool;
    impl Tool for WriteTool {
        fn name(&self) -> &str {
            "writer"
        }
        fn description(&self) -> &str {
            ""
        }
        fn schema(&self) -> ToolSchema {
            ToolSchema::new("writer", "")
        }
        fn execute(&self, _: serde_json::Value) -> Result<serde_json::Value, ToolError> {
            Ok(serde_json::json!({"ok": true}))
        }
        // No is_read_only override → defaults to false.
    }

    #[test]
    fn read_only_flag_propagates_through_registry() {
        let reg = ToolRegistry::new();
        reg.register(Box::new(ReadTool));
        reg.register(Box::new(WriteTool));

        assert!(reg.is_read_only("reader"));
        assert!(!reg.is_read_only("writer"));
    }

    #[test]
    fn unknown_tool_treated_as_mutating() {
        let reg = ToolRegistry::new();
        assert!(!reg.is_read_only("nonexistent"));
    }

    #[test]
    fn builtin_read_tools_are_marked_read_only() {
        let reg = ToolRegistry::new();
        for tool in crate::tools::builtin_tools() {
            reg.register(tool);
        }
        for name in ["read_file", "list_files", "glob", "grep", "search_files"] {
            assert!(reg.is_read_only(name), "{} should be read-only", name);
        }
        for name in ["write_file", "edit_file", "run_command", "run_script"] {
            assert!(!reg.is_read_only(name), "{} should be mutating", name);
        }
    }

    #[test]
    fn concurrent_read_only_calls_dont_deadlock() {
        // RwLock should allow concurrent reads. This test exercises the
        // exact pattern handle_tool_calls_parallel uses: clone + execute.
        use std::sync::Arc;
        let reg = Arc::new(ToolRegistry::new());
        reg.register(Box::new(ReadTool));

        let mut threads = Vec::new();
        for _ in 0..16 {
            let r = reg.clone();
            threads.push(std::thread::spawn(move || {
                for _ in 0..50 {
                    r.execute_by_name("reader", &serde_json::json!({})).unwrap();
                }
            }));
        }
        for t in threads {
            t.join().unwrap();
        }
    }

    // Tool yang override execute_streaming untuk memancarkan dua delta;
    // registry harus meneruskan callback apa adanya.
    struct Counter;
    impl Tool for Counter {
        fn name(&self) -> &str {
            "counter"
        }
        fn description(&self) -> &str {
            "dummy"
        }
        fn schema(&self) -> ToolSchema {
            ToolSchema::new("counter", "dummy")
        }
        fn execute(&self, _: serde_json::Value) -> Result<serde_json::Value, ToolError> {
            Ok(serde_json::json!({ "ok": true }))
        }
        fn execute_streaming(
            &self,
            _: serde_json::Value,
            on_progress: &dyn Fn(&str),
        ) -> Result<serde_json::Value, ToolError> {
            on_progress("alpha");
            on_progress("beta");
            Ok(serde_json::json!({ "ok": true }))
        }
    }

    #[test]
    fn registry_forwards_streaming_deltas() {
        let reg = ToolRegistry::new();
        reg.register(Box::new(Counter));
        let deltas = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let d2 = deltas.clone();
        let result = reg
            .execute_streaming_by_name("counter", &serde_json::json!({}), &move |s| {
                d2.lock().unwrap().push(s.to_string());
            })
            .unwrap();
        assert_eq!(result, serde_json::json!({ "ok": true }));
        assert_eq!(*deltas.lock().unwrap(), vec!["alpha", "beta"]);
    }
}
