use sqlx::Row;
use std::env;

#[tokio::test]
async fn test_graph_operations() {
    // 1. Initialize environment and DB pool.
    // This test mutates graph/document tables, so it must never use the normal
    // application DATABASE_URL.
    dotenvy::dotenv().ok();
    let database_url = match env::var("TENSOR_RAG_TEST_DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!(
                "Skipping graph integration test: set TENSOR_RAG_TEST_DATABASE_URL to a disposable PostgreSQL database to run it."
            );
            return;
        }
    };

    let mut db_pools = tensor_rag::db::DbPools::new();
    db_pools
        .init_postgres(&database_url)
        .await
        .expect("Failed to initialize database");
    let pool = db_pools
        .postgres
        .as_ref()
        .expect("Postgres pool not loaded");

    // 2. Clean up disposable test tables
    sqlx::query("DELETE FROM edges")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM documents")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM anchors")
        .execute(pool)
        .await
        .unwrap();

    // 3. Insert mock documents with entities
    // Doc A: shares "rust" with Doc B (1 shared), shares "database", "graph" with Doc C (2 shared)
    let doc_a_uuid = "doc-uuid-a";
    let doc_b_uuid = "doc-uuid-b";
    let doc_c_uuid = "doc-uuid-c";
    let doc_d_uuid = "doc-uuid-d";

    let entities_a = serde_json::json!(["rust", "database", "graph"]);
    let entities_b = serde_json::json!(["rust", "asynchronous"]);
    let entities_c = serde_json::json!(["database", "graph", "sql"]);
    let entities_d = serde_json::json!(["unrelated", "topic"]);

    // Insert Doc A
    sqlx::query(
        "INSERT INTO documents (text, bm25_score, filename, doc_uuid, entities, chunk_level) VALUES ($1, $2, $3, $4, $5, $6)"
    )
    .bind("This is text for A. We use rust, database and graph.")
    .bind(0.0f32)
    .bind("doc-a.txt")
    .bind(doc_a_uuid)
    .bind(&entities_a)
    .bind("macro")
    .execute(pool)
    .await
    .unwrap();

    // Insert Doc B
    sqlx::query(
        "INSERT INTO documents (text, bm25_score, filename, doc_uuid, entities, chunk_level) VALUES ($1, $2, $3, $4, $5, $6)"
    )
    .bind("This is text for B. It runs asynchronous rust code.")
    .bind(0.0f32)
    .bind("doc-b.txt")
    .bind(doc_b_uuid)
    .bind(&entities_b)
    .bind("macro")
    .execute(pool)
    .await
    .unwrap();

    // Insert Doc C
    sqlx::query(
        "INSERT INTO documents (text, bm25_score, filename, doc_uuid, entities, chunk_level) VALUES ($1, $2, $3, $4, $5, $6)"
    )
    .bind("This is text for C. Relational database, sql queries and graph paths.")
    .bind(0.0f32)
    .bind("doc-c.txt")
    .bind(doc_c_uuid)
    .bind(&entities_c)
    .bind("macro")
    .execute(pool)
    .await
    .unwrap();

    // Insert Doc D
    sqlx::query(
        "INSERT INTO documents (text, bm25_score, filename, doc_uuid, entities, chunk_level) VALUES ($1, $2, $3, $4, $5, $6)"
    )
    .bind("This is text for D. Completely unrelated topic.")
    .bind(0.0f32)
    .bind("doc-d.txt")
    .bind(doc_d_uuid)
    .bind(&entities_d)
    .bind("macro")
    .execute(pool)
    .await
    .unwrap();

    // 4. Insert manual edges to form path A -> B -> C
    // Doc D remains disconnected
    sqlx::query(
        "INSERT INTO edges (source_doc_uuid, target_doc_uuid, edge_type) VALUES ($1, $2, $3)",
    )
    .bind(doc_a_uuid)
    .bind(doc_b_uuid)
    .bind("shares_topic")
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO edges (source_doc_uuid, target_doc_uuid, edge_type) VALUES ($1, $2, $3)",
    )
    .bind(doc_b_uuid)
    .bind(doc_c_uuid)
    .bind("references")
    .execute(pool)
    .await
    .unwrap();

    // 5. Test get_document_neighborhood
    // Depth 1: Should retrieve A and B, plus 1 edge (A -> B)
    let graph_depth_1 = tensor_rag::graph::get_document_neighborhood(pool, doc_a_uuid, 1)
        .await
        .unwrap();
    let nodes_1 = graph_depth_1.get("nodes").unwrap().as_array().unwrap();
    let edges_1 = graph_depth_1.get("edges").unwrap().as_array().unwrap();

    // A and B must be in nodes
    let node_uuids_1: Vec<&str> = nodes_1
        .iter()
        .map(|n| n.get("doc_uuid").unwrap().as_str().unwrap())
        .collect();
    assert!(node_uuids_1.contains(&doc_a_uuid));
    assert!(node_uuids_1.contains(&doc_b_uuid));
    assert!(!node_uuids_1.contains(&doc_c_uuid)); // C is 2 hops away, shouldn't be here
    assert_eq!(edges_1.len(), 1);

    // Depth 2: Should retrieve A, B and C, plus 2 edges (A -> B, B -> C)
    let graph_depth_2 = tensor_rag::graph::get_document_neighborhood(pool, doc_a_uuid, 2)
        .await
        .unwrap();
    let nodes_2 = graph_depth_2.get("nodes").unwrap().as_array().unwrap();
    let edges_2 = graph_depth_2.get("edges").unwrap().as_array().unwrap();

    let node_uuids_2: Vec<&str> = nodes_2
        .iter()
        .map(|n| n.get("doc_uuid").unwrap().as_str().unwrap())
        .collect();
    assert!(node_uuids_2.contains(&doc_a_uuid));
    assert!(node_uuids_2.contains(&doc_b_uuid));
    assert!(node_uuids_2.contains(&doc_c_uuid));
    assert_eq!(edges_2.len(), 2);

    // Depth 1 for Doc D: Should return only Doc D, 0 edges
    let graph_d = tensor_rag::graph::get_document_neighborhood(pool, doc_d_uuid, 1)
        .await
        .unwrap();
    let nodes_d = graph_d.get("nodes").unwrap().as_array().unwrap();
    let edges_d = graph_d.get("edges").unwrap().as_array().unwrap();
    assert_eq!(nodes_d.len(), 1);
    assert_eq!(
        nodes_d[0].get("doc_uuid").unwrap().as_str().unwrap(),
        doc_d_uuid
    );
    assert!(edges_d.is_empty());

    // 6. Test find_shortest_path
    // Path A -> C (length 3, through B)
    let path_a_c = tensor_rag::graph::find_shortest_path(pool, doc_a_uuid, doc_c_uuid)
        .await
        .unwrap();
    assert_eq!(
        path_a_c,
        vec![
            doc_a_uuid.to_string(),
            doc_b_uuid.to_string(),
            doc_c_uuid.to_string()
        ]
    );

    // Path A -> D (disconnected, should return empty)
    let path_a_d = tensor_rag::graph::find_shortest_path(pool, doc_a_uuid, doc_d_uuid)
        .await
        .unwrap();
    assert!(path_a_d.is_empty());

    // 7. Test auto_link_by_shared_entities
    // Running auto-linker with threshold 2 should link A and C (share "database", "graph")
    // A and B share only "rust" (1), so they are not linked.
    let linked_count = tensor_rag::graph::auto_link_by_shared_entities(pool, 2)
        .await
        .unwrap();
    assert_eq!(linked_count, 1); // Edge A <-> C created

    // Check if new edge is stored
    let edge_check =
        sqlx::query("SELECT id, metadata FROM edges WHERE edge_type = 'shares_entities'")
            .fetch_all(pool)
            .await
            .unwrap();
    assert_eq!(edge_check.len(), 1);
    let meta: serde_json::Value = edge_check[0].try_get("metadata").unwrap();
    assert_eq!(meta.get("shared_count").unwrap().as_i64().unwrap(), 2);

    // Running again should not create duplicate edges
    let linked_count_repeat = tensor_rag::graph::auto_link_by_shared_entities(pool, 2)
        .await
        .unwrap();
    assert_eq!(linked_count_repeat, 0);

    // 8. Test delete_document_edges
    // Deleting Doc B should clear all its edges (A -> B, B -> C)
    tensor_rag::graph::delete_document_edges(pool, doc_b_uuid)
        .await
        .unwrap();

    let edges_after_delete = sqlx::query("SELECT id FROM edges")
        .fetch_all(pool)
        .await
        .unwrap();
    // Only the auto-linked A -> C shares_entities edge should remain
    assert_eq!(edges_after_delete.len(), 1);
}
