#![cfg(feature = "integration")]

#[tokio::test]
async fn test_qdrant_debug() {
    dotenvy::dotenv().ok();
    let qdrant_url =
        std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6334".to_string());
    println!("Connecting to Qdrant at: {}", qdrant_url);

    let mut pools = tensor_rag::db::DbPools::new();
    pools.add_qdrant_node("default", &qdrant_url).await.unwrap();

    let client = pools.qdrant_clients.get("default").unwrap();
    let collection = tensor_rag::config::get_qdrant_collection();

    let search_points = qdrant_client::qdrant::SearchPoints {
        collection_name: collection,
        vector: vec![0.1f32; 768],
        limit: 2,
        with_vectors: Some(qdrant_client::qdrant::WithVectorsSelector {
            selector_options: Some(
                qdrant_client::qdrant::with_vectors_selector::SelectorOptions::Enable(true),
            ),
        }),
        ..Default::default()
    };

    match client.search_points(search_points).await {
        Ok(response) => {
            for (i, point) in response.result.iter().enumerate() {
                if let Some(ref vectors) = point.vectors {
                    if let Some(qdrant_client::qdrant::vectors_output::VectorsOptions::Vector(
                        ref v,
                    )) = vectors.vectors_options
                    {
                        #[allow(deprecated)]
                        let vec_data = if let Some(
                            qdrant_client::qdrant::vector_output::Vector::Dense(dense),
                        ) = &v.vector
                        {
                            dense.data.clone()
                        } else {
                            v.data.clone()
                        };
                        println!("Point {} vector data length: {}", i, vec_data.len());
                    }
                }
            }
        }
        Err(err) => {
            println!("Qdrant debug search failed: {}", err);
        }
    }
}
