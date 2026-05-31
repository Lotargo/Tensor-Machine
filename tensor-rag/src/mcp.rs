use tokio_util::sync::CancellationToken;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use tokio::sync::mpsc;
use std::sync::Mutex;
use std::time::Instant;


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

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// A trait for handling specific tool calls from the MCP.
pub trait ToolHandler: Send + Sync {
    fn handle<'a>(
        &'a self,
        args: JsonValue,
        db_pools: &'a crate::db::DbPools,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send + 'a>>;
}


/// Simple Token Bucket Rate Limiter
pub struct RateLimiter {
    capacity: u32,
    tokens: Mutex<f64>,
    last_replenished: Mutex<Instant>,
    refill_rate_per_sec: f64,
}

impl RateLimiter {
    pub fn new(capacity: u32, refill_rate_per_sec: f64) -> Self {
        Self {
            capacity,
            tokens: Mutex::new(capacity as f64),
            last_replenished: Mutex::new(Instant::now()),
            refill_rate_per_sec,
        }
    }

    pub fn acquire(&self) -> bool {
        let mut tokens = self.tokens.lock().unwrap();
        let mut last_replenished = self.last_replenished.lock().unwrap();
        let now = Instant::now();
        let elapsed = now.duration_since(*last_replenished).as_secs_f64();

        *tokens = (*tokens + elapsed * self.refill_rate_per_sec).min(self.capacity as f64);
        *last_replenished = now;

        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }
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
    fn handle<'a>(
        &'a self,
        args: JsonValue,
        db_pools: &'a crate::db::DbPools,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send + 'a>> {
        Box::pin(async move {
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

            let mut instruction_count = 0;
            let max_instructions = 100_000;

            let result = execute_program(&program, &mut vm, db_pools, &mut instruction_count, max_instructions)
                .await
                .map_err(|e| format!("Execution error: {}", e))?;

            match result {
                Some(crate::vm::Value::Result(topk)) => {
                    let mut fbb = flatbuffers::FlatBufferBuilder::new();

                    // Note: The indices are cast to i32 to match the FlatBuffers schema ([int]).
                    // This is safe for now, but will silently truncate if the index exceeds 2^31-1.
                    let indices_i32: Vec<i32> =
                        topk.indices.into_iter().map(|idx| idx as i32).collect();

                    let top_indices_vec = fbb.create_vector(&indices_i32);
                    let top_scores_vec = fbb.create_vector(&topk.scores);

                    let args = crate::result_generated::tensor_rag::fbs::SearchResultArgs {
                        top_indices: Some(top_indices_vec),
                        top_scores: Some(top_scores_vec),
                    };

                    let result_offset =
                        crate::result_generated::tensor_rag::fbs::SearchResult::create(
                            &mut fbb, &args,
                        );
                    fbb.finish(result_offset, None);

                    Ok(fbb.finished_data().to_vec())
                }
                _ => Err("VM did not return a valid TopKResult".to_string()),
            }
        })
    }
}

pub fn register_search_tool(server: &mut McpServer) {
    server.register_tool("search", Box::new(SearchTool));
}

/// The core MCP server that maintains connections and routes tool calls.
pub struct McpServer {
    tools: HashMap<String, Box<dyn ToolHandler>>,
    sessions: HashMap<String, McpSession>,
    pub db_pools: Arc<crate::db::DbPools>,
    pub rate_limiter: Arc<RateLimiter>,
}

impl Default for McpServer {
    fn default() -> Self {
        Self::new(Arc::new(crate::db::DbPools::new()))
    }
}

impl McpServer {
    pub fn new(db_pools: Arc<crate::db::DbPools>) -> Self {
        Self {
            tools: HashMap::new(),
            sessions: HashMap::new(),
            db_pools,
            rate_limiter: Arc::new(RateLimiter::new(100, 100.0)), // 100 req/sec
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
        if !self.rate_limiter.acquire() {
            return Err("Rate limit exceeded".to_string());
        }

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
            match handler.handle(args, &self.db_pools).await {
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
    pub async fn run_server(&mut self, mut rx: mpsc::Receiver<McpRequest>, cancel_token: CancellationToken) {
        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    println!("Shutdown signal received, shutting down gracefully...");
                    // Graceful shutdown can wait for active connections here
                    break;
                }
                req = rx.recv() => {
                    match req {
                        Some(request) => {
                            match self.handle_request(request).await {
                                Ok(_) => {},
                                Err(e) => println!("Error handling request: {}", e),
                            }
                        }
                        None => break, // Channel closed
                    }
                }
            }
        }
    }
}

// async executor
pub fn execute_program<'a>(
    program: &'a crate::ir::Program,
    vm: &'a mut crate::vm::VM,
    db_pools: &'a crate::db::DbPools,
    instruction_count: &'a mut usize,
    max_instructions: usize,
) -> Pin<Box<dyn Future<Output = Result<Option<crate::vm::Value>, String>> + Send + 'a>> {
    Box::pin(async move {
        for op in &program.instructions {
            if *instruction_count >= max_instructions {
                return Err(format!("Execution error: Instruction limit exceeded ({})", max_instructions));
            }
            *instruction_count += 1;

            match &op.op {
                crate::ir::OpCode::RetrieveQdrant {
                    query: _,
                    filters,
                    top_k,
                } => {
                    // Currently Mocking using db_pools method. Wait, DbPools expects ndarray.
                    // Let's just create a dummy query vector for the mock I/O
                    let query_vec = ndarray::Array1::from_elem(768, 0.1);
                    vm.query_vector = query_vec.clone();

                    let (ids, vectors) =
                        db_pools.search_qdrant(&query_vec, filters, *top_k).await?;
                    let batch = crate::arrow::qdrant_to_record_batch(&ids, &vectors)
                        .map_err(|e| format!("Failed to convert qdrant results: {}", e))?;
                    vm.batch = Some(Arc::new(batch));
                }
                crate::ir::OpCode::RetrievePostgres => {
                    let batch = vm
                        .batch
                        .as_ref()
                        .ok_or("No batch found for RetrievePostgres")?;

                    // We mock fetching BM25 scores (actually we need IDs from batch)
                    let id_col = batch.column_by_name("id").ok_or("Missing id column")?;
                    let id_arr = id_col
                        .as_any()
                        .downcast_ref::<arrow::array::Int64Array>()
                        .ok_or("Not Int64Array")?;
                    let real_ids: Vec<usize> =
                        id_arr.iter().map(|v| v.unwrap_or(0) as usize).collect();
                    let scores = db_pools.fetch_bm25_scores(&real_ids).await;

                    vm.stack.push(crate::vm::Value::ArrowArray(Arc::new(
                        arrow::array::Float32Array::from(scores),
                    )));
                }
                crate::ir::OpCode::IfConfidence(threshold, low_plan, high_plan) => {
                    let a = vm.stack.pop().ok_or("Stack underflow in IfConfidence")?;
                    let mut max_score = 0.0;
                    match &a {
                        crate::vm::Value::Tensor1D(t) => {
                            for &score in t.iter() {
                                if score > max_score {
                                    max_score = score;
                                }
                            }
                        }
                        crate::vm::Value::ArrowArray(arr) => {
                            let t_f32 = arr
                                .as_any()
                                .downcast_ref::<arrow::array::Float32Array>()
                                .ok_or("Not Float32Array")?;
                            for score in t_f32.iter().flatten() {
                                if score > max_score {
                                    max_score = score;
                                }
                            }
                        }
                        _ => return Err("Invalid type for IfConfidence condition".to_string()),
                    }

                    vm.stack.push(a);

                    let branch_plan = if max_score >= *threshold {
                        high_plan
                    } else {
                        low_plan
                    };

                    if let Some(res) = execute_program(branch_plan, vm, db_pools, instruction_count, max_instructions).await? {
                        return Ok(Some(res));
                    }
                }
                _ => {
                    if let Some(res) = vm.step(&op.op)? {
                        return Ok(Some(res));
                    }
                }
            }
        }
        Ok(None)
    })
}
