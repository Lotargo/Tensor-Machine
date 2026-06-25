mod agent;
mod context;

use crate::mcp::{SearchTextTool, ToolHandler};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::error::Error as StdError;
use std::sync::Arc;
use std::time::Duration;
use tracing::warn;

pub use context::{AiChatMode, ContextOverflowStrategy};

pub const DEFAULT_SYSTEM_PROMPT: &str = r#"You are a grounded RAG assistant for the local document library. For questions
about local documents, answer only from retrieved passages and never fill gaps
from outside knowledge. If the retrieved library context does not contain enough
evidence, say that explicitly. When you rely on retrieved passages, cite them
inline as [1], [2], and so on.

When the user asks a general question that could apply to any of several
documents (e.g. "what was the mother's name?", "what happened at the beginning
of the document?", "describe the main character"):

1. First, retrieve the list of available documents from the RAG library.
2. If there are 2-4 documents, iterate: search each document separately and
   synthesise the answer from the results.
3. If there are more than 4 documents or they are clearly heterogeneous
   (different topics, genres, authors), show the user the list of available
   documents and ask which one they want to search in. After clarification,
   search only in the specified document."#;

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
    pub context: Option<String>,
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
    pub chat_mode: AiChatMode,
    pub context_strategy: ContextOverflowStrategy,
    pub context_token_limit: usize,
    pub agent_max_tool_rounds: usize,
    pub request_timeout_secs: u64,
    pub retry_attempts: usize,
    pub retry_delay_ms: u64,
}

pub fn ai_defaults_for_provider(provider: &str) -> (&'static str, &'static str) {
    match provider {
        "gemini" => (
            "https://generativelanguage.googleapis.com/v1beta/openai",
            "gemini-3.5-flash",
        ),
        "openrouter" => ("https://openrouter.ai/api/v1", ""),
        "zen" => ("https://opencode.ai/zen/v1", "big-pickle"),
        "local" => ("http://localhost:11434/v1", ""),
        "custom" => ("", ""),
        _ => ("https://api.openai.com/v1", "gpt-5.5"),
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
    let base_url =
        get_setting(settings, "AI_BASE_URL").unwrap_or_else(|| default_base_url.to_string());
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
    let system_prompt = get_setting(settings, "AI_SYSTEM_PROMPT")
        .unwrap_or_else(|| DEFAULT_SYSTEM_PROMPT.to_string());
    let chat_mode = context::AiChatMode::from_setting(
        &get_setting(settings, "AI_CHAT_MODE").unwrap_or_else(|| "rag".to_string()),
    );
    let context_strategy = context::ContextOverflowStrategy::from_setting(
        &get_setting(settings, "AI_CONTEXT_STRATEGY").unwrap_or_else(|| "trim_middle".to_string()),
    );
    let context_token_limit = get_setting(settings, "AI_CONTEXT_TOKEN_LIMIT")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(16000)
        .max(1024);
    let agent_max_tool_rounds = get_setting(settings, "AI_AGENT_MAX_TOOL_ROUNDS")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(8)
        .clamp(1, 24);
    let request_timeout_secs = get_setting(settings, "AI_REQUEST_TIMEOUT_SECS")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(120)
        .max(1);
    let retry_attempts = get_setting(settings, "AI_RETRY_ATTEMPTS")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(3)
        .max(1);
    let retry_delay_ms = get_setting(settings, "AI_RETRY_DELAY_MS")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(1_000);

    if base_url.is_empty() {
        return Err(
            "AI_BASE_URL is empty. Choose a provider or specify a custom base URL.".to_string(),
        );
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
        chat_mode,
        context_strategy,
        context_token_limit,
        agent_max_tool_rounds,
        request_timeout_secs,
        retry_attempts,
        retry_delay_ms,
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
    let query = last_user_query(&request.messages)?;

    let (completion, sources) = if config.chat_mode == AiChatMode::Agent {
        let messages = context::build_agent_messages(
            &config,
            &request.messages,
            request.doc_uuid.as_deref(),
            request.scope_to_selected_document,
        )?;
        agent::run_agent_completion(
            &db_pools,
            &config,
            messages,
            request.use_retrieval,
            request.doc_uuid.as_deref(),
            request.scope_to_selected_document,
            None,
        )
        .await?
    } else {
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
        let request_messages =
            context::build_completion_messages(&config, &request.messages, &sources)?;
        let completion = request_completion_from_messages(&config, &request_messages).await?;
        (completion, sources)
    };

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
    let query = last_user_query(&request.messages)?;

    let done_event = if config.chat_mode == AiChatMode::Agent {
        send_sse(
            &tx,
            "thinking",
            &json!({
                "phase": "reasoning",
                "label": "Agent is deciding whether to call MCP tools"
            }),
        )
        .await?;

        let messages = context::build_agent_messages(
            &config,
            &request.messages,
            request.doc_uuid.as_deref(),
            request.scope_to_selected_document,
        )?;
        agent::stream_agent_completion(
            &db_pools,
            &config,
            messages,
            request.use_retrieval,
            request.doc_uuid.as_deref(),
            request.scope_to_selected_document,
            &tx,
        )
        .await?
    } else {
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
            let sources =
                retrieve_sources(&db_pools, &query, scoped_doc_uuid, config.rag_top_k).await?;
            send_sse(
                &tx,
                "sources",
                &serde_json::to_value(&sources).map_err(|e| e.to_string())?,
            )
            .await?;
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

        let request_messages =
            context::build_completion_messages(&config, &request.messages, &sources)?;
        request_completion_stream_from_messages(&config, &request_messages, &tx).await?;
        ChatDoneEvent {
            provider: config.provider.clone(),
            model: config.model.clone(),
            sources,
        }
    };

    send_sse(
        &tx,
        "done",
        &serde_json::to_value(done_event).map_err(|e| e.to_string())?,
    )
    .await?;
    Ok(())
}

fn last_user_query(messages: &[ChatMessage]) -> Result<String, String> {
    messages
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map(|message| message.content.trim().to_string())
        .filter(|text| !text.is_empty())
        .ok_or_else(|| "Chat request must include a non-empty user message.".to_string())
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
    let text = String::from_utf8_lossy(&bytes).to_string();
    parse_search_sources(&text)
}

pub(crate) fn parse_search_sources(payload_text: &str) -> Result<Vec<ChatSource>, String> {
    let payload: serde_json::Value = serde_json::from_str(payload_text)
        .map_err(|err| format!("Failed to decode MCP search response: {err}"))?;

    let documents = payload
        .get("documents")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let doc_scores = payload
        .get("document_scores")
        .and_then(|value| value.as_array())
        .cloned();
    let scores = payload
        .get("scores")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    let mut chat_sources = Vec::new();
    let mut doc_counter = 0;

    for value in documents {
        let text = match value.as_str() {
            Some(t) => t.trim(),
            None => continue,
        };
        if text.is_empty() {
            continue;
        }

        let (title, context, excerpt) = parse_source_block(text);

        let score = if let Some(ref ds) = doc_scores {
            ds.get(doc_counter)
                .and_then(|score| score.as_f64())
                .map(|score| score as f32)
        } else {
            scores
                .get(doc_counter)
                .and_then(|score| score.as_f64())
                .map(|score| score as f32)
        };

        chat_sources.push(ChatSource {
            index: doc_counter + 1,
            title,
            context,
            excerpt,
            score,
        });

        doc_counter += 1;
    }

    Ok(chat_sources)
}

fn parse_source_block(text: &str) -> (String, Option<String>, String) {
    let mut lines = text.lines();
    let mut title = "Library result".to_string();
    let mut context = None;
    let mut body_lines = Vec::new();

    if let Some(first_line) = lines.next() {
        if first_line.starts_with(">>> НАЧАЛО ДОКУМЕНТА") {
            if let Some(second_line) = lines.next() {
                if let Some(t) = second_line.strip_prefix("Title: ") {
                    title = t.trim().to_string();
                } else {
                    body_lines.push(second_line);
                }
            }
            if let Some(third_line) = lines.next() {
                if let Some(c) = third_line.strip_prefix("Context: ") {
                    context = Some(c.trim().to_string());
                } else {
                    body_lines.push(third_line);
                }
            }

            for line in lines {
                if !line.starts_with(">>> КОНЕЦ ДОКУМЕНТА") {
                    body_lines.push(line);
                }
            }

            return (title, context, body_lines.join("\n").trim().to_string());
        } else {
            body_lines.push(first_line);
        }
    }

    for line in lines {
        body_lines.push(line);
    }

    let fallback_text = body_lines.join("\n").trim().to_string();
    if let Some(source) = fallback_text
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("Source: "))
    {
        let excerpt = fallback_text
            .lines()
            .skip(1)
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();
        return (source.trim().to_string(), None, excerpt);
    }

    (title, None, fallback_text)
}

pub(crate) async fn request_completion_from_messages(
    config: &AiConfig,
    messages: &[serde_json::Value],
) -> Result<String, String> {
    let payload = request_chat_payload(config, messages, None, false).await?;
    extract_message_content(&payload)
        .ok_or_else(|| "AI provider response does not contain an assistant message.".to_string())
}

pub(crate) async fn request_completion_stream_from_messages(
    config: &AiConfig,
    messages: &[serde_json::Value],
    tx: &tokio::sync::mpsc::Sender<String>,
) -> Result<(), String> {
    let response = send_chat_request(config, messages, None, true).await?;
    let status = response.status();
    if !status.is_success() {
        let payload: serde_json::Value = response
            .json()
            .await
            .map_err(|err| format!("Failed to decode AI provider response: {err}"))?;
        return Err(provider_error_message(status, &payload));
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

pub(crate) async fn request_chat_payload(
    config: &AiConfig,
    messages: &[serde_json::Value],
    tools: Option<Vec<serde_json::Value>>,
    stream: bool,
) -> Result<serde_json::Value, String> {
    let response = send_chat_request(config, messages, tools, stream).await?;
    let status = response.status();
    let payload: serde_json::Value = response
        .json()
        .await
        .map_err(|err| format!("Failed to decode AI provider response: {err}"))?;

    if !status.is_success() {
        return Err(provider_error_message(status, &payload));
    }

    Ok(payload)
}

async fn send_chat_request(
    config: &AiConfig,
    messages: &[serde_json::Value],
    tools: Option<Vec<serde_json::Value>>,
    stream: bool,
) -> Result<reqwest::Response, String> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(crate::config::get_http_connect_timeout_secs()))
        .timeout(Duration::from_secs(config.request_timeout_secs))
        .build()
        .map_err(|err| format!("Failed to build AI provider client: {err}"))?;
    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    let mut body = json!({
        "model": config.model,
        "messages": messages,
        "temperature": config.temperature,
        "stream": stream
    });

    if let Some(tools) = tools {
        body["tools"] = json!(tools);
        body["tool_choice"] = json!("auto");
    }

    let retry_delay = Duration::from_millis(config.retry_delay_ms);
    let mut last_error = None;
    for attempt in 1..=config.retry_attempts {
        let mut builder = client.post(&url).json(&body);
        if !config.api_key.is_empty() {
            builder = builder.header("Authorization", format!("Bearer {}", config.api_key));
        }
        if config.provider == "openrouter" {
            builder = builder
                .header("HTTP-Referer", "http://localhost:8080")
                .header("X-Title", "Tensor RAG");
        }

        match builder.send().await {
            Ok(response) => {
                let status = response.status();
                if is_retryable_ai_status(status) && attempt < config.retry_attempts {
                    warn!(
                        attempt,
                        attempts = config.retry_attempts,
                        status = %status,
                        "AI provider returned retryable status"
                    );
                    tokio::time::sleep(retry_delay).await;
                    continue;
                }
                return Ok(response);
            }
            Err(err) => {
                let detailed = describe_reqwest_error(&err);
                warn!(
                    attempt,
                    attempts = config.retry_attempts,
                    "AI provider request failed: {}",
                    detailed
                );
                last_error = Some(detailed);
                if attempt < config.retry_attempts {
                    tokio::time::sleep(retry_delay).await;
                }
            }
        }
    }

    Err(format!(
        "Failed to call AI provider after {} attempt(s): {}",
        config.retry_attempts,
        last_error.unwrap_or_else(|| "unknown error".to_string())
    ))
}

fn is_retryable_ai_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
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

fn provider_error_message(status: reqwest::StatusCode, payload: &serde_json::Value) -> String {
    payload
        .get("error")
        .and_then(|value| value.get("message"))
        .and_then(|value| value.as_str())
        .map(|message| format!("AI provider error: {message}"))
        .unwrap_or_else(|| format!("AI provider request failed with status {status}"))
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

        let payload: serde_json::Value = serde_json::from_str(data)
            .map_err(|err| format!("Failed to parse streamed chunk: {err}"))?;
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

pub(crate) async fn send_sse(
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

pub(crate) fn extract_message_content(payload: &serde_json::Value) -> Option<String> {
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
