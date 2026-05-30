# Phase 1 MVP — Implementation Documentation

## Overview

Phase 1 implements the MVP core described in `APL_RAG_ARCHITECTURE.md` §IX (items 1–4):
an MCP server accepting JSON or s-expression queries, a Planner that compiles them
into linear IR bytecode, connection pools for Qdrant and PostgreSQL, and a stack-based
Tensor VM executing 17 opcodes over Arrow-native arrays.

---

## 1. MCP Server (`src/mcp.rs`, 243 lines)

### Architecture

- **`McpServer`** — holds a `HashMap<String, Box<dyn ToolHandler>>` for dispatch,
  a `HashMap<String, McpSession>` for session state, and an `Arc<DbPools>` reference.
- **`McpSession`** — per-session `mpsc::Sender<McpRequest>` channel plus an
  `expected_sequence` counter enforcing **guaranteed message ordering**.
- **`McpRequest`** / **`McpResponse`** — serde-serializable JSON structs with
  `tool`, `args`, `session_id`, and `sequence_number`.

### Request Lifecycle

1. `handle_request()` validates the session exists and sequence number matches.
2. On mismatch, returns an error with expected vs. actual sequence.
3. After validation, forwards the request into the session channel (for optional
   background consumption) and dispatches `args` to the registered `ToolHandler`.
4. The handler returns raw `Vec<u8>` bytes (FlatBuffer-encoded `SearchResult`).

### Tool Registration

```rust
pub trait ToolHandler: Send + Sync {
    fn handle<'a>(&'a self, args: JsonValue, db_pools: &'a DbPools)
        -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send + 'a>>;
}
```

- **`SearchTool`** — the single registered tool. Accepts JSON (deserialized into
  `QueryRequest` → `AstNode::Search`) **or** a `"dsl"` string key triggering
  `dsl::parse_dsl()` for s-expression input.
- Calls `Planner::compile(ast)` → `VM::new().run(program)` with I/O resolved by
  `execute_program()` (async overlay on the synchronous VM).
- The result `Value::Result(TopKResult)` is serialized to a FlatBuffer
  (`tensor_rag.fbs.SearchResult`) with `top_indices: [int]` and `top_scores: [float]`.

### 10k Session Test

`lib.rs:test_10k_mcp_sessions` spawns 10,000 sessions via `create_session()` and
asserts RSS increase < 500 MB. Measured ~0.01 MB/session overhead.

---

## 2. DB Connector Pools (`src/db.rs`, 158 lines)

### `DbPools`

```rust
pub struct DbPools {
    pub postgres: Option<PgPool>,            // sqlx::PgPool, max 5 connections
    pub qdrant_channels: HashMap<String, Channel>,  // tonic gRPC channels
    pub ml_servers: HashMap<String, Channel>,
}
```

All fields are `Clone` (via `Arc`-like refcounting in `sqlx`/`tonic`).

### Initialization

| Method                     | Pool                                           | Config        |
| -------------------------- | ---------------------------------------------- | ------------- |
| `init_postgres(url)`       | `PgPoolOptions::new().max_connections(5)`      | Async connect |
| `add_qdrant_node(id, url)` | `tonic::Endpoint::from_shared(url)?.connect()` | Async connect |
| `add_ml_server(id, url)`   | Same as Qdrant                                 | Async connect |

### Mock I/O (MVP default)

When no real backend is configured, methods return synthetic data:

- **`search_qdrant()`** — builds a JSON payload logging the query intent
  (vector, limit, filters rendered as Qdrant `must` conditions). Returns `top_k`
  dummy IDs `(0..top_k)` and vectors `[0.1; 768]`.
- **`fetch_bm25_scores()`** — queries `SELECT id, bm25_score FROM documents WHERE
id = ANY($1)`. Returns `1.0` for each ID when no real pool exists.
- **`fetch_metadata()`** — queries `SELECT id, text FROM documents WHERE
id = ANY($1)`. Returns `"Metadata for doc {id}"` fallback.

Filters support operators: `==`, `>=`, `>`, `<=`, `<`, `in`, with a fallback to
`match` for unknown operators.

---

## 3. JSON/DSL → IR Planner Compilation (`src/ir.rs`, 253 lines)

### Data Model

```
AstNode
├── Search(QueryRequest)
│     ├── query: String
│     ├── filters: Option<Vec<Filter>>
│     ├── rerank_strategy: RerankStrategy
│     │     ├── SemanticOnly
│     │     ├── Bm25Only
│     │     ├── Hybrid { semantic_weight, bm25_weight }
│     │     ├── Mmr { diversity }
│     │     ├── Rrf { k }
│     │     └── Dynamic { name, args }
│     └── final_top_k: usize
├── MultiHop { query, hops }
├── SelfRag { query, threshold, low_plan, high_plan }
└── CorrectiveRag { query, fallback_query }
```

### Planner::compile(AstNode) → Program

The `Planner` walks the AST and emits a linear `Vec<Instruction>`.

| AST Node                    | Emitted IR Sequence                                                                                                           |
| --------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| **Search + SemanticOnly**   | `RetrieveQdrant(top_k×10)` → `ComputeSemantic` → `TopK(k)` → `Emit`                                                           |
| **Search + Bm25Only**       | `RetrieveQdrant` → `ComputeSemantic` → `RetrievePostgres` → `TopK(k)` → `Emit`                                                |
| **Search + Hybrid(w1,w2)**  | `RetrieveQdrant` → `ComputeSemantic` → `RetrievePostgres` → `LoadConst(w1)` → `LoadConst(w2)` → `Fusion` → `TopK(k)` → `Emit` |
| **Search + Mmr(div)**       | `RetrieveQdrant` → `ComputeSemantic` → `Mmr(div)` → `TopK(k)` → `Emit`                                                        |
| **Search + Rrf(k)**         | `RetrieveQdrant` → `ComputeSemantic` → `RetrievePostgres` → `Rrf(k)` → `TopK(k)` → `Emit`                                     |
| **Search + Dynamic**        | `RetrieveQdrant` → `ComputeSemantic` → `CallDynamic(name,args)` → `TopK(k)` → `Emit`                                          |
| **MultiHop(n)**             | `RetrieveQdrant` × `n` → `ComputeSemantic` → `TopK(10)` → `Emit`                                                              |
| **SelfRag(th,low,high)**    | `RetrieveQdrant` → `ComputeSemantic` → `IfConfidence(th, compile(low), compile(high))`                                        |
| **CorrectiveRag(fallback)** | `RetrieveQdrant`(fallback) → `ComputeSemantic` → `TopK(10)` → `Emit`                                                          |

### JSON Input

`QueryRequest` derives `Deserialize` via serde. The `RerankStrategy` uses
`#[serde(tag = "type")]` for JSON discriminator (`"semantic"`, `"bm25"`,
`"hybrid"`, `"mmr"`, `"rrf"`, `"dynamic"`).

### S-Expression Input (`src/dsl.rs`, 447 lines)

Parser via `lexpr` from `steel-scheme`. Supports:

```
(search :query "quantum physics"
        :filters (>= year 2023)
        :rerank (hybrid :weights (sem 0.7 bm25 0.3))
        :final-top-k 5)
```

Rerank strategies: `semantic`, `bm25`, `hybrid`, `mmr` (`:diversity`), `rrf` (`:k`),
or any custom name (captured as `Dynamic` with key-value args).

---

## 4. Tensor VM — Base Opcodes (`src/vm.rs`, 532 lines)

### Value Stack Types

```rust
pub enum Value {
    Scalar(f32),
    Tensor1D(Array1<f32>),       // ndarray-backed
    ArrowArray(Arc<dyn Array>),  // zero-copy Arrow
    Result(TopKResult),
}
```

### VM State

```rust
pub struct VM {
    pub stack: Vec<Value>,
    pub batch: Option<Arc<RecordBatch>>,  // Arrow batch from I/O
    pub query_vector: Array1<f32>,        // 768-d query embedding
}
```

### Implemented Opcodes

| OpCode              | Stack Effect           | Implementation                                                            |
| ------------------- | ---------------------- | ------------------------------------------------------------------------- |
| `LoadConst(f32)`    | `→ scalar`             | Push scalar onto stack                                                    |
| `Add`               | `(t1, t2) → t`         | Element-wise add via `ndarray` or `arrow::compute::kernels::numeric::add` |
| `Mul`               | `(t, s) → t`           | Tensor-scalar mul via `ndarray` or `arrow::compute`                       |
| `Div`               | `(t1, t2) → t`         | Element-wise div via `arrow::compute::kernels::numeric::div`              |
| `Fusion`            | `(t1, t2, w1, w2) → t` | `t1 * w1 + t2 * w2` (weighted blend)                                      |
| `Matmul`            | `(t1, t2) → scalar`    | Dot product: `ndarray::dot` or `arrow::compute::sum(mul(t1,t2))`          |
| `Threshold(f)`      | `(t) → t`              | Zero out values < threshold                                               |
| `Sort`              | `(t) → t`              | Descending sort                                                           |
| `Slice(i, j)`       | `(t) → t`              | Range slice `[i..j)`                                                      |
| `TopK(k)`           | `(t) → Result`         | Descending sort, take top-k indices+scores                                |
| `Emit`              | `(v) → ✓`              | Pop and return as final result                                            |
| `ComputeSemantic`   | `→ t`                  | Dot product of `query_vector` with each batch row (768-d)                 |
| `Mmr(f)`            | `(t) → t`              | **Stub** — no-op                                                          |
| `Rrf(k)`            | `(t) → t`              | **Stub** — no-op                                                          |
| `CallDynamic(n, a)` | `(t) → t`              | **Stub** — no-op                                                          |

### I/O / Branching Opcodes (orchestrator-routed, not in `step()`)

| OpCode                        | Handler                                                                                        |
| ----------------------------- | ---------------------------------------------------------------------------------------------- |
| `RetrieveQdrant`              | `execute_program()` in `mcp.rs` — calls `DbPools::search_qdrant()`, builds Arrow `RecordBatch` |
| `RetrievePostgres`            | `execute_program()` — calls `DbPools::fetch_bm25_scores()`, pushes `Float32Array`              |
| `IfConfidence(th, low, high)` | `execute_program()` — pops tensor, finds max score, recurses into `low` or `high` sub-program  |

### Backward Compatibility `run()` Method

`VM::run()` provides a synchronous path for unit tests. It handles I/O opcodes
in-process (generating mock Arrow batches of dimension `top_k × 768` with all
values = 0.1, BM25 = 0.0, texts = ""). This allows the Planner + VM chain to be
tested without any live database connection.

### Arrow Schema (`src/arrow.rs`, 159 lines)

```
id:         Int64
vector:     FixedSizeList(Float32, 768)
bm25_score: Float32
text:       Utf8
```

- `qdrant_to_record_batch()` — builds batch from Qdrant results.
- `postgres_to_record_batch()` — builds batch from PostgreSQL results.
- `extract_vector_scores()` — flattens vector column to a contiguous `Float32Array`.
- `extract_float_column()` — generic float column extraction.

### Performance

The P99 latency test (`lib.rs:test_p99_latency_top_1000`) runs 100 iterations
of a `SemanticOnly` search with `final_top_k = 100` (→ 1000 candidates).
Assertion: < 5000 µs. Observed ~50 µs in CI.

---

## FlatBuffer Response Protocol (`src/result.fbs`)

```
namespace tensor_rag.fbs;

table SearchResult {
  top_indices: [int];
  top_scores: [float];
}

root_type SearchResult;
```

Generated Rust bindings in `result_generated.rs` (218 lines). The `SearchTool::handle`
method serializes final `TopKResult` into this compact binary format before returning
to the LLM agent.
