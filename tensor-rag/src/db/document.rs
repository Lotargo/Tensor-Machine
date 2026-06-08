use super::DbPools;
use super::types::{CollectionHealth, EnrichedDocumentInsert, StoredDocument};
use qdrant_client::Qdrant;
use qdrant_client::qdrant::{
    Filter as QFilter, PointId, PointStruct, PointsIdsList, PointsSelector, UpsertPoints,
    point_id::PointIdOptions, points_selector::PointsSelectorOneOf,
};
use sqlx::{Postgres, QueryBuilder, Row};

pub async fn recreate_default_collection(client: &Qdrant, collection: &str) -> Result<(), String> {
    client
        .create_collection(qdrant_client::qdrant::CreateCollection {
            collection_name: collection.to_string(),
            vectors_config: Some(qdrant_client::qdrant::VectorsConfig {
                config: Some(qdrant_client::qdrant::vectors_config::Config::Params(
                    qdrant_client::qdrant::VectorParams {
                        size: crate::config::get_embedding_dim() as u64,
                        distance: qdrant_client::qdrant::Distance::Cosine.into(),
                        ..Default::default()
                    },
                )),
            }),
            ..Default::default()
        })
        .await
        .map(|_| ())
        .map_err(|e| format!("Qdrant collection recreate failed: {}", e))
}

pub fn extract_vector_dim(info: &qdrant_client::qdrant::CollectionInfo) -> Option<usize> {
    use qdrant_client::qdrant::vectors_config;
    info.config
        .as_ref()?
        .params
        .as_ref()?
        .vectors_config
        .as_ref()?
        .config
        .as_ref()
        .and_then(|config| match config {
            vectors_config::Config::Params(params) => Some(params.size as usize),
            vectors_config::Config::ParamsMap(map) => {
                map.map.values().next().map(|params| params.size as usize)
            }
        })
}

pub async fn insert_document(
    pools: &DbPools,
    text: &str,
    vector: ndarray::Array1<f32>,
    filename: &str,
    doc_uuid: &str,
    chapter_id: &str,
    chunk_index: i32,
) -> Result<usize, String> {
    let ids = insert_documents_enriched_batch(
        pools,
        vec![EnrichedDocumentInsert {
            text,
            vector,
            filename,
            doc_uuid,
            chapter_id,
            chunk_index,
            chunk_level: "medium",
            parent_chunk_index: None,
            macro_index: None,
            medium_index: None,
            header_chain: &[],
            zone_keywords: &[],
            entities: &[],
        }],
    )
    .await?;
    ids.into_iter()
        .next()
        .ok_or_else(|| "Document insert returned no ids".to_string())
}

#[allow(clippy::too_many_arguments)]
pub async fn insert_document_enriched(
    pools: &DbPools,
    text: &str,
    vector: ndarray::Array1<f32>,
    filename: &str,
    doc_uuid: &str,
    chapter_id: &str,
    chunk_index: i32,
    chunk_level: &str,
    parent_chunk_index: Option<i32>,
    macro_index: Option<i32>,
    medium_index: Option<i32>,
    header_chain: &[String],
    zone_keywords: &[String],
    entities: &[String],
) -> Result<usize, String> {
    let ids = insert_documents_enriched_batch(
        pools,
        vec![EnrichedDocumentInsert {
            text,
            vector,
            filename,
            doc_uuid,
            chapter_id,
            chunk_index,
            chunk_level,
            parent_chunk_index,
            macro_index,
            medium_index,
            header_chain,
            zone_keywords,
            entities,
        }],
    )
    .await?;
    ids.into_iter()
        .next()
        .ok_or_else(|| "Document insert returned no ids".to_string())
}

pub fn build_document_payload(
    filename: &str,
    doc_uuid: &str,
    chapter_id: &str,
    chunk_index: i32,
    chunk_level: &str,
    parent_chunk_index: Option<i32>,
    macro_index: Option<i32>,
    medium_index: Option<i32>,
    entities_json: serde_json::Value,
) -> std::collections::HashMap<String, serde_json::Value> {
    let mut payload = std::collections::HashMap::new();
    payload.insert(
        "filename".to_string(),
        serde_json::Value::String(filename.to_string()),
    );
    payload.insert(
        "doc_uuid".to_string(),
        serde_json::Value::String(doc_uuid.to_string()),
    );
    payload.insert(
        "chapter_id".to_string(),
        serde_json::Value::String(chapter_id.to_string()),
    );
    payload.insert(
        "chunk_index".to_string(),
        serde_json::Value::Number(serde_json::Number::from(chunk_index)),
    );
    payload.insert(
        "chunk_level".to_string(),
        serde_json::Value::String(chunk_level.to_string()),
    );
    if let Some(parent) = parent_chunk_index {
        payload.insert(
            "parent_chunk_index".to_string(),
            serde_json::Value::Number(parent.into()),
        );
    }
    if let Some(idx) = macro_index {
        payload.insert(
            "macro_index".to_string(),
            serde_json::Value::Number(idx.into()),
        );
    }
    if let Some(idx) = medium_index {
        payload.insert(
            "medium_index".to_string(),
            serde_json::Value::Number(idx.into()),
        );
    }
    payload.insert("entities".to_string(), entities_json);
    payload
}

pub async fn insert_documents_enriched_batch(
    pools: &DbPools,
    documents: Vec<EnrichedDocumentInsert<'_>>,
) -> Result<Vec<usize>, String> {
    if documents.is_empty() {
        return Ok(Vec::new());
    }

    // 1. Write rows to Postgres so Qdrant point IDs match database IDs.
    let pool = pools
        .postgres
        .as_ref()
        .ok_or("PostgreSQL pool not initialized")?;

    let header_chain_json: Vec<serde_json::Value> = documents
        .iter()
        .map(|document| serde_json::json!(document.header_chain))
        .collect();
    let zone_keywords_json: Vec<serde_json::Value> = documents
        .iter()
        .map(|document| serde_json::json!(document.zone_keywords))
        .collect();
    let entities_json: Vec<serde_json::Value> = documents
        .iter()
        .map(|document| serde_json::json!(document.entities))
        .collect();

    let mut builder = QueryBuilder::<Postgres>::new(
        "INSERT INTO documents (
            text, bm25_score, filename, doc_uuid, chapter_id, chunk_index,
            chunk_level, parent_chunk_index, macro_index, medium_index,
            header_chain, zone_keywords, entities
        ) ",
    );
    builder.push_values(
        documents
            .iter()
            .zip(header_chain_json.iter())
            .zip(zone_keywords_json.iter())
            .zip(entities_json.iter()),
        |mut row, (((document, header_chain), zone_keywords), entities)| {
            row.push_bind(document.text)
                .push_bind(0.0f32)
                .push_bind(document.filename)
                .push_bind(document.doc_uuid)
                .push_bind(document.chapter_id)
                .push_bind(document.chunk_index)
                .push_bind(document.chunk_level)
                .push_bind(document.parent_chunk_index)
                .push_bind(document.macro_index)
                .push_bind(document.medium_index)
                .push_bind(header_chain)
                .push_bind(zone_keywords)
                .push_bind(entities);
        },
    );
    builder.push(" RETURNING id");

    let rows = builder
        .build()
        .fetch_all(pool)
        .await
        .map_err(|e| format!("PostgreSQL batch insert failed: {}", e))?;

    let mut doc_ids = Vec::with_capacity(documents.len());
    for row in rows {
        let id: i64 = row
            .try_get("id")
            .map_err(|e| format!("Failed to get inserted id: {}", e))?;
        doc_ids.push(id as usize);
    }

    if doc_ids.len() != documents.len() {
        return Err(format!(
            "PostgreSQL batch insert returned {} ids for {} documents",
            doc_ids.len(),
            documents.len()
        ));
    }

    let mut points = Vec::with_capacity(documents.len());
    for (document, (doc_id, entities_json)) in documents
        .iter()
        .zip(doc_ids.iter().copied().zip(entities_json.into_iter()))
    {
        let payload = build_document_payload(
            document.filename,
            document.doc_uuid,
            document.chapter_id,
            document.chunk_index,
            document.chunk_level,
            document.parent_chunk_index,
            document.macro_index,
            document.medium_index,
            entities_json,
        );
        points.push(PointStruct::new(
            doc_id as u64,
            document.vector.to_vec(),
            payload,
        ));
    }

    // 2. Write vectors to Qdrant in one upsert for the whole embedding batch.
    let client = pools
        .qdrant_clients
        .get("default")
        .ok_or("Qdrant client not initialized")?;
    let collection = crate::config::get_qdrant_collection();

    let upsert_result = client
        .upsert_points(UpsertPoints {
            collection_name: collection.clone(),
            points,
            wait: Some(crate::config::get_qdrant_upsert_wait()),
            ..Default::default()
        })
        .await;

    match upsert_result {
        Ok(_) => Ok(doc_ids),
        Err(e) => Err(format!("Qdrant upsert failed: {}", e)),
    }
}

pub async fn update_document_semantic_layout(
    pools: &DbPools,
    doc_uuid: &str,
    layout: &crate::clustering::SemanticClusterLayout,
) -> Result<(), String> {
    let pool = pools
        .postgres
        .as_ref()
        .ok_or("PostgreSQL pool not initialized")?;

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("Failed to start semantic layout transaction: {}", e))?;

    for point in &layout.points {
        sqlx::query(
            "UPDATE documents
             SET cluster_id = $1,
                 cluster_distance = $2,
                 layout_hint = $3,
                 clustering_algorithm = $4,
                 clustering_projection = $5
             WHERE doc_uuid = $6 AND chunk_index = $7",
        )
        .bind(point.cluster_id)
        .bind(point.distance_to_centroid)
        .bind(serde_json::json!(point.layout_hint))
        .bind(&layout.algorithm)
        .bind(&layout.projection)
        .bind(doc_uuid)
        .bind(point.chunk_index)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Failed to update semantic layout: {}", e))?;
    }

    tx.commit()
        .await
        .map_err(|e| format!("Failed to commit semantic layout: {}", e))?;
    Ok(())
}

pub async fn delete_document(pools: &DbPools, doc_id: usize) -> Result<(), String> {
    let pool = pools
        .postgres
        .as_ref()
        .ok_or("PostgreSQL pool not initialized")?;

    // 1. Find the doc_uuid for this doc_id
    let row_opt = sqlx::query("SELECT doc_uuid FROM documents WHERE id = $1")
        .bind(doc_id as i64)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("Failed to find doc_uuid: {}", e))?;

    let doc_uuid: Option<String> = match row_opt {
        Some(row) => row.try_get("doc_uuid").ok(),
        None => None,
    };

    let mut anchor_ids_to_delete = Vec::new();
    let ids_to_delete: Vec<i64> = if let Some(ref uuid_str) = doc_uuid {
        if !uuid_str.is_empty() {
            // Delete associated graph edges first
            crate::graph::delete_document_edges(pool, uuid_str).await?;

            // Collect anchor IDs so their embeddings can be removed from Qdrant too
            let anchor_rows = sqlx::query("SELECT id FROM anchors WHERE doc_uuid = $1")
                .bind(uuid_str)
                .fetch_all(pool)
                .await
                .map_err(|e| format!("Failed to fetch anchor IDs for uuid: {}", e))?;
            anchor_ids_to_delete = anchor_rows
                .into_iter()
                .map(|r| r.try_get::<i64, _>("id").unwrap_or(0))
                .filter(|id| *id > 0)
                .collect();

            // Find all document IDs with the same doc_uuid
            let rows = sqlx::query("SELECT id FROM documents WHERE doc_uuid = $1")
                .bind(uuid_str)
                .fetch_all(pool)
                .await
                .map_err(|e| format!("Failed to fetch doc IDs for uuid: {}", e))?;
            rows.into_iter()
                .map(|r| r.try_get::<i64, _>("id").unwrap_or(0))
                .collect()
        } else {
            vec![doc_id as i64]
        }
    } else {
        vec![doc_id as i64]
    };

    // 2. Delete all from Postgres
    sqlx::query("DELETE FROM documents WHERE id = ANY($1)")
        .bind(&ids_to_delete[..])
        .execute(pool)
        .await
        .map_err(|e| format!("PostgreSQL delete failed: {}", e))?;

    if let Some(ref uuid_str) = doc_uuid {
        if !uuid_str.is_empty() {
            sqlx::query("DELETE FROM anchors WHERE doc_uuid = $1")
                .bind(uuid_str)
                .execute(pool)
                .await
                .map_err(|e| format!("Anchor delete failed: {}", e))?;
        }
    }

    // 3. Delete all from Qdrant
    if let Some(client) = pools.qdrant_clients.get("default") {
        let collection = crate::config::get_qdrant_collection();
        let points: Vec<PointId> = ids_to_delete
            .iter()
            .map(|&id| PointId {
                point_id_options: Some(PointIdOptions::Num(id as u64)),
            })
            .collect();

        let _ = client
            .delete_points(qdrant_client::qdrant::DeletePoints {
                collection_name: collection.clone(),
                points: Some(qdrant_client::qdrant::PointsSelector {
                    points_selector_one_of: Some(PointsSelectorOneOf::Points(PointsIdsList {
                        ids: points,
                    })),
                }),
                ..Default::default()
            })
            .await;

        if !anchor_ids_to_delete.is_empty() {
            let anchors_collection = crate::config::get_qdrant_anchors_collection();
            let anchor_points: Vec<PointId> = anchor_ids_to_delete
                .iter()
                .map(|&id| PointId {
                    point_id_options: Some(PointIdOptions::Num(id as u64)),
                })
                .collect();

            let _ = client
                .delete_points(qdrant_client::qdrant::DeletePoints {
                    collection_name: anchors_collection,
                    points: Some(qdrant_client::qdrant::PointsSelector {
                        points_selector_one_of: Some(PointsSelectorOneOf::Points(PointsIdsList {
                            ids: anchor_points,
                        })),
                    }),
                    ..Default::default()
                })
                .await;
        }
    }

    Ok(())
}

pub async fn clear_documents(pools: &DbPools) -> Result<(), String> {
    let pool = pools
        .postgres
        .as_ref()
        .ok_or("PostgreSQL pool not initialized")?;
    sqlx::query("TRUNCATE TABLE documents, anchors, edges RESTART IDENTITY")
        .execute(pool)
        .await
        .map_err(|e| format!("PostgreSQL truncate failed: {}", e))?;

    if let Some(client) = pools.qdrant_clients.get("default") {
        let collection = crate::config::get_qdrant_collection();
        let anchors_collection = crate::config::get_qdrant_anchors_collection();
        let _ = client
            .delete_points(qdrant_client::qdrant::DeletePoints {
                collection_name: collection.clone(),
                points: Some(PointsSelector {
                    points_selector_one_of: Some(PointsSelectorOneOf::Filter(QFilter::default())),
                }),
                ..Default::default()
            })
            .await;
        let _ = client
            .delete_points(qdrant_client::qdrant::DeletePoints {
                collection_name: anchors_collection,
                points: Some(PointsSelector {
                    points_selector_one_of: Some(PointsSelectorOneOf::Filter(QFilter::default())),
                }),
                ..Default::default()
            })
            .await;
    }

    Ok(())
}

pub async fn hard_reset_database(pools: &DbPools) -> Result<(), String> {
    let pool = pools
        .postgres
        .as_ref()
        .ok_or("PostgreSQL pool not initialized")?;
    sqlx::query("TRUNCATE TABLE documents RESTART IDENTITY")
        .execute(pool)
        .await
        .map_err(|e| format!("PostgreSQL truncate failed: {}", e))?;

    if let Some(client) = pools.qdrant_clients.get("default") {
        let collection = crate::config::get_qdrant_collection();
        let _ = client.delete_collection(&collection).await;
        recreate_default_collection(client, &collection).await?;
    }

    Ok(())
}

pub async fn reindex_database(pools: &DbPools) -> Result<(), String> {
    let pool = pools
        .postgres
        .as_ref()
        .ok_or("PostgreSQL pool not initialized")?;

    // 1. Fetch all document chunks from PostgreSQL
    let rows = sqlx::query("SELECT id, text, filename, doc_uuid FROM documents ORDER BY id ASC")
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to fetch documents from PostgreSQL: {}", e))?;

    if rows.is_empty() {
        return Err("No documents found in PostgreSQL to reindex.".to_string());
    }

    // 2. Recreate Qdrant collection with the current configured dimension
    let client = pools
        .qdrant_clients
        .get("default")
        .ok_or("Qdrant client not initialized")?;
    let collection = crate::config::get_qdrant_collection();
    let _ = client.delete_collection(&collection).await;
    recreate_default_collection(client, &collection).await?;

    // 3. Re-embed and upsert each chunk to Qdrant
    for row in rows {
        let id: i64 = row.try_get("id").map_err(|e| e.to_string())?;
        let text: String = row.try_get("text").map_err(|e| e.to_string())?;
        let filename: String = row
            .try_get("filename")
            .unwrap_or_else(|_| "Unknown".to_string());
        let doc_uuid: String = row.try_get("doc_uuid").unwrap_or_default();

        // Generate new embedding using the current dimension
        let vector = pools.get_embedding(&text, false).await?;

        let mut payload = std::collections::HashMap::new();
        payload.insert("filename".to_string(), serde_json::Value::String(filename));
        payload.insert("doc_uuid".to_string(), serde_json::Value::String(doc_uuid));

        let point = PointStruct::new(id as u64, vector.to_vec(), payload);

        client
            .upsert_points(UpsertPoints {
                collection_name: collection.clone(),
                points: vec![point],
                wait: Some(true),
                ..Default::default()
            })
            .await
            .map_err(|e| format!("Qdrant upsert failed: {}", e))?;
    }

    Ok(())
}

pub async fn get_collection_health(pools: &DbPools) -> CollectionHealth {
    let collection_name = crate::config::get_qdrant_collection();
    let configured_dim = crate::config::get_embedding_dim();

    if let Some(client) = pools.qdrant_clients.get("default") {
        match client.collection_info(&collection_name).await {
            Ok(info) => {
                let actual_dim = info.result.as_ref().and_then(|res| extract_vector_dim(res));
                let points_count = info
                    .result
                    .as_ref()
                    .and_then(|r| r.points_count)
                    .unwrap_or(0);
                let dimension_matches = actual_dim == Some(configured_dim);
                CollectionHealth {
                    collection_name,
                    exists: true,
                    configured_dim,
                    actual_dim,
                    points_count: Some(points_count),
                    dimension_matches,
                }
            }
            Err(_) => CollectionHealth {
                collection_name,
                exists: false,
                configured_dim,
                actual_dim: None,
                points_count: None,
                dimension_matches: false,
            },
        }
    } else {
        CollectionHealth {
            collection_name,
            exists: false,
            configured_dim,
            actual_dim: None,
            points_count: None,
            dimension_matches: false,
        }
    }
}

pub async fn fetch_document_by_uuid(
    pools: &DbPools,
    doc_uuid: &str,
) -> Result<Option<StoredDocument>, String> {
    let pool = pools
        .postgres
        .as_ref()
        .ok_or("PostgreSQL pool not initialized".to_string())?;

    let rows = sqlx::query(
        "SELECT id, text, filename, doc_uuid
         FROM documents
         WHERE doc_uuid = $1
         ORDER BY id ASC",
    )
    .bind(doc_uuid)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch document by UUID: {}", e))?;

    if rows.is_empty() {
        return Ok(None);
    }

    let filename = rows[0]
        .try_get::<String, _>("filename")
        .unwrap_or_else(|_| "Unknown".to_string());
    let joined_text = rows
        .iter()
        .map(|row| row.try_get::<String, _>("text").unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n\n");

    Ok(Some(StoredDocument {
        filename,
        doc_uuid: doc_uuid.to_string(),
        text: joined_text,
    }))
}

pub async fn resolve_doc_uuid(pools: &DbPools, input: &str) -> Option<String> {
    let pool = pools.postgres.as_ref()?;

    // 1. Try exact match on doc_uuid
    let row_opt =
        sqlx::query("SELECT DISTINCT doc_uuid FROM documents WHERE doc_uuid = $1 LIMIT 1")
            .bind(input)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();

    if let Some(row) = row_opt {
        let uuid: String = row.try_get("doc_uuid").unwrap_or_default();
        if !uuid.is_empty() {
            return Some(uuid);
        }
    }

    // 2. Try exact match on filename
    let row_opt =
        sqlx::query("SELECT DISTINCT doc_uuid FROM documents WHERE filename = $1 LIMIT 1")
            .bind(input)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();

    if let Some(row) = row_opt {
        let uuid: String = row.try_get("doc_uuid").unwrap_or_default();
        if !uuid.is_empty() {
            return Some(uuid);
        }
    }

    // 3. Try partial case-insensitive match on filename
    let pattern = format!("%{}%", input);
    let row_opt =
        sqlx::query("SELECT DISTINCT doc_uuid FROM documents WHERE filename ILIKE $1 LIMIT 1")
            .bind(&pattern)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();

    if let Some(row) = row_opt {
        let uuid: String = row.try_get("doc_uuid").unwrap_or_default();
        if !uuid.is_empty() {
            return Some(uuid);
        }
    }

    None
}
