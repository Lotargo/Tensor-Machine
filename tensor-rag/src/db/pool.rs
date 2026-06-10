use qdrant_client::Qdrant;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tonic::transport::{Channel, Endpoint};

use super::circuit_breaker::CircuitBreaker;
use super::types::{ChunkDetail, CollectionHealth, EnrichedDocumentInsert, StoredDocument};

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
            let template = std::env::var("EMBEDDING_QUERY_PREFIX").unwrap_or_else(|_| {
                "Instruct: Given a question about a local document library, retrieve passages that contain the answer, including paraphrases and nearby narrative context.\nQuery: {}".to_string()
            });
            if template.contains("{}") {
                template.replace("{}", text)
            } else {
                format!("{}{}", template, text)
            }
        } else {
            let template = std::env::var("EMBEDDING_PASSAGE_PREFIX").unwrap_or_default();
            if template.contains("{}") {
                template.replace("{}", text)
            } else {
                format!("{}{}", template, text)
            }
        }
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
        let _ = sqlx::query("ALTER TABLE documents ADD COLUMN IF NOT EXISTS cluster_id INTEGER;")
            .execute(&pool)
            .await;
        let _ =
            sqlx::query("ALTER TABLE documents ADD COLUMN IF NOT EXISTS cluster_distance REAL;")
                .execute(&pool)
                .await;
        let _ = sqlx::query("ALTER TABLE documents ADD COLUMN IF NOT EXISTS layout_hint JSONB;")
            .execute(&pool)
            .await;
        let _ = sqlx::query(
            "ALTER TABLE documents ADD COLUMN IF NOT EXISTS clustering_algorithm TEXT;",
        )
        .execute(&pool)
        .await;
        let _ = sqlx::query(
            "ALTER TABLE documents ADD COLUMN IF NOT EXISTS clustering_projection TEXT;",
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
        let _ = sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_documents_cluster ON documents(doc_uuid, cluster_id);",
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

        // Create entities table for spatial representation of named entities
        let _ = sqlx::query(
            "CREATE TABLE IF NOT EXISTS entities (
                id BIGSERIAL PRIMARY KEY,
                doc_uuid TEXT NOT NULL,
                text TEXT NOT NULL,
                cluster_id INTEGER,
                cluster_distance REAL,
                layout_hint JSONB,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                UNIQUE(doc_uuid, text)
            );",
        )
        .execute(&pool)
        .await;
        let _ = sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_entities_doc_uuid ON entities(doc_uuid);",
        )
        .execute(&pool)
        .await;
        let _ = sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_entities_text ON entities(doc_uuid, text);",
        )
        .execute(&pool)
        .await;
        let _ = sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_entities_cluster ON entities(doc_uuid, cluster_id);",
        )
        .execute(&pool)
        .await;

        // Create chunk_entities junction table
        let _ = sqlx::query(
            "CREATE TABLE IF NOT EXISTS chunk_entities (
                chunk_id BIGINT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                entity_id BIGINT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
                PRIMARY KEY (chunk_id, entity_id)
            );",
        )
        .execute(&pool)
        .await;
        let _ = sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_chunk_entities_entity ON chunk_entities(entity_id);",
        )
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
        let entities_collection = crate::config::get_qdrant_entities_collection();

        let qdrant_timeout = std::time::Duration::from_secs(10);

        if node_id == "default" {
            // Create main collection
            let check = tokio::time::timeout(
                qdrant_timeout,
                client.collection_info(&collection),
            )
            .await;
            if check.is_err() || check.unwrap().is_err() {
                tracing::info!("Creating Qdrant collection '{}'...", collection);
                let _ = tokio::time::timeout(
                    qdrant_timeout,
                    client.create_collection(qdrant_client::qdrant::CreateCollection {
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
                    }),
                )
                .await;
            }

            // Create anchors collection
            let check = tokio::time::timeout(
                qdrant_timeout,
                client.collection_info(&anchors_collection),
            )
            .await;
            if check.is_err() || check.unwrap().is_err() {
                tracing::info!(
                    "Creating Qdrant anchors collection '{}'...",
                    anchors_collection
                );
                let _ = tokio::time::timeout(
                    qdrant_timeout,
                    client.create_collection(qdrant_client::qdrant::CreateCollection {
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
                    }),
                )
                .await;
            }

            // Create entities collection
            let check = tokio::time::timeout(
                qdrant_timeout,
                client.collection_info(&entities_collection),
            )
            .await;
            if check.is_err() || check.unwrap().is_err() {
                tracing::info!(
                    "Creating Qdrant entities collection '{}'...",
                    entities_collection
                );
                let _ = tokio::time::timeout(
                    qdrant_timeout,
                    client.create_collection(qdrant_client::qdrant::CreateCollection {
                        collection_name: entities_collection.clone(),
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
                    }),
                )
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
}

// ----------------------------------------------------------------------------
// Delegation methods to keep public API of DbPools unchanged
// ----------------------------------------------------------------------------
impl DbPools {
    // Document operations
    pub async fn insert_document(
        &self,
        text: &str,
        vector: ndarray::Array1<f32>,
        filename: &str,
        doc_uuid: &str,
        chapter_id: &str,
        chunk_index: i32,
    ) -> Result<usize, String> {
        super::document::insert_document(
            self,
            text,
            vector,
            filename,
            doc_uuid,
            chapter_id,
            chunk_index,
        )
        .await
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
        super::document::insert_document_enriched(
            self,
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
        )
        .await
    }

    pub async fn insert_documents_enriched_batch(
        &self,
        documents: Vec<EnrichedDocumentInsert<'_>>,
    ) -> Result<Vec<usize>, String> {
        super::document::insert_documents_enriched_batch(self, documents).await
    }

    pub async fn update_document_semantic_layout(
        &self,
        doc_uuid: &str,
        layout: &crate::clustering::SemanticClusterLayout,
    ) -> Result<(), String> {
        super::document::update_document_semantic_layout(self, doc_uuid, layout).await
    }

    pub async fn delete_document(&self, doc_id: usize) -> Result<(), String> {
        super::document::delete_document(self, doc_id).await
    }

    pub async fn clear_documents(&self) -> Result<(), String> {
        super::document::clear_documents(self).await
    }

    pub async fn hard_reset_database(&self) -> Result<(), String> {
        super::document::hard_reset_database(self).await
    }

    pub async fn reindex_database(&self) -> Result<(), String> {
        super::document::reindex_database(self).await
    }

    pub async fn get_collection_health(&self) -> CollectionHealth {
        super::document::get_collection_health(self).await
    }

    pub async fn fetch_document_by_uuid(
        &self,
        doc_uuid: &str,
    ) -> Result<Option<StoredDocument>, String> {
        super::document::fetch_document_by_uuid(self, doc_uuid).await
    }

    pub async fn resolve_doc_uuid(&self, input: &str) -> Option<String> {
        super::document::resolve_doc_uuid(self, input).await
    }

    // Search operations
    pub async fn search_qdrant(
        &self,
        query: &ndarray::Array1<f32>,
        filters: &Option<Vec<crate::ir::Filter>>,
        top_k: usize,
    ) -> Result<(Vec<usize>, Vec<ndarray::Array1<f32>>), String> {
        super::search::search_qdrant(self, query, filters, top_k).await
    }

    pub async fn fetch_bm25_scores(&self, ids: &[usize], query: &str) -> Vec<f32> {
        super::search::fetch_bm25_scores(self, ids, query).await
    }

    pub async fn fetch_document_chunk_vectors(
        &self,
        doc_uuid: &str,
        limit: usize,
    ) -> Result<Vec<(usize, Vec<f32>)>, String> {
        super::search::fetch_document_chunk_vectors(self, doc_uuid, limit).await
    }

    pub async fn search_keyword_candidate_ids(
        &self,
        query: &str,
        doc_uuid: Option<&str>,
        chapter_id: Option<&str>,
        limit: usize,
    ) -> Vec<usize> {
        super::search::search_keyword_candidate_ids(self, query, doc_uuid, chapter_id, limit).await
    }

    pub async fn search_structured_candidate_ids(
        &self,
        query: &str,
        doc_uuid: Option<&str>,
        chapter_id: Option<&str>,
        limit: usize,
    ) -> Vec<usize> {
        super::search::search_structured_candidate_ids(self, query, doc_uuid, chapter_id, limit)
            .await
    }

    pub async fn search_opening_candidate_ids(
        &self,
        query: &str,
        doc_uuid: Option<&str>,
        limit: usize,
    ) -> Vec<usize> {
        super::search::search_opening_candidate_ids(self, query, doc_uuid, limit).await
    }

    pub async fn fetch_metadata(&self, ids: &[usize]) -> Vec<String> {
        super::search::fetch_metadata(self, ids).await
    }

    pub async fn fetch_chunk_details(&self, ids: &[usize]) -> Result<Vec<ChunkDetail>, String> {
        super::search::fetch_chunk_details(self, ids).await
    }

    pub async fn fetch_neighbor_chunks(
        &self,
        seed_details: &[ChunkDetail],
        max_neighbors: usize,
    ) -> Result<Vec<ChunkDetail>, String> {
        super::search::fetch_neighbor_chunks(self, seed_details, max_neighbors).await
    }

    pub async fn fetch_structured_span_details(
        &self,
        query: &str,
        seed_details: &[ChunkDetail],
        max_spans: usize,
    ) -> Result<Vec<ChunkDetail>, String> {
        super::search::fetch_structured_span_details(self, query, seed_details, max_spans).await
    }

    pub async fn fetch_neighbors(
        &self,
        keys: &[(String, i32)],
    ) -> Result<std::collections::HashMap<(String, i32), String>, String> {
        super::search::fetch_neighbors(self, keys).await
    }

    pub async fn fetch_parent_chunks(
        &self,
        keys: &[(String, i32)],
    ) -> Result<std::collections::HashMap<(String, i32), ChunkDetail>, String> {
        super::search::fetch_parent_chunks(self, keys).await
    }

    pub async fn fetch_entity_scores(&self, ids: &[usize], query_entities: &[String]) -> Vec<f32> {
        super::search::fetch_entity_scores(self, ids, query_entities).await
    }

    pub async fn search_entity_candidate_ids(
        &self,
        query_entities: &[String],
        doc_uuid: Option<&str>,
        limit: usize,
    ) -> Vec<usize> {
        super::search::search_entity_candidate_ids(self, query_entities, doc_uuid, limit).await
    }

    pub async fn search_entity_vector_candidates(
        &self,
        query_vector: &ndarray::Array1<f32>,
        doc_uuid: Option<&str>,
        limit: usize,
    ) -> Vec<usize> {
        super::search::search_entity_vector_candidates(self, query_vector, doc_uuid, limit).await
    }

    // Metadata operations
    pub async fn update_document_metadata(
        &self,
        doc_uuid: &str,
        key: &str,
        value: serde_json::Value,
    ) -> Result<(), String> {
        super::metadata::update_document_metadata(self, doc_uuid, key, value).await
    }

    pub async fn fetch_document_metadata_by_uuid(
        &self,
        doc_uuid: &str,
    ) -> Result<Option<serde_json::Value>, String> {
        super::metadata::fetch_document_metadata_by_uuid(self, doc_uuid).await
    }

    // Anchor operations
    pub async fn insert_anchor(
        &self,
        doc_uuid: &str,
        name: &str,
        text: &str,
    ) -> Result<i64, String> {
        super::anchors::insert_anchor(self, doc_uuid, name, text).await
    }

    pub async fn insert_anchor_embedding(
        &self,
        anchor_id: i64,
        vector: ndarray::Array1<f32>,
        doc_uuid: &str,
        name: &str,
    ) -> Result<(), String> {
        super::anchors::insert_anchor_embedding(self, anchor_id, vector, doc_uuid, name).await
    }

    pub async fn insert_anchor_embeddings_batch(
        &self,
        anchors: Vec<(i64, ndarray::Array1<f32>, &str, &str)>,
    ) -> Result<(), String> {
        super::anchors::insert_anchor_embeddings_batch(self, anchors).await
    }

    pub async fn search_anchors_dense(
        &self,
        query_vector: &ndarray::Array1<f32>,
        top_k: usize,
    ) -> Result<(Vec<i64>, Vec<f32>, Vec<String>), String> {
        super::anchors::search_anchors_dense(self, query_vector, top_k).await
    }

    pub async fn search_anchors_bm25(
        &self,
        query: &str,
        top_k: usize,
    ) -> Result<Vec<(i64, String, String, f32)>, String> {
        super::anchors::search_anchors_bm25(self, query, top_k).await
    }

    pub async fn fetch_anchors_by_doc_uuid(
        &self,
        doc_uuid: &str,
    ) -> Result<Vec<serde_json::Value>, String> {
        super::anchors::fetch_anchors_by_doc_uuid(self, doc_uuid).await
    }

    pub fn extract_anchors_from_markdown(markdown: &str) -> Vec<(String, String)> {
        super::anchors::extract_anchors_from_markdown(markdown)
    }

    pub async fn auto_extract_anchors(
        &self,
        doc_uuid: &str,
        markdown_text: &str,
    ) -> Result<Vec<i64>, String> {
        super::anchors::auto_extract_anchors(self, doc_uuid, markdown_text).await
    }

    // Edge operations
    pub async fn insert_edge(
        &self,
        source: &str,
        target: &str,
        edge_type: &str,
        metadata: Option<serde_json::Value>,
    ) -> Result<i64, String> {
        super::edges::insert_edge(self, source, target, edge_type, metadata).await
    }

    pub async fn fetch_edges_by_doc_uuid(
        &self,
        doc_uuid: &str,
        edge_type: Option<&str>,
    ) -> Result<Vec<serde_json::Value>, String> {
        super::edges::fetch_edges_by_doc_uuid(self, doc_uuid, edge_type).await
    }

    // Entity operations
    pub async fn upsert_entities(
        &self,
        doc_uuid: &str,
        entity_texts: &[String],
    ) -> Result<std::collections::HashMap<String, i64>, String> {
        super::entities::upsert_entities(self, doc_uuid, entity_texts).await
    }

    pub async fn link_chunk_entities(
        &self,
        chunk_id: i64,
        entity_ids: &[i64],
    ) -> Result<(), String> {
        super::entities::link_chunk_entities(self, chunk_id, entity_ids).await
    }

    pub async fn embed_and_store_entities(
        &self,
        doc_uuid: &str,
        entities: &[(i64, String)],
    ) -> Result<(), String> {
        super::entities::embed_and_store_entities(self, doc_uuid, entities).await
    }

    pub async fn update_entity_layout(
        &self,
        doc_uuid: &str,
        layout_points: &[(i64, i32, f32, [f32; 3])],
    ) -> Result<(), String> {
        super::entities::update_entity_layout(self, doc_uuid, layout_points).await
    }

    pub async fn delete_document_entities(&self, doc_uuid: &str) -> Result<(), String> {
        super::entities::delete_document_entities(self, doc_uuid).await
    }

    pub async fn fetch_document_entities(
        &self,
        doc_uuid: &str,
    ) -> Result<Vec<super::entities::EntityRecord>, String> {
        super::entities::fetch_document_entities(self, doc_uuid).await
    }
}

impl Default for DbPools {
    fn default() -> Self {
        Self::new()
    }
}
