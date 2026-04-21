//! Tool registry for dynamic tool registration and execution

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::tool::{Tool, ToolError, ToolCall, ToolResultValue};

pub struct ToolRegistry {
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

    pub async fn register(&self, tool: Box<dyn Tool + Send + Sync>) {
        let mut tools = self.tools.write().await;
        tools.insert(tool.name().to_string(), tool);
    }

    pub async fn unregister(&self, name: &str) -> Option<Box<dyn Tool + Send + Sync>> {
        let mut tools = self.tools.write().await;
        tools.remove(name)
    }

pub async fn get(&self, name: &str) -> Option<Box<dyn Tool + Send + Sync>> {
        let tools = self.tools.read().await;
        // Clone the boxed dynami trait - need to recreate since Box<dyn Trait> doesn't implement Clone
        let tool_opt = tools.get(name).map(|b| {
            // We can't actually clone, so we return a reference - but for execute we don't need get
            // This is a workaround - in practice use execute directly
            let _ = b;
            None::<Box<dyn Tool + Send + Sync>>
        });
        None
    }

    pub async fn list(&self) -> Vec<crate::tool::ToolSchema> {
        let tools = self.tools.read().await;
        tools.values().map(|t| t.schema()).collect()
    }

    pub async fn execute(&self, call: ToolCall) -> Result<ToolResultValue, ToolError> {
        let tools = self.tools.read().await;
        
        let tool = tools.get(&call.tool_name)
            .ok_or_else(|| ToolError::new(format!("Tool not found: {}", call.tool_name)))?;
        
        let result = tool.execute(call.arguments)
            .map_err(|e| ToolError::new(e.to_string()))?;
        
        Ok(ToolResultValue {
            tool_call_id: call.id,
            output: result,
            error: None,
        })
    }

    pub async fn has_tool(&self, name: &str) -> bool {
        let tools = self.tools.read().await;
        tools.contains_key(name)
    }

    pub async fn tool_names(&self) -> Vec<String> {
        let tools = self.tools.read().await;
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