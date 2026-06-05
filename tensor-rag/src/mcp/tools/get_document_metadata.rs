use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use serde_json::Value as JsonValue;
use super::ToolHandler;

pub struct GetDocumentMetadataTool;

impl ToolHandler for GetDocumentMetadataTool {
    fn handle<'a>(
        &'a self,
        args: JsonValue,
        db_pools: &'a crate::db::DbPools,
        _metrics: Option<Arc<crate::metrics::Metrics>>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send + 'a>> {
        Box::pin(async move {
            let doc_uuid = args
                .get("doc_uuid")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required parameter 'doc_uuid'".to_string())?;

            let resolved_uuid = db_pools
                .resolve_doc_uuid(doc_uuid)
                .await
                .ok_or_else(|| format!("Could not resolve document: {}", doc_uuid))?;

            match db_pools
                .fetch_document_metadata_by_uuid(&resolved_uuid)
                .await?
            {
                Some(meta) => {
                    let formatted = serde_json::to_string_pretty(&meta)
                        .map_err(|e| format!("Failed to format response: {}", e))?;
                    Ok(formatted.into_bytes())
                }
                None => Err(format!(
                    "Document metadata not found for UUID: {}",
                    resolved_uuid
                )),
            }
        })
    }
}
