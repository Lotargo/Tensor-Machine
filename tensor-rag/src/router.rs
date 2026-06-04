use crate::mcp::{McpRequest, McpResponse};
use axum::{
    Router,
    body::Body,
    extract::{Json, Multipart, Path as AxumPath, State},
    routing::post,
};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

pub struct ServerControl {
    pub cancel_token: CancellationToken,
    pub restart_requested: std::sync::atomic::AtomicBool,
}

pub struct AppState {
    pub sessions: Arc<RwLock<HashMap<String, mpsc::Sender<McpRequest>>>>,
    pub workers: Vec<mpsc::Sender<McpRequest>>,
    pub next_worker: std::sync::atomic::AtomicUsize,
    pub db_pools: Arc<crate::db::DbPools>,
    pub control: Arc<ServerControl>,
}

const AI_PROVIDER_OPTIONS: &[(&str, &str)] = &[
    ("openai", "OpenAI"),
    ("gemini", "Gemini"),
    ("openrouter", "OpenRouter"),
    ("local", "Ollama / LM Studio"),
    ("custom", "Custom OpenAI-compatible"),
];

const AI_CHAT_MODE_OPTIONS: &[(&str, &str)] = &[
    ("rag", "RAG prompt mode"),
    ("agent", "Agent mode with MCP tools"),
];

const AI_CONTEXT_STRATEGY_OPTIONS: &[(&str, &str)] = &[
    ("trim_middle", "Trim the middle"),
    ("sliding_window", "Sliding window"),
    ("stop", "Stop at limit"),
];

struct SettingDescriptor {
    key: &'static str,
    label: &'static str,
    description: &'static str,
    category: &'static str,
    field_type: &'static str,
    placeholder: &'static str,
    options: &'static [(&'static str, &'static str)],
}

const EDITABLE_SETTINGS: &[SettingDescriptor] = &[
    SettingDescriptor {
        key: "AI_PROVIDER",
        label: "AI Provider",
        description: "Select the chat provider preset. Gemini uses its official OpenAI-compatible endpoint.",
        category: "ai",
        field_type: "select",
        placeholder: "",
        options: AI_PROVIDER_OPTIONS,
    },
    SettingDescriptor {
        key: "AI_BASE_URL",
        label: "AI Base URL",
        description: "OpenAI-compatible base URL for chat completions. Leave the preset value unless you use a custom provider.",
        category: "ai",
        field_type: "text",
        placeholder: "https://api.openai.com/v1",
        options: &[],
    },
    SettingDescriptor {
        key: "AI_MODEL",
        label: "AI Model",
        description: "Chat model identifier passed to the provider.",
        category: "ai",
        field_type: "text",
        placeholder: "gpt-4.1-mini",
        options: &[],
    },
    SettingDescriptor {
        key: "AI_API_KEY",
        label: "AI API Key",
        description: "API key for the selected chat provider. Local providers may leave this blank.",
        category: "ai",
        field_type: "password",
        placeholder: "",
        options: &[],
    },
    SettingDescriptor {
        key: "AI_TEMPERATURE",
        label: "AI Temperature",
        description: "Sampling temperature for chat completions.",
        category: "ai",
        field_type: "number",
        placeholder: "0.2",
        options: &[],
    },
    SettingDescriptor {
        key: "AI_CHAT_MODE",
        label: "Chat Mode",
        description: "RAG prompt mode always injects retrieved passages when retrieval is enabled. Agent mode lets the model decide whether to call MCP tools at all.",
        category: "ai",
        field_type: "select",
        placeholder: "",
        options: AI_CHAT_MODE_OPTIONS,
    },
    SettingDescriptor {
        key: "AI_RAG_TOP_K",
        label: "RAG Top-K",
        description: "How many retrieved passages to pass into the chat context.",
        category: "ai",
        field_type: "number",
        placeholder: "5",
        options: &[],
    },
    SettingDescriptor {
        key: "AI_CONTEXT_TOKEN_LIMIT",
        label: "Chat Context Token Limit",
        description: "Maximum prompt budget in tokens for the conversation context. Uses the project tokenizer.",
        category: "ai",
        field_type: "number",
        placeholder: "16000",
        options: &[],
    },
    SettingDescriptor {
        key: "AI_CONTEXT_STRATEGY",
        label: "Overflow Strategy",
        description: "What to do when the conversation exceeds the configured context token limit.",
        category: "ai",
        field_type: "select",
        placeholder: "",
        options: AI_CONTEXT_STRATEGY_OPTIONS,
    },
    SettingDescriptor {
        key: "AI_AGENT_MAX_TOOL_ROUNDS",
        label: "Agent Tool Rounds",
        description: "Maximum number of MCP tool-calling rounds in agent mode before the model must answer.",
        category: "ai",
        field_type: "number",
        placeholder: "4",
        options: &[],
    },
    SettingDescriptor {
        key: "AI_SYSTEM_PROMPT",
        label: "AI System Prompt",
        description: "High-level instruction for the chat assistant. Used in both RAG prompt mode and agent mode.",
        category: "ai",
        field_type: "textarea",
        placeholder: "",
        options: &[],
    },
    SettingDescriptor {
        key: "MISTRAL_API_KEY",
        label: "Mistral API Key",
        description: "Credentials for OCR extraction.",
        category: "ocr",
        field_type: "password",
        placeholder: "",
        options: &[],
    },
    SettingDescriptor {
        key: "EMBEDDING_SERVER_URL",
        label: "Embedding Server URL",
        description: "Base URL for the local embedding endpoint.",
        category: "embeddings",
        field_type: "text",
        placeholder: "http://localhost:11434/v1/embeddings",
        options: &[],
    },
    SettingDescriptor {
        key: "EMBEDDING_MODEL_NAME",
        label: "Embedding Model Name",
        description: "The model identifier to pass to the embedding server.",
        category: "embeddings",
        field_type: "text",
        placeholder: "nomic-embed-text",
        options: &[],
    },
    SettingDescriptor {
        key: "EMBEDDING_QUERY_PREFIX",
        label: "Embedding Query Prefix",
        description: "Instruction prefix for query embeddings.",
        category: "embeddings",
        field_type: "text",
        placeholder: "query: ",
        options: &[],
    },
    SettingDescriptor {
        key: "EMBEDDING_PASSAGE_PREFIX",
        label: "Embedding Passage Prefix",
        description: "Instruction prefix for passage embeddings.",
        category: "embeddings",
        field_type: "text",
        placeholder: "passage: ",
        options: &[],
    },
    SettingDescriptor {
        key: "EMBEDDING_DIM",
        label: "Embedding Dimension",
        description: "Expected embedding vector size.",
        category: "embeddings",
        field_type: "number",
        placeholder: "768",
        options: &[],
    },
    SettingDescriptor {
        key: "QDRANT_COLLECTION",
        label: "Qdrant Collection",
        description: "Target Qdrant collection name.",
        category: "vector",
        field_type: "text",
        placeholder: "default",
        options: &[],
    },
    SettingDescriptor {
        key: "MCP_RATE_LIMIT",
        label: "MCP Rate Limit",
        description: "Token bucket capacity for MCP requests.",
        category: "mcp",
        field_type: "number",
        placeholder: "100",
        options: &[],
    },
    SettingDescriptor {
        key: "MCP_RATE_LIMIT_REFILL",
        label: "MCP Rate Refill",
        description: "Refill rate per second for MCP rate limiting.",
        category: "mcp",
        field_type: "number",
        placeholder: "100.0",
        options: &[],
    },
    SettingDescriptor {
        key: "MCP_SEARCH_DEFAULT_TOP_K",
        label: "Default Search Top-K",
        description: "Default number of results returned by MCP search.",
        category: "mcp",
        field_type: "number",
        placeholder: "5",
        options: &[],
    },
    SettingDescriptor {
        key: "MCP_SEARCH_MAX_TOP_K",
        label: "Maximum Search Top-K",
        description: "Upper bound for MCP search top-k.",
        category: "mcp",
        field_type: "number",
        placeholder: "20",
        options: &[],
    },
    SettingDescriptor {
        key: "MCP_SEARCH_MAX_RESPONSE_TOKENS",
        label: "Search Response Token Budget",
        description: "Soft token budget for MCP search responses.",
        category: "mcp",
        field_type: "number",
        placeholder: "10000",
        options: &[],
    },
    SettingDescriptor {
        key: "MCP_READ_DOCUMENT_MAX_TOKENS",
        label: "Read Document Token Limit",
        description: "Hard token budget for full document reads.",
        category: "mcp",
        field_type: "number",
        placeholder: "10000",
        options: &[],
    },
    SettingDescriptor {
        key: "TOKENIZER_MODEL",
        label: "Tokenizer Model",
        description: "Tokenizer used for chunking and response token accounting.",
        category: "mcp",
        field_type: "text",
        placeholder: "o200k_harmony",
        options: &[],
    },
    SettingDescriptor {
        key: "CB_FAILURE_THRESHOLD",
        label: "Circuit Breaker Threshold",
        description: "Failures before a backend circuit opens.",
        category: "system",
        field_type: "number",
        placeholder: "5",
        options: &[],
    },
    SettingDescriptor {
        key: "CB_COOLDOWN_SECS",
        label: "Circuit Breaker Cooldown",
        description: "Cooldown in seconds before half-open state.",
        category: "system",
        field_type: "number",
        placeholder: "30",
        options: &[],
    },
    SettingDescriptor {
        key: "TENSOR_VM_BACKEND",
        label: "VM Backend",
        description: "Execution backend: simd, cpu, or gpu.",
        category: "system",
        field_type: "text",
        placeholder: "simd",
        options: &[],
    },
    SettingDescriptor {
        key: "VM_MAX_INSTRUCTIONS",
        label: "VM Instruction Limit",
        description: "Watchdog limit for VM instruction count.",
        category: "system",
        field_type: "number",
        placeholder: "100000",
        options: &[],
    },
    SettingDescriptor {
        key: "TENSOR_VM_SOCKET_PATH",
        label: "VM Socket Path",
        description: "IPC socket path for the VM worker.",
        category: "system",
        field_type: "text",
        placeholder: "tensor_rag_vm.sock",
        options: &[],
    },
    SettingDescriptor {
        key: "DATABASE_URL",
        label: "PostgreSQL URL",
        description: "PostgreSQL connection string.",
        category: "database",
        field_type: "text",
        placeholder: "postgres://postgres:postgres@localhost:5432/tensor_rag",
        options: &[],
    },
    SettingDescriptor {
        key: "QDRANT_URL",
        label: "Qdrant URL",
        description: "Qdrant service URL.",
        category: "database",
        field_type: "text",
        placeholder: "http://localhost:6334",
        options: &[],
    },
];

#[derive(serde::Serialize)]
struct EditableSettingOption {
    value: String,
    label: String,
}

#[derive(serde::Serialize)]
struct EditableSetting {
    key: String,
    label: String,
    description: String,
    value: String,
    category: String,
    field_type: String,
    placeholder: String,
    options: Vec<EditableSettingOption>,
}

#[derive(serde::Deserialize)]
struct SettingsUpdateRequest {
    values: HashMap<String, String>,
}

pub fn mcp_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", axum::routing::get(serve_index))
        .route("/favicon.ico", axum::routing::get(serve_favicon))
        .route("/style.css", axum::routing::get(serve_css))
        .route("/app.js", axum::routing::get(serve_js))
        .route("/app/*path", axum::routing::get(serve_app_asset))
        .route("/assets/*path", axum::routing::get(serve_static_asset))
        .route("/marked.min.js", axum::routing::get(serve_marked))
        .route("/katex.min.js", axum::routing::get(serve_katex_js))
        .route("/katex.min.css", axum::routing::get(serve_katex_css))
        .route("/mermaid.min.js", axum::routing::get(serve_mermaid))
        .route("/mammoth.browser.min.js", axum::routing::get(serve_mammoth))
        .route("/documents", axum::routing::get(handle_list_documents))
        .route(
            "/documents/:id",
            axum::routing::delete(handle_delete_document),
        )
        .route(
            "/settings",
            axum::routing::get(handle_get_settings).post(handle_update_settings),
        )
        .route("/settings/restart", post(handle_restart_server))
        .route("/hard-reset", post(handle_hard_reset_database))
        .route("/reindex", post(handle_reindex_database))
        .route("/chat", post(handle_chat))
        .route("/chat/stream", post(handle_chat_stream))
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

fn env_file_path() -> PathBuf {
    dotenvy::dotenv().unwrap_or_else(|_| {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".env")
    })
}

fn read_env_file_map() -> HashMap<String, String> {
    let path = env_file_path();
    let content = std::fs::read_to_string(path).unwrap_or_default();
    let mut values = HashMap::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            values.insert(key.trim().to_string(), value.to_string());
        }
    }

    values
}

fn write_env_updates(updates: &HashMap<String, String>) -> Result<(), String> {
    let path = env_file_path();
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut seen = HashSet::new();
    let mut output = Vec::new();

    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || !trimmed.contains('=') {
            output.push(line.to_string());
            continue;
        }

        if let Some((key, _)) = trimmed.split_once('=') {
            let key = key.trim();
            if let Some(new_value) = updates.get(key) {
                output.push(format!("{}={}", key, new_value));
                seen.insert(key.to_string());
            } else {
                output.push(line.to_string());
            }
        }
    }

    for (key, value) in updates {
        if !seen.contains(key) {
            output.push(format!("{}={}", key, value));
        }
    }

    std::fs::write(path, output.join("\n")).map_err(|e| format!("Failed to update .env: {}", e))
}

async fn handle_get_settings(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    let env_values = read_env_file_map();
    let settings = EDITABLE_SETTINGS
        .iter()
        .map(|setting| EditableSetting {
            key: setting.key.to_string(),
            label: setting.label.to_string(),
            description: setting.description.to_string(),
            value: env_values
                .get(setting.key)
                .cloned()
                .unwrap_or_else(|| std::env::var(setting.key).unwrap_or_default()),
            category: setting.category.to_string(),
            field_type: setting.field_type.to_string(),
            placeholder: setting.placeholder.to_string(),
            options: setting
                .options
                .iter()
                .map(|(value, label)| EditableSettingOption {
                    value: (*value).to_string(),
                    label: (*label).to_string(),
                })
                .collect(),
        })
        .collect::<Vec<_>>();

    let collection_health = state.db_pools.get_collection_health().await;

    Ok(Json(serde_json::json!({
        "settings": settings,
        "collection_health": collection_health
    })))
}

async fn handle_update_settings(
    Json(request): Json<SettingsUpdateRequest>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    let allowed_keys = EDITABLE_SETTINGS
        .iter()
        .map(|setting| setting.key)
        .collect::<HashSet<_>>();

    let filtered = request
        .values
        .into_iter()
        .filter(|(key, _)| allowed_keys.contains(key.as_str()))
        .collect::<HashMap<_, _>>();

    write_env_updates(&filtered).map_err(|e| {
        error!("{}", e);
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(serde_json::json!({
        "status": "success",
        "message": "Settings saved to .env. Restart the server to apply changes."
    })))
}

async fn handle_restart_server(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    state
        .control
        .restart_requested
        .store(true, std::sync::atomic::Ordering::SeqCst);
    state.control.cancel_token.cancel();

    Ok(Json(serde_json::json!({
        "status": "success",
        "message": "Restart initiated."
    })))
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
                "error": "The uploaded file is empty or missing."
            })),
        );
    }

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
                        "error": format!("Failed to decode the text file as UTF-8: {}", e)
                    })),
                );
            }
        }
    } else {
        let mistral_key = match std::env::var("MISTRAL_API_KEY") {
            Ok(key) => key,
            Err(_) => {
                error!("MISTRAL_API_KEY env var not set");
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "status": "error",
                        "error": "Configuration error: MISTRAL_API_KEY is not set."
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
                    axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                    Json(serde_json::json!({
                        "status": "error",
                        "error": format!("Mistral OCR error: {}", e)
                    })),
                );
            }
        }
    };

    use crate::chunker::HierarchicalChunker;
    let chunker = HierarchicalChunker::default();
    let chunks = chunker.chunk(&ocr_text, &filename);

    info!("Split document into {} chunks.", chunks.len());

    let mut doc_ids = Vec::new();
    let doc_uuid = uuid::Uuid::new_v4().to_string();
    for item in chunks {
        let vector = match state
            .db_pools
            .get_embedding(&item.embedding_text, false)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                error!("Failed to generate vector embedding: {}", e);
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "status": "error",
                        "error": format!("Failed to generate vector embedding: {}. Please check that the embedding server is running and configured correctly.", e)
                    })),
                );
            }
        };
        match state
            .db_pools
            .insert_document_enriched(
                &item.text,
                vector,
                &filename,
                &doc_uuid,
                &item.chapter_id,
                item.chunk_index,
                item.level.as_str(),
                item.parent_chunk_index,
                item.macro_index,
                item.medium_index,
                &item.header_chain,
                &item.zone_keywords,
                &item.entities,
            )
            .await
        {
            Ok(id) => doc_ids.push(id),
            Err(e) => {
                error!("Failed to store document chunk: {}", e);
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "status": "error",
                        "error": format!("Failed to store a document chunk in the database: {}", e)
                    })),
                );
            }
        }
    }

    // Auto-extract anchors from headings in the OCR text
    let anchor_count = state
        .db_pools
        .auto_extract_anchors(&doc_uuid, &ocr_text)
        .await
        .map(|ids| ids.len())
        .unwrap_or(0);

    (
        axum::http::StatusCode::OK,
        Json(serde_json::json!({
            "status": "success",
            "ids": doc_ids,
            "filename": filename,
            "doc_uuid": doc_uuid,
            "anchors_extracted": anchor_count
        })),
    )
}

async fn run_mistral_ocr(filename: &str, data: Vec<u8>, api_key: &str) -> Result<String, String> {
    let client = reqwest::Client::new();

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
        return Err(format!(
            "File upload failed ({}): {}",
            file_status, err_text
        ));
    }

    let file_info = file_resp
        .json::<FileUploadResponse>()
        .await
        .map_err(|e| format!("Failed to parse file upload response: {}", e))?;

    info!("File uploaded successfully. File ID: {}", file_info.id);

    let ocr_payload = serde_json::json!({
        "model": "mistral-ocr-latest",
        "document": {
            "type": "file",
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

    Ok(ocr_result
        .pages
        .iter()
        .map(|page| page.markdown.as_str())
        .collect::<Vec<&str>>()
        .join("\n\n"))
}

async fn serve_index() -> axum::response::Html<&'static str> {
    axum::response::Html(include_str!("../static/index.html"))
}

async fn serve_css() -> impl axum::response::IntoResponse {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("text/css; charset=utf-8"),
    );
    (headers, include_str!("../static/style.css"))
}

async fn serve_js() -> impl axum::response::IntoResponse {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/javascript; charset=utf-8"),
    );
    (headers, include_str!("../static/app.js"))
}

async fn serve_app_asset(
    AxumPath(path): AxumPath<String>,
) -> Result<impl axum::response::IntoResponse, axum::http::StatusCode> {
    serve_static_file("app", path).await
}

async fn serve_static_asset(
    AxumPath(path): AxumPath<String>,
) -> Result<impl axum::response::IntoResponse, axum::http::StatusCode> {
    serve_static_file("assets", path).await
}

async fn serve_static_file(
    root: &str,
    path: String,
) -> Result<impl axum::response::IntoResponse, axum::http::StatusCode> {
    let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("static")
        .join(root)
        .canonicalize()
        .map_err(|_| axum::http::StatusCode::NOT_FOUND)?;
    let sanitized = path.trim_start_matches(['/', '\\']);
    let candidate = base_dir.join(sanitized);
    let resolved = candidate
        .canonicalize()
        .map_err(|_| axum::http::StatusCode::NOT_FOUND)?;

    if !resolved.starts_with(&base_dir) || !resolved.is_file() {
        return Err(axum::http::StatusCode::NOT_FOUND);
    }

    let content = std::fs::read(&resolved).map_err(|_| axum::http::StatusCode::NOT_FOUND)?;
    let content_type = match resolved
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
    {
        "js" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "text/plain; charset=utf-8",
    };

    Ok(([(axum::http::header::CONTENT_TYPE, content_type)], content))
}

async fn serve_favicon() -> Result<impl axum::response::IntoResponse, axum::http::StatusCode> {
    serve_static_file("assets/icons", "chat.png".to_string()).await
}

async fn handle_list_documents(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    let pool = state
        .db_pools
        .postgres
        .as_ref()
        .ok_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let rows = sqlx::query(
        "SELECT id, text, bm25_score, filename, doc_uuid FROM documents ORDER BY id DESC LIMIT 1000",
    )
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
            "doc_uuid": doc_uuid.unwrap_or_default()
        }));
    }

    let collection_health = state.db_pools.get_collection_health().await;
    let warning = if collection_health.exists && !collection_health.dimension_matches {
        Some(format!(
            "Qdrant collection '{}' is configured for {} dimensions, but the server expects {}. Search and indexing may fail until a hard reset is performed.",
            collection_health.collection_name,
            collection_health.actual_dim.unwrap_or_default(),
            collection_health.configured_dim
        ))
    } else {
        None
    };

    Ok(Json(serde_json::json!({
        "documents": docs,
        "collection_health": collection_health,
        "warning": warning
    })))
}

async fn serve_marked() -> impl axum::response::IntoResponse {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/javascript"),
    );
    (
        headers,
        include_bytes!("../static/marked.min.js").as_slice(),
    )
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
    (
        headers,
        include_bytes!("../static/katex.min.css").as_slice(),
    )
}

async fn serve_mermaid() -> impl axum::response::IntoResponse {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/javascript"),
    );
    (
        headers,
        include_bytes!("../static/mermaid.min.js").as_slice(),
    )
}

async fn serve_mammoth() -> impl axum::response::IntoResponse {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/javascript"),
    );
    (
        headers,
        include_bytes!("../static/mammoth.browser.min.js").as_slice(),
    )
}

async fn handle_delete_document(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<usize>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    state.db_pools.delete_document(id).await.map_err(|e| {
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
    state.db_pools.clear_documents().await.map_err(|e| {
        error!("Failed to clear database: {}", e);
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(serde_json::json!({
        "status": "success",
        "message": "All documents were removed. The current collection schema was preserved."
    })))
}

async fn handle_hard_reset_database(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    state.db_pools.hard_reset_database().await.map_err(|e| {
        error!("Failed to perform hard reset: {}", e);
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(serde_json::json!({
        "status": "success",
        "message": "Hard reset complete. PostgreSQL data was cleared and the Qdrant collection was recreated."
    })))
}

async fn handle_reindex_database(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    state.db_pools.reindex_database().await.map_err(|e| {
        error!("Failed to reindex database: {}", e);
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(serde_json::json!({
        "status": "success",
        "message": "Reindexing complete. Qdrant collection was recreated and all existing document chunks were re-embedded."
    })))
}

async fn handle_chat(
    State(state): State<Arc<AppState>>,
    Json(request): Json<crate::ai::ChatRequest>,
) -> Result<impl axum::response::IntoResponse, axum::http::StatusCode> {
    let settings = read_env_file_map();
    match crate::ai::run_chat(state.db_pools.clone(), &settings, request).await {
        Ok(response) => Ok((
            axum::http::StatusCode::OK,
            Json(
                serde_json::to_value(response)
                    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?,
            ),
        )),
        Err(err) => {
            error!("Chat request failed: {}", err);
            let status = if err.contains("PERMISSION_DENIED") || err.contains("denied access") {
                axum::http::StatusCode::FORBIDDEN
            } else if err.contains("API key") || err.contains("Unauthorized") || err.contains("401")
            {
                axum::http::StatusCode::UNAUTHORIZED
            } else {
                axum::http::StatusCode::BAD_GATEWAY
            };

            Ok((
                status,
                Json(serde_json::json!({
                    "error": err
                })),
            ))
        }
    }
}

async fn handle_chat_stream(
    State(state): State<Arc<AppState>>,
    Json(request): Json<crate::ai::ChatRequest>,
) -> Result<impl axum::response::IntoResponse, axum::http::StatusCode> {
    let settings = read_env_file_map();
    let db_pools = state.db_pools.clone();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<bytes::Bytes, std::convert::Infallible>>(64);

    tokio::spawn(async move {
        let (sse_tx, mut sse_rx) = tokio::sync::mpsc::channel::<String>(64);

        let worker = tokio::spawn(async move {
            crate::ai::stream_chat(db_pools, &settings, request, sse_tx).await
        });

        while let Some(frame) = sse_rx.recv().await {
            if tx.send(Ok(bytes::Bytes::from(frame))).await.is_err() {
                return;
            }
        }

        if let Ok(Err(err)) = worker.await {
            let status = if err.contains("PERMISSION_DENIED") || err.contains("denied access") {
                "forbidden"
            } else if err.contains("API key") || err.contains("Unauthorized") || err.contains("401")
            {
                "unauthorized"
            } else {
                "error"
            };
            let frame = format!(
                "event: error\ndata: {}\n\n",
                serde_json::json!({
                    "status": status,
                    "message": err
                })
            );
            let _ = tx.send(Ok(bytes::Bytes::from(frame))).await;
        }
    });

    Ok((
        [
            (
                axum::http::header::CONTENT_TYPE,
                "text/event-stream; charset=utf-8",
            ),
            (axum::http::header::CACHE_CONTROL, "no-cache"),
            (axum::http::header::CONNECTION, "keep-alive"),
        ],
        Body::from_stream(tokio_stream::wrappers::ReceiverStream::new(rx)),
    ))
}
