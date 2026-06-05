use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use serde_json::Value as JsonValue;
use super::ToolHandler;

pub struct ReadDocumentTool;

impl ToolHandler for ReadDocumentTool {
    fn handle<'a>(
        &'a self,
        args: JsonValue,
        db_pools: &'a crate::db::DbPools,
        _metrics: Option<Arc<crate::metrics::Metrics>>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send + 'a>> {
        Box::pin(async move {
            let doc_uuid = args
                .get("doc_uuid")
                .and_then(|value| value.as_str())
                .ok_or("Missing required argument: doc_uuid")?;

            let document = db_pools
                .fetch_document_by_uuid(doc_uuid)
                .await?
                .ok_or_else(|| format!("Document not found for doc_uuid={}", doc_uuid))?;

            let token_count = crate::chunker::count_tokens(&document.text);
            let token_limit = crate::config::get_read_document_max_tokens();

            let response_json = if token_count > token_limit {
                serde_json::json!({
                    "doc_uuid": document.doc_uuid,
                    "filename": document.filename,
                    "token_count": token_count,
                    "token_limit": token_limit,
                    "warning": format!(
                        "Document exceeds the read limit: {} tokens > {} tokens. Full document read is blocked.",
                        token_count,
                        token_limit
                    ),
                    "content": serde_json::Value::Null
                })
            } else {
                serde_json::json!({
                    "doc_uuid": document.doc_uuid,
                    "filename": document.filename,
                    "token_count": token_count,
                    "token_limit": token_limit,
                    "warning": serde_json::Value::Null,
                    "content": document.text
                })
            };

            Ok(serde_json::to_vec(&response_json).unwrap())
        })
    }
}
