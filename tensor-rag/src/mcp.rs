use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

/// An incoming request from an LLM Agent using the Model Context Protocol.
#[derive(Debug, Deserialize, Serialize)]
pub struct McpRequest {
    pub tool: String,
    pub args: JsonValue,
    pub session_id: String,
    pub sequence_number: u64,
    #[serde(skip)]
    pub response_tx: Option<tokio::sync::oneshot::Sender<Result<McpResponse, String>>>,
}

impl Clone for McpRequest {
    fn clone(&self) -> Self {
        Self {
            tool: self.tool.clone(),
            args: self.args.clone(),
            session_id: self.session_id.clone(),
            sequence_number: self.sequence_number,
            response_tx: None,
        }
    }
}

/// A response back to the LLM Agent.
#[derive(Debug, Deserialize, Serialize, Clone)]
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
        metrics: Option<Arc<crate::metrics::Metrics>>,
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

use crate::shared_memory::SharedMemoryManager;
use crate::vm::{VmExecutionPayload, VmRequest, VmResponse, VmResultPayload};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::net::UnixStream;

#[cfg(not(unix))]
pub struct UnixStream;

#[cfg(not(unix))]
impl UnixStream {
    pub fn connect<P: AsRef<std::path::Path>>(_path: P) -> std::io::Result<Self> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Unix domain sockets are not supported on Windows",
        ))
    }
}

#[cfg(not(unix))]
impl std::io::Read for UnixStream {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        Ok(0)
    }
}

#[cfg(not(unix))]
impl std::io::Write for UnixStream {
    fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
        Ok(0)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub struct VmIpcClient {
    socket_path: String,
}

impl VmIpcClient {
    pub fn from_config() -> Self {
        Self::new(crate::config::get_vm_socket_path())
    }

    pub fn new(socket_path: String) -> Self {
        Self { socket_path }
    }

    pub fn execute_program(
        &self,
        session_id: &str,
        sequence: u64,
        program: crate::ir::Program,
        query_vector: Vec<f32>,
    ) -> Result<Option<crate::vm::Value>, String> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .map_err(|e| format!("Failed to connect to VM worker: {}", e))?;

        let shm_manager = SharedMemoryManager::new();
        let shm_name = format!("req_{}_{}", session_id, sequence);

        let req_payload = VmExecutionPayload {
            program,
            query_vector,
        };
        let encoded =
            bincode::serialize(&req_payload).map_err(|e| format!("Serialization error: {}", e))?;

        let region = shm_manager.allocate(&shm_name)?;
        {
            let mut file = region.get_file_for_write()?;
            file.write_all(&encoded)
                .map_err(|e| format!("Write error: {}", e))?;
        }

        let req = VmRequest::Execute {
            session_id: session_id.to_string(),
            sequence,
            shm_name: shm_name.clone(),
        };

        let payload = bincode::serialize(&req).unwrap();
        let len = payload.len() as u32;
        stream.write_all(&len.to_le_bytes()).unwrap();
        stream.write_all(&payload).unwrap();

        let mut len_buf = [0u8; 4];
        stream
            .read_exact(&mut len_buf)
            .map_err(|e| format!("Read len error: {}", e))?;
        let len = u32::from_le_bytes(len_buf) as usize;

        let mut resp_payload = vec![0u8; len];
        stream
            .read_exact(&mut resp_payload)
            .map_err(|e| format!("Read payload error: {}", e))?;

        let resp: VmResponse = bincode::deserialize(&resp_payload).unwrap();

        let _ = region.destroy();

        match resp {
            VmResponse::Result {
                shm_name: out_shm, ..
            } => {
                let out_region = shm_manager.open(&out_shm)?;
                let mut file = out_region.get_file_for_read()?;
                let mut buf = Vec::new();
                file.read_to_end(&mut buf)
                    .map_err(|e| format!("Read out error: {}", e))?;

                let res_payload: VmResultPayload = bincode::deserialize(&buf)
                    .map_err(|e| format!("Deserialization error: {}", e))?;
                let _ = out_region.destroy();

                Ok(res_payload.value.map(|v| crate::vm::Value::from(v)))
            }
            VmResponse::Error { message, .. } => Err(message),
            _ => Err("Unexpected response type".to_string()),
        }
    }
}

pub struct SearchTool;

fn trim_search_payload(
    indices: Vec<usize>,
    scores: Vec<f32>,
    documents: Vec<String>,
) -> serde_json::Value {
    let token_limit = crate::config::get_search_max_response_tokens();
    let total_results = indices.len().min(scores.len()).min(documents.len());
    let mut kept_indices = Vec::new();
    let mut kept_scores = Vec::new();
    let mut kept_documents = Vec::new();
    let mut consumed_tokens = 0usize;

    for ((index, score), document) in indices
        .into_iter()
        .zip(scores.into_iter())
        .zip(documents.into_iter())
    {
        let document_tokens = crate::chunker::count_tokens(&document);
        if consumed_tokens + document_tokens > token_limit {
            break;
        }

        consumed_tokens += document_tokens;
        kept_indices.push(index);
        kept_scores.push(score);
        kept_documents.push(document);
    }

    let trimmed = kept_indices.len() < total_results;
    let warning = if trimmed {
        Some(format!(
            "Search response was trimmed to {} tokens; trailing results were dropped.",
            token_limit
        ))
    } else {
        None
    };

    serde_json::json!({
        "indices": kept_indices,
        "scores": kept_scores,
        "documents": kept_documents,
        "token_limit": token_limit,
        "returned_tokens": consumed_tokens,
        "warning": warning
    })
}

impl ToolHandler for SearchTool {
    fn handle<'a>(
        &'a self,
        args: JsonValue,
        db_pools: &'a crate::db::DbPools,
        metrics: Option<Arc<crate::metrics::Metrics>>,
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
            let _ = vm.init_gpu().await;

            let mut instruction_count = 0;
            let max_instructions = crate::config::get_vm_max_instructions();

            let result = if std::env::var("TENSOR_VM_MODE").unwrap_or_default() == "ipc" {
                let client = VmIpcClient::from_config();
                client.execute_program("default_session", 0, program, vm.query_vector.to_vec())
            } else {
                execute_program(
                    &program,
                    &mut vm,
                    db_pools,
                    &mut instruction_count,
                    max_instructions,
                )
                .await
                .map_err(|e| format!("Execution error: {}", e))
            }?;

            match result {
                Some(crate::vm::Value::Result(topk)) => {
                    let mut fbb = flatbuffers::FlatBufferBuilder::new();

                    // Note: The indices are cast to i32 to match the FlatBuffers schema ([int]).
                    // This is safe for now, but will silently truncate if the index exceeds 2^31-1.
                    let indices_i32: Vec<i32> =
                        topk.indices.iter().map(|&idx| idx as i32).collect();

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

                    if let Some(m) = &metrics {
                        m.inc_results(topk.indices.len() as u64);
                    }

                    Ok(fbb.finished_data().to_vec())
                }
                _ => {
                    if let Some(m) = &metrics {
                        m.inc_failures();
                    }
                    Err("VM did not return a valid TopKResult".to_string())
                }
            }
        })
    }
}

pub struct SearchTextTool;

impl ToolHandler for SearchTextTool {
    fn handle<'a>(
        &'a self,
        args: JsonValue,
        db_pools: &'a crate::db::DbPools,
        metrics: Option<Arc<crate::metrics::Metrics>>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send + 'a>> {
        Box::pin(async move {
            let ast = if let Some(dsl_string) = args.get("dsl").and_then(|v| v.as_str()) {
                crate::dsl::parse_dsl(dsl_string)?
            } else {
                let req: crate::ir::QueryRequest = serde_json::from_value(args)
                    .map_err(|e| format!("Failed to parse QueryRequest JSON: {}", e))?;
                crate::ir::AstNode::Search(req)
            };

            let program = crate::ir::Planner::compile(ast);
            let mut vm = crate::vm::VM::new();
            let _ = vm.init_gpu().await;

            let mut instruction_count = 0;
            let max_instructions = crate::config::get_vm_max_instructions();

            let result = if std::env::var("TENSOR_VM_MODE").unwrap_or_default() == "ipc" {
                let client = VmIpcClient::from_config();
                client.execute_program("default_session", 0, program, vm.query_vector.to_vec())
            } else {
                execute_program(
                    &program,
                    &mut vm,
                    db_pools,
                    &mut instruction_count,
                    max_instructions,
                )
                .await
                .map_err(|e| format!("Execution error: {}", e))
            }?;

            match result {
                Some(crate::vm::Value::Result(topk)) => {
                    let metadata = db_pools.fetch_metadata(&topk.indices).await;
                    let response_json = trim_search_payload(topk.indices, topk.scores, metadata);

                    if let Some(m) = &metrics {
                        let count = response_json
                            .get("indices")
                            .and_then(|value| value.as_array())
                            .map(|items| items.len())
                            .unwrap_or(0);
                        m.inc_results(count as u64);
                    }

                    Ok(serde_json::to_vec(&response_json).unwrap())
                }
                _ => {
                    if let Some(m) = &metrics {
                        m.inc_failures();
                    }
                    Err("VM did not return a valid TopKResult".to_string())
                }
            }
        })
    }
}

pub struct ReadDocumentTool;

impl ToolHandler for ReadDocumentTool {
    fn handle<'a>(
        &'a self,
        args: JsonValue,
        db_pools: &'a crate::db::DbPools,
        _metrics: Option<Arc<crate::metrics::Metrics>>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send + 'a>> {
        Box::pin(async move {
            let doc_uuid = args
                .get("doc_uuid")
                .and_then(|value| value.as_str())
                .ok_or("Missing required argument: doc_uuid")?;

            let document = db_pools
                .fetch_document_by_uuid(doc_uuid)
                .await?
                .ok_or_else(|| format!("Document not found for doc_uuid={}", doc_uuid))?;

            let token_count = crate::chunker::count_tokens(&document.text);
            let token_limit = crate::config::get_read_document_max_tokens();

            let response_json = if token_count > token_limit {
                serde_json::json!({
                    "doc_uuid": document.doc_uuid,
                    "filename": document.filename,
                    "token_count": token_count,
                    "token_limit": token_limit,
                    "warning": format!(
                        "Document exceeds the read limit: {} tokens > {} tokens. Full document read is blocked.",
                        token_count,
                        token_limit
                    ),
                    "content": serde_json::Value::Null
                })
            } else {
                serde_json::json!({
                    "doc_uuid": document.doc_uuid,
                    "filename": document.filename,
                    "token_count": token_count,
                    "token_limit": token_limit,
                    "warning": serde_json::Value::Null,
                    "content": document.text
                })
            };

            Ok(serde_json::to_vec(&response_json).unwrap())
        })
    }
}

pub struct ListDocumentsTool;

impl ToolHandler for ListDocumentsTool {
    fn handle<'a>(
        &'a self,
        _args: JsonValue,
        db_pools: &'a crate::db::DbPools,
        _metrics: Option<Arc<crate::metrics::Metrics>>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send + 'a>> {
        Box::pin(async move {
            let pool = db_pools
                .postgres
                .as_ref()
                .ok_or("PostgreSQL pool not initialized")?;

            // Fetch distinct filename and doc_uuid from PostgreSQL
            let rows = sqlx::query(
                "SELECT DISTINCT filename, doc_uuid FROM documents WHERE doc_uuid IS NOT NULL AND doc_uuid <> '' ORDER BY filename"
            )
            .fetch_all(pool)
            .await
            .map_err(|e| format!("Database error: {}", e))?;

            use sqlx::Row;
            let mut docs = Vec::new();
            for row in rows {
                let filename: String = row.try_get("filename").unwrap_or_else(|_| "Unknown".to_string());
                let doc_uuid: String = row.try_get("doc_uuid").unwrap_or_default();
                docs.push(serde_json::json!({
                    "filename": filename,
                    "doc_uuid": doc_uuid
                }));
            }

            let response_json = serde_json::json!({
                "documents": docs
            });

            Ok(serde_json::to_vec(&response_json).unwrap())
        })
    }
}

pub struct HelpTool;

impl ToolHandler for HelpTool {
    fn handle<'a>(
        &'a self,
        _args: JsonValue,
        _db_pools: &'a crate::db::DbPools,
        _metrics: Option<Arc<crate::metrics::Metrics>>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send + 'a>> {
        Box::pin(async move {
            let help_text = r#"
# TENSOR MACHINE RAG HELP & DOCUMENTATION

## 1. System Overview
Tensor Machine is a hybrid search system combining vector search (dense embeddings) and keyword search (BM25 lexical scoring).
It supports document ingestion, automatic chunking, dense vector generation using E5 models, and vector storage in Qdrant, alongside metadata storage in PostgreSQL.

## 2. Tools Reference

### search
* **Purpose:** Queries the database using semantic search and lexical matching.
* **Arguments:**
  - `query` (string, required): The search text. DO NOT use search engine question words (such as 'what', 'how', 'why', 'что', 'как', 'почему', etc.). Always query using only descriptive search keywords or phrases. Example: "Arrow Flight vector VM transfer".
  - `final_top_k` (integer, optional): The number of top documents/segments to return. Defaults to the configured `.env` value.
  - `doc_uuid` (string, optional): Search only within the document with this UUID.
  - `alpha` (number, optional): Hybrid weighting where `1.0` is semantic-only and `0.0` is BM25-only.
  - `dsl` (string, optional): A query string written in S-expression DSL for advanced plans.
* **Best Practices:**
  - For general queries, use simple key phrases instead of long sentences. Do not use conversational filler or question words.
  - Run the `list_documents` tool first to find the `doc_uuid` if you want to restrict search to a single document.
  - Search responses are softly trimmed from the tail when they exceed the configured token budget.

### read_document
* **Purpose:** Reconstructs and returns the full text of one indexed document.
* **Arguments:**
  - `doc_uuid` (string, required): UUID of the target document returned by `list_documents`.
* **Behavior:**
  - If the document exceeds the configured full-read token limit, the tool returns a warning instead of the body.

### list_documents
* **Purpose:** Retrieves all indexed documents in the database.
* **Output:** A JSON array containing the `filename` and `doc_uuid` for each document. Use these UUIDs to search within specific files.

### help
* **Purpose:** Returns this documentation guide.

## 3. Query DSL (S-Expression)
If you require complex query orchestration, you can use the `dsl` field. Supported forms:
* Hybrid scoring: `(hybrid "query" semantic_weight bm25_weight)`
* MMR diversity: `(mmr "query" diversity_factor)`
* Reciprocal Rank Fusion: `(rrf "query" k)`
* Self-RAG conditional: `(self_rag "query" threshold low_plan_ast high_plan_ast)`
* Corrective RAG: `(corrective_rag "query" "fallback_query")`
"#;

            let response_json = serde_json::json!({
                "help": help_text
            });

            Ok(serde_json::to_vec(&response_json).unwrap())
        })
    }
}

pub fn register_search_tool(server: &mut McpServer) {
    server.register_tool("search", Box::new(SearchTool));
    server.register_tool("search_text", Box::new(SearchTextTool));
    server.register_tool("read_document", Box::new(ReadDocumentTool));
    server.register_tool("list_documents", Box::new(ListDocumentsTool));
    server.register_tool("help", Box::new(HelpTool));
}

/// The core MCP server that maintains connections and routes tool calls.
pub struct McpServer {
    tools: HashMap<String, Box<dyn ToolHandler>>,
    sessions: HashMap<String, McpSession>,
    pub db_pools: Arc<crate::db::DbPools>,
    pub rate_limiter: Arc<RateLimiter>,
    pub metrics: Option<Arc<crate::metrics::Metrics>>,
}

impl Default for McpServer {
    fn default() -> Self {
        Self::new(Arc::new(crate::db::DbPools::new()), None)
    }
}

impl McpServer {
    pub fn new(
        db_pools: Arc<crate::db::DbPools>,
        metrics: Option<Arc<crate::metrics::Metrics>>,
    ) -> Self {
        Self {
            tools: HashMap::new(),
            sessions: HashMap::new(),
            db_pools,
            rate_limiter: Arc::new(RateLimiter::new(crate::config::get_mcp_rate_limit(), crate::config::get_mcp_rate_limit_refill())),
            metrics,
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

        if !self.sessions.contains_key(&request.session_id) {
            let (tx, _rx) = mpsc::channel(100);
            let mut session = McpSession::new(request.session_id.clone(), tx);
            session.expected_sequence = request.sequence_number;
            self.sessions.insert(request.session_id.clone(), session);
        }

        let session = self.sessions.get_mut(&request.session_id).unwrap();
        if request.sequence_number != session.expected_sequence {
            return Err(format!(
                "Out of order message. Expected sequence: {}, but got: {}",
                session.expected_sequence, request.sequence_number
            ));
        }
        session.expected_sequence += 1;

        // Forward request to session channel for any background workers listening
        let _ = session.tx.send(request).await;

        if let Some(handler) = self.tools.get(&tool) {
            let start = Instant::now();
            if let Some(m) = &self.metrics {
                m.inc_requests();
                m.inc_active();
            }

            let result = handler
                .handle(args, &self.db_pools, self.metrics.clone())
                .await;

            if let Some(m) = &self.metrics {
                let db = &self.db_pools;
                m.set_cb_state("qdrant", db.qdrant_cb.state() as f64);
                m.set_cb_state("postgres", db.postgres_cb.state() as f64);
                m.set_cb_state("ml", db.ml_cb.state() as f64);
                m.dec_active();
                m.observe_latency(start.elapsed().as_secs_f64());
            }

            match result {
                Ok(body) => Ok(McpResponse {
                    status: 200,
                    body,
                    sequence_number: seq_num,
                }),
                Err(err) => {
                    if let Some(m) = &self.metrics {
                        m.inc_failures();
                    }
                    Err(err)
                }
            }
        } else {
            Err(format!("Tool not found: {}", tool))
        }
    }
    pub async fn run_server(
        &mut self,
        mut rx: mpsc::Receiver<McpRequest>,
        cancel_token: CancellationToken,
    ) {
        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    info!("Shutdown signal received, shutting down gracefully...");
                    // Graceful shutdown can wait for active connections here
                    break;
                }
                req = rx.recv() => {
                    match req {
                        Some(mut request) => {
                            let response_tx = request.response_tx.take();
                            let result = self.handle_request(request).await;
                            if let Some(tx) = response_tx {
                                let _ = tx.send(result.clone());
                            }
                            match result {
                                Ok(_) => {},
                                Err(e) => error!("Error handling request: {}", e),
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
                return Err(format!(
                    "Execution error: Instruction limit exceeded ({})",
                    max_instructions
                ));
            }
            *instruction_count += 1;

            match &op.op {
                crate::ir::OpCode::RetrieveQdrant {
                    query,
                    filters,
                    top_k,
                } => {
                    // Generate real query embedding via local model server (with E5 "query: " prefixing)
                    let query_vec = db_pools.get_embedding(query, true).await;
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

                    if let Some(res) = execute_program(
                        branch_plan,
                        vm,
                        db_pools,
                        instruction_count,
                        max_instructions,
                    )
                    .await?
                    {
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
