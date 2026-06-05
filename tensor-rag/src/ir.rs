use serde::{Deserialize, Serialize};

fn default_top_k() -> usize {
    crate::config::get_search_default_top_k()
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct QueryRequest {
    pub query: String,
    #[serde(default)]
    pub filters: Option<Vec<Filter>>,
    #[serde(default)]
    pub rerank_strategy: RerankStrategy,
    #[serde(default = "default_top_k")]
    pub final_top_k: usize,
    #[serde(default)]
    pub doc_uuid: Option<String>,
    #[serde(default)]
    pub alpha: Option<f32>,
    #[serde(default)]
    pub anchor_name: Option<String>,
    #[serde(default)]
    pub anchor_mode: Option<AnchorSearchMode>,
    #[serde(default)]
    pub metadata_filters: Option<Vec<Filter>>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub enum AnchorSearchMode {
    #[serde(rename = "exact")]
    Exact,
    #[serde(rename = "semantic")]
    Semantic,
    #[serde(rename = "both")]
    Both,
}

impl Default for AnchorSearchMode {
    fn default() -> Self {
        AnchorSearchMode::Semantic
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Filter {
    pub field: String,
    pub operator: String, // e.g., ">=", "==", "in"
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(tag = "type")]
pub enum RerankStrategy {
    #[serde(rename = "semantic")]
    SemanticOnly,
    #[serde(rename = "bm25")]
    Bm25Only,
    #[serde(rename = "hybrid")]
    Hybrid {
        semantic_weight: f32,
        bm25_weight: f32,
    },
    #[serde(rename = "mmr")]
    Mmr { diversity: f32 },
    #[serde(rename = "rrf")]
    Rrf { k: usize },
    #[serde(rename = "rsf")]
    Rsf {
        #[serde(default)]
        dense_weight: Option<f32>,
        #[serde(default)]
        bm25_weight: Option<f32>,
        #[serde(default)]
        entity_weight: Option<f32>,
    },
    #[serde(rename = "dynamic")]
    Dynamic {
        name: String,
        args: serde_json::Value,
    },
}

impl Default for RerankStrategy {
    fn default() -> Self {
        RerankStrategy::Hybrid {
            semantic_weight: 0.7,
            bm25_weight: 0.3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AstNode {
    Search(QueryRequest),
    MultiHop {
        query: String,
        hops: usize,
    },
    SelfRag {
        query: String,
        threshold: f32,
        low_plan: Box<AstNode>,
        high_plan: Box<AstNode>,
    },
    CorrectiveRag {
        query: String,
        fallback_query: String,
    },
    ColbertRetrieval {
        query: String,
        docs: Box<AstNode>,
    },
    AddMetadata {
        doc_uuid: String,
        key: String,
        value: serde_json::Value,
    },
    AddAnchor {
        doc_uuid: String,
        name: String,
        text: String,
    },
    AddEdge {
        source: String,
        target: String,
        edge_type: String,
        metadata: Option<serde_json::Value>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OpCode {
    /// Retrieve candidate IDs and vectors from Qdrant
    RetrieveQdrant {
        query: String,
        filters: Option<Vec<Filter>>,
        top_k: usize,
    },
    /// Fetch BM25 scores and metadata from Postgres for given IDs
    RetrievePostgres {
        query: String,
    },
    /// Compute semantic scores (dot product of query vector with retrieved vectors)
    ComputeSemantic,
    /// Load a scalar constant into memory
    LoadConst(f32),
    /// Element-wise addition of two tensors
    Add,
    /// Element-wise multiplication of a tensor by a scalar
    Mul,
    /// Weighted blending of two tensors (fusion)
    Fusion,
    /// Select the top K elements with their indices
    TopK(usize),
    /// Maximum Marginal Relevance for diversity
    Mmr(f32),
    /// Reciprocal Rank Fusion
    Rrf(usize),
    /// Reciprocal Score Fusion over dense/BM25/entity channels
    Rsf(crate::rsf::RsfConfig),
    /// Fetch entity co-occurrence scores for current candidates
    RetrieveEntityScores {
        query: String,
    },
    /// Branch execution based on max score threshold
    IfConfidence(f32, Program, Program),
    /// Return the result
    Emit,
    /// Element-wise division of two tensors
    Div,
    /// Thresholding: zero out values below threshold
    Threshold(f32),
    /// Sort tensor values descending
    Sort,
    /// Slice tensor values
    Slice(usize, usize),
    /// Matrix multiplication (dot product for 1D)
    Matmul,
    /// Call a dynamic strategy registered at runtime
    CallDynamic(String, serde_json::Value),
    /// Compute ColBERT MaxSim scores
    ColbertMaxSim,
    /// Mock load query tokens (M x 128)
    LoadColbertQuery(String),
    RetrieveAnchors {
        query: String,
        mode: AnchorSearchMode,
        top_k: usize,
    },
    RetrieveEdges {
        doc_uuid: String,
    },
    FilterByMetadata {
        filters: Vec<Filter>,
    },
    AnchorFusion {
        main_weight: f32,
        anchor_weight: f32,
    },
    AddMetadata {
        doc_uuid: String,
        key: String,
        value: serde_json::Value,
    },
    AddAnchor {
        doc_uuid: String,
        name: String,
        text: String,
    },
    AddEdge {
        source: String,
        target: String,
        edge_type: String,
        metadata: Option<serde_json::Value>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Instruction {
    pub op: OpCode,
    // Add operands or register indices here in the future if needed for a register VM.
    // For now, assuming a stack-based VM for simplicity of representation.
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Program {
    pub instructions: Vec<Instruction>,
}

impl Default for Program {
    fn default() -> Self {
        Self::new()
    }
}

impl Program {
    pub fn new() -> Self {
        Self {
            instructions: Vec::new(),
        }
    }

    pub fn push(&mut self, op: OpCode) {
        self.instructions.push(Instruction { op });
    }
}

pub struct Planner;

fn make_rsf_config(
    dense_weight: Option<f32>,
    bm25_weight: Option<f32>,
    entity_weight: Option<f32>,
) -> crate::rsf::RsfConfig {
    let mut config = crate::rsf::RsfConfig::default();
    if let Some(weight) = dense_weight {
        config.dense_weight = weight;
    }
    if let Some(weight) = bm25_weight {
        config.bm25_weight = weight;
    }
    if let Some(weight) = entity_weight {
        config.entity_weight = weight;
    }
    config
}

impl Planner {
    pub fn compile(ast: AstNode) -> Program {
        let mut program = Program::new();

        match ast {
            AstNode::Search(request) => {
                let final_top_k = request
                    .final_top_k
                    .clamp(1, crate::config::get_search_max_top_k());
                let initial_k = (final_top_k.saturating_mul(10)).max(100);

                // Override strategy if alpha is provided
                let strategy = if let Some(alpha) = request.alpha {
                    RerankStrategy::Hybrid {
                        semantic_weight: alpha,
                        bm25_weight: 1.0 - alpha,
                    }
                } else {
                    request.rerank_strategy.clone()
                };

                // Dynamically inject doc_uuid filter if provided
                let mut filters = request.filters.clone().unwrap_or_default();
                if let Some(ref uuid) = request.doc_uuid {
                    if !uuid.is_empty() {
                        filters.push(Filter {
                            field: "doc_uuid".to_string(),
                            operator: "==".to_string(),
                            value: serde_json::Value::String(uuid.clone()),
                        });
                    }
                }
                let filters_opt = if filters.is_empty() {
                    None
                } else {
                    Some(filters)
                };

                if let Some(ref anchor) = request.anchor_name {
                    let mode = request
                        .anchor_mode
                        .clone()
                        .unwrap_or(AnchorSearchMode::Semantic);

                    program.push(OpCode::RetrieveQdrant {
                        query: request.query.clone(),
                        filters: filters_opt,
                        top_k: initial_k,
                    });
                    program.push(OpCode::ComputeSemantic);

                    match strategy {
                        RerankStrategy::SemanticOnly => {
                            program.push(OpCode::TopK(final_top_k));
                        }
                        RerankStrategy::Bm25Only => {
                            program.push(OpCode::RetrievePostgres {
                                query: request.query.clone(),
                            });
                            program.push(OpCode::TopK(final_top_k));
                        }
                        RerankStrategy::Hybrid {
                            semantic_weight,
                            bm25_weight,
                        } => {
                            program.push(OpCode::RetrievePostgres {
                                query: request.query.clone(),
                            });
                            program.push(OpCode::LoadConst(semantic_weight));
                            program.push(OpCode::LoadConst(bm25_weight));
                            program.push(OpCode::Fusion);
                            program.push(OpCode::TopK(final_top_k));
                        }
                        RerankStrategy::Mmr { diversity } => {
                            program.push(OpCode::Mmr(diversity));
                            program.push(OpCode::TopK(final_top_k));
                        }
                        RerankStrategy::Rrf { k } => {
                            program.push(OpCode::RetrievePostgres {
                                query: request.query.clone(),
                            });
                            program.push(OpCode::Rrf(k));
                            program.push(OpCode::TopK(final_top_k));
                        }
                        RerankStrategy::Rsf {
                            dense_weight,
                            bm25_weight,
                            entity_weight,
                        } => {
                            program.push(OpCode::RetrievePostgres {
                                query: request.query.clone(),
                            });
                            program.push(OpCode::RetrieveEntityScores {
                                query: request.query.clone(),
                            });
                            program.push(OpCode::Rsf(make_rsf_config(
                                dense_weight,
                                bm25_weight,
                                entity_weight,
                            )));
                            program.push(OpCode::TopK(final_top_k));
                        }
                        RerankStrategy::Dynamic { name, args } => {
                            program.push(OpCode::CallDynamic(name, args));
                            program.push(OpCode::TopK(final_top_k));
                        }
                    }

                    if let Some(ref meta_filters) = request.metadata_filters {
                        if !meta_filters.is_empty() {
                            program.push(OpCode::FilterByMetadata {
                                filters: meta_filters.clone(),
                            });
                        }
                    }

                    program.push(OpCode::RetrieveAnchors {
                        query: anchor.clone(),
                        mode,
                        top_k: final_top_k,
                    });

                    program.push(OpCode::AnchorFusion {
                        main_weight: 0.7,
                        anchor_weight: 0.3,
                    });

                    program.push(OpCode::TopK(final_top_k));
                    program.push(OpCode::Emit);
                } else {
                    program.push(OpCode::RetrieveQdrant {
                        query: request.query.clone(),
                        filters: filters_opt,
                        top_k: initial_k,
                    });
                    program.push(OpCode::ComputeSemantic);

                    match strategy {
                        RerankStrategy::SemanticOnly => {
                            if let Some(ref meta_filters) = request.metadata_filters {
                                if !meta_filters.is_empty() {
                                    program.push(OpCode::FilterByMetadata {
                                        filters: meta_filters.clone(),
                                    });
                                }
                            }
                            program.push(OpCode::TopK(final_top_k));
                            program.push(OpCode::Emit);
                        }
                        RerankStrategy::Bm25Only => {
                            program.push(OpCode::RetrievePostgres {
                                query: request.query.clone(),
                            });
                            if let Some(ref meta_filters) = request.metadata_filters {
                                if !meta_filters.is_empty() {
                                    program.push(OpCode::FilterByMetadata {
                                        filters: meta_filters.clone(),
                                    });
                                }
                            }
                            program.push(OpCode::TopK(final_top_k));
                            program.push(OpCode::Emit);
                        }
                        RerankStrategy::Hybrid {
                            semantic_weight,
                            bm25_weight,
                        } => {
                            program.push(OpCode::RetrievePostgres {
                                query: request.query.clone(),
                            });
                            program.push(OpCode::LoadConst(semantic_weight));
                            program.push(OpCode::LoadConst(bm25_weight));
                            program.push(OpCode::Fusion);
                            if let Some(ref meta_filters) = request.metadata_filters {
                                if !meta_filters.is_empty() {
                                    program.push(OpCode::FilterByMetadata {
                                        filters: meta_filters.clone(),
                                    });
                                }
                            }
                            program.push(OpCode::TopK(final_top_k));
                            program.push(OpCode::Emit);
                        }
                        RerankStrategy::Mmr { diversity } => {
                            program.push(OpCode::Mmr(diversity));
                            if let Some(ref meta_filters) = request.metadata_filters {
                                if !meta_filters.is_empty() {
                                    program.push(OpCode::FilterByMetadata {
                                        filters: meta_filters.clone(),
                                    });
                                }
                            }
                            program.push(OpCode::TopK(final_top_k));
                            program.push(OpCode::Emit);
                        }
                        RerankStrategy::Rrf { k } => {
                            program.push(OpCode::RetrievePostgres {
                                query: request.query.clone(),
                            });
                            program.push(OpCode::Rrf(k));
                            if let Some(ref meta_filters) = request.metadata_filters {
                                if !meta_filters.is_empty() {
                                    program.push(OpCode::FilterByMetadata {
                                        filters: meta_filters.clone(),
                                    });
                                }
                            }
                            program.push(OpCode::TopK(final_top_k));
                            program.push(OpCode::Emit);
                        }
                        RerankStrategy::Rsf {
                            dense_weight,
                            bm25_weight,
                            entity_weight,
                        } => {
                            program.push(OpCode::RetrievePostgres {
                                query: request.query.clone(),
                            });
                            program.push(OpCode::RetrieveEntityScores {
                                query: request.query.clone(),
                            });
                            program.push(OpCode::Rsf(make_rsf_config(
                                dense_weight,
                                bm25_weight,
                                entity_weight,
                            )));
                            if let Some(ref meta_filters) = request.metadata_filters {
                                if !meta_filters.is_empty() {
                                    program.push(OpCode::FilterByMetadata {
                                        filters: meta_filters.clone(),
                                    });
                                }
                            }
                            program.push(OpCode::TopK(final_top_k));
                            program.push(OpCode::Emit);
                        }
                        RerankStrategy::Dynamic { name, args } => {
                            program.push(OpCode::CallDynamic(name, args));
                            if let Some(ref meta_filters) = request.metadata_filters {
                                if !meta_filters.is_empty() {
                                    program.push(OpCode::FilterByMetadata {
                                        filters: meta_filters.clone(),
                                    });
                                }
                            }
                            program.push(OpCode::TopK(final_top_k));
                            program.push(OpCode::Emit);
                        }
                    }
                }
            }
            AstNode::MultiHop { query, hops } => {
                for _ in 0..hops {
                    program.push(OpCode::RetrieveQdrant {
                        query: query.clone(),
                        filters: None,
                        top_k: 10,
                    });
                }
                program.push(OpCode::ComputeSemantic);
                program.push(OpCode::TopK(10));
                program.push(OpCode::Emit);
            }
            AstNode::SelfRag {
                query,
                threshold,
                low_plan,
                high_plan,
            } => {
                program.push(OpCode::RetrieveQdrant {
                    query: query.clone(),
                    filters: None,
                    top_k: 10,
                });
                program.push(OpCode::ComputeSemantic);

                let low_prog = Planner::compile(*low_plan);
                let high_prog = Planner::compile(*high_plan);
                program.push(OpCode::IfConfidence(threshold, low_prog, high_prog));
            }
            AstNode::CorrectiveRag {
                query: _,
                fallback_query,
            } => {
                program.push(OpCode::RetrieveQdrant {
                    query: fallback_query.clone(),
                    filters: None,
                    top_k: 10,
                });
                program.push(OpCode::ComputeSemantic);
                program.push(OpCode::TopK(10));
                program.push(OpCode::Emit);
            }
            AstNode::ColbertRetrieval { query, docs } => {
                let mut docs_prog = Planner::compile(*docs);
                if let Some(last) = docs_prog.instructions.last() {
                    if last.op == OpCode::Emit {
                        docs_prog.instructions.pop();
                    }
                }
                program.instructions.extend(docs_prog.instructions);
                program.push(OpCode::LoadColbertQuery(query));
                program.push(OpCode::ColbertMaxSim);
                program.push(OpCode::TopK(10));
                program.push(OpCode::Emit);
            }
            AstNode::AddMetadata {
                doc_uuid,
                key,
                value,
            } => {
                program.push(OpCode::AddMetadata {
                    doc_uuid,
                    key,
                    value,
                });
                program.push(OpCode::Emit);
            }
            AstNode::AddAnchor {
                doc_uuid,
                name,
                text,
            } => {
                program.push(OpCode::AddAnchor {
                    doc_uuid,
                    name,
                    text,
                });
                program.push(OpCode::Emit);
            }
            AstNode::AddEdge {
                source,
                target,
                edge_type,
                metadata,
            } => {
                program.push(OpCode::AddEdge {
                    source,
                    target,
                    edge_type,
                    metadata,
                });
                program.push(OpCode::Emit);
            }
        }

        program
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_planner_compile_hybrid() {
        let request_json = r#"{
            "query": "test query",
            "filters": null,
            "rerank_strategy": {
                "type": "hybrid",
                "semantic_weight": 0.7,
                "bm25_weight": 0.3
            },
            "final_top_k": 5
        }"#;

        let request: QueryRequest = serde_json::from_str(request_json).unwrap();
        let ast = AstNode::Search(request);
        let program = Planner::compile(ast);

        assert_eq!(program.instructions.len(), 8);
        assert!(matches!(
            program.instructions[0].op,
            OpCode::RetrieveQdrant { .. }
        ));
        assert_eq!(program.instructions[1].op, OpCode::ComputeSemantic);
        assert!(matches!(
            program.instructions[2].op,
            OpCode::RetrievePostgres { .. }
        ));
        assert_eq!(program.instructions[3].op, OpCode::LoadConst(0.7));
        assert_eq!(program.instructions[4].op, OpCode::LoadConst(0.3));
        assert_eq!(program.instructions[5].op, OpCode::Fusion);
        assert_eq!(program.instructions[6].op, OpCode::TopK(5));
        assert_eq!(program.instructions[7].op, OpCode::Emit);
    }

    #[test]
    fn test_query_request_deserialization_defaults() {
        let request_json = r#"{
            "query": "minimal query"
        }"#;

        let request: QueryRequest = serde_json::from_str(request_json).unwrap();
        assert_eq!(request.query, "minimal query");
        assert_eq!(request.filters, None);
        assert_eq!(
            request.rerank_strategy,
            RerankStrategy::Hybrid {
                semantic_weight: 0.7,
                bm25_weight: 0.3
            }
        );
        assert_eq!(request.final_top_k, 5);
        assert_eq!(request.doc_uuid, None);
        assert_eq!(request.alpha, None);
    }

    #[test]
    fn test_alpha_overrides_default_strategy_with_hybrid_weights() {
        let request_json = r#"{
            "query": "alpha query",
            "final_top_k": 3,
            "alpha": 0.25
        }"#;

        let request: QueryRequest = serde_json::from_str(request_json).unwrap();
        let program = Planner::compile(AstNode::Search(request));

        assert_eq!(program.instructions.len(), 8);
        assert!(matches!(
            program.instructions[0].op,
            OpCode::RetrieveQdrant { top_k: 100, .. }
        ));
        assert_eq!(program.instructions[1].op, OpCode::ComputeSemantic);
        assert!(matches!(
            program.instructions[2].op,
            OpCode::RetrievePostgres { .. }
        ));
        assert_eq!(program.instructions[3].op, OpCode::LoadConst(0.25));
        assert_eq!(program.instructions[4].op, OpCode::LoadConst(0.75));
        assert_eq!(program.instructions[5].op, OpCode::Fusion);
        assert_eq!(program.instructions[6].op, OpCode::TopK(3));
        assert_eq!(program.instructions[7].op, OpCode::Emit);
    }

    #[test]
    fn test_planner_compile_rsf() {
        let request_json = r#"{
            "query": "Alice Liddell and White Rabbit",
            "rerank_strategy": {
                "type": "rsf",
                "dense_weight": 0.6,
                "bm25_weight": 0.2,
                "entity_weight": 0.2
            },
            "final_top_k": 4
        }"#;

        let request: QueryRequest = serde_json::from_str(request_json).unwrap();
        let program = Planner::compile(AstNode::Search(request));

        assert!(matches!(
            program.instructions[2].op,
            OpCode::RetrievePostgres { .. }
        ));
        assert!(matches!(
            program.instructions[3].op,
            OpCode::RetrieveEntityScores { .. }
        ));
        assert!(matches!(program.instructions[4].op, OpCode::Rsf(_)));
        assert_eq!(program.instructions[5].op, OpCode::TopK(4));
    }

    #[test]
    fn test_final_top_k_is_clamped_by_configured_max() {
        let request = QueryRequest {
            query: "clamp query".to_string(),
            filters: None,
            rerank_strategy: RerankStrategy::SemanticOnly,
            final_top_k: usize::MAX,
            doc_uuid: None,
            alpha: None,
            anchor_name: None,
            anchor_mode: None,
            metadata_filters: None,
        };

        let program = Planner::compile(AstNode::Search(request));
        let expected_top_k = crate::config::get_search_max_top_k();

        assert!(matches!(
            program.instructions[0].op,
            OpCode::RetrieveQdrant { top_k, .. } if top_k == expected_top_k.saturating_mul(10)
        ));
        assert_eq!(program.instructions[2].op, OpCode::TopK(expected_top_k));
    }
}
