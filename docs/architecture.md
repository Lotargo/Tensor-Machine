# Architecture

Tensor-rag is organized into three layers that run in a single Rust process, eliminating IPC overhead and multi-runtime coordination. The core design treats semantic search as a vector relational DBMS with a query compiler — separating **planning** (DSL → IR) from **execution** (Tensor VM).

## Architectural Overview

```
┌─────────────────────────────────────────────────────────────┐
│                      LLM Agent                              │
│                  (MCP Client)                                │
└──────────────────────────┬──────────────────────────────────┘
                           │ MCP (JSON/FlatBuffer)
                           ▼
┌─────────────────────────────────────────────────────────────┐
│  Layer 1: Control Plane & Transport                         │
│  ┌─────────────┐  ┌──────────┐  ┌───────────────────────┐  │
│  │ MCP Server   │  │ Sessions │  │ Shared Memory Manager │  │
│  │ (mcp.rs)     │  │          │  │ (shared_memory.rs)    │  │
│  └─────────────┘  └──────────┘  └───────────────────────┘  │
│  ┌─────────────┐  ┌──────────┐                              │
│  │ DbPools      │  │ Arrow    │                              │
│  │ (db.rs)      │  │ (arrow.rs)│                             │
│  └─────────────┘  └──────────┘                              │
└──────────────────────────┬──────────────────────────────────┘
                           │ AstNode / QueryRequest
                           ▼
┌─────────────────────────────────────────────────────────────┐
│  Layer 2: Declarative DSL                                   │
│  ┌──────────────────────────────────────────────────────┐   │
│  │ S-Expression Parser (dsl.rs)                         │   │
│  │ search | multi-hop | self-rag | corrective-rag       │   │
│  └──────────────────────┬───────────────────────────────┘   │
│                         │ Planner::compile()                 │
│  ┌──────────────────────▼───────────────────────────────┐   │
│  │ IR / Bytecode (ir.rs)                                │   │
│  │ Linear opcode program — deterministic, pure           │   │
│  └──────────────────────┬───────────────────────────────┘   │
└──────────────────────────┬──────────────────────────────────┘
                           │ Program (bytecode)
                           ▼
┌─────────────────────────────────────────────────────────────┐
│  Layer 3: Tensor Execution Engine                           │
│  ┌──────────────────────────────────────────────────────┐   │
│  │ Stack-Based Vector VM (vm.rs)                        │   │
│  │ 17 opcodes · Arrow-native · SIMD-friendly            │   │
│  │ Dual path: Tensor1D (ndarray) + ArrowArray           │   │
│  └──────────────────────────────────────────────────────┘   │
└──────────────────────────┬──────────────────────────────────┘
                           │ TopKResult → FlatBuffer
                           ▼
                      McpResponse
```

## Layer 1: Control Plane & Transport

The Rust layer handles networking, memory management, and safety.

### MCP Server (`mcp.rs`)

Entry point for LLM agents via [Model Context Protocol](https://spec.modelcontextprotocol.io). Key responsibilities:

- Accept tool calls as `McpRequest` JSON payloads
- Maintain long-lived sessions with guaranteed message ordering via sequence numbers
- Route requests through the Planner → VM pipeline
- Return results as compact FlatBuffers (~50 bytes for TopK results)

### Connection Pools (`db.rs`)

Async connection pools managed by `DbPools`:

| Backend | Library | Protocol | Purpose |
|---------|---------|----------|---------|
| PostgreSQL | `sqlx` | TCP | BM25 scores, document text |
| Qdrant | `tonic` | gRPC | Vector embeddings, ANN search |
| ML Servers | `tonic` | gRPC | Embedding models, rerankers |

All backends fall back to synthetic mock data when not configured, enabling full pipeline testing without external infrastructure.

### Predicate Pushdown

Metadata filters (date, source, BM25 threshold) are applied server-side in Qdrant and PostgreSQL **before** vector transfer, minimizing data volume over the network.

### Shared Memory Manager (`shared_memory.rs`)

`mmap`-backed regions for zero-copy data exchange between subsystems. Uses temporary files (backed by tmpfs on Linux) as mmap regions, with Arrow IPC serialization for the actual data format.

## Layer 2: Declarative DSL

A Lisp-style s-expression language parsed via the `lexpr` crate that compiles queries into linear IR bytecode.

### Query Forms

| Form | Description |
|------|-------------|
| `(search ...)` | Single retrieval with filters, reranking, and top-k selection |
| `(multi-hop ...)` | Iterative multi-hop retrieval with N sequential passes |
| `(self-rag ...)` | Confidence-gated branching with low/high confidence sub-plans |
| `(corrective-rag ...)` | Fallback query re-route on primary failure |

### Example

```scheme
(search :query "quantum physics"
        :filters (>= year 2023)
        :retrieval (semantic :top 1000)
        :rerank (hybrid :weights (sem 0.7 bm25 0.3))
        :final-top-k 5)
```

### Compilation Pipeline

```
DSL string → parse_dsl() → AstNode → Planner::compile() → Program (bytecode)
```

The `Planner::compile()` step is deterministic and pure — AST in, bytecode out. This mirrors database query compilation and enables future optimizations (caching, AOT compilation).

JSON input is also accepted as a fallback via `serde_json` deserialization into `QueryRequest`, which is then converted to an `AstNode`.

## Layer 3: Tensor Execution Engine

A minimal stack-based vector VM with 17 opcodes that operates directly on Apache Arrow arrays.

### Design Principles

- **No I/O in the VM:** I/O opcodes (`RetrieveQdrant`, `RetrievePostgres`, `IfConfidence`) are handled by the async orchestrator in `mcp.rs`, not by the synchronous VM. The VM explicitly rejects I/O opcodes with an error.
- **Dual-path opcodes:** Every compute opcode handles both `Value::Tensor1D` (ndarray, backwards-compatible) and `Value::ArrowArray` (zero-copy Arrow).
- **Instruction limit guard:** Configurable maximum instruction count (default 100,000) prevents runaway execution.

### Stack Values

| Variant | Type | Description |
|---------|------|-------------|
| `Scalar` | `f32` | Single floating-point value |
| `Tensor1D` | `Array1<f32>` | 1D ndarray (backwards-compatible path) |
| `ArrowArray` | `ArrayRef` | Apache Arrow array (zero-copy path) |
| `Result` | `TopKResult` | Final output: indices + scores |

### Core Opcodes

| OpCode | Description |
|--------|-------------|
| `RetrieveQdrant` | Fetch candidate vectors from Qdrant (I/O — handled by orchestrator) |
| `RetrievePostgres` | Fetch BM25 scores from PostgreSQL (I/O — handled by orchestrator) |
| `ComputeSemantic` | Dot product of query vector against candidates |
| `LoadConst` | Push scalar constant onto stack |
| `Add` / `Mul` / `Div` | Element-wise tensor arithmetic |
| `Fusion` | Weighted blend: `a * w1 + b * w2` |
| `TopK` | Sort and select top-K indices and scores |
| `Mmr` | Maximum Marginal Relevance for diversity |
| `Rrf` | Reciprocal Rank Fusion |
| `IfConfidence` | Branch on max score threshold (I/O — handled by orchestrator) |
| `CallDynamic` | Runtime-dispatchable custom strategy |
| `Emit` | Return result |

All tensor operations use `arrow::compute` kernels for SIMD-friendly element-wise computation with no dynamic dispatch overhead.

## Zero-Copy Arrow Pipeline

All tensor data flows through Apache Arrow `RecordBatch` with a shared schema:

| Column | Arrow Type | Description |
|--------|------------|-------------|
| `id` | `Int64` | Document ID |
| `vector` | `FixedSizeList(Float32, 768)` | 768-dim embedding |
| `bm25_score` | `Float32` | Pre-computed BM25 score |
| `text` | `Utf8` | Document text |

### Data Flow

```
Qdrant (gRPC) → bytes::Bytes buffer
  ↓ (zero-copy when buffer layout matches)
Arrow RecordBatch construction
  ↓ (Arc<RecordBatch> — shared reference, no clone)
VM executes opcodes on Arrow arrays directly
  ↓ (flat.values() → zero-copy &[f32] slice)
FlatBuffer result (indices + scores, ~50 bytes)
```

**BM25 scores** are pre-computed at indexing time and fetched from PostgreSQL as a raw `Float32Array`. The VM only sees numeric arrays — no access to raw text.

## Query Lifecycle

Complete flow for a hybrid search query:

```
1. MCP receives tool call
   → DSL or JSON parsed into AstNode

2. Planner compiles AstNode → Program (linear IR bytecode)

3. Async parallel fetching:
   → Qdrant: Top-1000 vectors with filter year >= 2023
   → PostgreSQL: BM25 scores for same IDs

4. Binding: Data merged into Arrow RecordBatch

5. VM execution: mul(0.7) + mul(0.3) + add + topk(5)

6. Resolution: Top-5 texts returned via MCP as FlatBuffer
```

## Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| **Rust + Arrow** | Eliminates GC pauses, enables zero-copy data sharing, SIMD-vectorized compute via `arrow::compute` |
| **S-expressions for DSL** | Nested logic and rerank function composition map naturally to s-expressions; `lexpr` provides a ready-made parser |
| **Stack-based VM** | Simpler representation for a linear sequence of tensor operations; avoids register allocation complexity |
| **BM25 pre-computed** | Scores computed at indexing time, not at query time. The VM only sees numeric arrays |
| **Single-process triad** | Eliminates IPC overhead and multi-runtime coordination between Control Plane, DSL, and VM |
| **Planner/Executor split** | Deterministic compilation enables future optimizations (caching, AOT) independent of execution |

## Comparison with Python RAG Frameworks

| Characteristic | LangChain / LlamaIndex | Tensor-rag |
|----------------|------------------------|------------|
| Execution language | Python (GIL, GC) | Rust (no GC, compile-time safety) |
| Strategy extensibility | Imperative code changes | Declarative DSL + IR compiler |
| Zero-copy vectors | None (NumPy copies) | Arrow + mmap, no-copy transfer |
| P99 latency | 50–150 ms | < 5 ms target |
| Agent support | Limited by GIL | High-concurrency MCP |
| New rerank algorithms | Modify Python code | DSL macro (no core recompilation) |
