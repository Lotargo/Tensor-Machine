use sqlx::Row;
use super::DbPools;

pub async fn update_document_metadata(
    pools: &DbPools,
    doc_uuid: &str,
    key: &str,
    value: serde_json::Value,
) -> Result<(), String> {
    let pool = pools
        .postgres
        .as_ref()
        .ok_or("PostgreSQL pool not initialized")?;
    sqlx::query(
        "UPDATE documents SET metadata = jsonb_set(COALESCE(metadata, '{}'::jsonb), $1, $2, true) WHERE doc_uuid = $3"
    )
    .bind(&[key])
    .bind(&value)
    .bind(doc_uuid)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to update metadata: {}", e))?;
    Ok(())
}

pub async fn fetch_document_metadata_by_uuid(
    pools: &DbPools,
    doc_uuid: &str,
) -> Result<Option<serde_json::Value>, String> {
    let pool = pools
        .postgres
        .as_ref()
        .ok_or("PostgreSQL pool not initialized")?;

    // Get document metadata
    let row_opt = sqlx::query(
        "SELECT filename, doc_uuid, metadata FROM documents WHERE doc_uuid = $1 LIMIT 1",
    )
    .bind(doc_uuid)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Failed to fetch document metadata: {}", e))?;

    let row = match row_opt {
        Some(r) => r,
        None => return Ok(None),
    };

    let filename: String = row
        .try_get("filename")
        .unwrap_or_else(|_| "Unknown".to_string());
    let metadata: serde_json::Value = row.try_get("metadata").unwrap_or(serde_json::json!({}));

    // Get anchors
    let anchor_rows =
        sqlx::query("SELECT name, text FROM anchors WHERE doc_uuid = $1 ORDER BY id ASC")
            .bind(doc_uuid)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("Failed to fetch anchors: {}", e))?;

    let anchors: Vec<serde_json::Value> = anchor_rows.iter().map(|r| {
        serde_json::json!({
            "name": r.try_get::<String, _>("name").unwrap_or_default(),
            "text_preview": r.try_get::<String, _>("text").unwrap_or_default().chars().take(200).collect::<String>()
        })
    }).collect();

    // Get edges
    let edge_rows = sqlx::query(
        "SELECT source_doc_uuid, target_doc_uuid, edge_type, metadata FROM edges WHERE source_doc_uuid = $1 OR target_doc_uuid = $1 ORDER BY id ASC"
    )
    .bind(doc_uuid)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch edges: {}", e))?;

    let edges: Vec<serde_json::Value> = edge_rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "source": r.try_get::<String, _>("source_doc_uuid").unwrap_or_default(),
                "target": r.try_get::<String, _>("target_doc_uuid").unwrap_or_default(),
                "type": r.try_get::<String, _>("edge_type").unwrap_or_default(),
            })
        })
        .collect();

    Ok(Some(serde_json::json!({
        "doc_uuid": doc_uuid,
        "filename": filename,
        "metadata": metadata,
        "anchors": anchors,
        "edges": edges
    })))
}
