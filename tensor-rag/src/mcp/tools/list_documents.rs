use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use serde_json::Value as JsonValue;
use super::ToolHandler;

pub struct ListDocumentsTool;

impl ToolHandler for ListDocumentsTool {
    fn handle<'a>(
        &'a self,
        _args: JsonValue,
        db_pools: &'a crate::db::DbPools,
        _metrics: Option<Arc<crate::metrics::Metrics>>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send + 'a>> {
        Box::pin(async move {
            let pool = db_pools
                .postgres
                .as_ref()
                .ok_or("PostgreSQL pool not initialized")?;

            // Fetch distinct filename and doc_uuid from PostgreSQL
            let rows = sqlx::query(
                "SELECT DISTINCT filename, doc_uuid FROM documents WHERE doc_uuid IS NOT NULL AND doc_uuid <> '' ORDER BY filename"
            )
            .fetch_all(pool)
            .await
            .map_err(|e| format!("Database error: {}", e))?;

            use sqlx::Row;
            let mut docs = Vec::new();
            for row in rows {
                let filename: String = row
                    .try_get("filename")
                    .unwrap_or_else(|_| "Unknown".to_string());
                let doc_uuid: String = row.try_get("doc_uuid").unwrap_or_default();
                docs.push(serde_json::json!({
                    "filename": filename,
                    "doc_uuid": doc_uuid
                }));
            }

            let response_json = serde_json::json!({
                "documents": docs
            });

            Ok(serde_json::to_vec(&response_json).unwrap())
        })
    }
}
