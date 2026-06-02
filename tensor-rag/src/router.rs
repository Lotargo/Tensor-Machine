use crate::mcp::{McpRequest, McpResponse};
use axum::{
    Router,
    extract::{Json, State},
    routing::post,
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;
use tracing::{error, info};

pub struct AppState {
    // Session sticky map: Maps session_id to the worker's Sender
    pub sessions: Arc<RwLock<HashMap<String, mpsc::Sender<McpRequest>>>>,
    // Available workers
    pub workers: Vec<mpsc::Sender<McpRequest>>,
    // Round-robin counter
    pub next_worker: std::sync::atomic::AtomicUsize,
}

pub fn mcp_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/mcp", post(handle_mcp))
        .with_state(state)
}

async fn handle_mcp(
    State(state): State<Arc<AppState>>,
    Json(request): Json<McpRequest>,
) -> Result<Json<McpResponse>, axum::http::StatusCode> {
    let session_id = request.session_id.clone();
    let sender = {
        let sessions = state.sessions.read().unwrap();
        sessions.get(&session_id).cloned()
    };

    let sender = match sender {
        Some(s) => s,
        None => {
            // Assign to a new worker in a round-robin fashion
            let workers_len = state.workers.len();
            if workers_len == 0 {
                error!("No workers available");
                return Err(axum::http::StatusCode::SERVICE_UNAVAILABLE);
            }
            let idx = state
                .next_worker
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                % workers_len;
            let new_sender = state.workers[idx].clone();

            // Store the sticky assignment
            let mut sessions = state.sessions.write().unwrap();
            sessions.insert(session_id.clone(), new_sender.clone());
            info!("Assigned new session {} to worker {}", session_id, idx);
            new_sender
        }
    };

    // Forward the request to the chosen worker
    // Notice: McpRequest only contains `session_id`, `sequence_number`, `tool`, `args`.
    // And workers (`McpServer::run_server` listening on receiver) will process it.
    // Wait, the MCP Server expects a `McpRequest` to be sent to a receiver.
    // And it doesn't send responses back over a channel in the current design of `run_server`!

    // Let me check how `run_server` replies. Wait, `handle_request` returns the `McpResponse`.
    // But `run_server` loop:
    //  req = rx.recv() => match self.handle_request(request).await ...
    // It doesn't send the response anywhere, it just drops it.

    // To make this HTTP gateway work, we need a way for the router to get the response.
    // We can modify `McpRequest` to include an optional `oneshot::Sender<McpResponse>`!
    // I need to update `McpRequest` definition in `mcp.rs`.

    // For now, I will just send it and return a dummy ok response since `McpRequest` doesn't have a response channel yet.
    // Actually, I will modify `McpRequest` to include a response channel in the next step.

    // For now, assume it has a `response_tx`.
    let (tx, rx) = tokio::sync::oneshot::channel();

    let mut req = request;
    req.response_tx = Some(tx);

    if let Err(e) = sender.send(req).await {
        error!("Failed to route request: {}", e);
        return Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    }

    match rx.await {
        Ok(Ok(response)) => Ok(Json(response)),
        Ok(Err(e)) => {
            error!("Worker returned error: {}", e);
            Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
        }
        Err(_) => {
            error!("Worker died without responding");
            Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
