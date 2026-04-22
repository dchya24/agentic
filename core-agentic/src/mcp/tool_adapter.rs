//! Adapter to wrap MCP remote tools as local Tool trait implementations

use std::sync::{Arc, Mutex};

use crate::mcp::client::McpClient;
use crate::tool::{Tool, ToolError, ToolResult, ToolSchema};

type SharedClient = Arc<Mutex<McpClient>>;

pub struct McpToolAdapter {
    tool_name: String,
    tool_description: String,
    schema: ToolSchema,
    client: SharedClient,
}

pub fn mcp_tools_from_config(
    config: &crate::mcp::types::McpServerConfig,
) -> Result<Vec<Box<dyn Tool + Send + Sync>>, String> {
    let client = McpClient::connect(config)?;
    let tools = client.tools().to_vec();
    let schemas = client.tool_schemas();
    let shared = Arc::new(Mutex::new(client));

    Ok(tools
        .into_iter()
        .zip(schemas.into_iter())
        .map(|(mcp_tool, schema)| {
            let adapter = McpToolAdapter {
                tool_name: mcp_tool.name.clone(),
                tool_description: mcp_tool.description.clone().unwrap_or_default(),
                schema,
                client: shared.clone(),
            };
            Box::new(adapter) as Box<dyn Tool + Send + Sync>
        })
        .collect())
}

impl Tool for McpToolAdapter {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.tool_description
    }

    fn schema(&self) -> ToolSchema {
        self.schema.clone()
    }

    fn execute(&self, args: serde_json::Value) -> ToolResult<serde_json::Value> {
        let mut client = self
            .client
            .lock()
            .map_err(|e| ToolError::new(format!("Lock error: {}", e)))?;
        client.call_tool(&self.tool_name, args)
    }
}
