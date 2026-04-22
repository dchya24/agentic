//! MCP (Model Context Protocol) client module

pub mod client;
pub mod tool_adapter;
pub mod transport;
pub mod types;

pub use client::McpClient;
pub use tool_adapter::McpToolAdapter;
pub use types::McpServerConfig;
