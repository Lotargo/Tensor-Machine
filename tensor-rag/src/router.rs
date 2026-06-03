use crate::mcp::{McpRequest, McpResponse};
use axum::{
    Router,
    extract::{Json, State, Multipart},
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
    // DB Pools
    pub db_pools: Arc<crate::db::DbPools>,
}

pub fn mcp_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/mcp", post(handle_mcp))
        .route("/upload", post(handle_upload))
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

async fn handle_upload(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    let mut file_data = Vec::new();
    let mut filename = String::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" {
            filename = field.file_name().unwrap_or("document").to_string();
            if let Ok(bytes) = field.bytes().await {
                file_data = bytes.to_vec();
            }
        }
    }

    if file_data.is_empty() {
        error!("No file data uploaded");
        return Err(axum::http::StatusCode::BAD_REQUEST);
    }

    // 1. Process with Mistral OCR API
    let mistral_key = std::env::var("MISTRAL_API_KEY")
        .map_err(|_| {
            error!("MISTRAL_API_KEY env var not set");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;

    info!("Uploading file '{}' to Mistral OCR API...", filename);
    let ocr_text = run_mistral_ocr(&filename, file_data, &mistral_key).await
        .map_err(|e| {
            error!("Mistral OCR failed: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // 2. Chunk text
    use crate::chunker::{Chunker, ParagraphChunker};
    let chunker = ParagraphChunker::default();
    let chunks = chunker.chunk(&ocr_text);

    info!("Split document into {} chunks.", chunks.len());

    // 3. Generate embeddings and save to DB for each chunk
    let mut doc_ids = Vec::new();
    for chunk in chunks {
        if chunk.trim().is_empty() {
            continue;
        }
        // Generate embedding (is_query = false)
        let vector = state.db_pools.get_embedding(&chunk, false).await;
        // Store in PostgreSQL and Qdrant
        match state.db_pools.insert_document(&chunk, vector).await {
            Ok(id) => doc_ids.push(id),
            Err(e) => {
                error!("Failed to store document chunk: {}", e);
                return Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    }

    Ok(Json(serde_json::json!({
        "status": "success",
        "ids": doc_ids,
        "filename": filename
    })))
}

async fn run_mistral_ocr(filename: &str, data: Vec<u8>, api_key: &str) -> Result<String, String> {
    let client = reqwest::Client::new();

    // Step A: Upload File using /v1/files
    let part = reqwest::multipart::Part::bytes(data)
        .file_name(filename.to_string())
        .mime_str("application/octet-stream")
        .map_err(|e| e.to_string())?;

    let form = reqwest::multipart::Form::new()
        .part("file", part)
        .text("purpose", "ocr");

    #[derive(serde::Deserialize)]
    struct FileUploadResponse {
        id: String,
    }

    let file_resp = client
        .post("https://api.mistral.ai/v1/files")
        .header("Authorization", format!("Bearer {}", api_key))
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Failed to send upload request: {}", e))?;

    let file_status = file_resp.status();
    if !file_status.is_success() {
        let err_text = file_resp.text().await.unwrap_or_default();
        return Err(format!("File upload failed ({}): {}", file_status, err_text));
    }

    let file_info = file_resp
        .json::<FileUploadResponse>()
        .await
        .map_err(|e| format!("Failed to parse file upload response: {}", e))?;

    info!("File uploaded successfully. File ID: {}", file_info.id);

    // Step B: Send OCR Request using /v1/ocr
    let ocr_payload = serde_json::json!({
        "model": "mistral-ocr-latest",
        "document": {
            "type": "file_id",
            "file_id": file_info.id
        }
    });

    #[derive(serde::Deserialize)]
    struct OcrPage {
        markdown: String,
    }
    #[derive(serde::Deserialize)]
    struct OcrResponse {
        pages: Vec<OcrPage>,
    }

    info!("Sending OCR extraction request to Mistral...");
    let ocr_resp = client
        .post("https://api.mistral.ai/v1/ocr")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&ocr_payload)
        .send()
        .await
        .map_err(|e| format!("Failed to send OCR request: {}", e))?;

    let ocr_status = ocr_resp.status();
    if !ocr_status.is_success() {
        let err_text = ocr_resp.text().await.unwrap_or_default();
        return Err(format!("OCR request failed ({}): {}", ocr_status, err_text));
    }

    let ocr_result = ocr_resp
        .json::<OcrResponse>()
        .await
        .map_err(|e| format!("Failed to parse OCR response: {}", e))?;

    let full_markdown = ocr_result
        .pages
        .iter()
        .map(|page| page.markdown.as_str())
        .collect::<Vec<&str>>()
        .join("\n\n");

    Ok(full_markdown)
}
