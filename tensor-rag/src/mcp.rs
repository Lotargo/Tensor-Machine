use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;

/// An incoming request from an LLM Agent using the Model Context Protocol.
#[derive(Debug, Deserialize, Serialize)]
pub struct McpRequest {
    pub tool: String,
    pub args: JsonValue,
    pub session_id: String,
}

/// A response back to the LLM Agent.
#[derive(Debug, Deserialize, Serialize)]
pub struct McpResponse {
    pub status: u32,
    pub body: Vec<u8>,
}

/// A trait for handling specific tool calls from the MCP.
pub trait ToolHandler: Send + Sync {
    fn handle(&self, args: JsonValue) -> Result<McpResponse, String>;
}

/// The core MCP server that maintains connections and routes tool calls.
pub struct McpServer {
    tools: HashMap<String, Box<dyn ToolHandler>>,
}

impl Default for McpServer {
    fn default() -> Self {
        Self::new()
    }
}

impl McpServer {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a new tool handler.
    pub fn register_tool(&mut self, name: &str, handler: Box<dyn ToolHandler>) {
        self.tools.insert(name.to_string(), handler);
    }

    /// Dispatch an incoming request to the appropriate tool handler.
    pub fn handle_request(&self, request: McpRequest) -> Result<McpResponse, String> {
        if let Some(handler) = self.tools.get(&request.tool) {
            handler.handle(request.args)
        } else {
            Err(format!("Tool not found: {}", request.tool))
        }
    }
}
