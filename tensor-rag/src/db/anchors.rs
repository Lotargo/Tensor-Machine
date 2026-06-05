use qdrant_client::qdrant::{PointId, PointStruct, SearchPoints, UpsertPoints, point_id::PointIdOptions};
use sqlx::Row;
use super::DbPools;

pub async fn insert_anchor(
    pools: &DbPools,
    doc_uuid: &str,
    name: &str,
    text: &str,
) -> Result<i64, String> {
    let pool = pools
        .postgres
        .as_ref()
        .ok_or("PostgreSQL pool not initialized")?;

    let row = sqlx::query(
        "INSERT INTO anchors (doc_uuid, name, text) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(doc_uuid)
    .bind(name)
    .bind(text)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to insert anchor: {}", e))?;

    row.try_get("id")
        .map_err(|e| format!("Failed to get anchor id: {}", e))
}

pub async fn insert_anchor_embedding(
    pools: &DbPools,
    anchor_id: i64,
    vector: ndarray::Array1<f32>,
    doc_uuid: &str,
    name: &str,
) -> Result<(), String> {
    insert_anchor_embeddings_batch(pools, vec![(anchor_id, vector, doc_uuid, name)]).await
}

pub async fn insert_anchor_embeddings_batch(
    pools: &DbPools,
    anchors: Vec<(i64, ndarray::Array1<f32>, &str, &str)>,
) -> Result<(), String> {
    if anchors.is_empty() {
        return Ok(());
    }

    let client = pools
        .qdrant_clients
        .get("default")
        .ok_or("Qdrant client not initialized")?;
    let anchors_collection = crate::config::get_qdrant_anchors_collection();

    let points = anchors
        .into_iter()
        .map(|(anchor_id, vector, doc_uuid, name)| {
            let mut payload = std::collections::HashMap::new();
            payload.insert(
                "doc_uuid".to_string(),
                serde_json::Value::String(doc_uuid.to_string()),
            );
            payload.insert(
                "anchor_name".to_string(),
                serde_json::Value::String(name.to_string()),
            );
            PointStruct::new(anchor_id as u64, vector.to_vec(), payload)
        })
        .collect();

    client
        .upsert_points(UpsertPoints {
            collection_name: anchors_collection.clone(),
            points,
            wait: Some(crate::config::get_qdrant_upsert_wait()),
            ..Default::default()
        })
        .await
        .map_err(|e| format!("Qdrant anchor upsert failed: {}", e))?;

    Ok(())
}

pub async fn search_anchors_dense(
    pools: &DbPools,
    query_vector: &ndarray::Array1<f32>,
    top_k: usize,
) -> Result<(Vec<i64>, Vec<f32>, Vec<String>), String> {
    let client = pools
        .qdrant_clients
        .get("default")
        .ok_or("Qdrant client not initialized")?;
    let anchors_collection = crate::config::get_qdrant_anchors_collection();

    let search_points = SearchPoints {
        collection_name: anchors_collection.clone(),
        vector: query_vector.to_vec(),
        limit: top_k as u64,
        with_payload: Some(true.into()),
        ..Default::default()
    };

    let response = client
        .search_points(search_points)
        .await
        .map_err(|e| format!("Qdrant anchor search failed: {}", e))?;

    let mut ids = Vec::new();
    let mut scores = Vec::new();
    let mut names = Vec::new();

    for point in &response.result {
        let id = match &point.id {
            Some(PointId {
                point_id_options: Some(PointIdOptions::Num(n)),
            }) => *n as i64,
            _ => continue,
        };
        ids.push(id);
        scores.push(point.score);

        let name = point
            .payload
            .get("anchor_name")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_default();
        names.push(name);
    }

    Ok((ids, scores, names))
}

pub async fn search_anchors_bm25(
    pools: &DbPools,
    query: &str,
    top_k: usize,
) -> Result<Vec<(i64, String, String, f32)>, String> {
    let pool = pools
        .postgres
        .as_ref()
        .ok_or("PostgreSQL pool not initialized")?;

    let rows = sqlx::query(
        "SELECT id, doc_uuid, name, text FROM anchors WHERE to_tsvector('simple', text) @@ plainto_tsquery('simple', $1) LIMIT $2"
    )
    .bind(query)
    .bind(top_k as i64)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Anchor BM25 search failed: {}", e))?;

    let mut results = Vec::new();
    for r in &rows {
        let id: i64 = r.try_get("id").unwrap_or(0);
        let doc_uuid: String = r.try_get("doc_uuid").unwrap_or_default();
        let name: String = r.try_get("name").unwrap_or_default();
        let _text: String = r.try_get("text").unwrap_or_default();
        results.push((id, doc_uuid, name, 1.0f32));
    }

    Ok(results)
}

pub async fn fetch_anchors_by_doc_uuid(
    pools: &DbPools,
    doc_uuid: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let pool = pools
        .postgres
        .as_ref()
        .ok_or("PostgreSQL pool not initialized")?;

    let rows =
        sqlx::query("SELECT name, text FROM anchors WHERE doc_uuid = $1 ORDER BY id ASC")
            .bind(doc_uuid)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("Failed to fetch anchors: {}", e))?;

    Ok(rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "name": r.try_get::<String, _>("name").unwrap_or_default(),
                "text": r.try_get::<String, _>("text").unwrap_or_default()
            })
        })
        .collect())
}

pub fn extract_anchors_from_markdown(markdown: &str) -> Vec<(String, String)> {
    let mut anchors = Vec::new();
    let mut section_counter = 0u32;

    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") {
            section_counter += 1;
            let title = trimmed.trim_start_matches("# ").trim().to_string();
            anchors.push((format!("h1-{}", section_counter), title.clone()));
            anchors.push((format!("heading-{}", section_counter), title));
        } else if trimmed.starts_with("## ") {
            section_counter += 1;
            let title = trimmed.trim_start_matches("## ").trim().to_string();
            anchors.push((format!("h2-{}", section_counter), title.clone()));
            anchors.push((format!("heading-{}", section_counter), title));
        } else if trimmed.starts_with("### ") {
            section_counter += 1;
            let title = trimmed.trim_start_matches("### ").trim().to_string();
            anchors.push((format!("h3-{}", section_counter), title.clone()));
            anchors.push((format!("heading-{}", section_counter), title));
        } else if trimmed.starts_with("#### ") {
            section_counter += 1;
            let title = trimmed.trim_start_matches("#### ").trim().to_string();
            anchors.push((format!("h4-{}", section_counter), title));
        }
    }

    anchors
}

pub async fn auto_extract_anchors(
    pools: &DbPools,
    doc_uuid: &str,
    markdown_text: &str,
) -> Result<Vec<i64>, String> {
    let anchors = extract_anchors_from_markdown(markdown_text);
    let mut inserted_ids = Vec::new();
    let mut embedding_jobs = Vec::new();

    for (name, text) in anchors {
        let anchor_id = insert_anchor(pools, doc_uuid, &name, &text).await?;
        inserted_ids.push(anchor_id);
        embedding_jobs.push((anchor_id, name, text));
    }

    let batch_size = crate::config::get_embedding_batch_size().max(1);
    for batch in embedding_jobs.chunks(batch_size) {
        let texts: Vec<String> = batch.iter().map(|(_, _, text)| text.clone()).collect();
        match pools.get_embeddings(&texts, false).await {
            Ok(vectors) => {
                let anchor_points = batch
                    .iter()
                    .zip(vectors.into_iter())
                    .map(|((anchor_id, name, _), vector)| {
                        (*anchor_id, vector, doc_uuid, name.as_str())
                    })
                    .collect();
                if let Err(error) = insert_anchor_embeddings_batch(pools, anchor_points).await {
                    let anchor_names = batch
                        .iter()
                        .map(|(_, name, _)| name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    tracing::warn!(
                        "Failed to store anchor embedding batch for document {} [{}]: {}",
                        doc_uuid,
                        anchor_names,
                        error
                    );
                }
            }
            Err(error) => {
                let anchor_names = batch
                    .iter()
                    .map(|(_, name, _)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                tracing::warn!(
                    "Failed to embed anchor batch for document {} [{}]: {}",
                    doc_uuid,
                    anchor_names,
                    error
                );
            }
        }
    }

    Ok(inserted_ids)
}
