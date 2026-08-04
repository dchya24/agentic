//! Adapter to wrap MCP remote tools as local Tool trait implementations.
//!
//! Provides both sync (`McpToolAdapter`) and async (`AsyncMcpToolAdapter`)
//! adapters. The async adapter uses `tokio::runtime::Handle::block_on()`
//! to satisfy the synchronous `Tool` trait interface.

use std::sync::{Arc, Mutex};

use crate::mcp::client::{AsyncMcpClient, McpClient};
use crate::tool::{Tool, ToolError, ToolResult, ToolSchema};

// ---------------------------------------------------------------------------
// Sync adapter (original)
// ---------------------------------------------------------------------------

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
        .zip(schemas)
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

// ===========================================================================
// Async adapter
// ===========================================================================

type SharedAsyncClient = Arc<tokio::sync::Mutex<AsyncMcpClient>>;

/// Adapter that wraps an async `AsyncMcpClient` behind the sync `Tool` trait.
/// Uses `tokio::runtime::Handle::block_on()` to bridge sync→async.
pub struct AsyncMcpToolAdapter {
    tool_name: String,
    tool_description: String,
    schema: ToolSchema,
    client: SharedAsyncClient,
}

impl AsyncMcpToolAdapter {
    pub fn new(
        tool_name: String,
        tool_description: String,
        schema: ToolSchema,
        client: SharedAsyncClient,
    ) -> Self {
        Self {
            tool_name,
            tool_description,
            schema,
            client,
        }
    }
}

/// Connect to an MCP server asynchronously and return tool adapters
/// that can be registered in the sync `ToolRegistry`.
///
/// This must be called within a tokio runtime context.
pub async fn async_mcp_tools_from_config(
    config: &crate::mcp::types::McpServerConfig,
) -> Result<Vec<Box<dyn Tool + Send + Sync>>, String> {
    let client = AsyncMcpClient::connect(config).await?;
    let tools = client.tools().to_vec();
    let schemas = client.tool_schemas();
    let shared = Arc::new(tokio::sync::Mutex::new(client));

    Ok(tools
        .into_iter()
        .zip(schemas)
        .map(|(mcp_tool, schema)| {
            let adapter = AsyncMcpToolAdapter::new(
                mcp_tool.name.clone(),
                mcp_tool.description.clone().unwrap_or_default(),
                schema,
                shared.clone(),
            );
            Box::new(adapter) as Box<dyn Tool + Send + Sync>
        })
        .collect())
}

impl Tool for AsyncMcpToolAdapter {
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
        let client = self.client.clone();
        let tool_name = self.tool_name.clone();

        // Bridge sync → async using tokio handle
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|e| ToolError::new(format!("No tokio runtime available: {}", e)))?;

        handle.block_on(async {
            let mut client = client.lock().await;
            client.call_tool(&tool_name, args).await
        })
    }
}
