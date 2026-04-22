//! MCP client - manages connection to a single MCP server

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::mcp::transport::{HttpTransport, McpTransport, StdioTransport};
use crate::mcp::types::*;
use crate::tool::{ToolError, ToolParam, ToolSchema};

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed)
}

pub struct McpClient {
    transport: Box<dyn McpTransport>,
    server_info: Option<ServerInfo>,
    tools_cache: Vec<McpToolSchema>,
}

impl McpClient {
    pub fn connect(config: &McpServerConfig) -> Result<Self, String> {
        let transport: Box<dyn McpTransport> = if config.is_stdio() {
            let command = config
                .command
                .as_ref()
                .ok_or("Missing command for stdio transport")?;
            let args = config.args.as_ref().cloned().unwrap_or_default();
            let env = config.env.as_ref().cloned().unwrap_or_default();
            Box::new(StdioTransport::new(command, &args, &env)?)
        } else if config.is_http() {
            let url = config
                .url
                .as_ref()
                .ok_or("Missing URL for HTTP transport")?;
            let headers = config.headers.as_ref().cloned().unwrap_or_default();
            Box::new(HttpTransport::new(url, headers)?)
        } else {
            return Err(
                "MCP server config must have either 'command' (stdio) or 'url' (http)".into(),
            );
        };

        let mut client = Self {
            transport,
            server_info: None,
            tools_cache: Vec::new(),
        };

        client.initialize()?;
        client.tools_cache = client.discover_tools()?;

        Ok(client)
    }

    fn initialize(&mut self) -> Result<(), String> {
        let params = InitializeParams {
            protocol_version: "2024-11-05".into(),
            capabilities: serde_json::json!({}),
            client_info: ClientInfo {
                name: "core-agentic".into(),
                version: "0.1.0".into(),
            },
        };

        let request = JsonRpcRequest::new(
            next_id(),
            "initialize",
            Some(serde_json::to_value(params).map_err(|e| format!("Serialize error: {}", e))?),
        );

        let response = self.transport.send_and_recv(request)?;

        if let Some(error) = response.error {
            return Err(format!(
                "MCP initialize error (code {}): {}",
                error.code, error.message
            ));
        }

        let init_result: InitializeResult =
            serde_json::from_value(response.result.ok_or("No result in initialize response")?)
                .map_err(|e: serde_json::Error| {
                    format!("Failed to parse initialize result: {}", e)
                })?;

        self.server_info = init_result.server_info;

        let initialized = JsonRpcRequest::new(next_id(), "notifications/initialized", None);
        let _ = self.transport.send_and_recv(initialized);

        Ok(())
    }

    fn discover_tools(&mut self) -> Result<Vec<McpToolSchema>, String> {
        let request = JsonRpcRequest::new(next_id(), "tools/list", None);
        let response = self.transport.send_and_recv(request)?;

        if let Some(error) = response.error {
            return Err(format!(
                "MCP tools/list error (code {}): {}",
                error.code, error.message
            ));
        }

        let result: ToolsListResult =
            serde_json::from_value(response.result.ok_or("No result in tools/list response")?)
                .map_err(|e: serde_json::Error| {
                    format!("Failed to parse tools/list result: {}", e)
                })?;

        Ok(result.tools)
    }

    pub fn server_info(&self) -> Option<&ServerInfo> {
        self.server_info.as_ref()
    }

    pub fn tools(&self) -> &[McpToolSchema] {
        &self.tools_cache
    }

    pub fn tool_schemas(&self) -> Vec<ToolSchema> {
        self.tools_cache
            .iter()
            .map(|mcp_tool| {
                let mut params = HashMap::new();
                let mut required = Vec::new();

                if let Some(schema) = &mcp_tool.input_schema {
                    if let Some(props) = schema.get("properties").and_then(|p| p.as_object()) {
                        for (name, def) in props {
                            let param_type = def
                                .get("type")
                                .and_then(|t| t.as_str())
                                .unwrap_or("string")
                                .to_string();
                            let description = def
                                .get("description")
                                .and_then(|d| d.as_str())
                                .map(|s| s.to_string());
                            let default = def.get("default").cloned();

                            params.insert(
                                name.clone(),
                                ToolParam {
                                    param_type,
                                    description,
                                    default,
                                },
                            );
                        }
                    }

                    if let Some(req) = schema.get("required").and_then(|r| r.as_array()) {
                        for r in req {
                            if let Some(s) = r.as_str() {
                                required.push(s.to_string());
                            }
                        }
                    }
                }

                ToolSchema {
                    name: mcp_tool.name.clone(),
                    description: mcp_tool.description.clone().unwrap_or_default(),
                    parameters: params,
                    required,
                }
            })
            .collect()
    }

    pub fn call_tool(
        &mut self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, ToolError> {
        let params = ToolCallParams {
            name: name.to_string(),
            arguments,
        };

        let request = JsonRpcRequest::new(
            next_id(),
            "tools/call",
            Some(
                serde_json::to_value(&params)
                    .map_err(|e| ToolError::new(format!("Serialize error: {}", e)))?,
            ),
        );

        let response = self
            .transport
            .send_and_recv(request)
            .map_err(|e| ToolError::new(e))?;

        if let Some(error) = response.error {
            return Err(ToolError::new(format!(
                "MCP tool call error (code {}): {}",
                error.code, error.message
            )));
        }

        let call_result: ToolCallResult = serde_json::from_value(
            response
                .result
                .ok_or_else(|| ToolError::new("No result in tool call response"))?,
        )
        .map_err(|e: serde_json::Error| {
            ToolError::new(format!("Failed to parse tool call result: {}", e))
        })?;

        let text_parts: Vec<&str> = call_result
            .content
            .iter()
            .filter_map(|c| {
                if c.content_type == "text" {
                    c.text.as_deref()
                } else {
                    None
                }
            })
            .collect();

        Ok(serde_json::json!({
            "content": text_parts.join("\n"),
            "is_error": call_result.is_error.unwrap_or(false),
        }))
    }

    pub fn close(&mut self) {
        self.transport.close();
    }
}
