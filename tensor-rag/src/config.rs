use std::sync::OnceLock;

fn get_usize_env(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn get_u32_env(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn get_u64_env(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn get_bool_env(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .and_then(|s| match s.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
        .unwrap_or(default)
}

fn get_f64_env(key: &str, default: f64) -> f64 {
    std::env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

pub fn get_embedding_dim() -> usize {
    static EMBEDDING_DIM: OnceLock<usize> = OnceLock::new();
    *EMBEDDING_DIM.get_or_init(|| get_usize_env("EMBEDDING_DIM", 1024))
}

pub fn get_qdrant_collection() -> String {
    static QDRANT_COLLECTION: OnceLock<String> = OnceLock::new();
    QDRANT_COLLECTION
        .get_or_init(|| {
            std::env::var("QDRANT_COLLECTION").unwrap_or_else(|_| "default".to_string())
        })
        .clone()
}

pub fn get_mcp_rate_limit() -> u32 {
    static MCP_RATE_LIMIT: OnceLock<u32> = OnceLock::new();
    *MCP_RATE_LIMIT.get_or_init(|| get_u32_env("MCP_RATE_LIMIT", 100))
}

pub fn get_mcp_rate_limit_refill() -> f64 {
    static MCP_RATE_LIMIT_REFILL: OnceLock<f64> = OnceLock::new();
    *MCP_RATE_LIMIT_REFILL.get_or_init(|| get_f64_env("MCP_RATE_LIMIT_REFILL", 100.0))
}

pub fn get_cb_failure_threshold() -> u32 {
    static CB_FAILURE_THRESHOLD: OnceLock<u32> = OnceLock::new();
    *CB_FAILURE_THRESHOLD.get_or_init(|| get_u32_env("CB_FAILURE_THRESHOLD", 5))
}

pub fn get_cb_cooldown_secs() -> u64 {
    static CB_COOLDOWN_SECS: OnceLock<u64> = OnceLock::new();
    *CB_COOLDOWN_SECS.get_or_init(|| get_u64_env("CB_COOLDOWN_SECS", 30))
}

pub fn get_vm_backend() -> String {
    static VM_BACKEND: OnceLock<String> = OnceLock::new();
    VM_BACKEND
        .get_or_init(|| std::env::var("TENSOR_VM_BACKEND").unwrap_or_else(|_| "simd".to_string()))
        .clone()
}

pub fn get_vm_max_instructions() -> usize {
    static VM_MAX_INSTRUCTIONS: OnceLock<usize> = OnceLock::new();
    *VM_MAX_INSTRUCTIONS.get_or_init(|| get_usize_env("VM_MAX_INSTRUCTIONS", 100_000))
}

pub fn get_vm_socket_path() -> String {
    static VM_SOCKET_PATH: OnceLock<String> = OnceLock::new();
    VM_SOCKET_PATH
        .get_or_init(|| {
            std::env::var("TENSOR_VM_SOCKET_PATH")
                .unwrap_or_else(|_| "tensor_rag_vm.sock".to_string())
        })
        .clone()
}

pub fn get_search_default_top_k() -> usize {
    static SEARCH_DEFAULT_TOP_K: OnceLock<usize> = OnceLock::new();
    *SEARCH_DEFAULT_TOP_K.get_or_init(|| get_usize_env("MCP_SEARCH_DEFAULT_TOP_K", 5).max(1))
}

pub fn get_search_max_top_k() -> usize {
    static SEARCH_MAX_TOP_K: OnceLock<usize> = OnceLock::new();
    *SEARCH_MAX_TOP_K
        .get_or_init(|| get_usize_env("MCP_SEARCH_MAX_TOP_K", 20).max(get_search_default_top_k()))
}

pub fn get_search_max_response_tokens() -> usize {
    static SEARCH_MAX_RESPONSE_TOKENS: OnceLock<usize> = OnceLock::new();
    *SEARCH_MAX_RESPONSE_TOKENS
        .get_or_init(|| get_usize_env("MCP_SEARCH_MAX_RESPONSE_TOKENS", 5_000))
}

pub fn get_query_expansion_mode() -> crate::query_expansion::QueryExpansionMode {
    static QUERY_EXPANSION_MODE: OnceLock<crate::query_expansion::QueryExpansionMode> =
        OnceLock::new();
    *QUERY_EXPANSION_MODE.get_or_init(|| {
        crate::query_expansion::QueryExpansionMode::from_config(
            &std::env::var("QUERY_EXPANSION_MODE").unwrap_or_else(|_| "off".to_string()),
        )
    })
}

pub fn get_query_expansion_dictionary_paths() -> Vec<String> {
    static QUERY_EXPANSION_DICTIONARY_PATHS: OnceLock<Vec<String>> = OnceLock::new();
    QUERY_EXPANSION_DICTIONARY_PATHS
        .get_or_init(|| {
            std::env::var("QUERY_EXPANSION_DICTIONARY_PATHS")
                .unwrap_or_default()
                .split([';', ','])
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .clone()
}

pub fn get_query_expansion_max_terms() -> usize {
    static QUERY_EXPANSION_MAX_TERMS: OnceLock<usize> = OnceLock::new();
    *QUERY_EXPANSION_MAX_TERMS.get_or_init(|| get_usize_env("QUERY_EXPANSION_MAX_TERMS", 8))
}

pub fn get_read_document_max_tokens() -> usize {
    static READ_DOCUMENT_MAX_TOKENS: OnceLock<usize> = OnceLock::new();
    *READ_DOCUMENT_MAX_TOKENS.get_or_init(|| get_usize_env("MCP_READ_DOCUMENT_MAX_TOKENS", 10_000))
}

pub fn get_tokenizer_model() -> String {
    static TOKENIZER_MODEL: OnceLock<String> = OnceLock::new();
    TOKENIZER_MODEL
        .get_or_init(|| {
            std::env::var("TOKENIZER_MODEL").unwrap_or_else(|_| "o200k_harmony".to_string())
        })
        .clone()
}

pub fn get_embedding_model_name() -> String {
    static EMBEDDING_MODEL_NAME: OnceLock<String> = OnceLock::new();
    EMBEDDING_MODEL_NAME
        .get_or_init(|| {
            std::env::var("EMBEDDING_MODEL_NAME").unwrap_or_else(|_| {
                "jeffh/intfloat-multilingual-e5-large-instruct:q8_0".to_string()
            })
        })
        .clone()
}

pub fn get_embedding_batch_size() -> usize {
    static EMBEDDING_BATCH_SIZE: OnceLock<usize> = OnceLock::new();
    *EMBEDDING_BATCH_SIZE.get_or_init(|| get_usize_env("EMBEDDING_BATCH_SIZE", 32).max(1))
}

pub fn get_qdrant_anchors_collection() -> String {
    static QDRANT_ANCHORS: OnceLock<String> = OnceLock::new();
    QDRANT_ANCHORS
        .get_or_init(|| {
            std::env::var("QDRANT_ANCHORS_COLLECTION").unwrap_or_else(|_| "anchors".to_string())
        })
        .clone()
}

pub fn get_qdrant_upsert_wait() -> bool {
    static QDRANT_UPSERT_WAIT: OnceLock<bool> = OnceLock::new();
    *QDRANT_UPSERT_WAIT.get_or_init(|| get_bool_env("QDRANT_UPSERT_WAIT", false))
}

pub fn get_http_connect_timeout_secs() -> u64 {
    static HTTP_CONNECT_TIMEOUT_SECS: OnceLock<u64> = OnceLock::new();
    *HTTP_CONNECT_TIMEOUT_SECS.get_or_init(|| get_u64_env("HTTP_CONNECT_TIMEOUT_SECS", 15))
}

pub fn get_embedding_request_timeout_secs() -> u64 {
    static EMBEDDING_REQUEST_TIMEOUT_SECS: OnceLock<u64> = OnceLock::new();
    *EMBEDDING_REQUEST_TIMEOUT_SECS
        .get_or_init(|| get_u64_env("EMBEDDING_REQUEST_TIMEOUT_SECS", 120))
}

pub fn get_mistral_upload_timeout_secs() -> u64 {
    static MISTRAL_UPLOAD_TIMEOUT_SECS: OnceLock<u64> = OnceLock::new();
    *MISTRAL_UPLOAD_TIMEOUT_SECS.get_or_init(|| get_u64_env("MISTRAL_UPLOAD_TIMEOUT_SECS", 240))
}

pub fn get_mistral_ocr_timeout_secs() -> u64 {
    static MISTRAL_OCR_TIMEOUT_SECS: OnceLock<u64> = OnceLock::new();
    *MISTRAL_OCR_TIMEOUT_SECS.get_or_init(|| get_u64_env("MISTRAL_OCR_TIMEOUT_SECS", 300))
}

pub fn get_mistral_retry_attempts() -> usize {
    static MISTRAL_RETRY_ATTEMPTS: OnceLock<usize> = OnceLock::new();
    *MISTRAL_RETRY_ATTEMPTS.get_or_init(|| get_usize_env("MISTRAL_RETRY_ATTEMPTS", 3).max(1))
}

pub fn get_mistral_retry_delay_ms() -> u64 {
    static MISTRAL_RETRY_DELAY_MS: OnceLock<u64> = OnceLock::new();
    *MISTRAL_RETRY_DELAY_MS.get_or_init(|| get_u64_env("MISTRAL_RETRY_DELAY_MS", 1_000))
}

pub fn get_mistral_base_url() -> String {
    static MISTRAL_BASE_URL: OnceLock<String> = OnceLock::new();
    MISTRAL_BASE_URL
        .get_or_init(|| {
            std::env::var("MISTRAL_BASE_URL")
                .unwrap_or_else(|_| "https://api.mistral.ai/v1".to_string())
                .trim_end_matches('/')
                .to_string()
        })
        .clone()
}
