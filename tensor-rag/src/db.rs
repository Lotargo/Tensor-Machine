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
    Condition, FieldCondition, Filter as QFilter, Match, PointId, PointStruct, Range, SearchPoints,
    UpsertPoints, VectorsOutput, WithVectorsSelector, condition::ConditionOneOf,
    r#match::MatchValue, point_id::PointIdOptions,
};
use sqlx::{PgPool, Postgres, QueryBuilder, postgres::PgPoolOptions};
use std::collections::HashMap;
use std::time::Duration;
use tonic::transport::{Channel, Endpoint};

#[derive(Debug, Clone, serde::Serialize)]
pub struct StoredDocument {
    pub filename: String,
    pub doc_uuid: String,
    pub text: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChunkDetail {
    pub id: usize,
    pub text: String,
    pub filename: String,
    pub doc_uuid: String,
    pub chapter_id: String,
    pub chunk_index: i32,
    pub chunk_level: String,
    pub parent_chunk_index: Option<i32>,
    pub macro_index: Option<i32>,
    pub medium_index: Option<i32>,
    pub header_chain: Vec<String>,
    pub zone_keywords: Vec<String>,
    pub entities: Vec<String>,
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

pub struct EnrichedDocumentInsert<'a> {
    pub text: &'a str,
    pub vector: ndarray::Array1<f32>,
    pub filename: &'a str,
    pub doc_uuid: &'a str,
    pub chapter_id: &'a str,
    pub chunk_index: i32,
    pub chunk_level: &'a str,
    pub parent_chunk_index: Option<i32>,
    pub macro_index: Option<i32>,
    pub medium_index: Option<i32>,
    pub header_chain: &'a [String],
    pub zone_keywords: &'a [String],
    pub entities: &'a [String],
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
    fn format_embedding_input(text: &str, is_query: bool) -> String {
        if is_query {
            let template =
                std::env::var("EMBEDDING_QUERY_PREFIX").unwrap_or_else(|_| "query: ".to_string());
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
        }
    }

    async fn recreate_default_collection(client: &Qdrant, collection: &str) -> Result<(), String> {
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

    fn extract_vector_dim(info: &qdrant_client::qdrant::CollectionInfo) -> Option<usize> {
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
        let _ = sqlx::query(
            "ALTER TABLE documents ADD COLUMN IF NOT EXISTS metadata JSONB NOT NULL DEFAULT '{}';",
        )
        .execute(&pool)
        .await;
        let _ = sqlx::query("ALTER TABLE documents ADD COLUMN IF NOT EXISTS chapter_id TEXT;")
            .execute(&pool)
            .await;
        let _ = sqlx::query("ALTER TABLE documents ADD COLUMN IF NOT EXISTS chunk_index INTEGER;")
            .execute(&pool)
            .await;
        let _ = sqlx::query("ALTER TABLE documents ADD COLUMN IF NOT EXISTS chunk_level TEXT NOT NULL DEFAULT 'medium';")
            .execute(&pool)
            .await;
        let _ = sqlx::query(
            "ALTER TABLE documents ADD COLUMN IF NOT EXISTS parent_chunk_index INTEGER;",
        )
        .execute(&pool)
        .await;
        let _ = sqlx::query("ALTER TABLE documents ADD COLUMN IF NOT EXISTS macro_index INTEGER;")
            .execute(&pool)
            .await;
        let _ = sqlx::query("ALTER TABLE documents ADD COLUMN IF NOT EXISTS medium_index INTEGER;")
            .execute(&pool)
            .await;
        let _ = sqlx::query("ALTER TABLE documents ADD COLUMN IF NOT EXISTS header_chain JSONB NOT NULL DEFAULT '[]';")
            .execute(&pool)
            .await;
        let _ = sqlx::query("ALTER TABLE documents ADD COLUMN IF NOT EXISTS zone_keywords JSONB NOT NULL DEFAULT '[]';")
            .execute(&pool)
            .await;
        let _ = sqlx::query(
            "ALTER TABLE documents ADD COLUMN IF NOT EXISTS entities JSONB NOT NULL DEFAULT '[]';",
        )
        .execute(&pool)
        .await;
        let _ = sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_documents_group ON documents(doc_uuid, chunk_index);",
        )
        .execute(&pool)
        .await;
        let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_documents_hierarchy ON documents(doc_uuid, chunk_level, parent_chunk_index);")
            .execute(&pool)
            .await;
        let _ = sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_documents_entities ON documents USING GIN (entities);",
        )
        .execute(&pool)
        .await;

        // Create anchors and edges tables
        let _ = sqlx::query(
            "CREATE TABLE IF NOT EXISTS anchors (
                id BIGSERIAL PRIMARY KEY,
                doc_uuid TEXT NOT NULL,
                name TEXT NOT NULL,
                text TEXT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );",
        )
        .execute(&pool)
        .await;

        let _ =
            sqlx::query("CREATE INDEX IF NOT EXISTS idx_anchors_doc_uuid ON anchors(doc_uuid);")
                .execute(&pool)
                .await;

        let _ =
            sqlx::query("CREATE INDEX IF NOT EXISTS idx_anchors_name ON anchors(doc_uuid, name);")
                .execute(&pool)
                .await;

        let _ = sqlx::query(
            "CREATE TABLE IF NOT EXISTS edges (
                id BIGSERIAL PRIMARY KEY,
                source_doc_uuid TEXT NOT NULL,
                target_doc_uuid TEXT NOT NULL,
                edge_type TEXT NOT NULL,
                metadata JSONB NOT NULL DEFAULT '{}',
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );",
        )
        .execute(&pool)
        .await;

        let _ =
            sqlx::query("CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source_doc_uuid);")
                .execute(&pool)
                .await;

        let _ =
            sqlx::query("CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target_doc_uuid);")
                .execute(&pool)
                .await;

        let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_edges_type ON edges(edge_type);")
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
        let anchors_collection = crate::config::get_qdrant_anchors_collection();

        if node_id == "default" {
            // Create main collection
            if client.collection_info(&collection).await.is_err() {
                tracing::info!("Creating Qdrant collection '{}'...", collection);
                let _ = client
                    .create_collection(qdrant_client::qdrant::CreateCollection {
                        collection_name: collection.clone(),
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
                    .await;
            }

            // Create anchors collection
            if client.collection_info(&anchors_collection).await.is_err() {
                tracing::info!(
                    "Creating Qdrant anchors collection '{}'...",
                    anchors_collection
                );
                let _ = client
                    .create_collection(qdrant_client::qdrant::CreateCollection {
                        collection_name: anchors_collection.clone(),
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
                    .await;
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
    pub async fn get_embedding(
        &self,
        text: &str,
        is_query: bool,
    ) -> Result<ndarray::Array1<f32>, String> {
        let mut vectors = self.get_embeddings(&[text.to_string()], is_query).await?;
        vectors
            .pop()
            .ok_or_else(|| "Embedding response data vector is empty".to_string())
    }

    pub async fn get_embeddings(
        &self,
        texts: &[String],
        is_query: bool,
    ) -> Result<Vec<ndarray::Array1<f32>>, String> {
        let dim = crate::config::get_embedding_dim();
        if cfg!(test) {
            return Ok(texts
                .iter()
                .map(|_| ndarray::Array1::from_elem(dim, 0.1))
                .collect());
        }

        if texts.is_empty() {
            return Ok(Vec::new());
        }

        if self.ml_cb.is_open() {
            return Err("ML server circuit breaker is open".to_string());
        }

        let formatted_inputs: Vec<String> = texts
            .iter()
            .map(|text| Self::format_embedding_input(text, is_query))
            .collect();

        let url = std::env::var("EMBEDDING_SERVER_URL")
            .unwrap_or_else(|_| "http://localhost:11434/v1/embeddings".to_string());
        let model = crate::config::get_embedding_model_name();

        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(
                crate::config::get_http_connect_timeout_secs(),
            ))
            .timeout(Duration::from_secs(
                crate::config::get_embedding_request_timeout_secs(),
            ))
            .build()
            .map_err(|e| format!("Failed to build embedding HTTP client: {}", e))?;
        let input_payload = if formatted_inputs.len() == 1 {
            serde_json::Value::String(formatted_inputs[0].clone())
        } else {
            serde_json::json!(formatted_inputs)
        };
        let payload = serde_json::json!({
            "input": input_payload,
            "model": model
        });

        tracing::info!(
            "Requesting embeddings from '{}' with model '{}' (batch_size={}, total_chars={}, is_query={})",
            url,
            model,
            texts.len(),
            texts.iter().map(|text| text.len()).sum::<usize>(),
            is_query
        );

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
                    return Err(format!(
                        "Embedding server returned error status ({}): {}",
                        status, err_text
                    ));
                }

                match resp.json::<EmbeddingResponse>().await {
                    Ok(res_body) => {
                        if res_body.data.is_empty() {
                            self.ml_cb.record_failure();
                            return Err("Embedding response data vector is empty".to_string());
                        }

                        if res_body.data.len() != texts.len() {
                            self.ml_cb.record_failure();
                            return Err(format!(
                                "Embedding batch size mismatch: requested {}, got {}",
                                texts.len(),
                                res_body.data.len()
                            ));
                        }

                        let mut vectors = Vec::with_capacity(res_body.data.len());
                        for item in res_body.data {
                            if item.embedding.len() != dim {
                                self.ml_cb.record_failure();
                                return Err(format!(
                                    "Embedding dimension mismatch: expected {}, got {}",
                                    dim,
                                    item.embedding.len()
                                ));
                            }
                            vectors.push(ndarray::Array1::from(item.embedding));
                        }

                        self.ml_cb.record_success();
                        tracing::info!(
                            "Embedding batch completed successfully (batch_size={}, dim={})",
                            vectors.len(),
                            dim
                        );
                        Ok(vectors)
                    }
                    Err(e) => {
                        self.ml_cb.record_failure();
                        Err(format!("Failed to parse embedding response JSON: {}", e))
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to fetch embedding: {}", e);
                self.ml_cb.record_failure();
                Err(format!("Failed to connect to embedding server: {}", e))
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
        chapter_id: &str,
        chunk_index: i32,
    ) -> Result<usize, String> {
        let ids = self
            .insert_documents_enriched_batch(vec![EnrichedDocumentInsert {
                text,
                vector,
                filename,
                doc_uuid,
                chapter_id,
                chunk_index,
                chunk_level: "medium",
                parent_chunk_index: None,
                macro_index: None,
                medium_index: None,
                header_chain: &[],
                zone_keywords: &[],
                entities: &[],
            }])
            .await?;
        ids.into_iter()
            .next()
            .ok_or_else(|| "Document insert returned no ids".to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_document_enriched(
        &self,
        text: &str,
        vector: ndarray::Array1<f32>,
        filename: &str,
        doc_uuid: &str,
        chapter_id: &str,
        chunk_index: i32,
        chunk_level: &str,
        parent_chunk_index: Option<i32>,
        macro_index: Option<i32>,
        medium_index: Option<i32>,
        header_chain: &[String],
        zone_keywords: &[String],
        entities: &[String],
    ) -> Result<usize, String> {
        let ids = self
            .insert_documents_enriched_batch(vec![EnrichedDocumentInsert {
                text,
                vector,
                filename,
                doc_uuid,
                chapter_id,
                chunk_index,
                chunk_level,
                parent_chunk_index,
                macro_index,
                medium_index,
                header_chain,
                zone_keywords,
                entities,
            }])
            .await?;
        ids.into_iter()
            .next()
            .ok_or_else(|| "Document insert returned no ids".to_string())
    }

    fn build_document_payload(
        filename: &str,
        doc_uuid: &str,
        chapter_id: &str,
        chunk_index: i32,
        chunk_level: &str,
        parent_chunk_index: Option<i32>,
        macro_index: Option<i32>,
        medium_index: Option<i32>,
        entities_json: serde_json::Value,
    ) -> std::collections::HashMap<String, serde_json::Value> {
        let mut payload = std::collections::HashMap::new();
        payload.insert(
            "filename".to_string(),
            serde_json::Value::String(filename.to_string()),
        );
        payload.insert(
            "doc_uuid".to_string(),
            serde_json::Value::String(doc_uuid.to_string()),
        );
        payload.insert(
            "chapter_id".to_string(),
            serde_json::Value::String(chapter_id.to_string()),
        );
        payload.insert(
            "chunk_index".to_string(),
            serde_json::Value::Number(serde_json::Number::from(chunk_index)),
        );
        payload.insert(
            "chunk_level".to_string(),
            serde_json::Value::String(chunk_level.to_string()),
        );
        if let Some(parent) = parent_chunk_index {
            payload.insert(
                "parent_chunk_index".to_string(),
                serde_json::Value::Number(parent.into()),
            );
        }
        if let Some(idx) = macro_index {
            payload.insert(
                "macro_index".to_string(),
                serde_json::Value::Number(idx.into()),
            );
        }
        if let Some(idx) = medium_index {
            payload.insert(
                "medium_index".to_string(),
                serde_json::Value::Number(idx.into()),
            );
        }
        payload.insert("entities".to_string(), entities_json);
        payload
    }

    pub async fn insert_documents_enriched_batch(
        &self,
        documents: Vec<EnrichedDocumentInsert<'_>>,
    ) -> Result<Vec<usize>, String> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }

        // 1. Write rows to Postgres so Qdrant point IDs match database IDs.
        let pool = self
            .postgres
            .as_ref()
            .ok_or("PostgreSQL pool not initialized")?;

        let header_chain_json: Vec<serde_json::Value> = documents
            .iter()
            .map(|document| serde_json::json!(document.header_chain))
            .collect();
        let zone_keywords_json: Vec<serde_json::Value> = documents
            .iter()
            .map(|document| serde_json::json!(document.zone_keywords))
            .collect();
        let entities_json: Vec<serde_json::Value> = documents
            .iter()
            .map(|document| serde_json::json!(document.entities))
            .collect();

        let mut builder = QueryBuilder::<Postgres>::new(
            "INSERT INTO documents (
                text, bm25_score, filename, doc_uuid, chapter_id, chunk_index,
                chunk_level, parent_chunk_index, macro_index, medium_index,
                header_chain, zone_keywords, entities
            ) ",
        );
        builder.push_values(
            documents
                .iter()
                .zip(header_chain_json.iter())
                .zip(zone_keywords_json.iter())
                .zip(entities_json.iter()),
            |mut row, (((document, header_chain), zone_keywords), entities)| {
                row.push_bind(document.text)
                    .push_bind(0.0f32)
                    .push_bind(document.filename)
                    .push_bind(document.doc_uuid)
                    .push_bind(document.chapter_id)
                    .push_bind(document.chunk_index)
                    .push_bind(document.chunk_level)
                    .push_bind(document.parent_chunk_index)
                    .push_bind(document.macro_index)
                    .push_bind(document.medium_index)
                    .push_bind(header_chain)
                    .push_bind(zone_keywords)
                    .push_bind(entities);
            },
        );
        builder.push(" RETURNING id");

        use sqlx::Row;
        let rows = builder
            .build()
            .fetch_all(pool)
            .await
            .map_err(|e| format!("PostgreSQL batch insert failed: {}", e))?;

        let mut doc_ids = Vec::with_capacity(documents.len());
        for row in rows {
            let id: i64 = row
                .try_get("id")
                .map_err(|e| format!("Failed to get inserted id: {}", e))?;
            doc_ids.push(id as usize);
        }

        if doc_ids.len() != documents.len() {
            return Err(format!(
                "PostgreSQL batch insert returned {} ids for {} documents",
                doc_ids.len(),
                documents.len()
            ));
        }

        let mut points = Vec::with_capacity(documents.len());
        for (document, (doc_id, entities_json)) in documents
            .iter()
            .zip(doc_ids.iter().copied().zip(entities_json.into_iter()))
        {
            let payload = Self::build_document_payload(
                document.filename,
                document.doc_uuid,
                document.chapter_id,
                document.chunk_index,
                document.chunk_level,
                document.parent_chunk_index,
                document.macro_index,
                document.medium_index,
                entities_json,
            );
            points.push(PointStruct::new(
                doc_id as u64,
                document.vector.to_vec(),
                payload,
            ));
        }

        // 2. Write vectors to Qdrant in one upsert for the whole embedding batch.
        let client = self
            .qdrant_clients
            .get("default")
            .ok_or("Qdrant client not initialized")?;
        let collection = crate::config::get_qdrant_collection();

        let upsert_result = client
            .upsert_points(UpsertPoints {
                collection_name: collection.clone(),
                points,
                wait: Some(crate::config::get_qdrant_upsert_wait()),
                ..Default::default()
            })
            .await;

        match upsert_result {
            Ok(_) => Ok(doc_ids),
            Err(e) => Err(format!("Qdrant upsert failed: {}", e)),
        }
    }

    /// Deletes a document (all chunks sharing the same doc_uuid) from PostgreSQL and Qdrant
    pub async fn delete_document(&self, doc_id: usize) -> Result<(), String> {
        let pool = self
            .postgres
            .as_ref()
            .ok_or("PostgreSQL pool not initialized")?;

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

        let mut anchor_ids_to_delete = Vec::new();
        let ids_to_delete: Vec<i64> = if let Some(ref uuid_str) = doc_uuid {
            if !uuid_str.is_empty() {
                // Delete associated graph edges first
                crate::graph::delete_document_edges(pool, uuid_str).await?;

                // Collect anchor IDs so their embeddings can be removed from Qdrant too
                let anchor_rows = sqlx::query("SELECT id FROM anchors WHERE doc_uuid = $1")
                    .bind(uuid_str)
                    .fetch_all(pool)
                    .await
                    .map_err(|e| format!("Failed to fetch anchor IDs for uuid: {}", e))?;
                use sqlx::Row;
                anchor_ids_to_delete = anchor_rows
                    .into_iter()
                    .map(|r| r.try_get::<i64, _>("id").unwrap_or(0))
                    .filter(|id| *id > 0)
                    .collect();

                // Find all document IDs with the same doc_uuid
                let rows = sqlx::query("SELECT id FROM documents WHERE doc_uuid = $1")
                    .bind(uuid_str)
                    .fetch_all(pool)
                    .await
                    .map_err(|e| format!("Failed to fetch doc IDs for uuid: {}", e))?;
                rows.into_iter()
                    .map(|r| r.try_get::<i64, _>("id").unwrap_or(0))
                    .collect()
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

        if let Some(ref uuid_str) = doc_uuid {
            if !uuid_str.is_empty() {
                sqlx::query("DELETE FROM anchors WHERE doc_uuid = $1")
                    .bind(uuid_str)
                    .execute(pool)
                    .await
                    .map_err(|e| format!("Anchor delete failed: {}", e))?;
            }
        }

        // 3. Delete all from Qdrant
        if let Some(client) = self.qdrant_clients.get("default") {
            let collection = crate::config::get_qdrant_collection();
            let points: Vec<PointId> = ids_to_delete
                .iter()
                .map(|&id| PointId {
                    point_id_options: Some(PointIdOptions::Num(id as u64)),
                })
                .collect();

            let _ = client
                .delete_points(qdrant_client::qdrant::DeletePoints {
                    collection_name: collection.clone(),
                    points: Some(qdrant_client::qdrant::PointsSelector {
                        points_selector_one_of: Some(
                            qdrant_client::qdrant::points_selector::PointsSelectorOneOf::Points(
                                qdrant_client::qdrant::PointsIdsList { ids: points },
                            ),
                        ),
                    }),
                    ..Default::default()
                })
                .await;

            if !anchor_ids_to_delete.is_empty() {
                let anchors_collection = crate::config::get_qdrant_anchors_collection();
                let anchor_points: Vec<PointId> = anchor_ids_to_delete
                    .iter()
                    .map(|&id| PointId {
                        point_id_options: Some(PointIdOptions::Num(id as u64)),
                    })
                    .collect();

                let _ = client
                    .delete_points(qdrant_client::qdrant::DeletePoints {
                        collection_name: anchors_collection,
                        points: Some(qdrant_client::qdrant::PointsSelector {
                            points_selector_one_of: Some(
                                qdrant_client::qdrant::points_selector::PointsSelectorOneOf::Points(
                                    qdrant_client::qdrant::PointsIdsList { ids: anchor_points },
                                ),
                            ),
                        }),
                        ..Default::default()
                    })
                    .await;
            }
        }

        Ok(())
    }

    /// Clears all documents from PostgreSQL and Qdrant (preserving schema)
    pub async fn clear_documents(&self) -> Result<(), String> {
        let pool = self
            .postgres
            .as_ref()
            .ok_or("PostgreSQL pool not initialized")?;
        sqlx::query("TRUNCATE TABLE documents, anchors, edges RESTART IDENTITY")
            .execute(pool)
            .await
            .map_err(|e| format!("PostgreSQL truncate failed: {}", e))?;

        if let Some(client) = self.qdrant_clients.get("default") {
            let collection = crate::config::get_qdrant_collection();
            let anchors_collection = crate::config::get_qdrant_anchors_collection();
            let _ = client
                .delete_points(qdrant_client::qdrant::DeletePoints {
                    collection_name: collection.clone(),
                    points: Some(qdrant_client::qdrant::PointsSelector {
                        points_selector_one_of: Some(
                            qdrant_client::qdrant::points_selector::PointsSelectorOneOf::Filter(
                                qdrant_client::qdrant::Filter::default(),
                            ),
                        ),
                    }),
                    ..Default::default()
                })
                .await;
            let _ = client
                .delete_points(qdrant_client::qdrant::DeletePoints {
                    collection_name: anchors_collection,
                    points: Some(qdrant_client::qdrant::PointsSelector {
                        points_selector_one_of: Some(
                            qdrant_client::qdrant::points_selector::PointsSelectorOneOf::Filter(
                                qdrant_client::qdrant::Filter::default(),
                            ),
                        ),
                    }),
                    ..Default::default()
                })
                .await;
        }

        Ok(())
    }

    /// Removes all stored documents and recreates the default Qdrant collection schema.
    pub async fn hard_reset_database(&self) -> Result<(), String> {
        let pool = self
            .postgres
            .as_ref()
            .ok_or("PostgreSQL pool not initialized")?;
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
        let pool = self
            .postgres
            .as_ref()
            .ok_or("PostgreSQL pool not initialized")?;

        // 1. Fetch all document chunks from PostgreSQL
        let rows =
            sqlx::query("SELECT id, text, filename, doc_uuid FROM documents ORDER BY id ASC")
                .fetch_all(pool)
                .await
                .map_err(|e| format!("Failed to fetch documents from PostgreSQL: {}", e))?;

        if rows.is_empty() {
            return Err("No documents found in PostgreSQL to reindex.".to_string());
        }

        // 2. Recreate Qdrant collection with the current configured dimension
        let client = self
            .qdrant_clients
            .get("default")
            .ok_or("Qdrant client not initialized")?;
        let collection = crate::config::get_qdrant_collection();
        let _ = client.delete_collection(&collection).await;
        Self::recreate_default_collection(client, &collection).await?;

        // 3. Re-embed and upsert each chunk to Qdrant
        use sqlx::Row;
        for row in rows {
            let id: i64 = row.try_get("id").map_err(|e| e.to_string())?;
            let text: String = row.try_get("text").map_err(|e| e.to_string())?;
            let filename: String = row
                .try_get("filename")
                .unwrap_or_else(|_| "Unknown".to_string());
            let doc_uuid: String = row.try_get("doc_uuid").unwrap_or_default();

            // Generate new embedding using the current dimension
            let vector = self.get_embedding(&text, false).await?;

            let mut payload = std::collections::HashMap::new();
            payload.insert("filename".to_string(), serde_json::Value::String(filename));
            payload.insert("doc_uuid".to_string(), serde_json::Value::String(doc_uuid));

            let point = PointStruct::new(id as u64, vector.to_vec(), payload);

            client
                .upsert_points(UpsertPoints {
                    collection_name: collection.clone(),
                    points: vec![point],
                    wait: Some(true),
                    ..Default::default()
                })
                .await
                .map_err(|e| format!("Qdrant upsert failed: {}", e))?;
        }

        Ok(())
    }

    pub async fn get_collection_health(&self) -> CollectionHealth {
        let collection_name = crate::config::get_qdrant_collection();
        let configured_dim = crate::config::get_embedding_dim();

        if let Some(client) = self.qdrant_clients.get("default") {
            match client.collection_info(&collection_name).await {
                Ok(info) => {
                    let actual_dim = info
                        .result
                        .as_ref()
                        .and_then(|res| Self::extract_vector_dim(res));
                    let points_count = info
                        .result
                        .as_ref()
                        .and_then(|r| r.points_count)
                        .unwrap_or(0);
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
                    let vec_data =
                        if let Some(qdrant_client::qdrant::vector_output::Vector::Dense(dense)) =
                            &v.vector
                        {
                            dense.data.clone()
                        } else {
                            #[allow(deprecated)]
                            v.data.clone()
                        };
                    vectors.push(ndarray::Array1::from(vec_data));
                } else {
                    vectors.push(ndarray::Array1::from_elem(
                        crate::config::get_embedding_dim(),
                        0.1,
                    ));
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
                vectors.push(ndarray::Array1::from_elem(
                    crate::config::get_embedding_dim(),
                    0.1,
                ));
            }
            self.qdrant_cb.record_success();
            Ok((ids, vectors))
        }
    }

    /// Fetches bm25_scores from Postgres for the given ids
    pub async fn fetch_bm25_scores(&self, ids: &[usize], query: &str) -> Vec<f32> {
        if self.postgres_cb.is_open() {
            return vec![1.0; ids.len()]; // Fallback
        }
        if let Some(pool) = &self.postgres {
            use sqlx::Row;
            let ids_i64: Vec<i64> = ids.iter().map(|&x| x as i64).collect();
            let lexical_terms = tokenize_lexical_query(query);
            let tsquery_or = build_tsquery_or(&lexical_terms);
            let phrase_query = query.trim();
            let phrase_pattern = format!("%{}%", escape_like_pattern(phrase_query));

            let rows = if !tsquery_or.is_empty() || !phrase_query.is_empty() {
                sqlx::query(
                    r#"
                    SELECT
                        id,
                        (
                            CASE
                                WHEN $2 <> '' AND lower(text) LIKE lower($3) ESCAPE '\' THEN 5.0
                                ELSE 0.0
                            END
                            + CASE
                                WHEN $2 <> '' THEN ts_rank_cd(to_tsvector('simple', text), websearch_to_tsquery('simple', $2))
                                ELSE 0.0
                            END
                            + CASE
                                WHEN $4 <> '' THEN ts_rank_cd(to_tsvector('simple', text), to_tsquery('simple', $4))
                                ELSE 0.0
                            END
                        )::real AS rank
                    FROM documents
                    WHERE id = ANY($1)
                    "#,
                )
                .bind(&ids_i64[..])
                .bind(phrase_query)
                .bind(&phrase_pattern)
                .bind(&tsquery_or)
                .fetch_all(pool)
                .await
            } else {
                sqlx::query(
                    "SELECT id, 0.0::real as rank 
                     FROM documents 
                     WHERE id = ANY($1)",
                )
                .bind(&ids_i64[..])
                .fetch_all(pool)
                .await
            };

            if let Ok(rows) = rows {
                let mut map = std::collections::HashMap::new();
                for row in rows {
                    let id: i64 = row.try_get("id").unwrap_or(-1);
                    let score: f32 = row.try_get("rank").unwrap_or(0.0);
                    map.insert(id, score);
                }
                self.postgres_cb.record_success();
                return ids_i64
                    .iter()
                    .map(|&id| *map.get(&id).unwrap_or(&0.0))
                    .collect();
            } else {
                self.postgres_cb.record_failure();
            }
        }
        vec![0.0; ids.len()]
    }

    pub async fn search_keyword_candidate_ids(
        &self,
        query: &str,
        doc_uuid: Option<&str>,
        chapter_id: Option<&str>,
        limit: usize,
    ) -> Vec<usize> {
        let Some(pool) = &self.postgres else {
            return Vec::new();
        };

        let phrase_query = query.trim();
        let lexical_terms = tokenize_lexical_query(query);
        let tsquery_or = build_tsquery_or(&lexical_terms);
        if phrase_query.is_empty() && tsquery_or.is_empty() {
            return Vec::new();
        }

        let phrase_pattern = format!("%{}%", escape_like_pattern(phrase_query));
        let mut builder = QueryBuilder::<Postgres>::new(
            r#"
            SELECT id
            FROM documents
            WHERE 1=1
            "#,
        );

        if let Some(uuid) = doc_uuid {
            builder.push(" AND doc_uuid = ").push_bind(uuid);
        }
        if let Some(chapter) = chapter_id {
            builder.push(" AND chapter_id = ").push_bind(chapter);
        }
        builder
            .push(" AND (")
            .push("lower(text) LIKE lower(")
            .push_bind(&phrase_pattern)
            .push(") ESCAPE '\\'")
            .push(" OR to_tsvector('simple', text) @@ websearch_to_tsquery('simple', ")
            .push_bind(phrase_query)
            .push(")")
            .push(" OR to_tsvector('simple', text) @@ to_tsquery('simple', ")
            .push_bind(&tsquery_or)
            .push("))")
            .push(
                r#"
                ORDER BY (
                    CASE
                        WHEN lower(text) LIKE lower("#,
            )
            .push_bind(&phrase_pattern)
            .push(
                r#") ESCAPE '\' THEN 100.0 ELSE 0.0 END
                    + CASE
                        WHEN "#,
            )
            .push_bind(phrase_query)
            .push(
                r#" <> '' THEN ts_rank_cd(to_tsvector('simple', text), websearch_to_tsquery('simple', "#,
            )
            .push_bind(phrase_query)
            .push(
                r#")) ELSE 0.0 END
                    + CASE
                        WHEN "#,
            )
            .push_bind(&tsquery_or)
            .push(
                r#" <> '' THEN ts_rank_cd(to_tsvector('simple', text), to_tsquery('simple', "#,
            )
            .push_bind(&tsquery_or)
            .push(
                r#")) ELSE 0.0 END
                ) DESC,
                chunk_index ASC
                LIMIT "#,
            );
        builder.push_bind(limit as i64);

        let query = builder.build();
        match query.fetch_all(pool).await {
            Ok(rows) => {
                use sqlx::Row;
                rows.into_iter()
                    .filter_map(|row| row.try_get::<i64, _>("id").ok())
                    .map(|id| id as usize)
                    .collect()
            }
            Err(error) => {
                tracing::warn!("Keyword candidate search failed: {}", error);
                Vec::new()
            }
        }
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

    pub async fn fetch_chunk_details(&self, ids: &[usize]) -> Result<Vec<ChunkDetail>, String> {
        let pool = self
            .postgres
            .as_ref()
            .ok_or("PostgreSQL pool not initialized")?;
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        use sqlx::Row;
        let ids_i64: Vec<i64> = ids.iter().map(|&x| x as i64).collect();
        let rows = sqlx::query(
            "SELECT id, text,
                    COALESCE(filename, '') as filename,
                    COALESCE(doc_uuid, '') as doc_uuid,
                    COALESCE(chapter_id, '') as chapter_id,
                    COALESCE(chunk_index, 0) as chunk_index,
                    COALESCE(chunk_level, 'medium') as chunk_level,
                    parent_chunk_index, macro_index, medium_index,
                    COALESCE(header_chain, '[]'::jsonb) as header_chain,
                    COALESCE(zone_keywords, '[]'::jsonb) as zone_keywords,
                    COALESCE(entities, '[]'::jsonb) as entities
             FROM documents WHERE id = ANY($1)",
        )
        .bind(&ids_i64[..])
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to fetch chunk details: {}", e))?;

        let mut details = Vec::new();
        for row in rows {
            let id: i64 = row.try_get("id").unwrap_or(-1);
            let text: String = row.try_get("text").unwrap_or_default();
            let filename: String = row.try_get("filename").unwrap_or_default();
            let doc_uuid: String = row.try_get("doc_uuid").unwrap_or_default();
            let chapter_id: String = row.try_get("chapter_id").unwrap_or_default();
            let chunk_index: i32 = row.try_get("chunk_index").unwrap_or(0);
            let chunk_level: String = row
                .try_get("chunk_level")
                .unwrap_or_else(|_| "medium".to_string());
            let parent_chunk_index: Option<i32> = row.try_get("parent_chunk_index").unwrap_or(None);
            let macro_index: Option<i32> = row.try_get("macro_index").unwrap_or(None);
            let medium_index: Option<i32> = row.try_get("medium_index").unwrap_or(None);
            let header_chain =
                json_array_to_strings(row.try_get("header_chain").unwrap_or(serde_json::json!([])));
            let zone_keywords = json_array_to_strings(
                row.try_get("zone_keywords")
                    .unwrap_or(serde_json::json!([])),
            );
            let entities =
                json_array_to_strings(row.try_get("entities").unwrap_or(serde_json::json!([])));
            details.push(ChunkDetail {
                id: id as usize,
                text,
                filename,
                doc_uuid,
                chapter_id,
                chunk_index,
                chunk_level,
                parent_chunk_index,
                macro_index,
                medium_index,
                header_chain,
                zone_keywords,
                entities,
            });
        }
        Ok(details)
    }

    pub async fn fetch_neighbors(
        &self,
        keys: &[(String, i32)],
    ) -> Result<std::collections::HashMap<(String, i32), String>, String> {
        let pool = self
            .postgres
            .as_ref()
            .ok_or("PostgreSQL pool not initialized")?;
        if keys.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        let mut query_str =
            String::from("SELECT doc_uuid, chunk_index, text FROM documents WHERE ");
        for i in 0..keys.len() {
            if i > 0 {
                query_str.push_str(" OR ");
            }
            query_str.push_str(&format!(
                "(doc_uuid = ${} AND chunk_index = ${})",
                i * 2 + 1,
                i * 2 + 2
            ));
        }

        let mut query = sqlx::query(&query_str);
        for (uuid, idx) in keys {
            query = query.bind(uuid).bind(*idx);
        }

        let rows = query
            .fetch_all(pool)
            .await
            .map_err(|e| format!("Failed to fetch neighbors: {}", e))?;

        use sqlx::Row;
        let mut map = std::collections::HashMap::new();
        for r in rows {
            let doc_uuid: String = r.try_get("doc_uuid").unwrap_or_default();
            let chunk_index: i32 = r.try_get("chunk_index").unwrap_or(-1);
            let text: String = r.try_get("text").unwrap_or_default();
            map.insert((doc_uuid, chunk_index), text);
        }
        Ok(map)
    }

    pub async fn fetch_parent_chunks(
        &self,
        keys: &[(String, i32)],
    ) -> Result<std::collections::HashMap<(String, i32), ChunkDetail>, String> {
        let pool = self
            .postgres
            .as_ref()
            .ok_or("PostgreSQL pool not initialized")?;
        if keys.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        let mut query_str = String::from(
            "SELECT id, text, COALESCE(filename, '') as filename,
                    COALESCE(doc_uuid, '') as doc_uuid,
                    COALESCE(chapter_id, '') as chapter_id,
                    COALESCE(chunk_index, 0) as chunk_index,
                    COALESCE(chunk_level, 'medium') as chunk_level,
                    parent_chunk_index, macro_index, medium_index,
                    COALESCE(header_chain, '[]'::jsonb) as header_chain,
                    COALESCE(zone_keywords, '[]'::jsonb) as zone_keywords,
                    COALESCE(entities, '[]'::jsonb) as entities
             FROM documents WHERE ",
        );
        for i in 0..keys.len() {
            if i > 0 {
                query_str.push_str(" OR ");
            }
            query_str.push_str(&format!(
                "(doc_uuid = ${} AND chunk_index = ${})",
                i * 2 + 1,
                i * 2 + 2
            ));
        }

        let mut query = sqlx::query(&query_str);
        for (uuid, idx) in keys {
            query = query.bind(uuid).bind(*idx);
        }

        use sqlx::Row;
        let rows = query
            .fetch_all(pool)
            .await
            .map_err(|e| format!("Failed to fetch parents: {}", e))?;
        let mut map = std::collections::HashMap::new();
        for row in rows {
            let doc_uuid: String = row.try_get("doc_uuid").unwrap_or_default();
            let chunk_index: i32 = row.try_get("chunk_index").unwrap_or(0);
            map.insert(
                (doc_uuid.clone(), chunk_index),
                ChunkDetail {
                    id: row.try_get::<i64, _>("id").unwrap_or(-1) as usize,
                    text: row.try_get("text").unwrap_or_default(),
                    filename: row.try_get("filename").unwrap_or_default(),
                    doc_uuid,
                    chapter_id: row.try_get("chapter_id").unwrap_or_default(),
                    chunk_index,
                    chunk_level: row
                        .try_get("chunk_level")
                        .unwrap_or_else(|_| "medium".to_string()),
                    parent_chunk_index: row.try_get("parent_chunk_index").unwrap_or(None),
                    macro_index: row.try_get("macro_index").unwrap_or(None),
                    medium_index: row.try_get("medium_index").unwrap_or(None),
                    header_chain: json_array_to_strings(
                        row.try_get("header_chain").unwrap_or(serde_json::json!([])),
                    ),
                    zone_keywords: json_array_to_strings(
                        row.try_get("zone_keywords")
                            .unwrap_or(serde_json::json!([])),
                    ),
                    entities: json_array_to_strings(
                        row.try_get("entities").unwrap_or(serde_json::json!([])),
                    ),
                },
            );
        }
        Ok(map)
    }

    pub async fn fetch_entity_scores(&self, ids: &[usize], query_entities: &[String]) -> Vec<f32> {
        if ids.is_empty() || query_entities.len() < 2 {
            return vec![0.0; ids.len()];
        }
        let Some(pool) = &self.postgres else {
            return vec![0.0; ids.len()];
        };
        use sqlx::Row;
        let ids_i64: Vec<i64> = ids.iter().map(|&x| x as i64).collect();
        let rows = sqlx::query("SELECT id, COALESCE(entities, '[]'::jsonb) as entities FROM documents WHERE id = ANY($1)")
            .bind(&ids_i64[..])
            .fetch_all(pool)
            .await;
        let Ok(rows) = rows else {
            return vec![0.0; ids.len()];
        };
        let wanted: Vec<String> = query_entities.iter().map(|s| s.to_lowercase()).collect();
        let mut map = std::collections::HashMap::new();
        for row in rows {
            let id: i64 = row.try_get("id").unwrap_or(-1);
            let entities =
                json_array_to_strings(row.try_get("entities").unwrap_or(serde_json::json!([])));
            let hits = entities
                .iter()
                .filter(|entity| {
                    wanted.iter().any(|wanted| {
                        entity.to_lowercase().contains(wanted)
                            || wanted.contains(&entity.to_lowercase())
                    })
                })
                .count();
            map.insert(id, hits as f32 / wanted.len().max(1) as f32);
        }
        ids_i64
            .iter()
            .map(|id| *map.get(id).unwrap_or(&0.0))
            .collect()
    }

    pub async fn search_entity_candidate_ids(
        &self,
        query_entities: &[String],
        doc_uuid: Option<&str>,
        limit: usize,
    ) -> Vec<usize> {
        if query_entities.len() < 2 {
            return Vec::new();
        }
        let Some(pool) = &self.postgres else {
            return Vec::new();
        };
        let wanted: Vec<String> = query_entities.iter().map(|s| s.to_lowercase()).collect();
        let rows = if let Some(uuid) = doc_uuid {
            sqlx::query("SELECT id, COALESCE(entities, '[]'::jsonb) as entities FROM documents WHERE doc_uuid = $1 AND jsonb_array_length(COALESCE(entities, '[]'::jsonb)) > 0 LIMIT $2")
                .bind(uuid)
                .bind((limit * 5).max(50) as i64)
                .fetch_all(pool)
                .await
        } else {
            sqlx::query("SELECT id, COALESCE(entities, '[]'::jsonb) as entities FROM documents WHERE jsonb_array_length(COALESCE(entities, '[]'::jsonb)) > 0 LIMIT $1")
                .bind((limit * 5).max(50) as i64)
                .fetch_all(pool)
                .await
        };
        let Ok(rows) = rows else {
            return Vec::new();
        };
        use sqlx::Row;
        let mut scored = Vec::new();
        for row in rows {
            let id: i64 = row.try_get("id").unwrap_or(-1);
            let entities =
                json_array_to_strings(row.try_get("entities").unwrap_or(serde_json::json!([])));
            let hits = entities
                .iter()
                .filter(|entity| {
                    wanted.iter().any(|wanted| {
                        entity.to_lowercase().contains(wanted)
                            || wanted.contains(&entity.to_lowercase())
                    })
                })
                .count();
            if hits >= 2 || hits == wanted.len() {
                scored.push((id as usize, hits));
            }
        }
        scored.sort_by(|a, b| b.1.cmp(&a.1));
        scored.into_iter().take(limit).map(|(id, _)| id).collect()
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

    // ------------------------------------------------------------------------
    // Metadata operations
    // ------------------------------------------------------------------------

    /// Updates document metadata (merges with existing)
    pub async fn update_document_metadata(
        &self,
        doc_uuid: &str,
        key: &str,
        value: serde_json::Value,
    ) -> Result<(), String> {
        let pool = self
            .postgres
            .as_ref()
            .ok_or("PostgreSQL pool not initialized")?;
        sqlx::query(
            "UPDATE documents SET metadata = jsonb_set(COALESCE(metadata, '{}'::jsonb), $1, $2, true) WHERE doc_uuid = $3"
        )
        .bind(&[key])
        .bind(&value)
        .bind(doc_uuid)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to update metadata: {}", e))?;
        Ok(())
    }

    /// Fetches top-level metadata, anchors, and edges for a document
    pub async fn fetch_document_metadata_by_uuid(
        &self,
        doc_uuid: &str,
    ) -> Result<Option<serde_json::Value>, String> {
        let pool = self
            .postgres
            .as_ref()
            .ok_or("PostgreSQL pool not initialized")?;

        use sqlx::Row;

        // Get document metadata
        let row_opt = sqlx::query(
            "SELECT filename, doc_uuid, metadata FROM documents WHERE doc_uuid = $1 LIMIT 1",
        )
        .bind(doc_uuid)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("Failed to fetch document metadata: {}", e))?;

        let row = match row_opt {
            Some(r) => r,
            None => return Ok(None),
        };

        let filename: String = row
            .try_get("filename")
            .unwrap_or_else(|_| "Unknown".to_string());
        let metadata: serde_json::Value = row.try_get("metadata").unwrap_or(serde_json::json!({}));

        // Get anchors
        let anchor_rows =
            sqlx::query("SELECT name, text FROM anchors WHERE doc_uuid = $1 ORDER BY id ASC")
                .bind(doc_uuid)
                .fetch_all(pool)
                .await
                .map_err(|e| format!("Failed to fetch anchors: {}", e))?;

        let anchors: Vec<serde_json::Value> = anchor_rows.iter().map(|r| {
            serde_json::json!({
                "name": r.try_get::<String, _>("name").unwrap_or_default(),
                "text_preview": r.try_get::<String, _>("text").unwrap_or_default().chars().take(200).collect::<String>()
            })
        }).collect();

        // Get edges
        let edge_rows = sqlx::query(
            "SELECT source_doc_uuid, target_doc_uuid, edge_type, metadata FROM edges WHERE source_doc_uuid = $1 OR target_doc_uuid = $1 ORDER BY id ASC"
        )
        .bind(doc_uuid)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to fetch edges: {}", e))?;

        let edges: Vec<serde_json::Value> = edge_rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "source": r.try_get::<String, _>("source_doc_uuid").unwrap_or_default(),
                    "target": r.try_get::<String, _>("target_doc_uuid").unwrap_or_default(),
                    "type": r.try_get::<String, _>("edge_type").unwrap_or_default(),
                })
            })
            .collect();

        Ok(Some(serde_json::json!({
            "doc_uuid": doc_uuid,
            "filename": filename,
            "metadata": metadata,
            "anchors": anchors,
            "edges": edges
        })))
    }

    // ------------------------------------------------------------------------
    // Anchor operations
    // ------------------------------------------------------------------------

    /// Inserts an anchor into PostgreSQL
    pub async fn insert_anchor(
        &self,
        doc_uuid: &str,
        name: &str,
        text: &str,
    ) -> Result<i64, String> {
        let pool = self
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

        use sqlx::Row;
        row.try_get("id")
            .map_err(|e| format!("Failed to get anchor id: {}", e))
    }

    /// Inserts an anchor embedding into Qdrant anchors collection
    pub async fn insert_anchor_embedding(
        &self,
        anchor_id: i64,
        vector: ndarray::Array1<f32>,
        doc_uuid: &str,
        name: &str,
    ) -> Result<(), String> {
        self.insert_anchor_embeddings_batch(vec![(anchor_id, vector, doc_uuid, name)])
            .await
    }

    pub async fn insert_anchor_embeddings_batch(
        &self,
        anchors: Vec<(i64, ndarray::Array1<f32>, &str, &str)>,
    ) -> Result<(), String> {
        if anchors.is_empty() {
            return Ok(());
        }

        let client = self
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

    /// Search anchors by dense vector similarity
    pub async fn search_anchors_dense(
        &self,
        query_vector: &ndarray::Array1<f32>,
        top_k: usize,
    ) -> Result<(Vec<i64>, Vec<f32>, Vec<String>), String> {
        let client = self
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

    /// Search anchors by BM25 on their text (PostgreSQL full-text search)
    pub async fn search_anchors_bm25(
        &self,
        query: &str,
        top_k: usize,
    ) -> Result<Vec<(i64, String, String, f32)>, String> {
        let pool = self
            .postgres
            .as_ref()
            .ok_or("PostgreSQL pool not initialized")?;

        use sqlx::Row;
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

    /// Fetches all anchors for a document
    pub async fn fetch_anchors_by_doc_uuid(
        &self,
        doc_uuid: &str,
    ) -> Result<Vec<serde_json::Value>, String> {
        let pool = self
            .postgres
            .as_ref()
            .ok_or("PostgreSQL pool not initialized")?;

        use sqlx::Row;
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

    // ------------------------------------------------------------------------
    // Edge operations
    // ------------------------------------------------------------------------

    /// Inserts an edge between two documents
    pub async fn insert_edge(
        &self,
        source: &str,
        target: &str,
        edge_type: &str,
        metadata: Option<serde_json::Value>,
    ) -> Result<i64, String> {
        let pool = self
            .postgres
            .as_ref()
            .ok_or("PostgreSQL pool not initialized")?;

        let meta = metadata.unwrap_or(serde_json::json!({}));
        let row = sqlx::query(
            "INSERT INTO edges (source_doc_uuid, target_doc_uuid, edge_type, metadata) VALUES ($1, $2, $3, $4) RETURNING id"
        )
        .bind(source)
        .bind(target)
        .bind(edge_type)
        .bind(&meta)
        .fetch_one(pool)
        .await
        .map_err(|e| format!("Failed to insert edge: {}", e))?;

        use sqlx::Row;
        row.try_get("id")
            .map_err(|e| format!("Failed to get edge id: {}", e))
    }

    /// Fetches all edges for a document (both incoming and outgoing)
    pub async fn fetch_edges_by_doc_uuid(
        &self,
        doc_uuid: &str,
        edge_type: Option<&str>,
    ) -> Result<Vec<serde_json::Value>, String> {
        let pool = self
            .postgres
            .as_ref()
            .ok_or("PostgreSQL pool not initialized")?;

        use sqlx::Row;
        let rows = if let Some(etype) = edge_type {
            sqlx::query(
                "SELECT source_doc_uuid, target_doc_uuid, edge_type, metadata FROM edges WHERE (source_doc_uuid = $1 OR target_doc_uuid = $1) AND edge_type = $2 ORDER BY id ASC"
            )
            .bind(doc_uuid)
            .bind(etype)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("Failed to fetch edges: {}", e))?
        } else {
            sqlx::query(
                "SELECT source_doc_uuid, target_doc_uuid, edge_type, metadata FROM edges WHERE source_doc_uuid = $1 OR target_doc_uuid = $1 ORDER BY id ASC"
            )
            .bind(doc_uuid)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("Failed to fetch edges: {}", e))?
        };

        Ok(rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "source": r.try_get::<String, _>("source_doc_uuid").unwrap_or_default(),
                    "target": r.try_get::<String, _>("target_doc_uuid").unwrap_or_default(),
                    "type": r.try_get::<String, _>("edge_type").unwrap_or_default(),
                })
            })
            .collect())
    }

    /// Extracts markdown headings as anchors
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

    /// Automatically extract and store anchors from markdown text for a document
    pub async fn auto_extract_anchors(
        &self,
        doc_uuid: &str,
        markdown_text: &str,
    ) -> Result<Vec<i64>, String> {
        let anchors = Self::extract_anchors_from_markdown(markdown_text);
        let mut inserted_ids = Vec::new();
        let mut embedding_jobs = Vec::new();

        for (name, text) in anchors {
            let anchor_id = self.insert_anchor(doc_uuid, &name, &text).await?;
            inserted_ids.push(anchor_id);
            embedding_jobs.push((anchor_id, name, text));
        }

        let batch_size = crate::config::get_embedding_batch_size().max(1);
        for batch in embedding_jobs.chunks(batch_size) {
            let texts: Vec<String> = batch.iter().map(|(_, _, text)| text.clone()).collect();
            match self.get_embeddings(&texts, false).await {
                Ok(vectors) => {
                    let anchor_points = batch
                        .iter()
                        .zip(vectors.into_iter())
                        .map(|((anchor_id, name, _), vector)| {
                            (*anchor_id, vector, doc_uuid, name.as_str())
                        })
                        .collect();
                    if let Err(error) = self.insert_anchor_embeddings_batch(anchor_points).await {
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

    /// Dynamically resolves a search string (which could be a doc_uuid, filename, or partial filename)
    /// to the corresponding doc_uuid from PostgreSQL.
    pub async fn resolve_doc_uuid(&self, input: &str) -> Option<String> {
        let pool = self.postgres.as_ref()?;
        use sqlx::Row;

        // 1. Try exact match on doc_uuid
        let row_opt =
            sqlx::query("SELECT DISTINCT doc_uuid FROM documents WHERE doc_uuid = $1 LIMIT 1")
                .bind(input)
                .fetch_optional(pool)
                .await
                .ok()
                .flatten();

        if let Some(row) = row_opt {
            let uuid: String = row.try_get("doc_uuid").unwrap_or_default();
            if !uuid.is_empty() {
                return Some(uuid);
            }
        }

        // 2. Try exact match on filename
        let row_opt =
            sqlx::query("SELECT DISTINCT doc_uuid FROM documents WHERE filename = $1 LIMIT 1")
                .bind(input)
                .fetch_optional(pool)
                .await
                .ok()
                .flatten();

        if let Some(row) = row_opt {
            let uuid: String = row.try_get("doc_uuid").unwrap_or_default();
            if !uuid.is_empty() {
                return Some(uuid);
            }
        }

        // 3. Try partial case-insensitive match on filename
        let pattern = format!("%{}%", input);
        let row_opt =
            sqlx::query("SELECT DISTINCT doc_uuid FROM documents WHERE filename ILIKE $1 LIMIT 1")
                .bind(&pattern)
                .fetch_optional(pool)
                .await
                .ok()
                .flatten();

        if let Some(row) = row_opt {
            let uuid: String = row.try_get("doc_uuid").unwrap_or_default();
            if !uuid.is_empty() {
                return Some(uuid);
            }
        }

        None
    }
}

impl Default for DbPools {
    fn default() -> Self {
        Self::new()
    }
}

fn json_array_to_strings(value: serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToString::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn tokenize_lexical_query(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .filter(|word| !word.is_empty())
        .map(|word| word.replace(|c: char| !c.is_alphanumeric(), ""))
        .filter(|word| !word.is_empty())
        .collect()
}

fn build_tsquery_or(terms: &[String]) -> String {
    terms.join(" | ")
}

fn escape_like_pattern(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '%' | '_' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}
