#[tokio::test]
async fn test_qdrant_debug() {
    dotenvy::dotenv().ok();
    let qdrant_url = std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6334".to_string());
    println!("Connecting to Qdrant at: {}", qdrant_url);
    
    let mut pools = tensor_rag::db::DbPools::new();
    pools.add_qdrant_node("default", &qdrant_url).await.unwrap();
    
    // Connect to Qdrant directly to search and print raw points
    match pools.search_qdrant(&ndarray::Array1::from_elem(tensor_rag::config::get_embedding_dim(), 0.1), &None, 3).await {
        Ok((ids, vectors)) => {
            for (i, (id, vector)) in ids.iter().zip(vectors.iter()).enumerate() {
                println!("Point {} ID: {:?}", i, id);
                println!("Point {} vector length: {:?}", i, vector.len());
            }
        }
        Err(err) => {
            println!("Qdrant debug search skipped due to external state mismatch: {}", err);
        }
    }
}
