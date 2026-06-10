use crate::mcp::{
    GetDocumentMetadataTool, HelpTool, ListDocumentsTool, ReadDocumentTool, SearchTextTool,
    ToolHandler,
};
use serde_json::json;

use super::{
    AiConfig, ChatDoneEvent, ChatSource, extract_message_content, parse_search_sources, send_sse,
};

/// Parse DSML tool call tokens from content text.
/// DeepSeek models may output: <｜DSML｜tool_calls><｜DSML｜invoke name="search_text">...
fn parse_dsml_tool_calls(content: &str) -> Vec<ToolCall> {
    let mut calls = Vec::new();
    let mut pos = 0;
    while let Some(invoke_start) = content[pos..].find("<｜DSML｜invoke name=\"") {
        let absolute_start = pos + invoke_start;
        let name_start = absolute_start + "<｜DSML｜invoke name=\"".len();
        let Some(name_end) = content[name_start..].find('"') else {
            break;
        };
        let tool_name = &content[name_start..name_start + name_end];

        // Find the closing </｜DSML｜invoke> or </｜DSML｜parameter> blocks to extract parameters
        let params_start = name_start + name_end + 1;
        let mut args = serde_json::Map::new();

        // Parse parameters: <｜DSML｜parameter name="param_name" string="true">value</｜DSML｜parameter>
        let mut param_pos = params_start;
        while let Some(param_start) = content[param_pos..].find("<｜DSML｜parameter name=\"") {
            let p_abs = param_pos + param_start + "<｜DSML｜parameter name=\"".len();
            let Some(p_name_end) = content[p_abs..].find('"') else {
                break;
            };
            let param_name = &content[p_abs..p_abs + p_name_end];

            // Check if string="true"
            let after_name = p_abs + p_name_end + 1;
            let is_string = content[after_name..].starts_with(" string=\"true\">");

            // Find the value start
            let value_start = if is_string {
                after_name + " string=\"true\">".len()
            } else {
                content[after_name..].find('>').map(|i| after_name + i + 1).unwrap_or(0)
            };

            // Find closing tag
            let closing_tag = "</｜DSML｜parameter>";
            let Some(value_end) = content[value_start..].find(closing_tag) else {
                break;
            };
            let raw_value = content[value_start..value_start + value_end].trim();

            let json_value = if is_string {
                serde_json::Value::String(raw_value.to_string())
            } else {
                serde_json::from_str(raw_value).unwrap_or_else(|_| serde_json::Value::String(raw_value.to_string()))
            };
            args.insert(param_name.to_string(), json_value);

            param_pos = value_start + value_end + closing_tag.len();
        }

        calls.push(ToolCall {
            id: format!("dsml-{}", calls.len()),
            name: tool_name.to_string(),
            arguments: serde_json::Value::Object(args),
        });

        // Move past this invoke block
        if let Some(end) = content[param_pos..].find("</｜DSML｜invoke>") {
            pos = param_pos + end + "</｜DSML｜invoke>".len();
        } else {
            break;
        }
    }
    calls
}

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
        preflight_search(
            db_pools,
            config,
            &mut messages,
            &mut collected_sources,
            selected_doc_uuid,
            scope_to_selected_document,
            tx,
        )
        .await?;

        for _round in 0..config.agent_max_tool_rounds {
            let payload =
                super::request_chat_payload(config, &messages, Some(tool_definitions()), false)
                    .await?;
            let mut message = payload
                .get("choices")
                .and_then(|value| value.get(0))
                .and_then(|value| value.get("message"))
                .cloned()
                .ok_or_else(|| {
                    "AI provider response does not contain an assistant message.".to_string()
                })?;

            let tool_calls = extract_tool_calls(&message);
            let tool_calls = if tool_calls.is_empty() {
                // Try parsing DSML format from content (DeepSeek models)
                if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
                    parse_dsml_tool_calls(content)
                } else {
                    Vec::new()
                }
            } else {
                tool_calls
            };
            if tool_calls.is_empty() {
                let content = extract_message_content(&payload).ok_or_else(|| {
                    "AI provider response does not contain an assistant message.".to_string()
                })?;
                return Ok((content, collected_sources));
            }

            // If we parsed DSML, inject tool_calls into message for proper format
            if message.get("tool_calls").is_none() && !tool_calls.is_empty() {
                let tc_json: Vec<serde_json::Value> = tool_calls
                    .iter()
                    .map(|tc| {
                        json!({
                            "id": tc.id,
                            "type": "function",
                            "function": {
                                "name": tc.name,
                                "arguments": tc.arguments.to_string()
                            }
                        })
                    })
                    .collect();
                message["tool_calls"] = json!(tc_json);
                // Strip DSML from content
                if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
                    if content.contains("<｜DSML｜") {
                        let cleaned = content
                            .split("<｜DSML｜")
                            .next()
                            .unwrap_or(content)
                            .trim()
                            .to_string();
                        message["content"] = json!(cleaned);
                    }
                }
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
        preflight_search(
            db_pools,
            config,
            &mut messages,
            &mut collected_sources,
            selected_doc_uuid,
            scope_to_selected_document,
            Some(tx),
        )
        .await?;
        if !collected_sources.is_empty() {
            send_sse(
                tx,
                "sources",
                &serde_json::to_value(&collected_sources).map_err(|e| e.to_string())?,
            )
            .await?;
        }

        for _round in 0..config.agent_max_tool_rounds {
            let payload =
                super::request_chat_payload(config, &messages, Some(tool_definitions()), false)
                    .await?;
            let mut message = payload
                .get("choices")
                .and_then(|value| value.get(0))
                .and_then(|value| value.get("message"))
                .cloned()
                .ok_or_else(|| {
                    "AI provider response does not contain an assistant message.".to_string()
                })?;

            let tool_calls = extract_tool_calls(&message);
            let tool_calls = if tool_calls.is_empty() {
                // Try parsing DSML format from content (DeepSeek models)
                if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
                    parse_dsml_tool_calls(content)
                } else {
                    Vec::new()
                }
            } else {
                tool_calls
            };
            if tool_calls.is_empty() {
                break;
            }

            // If we parsed DSML, inject tool_calls into message for proper format
            if message.get("tool_calls").is_none() && !tool_calls.is_empty() {
                let tc_json: Vec<serde_json::Value> = tool_calls
                    .iter()
                    .map(|tc| {
                        json!({
                            "id": tc.id,
                            "type": "function",
                            "function": {
                                "name": tc.name,
                                "arguments": tc.arguments.to_string()
                            }
                        })
                    })
                    .collect();
                message["tool_calls"] = json!(tc_json);
                if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
                    if content.contains("<｜DSML｜") {
                        let cleaned = content
                            .split("<｜DSML｜")
                            .next()
                            .unwrap_or(content)
                            .trim()
                            .to_string();
                        message["content"] = json!(cleaned);
                    }
                }
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
                    send_sse(
                        tx,
                        "sources",
                        &serde_json::to_value(&collected_sources).map_err(|e| e.to_string())?,
                    )
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

async fn preflight_search(
    db_pools: &crate::db::DbPools,
    config: &AiConfig,
    messages: &mut Vec<serde_json::Value>,
    collected_sources: &mut Vec<ChatSource>,
    selected_doc_uuid: Option<&str>,
    scope_to_selected_document: bool,
    tx: Option<&tokio::sync::mpsc::Sender<String>>,
) -> Result<(), String> {
    let Some(query) = latest_user_message(messages) else {
        return Ok(());
    };

    if let Some(tx) = tx {
        send_sse(
            tx,
            "tool",
            &json!({
                "name": "search_text",
                "label": human_tool_label("search_text")
            }),
        )
        .await?;
    }

    let mut args = json!({
        "query": query,
        "final_top_k": config.rag_top_k
    });
    if scope_to_selected_document {
        if let Some(doc_uuid) = selected_doc_uuid {
            args["doc_uuid"] = json!(doc_uuid);
        }
    }

    let search_tool = SearchTextTool;
    let bytes = search_tool.handle(args, db_pools, None).await?;
    let text = String::from_utf8_lossy(&bytes).to_string();
    let mut sources = parse_search_sources(&text)?;
    collected_sources.append(&mut sources);

    messages.push(json!({
        "role": "system",
        "content": build_preflight_sources_context(&text)
    }));

    Ok(())
}

fn latest_user_message(messages: &[serde_json::Value]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|message| message.get("role").and_then(|role| role.as_str()) == Some("user"))
        .and_then(|message| message.get("content").and_then(|content| content.as_str()))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToString::to_string)
}

fn build_preflight_sources_context(search_response_json: &str) -> String {
    format!(
        "Mandatory local RAG search was executed before answering. Use only the retrieved local-library passages below as evidence for document questions. If they do not contain enough evidence, say that the local library context is insufficient and do not answer from pretrained knowledge.\n\nRaw search_text response:\n{}",
        search_response_json
    )
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
            Some(ToolCall {
                id,
                name,
                arguments,
            })
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
        _ => return Ok((format!("Unsupported tool: {}", tool_call.name), Vec::new())),
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
                "description": "Search the local RAG document library for relevant passages. Use filters for specific chapter/section constraints.",
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
                        },
                        "filters": {
                            "type": "array",
                            "description": "Metadata filters to apply to search. For chapter search: first get metadata to inspect exact chapter_id values, then pass a filter such as [{\"field\": \"chapter_id\", \"operator\": \"==\", \"value\": \"27\"}].",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "field": {
                                        "type": "string",
                                        "enum": ["chapter_id", "doc_uuid", "chunk_level", "chunk_index"]
                                    },
                                    "operator": {
                                        "type": "string",
                                        "enum": ["==", "!=", ">", ">=", "<", "<=", "in"]
                                    },
                                    "value": {}
                                },
                                "required": ["field", "operator", "value"]
                            }
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
