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
    pub async fn search_qdrant(&self, _query: &ndarray::Array1<f32>, _filters: &Option<Vec<crate::ir::Filter>>, top_k: usize) -> (Vec<usize>, Vec<ndarray::Array1<f32>>) {
        let mut ids = Vec::new();
        let mut vectors = Vec::new();
        for i in 0..top_k {
            ids.push(i);
            vectors.push(ndarray::Array1::from_elem(768, 0.1));
        }
        (ids, vectors)
    }

    /// Mock method for fetching bm25_scores from Postgres (Task 1.4.2)
    pub async fn fetch_bm25_scores(&self, ids: &[usize]) -> Vec<f32> {
        vec![1.0; ids.len()]
    }

    /// Mock method for fetching metadata from Postgres (Task 1.4.3)
    pub async fn fetch_metadata(&self, ids: &[usize]) -> Vec<String> {
        ids.iter().map(|id| format!("Metadata for doc {}", id)).collect()
    }
}

impl Default for DbPools {
    fn default() -> Self {
        Self::new()
    }
}
