use crate::chunker::count_tokens;
use serde_json::json;

use super::{AiConfig, ChatMessage, ChatSource};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiChatMode {
    Rag,
    Agent,
}

impl AiChatMode {
    pub fn from_setting(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "agent" => Self::Agent,
            _ => Self::Rag,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextOverflowStrategy {
    TrimMiddle,
    SlidingWindow,
    Stop,
}

impl ContextOverflowStrategy {
    pub fn from_setting(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "sliding_window" => Self::SlidingWindow,
            "stop" => Self::Stop,
            _ => Self::TrimMiddle,
        }
    }
}

pub fn build_completion_messages(
    config: &AiConfig,
    messages: &[ChatMessage],
    sources: &[ChatSource],
) -> Result<Vec<serde_json::Value>, String> {
    let mut request_messages = vec![json!({
        "role": "system",
        "content": config.system_prompt
    })];

    if !sources.is_empty() {
        request_messages.push(json!({
            "role": "system",
            "content": build_sources_context(sources)
        }));
    }

    request_messages.extend(
        messages
            .iter()
            .map(|message| {
                json!({
                    "role": normalize_role(&message.role),
                    "content": message.content
                })
            })
            .collect::<Vec<_>>(),
    );

    trim_messages_to_budget(
        request_messages,
        config.context_token_limit,
        config.context_strategy,
    )
}

pub fn build_agent_messages(
    config: &AiConfig,
    messages: &[ChatMessage],
    doc_uuid: Option<&str>,
    scope_to_selected_document: bool,
) -> Result<Vec<serde_json::Value>, String> {
    let mut request_messages = vec![json!({
        "role": "system",
        "content": config.system_prompt
    })];
    request_messages.push(json!({
        "role": "system",
        "content": agent_instruction(doc_uuid, scope_to_selected_document, config.rag_top_k)
    }));
    request_messages.extend(
        messages
            .iter()
            .map(|message| {
                json!({
                    "role": normalize_role(&message.role),
                    "content": message.content
                })
            })
            .collect::<Vec<_>>(),
    );

    trim_messages_to_budget(
        request_messages,
        config.context_token_limit,
        config.context_strategy,
    )
}

fn normalize_role(role: &str) -> &str {
    match role {
        "system" | "assistant" | "user" | "tool" => role,
        _ => "user",
    }
}

fn build_sources_context(sources: &[ChatSource]) -> String {
    let context = sources
        .iter()
        .map(|source| format!("[{}] {}\n{}", source.index, source.title, source.excerpt))
        .collect::<Vec<_>>()
        .join("\n\n");

    format!(
        "Retrieved context from the local RAG library:\n\n{}\n\nFor questions about local documents, use only this retrieved context as evidence. Do not answer from outside knowledge. If this context is insufficient, say so explicitly.",
        context
    )
}

fn agent_instruction(
    doc_uuid: Option<&str>,
    scope_to_selected_document: bool,
    rag_top_k: usize,
) -> String {
    let mut lines = vec![
        "For questions about local documents or the document library, search_text is mandatory before answering.".to_string(),
        "The application may already inject mandatory search_text results into the conversation; treat those passages as the only evidence for local-document answers.".to_string(),
        "For local-document questions, do not answer from pretrained knowledge, even if the work is famous or you know the answer.".to_string(),
        format!(
            "When using search_text, prefer concise keyword queries and keep final_top_k at or below {}.",
            rag_top_k
        ),
        "If retrieved tool results do not contain enough evidence, say that the local library context is insufficient instead of answering from memory.".to_string(),
        "For non-library questions, answer directly if no tool is needed.".to_string(),
        "When the user asks a general question that could apply to any of several documents (e.g. \"what was the mother's name?\", \"what happened at the beginning?\", \"describe the main character\"):".to_string(),
        "1. Call list_documents to get the available documents.".to_string(),
        "2. If the list has 2-4 documents, call search_text for each document separately using the document title or id as a scope, then combine results.".to_string(),
        "3. If the list has more than 4 documents or they are clearly unrelated, present the list to the user and ask which document to search in.".to_string(),
    ];

    if scope_to_selected_document {
        if let Some(doc_uuid) = doc_uuid {
            lines.push(format!(
                "When you use search_text or read_document, stay within the selected document doc_uuid={}.",
                doc_uuid
            ));
        }
    }

    lines.join("\n")
}

fn trim_messages_to_budget(
    messages: Vec<serde_json::Value>,
    token_limit: usize,
    strategy: ContextOverflowStrategy,
) -> Result<Vec<serde_json::Value>, String> {
    if token_limit == 0 {
        return Ok(messages);
    }

    if estimate_messages_tokens(&messages) <= token_limit {
        return Ok(messages);
    }

    match strategy {
        ContextOverflowStrategy::Stop => Err(format!(
            "Conversation exceeds the configured context limit of {} tokens. Choose another overflow strategy or raise the limit.",
            token_limit
        )),
        ContextOverflowStrategy::SlidingWindow => Ok(trim_sliding_window(messages, token_limit)),
        ContextOverflowStrategy::TrimMiddle => Ok(trim_middle(messages, token_limit)),
    }
}

fn trim_sliding_window(
    messages: Vec<serde_json::Value>,
    token_limit: usize,
) -> Vec<serde_json::Value> {
    let (system_messages, conversation_messages) = split_system_and_conversation(messages);
    let mut kept = Vec::new();
    let mut acc = system_messages.clone();

    for message in conversation_messages.into_iter().rev() {
        let mut candidate = acc.clone();
        candidate.insert(system_messages.len(), message.clone());
        candidate.extend(kept.iter().cloned());
        if estimate_messages_tokens(&candidate) > token_limit {
            continue;
        }
        kept.insert(0, message);
        acc = candidate;
    }

    let mut result = system_messages;
    result.extend(kept);
    result
}

fn trim_middle(messages: Vec<serde_json::Value>, token_limit: usize) -> Vec<serde_json::Value> {
    let (system_messages, conversation_messages) = split_system_and_conversation(messages);
    if conversation_messages.len() <= 2 {
        return trim_sliding_window(
            system_messages
                .into_iter()
                .chain(conversation_messages)
                .collect(),
            token_limit,
        );
    }

    let first = conversation_messages.first().cloned();
    let last = conversation_messages.last().cloned();
    let middle = &conversation_messages[1..conversation_messages.len() - 1];
    let mut tail = Vec::new();

    for message in middle.iter().rev() {
        let mut candidate = system_messages.clone();
        if let Some(first) = &first {
            candidate.push(first.clone());
        }
        let mut next_tail = vec![message.clone()];
        next_tail.extend(tail.iter().cloned());
        candidate.extend(next_tail.clone());
        if let Some(last) = &last {
            candidate.push(last.clone());
        }
        if estimate_messages_tokens(&candidate) <= token_limit {
            tail = next_tail;
        }
    }

    let mut result = system_messages;
    if let Some(first) = first {
        result.push(first);
    }
    result.extend(tail);
    if let Some(last) = last {
        if result.last() != Some(&last) {
            result.push(last);
        }
    }

    if estimate_messages_tokens(&result) > token_limit {
        trim_sliding_window(result, token_limit)
    } else {
        result
    }
}

fn split_system_and_conversation(
    messages: Vec<serde_json::Value>,
) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
    let mut system_messages = Vec::new();
    let mut conversation_messages = Vec::new();

    for message in messages {
        let role = message
            .get("role")
            .and_then(|value| value.as_str())
            .unwrap_or("user");
        if role == "system" {
            system_messages.push(message);
        } else {
            conversation_messages.push(message);
        }
    }

    (system_messages, conversation_messages)
}

fn estimate_messages_tokens(messages: &[serde_json::Value]) -> usize {
    messages
        .iter()
        .map(|message| 8 + estimate_message_tokens(message))
        .sum()
}

fn estimate_message_tokens(message: &serde_json::Value) -> usize {
    if let Some(content) = message.get("content") {
        return estimate_content_tokens(content);
    }
    0
}

fn estimate_content_tokens(content: &serde_json::Value) -> usize {
    if let Some(text) = content.as_str() {
        return count_tokens(text);
    }
    if let Some(parts) = content.as_array() {
        return parts
            .iter()
            .map(|part| {
                part.get("text")
                    .and_then(|value| value.as_str())
                    .map(count_tokens)
                    .unwrap_or_else(|| count_tokens(&part.to_string()))
            })
            .sum();
    }
    count_tokens(&content.to_string())
}
