//! MCP transport layer - stdio and HTTP

use crate::mcp::types::{JsonRpcRequest, JsonRpcResponse};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::process::{Child, ChildStdin, Command, Stdio};

pub trait McpTransport: Send + Sync {
    fn send_and_recv(&mut self, request: JsonRpcRequest) -> Result<JsonRpcResponse, String>;
    fn close(&mut self);
}

pub struct StdioTransport {
    child: Child,
    stdin: ChildStdin,
    stdout: std::io::BufReader<std::process::ChildStdout>,
}

impl StdioTransport {
    pub fn new(
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Self, String> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        for (key, value) in env {
            cmd.env(key, value);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn MCP server '{}': {}", command, e))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Failed to get stdin of MCP server".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Failed to get stdout of MCP server".to_string())?;

        Ok(Self {
            child,
            stdin,
            stdout: std::io::BufReader::new(stdout),
        })
    }
}

impl McpTransport for StdioTransport {
    fn send_and_recv(&mut self, request: JsonRpcRequest) -> Result<JsonRpcResponse, String> {
        let msg = serde_json::to_string(&request)
            .map_err(|e| format!("Failed to serialize request: {}", e))?;
        let line = format!("{}\n", msg);

        self.stdin
            .write_all(line.as_bytes())
            .map_err(|e| format!("Failed to write to MCP server stdin: {}", e))?;
        self.stdin
            .flush()
            .map_err(|e| format!("Failed to flush stdin: {}", e))?;

        let mut response_line = String::new();
        self.stdout
            .read_line(&mut response_line)
            .map_err(|e| format!("Failed to read from MCP server stdout: {}", e))?;

        let response: JsonRpcResponse =
            serde_json::from_str(response_line.trim()).map_err(|e| {
                format!(
                    "Failed to parse MCP response: {} (input: {:?})",
                    e, response_line
                )
            })?;

        Ok(response)
    }

    fn close(&mut self) {
        let _ = self.stdin.flush();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        self.close();
    }
}

pub struct HttpTransport {
    url: String,
    client: reqwest::blocking::Client,
    headers: HashMap<String, String>,
}

impl HttpTransport {
    pub fn new(url: &str, headers: HashMap<String, String>) -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

        Ok(Self {
            url: url.to_string(),
            client,
            headers,
        })
    }
}

impl McpTransport for HttpTransport {
    fn send_and_recv(&mut self, request: JsonRpcRequest) -> Result<JsonRpcResponse, String> {
        let mut req = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json");

        for (key, value) in &self.headers {
            req = req.header(key.as_str(), value.as_str());
        }

        let response = req
            .json(&request)
            .send()
            .map_err(|e| format!("HTTP request to MCP server failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(format!("MCP server returned HTTP {}: {}", status, body));
        }

        let rpc_response: JsonRpcResponse = response
            .json()
            .map_err(|e| format!("Failed to parse MCP HTTP response: {}", e))?;

        Ok(rpc_response)
    }

    fn close(&mut self) {}
}
