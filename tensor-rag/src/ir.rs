use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
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

#[derive(Debug, Deserialize, Serialize, Clone)]
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
}

#[derive(Debug, Clone, PartialEq)]
pub enum OpCode {
    /// Retrieve candidate IDs and vectors from Qdrant
    RetrieveQdrant { query: String, filters: Option<Vec<Filter>>, top_k: usize },
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
    /// Return the result
    Emit,
}

#[derive(Debug, Clone)]
pub struct Instruction {
    pub op: OpCode,
    // Add operands or register indices here in the future if needed for a register VM.
    // For now, assuming a stack-based VM for simplicity of representation.
}

#[derive(Debug, Clone)]
pub struct Program {
    pub instructions: Vec<Instruction>,
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
    pub fn compile(request: QueryRequest) -> Program {
        let mut program = Program::new();

        // 1. Retrieve candidates from Qdrant
        // We typically retrieve more candidates initially before reranking. Let's assume a default multiplier.
        let initial_k = request.final_top_k * 10;
        program.push(OpCode::RetrieveQdrant {
            query: request.query.clone(),
            filters: request.filters.clone(),
            top_k: initial_k,
        });

        // Compute semantic scores based on the retrieved vectors
        program.push(OpCode::ComputeSemantic);

        match request.rerank_strategy {
            RerankStrategy::SemanticOnly => {
                // No extra scores needed, just sort the semantic scores
                program.push(OpCode::TopK(request.final_top_k));
                program.push(OpCode::Emit);
            }
            RerankStrategy::Bm25Only => {
                // Need to fetch BM25 scores from Postgres
                program.push(OpCode::RetrievePostgres);
                // In a real VM, we would swap or drop the semantic scores,
                // but for now we assume RetrievePostgres puts BM25 scores on top of the stack.
                program.push(OpCode::TopK(request.final_top_k));
                program.push(OpCode::Emit);
            }
            RerankStrategy::Hybrid { semantic_weight, bm25_weight } => {
                // Need both scores
                program.push(OpCode::RetrievePostgres);

                // Assuming stack state: [..., SemanticScores, Bm25Scores]
                // We need to multiply SemanticScores by semantic_weight,
                // multiply Bm25Scores by bm25_weight, and then Add.
                // In a stack VM, this would require some stack manipulation.
                // For simplicity, we can use the Fusion instruction which does weighted blending.
                // Let's emit LoadConsts for weights and then Fusion.
                program.push(OpCode::LoadConst(semantic_weight));
                program.push(OpCode::LoadConst(bm25_weight));
                program.push(OpCode::Fusion);

                program.push(OpCode::TopK(request.final_top_k));
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
        let program = Planner::compile(request);

        assert_eq!(program.instructions.len(), 8);
        assert!(matches!(program.instructions[0].op, OpCode::RetrieveQdrant { .. }));
        assert_eq!(program.instructions[1].op, OpCode::ComputeSemantic);
        assert_eq!(program.instructions[2].op, OpCode::RetrievePostgres);
        assert_eq!(program.instructions[3].op, OpCode::LoadConst(0.7));
        assert_eq!(program.instructions[4].op, OpCode::LoadConst(0.3));
        assert_eq!(program.instructions[5].op, OpCode::Fusion);
        assert_eq!(program.instructions[6].op, OpCode::TopK(5));
        assert_eq!(program.instructions[7].op, OpCode::Emit);
    }
}
