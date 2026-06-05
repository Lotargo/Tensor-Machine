use sqlx::Row;
use super::DbPools;

pub async fn insert_edge(
    pools: &DbPools,
    source: &str,
    target: &str,
    edge_type: &str,
    metadata: Option<serde_json::Value>,
) -> Result<i64, String> {
    let pool = pools
        .postgres
        .as_ref()
        .ok_or("PostgreSQL pool not initialized")?;

    let meta = metadata.unwrap_or(serde_json::json!({}));
    let row = sqlx::query(
        "INSERT INTO edges (source_doc_uuid, target_doc_uuid, edge_type, metadata) VALUES ($1, $2, $3, $4) RETURNING id"
    )
    .bind(source)
    .bind(target)
    .bind(edge_type)
    .bind(&meta)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to insert edge: {}", e))?;

    row.try_get("id")
        .map_err(|e| format!("Failed to get edge id: {}", e))
}

pub async fn fetch_edges_by_doc_uuid(
    pools: &DbPools,
    doc_uuid: &str,
    edge_type: Option<&str>,
) -> Result<Vec<serde_json::Value>, String> {
    let pool = pools
        .postgres
        .as_ref()
        .ok_or("PostgreSQL pool not initialized")?;

    let rows = if let Some(etype) = edge_type {
        sqlx::query(
            "SELECT source_doc_uuid, target_doc_uuid, edge_type, metadata FROM edges WHERE (source_doc_uuid = $1 OR target_doc_uuid = $1) AND edge_type = $2 ORDER BY id ASC"
        )
        .bind(doc_uuid)
        .bind(etype)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to fetch edges: {}", e))?
    } else {
        sqlx::query(
            "SELECT source_doc_uuid, target_doc_uuid, edge_type, metadata FROM edges WHERE source_doc_uuid = $1 OR target_doc_uuid = $1 ORDER BY id ASC"
        )
        .bind(doc_uuid)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to fetch edges: {}", e))?
    };

    Ok(rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "source": r.try_get::<String, _>("source_doc_uuid").unwrap_or_default(),
                "target": r.try_get::<String, _>("target_doc_uuid").unwrap_or_default(),
                "type": r.try_get::<String, _>("edge_type").unwrap_or_default(),
            })
        })
        .collect())
}
