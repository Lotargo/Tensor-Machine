use sqlx::PgPool;
use sqlx::Row;
use std::collections::HashSet;

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
        "#
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
        "#
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
    // 1. Find pairs of documents sharing at least the threshold number of entities
    let rows = sqlx::query(
        r#"
        WITH doc_entities AS (
            SELECT DISTINCT doc_uuid, jsonb_array_elements_text(entities) as entity
            FROM documents
            WHERE entities IS NOT NULL AND jsonb_typeof(entities) = 'array'
        ),
        shared_entity_pairs AS (
            SELECT 
                de1.doc_uuid as doc1,
                de2.doc_uuid as doc2,
                COUNT(de1.entity) as shared_count,
                jsonb_agg(de1.entity) as shared_entities
            FROM doc_entities de1
            JOIN doc_entities de2 ON de1.entity = de2.entity AND de1.doc_uuid < de2.doc_uuid
            GROUP BY de1.doc_uuid, de2.doc_uuid
            HAVING COUNT(de1.entity) >= $1::BIGINT
        )
        SELECT doc1, doc2, shared_count, shared_entities FROM shared_entity_pairs;
        "#
    )
    .bind(threshold as i64)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to find shared entity pairs: {}", e))?;

    let mut new_edges_count = 0;

    // 2. Insert edge for each pair if it doesn't exist yet
    for r in rows {
        let doc1: String = r.try_get("doc1").unwrap_or_default();
        let doc2: String = r.try_get("doc2").unwrap_or_default();
        let count: i64 = r.try_get("shared_count").unwrap_or(0);
        let shared: serde_json::Value = r.try_get("shared_entities").unwrap_or(serde_json::json!([]));

        let metadata = serde_json::json!({
            "shared_count": count,
            "shared_entities": shared,
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
        .bind(&doc1)
        .bind(&doc2)
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

/// Deletes all edges associated with a document (both incoming and outgoing).
pub async fn delete_document_edges(pool: &PgPool, doc_uuid: &str) -> Result<(), String> {
    sqlx::query(
        r#"
        DELETE FROM edges 
        WHERE source_doc_uuid = $1 OR target_doc_uuid = $1
        "#
    )
    .bind(doc_uuid)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to delete edges associated with document '{}': {}", doc_uuid, e))?;

    Ok(())
}
