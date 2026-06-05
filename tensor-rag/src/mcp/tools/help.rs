use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use serde_json::Value as JsonValue;
use super::ToolHandler;

pub struct HelpTool;

impl ToolHandler for HelpTool {
    fn handle<'a>(
        &'a self,
        _args: JsonValue,
        _db_pools: &'a crate::db::DbPools,
        _metrics: Option<Arc<crate::metrics::Metrics>>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send + 'a>> {
        Box::pin(async move {
            let help_text = r#"
# TENSOR MACHINE RAG HELP & DOCUMENTATION

## 1. System Overview
Tensor Machine is a hybrid search system combining vector search (dense embeddings) and keyword search (BM25 lexical scoring).
It supports document ingestion, automatic chunking, dense vector generation using E5 models, and vector storage in Qdrant, alongside metadata storage in PostgreSQL.

## 2. Tools Reference

### search
* **Purpose:** Queries the database using semantic search and lexical matching.
* **Arguments:**
  - `query` (string, required): The search text. Formulate the query in the SAME language as the user's message/question (e.g., Russian, English, Chinese). DO NOT use search engine question words (such as 'what', 'how', 'why', 'что', 'как', 'почему', etc.). Always query using only descriptive search keywords or phrases in the matching language. Examples: "скорость подключения Arrow Flight" (Russian) or "Arrow Flight connection speed" (English).
  - `final_top_k` (integer, optional): The number of top documents/segments to return. Defaults to the configured `.env` value.
  - `doc_uuid` (string, optional): Search only within the document with this UUID.
  - `alpha` (number, optional): Hybrid weighting where `1.0` is semantic-only and `0.0` is BM25-only.
  - `dsl` (string, optional): A query string written in S-expression DSL for advanced plans.
* **Best Practices:**
  - For general queries, use simple key phrases instead of long sentences. Do not use conversational filler or question words.
  - Run the `list_documents` tool first to find the `doc_uuid` if you want to restrict search to a single document.
  - Search responses are softly trimmed from the tail when they exceed the configured token budget.

### read_document
* **Purpose:** Reconstructs and returns the full text of one indexed document.
* **Arguments:**
  - `doc_uuid` (string, required): UUID of the target document returned by `list_documents`.
* **Behavior:**
  - If the document exceeds the configured full-read token limit, the tool returns a warning instead of the body.

### list_documents
* **Purpose:** Retrieves all indexed documents in the database.
* **Output:** A JSON array containing the `filename` and `doc_uuid` for each document. Use these UUIDs to search within specific files.

### help
* **Purpose:** Returns this documentation guide.

## 3. Query DSL (S-Expression)
If you require complex query orchestration, you can use the `dsl` field. Supported forms:
* Hybrid scoring: `(hybrid "query" semantic_weight bm25_weight)`
* MMR diversity: `(mmr "query" diversity_factor)`
* Reciprocal Rank Fusion: `(rrf "query" k)`
* Self-RAG conditional: `(self_rag "query" threshold low_plan_ast high_plan_ast)`
* Corrective RAG: `(corrective_rag "query" "fallback_query")`
"#;

            let response_json = serde_json::json!({
                "help": help_text
            });

            Ok(serde_json::to_vec(&response_json).unwrap())
        })
    }
}
