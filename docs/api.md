# API Reference

## MCP Protocol

The server accepts JSON payloads via the Model Context Protocol (MCP).

### McpRequest

```json
{
  "tool": "search",
  "args": { ... },
  "session_id": "string",
  "sequence_number": 1
}
```

| Field | Type | Description |
|-------|------|-------------|
| `tool` | `string` | Tool name (currently only `"search"` is supported) |
| `args` | `object` | Tool-specific arguments (see below) |
| `session_id` | `string` | Session identifier for message ordering |
| `sequence_number` | `integer` | Monotonically increasing sequence number within the session |

### McpResponse

Returns a result object with the search hits serialized as a FlatBuffer (binary).

### Tool: `search`

Accepts either a structured JSON query or a DSL string.

**JSON mode:**

```json
{
  "query": "quantum physics",
  "filters": {
    "year": { "gte": 2023 },
    "source": "arxiv"
  },
  "rerank_strategy": {
    "type": "hybrid",
    "semantic_weight": 0.7,
    "bm25_weight": 0.3
  },
  "final_top_k": 5
}
```

**DSL mode:**

```json
{
  "dsl": "(search :query \"quantum physics\" :final-top-k 5)"
}
```

### Error Responses

| Error | Description |
|-------|-------------|
| Out-of-order sequence | `sequence_number` does not match expected value for the session |
| Unknown tool | The `tool` field does not match a registered tool |
| Parse error | DSL or JSON could not be parsed |
| VM instruction limit | Execution exceeded the maximum instruction count (default 100,000) |

---

## DSL Reference

A Lisp-style s-expression language parsed via the `lexpr` crate.

### Query Forms

#### `search`

Single retrieval with optional filters, reranking, and top-k selection.

```scheme
(search :query "search terms"
        :filters (>= year 2023)
        :retrieval (semantic :top 1000)
        :rerank (hybrid :weights (sem 0.7 bm25 0.3))
        :final-top-k 5)
```

| Parameter | Type | Description |
|-----------|------|-------------|
| `:query` | `string` | Search query text |
| `:filters` | `sexpr` | Metadata filter expression |
| `:retrieval` | `sexpr` | Retrieval strategy (`semantic`, `bm25`) with `:top` count |
| `:rerank` | `sexpr` | Rerank strategy (see below) |
| `:final-top-k` | `integer` | Number of results to return |

#### `multi-hop`

Iterative multi-hop retrieval with N sequential passes.

```scheme
(multi-hop :query "quantum error correction"
           :hops 3
           :rerank (hybrid :weights (sem 0.6 bm25 0.4))
           :final-top-k 5)
```

| Parameter | Type | Description |
|-----------|------|-------------|
| `:query` | `string` | Initial search query |
| `:hops` | `integer` | Number of retrieval passes |
| `:rerank` | `sexpr` | Rerank strategy applied at each hop |
| `:final-top-k` | `integer` | Final result count |

#### `self-rag`

Confidence-gated branching with low/high confidence sub-plans.

```scheme
(self-rag :query "latest AI research"
          :threshold 0.8
          :low-confidence (search :query "AI research survey" :final-top-k 10)
          :high-confidence (search :query "latest AI research" :final-top-k 5)
          :final-top-k 5)
```

| Parameter | Type | Description |
|-----------|------|-------------|
| `:query` | `string` | Search query |
| `:threshold` | `float` | Confidence threshold for branching |
| `:low-confidence` | `sexpr` | Sub-plan when max score < threshold |
| `:high-confidence` | `sexpr` | Sub-plan when max score >= threshold |
| `:final-top-k` | `integer` | Final result count |

#### `corrective-rag`

Fallback query re-route on primary failure.

```scheme
(corrective-rag :query "specific technical detail"
                :fallback-query "technical overview"
                :rerank (rrf)
                :final-top-k 5)
```

| Parameter | Type | Description |
|-----------|------|-------------|
| `:query` | `string` | Primary search query |
| `:fallback-query` | `string` | Fallback query if primary yields low confidence |
| `:rerank` | `sexpr` | Rerank strategy |
| `:final-top-k` | `integer` | Final result count |

### Rerank Strategies

| Strategy | Syntax | Description |
|----------|--------|-------------|
| `semantic` | `(semantic)` | Pure vector similarity (dot product) |
| `bm25` | `(bm25)` | Pure BM25 scoring |
| `hybrid` | `(hybrid :weights (sem 0.7 bm25 0.3))` | Weighted blend of semantic and BM25 |
| `mmr` | `(mmr :lambda 0.7)` | Maximum Marginal Relevance for diversity |
| `rrf` | `(rrf)` | Reciprocal Rank Fusion |
| `dynamic` | `(custom-name :key value ...)` | Runtime-registered custom strategy |

### Filter Expressions

```scheme
(>= year 2023)          ; year >= 2023
(= source "arxiv")      ; source equals "arxiv"
(and (>= year 2023) (= source "arxiv"))  ; combined filter
```

---

## VM Opcodes

The stack-based vector VM supports 17 opcodes. All compute opcodes operate on Arrow arrays for SIMD-friendly execution.

### Data Movement

| OpCode | Stack Effect | Description |
|--------|-------------|-------------|
| `LoadConst` | `→ scalar` | Push a scalar constant onto the stack |

### Retrieval (I/O — handled by orchestrator)

| OpCode | Stack Effect | Description |
|--------|-------------|-------------|
| `RetrieveQdrant` | `→ arrow_array` | Fetch candidate vectors from Qdrant |
| `RetrievePostgres` | `→ arrow_array` | Fetch BM25 scores from PostgreSQL |

### Arithmetic

| OpCode | Stack Effect | Description |
|--------|-------------|-------------|
| `Add` | `a b → a+b` | Element-wise tensor addition |
| `Mul` | `a b → a*b` | Element-wise tensor-scalar multiplication |
| `Div` | `a b → a/b` | Element-wise tensor division |

### Compute

| OpCode | Stack Effect | Description |
|--------|-------------|-------------|
| `ComputeSemantic` | `query candidates → scores` | Dot product of query vector against candidates |
| `Fusion` | `a b w1 w2 → blended` | Weighted blend: `a * w1 + b * w2` |

### Selection

| OpCode | Stack Effect | Description |
|--------|-------------|-------------|
| `TopK` | `scores k → indices scores` | Sort and select top-K indices and scores |
| `Mmr` | `scores lambda → indices scores` | Maximum Marginal Relevance |
| `Rrf` | `scores → indices scores` | Reciprocal Rank Fusion |

### Control Flow

| OpCode | Stack Effect | Description |
|--------|-------------|-------------|
| `IfConfidence` | `scores threshold → branch` | Branch on max score threshold (handled by orchestrator) |
| `CallDynamic` | `args → result` | Runtime-dispatchable custom strategy |
| `Emit` | `result →` | Return result and end execution |

### Value Types

| Variant | Rust Type | Description |
|---------|-----------|-------------|
| `Scalar(f32)` | `f32` | Single floating-point value |
| `Tensor1D(Array1<f32>)` | `ndarray::Array1<f32>` | 1D tensor (backwards-compatible path) |
| `ArrowArray(ArrayRef)` | `arrow::array::ArrayRef` | Arrow array (zero-copy path) |
| `Result(TopKResult)` | `{ indices: Vec<usize>, scores: Vec<f32> }` | Final output |

---

## Programmatic Rust API

### DbPools

```rust
use tensor_rag::db::DbPools;

let mut pools = DbPools::new();

// PostgreSQL
pools.init_postgres("postgres://user:pass@localhost/db").await?;

// Qdrant nodes
pools.add_qdrant_node("default", "http://localhost:6334").await?;

// ML servers
pools.add_ml_server("embedding", "http://embedding-service:50051").await?;
```

### ToolHandler Trait

Custom tools can be registered via the `ToolHandler` trait:

```rust
#[async_trait]
trait ToolHandler {
    fn name(&self) -> &str;
    async fn handle(&self, args: &serde_json::Value) -> Result<McpResponse>;
}
```

### Planner

```rust
use tensor_rag::ir::{Planner, AstNode, Program};

let ast = parse_dsl("(search :query \"test\" :final-top-k 5)")?;
let program: Program = Planner::compile(&ast)?;
// program contains linear bytecode ready for VM execution
```

### SharedMemoryManager

```rust
use tensor_rag::shared_memory::SharedMemoryManager;

let mut manager = SharedMemoryManager::new("/dev/shm".into());
let region = manager.create_region(1024 * 1024)?; // 1MB region
// region.data() returns a &mut [u8] backed by mmap
```
