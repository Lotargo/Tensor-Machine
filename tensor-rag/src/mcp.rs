use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use tokio::sync::mpsc;

/// An incoming request from an LLM Agent using the Model Context Protocol.
#[derive(Debug, Deserialize, Serialize)]
pub struct McpRequest {
    pub tool: String,
    pub args: JsonValue,
    pub session_id: String,
    pub sequence_number: u64,
}

/// A response back to the LLM Agent.
#[derive(Debug, Deserialize, Serialize)]
pub struct McpResponse {
    pub status: u32,
    pub body: Vec<u8>,
    pub sequence_number: u64,
}

/// A trait for handling specific tool calls from the MCP.
pub trait ToolHandler: Send + Sync {
    fn handle(&self, args: JsonValue) -> Result<Vec<u8>, String>;
}

/// Represents an active MCP session with guaranteed message ordering
pub struct McpSession {
    pub session_id: String,
    pub expected_sequence: u64,
    pub tx: mpsc::Sender<McpRequest>,
}

impl McpSession {
    pub fn new(session_id: String, tx: mpsc::Sender<McpRequest>) -> Self {
        Self {
            session_id,
            expected_sequence: 1, // Start expecting seq #1
            tx,
        }
    }
}

pub struct SearchTool;

impl ToolHandler for SearchTool {
    fn handle(&self, args: JsonValue) -> Result<Vec<u8>, String> {
        let ast = if let Some(dsl_string) = args.get("dsl").and_then(|v| v.as_str()) {
            crate::dsl::parse_dsl(dsl_string)?
        } else {
            // For JSON fallback, wrap QueryRequest inside AstNode::Search
            let req: crate::ir::QueryRequest = serde_json::from_value(args)
                .map_err(|e| format!("Failed to parse QueryRequest JSON: {}", e))?;
            crate::ir::AstNode::Search(req)
        };

        let program = crate::ir::Planner::compile(ast);
        let mut vm = crate::vm::VM::new();

        let result = vm.run(program)
            .map_err(|e| format!("VM Execution error: {}", e))?;

        match result {
            Some(crate::vm::Value::Result(topk)) => {
                let mut json_obj = serde_json::Map::new();
                json_obj.insert("indices".to_string(), serde_json::json!(topk.indices));
                json_obj.insert("scores".to_string(), serde_json::json!(topk.scores));

                let json_body = serde_json::to_vec(&JsonValue::Object(json_obj))
                    .map_err(|e| format!("Failed to serialize result: {}", e))?;

                Ok(json_body)
            }
            _ => Err("VM did not return a valid TopKResult".to_string())
        }
    }
}

pub fn register_search_tool(server: &mut McpServer) {
    server.register_tool("search", Box::new(SearchTool));
}

/// The core MCP server that maintains connections and routes tool calls.
pub struct McpServer {
    tools: HashMap<String, Box<dyn ToolHandler>>,
    sessions: HashMap<String, McpSession>,
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
            sessions: HashMap::new(),
        }
    }

    /// Register a new tool handler.
    pub fn register_tool(&mut self, name: &str, handler: Box<dyn ToolHandler>) {
        self.tools.insert(name.to_string(), handler);
    }

    /// Create and register a new session
    pub fn create_session(&mut self, session_id: String) -> mpsc::Receiver<McpRequest> {
        let (tx, rx) = mpsc::channel(100);
        let session = McpSession::new(session_id.clone(), tx);
        self.sessions.insert(session_id, session);
        rx
    }

    /// Dispatch an incoming request to the appropriate tool handler.
    pub async fn handle_request(&mut self, request: McpRequest) -> Result<McpResponse, String> {
        // Validate session and sequence number for message ordering
        let seq_num = request.sequence_number;
        let tool = request.tool.clone();
        let args = request.args.clone();

        if let Some(session) = self.sessions.get_mut(&request.session_id) {
            if request.sequence_number != session.expected_sequence {
                return Err(format!(
                    "Out of order message. Expected sequence: {}, but got: {}",
                    session.expected_sequence, request.sequence_number
                ));
            }
            session.expected_sequence += 1;

            // Forward request to session channel for any background workers listening
            let _ = session.tx.send(request).await;
        } else {
            return Err(format!("Session not found: {}", request.session_id));
        }

        if let Some(handler) = self.tools.get(&tool) {
            match handler.handle(args) {
                Ok(body) => Ok(McpResponse {
                    status: 200,
                    body,
                    sequence_number: seq_num,
                }),
                Err(err) => Err(err),
            }
        } else {
            Err(format!("Tool not found: {}", tool))
        }
    }
}
