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
}

impl Default for DbPools {
    fn default() -> Self {
        Self::new()
    }
}
