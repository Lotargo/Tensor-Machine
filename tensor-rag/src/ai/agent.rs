use crate::mcp::{
    GetDocumentMetadataTool, HelpTool, ListDocumentsTool, ReadDocumentTool, SearchTextTool,
    ToolHandler,
};
use once_cell::sync::Lazy;
use regex::{Captures, Regex};
use serde_json::json;

use super::{
    AiConfig, ChatDoneEvent, ChatSource, extract_message_content, parse_search_sources, send_sse,
};

/// Parse DSML tool call tokens from content text.
/// DeepSeek models may output: <｜DSML｜tool_calls><｜DSML｜invoke name="search_text">...
fn parse_dsml_tool_calls(content: &str) -> Vec<ToolCall> {
    static INVOKE_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"(?s)<DSML:invoke\s+[^>]*\bname\s*=\s*"([^"]+)"[^>]*>(.*?)</DSML:invoke>"#)
            .expect("valid DSML invoke regex")
    });
    static PARAM_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"(?s)<DSML:parameter\s+([^>]*)>(.*?)</DSML:parameter>"#)
            .expect("valid DSML parameter regex")
    });

    let content = canonicalize_dsml_markup(content);
    let mut calls = Vec::new();
    for invoke in INVOKE_RE.captures_iter(&content) {
        let tool_name = invoke
            .get(1)
            .map(|value| value.as_str().trim())
            .unwrap_or_default();
        if tool_name.is_empty() {
            continue;
        }

        let body = invoke
            .get(2)
            .map(|value| value.as_str())
            .unwrap_or_default();
        let mut args = serde_json::Map::new();

        for parameter in PARAM_RE.captures_iter(body) {
            let attrs = parameter
                .get(1)
                .map(|value| value.as_str())
                .unwrap_or_default();
            let Some(param_name) = dsml_attr(attrs, "name") else {
                continue;
            };
            let raw_value = parameter
                .get(2)
                .map(|value| value.as_str().trim())
                .unwrap_or_default();
            let is_string = dsml_attr(attrs, "string").as_deref() == Some("true");

            let json_value = if is_string {
                serde_json::Value::String(raw_value.to_string())
            } else {
                serde_json::from_str(raw_value)
                    .unwrap_or_else(|_| serde_json::Value::String(raw_value.to_string()))
            };
            args.insert(param_name, json_value);
        }

        calls.push(ToolCall {
            id: format!("dsml-{}", calls.len()),
            name: tool_name.to_string(),
            arguments: serde_json::Value::Object(args),
        });
    }
    calls
}

fn canonicalize_dsml_markup(content: &str) -> String {
    static DSML_TAG_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"<\s*(/?)\s*[|｜]\s*DSML\s*[|｜]\s*(?:[|｜]\s*)?([^>]*)>"#)
            .expect("valid DSML tag regex")
    });

    DSML_TAG_RE
        .replace_all(content, |caps: &Captures| {
            let slash = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
            let body = caps.get(2).map(|m| m.as_str().trim()).unwrap_or_default();
            format!("<{}DSML:{}>", slash, body)
        })
        .into_owned()
}

fn contains_dsml_markup(content: &str) -> bool {
    content.contains("<｜DSML｜") || content.contains("<|DSML|") || content.contains("< | DSML |")
}

fn strip_dsml_tool_markup(content: &str) -> String {
    static INVOKE_BLOCK_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"(?s)<DSML:invoke\b[^>]*>.*?</DSML:invoke>"#).expect("valid DSML block regex")
    });
    static TOOL_CALLS_TAG_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"(?s)</?DSML:tool_calls[^>]*>"#).expect("valid DSML tool_calls regex")
    });

    let canonical = canonicalize_dsml_markup(content);
    let without_invokes = INVOKE_BLOCK_RE.replace_all(&canonical, "");
    TOOL_CALLS_TAG_RE
        .replace_all(&without_invokes, "")
        .trim()
        .to_string()
}

fn sanitize_final_answer(content: String) -> String {
    if !contains_dsml_markup(&content) {
        return content;
    }

    let cleaned = strip_dsml_tool_markup(&content);
    if cleaned.is_empty() {
        "The model returned tool-call markup instead of a final answer; the raw tool call was suppressed. Please retry the question.".to_string()
    } else {
        cleaned
    }
}

fn dsml_attr(attrs: &str, name: &str) -> Option<String> {
    static ATTR_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"([A-Za-z0-9_-]+)\s*=\s*"([^"]*)""#).expect("valid attr regex"));

    ATTR_RE.captures_iter(attrs).find_map(|capture| {
        let attr_name = capture.get(1)?.as_str();
        if attr_name == name {
            capture.get(2).map(|value| value.as_str().to_string())
        } else {
            None
        }
    })
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

            let native_tool_calls = extract_tool_calls(&message);
            let (tool_calls, parsed_dsml_tool_calls) = if native_tool_calls.is_empty() {
                // Try parsing DSML format from content (DeepSeek models)
                if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
                    let parsed = parse_dsml_tool_calls(content);
                    let parsed_dsml = !parsed.is_empty();
                    (parsed, parsed_dsml)
                } else {
                    (Vec::new(), false)
                }
            } else {
                (native_tool_calls, false)
            };
            if tool_calls.is_empty() {
                let content = extract_message_content(&payload).ok_or_else(|| {
                    "AI provider response does not contain an assistant message.".to_string()
                })?;
                return Ok((sanitize_final_answer(content), collected_sources));
            }

            // If we parsed DSML, inject tool_calls into message for proper format
            if parsed_dsml_tool_calls {
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
                    message["content"] = json!(strip_dsml_tool_markup(content));
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
                assign_global_source_indices(&mut tool_sources, collected_sources.len());
                let tool_content = if tool_sources.is_empty() {
                    tool_response
                } else {
                    build_numbered_tool_sources_context(&tool_sources)
                };
                collected_sources.append(&mut tool_sources);
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": tool_call.id,
                    "content": tool_content
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

            let native_tool_calls = extract_tool_calls(&message);
            let (tool_calls, parsed_dsml_tool_calls) = if native_tool_calls.is_empty() {
                // Try parsing DSML format from content (DeepSeek models)
                if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
                    let parsed = parse_dsml_tool_calls(content);
                    let parsed_dsml = !parsed.is_empty();
                    (parsed, parsed_dsml)
                } else {
                    (Vec::new(), false)
                }
            } else {
                (native_tool_calls, false)
            };
            if tool_calls.is_empty() {
                break;
            }

            // If we parsed DSML, inject tool_calls into message for proper format
            if parsed_dsml_tool_calls {
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
                    message["content"] = json!(strip_dsml_tool_markup(content));
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
                let tool_content;
                if !tool_sources.is_empty() {
                    assign_global_source_indices(&mut tool_sources, collected_sources.len());
                    tool_content = build_numbered_tool_sources_context(&tool_sources);
                    collected_sources.append(&mut tool_sources);
                    send_sse(
                        tx,
                        "sources",
                        &serde_json::to_value(&collected_sources).map_err(|e| e.to_string())?,
                    )
                    .await?;
                } else {
                    tool_content = tool_response;
                }
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": tool_call.id,
                    "content": tool_content
                }));
            }
        }
    }

    let content = super::request_completion_from_messages(config, &messages).await?;
    send_sse(
        tx,
        "token",
        &json!({ "delta": sanitize_final_answer(content) }),
    )
    .await?;
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
    assign_global_source_indices(&mut sources, collected_sources.len());
    let preflight_context = build_preflight_sources_context(&sources, &text);
    collected_sources.append(&mut sources);

    messages.push(json!({
        "role": "system",
        "content": preflight_context
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

fn build_preflight_sources_context(sources: &[ChatSource], search_response_json: &str) -> String {
    if sources.is_empty() {
        return format!(
            "Mandatory local RAG search was executed before answering, but it returned no visible sources. If the local library context is insufficient, say so explicitly.\n\nRaw search_text response:\n{}",
            search_response_json
        );
    }

    format!(
        "Mandatory local RAG search was executed before answering. Use only the globally numbered local-library sources below as evidence for document questions. Cite the exact global source IDs shown here, e.g. [2] or [3]. Do not restart numbering for later searches. If these sources do not contain enough evidence, say that the local library context is insufficient and do not answer from pretrained knowledge.\n\n{}",
        build_numbered_sources_context(sources)
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

fn assign_global_source_indices(sources: &mut [ChatSource], existing_count: usize) {
    for (offset, source) in sources.iter_mut().enumerate() {
        source.index = existing_count + offset + 1;
    }
}

fn build_numbered_tool_sources_context(sources: &[ChatSource]) -> String {
    format!(
        "search_text returned the following globally numbered sources. Cite these exact global source IDs in the final answer; do not renumber this search from [1].\n\n{}",
        build_numbered_sources_context(sources)
    )
}

fn build_numbered_sources_context(sources: &[ChatSource]) -> String {
    sources
        .iter()
        .map(|source| {
            let mut parts = vec![format!("[{}] {}", source.index, source.title)];
            if let Some(context) = source.context.as_ref().filter(|value| !value.trim().is_empty())
            {
                parts.push(format!("Context: {}", context.trim()));
            }
            parts.push(source.excerpt.trim().to_string());
            parts.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn dedupe_sources(sources: Vec<ChatSource>) -> Vec<ChatSource> {
    let mut seen = std::collections::HashSet::new();
    let mut deduped = Vec::new();

    for source in sources {
        let key = format!("{}::{}", source.title, source.excerpt);
        if seen.insert(key) {
            deduped.push(source);
        }
    }

    deduped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fullwidth_dsml_tool_call() {
        let content = r#"<｜DSML｜tool_calls><｜DSML｜invoke name="search_text"><｜DSML｜parameter name="doc_uuid" string="true">73ebd244-f30c-447f-9784-0096c2c1f2b3</｜DSML｜parameter><｜DSML｜parameter name="query" string="true">Глава 23</｜DSML｜parameter><｜DSML｜parameter name="final_top_k" string="false">5</｜DSML｜parameter></｜DSML｜invoke></｜DSML｜tool_calls>"#;

        let calls = parse_dsml_tool_calls(content);

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "search_text");
        assert_eq!(calls[0].arguments["query"], "Глава 23");
        assert_eq!(calls[0].arguments["final_top_k"], 5);
    }

    #[test]
    fn parses_ascii_spaced_dsml_tool_call() {
        let content = r#"< | DSML | | tool_calls>
< | DSML | | invoke name="search_text">
< | DSML | | parameter name="query" string="true">Глава 23</ | DSML | | parameter>
< | DSML | | parameter name="final_top_k" string="false">5</ | DSML | | parameter>
</ | DSML | | invoke>
</ | DSML | | tool_calls>"#;

        let calls = parse_dsml_tool_calls(content);

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "search_text");
        assert_eq!(calls[0].arguments["query"], "Глава 23");
        assert_eq!(calls[0].arguments["final_top_k"], 5);
    }

    #[test]
    fn strips_dsml_from_assistant_content() {
        let content = r#"Сейчас поищу.
<｜DSML｜tool_calls><｜DSML｜invoke name="search_text"><｜DSML｜parameter name="query" string="true">test</｜DSML｜parameter></｜DSML｜invoke></｜DSML｜tool_calls>"#;

        assert_eq!(strip_dsml_tool_markup(content), "Сейчас поищу.");
    }

    #[test]
    fn assigns_global_source_indices_across_searches() {
        let mut first_search = vec![
            test_source(1, "doc", "first"),
            test_source(2, "doc", "second"),
        ];
        let mut second_search = vec![
            test_source(1, "doc", "third"),
            test_source(2, "doc", "fourth"),
        ];

        assign_global_source_indices(&mut first_search, 0);
        assign_global_source_indices(&mut second_search, first_search.len());

        assert_eq!(first_search[0].index, 1);
        assert_eq!(first_search[1].index, 2);
        assert_eq!(second_search[0].index, 3);
        assert_eq!(second_search[1].index, 4);

        let context = build_numbered_tool_sources_context(&second_search);
        assert!(context.contains("[3] doc"));
        assert!(context.contains("[4] doc"));
        assert!(context.contains("do not renumber"));
    }

    #[test]
    fn dedupe_sources_preserves_assigned_indices() {
        let sources = vec![
            test_source(1, "doc", "same"),
            test_source(2, "doc", "same"),
            test_source(3, "doc", "different"),
        ];

        let deduped = dedupe_sources(sources);

        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0].index, 1);
        assert_eq!(deduped[1].index, 3);
    }

    fn test_source(index: usize, title: &str, excerpt: &str) -> ChatSource {
        ChatSource {
            index,
            title: title.to_string(),
            context: None,
            excerpt: excerpt.to_string(),
            score: None,
        }
    }
}
