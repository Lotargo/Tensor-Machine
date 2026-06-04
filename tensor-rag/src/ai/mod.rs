use crate::mcp::{SearchTextTool, ToolHandler};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

pub const DEFAULT_SYSTEM_PROMPT: &str = "You are a grounded RAG assistant for the local document library. Answer clearly, stay close to the retrieved context, and say when the library does not contain enough evidence. When you rely on retrieved passages, cite them inline as [1], [2], and so on.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub doc_uuid: Option<String>,
    #[serde(default)]
    pub scope_to_selected_document: bool,
    #[serde(default = "default_use_retrieval")]
    pub use_retrieval: bool,
}

fn default_use_retrieval() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatSource {
    pub index: usize,
    pub title: String,
    pub excerpt: String,
    pub score: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatResponse {
    pub message: ChatMessage,
    pub sources: Vec<ChatSource>,
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatDoneEvent {
    pub provider: String,
    pub model: String,
    pub sources: Vec<ChatSource>,
}

#[derive(Debug, Clone)]
pub struct AiConfig {
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub temperature: f32,
    pub rag_top_k: usize,
    pub system_prompt: String,
}

pub fn ai_defaults_for_provider(provider: &str) -> (&'static str, &'static str) {
    match provider {
        "gemini" => (
            "https://generativelanguage.googleapis.com/v1beta/openai",
            "gemini-3.5-flash",
        ),
        "openrouter" => ("https://openrouter.ai/api/v1", "openai/gpt-4.1-mini"),
        "local" => ("http://localhost:11434/v1", "llama3.1"),
        "custom" => ("", ""),
        _ => ("https://api.openai.com/v1", "gpt-4.1-mini"),
    }
}

fn get_setting(settings: &HashMap<String, String>, key: &str) -> Option<String> {
    settings
        .get(key)
        .cloned()
        .or_else(|| std::env::var(key).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn load_ai_config(settings: &HashMap<String, String>) -> Result<AiConfig, String> {
    let provider = get_setting(settings, "AI_PROVIDER").unwrap_or_else(|| "openai".to_string());
    let (default_base_url, default_model) = ai_defaults_for_provider(&provider);
    let base_url = get_setting(settings, "AI_BASE_URL").unwrap_or_else(|| default_base_url.to_string());
    let model = get_setting(settings, "AI_MODEL").unwrap_or_else(|| default_model.to_string());
    let api_key = get_setting(settings, "AI_API_KEY").unwrap_or_default();
    let temperature = get_setting(settings, "AI_TEMPERATURE")
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(0.2)
        .clamp(0.0, 2.0);
    let rag_top_k = get_setting(settings, "AI_RAG_TOP_K")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(5)
        .max(1);
    let system_prompt =
        get_setting(settings, "AI_SYSTEM_PROMPT").unwrap_or_else(|| DEFAULT_SYSTEM_PROMPT.to_string());

    if base_url.is_empty() {
        return Err("AI_BASE_URL is empty. Choose a provider or specify a custom base URL.".to_string());
    }
    if model.is_empty() {
        return Err("AI_MODEL is empty. Choose a model in the AI settings.".to_string());
    }

    Ok(AiConfig {
        provider,
        base_url,
        model,
        api_key,
        temperature,
        rag_top_k,
        system_prompt,
    })
}

pub async fn run_chat(
    db_pools: Arc<crate::db::DbPools>,
    settings: &HashMap<String, String>,
    request: ChatRequest,
) -> Result<ChatResponse, String> {
    if request.messages.is_empty() {
        return Err("Chat request must include at least one message.".to_string());
    }

    let config = load_ai_config(settings)?;
    let query = request
        .messages
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map(|message| message.content.trim().to_string())
        .filter(|text| !text.is_empty())
        .ok_or_else(|| "Chat request must include a non-empty user message.".to_string())?;

    let sources = if request.use_retrieval {
        let scoped_doc_uuid = if request.scope_to_selected_document {
            request.doc_uuid.clone()
        } else {
            None
        };
        retrieve_sources(&db_pools, &query, scoped_doc_uuid, config.rag_top_k).await?
    } else {
        Vec::new()
    };
    let completion = request_completion(&config, &request.messages, &sources).await?;

    Ok(ChatResponse {
        message: ChatMessage {
            role: "assistant".to_string(),
            content: completion,
        },
        sources,
        provider: config.provider,
        model: config.model,
    })
}

pub async fn stream_chat(
    db_pools: Arc<crate::db::DbPools>,
    settings: &HashMap<String, String>,
    request: ChatRequest,
    tx: tokio::sync::mpsc::Sender<String>,
) -> Result<(), String> {
    if request.messages.is_empty() {
        return Err("Chat request must include at least one message.".to_string());
    }

    let config = load_ai_config(settings)?;
    let query = request
        .messages
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map(|message| message.content.trim().to_string())
        .filter(|text| !text.is_empty())
        .ok_or_else(|| "Chat request must include a non-empty user message.".to_string())?;

    let sources = if request.use_retrieval {
        send_sse(
            &tx,
            "thinking",
            &json!({
                "phase": "retrieval",
                "label": "Searching the library for relevant passages"
            }),
        )
        .await?;

        let scoped_doc_uuid = if request.scope_to_selected_document {
            request.doc_uuid.clone()
        } else {
            None
        };
        let sources = retrieve_sources(&db_pools, &query, scoped_doc_uuid, config.rag_top_k).await?;
        send_sse(&tx, "sources", &serde_json::to_value(&sources).map_err(|e| e.to_string())?).await?;
        sources
    } else {
        Vec::new()
    };
    send_sse(
        &tx,
        "thinking",
        &json!({
            "phase": "reasoning",
            "label": "Reasoning over the retrieved context"
        }),
    )
    .await?;

    request_completion_stream(&config, &request.messages, &sources, &tx).await?;
    send_sse(
        &tx,
        "done",
        &serde_json::to_value(ChatDoneEvent {
            provider: config.provider,
            model: config.model,
            sources,
        })
        .map_err(|e| e.to_string())?,
    )
    .await?;
    Ok(())
}

async fn retrieve_sources(
    db_pools: &crate::db::DbPools,
    query: &str,
    doc_uuid: Option<String>,
    rag_top_k: usize,
) -> Result<Vec<ChatSource>, String> {
    let search_tool = SearchTextTool;
    let mut args = json!({
        "query": query,
        "final_top_k": rag_top_k
    });
    if let Some(doc_uuid) = doc_uuid {
        args["doc_uuid"] = json!(doc_uuid);
    }

    let bytes = search_tool.handle(args, db_pools, None).await?;
    let payload: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|err| format!("Failed to decode MCP search response: {err}"))?;

    let documents = payload
        .get("documents")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let scores = payload
        .get("scores")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    Ok(documents
        .iter()
        .enumerate()
        .filter_map(|(index, value)| {
            let text = value.as_str()?.trim();
            if text.is_empty() {
                return None;
            }
            let (title, excerpt) = parse_source_block(text);
            let score = scores.get(index).and_then(|score| score.as_f64()).map(|score| score as f32);
            Some(ChatSource {
                index: index + 1,
                title,
                excerpt,
                score,
            })
        })
        .collect())
}

fn parse_source_block(text: &str) -> (String, String) {
    let mut lines = text.lines();
    if let Some(first_line) = lines.next() {
        if let Some(source) = first_line.strip_prefix("Source: ") {
            let excerpt = lines.collect::<Vec<_>>().join("\n").trim().to_string();
            return (
                source.trim().to_string(),
                if excerpt.is_empty() {
                    text.trim().to_string()
                } else {
                    excerpt
                },
            );
        }
    }

    ("Library result".to_string(), text.trim().to_string())
}

async fn request_completion(
    config: &AiConfig,
    messages: &[ChatMessage],
    sources: &[ChatSource],
) -> Result<String, String> {
    let client = reqwest::Client::new();
    let request_messages = build_request_messages(config, messages, sources);

    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    let mut builder = client.post(url).json(&json!({
        "model": config.model,
        "messages": request_messages,
        "temperature": config.temperature
    }));

    if !config.api_key.is_empty() {
        builder = builder.header("Authorization", format!("Bearer {}", config.api_key));
    }
    if config.provider == "openrouter" {
        builder = builder
            .header("HTTP-Referer", "http://localhost:8080")
            .header("X-Title", "Tensor RAG");
    }

    let response = builder
        .send()
        .await
        .map_err(|err| format!("Failed to call AI provider: {err}"))?;
    let status = response.status();
    let payload: serde_json::Value = response
        .json()
        .await
        .map_err(|err| format!("Failed to decode AI provider response: {err}"))?;

    if !status.is_success() {
        return Err(payload
            .get("error")
            .and_then(|value| value.get("message"))
            .and_then(|value| value.as_str())
            .map(|message| format!("AI provider error: {message}"))
            .unwrap_or_else(|| format!("AI provider request failed with status {status}")));
    }

    extract_message_content(&payload)
        .ok_or_else(|| "AI provider response does not contain an assistant message.".to_string())
}

async fn request_completion_stream(
    config: &AiConfig,
    messages: &[ChatMessage],
    sources: &[ChatSource],
    tx: &tokio::sync::mpsc::Sender<String>,
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    let mut builder = client.post(url).json(&json!({
        "model": config.model,
        "messages": build_request_messages(config, messages, sources),
        "temperature": config.temperature,
        "stream": true
    }));

    if !config.api_key.is_empty() {
        builder = builder.header("Authorization", format!("Bearer {}", config.api_key));
    }
    if config.provider == "openrouter" {
        builder = builder
            .header("HTTP-Referer", "http://localhost:8080")
            .header("X-Title", "Tensor RAG");
    }

    let response = builder
        .send()
        .await
        .map_err(|err| format!("Failed to call AI provider: {err}"))?;
    let status = response.status();
    if !status.is_success() {
        let payload: serde_json::Value = response
            .json()
            .await
            .map_err(|err| format!("Failed to decode AI provider response: {err}"))?;
        return Err(payload
            .get("error")
            .and_then(|value| value.get("message"))
            .and_then(|value| value.as_str())
            .map(|message| format!("AI provider error: {message}"))
            .unwrap_or_else(|| format!("AI provider request failed with status {status}")));
    }

    let mut buffer = String::new();
    let mut upstream = response.bytes_stream();

    while let Some(chunk) = upstream.next().await {
        let chunk = chunk.map_err(|err| format!("Streaming error from AI provider: {err}"))?;
        let text = String::from_utf8_lossy(&chunk).replace("\r\n", "\n");
        buffer.push_str(&text);

        while let Some(idx) = buffer.find("\n\n") {
            let frame = buffer[..idx].to_string();
            buffer = buffer[idx + 2..].to_string();
            process_provider_sse_frame(&frame, tx).await?;
        }
    }

    if !buffer.trim().is_empty() {
        process_provider_sse_frame(&buffer, tx).await?;
    }

    Ok(())
}

fn build_request_messages(
    config: &AiConfig,
    messages: &[ChatMessage],
    sources: &[ChatSource],
) -> Vec<serde_json::Value> {
    let mut request_messages = Vec::new();
    request_messages.push(json!({
        "role": "system",
        "content": config.system_prompt
    }));

    if !sources.is_empty() {
        let context = sources
            .iter()
            .map(|source| format!("[{}] {}\n{}", source.index, source.title, source.excerpt))
            .collect::<Vec<_>>()
            .join("\n\n");
        request_messages.push(json!({
            "role": "system",
            "content": format!(
                "Retrieved context from the local RAG library:\n\n{}\n\nUse this context when it is relevant. If it is insufficient, say so explicitly.",
                context
            )
        }));
    }

    for message in messages {
        let role = match message.role.as_str() {
            "system" | "assistant" | "user" => message.role.as_str(),
            _ => "user",
        };
        request_messages.push(json!({
            "role": role,
            "content": message.content
        }));
    }

    request_messages
}

async fn process_provider_sse_frame(
    frame: &str,
    tx: &tokio::sync::mpsc::Sender<String>,
) -> Result<(), String> {
    for line in frame.lines() {
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };

        if data.trim() == "[DONE]" {
            return Ok(());
        }

        let payload: serde_json::Value =
            serde_json::from_str(data).map_err(|err| format!("Failed to parse streamed chunk: {err}"))?;
        let delta = payload
            .get("choices")
            .and_then(|value| value.get(0))
            .and_then(|value| value.get("delta"))
            .and_then(extract_delta_content);

        if let Some(delta) = delta {
            send_sse(tx, "token", &json!({ "delta": delta })).await?;
        }
    }

    Ok(())
}

fn extract_delta_content(delta: &serde_json::Value) -> Option<String> {
    let content = delta.get("content")?;
    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }

    let parts = content.as_array()?;
    let joined = parts
        .iter()
        .filter_map(|part| {
            if let Some(text) = part.get("text").and_then(|value| value.as_str()) {
                return Some(text.to_string());
            }
            part.get("content")
                .and_then(|value| value.as_str())
                .map(|text| text.to_string())
        })
        .collect::<Vec<_>>()
        .join("");

    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

async fn send_sse(
    tx: &tokio::sync::mpsc::Sender<String>,
    event: &str,
    payload: &serde_json::Value,
) -> Result<(), String> {
    let frame = format!(
        "event: {}\ndata: {}\n\n",
        event,
        serde_json::to_string(payload).map_err(|err| err.to_string())?
    );
    tx.send(frame)
        .await
        .map_err(|_| "SSE channel closed before the response was delivered.".to_string())
}

fn extract_message_content(payload: &serde_json::Value) -> Option<String> {
    let message = payload.get("choices")?.get(0)?.get("message")?;
    let content = message.get("content")?;

    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }

    let parts = content.as_array()?;
    let merged = parts
        .iter()
        .filter_map(|part| {
            if let Some(text) = part.get("text").and_then(|value| value.as_str()) {
                return Some(text.to_string());
            }
            part.get("content")
                .and_then(|value| value.as_str())
                .map(|text| text.to_string())
        })
        .collect::<Vec<_>>()
        .join("\n");

    if merged.trim().is_empty() {
        None
    } else {
        Some(merged)
    }
}
