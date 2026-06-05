use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use serde_json::Value as JsonValue;
use super::ToolHandler;
use crate::mcp::ipc::VmIpcClient;
use crate::mcp::executor::execute_program;

pub struct SearchTool;

impl ToolHandler for SearchTool {
    fn handle<'a>(
        &'a self,
        args: JsonValue,
        db_pools: &'a crate::db::DbPools,
        metrics: Option<Arc<crate::metrics::Metrics>>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send + 'a>> {
        Box::pin(async move {
            let mut ast = if let Some(dsl_string) = args.get("dsl").and_then(|v| v.as_str()) {
                crate::dsl::parse_dsl(dsl_string)?
            } else {
                // For JSON fallback, wrap QueryRequest inside AstNode::Search
                let req: crate::ir::QueryRequest = serde_json::from_value(args)
                    .map_err(|e| format!("Failed to parse QueryRequest JSON: {}", e))?;
                crate::ir::AstNode::Search(req)
            };

            match &mut ast {
                crate::ir::AstNode::Search(req) => {
                    let (real_query, _history) = crate::response::prune_query_history(&req.query);
                    req.query = real_query;
                    if let Some(ref uuid_or_file) = req.doc_uuid {
                        if let Some(resolved) = db_pools.resolve_doc_uuid(uuid_or_file).await {
                            req.doc_uuid = Some(resolved);
                        }
                    }
                }
                crate::ir::AstNode::AddMetadata { doc_uuid, .. } => {
                    if let Some(resolved) = db_pools.resolve_doc_uuid(doc_uuid).await {
                        *doc_uuid = resolved;
                    }
                }
                crate::ir::AstNode::AddAnchor { doc_uuid, .. } => {
                    if let Some(resolved) = db_pools.resolve_doc_uuid(doc_uuid).await {
                        *doc_uuid = resolved;
                    }
                }
                _ => {}
            }

            let program = crate::ir::Planner::compile(ast);
            let mut vm = crate::vm::VM::new();
            let _ = vm.init_gpu().await;

            let mut instruction_count = 0;
            let max_instructions = crate::config::get_vm_max_instructions();

            let result = if std::env::var("TENSOR_VM_MODE").unwrap_or_default() == "ipc" {
                let client = VmIpcClient::from_config();
                client.execute_program("default_session", 0, program, vm.query_vector.to_vec())
            } else {
                execute_program(
                    &program,
                    &mut vm,
                    db_pools,
                    &mut instruction_count,
                    max_instructions,
                )
                .await
                .map_err(|e| format!("Execution error: {}", e))
            }?;

            match result {
                Some(crate::vm::Value::Result(topk)) => {
                    let mut fbb = flatbuffers::FlatBufferBuilder::new();

                    // Note: The indices are cast to i32 to match the FlatBuffers schema ([int]).
                    // This is safe for now, but will silently truncate if the index exceeds 2^31-1.
                    let indices_i32: Vec<i32> =
                        topk.indices.iter().map(|&idx| idx as i32).collect();

                    let top_indices_vec = fbb.create_vector(&indices_i32);
                    let top_scores_vec = fbb.create_vector(&topk.scores);

                    let args = crate::result_generated::tensor_rag::fbs::SearchResultArgs {
                        top_indices: Some(top_indices_vec),
                        top_scores: Some(top_scores_vec),
                    };

                    let result_offset =
                        crate::result_generated::tensor_rag::fbs::SearchResult::create(
                            &mut fbb, &args,
                        );
                    fbb.finish(result_offset, None);

                    if let Some(m) = &metrics {
                        m.inc_results(topk.indices.len() as u64);
                    }

                    Ok(fbb.finished_data().to_vec())
                }
                _ => {
                    if let Some(m) = &metrics {
                        m.inc_failures();
                    }
                    Err("VM did not return a valid TopKResult".to_string())
                }
            }
        })
    }
}
