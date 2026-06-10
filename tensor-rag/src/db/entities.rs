use qdrant_client::qdrant::{PointStruct, UpsertPoints};
use sqlx::Row;
use std::collections::HashMap;
use super::DbPools;

/// A unique entity with its metadata.
#[derive(Debug, Clone)]
pub struct EntityRecord {
    pub id: i64,
    pub doc_uuid: String,
    pub text: String,
    pub cluster_id: Option<i32>,
    pub cluster_distance: Option<f32>,
    pub layout_hint: Option<[f32; 3]>,
}

/// Insert or get unique entities for a document, returning a map of entity_text -> entity_id.
/// Uses ON CONFLICT to deduplicate entities within a document.
pub async fn upsert_entities(
    pools: &DbPools,
    doc_uuid: &str,
    entity_texts: &[String],
) -> Result<HashMap<String, i64>, String> {
    let pool = pools
        .postgres
        .as_ref()
        .ok_or("PostgreSQL pool not initialized")?;

    // Deduplicate input
    let unique_texts: Vec<&String> = entity_texts
        .iter()
        .filter(|t| !t.is_empty())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    if unique_texts.is_empty() {
        return Ok(HashMap::new());
    }

    // Fetch existing entities for this document
    let existing_rows = sqlx::query("SELECT id, text FROM entities WHERE doc_uuid = $1")
        .bind(doc_uuid)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to fetch existing entities: {}", e))?;

    let mut map: HashMap<String, i64> = existing_rows
        .into_iter()
        .filter_map(|row| {
            let id: i64 = row.try_get("id").ok()?;
            let text: String = row.try_get("text").ok()?;
            Some((text, id))
        })
        .collect();

    // Insert only new entities
    for text in unique_texts {
        if map.contains_key(text.as_str()) {
            continue;
        }
        let row = sqlx::query(
            "INSERT INTO entities (doc_uuid, text) VALUES ($1, $2)
             ON CONFLICT (doc_uuid, text) DO UPDATE SET text = EXCLUDED.text
             RETURNING id",
        )
        .bind(doc_uuid)
        .bind(text.as_str())
        .fetch_one(pool)
        .await
        .map_err(|e| format!("Failed to upsert entity '{}': {}", text, e))?;

        let id: i64 = row.try_get("id").map_err(|e| format!("Failed to get entity id: {}", e))?;
        map.insert(text.clone(), id);
    }

    Ok(map)
}

/// Create chunk-entity links for a single chunk.
pub async fn link_chunk_entities(
    pools: &DbPools,
    chunk_id: i64,
    entity_ids: &[i64],
) -> Result<(), String> {
    let pool = pools
        .postgres
        .as_ref()
        .ok_or("PostgreSQL pool not initialized")?;

    if entity_ids.is_empty() {
        return Ok(());
    }

    // Batch insert usingunnest
    let chunk_ids: Vec<i64> = vec![chunk_id; entity_ids.len()];
    let _ = sqlx::query(
        "INSERT INTO chunk_entities (chunk_id, entity_id)
         SELECT unnest($1::bigint[]), unnest($2::bigint[])
         ON CONFLICT DO NOTHING",
    )
    .bind(&chunk_ids)
    .bind(entity_ids)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to batch link chunk {} to entities: {}", chunk_id, e))?;

    Ok(())
}

/// Embed entities and upsert them into Qdrant.
pub async fn embed_and_store_entities(
    pools: &DbPools,
    doc_uuid: &str,
    entities: &[(i64, String)],
) -> Result<(), String> {
    if entities.is_empty() {
        return Ok(());
    }

    let client = pools
        .qdrant_clients
        .get("default")
        .ok_or("Qdrant client not initialized")?;
    let entities_collection = crate::config::get_qdrant_entities_collection();

    // Embed entity texts in batches using index-based chunking
    let batch_size = crate::config::get_embedding_batch_size().max(1);
    let total = entities.len();

    for start in (0..total).step_by(batch_size) {
        let end = (start + batch_size).min(total);
        let batch = &entities[start..end];
        let texts: Vec<String> = batch.iter().map(|(_, text)| text.clone()).collect();

        let vectors = pools
            .get_embeddings(&texts, false)
            .await
            .map_err(|e| format!("Failed to embed entities: {}", e))?;

        let points: Vec<PointStruct> = batch
            .iter()
            .zip(vectors.iter())
            .map(|((entity_id, entity_text), vector)| {
                let mut payload = std::collections::HashMap::new();
                payload.insert(
                    "doc_uuid".to_string(),
                    serde_json::Value::String(doc_uuid.to_string()),
                );
                payload.insert(
                    "entity_text".to_string(),
                    serde_json::Value::String(entity_text.clone()),
                );
                PointStruct::new(*entity_id as u64, vector.to_vec(), payload)
            })
            .collect();

        client
            .upsert_points(UpsertPoints {
                collection_name: entities_collection.clone(),
                points,
                wait: Some(crate::config::get_qdrant_upsert_wait()),
                ..Default::default()
            })
            .await
            .map_err(|e| format!("Qdrant entity upsert failed: {}", e))?;
    }

    Ok(())
}

/// Update entity cluster assignments and layout hints in the database.
pub async fn update_entity_layout(
    pools: &DbPools,
    doc_uuid: &str,
    layout_points: &[(i64, i32, f32, [f32; 3])],
) -> Result<(), String> {
    let pool = pools
        .postgres
        .as_ref()
        .ok_or("PostgreSQL pool not initialized")?;

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("Failed to start entity layout transaction: {}", e))?;

    for (entity_id, cluster_id, distance, layout_hint) in layout_points {
        sqlx::query(
            "UPDATE entities
             SET cluster_id = $1,
                 cluster_distance = $2,
                 layout_hint = $3
             WHERE id = $4 AND doc_uuid = $5",
        )
        .bind(cluster_id)
        .bind(distance)
        .bind(serde_json::json!(layout_hint))
        .bind(entity_id)
        .bind(doc_uuid)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Failed to update entity layout: {}", e))?;
    }

    tx.commit()
        .await
        .map_err(|e| format!("Failed to commit entity layout: {}", e))?;
    Ok(())
}

/// Delete all entity data for a document (entities + chunk_entities + Qdrant points).
pub async fn delete_document_entities(pools: &DbPools, doc_uuid: &str) -> Result<(), String> {
    let pool = pools
        .postgres
        .as_ref()
        .ok_or("PostgreSQL pool not initialized")?;

    // Get entity IDs before deleting (for Qdrant cleanup)
    let entity_ids: Vec<i64> = sqlx::query("SELECT id FROM entities WHERE doc_uuid = $1")
        .bind(doc_uuid)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to fetch entity IDs for deletion: {}", e))?
        .iter()
        .filter_map(|row| row.try_get("id").ok())
        .collect();

    // Delete chunk_entities and entities (CASCADE handles chunk_entities)
    let _ = sqlx::query("DELETE FROM entities WHERE doc_uuid = $1")
        .bind(doc_uuid)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to delete entities: {}", e))?;

    // Delete from Qdrant
    if !entity_ids.is_empty() {
        if let Some(client) = pools.qdrant_clients.get("default") {
            let entities_collection = crate::config::get_qdrant_entities_collection();
            let point_ids: Vec<qdrant_client::qdrant::PointId> = entity_ids
                .iter()
                .map(|&id| qdrant_client::qdrant::PointId {
                    point_id_options: Some(
                        qdrant_client::qdrant::point_id::PointIdOptions::Num(id as u64),
                    ),
                })
                .collect();

            let _ = client
                .delete_points(qdrant_client::qdrant::DeletePoints {
                    collection_name: entities_collection,
                    points: Some(qdrant_client::qdrant::PointsSelector {
                        points_selector_one_of: Some(
                            qdrant_client::qdrant::points_selector::PointsSelectorOneOf::Points(
                                qdrant_client::qdrant::PointsIdsList { ids: point_ids },
                            ),
                        ),
                    }),
                    ..Default::default()
                })
                .await;
        }
    }

    Ok(())
}

/// Fetch all entities for a document from the database.
pub async fn fetch_document_entities(
    pools: &DbPools,
    doc_uuid: &str,
) -> Result<Vec<EntityRecord>, String> {
    let pool = pools
        .postgres
        .as_ref()
        .ok_or("PostgreSQL pool not initialized")?;

    let rows = sqlx::query(
        "SELECT id, doc_uuid, text, cluster_id, cluster_distance, layout_hint
         FROM entities WHERE doc_uuid = $1 ORDER BY id",
    )
    .bind(doc_uuid)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch entities: {}", e))?;

    let mut entities = Vec::new();
    for row in rows {
        let id: i64 = row.try_get("id").unwrap_or(0);
        let text: String = row.try_get("text").unwrap_or_default();
        let cluster_id: Option<i32> = row.try_get("cluster_id").ok();
        let cluster_distance: Option<f32> = row.try_get("cluster_distance").ok();
        let layout_hint: Option<[f32; 3]> = row
            .try_get::<Option<serde_json::Value>, _>("layout_hint")
            .ok()
            .flatten()
            .and_then(|v| serde_json::from_value(v).ok());

        entities.push(EntityRecord {
            id,
            doc_uuid: doc_uuid.to_string(),
            text,
            cluster_id,
            cluster_distance,
            layout_hint,
        });
    }
    Ok(entities)
}

/// Collect unique entities from all chunks of a document and return deduplicated list.
pub fn collect_unique_entities(chunk_entities_list: &[Vec<String>]) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    for entities in chunk_entities_list {
        for entity in entities {
            if !entity.is_empty() {
                seen.insert(entity.clone());
            }
        }
    }
    seen.into_iter().collect()
}
