use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use serde_json::Value as JsonValue;
use super::ToolHandler;
use crate::mcp::executor::execute_program;

pub struct ExecTool;

impl ToolHandler for ExecTool {
    fn handle<'a>(
        &'a self,
        args: JsonValue,
        db_pools: &'a crate::db::DbPools,
        _metrics: Option<Arc<crate::metrics::Metrics>>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send + 'a>> {
        Box::pin(async move {
            let dsl_string = args
                .get("dsl")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required parameter 'dsl'".to_string())?;

            let mut ast = crate::dsl::parse_dsl(dsl_string)?;

            match &mut ast {
                crate::ir::AstNode::Search(req) => {
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

            let result = execute_program(
                &program,
                &mut vm,
                db_pools,
                &mut 0,
                crate::config::get_vm_max_instructions(),
            )
            .await?;

            let value = result.unwrap_or(crate::vm::Value::Scalar(0.0));
            let response = serde_json::json!({
                "result": match value {
                    crate::vm::Value::Scalar(s) => serde_json::Value::Number(serde_json::Number::from_f64(s as f64).unwrap()),
                    crate::vm::Value::Tensor1D(t) => serde_json::json!(t.to_vec()),
                    _ => serde_json::json!("success")
                }
            });

            Ok(serde_json::to_vec(&response).unwrap())
        })
    }
}
