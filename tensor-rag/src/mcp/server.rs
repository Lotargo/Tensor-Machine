use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use super::types::{McpRequest, McpResponse, McpSession, RateLimiter};
use super::tools::ToolHandler;

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
            rate_limiter: Arc::new(RateLimiter::new(
                crate::config::get_mcp_rate_limit(),
                crate::config::get_mcp_rate_limit_refill(),
            )),
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
