//! Tool registry for dynamic tool registration and execution

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::tool::{Tool, ToolCall, ToolError, ToolResultValue};

pub struct ToolRegistry {
    tools: Arc<Mutex<HashMap<String, Box<dyn Tool + Send + Sync>>>>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn register(&self, tool: Box<dyn Tool + Send + Sync>) {
        let mut tools = self.tools.lock().unwrap();
        tools.insert(tool.name().to_string(), tool);
    }

    pub fn unregister(&self, name: &str) -> Option<Box<dyn Tool + Send + Sync>> {
        let mut tools = self.tools.lock().unwrap();
        tools.remove(name)
    }

    pub fn list(&self) -> Vec<crate::tool::ToolSchema> {
        let tools = self.tools.lock().unwrap();
        tools.values().map(|t| t.schema()).collect()
    }

    pub fn execute(&self, call: ToolCall) -> Result<ToolResultValue, ToolError> {
        let tools = self.tools.lock().unwrap();

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
        let tools = self.tools.lock().unwrap();
        tools.contains_key(name)
    }

    pub fn tool_names(&self) -> Vec<String> {
        let tools = self.tools.lock().unwrap();
        tools.keys().cloned().collect()
    }
}

impl Clone for ToolRegistry {
    fn clone(&self) -> Self {
        Self {
            tools: self.tools.clone(),
        }
    }
}
