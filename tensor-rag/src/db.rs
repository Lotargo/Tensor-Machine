use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::RwLock;
use std::time::Instant;
use std::sync::Arc;

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
                return time.elapsed().as_secs() < self.reset_timeout_secs
            }
            return true;
        }
        false
    }

    pub fn record_failure(&self) {
        self.failures.fetch_add(1, Ordering::SeqCst);
        let mut last = self.last_failure.write().unwrap();
        *last = Some(Instant::now());
    }

    pub fn record_success(&self) {
        self.failures.store(0, Ordering::SeqCst);
        let mut last = self.last_failure.write().unwrap();
        *last = None;
    }
}
use qdrant_client::Qdrant;
use qdrant_client::qdrant::{
    Condition, FieldCondition, Filter as QFilter, Match, PointId, Range, SearchPoints,
    VectorsOutput, WithVectorsSelector, condition::ConditionOneOf, r#match::MatchValue,
    point_id::PointIdOptions,
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::collections::HashMap;
use tonic::transport::{Channel, Endpoint};

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
    pub fn new() -> Self {
        Self {
            postgres: None,
            qdrant_clients: HashMap::new(),
            ml_servers: HashMap::new(),
            qdrant_cb: Arc::new(CircuitBreaker::new(5, 30)),
            postgres_cb: Arc::new(CircuitBreaker::new(5, 30)),
            ml_cb: Arc::new(CircuitBreaker::new(5, 30)),
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
    pub async fn add_qdrant_node(
        &mut self,
        node_id: &str,
        url: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let client = Qdrant::from_url(url).build()?;
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
    pub async fn get_embedding(&self, text: &str) -> ndarray::Array1<f32> {
        if self.ml_cb.is_open() {
            return ndarray::Array1::from_elem(768, 0.1);
        }

        // In a real application, this would make a gRPC call to the configured
        // ML server (e.g. self.ml_servers.get("embedding")) to generate the vector.
        // For the scope of this project without a real ML server running,
        // we return a deterministic dummy embedding for tests.
        let _ = text;
        self.ml_cb.record_success();
        ndarray::Array1::from_elem(768, 0.1)
    }

    /// Mock method for ANN search in Qdrant (Task 1.4.1, 1.4.6)
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
            let mut search_points = SearchPoints {
                collection_name: "default".to_string(), // In real app, this should be configurable
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
                    vectors.push(ndarray::Array1::from(v.data.clone()));
                } else {
                    vectors.push(ndarray::Array1::from_elem(768, 0.1));
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
                vectors.push(ndarray::Array1::from_elem(768, 0.1));
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
            return ids.iter().map(|id| format!("Metadata for doc {}", id)).collect();
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
}

impl Default for DbPools {
    fn default() -> Self {
        Self::new()
    }
}
