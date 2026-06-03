use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

pub const CB_CLOSED: u8 = 0;
pub const CB_OPEN: u8 = 1;
pub const CB_HALF_OPEN: u8 = 2;

pub struct CircuitBreaker {
    failures: AtomicU32,
    last_failure: RwLock<Option<Instant>>,
    threshold: u32,
    reset_timeout_secs: u64,
}

impl CircuitBreaker {
    pub fn new(threshold: u32, reset_timeout_secs: u64) -> Self {
        Self {
            failures: AtomicU32::new(0),
            last_failure: RwLock::new(None),
            threshold,
            reset_timeout_secs,
        }
    }

    pub fn is_open(&self) -> bool {
        if self.failures.load(Ordering::SeqCst) >= self.threshold {
            let last = *self.last_failure.read().unwrap();
            if let Some(time) = last {
                return time.elapsed().as_secs() < self.reset_timeout_secs;
            }
            return true;
        }
        false
    }

    /// Returns the circuit breaker state: 0=Closed, 1=Open, 2=Half-Open.
    pub fn state(&self) -> u8 {
        if self.failures.load(Ordering::SeqCst) < self.threshold {
            return CB_CLOSED;
        }
        let last = *self.last_failure.read().unwrap();
        match last {
            Some(time) if time.elapsed().as_secs() >= self.reset_timeout_secs => CB_HALF_OPEN,
            _ => CB_OPEN,
        }
    }

    pub fn record_failure(&self) {
        self.failures.fetch_add(1, Ordering::SeqCst);
        let mut last = self.last_failure.write().unwrap();
        *last = Some(Instant::now());
    }

    pub fn record_success(&self) {
        let mut last = self.last_failure.write().unwrap();
        *last = None;
        self.failures.store(0, Ordering::SeqCst);
    }
}
use qdrant_client::Qdrant;
use qdrant_client::qdrant::{
    Condition, FieldCondition, Filter as QFilter, Match, PointId, Range, SearchPoints,
    VectorsOutput, WithVectorsSelector, condition::ConditionOneOf, r#match::MatchValue,
    point_id::PointIdOptions, PointStruct, UpsertPoints,
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::collections::HashMap;
use tonic::transport::{Channel, Endpoint};

#[derive(Debug, Clone, serde::Serialize)]
pub struct StoredDocument {
    pub filename: String,
    pub doc_uuid: String,
    pub text: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CollectionHealth {
    pub collection_name: String,
    pub exists: bool,
    pub configured_dim: usize,
    pub actual_dim: Option<usize>,
    pub points_count: Option<u64>,
    pub dimension_matches: bool,
}

/// A simple connection pool manager for various databases and external ML services.
#[derive(Clone)]
pub struct DbPools {
    pub postgres: Option<PgPool>,
    pub qdrant_clients: HashMap<String, Qdrant>,
    pub ml_servers: HashMap<String, Channel>,
    pub qdrant_cb: Arc<CircuitBreaker>,
    pub postgres_cb: Arc<CircuitBreaker>,
    pub ml_cb: Arc<CircuitBreaker>,
}

impl DbPools {
    async fn recreate_default_collection(
        client: &Qdrant,
        collection: &str,
    ) -> Result<(), String> {
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

    fn extract_vector_dim(
        info: &qdrant_client::qdrant::CollectionInfo,
    ) -> Option<usize> {
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

    pub fn new() -> Self {
        let threshold = crate::config::get_cb_failure_threshold();
        let cooldown = crate::config::get_cb_cooldown_secs();
        Self {
            postgres: None,
            qdrant_clients: HashMap::new(),
            ml_servers: HashMap::new(),
            qdrant_cb: Arc::new(CircuitBreaker::new(threshold, cooldown)),
            postgres_cb: Arc::new(CircuitBreaker::new(threshold, cooldown)),
            ml_cb: Arc::new(CircuitBreaker::new(threshold, cooldown)),
        }
    }

    /// Initializes PostgreSQL pool
    pub async fn init_postgres(&mut self, database_url: &str) -> Result<(), sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await?;

        // Ensure columns for document grouping exist
        let _ = sqlx::query("ALTER TABLE documents ADD COLUMN IF NOT EXISTS filename TEXT;")
            .execute(&pool)
            .await;
        let _ = sqlx::query("ALTER TABLE documents ADD COLUMN IF NOT EXISTS doc_uuid TEXT;")
            .execute(&pool)
            .await;

        self.postgres = Some(pool);
        Ok(())
    }

    /// Adds a connection channel to a Qdrant node
    pub async fn add_qdrant_node(
        &mut self,
        node_id: &str,
        url: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let client = Qdrant::from_url(url).build()?;
        let collection = crate::config::get_qdrant_collection();

        if node_id == "default" {
            if client.collection_info(&collection).await.is_err() {
                tracing::info!("Creating Qdrant collection '{}'...", collection);
                let _ = client.create_collection(qdrant_client::qdrant::CreateCollection {
                    collection_name: collection.clone(),
                    vectors_config: Some(qdrant_client::qdrant::VectorsConfig {
                        config: Some(qdrant_client::qdrant::vectors_config::Config::Params(
                            qdrant_client::qdrant::VectorParams {
                                size: crate::config::get_embedding_dim() as u64,
                                distance: qdrant_client::qdrant::Distance::Cosine.into(),
                                ..Default::default()
                            }
                        ))
                    }),
                    ..Default::default()
                }).await;
            }
        }

        self.qdrant_clients.insert(node_id.to_string(), client);
        Ok(())
    }

    /// Adds a connection channel to an external ML server
    pub async fn add_ml_server(
        &mut self,
        server_id: &str,
        url: &str,
    ) -> Result<(), tonic::transport::Error> {
        let endpoint = Endpoint::from_shared(url.to_string())?;
        let channel = endpoint.connect().await?;
        self.ml_servers.insert(server_id.to_string(), channel);
        Ok(())
    }

    /// Fetches an embedding vector for the given text from the ML server
    pub async fn get_embedding(&self, text: &str, is_query: bool) -> Result<ndarray::Array1<f32>, String> {
        let dim = crate::config::get_embedding_dim();
        if cfg!(test) {
            return Ok(ndarray::Array1::from_elem(dim, 0.1));
        }

        if self.ml_cb.is_open() {
            return Err("ML server circuit breaker is open".to_string());
        }

        // Configure E5 instructions dynamically (with fallback to default E5 format)
        let formatted_text = if is_query {
            let template = std::env::var("EMBEDDING_QUERY_PREFIX")
                .unwrap_or_else(|_| "query: ".to_string());
            if template.contains("{}") {
                template.replace("{}", text)
            } else {
                format!("{}{}", template, text)
            }
        } else {
            let template = std::env::var("EMBEDDING_PASSAGE_PREFIX")
                .unwrap_or_else(|_| "passage: ".to_string());
            if template.contains("{}") {
                template.replace("{}", text)
            } else {
                format!("{}{}", template, text)
            }
        };

        tracing::debug!("E5 model embedding input text: {:?}", formatted_text);

        let url = std::env::var("EMBEDDING_SERVER_URL")
            .unwrap_or_else(|_| "http://localhost:11434/v1/embeddings".to_string());
        let model = crate::config::get_embedding_model_name();

        let client = reqwest::Client::new();
        let payload = serde_json::json!({
            "input": formatted_text,
            "model": model
        });

        match client.post(&url).json(&payload).send().await {
            Ok(resp) => {
                #[derive(serde::Deserialize)]
                struct EmbeddingData {
                    embedding: Vec<f32>,
                }
                #[derive(serde::Deserialize)]
                struct EmbeddingResponse {
                    data: Vec<EmbeddingData>,
                }

                let status = resp.status();
                if !status.is_success() {
                    let err_text = resp.text().await.unwrap_or_default();
                    self.ml_cb.record_failure();
                    return Err(format!("Embedding server returned error status ({}): {}", status, err_text));
                }

                match resp.json::<EmbeddingResponse>().await {
                    Ok(res_body) => {
                        if let Some(first) = res_body.data.first() {
                            if first.embedding.len() == dim {
                                self.ml_cb.record_success();
                                return Ok(ndarray::Array1::from(first.embedding.clone()));
                            } else {
                                self.ml_cb.record_failure();
                                return Err(format!(
                                    "Embedding dimension mismatch: expected {}, got {}",
                                    dim,
                                    first.embedding.len()
                                ));
                            }
                        } else {
                            self.ml_cb.record_failure();
                            return Err("Embedding response data vector is empty".to_string());
                        }
                    }
                    Err(e) => {
                        self.ml_cb.record_failure();
                        return Err(format!("Failed to parse embedding response JSON: {}", e));
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to fetch embedding: {}", e);
                self.ml_cb.record_failure();
                return Err(format!("Failed to connect to embedding server: {}", e));
            }
        }
    }

    /// Inserts text into Postgres documents, gets the id, then upserts the vector + id to Qdrant.
    pub async fn insert_document(
        &self,
        text: &str,
        vector: ndarray::Array1<f32>,
        filename: &str,
        doc_uuid: &str,
    ) -> Result<usize, String> {
        // 1. Write to Postgres
        let pool = self.postgres.as_ref().ok_or("PostgreSQL pool not initialized")?;

        let row = sqlx::query("INSERT INTO documents (text, bm25_score, filename, doc_uuid) VALUES ($1, $2, $3, $4) RETURNING id")
            .bind(text)
            .bind(0.0f32)
            .bind(filename)
            .bind(doc_uuid)
            .fetch_one(pool)
            .await
            .map_err(|e| format!("PostgreSQL insert failed: {}", e))?;

        use sqlx::Row;
        let id: i64 = row.try_get("id").map_err(|e| format!("Failed to get inserted id: {}", e))?;
        let doc_id = id as usize;

        // 2. Write vector to Qdrant
        let client = self.qdrant_clients.get("default").ok_or("Qdrant client not initialized")?;

        let mut payload = std::collections::HashMap::new();
        payload.insert("filename".to_string(), serde_json::Value::String(filename.to_string()));
        payload.insert("doc_uuid".to_string(), serde_json::Value::String(doc_uuid.to_string()));

        let collection = crate::config::get_qdrant_collection();
        let point = PointStruct::new(
            doc_id as u64,
            vector.to_vec(),
            payload,
        );

        let upsert_result = client.upsert_points(UpsertPoints {
            collection_name: collection.clone(),
            points: vec![point],
            wait: Some(true),
            ..Default::default()
        }).await;

        match upsert_result {
            Ok(_) => Ok(doc_id),
            Err(e) => Err(format!("Qdrant upsert failed: {}", e)),
        }
    }

    /// Deletes a document (all chunks sharing the same doc_uuid) from PostgreSQL and Qdrant
    pub async fn delete_document(&self, doc_id: usize) -> Result<(), String> {
        let pool = self.postgres.as_ref().ok_or("PostgreSQL pool not initialized")?;

        // 1. Find the doc_uuid for this doc_id
        let row_opt = sqlx::query("SELECT doc_uuid FROM documents WHERE id = $1")
            .bind(doc_id as i64)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("Failed to find doc_uuid: {}", e))?;

        let doc_uuid: Option<String> = match row_opt {
            Some(row) => {
                use sqlx::Row;
                row.try_get("doc_uuid").ok()
            }
            None => None,
        };

        let ids_to_delete: Vec<i64> = if let Some(ref uuid_str) = doc_uuid {
            if !uuid_str.is_empty() {
                // Find all document IDs with the same doc_uuid
                let rows = sqlx::query("SELECT id FROM documents WHERE doc_uuid = $1")
                    .bind(uuid_str)
                    .fetch_all(pool)
                    .await
                    .map_err(|e| format!("Failed to fetch doc IDs for uuid: {}", e))?;
                use sqlx::Row;
                rows.into_iter().map(|r| r.try_get::<i64, _>("id").unwrap_or(0)).collect()
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

        // 3. Delete all from Qdrant
        if let Some(client) = self.qdrant_clients.get("default") {
            let collection = crate::config::get_qdrant_collection();
            let points: Vec<PointId> = ids_to_delete.iter().map(|&id| PointId {
                point_id_options: Some(PointIdOptions::Num(id as u64)),
            }).collect();

            let _ = client.delete_points(qdrant_client::qdrant::DeletePoints {
                collection_name: collection.clone(),
                points: Some(qdrant_client::qdrant::PointsSelector {
                    points_selector_one_of: Some(
                        qdrant_client::qdrant::points_selector::PointsSelectorOneOf::Points(
                            qdrant_client::qdrant::PointsIdsList {
                                ids: points
                            }
                        )
                    )
                }),
                ..Default::default()
            }).await;
        }

        Ok(())
    }

    /// Clears all documents from PostgreSQL and Qdrant (preserving schema)
    pub async fn clear_documents(&self) -> Result<(), String> {
        let pool = self.postgres.as_ref().ok_or("PostgreSQL pool not initialized")?;
        sqlx::query("TRUNCATE TABLE documents RESTART IDENTITY")
            .execute(pool)
            .await
            .map_err(|e| format!("PostgreSQL truncate failed: {}", e))?;

        if let Some(client) = self.qdrant_clients.get("default") {
            let collection = crate::config::get_qdrant_collection();
            let _ = client.delete_points(qdrant_client::qdrant::DeletePoints {
                collection_name: collection.clone(),
                points: Some(qdrant_client::qdrant::PointsSelector {
                    points_selector_one_of: Some(
                        qdrant_client::qdrant::points_selector::PointsSelectorOneOf::Filter(
                            qdrant_client::qdrant::Filter::default()
                        )
                    )
                }),
                ..Default::default()
            }).await;
        }

        Ok(())
    }

    /// Removes all stored documents and recreates the default Qdrant collection schema.
    pub async fn hard_reset_database(&self) -> Result<(), String> {
        let pool = self.postgres.as_ref().ok_or("PostgreSQL pool not initialized")?;
        sqlx::query("TRUNCATE TABLE documents RESTART IDENTITY")
            .execute(pool)
            .await
            .map_err(|e| format!("PostgreSQL truncate failed: {}", e))?;

        if let Some(client) = self.qdrant_clients.get("default") {
            let collection = crate::config::get_qdrant_collection();
            let _ = client.delete_collection(&collection).await;
            Self::recreate_default_collection(client, &collection).await?;
        }

        Ok(())
    }

    /// Recreates the Qdrant collection with the current dimension and re-embeds all documents from PostgreSQL.
    pub async fn reindex_database(&self) -> Result<(), String> {
        let pool = self.postgres.as_ref().ok_or("PostgreSQL pool not initialized")?;

        // 1. Fetch all document chunks from PostgreSQL
        let rows = sqlx::query("SELECT id, text, filename, doc_uuid FROM documents ORDER BY id ASC")
            .fetch_all(pool)
            .await
            .map_err(|e| format!("Failed to fetch documents from PostgreSQL: {}", e))?;

        if rows.is_empty() {
            return Err("No documents found in PostgreSQL to reindex.".to_string());
        }

        // 2. Recreate Qdrant collection with the current configured dimension
        let client = self.qdrant_clients.get("default").ok_or("Qdrant client not initialized")?;
        let collection = crate::config::get_qdrant_collection();
        let _ = client.delete_collection(&collection).await;
        Self::recreate_default_collection(client, &collection).await?;

        // 3. Re-embed and upsert each chunk to Qdrant
        use sqlx::Row;
        for row in rows {
            let id: i64 = row.try_get("id").map_err(|e| e.to_string())?;
            let text: String = row.try_get("text").map_err(|e| e.to_string())?;
            let filename: String = row.try_get("filename").unwrap_or_else(|_| "Unknown".to_string());
            let doc_uuid: String = row.try_get("doc_uuid").unwrap_or_default();

            // Generate new embedding using the current dimension
            let vector = self.get_embedding(&text, false).await?;

            let mut payload = std::collections::HashMap::new();
            payload.insert("filename".to_string(), serde_json::Value::String(filename));
            payload.insert("doc_uuid".to_string(), serde_json::Value::String(doc_uuid));

            let point = PointStruct::new(
                id as u64,
                vector.to_vec(),
                payload,
            );

            client.upsert_points(UpsertPoints {
                collection_name: collection.clone(),
                points: vec![point],
                wait: Some(true),
                ..Default::default()
            }).await.map_err(|e| format!("Qdrant upsert failed: {}", e))?;
        }

        Ok(())
    }

    pub async fn get_collection_health(&self) -> CollectionHealth {
        let collection_name = crate::config::get_qdrant_collection();
        let configured_dim = crate::config::get_embedding_dim();

        if let Some(client) = self.qdrant_clients.get("default") {
            match client.collection_info(&collection_name).await {
                Ok(info) => {
                    let actual_dim = info.result.as_ref().and_then(|res| Self::extract_vector_dim(res));
                    let points_count = info.result.as_ref().and_then(|r| r.points_count).unwrap_or(0);
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

    /// Mock method for ANN search in Qdrant (Task 1.4.1, 1.4.6)
    #[allow(deprecated)]
    pub async fn search_qdrant(
        &self,
        query: &ndarray::Array1<f32>,
        filters: &Option<Vec<crate::ir::Filter>>,
        top_k: usize,
    ) -> Result<(Vec<usize>, Vec<ndarray::Array1<f32>>), String> {
        if self.qdrant_cb.is_open() {
            return Err("Circuit breaker is open for Qdrant".to_string());
        }
        let client_opt = self.qdrant_clients.get("default");

        if let Some(client) = client_opt {
            let collection = crate::config::get_qdrant_collection();
            let mut search_points = SearchPoints {
                collection_name: collection.clone(),
                vector: query.to_vec(),
                limit: top_k as u64,
                with_vectors: Some(WithVectorsSelector {
                    selector_options: Some(
                        qdrant_client::qdrant::with_vectors_selector::SelectorOptions::Enable(true),
                    ),
                }),
                with_payload: None,
                ..Default::default()
            };

            if let Some(filters) = filters {
                let must: Vec<Condition> = filters.iter()
                    .map(|filter| {
                        let condition_one_of = match filter.operator.as_str() {
                            "==" | "=" => {
                                ConditionOneOf::Field(FieldCondition {
                                    key: filter.field.clone(),
                                    r#match: Some(Match {
                                        match_value: match &filter.value {
                                            serde_json::Value::String(s) => Some(MatchValue::Keyword(s.clone())),
                                            serde_json::Value::Number(n) => n.as_i64().map(MatchValue::Integer),
                                            serde_json::Value::Bool(b) => Some(MatchValue::Boolean(*b)),
                                            _ => None
                                        }
                                    }),
                                    ..Default::default()
                                })
                            },
                            ">=" => {
                                ConditionOneOf::Field(FieldCondition {
                                    key: filter.field.clone(),
                                    range: Some(Range {
                                        gte: filter.value.as_f64(),
                                        ..Default::default()
                                    }),
                                    ..Default::default()
                                })
                            },
                            ">" => {
                                ConditionOneOf::Field(FieldCondition {
                                    key: filter.field.clone(),
                                    range: Some(Range {
                                        gt: filter.value.as_f64(),
                                        ..Default::default()
                                    }),
                                    ..Default::default()
                                })
                            },
                            "<=" => {
                                ConditionOneOf::Field(FieldCondition {
                                    key: filter.field.clone(),
                                    range: Some(Range {
                                        lte: filter.value.as_f64(),
                                        ..Default::default()
                                    }),
                                    ..Default::default()
                                })
                            },
                            "<" => {
                                ConditionOneOf::Field(FieldCondition {
                                    key: filter.field.clone(),
                                    range: Some(Range {
                                        lt: filter.value.as_f64(),
                                        ..Default::default()
                                    }),
                                    ..Default::default()
                                })
                            },
                            "in" => {
                                ConditionOneOf::Field(FieldCondition {
                                    key: filter.field.clone(),
                                    r#match: Some(Match {
                                        match_value: match &filter.value {
                                            serde_json::Value::Array(arr) => {
                                                if let Some(first) = arr.first() {
                                                    if first.is_string() {
                                                        Some(MatchValue::Keywords(qdrant_client::qdrant::RepeatedStrings { strings: arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect() }))
                                                    } else if first.is_i64() {
                                                        Some(MatchValue::Integers(qdrant_client::qdrant::RepeatedIntegers { integers: arr.iter().filter_map(|v| v.as_i64()).collect() }))
                                                    } else {
                                                        None
                                                    }
                                                } else {
                                                    None
                                                }
                                            },
                                            _ => None
                                        }
                                    }),
                                    ..Default::default()
                                })
                            },
                            _ => {
                                ConditionOneOf::Field(FieldCondition {
                                    key: filter.field.clone(),
                                    r#match: Some(Match {
                                        match_value: match &filter.value {
                                            serde_json::Value::String(s) => Some(MatchValue::Keyword(s.clone())),
                                            serde_json::Value::Number(n) => n.as_i64().map(MatchValue::Integer),
                                            serde_json::Value::Bool(b) => Some(MatchValue::Boolean(*b)),
                                            _ => None
                                        }
                                    }),
                                    ..Default::default()
                                })
                            }
                        };
                        Condition {
                            condition_one_of: Some(condition_one_of)
                        }
                    })
                    .collect();
                search_points.filter = Some(QFilter {
                    must,
                    ..Default::default()
                });
            }

            let response = match client.search_points(search_points).await {
                Ok(res) => res,
                Err(e) => {
                    self.qdrant_cb.record_failure();
                    return Err(e.to_string());
                }
            };

            let mut ids = Vec::new();
            let mut vectors = Vec::new();
            for point in &response.result {
                let id = match &point.id {
                    Some(PointId {
                        point_id_options: Some(PointIdOptions::Num(n)),
                    }) => *n as usize,
                    _ => continue,
                };
                ids.push(id);
                if let Some(VectorsOutput {
                    vectors_options:
                        Some(qdrant_client::qdrant::vectors_output::VectorsOptions::Vector(v)),
                }) = &point.vectors
                {
                    let vec_data = if let Some(qdrant_client::qdrant::vector_output::Vector::Dense(dense)) = &v.vector
                    {
                        dense.data.clone()
                    } else {
                        #[allow(deprecated)]
                        v.data.clone()
                    };
                    vectors.push(ndarray::Array1::from(vec_data));
                } else {
                    vectors.push(ndarray::Array1::from_elem(crate::config::get_embedding_dim(), 0.1));
                }
            }
            self.qdrant_cb.record_success();
            Ok((ids, vectors))
        } else {
            // Mock fallback
            let mut ids = Vec::new();
            let mut vectors = Vec::new();
            for i in 0..top_k {
                ids.push(i);
                vectors.push(ndarray::Array1::from_elem(crate::config::get_embedding_dim(), 0.1));
            }
            self.qdrant_cb.record_success();
            Ok((ids, vectors))
        }
    }

    /// Fetches bm25_scores from Postgres for the given ids
    pub async fn fetch_bm25_scores(&self, ids: &[usize]) -> Vec<f32> {
        if self.postgres_cb.is_open() {
            return vec![1.0; ids.len()]; // Fallback
        }
        if let Some(pool) = &self.postgres {
            use sqlx::Row;
            let ids_i64: Vec<i64> = ids.iter().map(|&x| x as i64).collect();
            let rows = sqlx::query("SELECT id, bm25_score FROM documents WHERE id = ANY($1)")
                .bind(&ids_i64[..])
                .fetch_all(pool)
                .await;

            if let Ok(rows) = rows {
                let mut map = std::collections::HashMap::new();
                for row in rows {
                    let id: i64 = row.try_get("id").unwrap_or(-1);
                    let score: f32 = row.try_get("bm25_score").unwrap_or(1.0);
                    map.insert(id, score);
                }
                self.postgres_cb.record_success();
                return ids_i64
                    .iter()
                    .map(|&id| *map.get(&id).unwrap_or(&1.0))
                    .collect();
            } else {
                self.postgres_cb.record_failure();
            }
        }
        vec![1.0; ids.len()]
    }

    /// Fetches metadata from Postgres for the given ids
    pub async fn fetch_metadata(&self, ids: &[usize]) -> Vec<String> {
        if self.postgres_cb.is_open() {
            return ids
                .iter()
                .map(|id| format!("Metadata for doc {}", id))
                .collect();
        }
        if let Some(pool) = &self.postgres {
            use sqlx::Row;
            let ids_i64: Vec<i64> = ids.iter().map(|&x| x as i64).collect();
            let rows = sqlx::query("SELECT id, text FROM documents WHERE id = ANY($1)")
                .bind(&ids_i64[..])
                .fetch_all(pool)
                .await;

            if let Ok(rows) = rows {
                let mut map = std::collections::HashMap::new();
                for row in rows {
                    let id: i64 = row.try_get("id").unwrap_or(-1);
                    let text: String = row.try_get("text").unwrap_or_default();
                    map.insert(id, text);
                }
                self.postgres_cb.record_success();
                return ids_i64
                    .iter()
                    .map(|&id| {
                        map.get(&id)
                            .cloned()
                            .unwrap_or_else(|| format!("Metadata for doc {}", id))
                    })
                    .collect();
            } else {
                self.postgres_cb.record_failure();
            }
        }
        ids.iter()
            .map(|id| format!("Metadata for doc {}", id))
            .collect()
    }

    pub async fn fetch_document_by_uuid(
        &self,
        doc_uuid: &str,
    ) -> Result<Option<StoredDocument>, String> {
        let pool = self
            .postgres
            .as_ref()
            .ok_or("PostgreSQL pool not initialized".to_string())?;

        use sqlx::Row;
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
}

impl Default for DbPools {
    fn default() -> Self {
        Self::new()
    }
}
