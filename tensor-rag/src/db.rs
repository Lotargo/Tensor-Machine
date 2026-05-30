use sqlx::{PgPool, postgres::PgPoolOptions};
use tonic::transport::{Channel, Endpoint};
use std::collections::HashMap;


/// A simple connection pool manager for various databases and external ML services.
#[derive(Clone)]
pub struct DbPools {
    pub postgres: Option<PgPool>,
    pub qdrant_channels: HashMap<String, Channel>,
    pub ml_servers: HashMap<String, Channel>,
}

impl DbPools {
    pub fn new() -> Self {
        Self {
            postgres: None,
            qdrant_channels: HashMap::new(),
            ml_servers: HashMap::new(),
        }
    }

    /// Initializes PostgreSQL pool
    pub async fn init_postgres(&mut self, database_url: &str) -> Result<(), sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await?;
        self.postgres = Some(pool);
        Ok(())
    }

    /// Adds a connection channel to a Qdrant node
    pub async fn add_qdrant_node(&mut self, node_id: &str, url: &str) -> Result<(), tonic::transport::Error> {
        let endpoint = Endpoint::from_shared(url.to_string())?;
        let channel = endpoint.connect().await?;
        self.qdrant_channels.insert(node_id.to_string(), channel);
        Ok(())
    }

    /// Adds a connection channel to an external ML server
    pub async fn add_ml_server(&mut self, server_id: &str, url: &str) -> Result<(), tonic::transport::Error> {
        let endpoint = Endpoint::from_shared(url.to_string())?;
        let channel = endpoint.connect().await?;
        self.ml_servers.insert(server_id.to_string(), channel);
        Ok(())
    }

    /// Mock method for ANN search in Qdrant (Task 1.4.1, 1.4.6)
    pub async fn search_qdrant(&self, query: &ndarray::Array1<f32>, filters: &Option<Vec<crate::ir::Filter>>, top_k: usize) -> (Vec<usize>, Vec<ndarray::Array1<f32>>) {
        // Construct the basic Qdrant grpc/REST payload payload log representation
        let qdrant_request_payload = serde_json::json!({
            "vector": query.to_vec(),
            "limit": top_k,
            "filter": filters.as_ref().map(|f| {
                let must_conditions: Vec<_> = f.iter().map(|filter| {
                    match filter.operator.as_str() {
                        "==" | "=" => serde_json::json!({
                            "key": filter.field,
                            "match": { "value": filter.value }
                        }),
                        ">=" => serde_json::json!({
                            "key": filter.field,
                            "range": { "gte": filter.value }
                        }),
                        ">" => serde_json::json!({
                            "key": filter.field,
                            "range": { "gt": filter.value }
                        }),
                        "<=" => serde_json::json!({
                            "key": filter.field,
                            "range": { "lte": filter.value }
                        }),
                        "<" => serde_json::json!({
                            "key": filter.field,
                            "range": { "lt": filter.value }
                        }),
                        "in" => serde_json::json!({
                            "key": filter.field,
                            "match": { "any": filter.value }
                        }),
                        _ => serde_json::json!({
                            "key": filter.field,
                            "match": { "value": filter.value } // fallback
                        })
                    }
                }).collect();
                serde_json::json!({ "must": must_conditions })
            })
        });

        // Simulate sending a real gRPC/HTTP request to Qdrant if channel exists
        if let Some(_channel) = self.qdrant_channels.get("default") {
            // Note: In real gRPC, you would use a pre-generated client here with `_channel.clone()`
            let _payload_str = serde_json::to_string(&qdrant_request_payload).unwrap_or_default();
            // let mut client = qdrant_client::grpc::QdrantClient::new(_channel.clone());
            // let _res = client.search(tonic::Request::new(search_points)).await;
        }

        let mut ids = Vec::new();
        let mut vectors = Vec::new();
        for i in 0..top_k {
            ids.push(i);
            vectors.push(ndarray::Array1::from_elem(768, 0.1));
        }
        (ids, vectors)
    }

    /// Fetches bm25_scores from Postgres for the given ids
    pub async fn fetch_bm25_scores(&self, ids: &[usize]) -> Vec<f32> {
        if let Some(pool) = &self.postgres {
            use sqlx::Row;
            let ids_i64: Vec<i64> = ids.iter().map(|&x| x as i64).collect();
            let rows = sqlx::query("SELECT id, bm25_score FROM documents WHERE id = ANY($1)")
                .bind(&ids_i64[..])
                .fetch_all(pool).await;

            if let Ok(rows) = rows {
                let mut map = std::collections::HashMap::new();
                for row in rows {
                    let id: i64 = row.try_get("id").unwrap_or(-1);
                    let score: f32 = row.try_get("bm25_score").unwrap_or(1.0);
                    map.insert(id, score);
                }
                return ids_i64.iter().map(|&id| *map.get(&id).unwrap_or(&1.0)).collect();
            }
        }
        vec![1.0; ids.len()]
    }

    /// Fetches metadata from Postgres for the given ids
    pub async fn fetch_metadata(&self, ids: &[usize]) -> Vec<String> {
        if let Some(pool) = &self.postgres {
            use sqlx::Row;
            let ids_i64: Vec<i64> = ids.iter().map(|&x| x as i64).collect();
            let rows = sqlx::query("SELECT id, text FROM documents WHERE id = ANY($1)")
                .bind(&ids_i64[..])
                .fetch_all(pool).await;

            if let Ok(rows) = rows {
                let mut map = std::collections::HashMap::new();
                for row in rows {
                    let id: i64 = row.try_get("id").unwrap_or(-1);
                    let text: String = row.try_get("text").unwrap_or_default();
                    map.insert(id, text);
                }
                return ids_i64.iter().map(|&id| map.get(&id).cloned().unwrap_or_else(|| format!("Metadata for doc {}", id))).collect();
            }
        }
        ids.iter().map(|id| format!("Metadata for doc {}", id)).collect()
    }
}

impl Default for DbPools {
    fn default() -> Self {
        Self::new()
    }
}
