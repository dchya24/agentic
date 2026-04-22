//! Tool registry for dynamic tool registration and execution

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::providers::{ToolDefinition, ToolFunction};
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

    pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let tools = self.tools.lock().unwrap();
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
        let tools = self.tools.lock().unwrap();
        let tool = tools
            .get(name)
            .ok_or_else(|| ToolError::new(format!("Tool not found: {}", name)))?;
        tool.execute(args.clone())
    }
}

impl Clone for ToolRegistry {
    fn clone(&self) -> Self {
        Self {
            tools: self.tools.clone(),
        }
    }
}
