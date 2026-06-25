use crate::mcp::{McpRequest, McpResponse};
use axum::{
    Router,
    body::Body,
    extract::{DefaultBodyLimit, Json, Multipart, Path as AxumPath, State},
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
};
use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::error::Error as StdError;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tokio_stream::{StreamExt, wrappers::BroadcastStream};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

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

#[derive(Clone, serde::Serialize)]
struct UploadProgressEvent {
    step: String,
    status: String,
    message: String,
    current: Option<usize>,
    total: Option<usize>,
}

static UPLOAD_PROGRESS: once_cell::sync::Lazy<
    RwLock<HashMap<String, broadcast::Sender<UploadProgressEvent>>>,
> = once_cell::sync::Lazy::new(|| RwLock::new(HashMap::new()));
static UPLOAD_CANCEL: once_cell::sync::Lazy<RwLock<HashMap<String, CancellationToken>>> =
    once_cell::sync::Lazy::new(|| RwLock::new(HashMap::new()));

#[derive(Clone, serde::Serialize)]
struct MaintenanceProgressEvent {
    job_id: String,
    step: String,
    status: String,
    message: String,
    current: Option<usize>,
    total: Option<usize>,
    checked: usize,
    reclustered: usize,
    updated_rows: usize,
    warnings: Vec<String>,
}

static MAINTENANCE_PROGRESS: once_cell::sync::Lazy<
    RwLock<HashMap<String, broadcast::Sender<MaintenanceProgressEvent>>>,
> = once_cell::sync::Lazy::new(|| RwLock::new(HashMap::new()));
static MAINTENANCE_CANCEL: once_cell::sync::Lazy<RwLock<HashMap<String, CancellationToken>>> =
    once_cell::sync::Lazy::new(|| RwLock::new(HashMap::new()));

const AI_PROVIDER_OPTIONS: &[(&str, &str)] = &[
    ("openai", "OpenAI"),
    ("gemini", "Gemini"),
    ("openrouter", "OpenRouter"),
    ("zen", "Zen"),
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
        key: "RSF_DENSE_WEIGHT",
        label: "RSF Vector (Dense) Weight",
        description: "Weight for vector semantic matching (0.0 to 1.0).",
        category: "rsf",
        field_type: "number",
        placeholder: "0.30",
        options: &[],
    },
    SettingDescriptor {
        key: "RSF_BM25_WEIGHT",
        label: "RSF Keyword (BM25) Weight",
        description: "Weight for keyword matching (0.0 to 1.0).",
        category: "rsf",
        field_type: "number",
        placeholder: "0.35",
        options: &[],
    },
    SettingDescriptor {
        key: "RSF_ENTITY_WEIGHT",
        label: "RSF Entity Weight",
        description: "Weight for extracted entity boosting (0.0 to 1.0).",
        category: "rsf",
        field_type: "number",
        placeholder: "0.10",
        options: &[],
    },
    SettingDescriptor {
        key: "RSF_STRUCTURAL_WEIGHT",
        label: "RSF Structural Weight",
        description: "Weight for document structural headers context (0.0 to 1.0).",
        category: "rsf",
        field_type: "number",
        placeholder: "0.10",
        options: &[],
    },
    SettingDescriptor {
        key: "RSF_RECENCY_WEIGHT",
        label: "RSF Recency Weight",
        description: "Weight for document recency bias (0.0 to 1.0).",
        category: "rsf",
        field_type: "number",
        placeholder: "0.05",
        options: &[],
    },
    SettingDescriptor {
        key: "RSF_CLUSTER_WEIGHT",
        label: "RSF Cluster (Density) Weight",
        description: "Weight for dense cluster semantic bonus (0.0 to 1.0).",
        category: "rsf",
        field_type: "number",
        placeholder: "0.10",
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
        description: "Maximum number of MCP tool-calling rounds in agent mode before the model must answer. Higher values help complex and multi-hop searches.",
        category: "ai",
        field_type: "number",
        placeholder: "8",
        options: &[],
    },
    SettingDescriptor {
        key: "AI_REQUEST_TIMEOUT_SECS",
        label: "AI Request Timeout",
        description: "Timeout for AI provider chat completion requests.",
        category: "ai",
        field_type: "number",
        placeholder: "120",
        options: &[],
    },
    SettingDescriptor {
        key: "AI_RETRY_ATTEMPTS",
        label: "AI Retry Attempts",
        description: "Retry attempts for transient AI provider network errors and 429/5xx responses.",
        category: "ai",
        field_type: "number",
        placeholder: "3",
        options: &[],
    },
    SettingDescriptor {
        key: "AI_RETRY_DELAY_MS",
        label: "AI Retry Delay",
        description: "Delay between AI provider retry attempts in milliseconds.",
        category: "ai",
        field_type: "number",
        placeholder: "1000",
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
        key: "MISTRAL_BASE_URL",
        label: "Mistral Base URL",
        description: "Base URL for Mistral API calls.",
        category: "ocr",
        field_type: "text",
        placeholder: "https://api.mistral.ai/v1",
        options: &[],
    },
    SettingDescriptor {
        key: "MISTRAL_UPLOAD_TIMEOUT_SECS",
        label: "Mistral Upload Timeout",
        description: "Timeout for uploading source files to Mistral.",
        category: "ocr",
        field_type: "number",
        placeholder: "240",
        options: &[],
    },
    SettingDescriptor {
        key: "MISTRAL_RETRY_ATTEMPTS",
        label: "Mistral Retry Attempts",
        description: "Number of attempts for transient Mistral network requests.",
        category: "ocr",
        field_type: "number",
        placeholder: "3",
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
        placeholder: "jeffh/intfloat-multilingual-e5-large-instruct:q8_0",
        options: &[],
    },
    SettingDescriptor {
        key: "EMBEDDING_BATCH_SIZE",
        label: "Embedding Batch Size",
        description: "How many chunks to send in one embedding request during ingestion.",
        category: "embeddings",
        field_type: "number",
        placeholder: "32",
        options: &[],
    },
    SettingDescriptor {
        key: "EMBEDDING_QUERY_PREFIX",
        label: "Embedding Query Prefix",
        description: "Instruction prefix for query embeddings.",
        category: "embeddings",
        field_type: "text",
        placeholder: "Instruct: Given a question about a local document library, retrieve passages that contain the answer, including paraphrases and nearby narrative context.\\nQuery: {}",
        options: &[],
    },
    SettingDescriptor {
        key: "EMBEDDING_PASSAGE_PREFIX",
        label: "Embedding Passage Prefix",
        description: "Instruction prefix for passage embeddings.",
        category: "embeddings",
        field_type: "text",
        placeholder: "",
        options: &[],
    },
    SettingDescriptor {
        key: "EMBEDDING_DIM",
        label: "Embedding Dimension",
        description: "Expected embedding vector size.",
        category: "embeddings",
        field_type: "number",
        placeholder: "1024",
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
        key: "QDRANT_UPSERT_WAIT",
        label: "Qdrant Upsert Wait",
        description: "Whether ingestion waits for Qdrant writes to be applied before continuing.",
        category: "vector",
        field_type: "text",
        placeholder: "false",
        options: &[],
    },
    SettingDescriptor {
        key: "EXACT_SEMANTIC_LAYOUT_LIMIT",
        label: "Semantic Layout Limit",
        description: "Maximum chunks to run exact UMAP/HDBSCAN layout before falling back to coarse prefix projection.",
        category: "vector",
        field_type: "number",
        placeholder: "2500",
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
        placeholder: "5000",
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
        .route("/vendor/*path", axum::routing::get(serve_vendor_asset))
        .route("/marked.min.js", axum::routing::get(serve_marked))
        .route("/katex.min.js", axum::routing::get(serve_katex_js))
        .route("/katex.min.css", axum::routing::get(serve_katex_css))
        .route("/mermaid.min.js", axum::routing::get(serve_mermaid))
        .route("/mammoth.browser.min.js", axum::routing::get(serve_mammoth))
        .route("/documents", axum::routing::get(handle_list_documents))
        .route(
            "/documents/path",
            axum::routing::get(handle_find_shortest_path),
        )
        .route(
            "/documents/auto-link",
            axum::routing::post(handle_auto_link_documents),
        )
        .route(
            "/documents/:doc_uuid/graph",
            axum::routing::get(handle_get_document_graph),
        )
        .route(
            "/documents/:doc_uuid/details",
            axum::routing::get(handle_get_document_details),
        )
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
        .route("/maintenance/optimize", post(handle_start_library_optimization))
        .route(
            "/maintenance/progress/:job_id",
            get(handle_library_optimization_progress),
        )
        .route(
            "/maintenance/cancel/:job_id",
            post(handle_cancel_library_optimization),
        )
        .route("/chat", post(handle_chat))
        .route("/chat/stream", post(handle_chat_stream))
        .route("/mcp", post(handle_mcp))
        .route("/upload/progress/:upload_id", get(handle_upload_progress))
        .route("/upload/cancel/:upload_id", post(handle_cancel_upload))
        .route(
            "/upload",
            post(handle_upload).layer(DefaultBodyLimit::max(crate::config::get_upload_max_bytes())),
        )
        .route("/zen/models", get(handle_zen_models))
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

fn json_error(
    status: axum::http::StatusCode,
    message: impl Into<String>,
) -> (axum::http::StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({
            "status": "error",
            "error": message.into()
        })),
    )
}

fn status_for_db_error(message: &str) -> axum::http::StatusCode {
    if message.contains("PostgreSQL pool not initialized") {
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    } else {
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    }
}

fn env_file_path() -> PathBuf {
    let mut dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    loop {
        let candidate = dir.join(".env");
        if candidate.exists() {
            return candidate;
        }
        if !dir.pop() {
            break;
        }
    }

    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".env")
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

async fn handle_zen_models() -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let resp = client
        .get("https://opencode.ai/zen/v1/models")
        .send()
        .await
        .map_err(|_| axum::http::StatusCode::BAD_GATEWAY)?;

    let payload: serde_json::Value = resp
        .json()
        .await
        .map_err(|_| axum::http::StatusCode::BAD_GATEWAY)?;

    Ok(Json(payload))
}

fn upload_progress_channel(upload_id: &str) -> broadcast::Sender<UploadProgressEvent> {
    {
        let channels = UPLOAD_PROGRESS.read().unwrap();
        if let Some(sender) = channels.get(upload_id) {
            return sender.clone();
        }
    }

    let (sender, _) = broadcast::channel(64);
    let mut channels = UPLOAD_PROGRESS.write().unwrap();
    channels
        .entry(upload_id.to_string())
        .or_insert_with(|| sender.clone())
        .clone()
}

fn emit_upload_progress(
    upload_id: Option<&str>,
    step: &str,
    status: &str,
    message: impl Into<String>,
    current: Option<usize>,
    total: Option<usize>,
) {
    let Some(upload_id) = upload_id.filter(|id| !id.is_empty()) else {
        return;
    };
    let event = UploadProgressEvent {
        step: step.to_string(),
        status: status.to_string(),
        message: message.into(),
        current,
        total,
    };
    let _ = upload_progress_channel(upload_id).send(event);
}

fn close_upload_progress(upload_id: Option<&str>) {
    let Some(upload_id) = upload_id.filter(|id| !id.is_empty()) else {
        return;
    };
    UPLOAD_PROGRESS.write().unwrap().remove(upload_id);
    UPLOAD_CANCEL.write().unwrap().remove(upload_id);
}

fn upload_cancel_token(upload_id: Option<&str>) -> Option<CancellationToken> {
    let upload_id = upload_id.filter(|id| !id.is_empty())?;
    let mut tokens = UPLOAD_CANCEL.write().unwrap();
    Some(
        tokens
            .entry(upload_id.to_string())
            .or_insert_with(CancellationToken::new)
            .clone(),
    )
}

fn is_upload_cancelled(cancel_token: Option<&CancellationToken>) -> bool {
    cancel_token
        .map(CancellationToken::is_cancelled)
        .unwrap_or(false)
}

async fn handle_cancel_upload(
    AxumPath(upload_id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let token = {
        let tokens = UPLOAD_CANCEL.read().unwrap();
        tokens.get(&upload_id).cloned()
    };

    let Some(token) = token else {
        return Err(json_error(
            axum::http::StatusCode::NOT_FOUND,
            "Upload is not active or is already finished.",
        ));
    };

    token.cancel();
    Ok(Json(serde_json::json!({
        "status": "cancel_requested"
    })))
}

async fn cancel_upload_response(
    state: &Arc<AppState>,
    upload_id: Option<&str>,
    doc_ids: &[usize],
) -> (axum::http::StatusCode, Json<serde_json::Value>) {
    if let Some(first_id) = doc_ids.first().copied() {
        if let Err(error) = state.db_pools.delete_document(first_id).await {
            error!("Failed to clean up cancelled upload data: {}", error);
            emit_upload_progress(
                upload_id,
                "step-upload",
                "failed",
                format!("Upload cancelled, but cleanup failed: {}", error),
                None,
                None,
            );
            close_upload_progress(upload_id);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "status": "error",
                    "error": format!("Upload cancelled, but cleanup failed: {}", error)
                })),
            );
        }
    }

    close_upload_progress(upload_id);
    (
        axum::http::StatusCode::from_u16(499)
            .unwrap_or(axum::http::StatusCode::BAD_REQUEST),
        Json(serde_json::json!({
            "status": "cancelled",
            "error": "Upload cancelled. Partial document data was removed."
        })),
    )
}

async fn handle_upload_progress(
    AxumPath(upload_id): AxumPath<String>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let receiver = upload_progress_channel(&upload_id).subscribe();
    let stream = BroadcastStream::new(receiver).filter_map(|message| match message {
        Ok(progress) => {
            let data = serde_json::to_string(&progress).unwrap_or_else(|_| "{}".to_string());
            Some(Ok(Event::default().event("upload-progress").data(data)))
        }
        Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(skipped)) => {
            Some(Ok(Event::default()
                .event("upload-lagged")
                .data(serde_json::json!({ "skipped": skipped }).to_string())))
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[derive(serde::Deserialize)]
struct UploadQuery {
    upload_id: Option<String>,
}

async fn handle_upload(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(query): axum::extract::Query<UploadQuery>,
    mut multipart: Multipart,
) -> impl axum::response::IntoResponse {
    let upload_id = query.upload_id.as_deref();
    let cancel_token = upload_cancel_token(upload_id);
    let mut file_data = Vec::new();
    let mut filename = String::new();

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(error) => {
                error!("Failed to read multipart upload: {}", error);
                emit_upload_progress(
                    upload_id,
                    "step-upload",
                    "failed",
                    format!("Failed to read upload: {}", error),
                    None,
                    None,
                );
                close_upload_progress(upload_id);
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "status": "error",
                        "error": format!("Failed to read uploaded file: {}", error)
                    })),
                );
            }
        };
        let name = field.name().unwrap_or("").to_string();
        if name == "file" {
            filename = field.file_name().unwrap_or("document").to_string();
            let bytes = match field.bytes().await {
                Ok(bytes) => bytes,
                Err(error) => {
                    error!("Failed to read uploaded file bytes: {}", error);
                    emit_upload_progress(
                        upload_id,
                        "step-upload",
                        "failed",
                        format!("Failed to read file bytes: {}", error),
                        None,
                        None,
                    );
                    close_upload_progress(upload_id);
                    return (
                        axum::http::StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({
                            "status": "error",
                            "error": format!("Failed to read uploaded file bytes: {}", error)
                        })),
                    );
                }
            };
            file_data = bytes.to_vec();
        }
    }

    if is_upload_cancelled(cancel_token.as_ref()) {
        return cancel_upload_response(&state, upload_id, &[]).await;
    }

    if file_data.is_empty() {
        error!("No file data uploaded");
        emit_upload_progress(
            upload_id,
            "step-upload",
            "failed",
            "The uploaded file is empty or missing.",
            None,
            None,
        );
        close_upload_progress(upload_id);
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "error": "The uploaded file is empty or missing."
            })),
        );
    }
    emit_upload_progress(
        upload_id,
        "step-upload",
        "completed",
        format!("Received '{}'.", filename),
        Some(file_data.len()),
        None,
    );

    if is_upload_cancelled(cancel_token.as_ref()) {
        return cancel_upload_response(&state, upload_id, &[]).await;
    }

    let filename_lower = filename.to_lowercase();
    let ocr_text = if filename_lower.ends_with(".txt") || filename_lower.ends_with(".md") {
        emit_upload_progress(
            upload_id,
            "step-ocr",
            "completed",
            "OCR skipped for plain text input.",
            None,
            None,
        );
        match String::from_utf8(file_data) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to decode UTF-8 text file: {}", e);
                emit_upload_progress(
                    upload_id,
                    "step-ocr",
                    "failed",
                    format!("Failed to decode text file: {}", e),
                    None,
                    None,
                );
                close_upload_progress(upload_id);
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
                emit_upload_progress(
                    upload_id,
                    "step-ocr",
                    "failed",
                    "MISTRAL_API_KEY is not set.",
                    None,
                    None,
                );
                close_upload_progress(upload_id);
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
        emit_upload_progress(
            upload_id,
            "step-ocr",
            "active",
            "Running Mistral OCR.",
            None,
            None,
        );
        match run_mistral_ocr(&filename, file_data, &mistral_key).await {
            Ok(text) => {
                emit_upload_progress(
                    upload_id,
                    "step-ocr",
                    "completed",
                    format!("OCR extracted {} characters.", text.len()),
                    Some(text.len()),
                    None,
                );
                text
            }
            Err(e) => {
                error!("Mistral OCR failed: {}", e);
                emit_upload_progress(
                    upload_id,
                    "step-ocr",
                    "failed",
                    format!("Mistral OCR error: {}", e),
                    None,
                    None,
                );
                close_upload_progress(upload_id);
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

    if is_upload_cancelled(cancel_token.as_ref()) {
        return cancel_upload_response(&state, upload_id, &[]).await;
    }

    info!(
        "OCR phase complete for '{}'. Extracted {} characters. Next phase uses tokenizer '{}' for hierarchical chunking.",
        filename,
        ocr_text.len(),
        crate::config::get_tokenizer_model()
    );

    use crate::chunker::HierarchicalChunker;
    emit_upload_progress(
        upload_id,
        "step-chunk",
        "active",
        "Tokenizing and chunking document.",
        None,
        None,
    );
    let chunker = HierarchicalChunker::default();
    let chunks = chunker.chunk(&ocr_text, &filename);
    emit_upload_progress(
        upload_id,
        "step-chunk",
        "completed",
        format!("Created {} chunks.", chunks.len()),
        Some(chunks.len()),
        None,
    );

    info!("Split document into {} chunks.", chunks.len());

    let mut doc_ids = Vec::new();
    let mut semantic_vectors: Vec<(i32, Vec<f32>)> = Vec::new();
    let doc_uuid = uuid::Uuid::new_v4().to_string();
    let total_chunks = chunks.len();
    let embedding_batch_size = crate::config::get_embedding_batch_size();
    info!(
        "Starting batched embedding for '{}' with batch_size={} across {} chunks.",
        filename, embedding_batch_size, total_chunks
    );
    let total_batches = total_chunks.div_ceil(embedding_batch_size);
    for (batch_idx, batch) in chunks.chunks(embedding_batch_size).enumerate() {
        if is_upload_cancelled(cancel_token.as_ref()) {
            return cancel_upload_response(&state, upload_id, &doc_ids).await;
        }

        let batch_start = batch_idx * embedding_batch_size;
        let batch_end = batch_start + batch.len();
        emit_upload_progress(
            upload_id,
            "step-embedding",
            "active",
            format!("Embedding batch {}/{}.", batch_idx + 1, total_batches),
            Some(batch_idx + 1),
            Some(total_batches),
        );
        info!(
            "Embedding batch {}/{} for '{}' covering chunks {}..{}",
            batch_idx + 1,
            total_batches,
            filename,
            batch_start + 1,
            batch_end
        );

        let embedding_inputs: Vec<String> = batch
            .iter()
            .map(|item| item.embedding_text.clone())
            .collect();
        let vectors = match state
            .db_pools
            .get_embeddings(&embedding_inputs, false)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                error!("Failed to generate batched vector embeddings: {}", e);
                emit_upload_progress(
                    upload_id,
                    "step-embedding",
                    "failed",
                    format!("Failed to generate vector embeddings: {}", e),
                    Some(batch_idx + 1),
                    Some(total_batches),
                );
                close_upload_progress(upload_id);
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "status": "error",
                        "error": format!("Failed to generate vector embeddings: {}. Please check that the embedding server is running and configured correctly.", e)
                    })),
                );
            }
        };

        if is_upload_cancelled(cancel_token.as_ref()) {
            return cancel_upload_response(&state, upload_id, &doc_ids).await;
        }

        semantic_vectors.extend(
            batch
                .iter()
                .zip(vectors.iter())
                .map(|(item, vector)| (item.chunk_index, vector.to_vec())),
        );

        let uploaded_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let documents: Vec<crate::db::EnrichedDocumentInsert<'_>> = batch
            .iter()
            .zip(vectors.into_iter())
            .map(|(item, vector)| {
                let word_count = item.text.split_whitespace().count();
                let token_count = crate::chunker::count_tokens(&item.text);
                let metadata = serde_json::json!({
                    "uploaded_at": uploaded_at,
                    "word_count": word_count,
                    "token_count": token_count,
                });
                crate::db::EnrichedDocumentInsert {
                    text: &item.text,
                    vector,
                    filename: &filename,
                    doc_uuid: &doc_uuid,
                    chapter_id: &item.chapter_id,
                    chunk_index: item.chunk_index,
                    chunk_level: item.level.as_str(),
                    parent_chunk_index: item.parent_chunk_index,
                    macro_index: item.macro_index,
                    medium_index: item.medium_index,
                    header_chain: &item.header_chain,
                    zone_keywords: &item.zone_keywords,
                    entities: &item.entities,
                    metadata,
                }
            })
            .collect();

        info!(
            "Storing chunk batch {}/{} for '{}' ({} chunks) into PostgreSQL and Qdrant.",
            batch_idx + 1,
            total_batches,
            filename,
            documents.len()
        );
        emit_upload_progress(
            upload_id,
            "step-db",
            "active",
            format!("Storing batch {}/{}.", batch_idx + 1, total_batches),
            Some(batch_idx + 1),
            Some(total_batches),
        );
        match state
            .db_pools
            .insert_documents_enriched_batch(documents)
            .await
        {
            Ok(ids) => {
                info!(
                    "Stored chunk batch {}/{} for '{}' as {} document rows.",
                    batch_idx + 1,
                    total_batches,
                    filename,
                    ids.len()
                );
                doc_ids.extend(ids);
            }
            Err(e) => {
                error!("Failed to store document chunk batch: {}", e);
                emit_upload_progress(
                    upload_id,
                    "step-db",
                    "failed",
                    format!("Failed to store document chunk batch: {}", e),
                    Some(batch_idx + 1),
                    Some(total_batches),
                );
                close_upload_progress(upload_id);
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "status": "error",
                        "error": format!("Failed to store a document chunk batch in the database: {}", e)
                    })),
                );
            }
        }

        if is_upload_cancelled(cancel_token.as_ref()) {
            return cancel_upload_response(&state, upload_id, &doc_ids).await;
        }
    }
    emit_upload_progress(
        upload_id,
        "step-embedding",
        "completed",
        format!("Generated embeddings for {} chunks.", total_chunks),
        Some(total_batches),
        Some(total_batches),
    );
    emit_upload_progress(
        upload_id,
        "step-db",
        "completed",
        format!("Stored {} chunks.", doc_ids.len()),
        Some(total_batches),
        Some(total_batches),
    );

    // --- Entity extraction, deduplication, embedding, and storage ---
    emit_upload_progress(
        upload_id,
        "step-entities",
        "active",
        "Extracting and embedding named entities.",
        None,
        None,
    );
    if is_upload_cancelled(cancel_token.as_ref()) {
        return cancel_upload_response(&state, upload_id, &doc_ids).await;
    }

    let chunk_entities_list: Vec<Vec<String>> = chunks.iter().map(|c| c.entities.clone()).collect();
    let unique_entity_texts = crate::db::entities::collect_unique_entities(&chunk_entities_list);

    if !unique_entity_texts.is_empty() {
        match state.db_pools.upsert_entities(&doc_uuid, &unique_entity_texts).await {
            Ok(entity_map) => {
                // Link chunks to entities
                for (chunk_idx, chunk) in chunks.iter().enumerate() {
                    if chunk_idx >= doc_ids.len() {
                        break;
                    }
                    let chunk_id = doc_ids[chunk_idx] as i64;
                    let entity_ids: Vec<i64> = chunk
                        .entities
                        .iter()
                        .filter_map(|text| entity_map.get(text).copied())
                        .collect();
                    if !entity_ids.is_empty() {
                        let _ = state.db_pools.link_chunk_entities(chunk_id, &entity_ids).await;
                    }
                }

                // Embed entities and store in Qdrant
                let entities_with_ids: Vec<(i64, String)> = entity_map
                    .iter()
                    .map(|(text, &id)| (id, text.clone()))
                    .collect();
                if let Err(e) = state
                    .db_pools
                    .embed_and_store_entities(&doc_uuid, &entities_with_ids)
                    .await
                {
                    error!("Failed to embed entities: {}", e);
                }

                // Cluster entities based on chunk cluster assignments
                let mut entity_chunks_map: std::collections::HashMap<String, Vec<(i32, f32)>> =
                    std::collections::HashMap::new();
                for point in &semantic_vectors {
                    let chunk_idx = point.0;
                    if (chunk_idx as usize) < chunks.len() {
                        for entity_text in &chunks[chunk_idx as usize].entities {
                            entity_chunks_map
                                .entry(entity_text.clone())
                                .or_default()
                                .push((chunk_idx, 0.0));
                        }
                    }
                }

                emit_upload_progress(
                    upload_id,
                    "step-entities",
                    "completed",
                    format!("Processed {} unique entities.", unique_entity_texts.len()),
                    None,
                    None,
                );
            }
            Err(e) => {
                error!("Failed to store entities: {}", e);
                emit_upload_progress(
                    upload_id,
                    "step-entities",
                    "failed",
                    format!("Failed to store entities: {}", e),
                    None,
                    None,
                );
            }
        }
    } else {
        emit_upload_progress(
            upload_id,
            "step-entities",
            "completed",
            "No entities found.",
            None,
            None,
        );
    }

    emit_upload_progress(
        upload_id,
        "step-clustering",
        "active",
        "Building semantic cluster layout.",
        None,
        None,
    );
    if semantic_vectors.is_empty() {
        emit_upload_progress(
            upload_id,
            "step-clustering",
            "completed",
            "No embeddings available for semantic clustering.",
            None,
            None,
        );
    } else {
        if is_upload_cancelled(cancel_token.as_ref()) {
            return cancel_upload_response(&state, upload_id, &doc_ids).await;
        }

        let layout = crate::clustering::build_semantic_cluster_layout(
            &semantic_vectors,
            Some(Default::default()),
        );
        match state
            .db_pools
            .update_document_semantic_layout(&doc_uuid, &layout)
            .await
        {
            Ok(()) => {
                emit_upload_progress(
                    upload_id,
                    "step-clustering",
                    "completed",
                    format!(
                        "Clustered {} chunks into {} semantic clusters.",
                        layout.points.len(),
                        layout.clusters.len()
                    ),
                    Some(layout.points.len()),
                    Some(layout.points.len()),
                );
            }
            Err(e) => {
                error!("Failed to store semantic cluster layout: {}", e);
                emit_upload_progress(
                    upload_id,
                    "step-clustering",
                    "failed",
                    format!("Failed to store semantic cluster layout: {}", e),
                    None,
                    None,
                );
                close_upload_progress(upload_id);
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "status": "error",
                        "error": format!("Failed to store semantic cluster layout: {}", e)
                    })),
                );
            }
        }
    }

    if is_upload_cancelled(cancel_token.as_ref()) {
        return cancel_upload_response(&state, upload_id, &doc_ids).await;
    }

    // Auto-extract anchors from headings in the OCR text
    info!("Starting anchor extraction for '{}'.", filename);
    let anchor_count = state
        .db_pools
        .auto_extract_anchors(&doc_uuid, &ocr_text)
        .await
        .map(|ids| ids.len())
        .unwrap_or(0);

    if is_upload_cancelled(cancel_token.as_ref()) {
        return cancel_upload_response(&state, upload_id, &doc_ids).await;
    }

    info!(
        "Upload pipeline complete for '{}'. Stored {} chunks and {} anchors.",
        filename,
        doc_ids.len(),
        anchor_count
    );
    close_upload_progress(upload_id);

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
    let connect_timeout = Duration::from_secs(crate::config::get_http_connect_timeout_secs());
    let upload_timeout = Duration::from_secs(crate::config::get_mistral_upload_timeout_secs());
    let ocr_timeout = Duration::from_secs(crate::config::get_mistral_ocr_timeout_secs());
    let retry_attempts = crate::config::get_mistral_retry_attempts();
    let retry_delay = Duration::from_millis(crate::config::get_mistral_retry_delay_ms());
    let base_url = crate::config::get_mistral_base_url();
    let files_url = format!("{}/files", base_url);
    let ocr_url = format!("{}/ocr", base_url);

    let upload_client = reqwest::Client::builder()
        .connect_timeout(connect_timeout)
        .timeout(upload_timeout)
        .build()
        .map_err(|e| format!("Failed to build Mistral upload client: {}", e))?;
    let ocr_client = reqwest::Client::builder()
        .connect_timeout(connect_timeout)
        .timeout(ocr_timeout)
        .build()
        .map_err(|e| format!("Failed to build Mistral OCR client: {}", e))?;

    #[derive(serde::Deserialize)]
    struct FileUploadResponse {
        id: String,
    }

    info!(
        "Sending file '{}' ({} bytes) to Mistral /files with timeout={}s, attempts={}",
        filename,
        data.len(),
        upload_timeout.as_secs(),
        retry_attempts
    );
    let file_resp = send_mistral_file_upload(
        &upload_client,
        &files_url,
        filename,
        &data,
        api_key,
        retry_attempts,
        retry_delay,
    )
    .await?;

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

    info!(
        "Sending OCR extraction request to Mistral for file_id={} with timeout={}s, attempts={}",
        file_info.id,
        ocr_timeout.as_secs(),
        retry_attempts
    );
    let ocr_resp = send_mistral_json_request(
        &ocr_client,
        &ocr_url,
        api_key,
        &ocr_payload,
        retry_attempts,
        retry_delay,
    )
    .await?;

    let ocr_status = ocr_resp.status();
    if !ocr_status.is_success() {
        let err_text = ocr_resp.text().await.unwrap_or_default();
        return Err(format!("OCR request failed ({}): {}", ocr_status, err_text));
    }

    let ocr_result = ocr_resp
        .json::<OcrResponse>()
        .await
        .map_err(|e| format!("Failed to parse OCR response: {}", e))?;

    info!(
        "Mistral OCR completed for '{}' with {} pages.",
        filename,
        ocr_result.pages.len()
    );

    Ok(ocr_result
        .pages
        .iter()
        .map(|page| page.markdown.as_str())
        .collect::<Vec<&str>>()
        .join("\n\n"))
}

async fn send_mistral_file_upload(
    client: &reqwest::Client,
    url: &str,
    filename: &str,
    data: &[u8],
    api_key: &str,
    attempts: usize,
    retry_delay: Duration,
) -> Result<reqwest::Response, String> {
    let mut last_error = None;
    for attempt in 1..=attempts {
        let part = reqwest::multipart::Part::bytes(data.to_vec())
            .file_name(filename.to_string())
            .mime_str("application/octet-stream")
            .map_err(|e| e.to_string())?;
        let form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("purpose", "ocr");

        match client
            .post(url)
            .header("Authorization", format!("Bearer {}", api_key))
            .multipart(form)
            .send()
            .await
        {
            Ok(response) => return Ok(response),
            Err(error) => {
                let detailed = describe_reqwest_error(&error);
                warn!(
                    attempt,
                    attempts, "Mistral file upload request failed: {}", detailed
                );
                last_error = Some(detailed);
                if attempt < attempts {
                    tokio::time::sleep(retry_delay).await;
                }
            }
        }
    }

    Err(format!(
        "Failed to send upload request after {} attempt(s): {}",
        attempts,
        last_error.unwrap_or_else(|| "unknown error".to_string())
    ))
}

async fn send_mistral_json_request(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    payload: &serde_json::Value,
    attempts: usize,
    retry_delay: Duration,
) -> Result<reqwest::Response, String> {
    let mut last_error = None;
    for attempt in 1..=attempts {
        match client
            .post(url)
            .header("Authorization", format!("Bearer {}", api_key))
            .json(payload)
            .send()
            .await
        {
            Ok(response) => return Ok(response),
            Err(error) => {
                let detailed = describe_reqwest_error(&error);
                warn!(
                    attempt,
                    attempts, "Mistral OCR request failed: {}", detailed
                );
                last_error = Some(detailed);
                if attempt < attempts {
                    tokio::time::sleep(retry_delay).await;
                }
            }
        }
    }

    Err(format!(
        "Failed to send OCR request after {} attempt(s): {}",
        attempts,
        last_error.unwrap_or_else(|| "unknown error".to_string())
    ))
}

fn describe_reqwest_error(error: &reqwest::Error) -> String {
    let mut details = vec![error.to_string()];
    if error.is_timeout() {
        details.push("kind=timeout".to_string());
    }
    if error.is_connect() {
        details.push("kind=connect".to_string());
    }
    if error.is_request() {
        details.push("kind=request".to_string());
    }
    if let Some(url) = error.url() {
        details.push(format!("url={}", url));
    }

    let mut source = error.source();
    while let Some(cause) = source {
        details.push(format!("caused by: {}", cause));
        source = cause.source();
    }

    details.join("; ")
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

async fn serve_vendor_asset(
    AxumPath(path): AxumPath<String>,
) -> Result<impl axum::response::IntoResponse, axum::http::StatusCode> {
    serve_static_file("vendor", path).await
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

#[derive(serde::Deserialize)]
struct GraphQuery {
    depth: Option<u32>,
}

async fn handle_get_document_graph(
    State(state): State<Arc<AppState>>,
    AxumPath(doc_uuid): AxumPath<String>,
    axum::extract::Query(query): axum::extract::Query<GraphQuery>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let Some(pool) = state.db_pools.postgres.as_ref() else {
        return Err(json_error(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "PostgreSQL is not connected. Check DATABASE_URL and restart the server.",
        ));
    };

    let depth = query.depth.unwrap_or(2);
    let graph_data = crate::graph::get_document_neighborhood(pool, &doc_uuid, depth)
        .await
        .map_err(|e| {
            error!("Failed to get document graph: {}", e);
            json_error(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get document graph: {}", e),
            )
        })?;

    Ok(Json(graph_data))
}

#[derive(serde::Deserialize)]
struct DocumentDetailsQuery {
    semantic: Option<bool>,
}

async fn handle_get_document_details(
    State(state): State<Arc<AppState>>,
    AxumPath(doc_uuid): AxumPath<String>,
    axum::extract::Query(query): axum::extract::Query<DocumentDetailsQuery>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let Some(pool) = state.db_pools.postgres.as_ref() else {
        return Err(json_error(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "PostgreSQL is not connected. Check DATABASE_URL and restart the server.",
        ));
    };

    use sqlx::Row;

    // 1. Fetch chunks
    let chunk_rows = sqlx::query(
        "SELECT id, text, chapter_id, chunk_index, chunk_level, parent_chunk_index, macro_index, medium_index,
                header_chain, zone_keywords, entities, cluster_id, cluster_distance, layout_hint,
                clustering_algorithm, clustering_projection
         FROM documents
         WHERE doc_uuid = $1
         ORDER BY chunk_index ASC",
    )
    .bind(&doc_uuid)
    .fetch_all(pool)
    .await
    .map_err(|e| {
        error!("Failed to fetch document chunks: {}", e);
        json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to fetch document chunks: {}", e),
        )
    })?;

    let mut chunks = Vec::new();
    let mut semantic_points = Vec::new();
    let mut clustering_algorithm: Option<String> = None;
    let mut clustering_projection: Option<String> = None;
    for r in chunk_rows {
        let id: i64 = r.try_get("id").unwrap_or(0);
        let text: String = r.try_get("text").unwrap_or_default();
        let chapter_id: String = r.try_get("chapter_id").unwrap_or_default();
        let chunk_index: i32 = r.try_get("chunk_index").unwrap_or(0);
        let chunk_level: String = r.try_get("chunk_level").unwrap_or_default();
        let parent_chunk_index: Option<i32> = r.try_get("parent_chunk_index").ok();
        let macro_index: Option<i32> = r.try_get("macro_index").ok();
        let medium_index: Option<i32> = r.try_get("medium_index").ok();
        let header_chain: serde_json::Value =
            r.try_get("header_chain").unwrap_or(serde_json::json!([]));
        let zone_keywords: serde_json::Value =
            r.try_get("zone_keywords").unwrap_or(serde_json::json!([]));
        let entities: serde_json::Value = r.try_get("entities").unwrap_or(serde_json::json!([]));
        let cluster_id: Option<i32> = r.try_get("cluster_id").ok();
        let cluster_distance: Option<f32> = r.try_get("cluster_distance").ok();
        let layout_hint: Option<serde_json::Value> = r.try_get("layout_hint").ok();
        let row_algorithm: Option<String> = r.try_get("clustering_algorithm").ok();
        let row_projection: Option<String> = r.try_get("clustering_projection").ok();
        if clustering_algorithm.is_none() {
            clustering_algorithm = row_algorithm.clone();
        }
        if clustering_projection.is_none() {
            clustering_projection = row_projection.clone();
        }

        if let (Some(cluster_id), Some(distance), Some(layout_value)) =
            (cluster_id, cluster_distance, layout_hint.clone())
        {
            if let Ok(layout_hint) = serde_json::from_value::<[f32; 3]>(layout_value) {
                semantic_points.push(crate::clustering::SemanticClusterPoint {
                    chunk_index,
                    cluster_id,
                    distance_to_centroid: distance,
                    layout_hint,
                });
            }
        }

        chunks.push(serde_json::json!({
            "id": id,
            "text": text,
            "chapter_id": chapter_id,
            "chunk_index": chunk_index,
            "chunk_level": chunk_level,
            "parent_chunk_index": parent_chunk_index,
            "macro_index": macro_index,
            "medium_index": medium_index,
            "header_chain": header_chain,
            "zone_keywords": zone_keywords,
            "entities": entities,
            "cluster_id": cluster_id,
            "cluster_distance": cluster_distance,
            "layout_hint": layout_hint
        }));
    }

    let include_semantic_clusters = query.semantic.unwrap_or(false);
    let semantic_clusters = if include_semantic_clusters && !semantic_points.is_empty() {
        let clusters = crate::clustering::summarize_semantic_clusters(&semantic_points);
        serde_json::json!({
            "algorithm": clustering_algorithm.unwrap_or_else(|| "hdbscan".to_string()),
            "projection": clustering_projection.unwrap_or_else(|| "umap-3d".to_string()),
            "points": semantic_points,
            "clusters": clusters
        })
    } else {
        serde_json::json!(null)
    };

    // 2. Fetch anchors
    let anchor_rows =
        sqlx::query("SELECT name, text FROM anchors WHERE doc_uuid = $1 ORDER BY id ASC")
            .bind(&doc_uuid)
            .fetch_all(pool)
            .await
            .map_err(|e| {
                error!("Failed to fetch anchors: {}", e);
                json_error(
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to fetch anchors: {}", e),
                )
            })?;

    let anchors: Vec<serde_json::Value> = anchor_rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "name": r.try_get::<String, _>("name").unwrap_or_default(),
                "text": r.try_get::<String, _>("text").unwrap_or_default()
            })
        })
        .collect();

    // 3. Fetch edges
    let edge_rows = sqlx::query(
        "SELECT id, source_doc_uuid, target_doc_uuid, edge_type, metadata FROM edges WHERE source_doc_uuid = $1 OR target_doc_uuid = $1 ORDER BY id ASC"
    )
    .bind(&doc_uuid)
    .fetch_all(pool)
    .await
    .map_err(|e| {
        error!("Failed to fetch edges: {}", e);
        json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to fetch edges: {}", e),
        )
    })?;

    let edges: Vec<serde_json::Value> = edge_rows.iter().map(|r| {
        serde_json::json!({
            "id": r.try_get::<i64, _>("id").unwrap_or(0),
            "source": r.try_get::<String, _>("source_doc_uuid").unwrap_or_default(),
            "target": r.try_get::<String, _>("target_doc_uuid").unwrap_or_default(),
            "type": r.try_get::<String, _>("edge_type").unwrap_or_default(),
            "metadata": r.try_get::<serde_json::Value, _>("metadata").unwrap_or(serde_json::json!({}))
        })
    }).collect();

    Ok(Json(serde_json::json!({
        "doc_uuid": doc_uuid,
        "chunks": chunks,
        "anchors": anchors,
        "edges": edges,
        "semantic_clusters_loaded": include_semantic_clusters,
        "semantic_clusters": semantic_clusters
    })))
}

#[derive(serde::Deserialize)]
struct PathQuery {
    start: String,
    end: String,
}

async fn handle_find_shortest_path(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(query): axum::extract::Query<PathQuery>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let Some(pool) = state.db_pools.postgres.as_ref() else {
        return Err(json_error(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "PostgreSQL is not connected. Check DATABASE_URL and restart the server.",
        ));
    };

    let path = crate::graph::find_shortest_path(pool, &query.start, &query.end)
        .await
        .map_err(|e| {
            error!("Pathfinding failed: {}", e);
            json_error(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Pathfinding failed: {}", e),
            )
        })?;

    Ok(Json(serde_json::json!({
        "path": path
    })))
}

#[derive(serde::Deserialize)]
struct AutoLinkQuery {
    threshold: Option<usize>,
}

async fn handle_auto_link_documents(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(query): axum::extract::Query<AutoLinkQuery>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let Some(pool) = state.db_pools.postgres.as_ref() else {
        return Err(json_error(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "PostgreSQL is not connected. Check DATABASE_URL and restart the server.",
        ));
    };

    let threshold = query.threshold.unwrap_or(3);
    let count = crate::graph::auto_link_by_shared_entities(pool, threshold)
        .await
        .map_err(|e| {
            error!("Auto-linking failed: {}", e);
            json_error(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Auto-linking failed: {}", e),
            )
        })?;

    Ok(Json(serde_json::json!({
        "status": "success",
        "edges_created": count
    })))
}

async fn handle_list_documents(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    let Some(pool) = state.db_pools.postgres.as_ref() else {
        let collection_health = state.db_pools.get_collection_health().await;
        return Ok(Json(serde_json::json!({
            "documents": [],
            "collection_health": collection_health,
            "warning": "PostgreSQL is not connected. Check DATABASE_URL and restart the server."
        })));
    };

    let rows = sqlx::query(
        r#"
        SELECT
            doc_uuid,
            MIN(id) AS id,
            MAX(filename) AS filename,
            COUNT(*) AS chunk_count,
            CASE
                WHEN COUNT(*) FILTER (WHERE chunk_level = 'macro') > 0
                    THEN COALESCE(SUM(char_length(text)) FILTER (WHERE chunk_level = 'macro'), 0)
                WHEN COUNT(*) FILTER (WHERE chunk_level = 'medium') > 0
                    THEN COALESCE(SUM(char_length(text)) FILTER (WHERE chunk_level = 'medium'), 0)
                WHEN COUNT(*) FILTER (WHERE chunk_level = 'micro') > 0
                    THEN COALESCE(SUM(char_length(text)) FILTER (WHERE chunk_level = 'micro'), 0)
                ELSE COALESCE(SUM(char_length(text)), 0)
            END AS char_count,
            COALESCE(
                (ARRAY_AGG(text ORDER BY chunk_index ASC) FILTER (WHERE chunk_level = 'macro'))[1],
                (ARRAY_AGG(text ORDER BY chunk_index ASC) FILTER (WHERE chunk_level = 'medium'))[1],
                (ARRAY_AGG(text ORDER BY chunk_index ASC) FILTER (WHERE chunk_level = 'micro'))[1],
                (ARRAY_AGG(text ORDER BY chunk_index ASC))[1],
                ''
            ) AS preview_text,
            MAX(id) AS last_id
        FROM documents
        GROUP BY doc_uuid
        ORDER BY last_id DESC
        "#,
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
        let char_count: i64 = row.try_get("char_count").unwrap_or(0);
        let chunk_count: i64 = row.try_get("chunk_count").unwrap_or(0);
        let preview_text: String = row.try_get("preview_text").unwrap_or_default();
        let filename: Option<String> = row.try_get("filename").ok();
        let doc_uuid: Option<String> = row.try_get("doc_uuid").ok();
        docs.push(serde_json::json!({
            "id": id,
            "preview_text": preview_text,
            "char_count": char_count,
            "chunk_count": chunk_count,
            "approx_tokens": ((char_count as f64) / 4.0).round() as i64,
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
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    state.db_pools.delete_document(id).await.map_err(|e| {
        error!("Failed to delete document: {}", e);
        json_error(status_for_db_error(&e), e)
    })?;

    Ok(Json(serde_json::json!({
        "status": "success",
        "id": id
    })))
}

async fn handle_clear_database(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    state.db_pools.clear_documents().await.map_err(|e| {
        error!("Failed to clear database: {}", e);
        json_error(status_for_db_error(&e), e)
    })?;

    Ok(Json(serde_json::json!({
        "status": "success",
        "message": "All documents were removed. The current collection schema was preserved."
    })))
}

async fn handle_hard_reset_database(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    state.db_pools.hard_reset_database().await.map_err(|e| {
        error!("Failed to perform hard reset: {}", e);
        json_error(status_for_db_error(&e), e)
    })?;

    Ok(Json(serde_json::json!({
        "status": "success",
        "message": "Hard reset complete. PostgreSQL data was cleared and the Qdrant collection was recreated."
    })))
}

async fn handle_reindex_database(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    state.db_pools.reindex_database().await.map_err(|e| {
        error!("Failed to reindex database: {}", e);
        json_error(status_for_db_error(&e), e)
    })?;

    Ok(Json(serde_json::json!({
        "status": "success",
        "message": "Reindexing complete. Qdrant collection was recreated and all existing document chunks were re-embedded."
    })))
}

fn maintenance_progress_channel(job_id: &str) -> broadcast::Sender<MaintenanceProgressEvent> {
    {
        let channels = MAINTENANCE_PROGRESS.read().unwrap();
        if let Some(sender) = channels.get(job_id) {
            return sender.clone();
        }
    }

    let (sender, _) = broadcast::channel(128);
    let mut channels = MAINTENANCE_PROGRESS.write().unwrap();
    channels
        .entry(job_id.to_string())
        .or_insert_with(|| sender.clone())
        .clone()
}

#[allow(clippy::too_many_arguments)]
fn emit_maintenance_progress(
    job_id: &str,
    step: &str,
    status: &str,
    message: impl Into<String>,
    current: Option<usize>,
    total: Option<usize>,
    checked: usize,
    reclustered: usize,
    updated_rows: usize,
    warnings: &[String],
) {
    let event = MaintenanceProgressEvent {
        job_id: job_id.to_string(),
        step: step.to_string(),
        status: status.to_string(),
        message: message.into(),
        current,
        total,
        checked,
        reclustered,
        updated_rows,
        warnings: warnings.to_vec(),
    };
    let _ = maintenance_progress_channel(job_id).send(event);
}

async fn handle_start_library_optimization(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let job_id = uuid::Uuid::new_v4().to_string();
    let cancel_token = CancellationToken::new();
    MAINTENANCE_CANCEL
        .write()
        .unwrap()
        .insert(job_id.clone(), cancel_token.clone());
    let _ = maintenance_progress_channel(&job_id);

    let state_for_job = state.clone();
    let job_id_for_task = job_id.clone();
    tokio::spawn(async move {
        run_library_optimization_job(state_for_job, job_id_for_task, cancel_token).await;
    });

    Ok(Json(serde_json::json!({
        "status": "started",
        "job_id": job_id
    })))
}

async fn handle_library_optimization_progress(
    AxumPath(job_id): AxumPath<String>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let receiver = maintenance_progress_channel(&job_id).subscribe();
    let stream = BroadcastStream::new(receiver).filter_map(|message| match message {
        Ok(progress) => {
            let data = serde_json::to_string(&progress).unwrap_or_else(|_| "{}".to_string());
            Some(Ok(Event::default().event("maintenance-progress").data(data)))
        }
        Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(skipped)) => {
            Some(Ok(Event::default()
                .event("maintenance-lagged")
                .data(serde_json::json!({ "skipped": skipped }).to_string())))
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn handle_cancel_library_optimization(
    AxumPath(job_id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let token = {
        let tokens = MAINTENANCE_CANCEL.read().unwrap();
        tokens.get(&job_id).cloned()
    };

    let Some(token) = token else {
        return Err(json_error(
            axum::http::StatusCode::NOT_FOUND,
            "Maintenance job is not active or is already finished.",
        ));
    };

    token.cancel();
    Ok(Json(serde_json::json!({
        "status": "cancel_requested"
    })))
}

async fn run_library_optimization_job(
    state: Arc<AppState>,
    job_id: String,
    cancel_token: CancellationToken,
) {
    use sqlx::Row;

    let mut checked = 0usize;
    let mut reclustered = 0usize;
    let mut updated_rows = 0usize;
    let mut warnings = Vec::<String>::new();

    let Some(pool) = state.db_pools.postgres.as_ref() else {
        warnings.push("PostgreSQL is not connected.".to_string());
        emit_maintenance_progress(
            &job_id,
            "check",
            "failed",
            "PostgreSQL is not connected.",
            None,
            None,
            checked,
            reclustered,
            updated_rows,
            &warnings,
        );
        MAINTENANCE_CANCEL.write().unwrap().remove(&job_id);
        return;
    };

    emit_maintenance_progress(
        &job_id,
        "check",
        "active",
        "Scanning indexed documents.",
        None,
        None,
        checked,
        reclustered,
        updated_rows,
        &warnings,
    );

    let docs = match sqlx::query(
        r#"
        SELECT doc_uuid,
               COALESCE(MAX(filename), 'Document') AS filename,
               COUNT(*)::bigint AS chunk_count,
               COUNT(*) FILTER (WHERE layout_hint IS NULL OR cluster_id IS NULL)::bigint AS missing_layout
        FROM documents
        WHERE COALESCE(doc_uuid, '') <> ''
        GROUP BY doc_uuid
        ORDER BY MAX(filename), doc_uuid
        "#,
    )
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows,
        Err(error) => {
            warnings.push(format!("Failed to scan documents: {}", error));
            emit_maintenance_progress(
                &job_id,
                "check",
                "failed",
                "Failed to scan documents.",
                None,
                None,
                checked,
                reclustered,
                updated_rows,
                &warnings,
            );
            MAINTENANCE_CANCEL.write().unwrap().remove(&job_id);
            return;
        }
    };

    let total = docs.len();
    for (idx, row) in docs.into_iter().enumerate() {
        if cancel_token.is_cancelled() {
            emit_maintenance_progress(
                &job_id,
                "cancelled",
                "failed",
                "Optimization cancelled by user.",
                Some(idx),
                Some(total),
                checked,
                reclustered,
                updated_rows,
                &warnings,
            );
            MAINTENANCE_CANCEL.write().unwrap().remove(&job_id);
            return;
        }

        let doc_uuid: String = row.try_get("doc_uuid").unwrap_or_default();
        let filename: String = row.try_get("filename").unwrap_or_else(|_| doc_uuid.clone());
        let chunk_count: i64 = row.try_get("chunk_count").unwrap_or(0);
        let missing_layout: i64 = row.try_get("missing_layout").unwrap_or(0);
        checked += 1;

        emit_maintenance_progress(
            &job_id,
            "cluster",
            "active",
            format!("Optimizing {}", filename),
            Some(idx + 1),
            Some(total),
            checked,
            reclustered,
            updated_rows,
            &warnings,
        );

        let id_rows = match sqlx::query(
            "SELECT id, chunk_index FROM documents WHERE doc_uuid = $1 ORDER BY chunk_index ASC",
        )
        .bind(&doc_uuid)
        .fetch_all(pool)
        .await
        {
            Ok(rows) => rows,
            Err(error) => {
                warnings.push(format!("{}: failed to read chunk map: {}", filename, error));
                continue;
            }
        };
        let id_to_chunk = id_rows
            .into_iter()
            .filter_map(|row| {
                let id: i64 = row.try_get("id").ok()?;
                let chunk_index: i32 = row.try_get("chunk_index").ok()?;
                Some((id as usize, chunk_index))
            })
            .collect::<HashMap<_, _>>();

        let vectors = match state
            .db_pools
            .fetch_document_chunk_vectors(&doc_uuid, chunk_count.max(1) as usize)
            .await
        {
            Ok(vectors) => vectors,
            Err(error) => {
                warnings.push(format!("{}: failed to fetch Qdrant vectors: {}", filename, error));
                continue;
            }
        };

        if vectors.is_empty() {
            warnings.push(format!("{}: no vectors found in Qdrant.", filename));
            continue;
        }

        let indexed_vectors = vectors
            .into_iter()
            .filter_map(|(id, vector)| id_to_chunk.get(&id).copied().map(|idx| (idx, vector)))
            .collect::<Vec<_>>();

        if indexed_vectors.is_empty() {
            warnings.push(format!("{}: vectors did not match PostgreSQL chunk ids.", filename));
            continue;
        }

        if missing_layout == 0 && indexed_vectors.len() == chunk_count as usize {
            // Recompute anyway: this is a manual optimization/defragmentation action.
        }

        let layout =
            crate::clustering::build_semantic_cluster_layout(&indexed_vectors, Some(Default::default()));
        match state
            .db_pools
            .update_document_semantic_layout(&doc_uuid, &layout)
            .await
        {
            Ok(()) => {
                reclustered += 1;
                updated_rows += layout.points.len();
            }
            Err(error) => warnings.push(format!("{}: failed to save layout: {}", filename, error)),
        }
    }

    if let Err(error) = sqlx::query("ANALYZE documents").execute(pool).await {
        warnings.push(format!("PostgreSQL ANALYZE failed: {}", error));
    }

    emit_maintenance_progress(
        &job_id,
        "complete",
        "completed",
        "Library optimization complete.",
        Some(total),
        Some(total),
        checked,
        reclustered,
        updated_rows,
        &warnings,
    );
    MAINTENANCE_CANCEL.write().unwrap().remove(&job_id);
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
