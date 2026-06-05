use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::sync::Mutex;
use std::time::Instant;
use tokio::sync::mpsc;

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
