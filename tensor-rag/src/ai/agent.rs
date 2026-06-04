use crate::mcp::{
    GetDocumentMetadataTool, HelpTool, ListDocumentsTool, ReadDocumentTool, SearchTextTool, ToolHandler,
};
use serde_json::json;

use super::{extract_message_content, parse_search_sources, send_sse, AiConfig, ChatDoneEvent, ChatSource};

pub async fn run_agent_completion(
    db_pools: &crate::db::DbPools,
    config: &AiConfig,
    mut messages: Vec<serde_json::Value>,
    use_tools: bool,
    selected_doc_uuid: Option<&str>,
    scope_to_selected_document: bool,
    tx: Option<&tokio::sync::mpsc::Sender<String>>,
) -> Result<(String, Vec<ChatSource>), String> {
    let mut collected_sources = Vec::new();

    if use_tools {
        for _round in 0..config.agent_max_tool_rounds {
            let payload = super::request_chat_payload(
                config,
                &messages,
                Some(tool_definitions()),
                false,
            )
            .await?;
            let message = payload
                .get("choices")
                .and_then(|value| value.get(0))
                .and_then(|value| value.get("message"))
                .cloned()
                .ok_or_else(|| "AI provider response does not contain an assistant message.".to_string())?;

            let tool_calls = extract_tool_calls(&message);
            if tool_calls.is_empty() {
                let content = extract_message_content(&payload).ok_or_else(|| {
                    "AI provider response does not contain an assistant message.".to_string()
                })?;
                return Ok((content, collected_sources));
            }

            messages.push(message);
            for tool_call in tool_calls {
                if let Some(tx) = tx {
                    send_sse(
                        tx,
                        "tool",
                        &json!({
                            "name": tool_call.name,
                            "label": human_tool_label(&tool_call.name)
                        }),
                    )
                    .await?;
                }

                let (tool_response, mut tool_sources) = execute_tool_call(
                    db_pools,
                    &tool_call,
                    selected_doc_uuid,
                    scope_to_selected_document,
                )
                .await?;
                collected_sources.append(&mut tool_sources);
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": tool_call.id,
                    "content": tool_response
                }));
            }
        }
    }

    let content = super::request_completion_from_messages(config, &messages).await?;
    Ok((content, collected_sources))
}

pub async fn stream_agent_completion(
    db_pools: &crate::db::DbPools,
    config: &AiConfig,
    mut messages: Vec<serde_json::Value>,
    use_tools: bool,
    selected_doc_uuid: Option<&str>,
    scope_to_selected_document: bool,
    tx: &tokio::sync::mpsc::Sender<String>,
) -> Result<ChatDoneEvent, String> {
    let mut collected_sources = Vec::new();

    if use_tools {
        for _round in 0..config.agent_max_tool_rounds {
            let payload = super::request_chat_payload(
                config,
                &messages,
                Some(tool_definitions()),
                false,
            )
            .await?;
            let message = payload
                .get("choices")
                .and_then(|value| value.get(0))
                .and_then(|value| value.get("message"))
                .cloned()
                .ok_or_else(|| "AI provider response does not contain an assistant message.".to_string())?;

            let tool_calls = extract_tool_calls(&message);
            if tool_calls.is_empty() {
                break;
            }

            messages.push(message);
            for tool_call in tool_calls {
                send_sse(
                    tx,
                    "tool",
                    &json!({
                        "name": tool_call.name,
                        "label": human_tool_label(&tool_call.name)
                    }),
                )
                .await?;

                let (tool_response, mut tool_sources) = execute_tool_call(
                    db_pools,
                    &tool_call,
                    selected_doc_uuid,
                    scope_to_selected_document,
                )
                .await?;
                if !tool_sources.is_empty() {
                    collected_sources.append(&mut tool_sources);
                    send_sse(tx, "sources", &serde_json::to_value(&collected_sources).map_err(|e| e.to_string())?)
                        .await?;
                }
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": tool_call.id,
                    "content": tool_response
                }));
            }
        }
    }

    super::request_completion_stream_from_messages(config, &messages, tx).await?;
    Ok(ChatDoneEvent {
        provider: config.provider.clone(),
        model: config.model.clone(),
        sources: dedupe_sources(collected_sources),
    })
}

#[derive(Debug, Clone)]
struct ToolCall {
    id: String,
    name: String,
    arguments: serde_json::Value,
}

fn extract_tool_calls(message: &serde_json::Value) -> Vec<ToolCall> {
    message
        .get("tool_calls")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|call| {
            let id = call.get("id")?.as_str()?.to_string();
            let function = call.get("function")?;
            let name = function.get("name")?.as_str()?.to_string();
            let arguments = function
                .get("arguments")
                .and_then(|value| value.as_str())
                .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
                .unwrap_or_else(|| json!({}));
            Some(ToolCall { id, name, arguments })
        })
        .collect()
}

async fn execute_tool_call(
    db_pools: &crate::db::DbPools,
    tool_call: &ToolCall,
    selected_doc_uuid: Option<&str>,
    scope_to_selected_document: bool,
) -> Result<(String, Vec<ChatSource>), String> {
    let args = sanitize_tool_args(
        &tool_call.name,
        tool_call.arguments.clone(),
        selected_doc_uuid,
        scope_to_selected_document,
    );
    let bytes = match tool_call.name.as_str() {
        "search_text" => SearchTextTool.handle(args, db_pools, None).await?,
        "read_document" => ReadDocumentTool.handle(args, db_pools, None).await?,
        "list_documents" => ListDocumentsTool.handle(args, db_pools, None).await?,
        "get_document_metadata" => GetDocumentMetadataTool.handle(args, db_pools, None).await?,
        "help" => HelpTool.handle(args, db_pools, None).await?,
        _ => {
            return Ok((
                format!("Unsupported tool: {}", tool_call.name),
                Vec::new(),
            ))
        }
    };

    let text = String::from_utf8_lossy(&bytes).to_string();
    let sources = if tool_call.name == "search_text" {
        parse_search_sources(&text)?
    } else {
        Vec::new()
    };
    Ok((text, sources))
}

fn sanitize_tool_args(
    name: &str,
    mut args: serde_json::Value,
    selected_doc_uuid: Option<&str>,
    scope_to_selected_document: bool,
) -> serde_json::Value {
    if name == "search_text" {
        if args.get("final_top_k").is_none() {
            args["final_top_k"] = json!(5);
        }
    }
    if scope_to_selected_document && matches!(name, "search_text" | "read_document") {
        if let Some(doc_uuid) = selected_doc_uuid {
            args["doc_uuid"] = json!(doc_uuid);
        }
    }
    args
}

fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "search_text",
                "description": "Search the local RAG document library for relevant passages.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Short keyword-style search query in the user's language."
                        },
                        "final_top_k": {
                            "type": "integer",
                            "description": "How many results to return."
                        },
                        "doc_uuid": {
                            "type": "string",
                            "description": "Optional document UUID to restrict the search scope."
                        }
                    },
                    "required": ["query"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "read_document",
                "description": "Read one indexed document if it fits within the configured token budget.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "doc_uuid": {
                            "type": "string",
                            "description": "Document UUID returned by list_documents."
                        }
                    },
                    "required": ["doc_uuid"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "list_documents",
                "description": "List the indexed documents available in the local library.",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "get_document_metadata",
                "description": "Fetch metadata for one indexed document.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "doc_uuid": {
                            "type": "string",
                            "description": "Document UUID returned by list_documents."
                        }
                    },
                    "required": ["doc_uuid"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "help",
                "description": "Read MCP tool usage guidance for the local library.",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        }),
    ]
}

fn human_tool_label(name: &str) -> &'static str {
    match name {
        "search_text" => "Searching the document library",
        "read_document" => "Reading the selected document",
        "list_documents" => "Listing indexed documents",
        "get_document_metadata" => "Inspecting document metadata",
        "help" => "Loading tool guidance",
        _ => "Using MCP tool",
    }
}

fn dedupe_sources(sources: Vec<ChatSource>) -> Vec<ChatSource> {
    let mut seen = std::collections::HashSet::new();
    let mut deduped = Vec::new();

    for source in sources {
        let key = format!("{}::{}", source.title, source.excerpt);
        if seen.insert(key) {
            deduped.push(ChatSource {
                index: deduped.len() + 1,
                ..source
            });
        }
    }

    deduped
}
