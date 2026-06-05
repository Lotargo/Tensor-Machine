use sqlx::PgPool;
use sqlx::Row;
use std::collections::{BTreeMap, BTreeSet, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
struct SharedEntityPair {
    doc1: String,
    doc2: String,
    shared_keys: Vec<String>,
    shared_labels: Vec<String>,
}

/// Fetches the sub-graph neighborhood (nodes and edges) up to max_depth hops around a starting doc_uuid.
pub async fn get_document_neighborhood(
    pool: &PgPool,
    start_uuid: &str,
    max_depth: u32,
) -> Result<serde_json::Value, String> {
    // 1. Recursive CTE query to get the list of unique doc_uuids within max_depth hops.
    // Handles traversing edges bidirectionally.
    let rows = sqlx::query(
        r#"
        WITH RECURSIVE graph_nodes AS (
            -- Base case: start with the seed document
            SELECT $1::TEXT as doc_uuid, 0 as depth
            UNION
            -- Recursive step: traverse edges in both directions
            SELECT 
                CASE 
                    WHEN e.source_doc_uuid = gn.doc_uuid THEN e.target_doc_uuid 
                    ELSE e.source_doc_uuid 
                END as doc_uuid,
                gn.depth + 1
            FROM edges e
            JOIN graph_nodes gn ON e.source_doc_uuid = gn.doc_uuid OR e.target_doc_uuid = gn.doc_uuid
            WHERE gn.depth < $2::INTEGER
        )
        SELECT DISTINCT doc_uuid FROM graph_nodes;
        "#
    )
    .bind(start_uuid)
    .bind(max_depth as i32)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to traverse neighborhood graph: {}", e))?;

    let mut doc_uuids: Vec<String> = rows
        .iter()
        .map(|r| r.try_get::<String, _>("doc_uuid").unwrap_or_default())
        .filter(|uuid| !uuid.is_empty())
        .collect();

    // Ensure the start_uuid is included in case no edges exist yet
    if !doc_uuids.contains(&start_uuid.to_string()) {
        doc_uuids.push(start_uuid.to_string());
    }

    // 2. Fetch document details (nodes) for the found uuids
    let doc_rows = sqlx::query(
        r#"
        SELECT DISTINCT doc_uuid, filename, metadata 
        FROM documents 
        WHERE doc_uuid = ANY($1)
        "#,
    )
    .bind(&doc_uuids[..])
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch nodes in neighborhood: {}", e))?;

    let mut nodes = Vec::new();
    let mut resolved_uuids = HashSet::new();

    for r in doc_rows {
        let uuid: String = r.try_get("doc_uuid").unwrap_or_default();
        let filename: String = r.try_get("filename").unwrap_or_default();
        let metadata: serde_json::Value = r.try_get("metadata").unwrap_or(serde_json::json!({}));
        resolved_uuids.insert(uuid.clone());
        nodes.push(serde_json::json!({
            "doc_uuid": uuid,
            "filename": filename,
            "metadata": metadata
        }));
    }

    // If some document uuids have no entries in the documents table, add placeholder nodes
    for uuid in &doc_uuids {
        if !resolved_uuids.contains(uuid) {
            nodes.push(serde_json::json!({
                "doc_uuid": uuid,
                "filename": "Unknown Document",
                "metadata": {}
            }));
        }
    }

    // 3. Fetch all edges connecting any of these documents
    let edge_rows = sqlx::query(
        r#"
        SELECT id, source_doc_uuid, target_doc_uuid, edge_type, metadata 
        FROM edges 
        WHERE source_doc_uuid = ANY($1) AND target_doc_uuid = ANY($1)
        ORDER BY id ASC
        "#,
    )
    .bind(&doc_uuids[..])
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch edges in neighborhood: {}", e))?;

    let edges: Vec<serde_json::Value> = edge_rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.try_get::<i64, _>("id").unwrap_or(0),
                "source": r.try_get::<String, _>("source_doc_uuid").unwrap_or_default(),
                "target": r.try_get::<String, _>("target_doc_uuid").unwrap_or_default(),
                "type": r.try_get::<String, _>("edge_type").unwrap_or_default(),
                "metadata": r.try_get::<serde_json::Value, _>("metadata").unwrap_or(serde_json::json!({}))
            })
        })
        .collect();

    Ok(serde_json::json!({
        "nodes": nodes,
        "edges": edges
    }))
}

/// Finds the shortest path between start_uuid and end_uuid using a BFS recursive CTE.
pub async fn find_shortest_path(
    pool: &PgPool,
    start_uuid: &str,
    end_uuid: &str,
) -> Result<Vec<String>, String> {
    if start_uuid == end_uuid {
        return Ok(vec![start_uuid.to_string()]);
    }

    // BFS recursive pathfinding query
    let row_opt = sqlx::query(
        r#"
        WITH RECURSIVE bfs_path AS (
            -- Base case: start from start_uuid
            SELECT 
                $1::TEXT as current_uuid,
                ARRAY[$1::TEXT] as path,
                0 as depth
            UNION ALL
            -- Recursive case: traverse to adjacent nodes not already visited
            SELECT 
                CASE 
                    WHEN e.source_doc_uuid = bp.current_uuid THEN e.target_doc_uuid 
                    ELSE e.source_doc_uuid 
                END as next_uuid,
                bp.path || ARRAY[
                    CASE 
                        WHEN e.source_doc_uuid = bp.current_uuid THEN e.target_doc_uuid 
                        ELSE e.source_doc_uuid 
                    END
                ] as path,
                bp.depth + 1
            FROM edges e
            JOIN bfs_path bp ON e.source_doc_uuid = bp.current_uuid OR e.target_doc_uuid = bp.current_uuid
            WHERE NOT (
                CASE 
                    WHEN e.source_doc_uuid = bp.current_uuid THEN e.target_doc_uuid 
                    ELSE e.source_doc_uuid 
                END = ANY(bp.path)
            ) AND bp.depth < 10 -- Safety search limit depth
        )
        SELECT path FROM bfs_path 
        WHERE current_uuid = $2::TEXT 
        ORDER BY depth ASC 
        LIMIT 1;
        "#
    )
    .bind(start_uuid)
    .bind(end_uuid)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Pathfinding CTE execution failed: {}", e))?;

    match row_opt {
        Some(row) => {
            let path: Vec<String> = row.try_get("path").map_err(|e| e.to_string())?;
            Ok(path)
        }
        None => Ok(Vec::new()), // No path exists
    }
}

/// Automatically links documents together if they share at least `threshold` unique entities.
pub async fn auto_link_by_shared_entities(
    pool: &PgPool,
    threshold: usize,
) -> Result<usize, String> {
    let rows = sqlx::query(
        r#"
        SELECT doc_uuid, COALESCE(entities, '[]'::jsonb) as entities
        FROM documents
        WHERE entities IS NOT NULL
          AND jsonb_typeof(entities) = 'array'
          AND jsonb_array_length(entities) > 0
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch document entities for graph linking: {}", e))?;

    let documents = rows
        .into_iter()
        .map(|row| {
            (
                row.try_get::<String, _>("doc_uuid").unwrap_or_default(),
                json_array_to_strings(row.try_get("entities").unwrap_or(serde_json::json!([]))),
            )
        })
        .filter(|(doc_uuid, entities)| !doc_uuid.is_empty() && !entities.is_empty())
        .collect::<Vec<_>>();

    let pairs = shared_entity_pairs(&documents, threshold);

    let mut new_edges_count = 0;

    for pair in pairs {
        let metadata = serde_json::json!({
            "shared_count": pair.shared_keys.len(),
            "shared_entity_keys": pair.shared_keys,
            "shared_entities": pair.shared_labels,
            "auto_linked": true
        });

        let result = sqlx::query(
            r#"
            INSERT INTO edges (source_doc_uuid, target_doc_uuid, edge_type, metadata)
            SELECT $1, $2, 'shares_entities', $3
            WHERE NOT EXISTS (
                SELECT 1 FROM edges 
                WHERE (source_doc_uuid = $1 AND target_doc_uuid = $2 AND edge_type = 'shares_entities')
                   OR (source_doc_uuid = $2 AND target_doc_uuid = $1 AND edge_type = 'shares_entities')
            )
            "#
        )
        .bind(&pair.doc1)
        .bind(&pair.doc2)
        .bind(&metadata)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to insert shares_entities edge: {}", e))?;

        if result.rows_affected() > 0 {
            new_edges_count += 1;
        }
    }

    Ok(new_edges_count)
}

fn shared_entity_pairs(
    documents: &[(String, Vec<String>)],
    threshold: usize,
) -> Vec<SharedEntityPair> {
    if threshold == 0 {
        return Vec::new();
    }

    let mut by_doc: BTreeMap<String, BTreeMap<String, BTreeSet<String>>> = BTreeMap::new();
    for (doc_uuid, entities) in documents {
        let doc_entities = by_doc.entry(doc_uuid.clone()).or_default();
        for entity in entities {
            let key = crate::normalizer::stem_phrase(entity);
            if key.is_empty() {
                continue;
            }
            doc_entities
                .entry(key)
                .or_default()
                .insert(entity.trim().to_string());
        }
    }

    let docs = by_doc.into_iter().collect::<Vec<_>>();
    let mut pairs = Vec::new();
    for left_idx in 0..docs.len() {
        for right_idx in (left_idx + 1)..docs.len() {
            let (doc1, left_entities) = &docs[left_idx];
            let (doc2, right_entities) = &docs[right_idx];
            let shared_keys = left_entities
                .keys()
                .filter(|key| right_entities.contains_key(*key))
                .cloned()
                .collect::<Vec<_>>();

            if shared_keys.len() < threshold {
                continue;
            }

            let shared_labels = shared_keys
                .iter()
                .flat_map(|key| {
                    left_entities
                        .get(key)
                        .into_iter()
                        .chain(right_entities.get(key))
                        .flat_map(|labels| labels.iter().cloned())
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();

            pairs.push(SharedEntityPair {
                doc1: doc1.clone(),
                doc2: doc2.clone(),
                shared_keys,
                shared_labels,
            });
        }
    }
    pairs
}

fn json_array_to_strings(value: serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .map(|array| {
            array
                .iter()
                .filter_map(|value| value.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// Deletes all edges associated with a document (both incoming and outgoing).
pub async fn delete_document_edges(pool: &PgPool, doc_uuid: &str) -> Result<(), String> {
    sqlx::query(
        r#"
        DELETE FROM edges 
        WHERE source_doc_uuid = $1 OR target_doc_uuid = $1
        "#,
    )
    .bind(doc_uuid)
    .execute(pool)
    .await
    .map_err(|e| {
        format!(
            "Failed to delete edges associated with document '{}': {}",
            doc_uuid, e
        )
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_entity_pairs_match_inflected_forms() {
        let documents = vec![
            (
                "doc-a".to_string(),
                vec!["кошками".to_string(), "Белый Кролик".to_string()],
            ),
            (
                "doc-b".to_string(),
                vec!["кошка".to_string(), "Белого Кролика".to_string()],
            ),
            ("doc-c".to_string(), vec!["unrelated".to_string()]),
        ];

        let pairs = shared_entity_pairs(&documents, 2);

        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].doc1, "doc-a");
        assert_eq!(pairs[0].doc2, "doc-b");
        assert_eq!(pairs[0].shared_keys.len(), 2);
        assert!(pairs[0].shared_labels.contains(&"кошками".to_string()));
        assert!(pairs[0].shared_labels.contains(&"кошка".to_string()));
    }

    #[test]
    fn shared_entity_pairs_deduplicate_chunk_entities_per_document() {
        let documents = vec![
            (
                "doc-a".to_string(),
                vec!["database".to_string(), "database".to_string()],
            ),
            ("doc-a".to_string(), vec!["databases".to_string()]),
            ("doc-b".to_string(), vec!["database".to_string()]),
        ];

        let pairs = shared_entity_pairs(&documents, 1);

        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].shared_keys, vec!["databas".to_string()]);
        assert_eq!(pairs[0].shared_labels.len(), 2);
    }
}
