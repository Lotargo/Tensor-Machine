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
        .route("/", axum::routing::get(serve_index))
        .route("/style.css", axum::routing::get(serve_css))
        .route("/app.js", axum::routing::get(serve_js))
        .route("/marked.min.js", axum::routing::get(serve_marked))
        .route("/katex.min.js", axum::routing::get(serve_katex_js))
        .route("/katex.min.css", axum::routing::get(serve_katex_css))
        .route("/mermaid.min.js", axum::routing::get(serve_mermaid))
        .route("/mammoth.browser.min.js", axum::routing::get(serve_mammoth))
        .route("/documents", axum::routing::get(handle_list_documents))
        .route("/documents/:id", axum::routing::delete(handle_delete_document))
        .route("/mcp", post(handle_mcp))
        .route("/upload", post(handle_upload))
        .route("/clear", post(handle_clear_database))
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
) -> impl axum::response::IntoResponse {
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
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "error": "Файл пуст или не был передан."
            })),
        );
    }

    // 1. Check file type.
    // If filename ends with .txt or .md (case insensitive), we can decode directly as UTF-8.
    let filename_lower = filename.to_lowercase();
    let ocr_text = if filename_lower.ends_with(".txt") || filename_lower.ends_with(".md") {
        match String::from_utf8(file_data) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to decode UTF-8 text file: {}", e);
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "status": "error",
                        "error": format!("Не удалось декодировать текстовый файл как UTF-8: {}", e)
                    })),
                );
            }
        }
    } else {
        // Run Mistral OCR
        let mistral_key = match std::env::var("MISTRAL_API_KEY") {
            Ok(key) => key,
            Err(_) => {
                error!("MISTRAL_API_KEY env var not set");
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "status": "error",
                        "error": "Ошибка конфигурации: Переменная окружения MISTRAL_API_KEY не задана."
                    })),
                );
            }
        };

        info!("Uploading file '{}' to Mistral OCR API...", filename);
        match run_mistral_ocr(&filename, file_data, &mistral_key).await {
            Ok(text) => text,
            Err(e) => {
                error!("Mistral OCR failed: {}", e);
                return (
                    axum::http::StatusCode::UNPROCESSABLE_ENTITY, // 422
                    Json(serde_json::json!({
                        "status": "error",
                        "error": format!("Mistral OCR ошибка: {}", e)
                    })),
                );
            }
        }
    };

    // 2. Chunk text
    use crate::chunker::{Chunker, TokenChunker};
    let chunker = TokenChunker::default();
    let chunks = chunker.chunk(&ocr_text);

    info!("Split document into {} chunks.", chunks.len());

    // 3. Generate embeddings and save to DB for each chunk
    let mut doc_ids = Vec::new();
    let doc_uuid = uuid::Uuid::new_v4().to_string();
    for chunk in chunks {
        if chunk.trim().is_empty() {
            continue;
        }
        // Generate embedding (is_query = false)
        let vector = state.db_pools.get_embedding(&chunk, false).await;
        // Store in PostgreSQL and Qdrant
        match state.db_pools.insert_document(&chunk, vector, &filename, &doc_uuid).await {
            Ok(id) => doc_ids.push(id),
            Err(e) => {
                error!("Failed to store document chunk: {}", e);
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "status": "error",
                        "error": format!("Не удалось сохранить сегмент документа в базу данных: {}", e)
                    })),
                );
            }
        }
    }

    (
        axum::http::StatusCode::OK,
        Json(serde_json::json!({
            "status": "success",
            "ids": doc_ids,
            "filename": filename
        })),
    )
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

async fn serve_index() -> axum::response::Html<&'static str> {
    axum::response::Html(include_str!("../static/index.html"))
}

async fn serve_css() -> impl axum::response::IntoResponse {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("text/css"),
    );
    (headers, include_str!("../static/style.css"))
}

async fn serve_js() -> impl axum::response::IntoResponse {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/javascript"),
    );
    (headers, include_str!("../static/app.js"))
}

async fn handle_list_documents(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    let pool = state
        .db_pools
        .postgres
        .as_ref()
        .ok_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let rows = sqlx::query("SELECT id, text, bm25_score, filename, doc_uuid FROM documents ORDER BY id DESC LIMIT 1000")
        .fetch_all(pool)
        .await
        .map_err(|e| {
            error!("Failed to fetch documents from database: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;

    use sqlx::Row;
    let mut docs = Vec::new();
    for row in rows {
        let id: i64 = row.try_get("id").unwrap_or(0);
        let text: String = row.try_get("text").unwrap_or_default();
        let bm25_score: f32 = row.try_get("bm25_score").unwrap_or(0.0);
        let filename: Option<String> = row.try_get("filename").ok();
        let doc_uuid: Option<String> = row.try_get("doc_uuid").ok();
        docs.push(serde_json::json!({
            "id": id,
            "text": text,
            "bm25_score": bm25_score,
            "filename": filename.unwrap_or_else(|| "Unknown".to_string()),
            "doc_uuid": doc_uuid.unwrap_or_else(|| "".to_string())
        }));
    }

    Ok(Json(serde_json::json!({
        "documents": docs
    })))
}

async fn serve_marked() -> impl axum::response::IntoResponse {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/javascript"),
    );
    (headers, include_bytes!("../static/marked.min.js").as_slice())
}

async fn serve_katex_js() -> impl axum::response::IntoResponse {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/javascript"),
    );
    (headers, include_bytes!("../static/katex.min.js").as_slice())
}

async fn serve_katex_css() -> impl axum::response::IntoResponse {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("text/css"),
    );
    (headers, include_bytes!("../static/katex.min.css").as_slice())
}

async fn serve_mermaid() -> impl axum::response::IntoResponse {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/javascript"),
    );
    (headers, include_bytes!("../static/mermaid.min.js").as_slice())
}

async fn serve_mammoth() -> impl axum::response::IntoResponse {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/javascript"),
    );
    (headers, include_bytes!("../static/mammoth.browser.min.js").as_slice())
}

async fn handle_delete_document(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<usize>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    state.db_pools.delete_document(id).await
        .map_err(|e| {
            error!("Failed to delete document: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(serde_json::json!({
        "status": "success",
        "id": id
    })))
}

async fn handle_clear_database(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    state.db_pools.clear_database().await
        .map_err(|e| {
            error!("Failed to clear database: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(serde_json::json!({
        "status": "success",
        "message": "База данных успешно очищена"
    })))
}

