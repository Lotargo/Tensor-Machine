use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct QueryRequest {
    pub query: String,
    pub filters: Option<Vec<Filter>>,
    pub rerank_strategy: RerankStrategy,
    pub final_top_k: usize,
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
    #[serde(rename = "dynamic")]
    Dynamic {
        name: String,
        args: serde_json::Value,
    },
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
    RetrievePostgres,
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

impl Planner {
    pub fn compile(ast: AstNode) -> Program {
        let mut program = Program::new();

        match ast {
            AstNode::Search(request) => {
                let initial_k = request.final_top_k * 10;
                program.push(OpCode::RetrieveQdrant {
                    query: request.query.clone(),
                    filters: request.filters.clone(),
                    top_k: initial_k,
                });

                program.push(OpCode::ComputeSemantic);

                match request.rerank_strategy {
                    RerankStrategy::SemanticOnly => {
                        program.push(OpCode::TopK(request.final_top_k));
                        program.push(OpCode::Emit);
                    }
                    RerankStrategy::Bm25Only => {
                        program.push(OpCode::RetrievePostgres);
                        program.push(OpCode::TopK(request.final_top_k));
                        program.push(OpCode::Emit);
                    }
                    RerankStrategy::Hybrid {
                        semantic_weight,
                        bm25_weight,
                    } => {
                        program.push(OpCode::RetrievePostgres);
                        program.push(OpCode::LoadConst(semantic_weight));
                        program.push(OpCode::LoadConst(bm25_weight));
                        program.push(OpCode::Fusion);
                        program.push(OpCode::TopK(request.final_top_k));
                        program.push(OpCode::Emit);
                    }
                    RerankStrategy::Mmr { diversity } => {
                        program.push(OpCode::Mmr(diversity));
                        program.push(OpCode::TopK(request.final_top_k));
                        program.push(OpCode::Emit);
                    }
                    RerankStrategy::Rrf { k } => {
                        program.push(OpCode::RetrievePostgres);
                        program.push(OpCode::Rrf(k));
                        program.push(OpCode::TopK(request.final_top_k));
                        program.push(OpCode::Emit);
                    }
                    RerankStrategy::Dynamic { name, args } => {
                        program.push(OpCode::CallDynamic(name, args));
                        program.push(OpCode::TopK(request.final_top_k));
                        program.push(OpCode::Emit);
                    }
                }
            }
            AstNode::MultiHop { query, hops } => {
                for _ in 0..hops {
                    program.push(OpCode::RetrieveQdrant {
                        query: query.clone(),
                        filters: None,
                        top_k: 10, // Mock fixed top_k
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
                // Corrective Rag mocks fallback
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
                // Remove the last Emit if there is one
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
        assert_eq!(program.instructions[2].op, OpCode::RetrievePostgres);
        assert_eq!(program.instructions[3].op, OpCode::LoadConst(0.7));
        assert_eq!(program.instructions[4].op, OpCode::LoadConst(0.3));
        assert_eq!(program.instructions[5].op, OpCode::Fusion);
        assert_eq!(program.instructions[6].op, OpCode::TopK(5));
        assert_eq!(program.instructions[7].op, OpCode::Emit);
    }
}
