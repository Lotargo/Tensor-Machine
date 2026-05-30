# tensor-rag

A next-generation RAG (Retrieval-Augmented Generation) system built entirely in Rust. Tensor-rag treats semantic search as a vector relational DBMS with a query compiler, separating planning from execution for predictable latency and memory safety.

---

## Architecture

Tensor-rag is organized into three layers that run in a single Rust process, eliminating IPC overhead and multi-runtime coordination.

### 1. Control Plane and Transport

The Rust layer handles networking, memory management, and safety.

- **MCP Server** — Entry point for LLM agents via [Model Context Protocol](https://spec.modelcontextprotocol.io). Maintains long-lived sessions with guaranteed message ordering via sequence numbers.
- **Async I/O** — Connection pools for Qdrant (gRPC via `tonic`), PostgreSQL (`sqlx`), and external ML servers.
- **Predicate Pushdown** — Metadata filters (date, source, BM25 threshold) are applied server-side before vector transfer, minimizing data volume.
- **Shared Memory Manager** — `mmap`-backed regions for zero-copy data exchange between subsystems.

### 2. Declarative DSL

A Lisp-style s-expression language compiled into linear IR bytecode. Parsed via the `lexpr` crate.

**Query forms:**

| Form | Description |
|------|-------------|
| `search` | Single retrieval with filters, reranking, and top-k selection |
| `multi-hop` | Iterative multi-hop retrieval with N sequential passes |
| `self-rag` | Confidence-gated branching with low/high confidence sub-plans |
| `corrective-rag` | Fallback query re-route on primary failure |

**Example:**

```scheme
(search :query "quantum physics"
        :filters (>= year 2023)
        :rerank (hybrid :weights (sem 0.7 bm25 0.3))
        :final-top-k 5)
```

JSON input is also accepted as a fallback via `serde_json` deserialization into `QueryRequest`.

**Rerank strategies:**

| Strategy | Description |
|----------|-------------|
| `semantic` | Pure vector similarity (dot product) |
| `bm25` | Pure BM25 scoring |
| `hybrid` | Weighted blend of semantic and BM25 |
| `mmr` | Maximum Marginal Relevance for diversity |
| `rrf` | Reciprocal Rank Fusion |
| `(custom-name ...)` | Dynamic runtime-registered strategies |

### 3. Tensor Execution Engine

A stack-based vector VM with 17 opcodes that operates directly on Apache Arrow arrays.

**Core opcodes:**

| OpCode | Description |
|--------|-------------|
| `RetrieveQdrant` | Fetch candidate vectors from Qdrant |
| `RetrievePostgres` | Fetch BM25 scores and text from PostgreSQL |
| `ComputeSemantic` | Dot product of query vector against candidates |
| `LoadConst` | Push scalar onto stack |
| `Add` / `Mul` / `Div` | Element-wise tensor arithmetic |
| `Fusion` | Weighted blend: `a·w₁ + b·w₂` |
| `TopK` | Sort and select top-K indices and scores |
| `Mmr` | Maximum Marginal Relevance |
| `Rrf` | Reciprocal Rank Fusion |
| `IfConfidence` | Branch on max score threshold |
| `CallDynamic` | Runtime-dispatchable custom strategy |
| `Emit` | Return result |

The VM uses `arrow::compute` kernels for SIMD-friendly element-wise operations. All tensor operations are vectorized at the CPU level with no dynamic dispatch overhead.

### Zero-Copy Arrow Pipeline

All tensor data flows through Apache Arrow `RecordBatch` with a shared schema:

| Column | Arrow Type | Description |
|--------|------------|-------------|
| `id` | `Int64` | Document ID |
| `vector` | `FixedSizeList(Float32, 768)` | 768-dim embedding |
| `bm25_score` | `Float32` | Pre-computed BM25 score |
| `text` | `Utf8` | Document text |

Data flow:

1. **Qdrant** returns ANN results via gRPC into a `bytes::Bytes` buffer.
2. **Arrow RecordBatch** is built from the buffer (zero-copy when buffer layout matches).
3. **VM** receives the batch as `Arc<RecordBatch>` — no clone of the result set.
4. **Instructions** read directly from Arrow arrays via `flat.values()` — a zero-copy `&[f32]` slice.
5. **Result** returns as a compact FlatBuffer (indices + scores, ~50 bytes).

---

## Getting Started

### Prerequisites

- Rust **2024 edition** (nightly or stable with `edition = "2024"` support)
- Cargo

### Building

```bash
git clone <repository>
cd tensor-rag

# Build the release binary
cargo build --release

# Run tests
cargo test
```

### Starting the MCP Server

The MCP server is the primary entry point. It accepts tool calls from LLM agents via the Model Context Protocol.

```bash
# Run with default mock I/O (no external databases required)
cargo run --release

# Or run the binary directly after build
./target/release/tensor-rag
```

When running with mock I/O, the server returns synthetic results for all queries. This is suitable for testing the Planner → VM pipeline without external infrastructure.

#### Connecting from an LLM Agent

Send a JSON payload to the MCP server:

```json
{
  "tool": "search",
  "args": {
    "query": "quantum physics",
    "filters": null,
    "rerank_strategy": {
      "type": "hybrid",
      "semantic_weight": 0.7,
      "bm25_weight": 0.3
    },
    "final_top_k": 5
  },
  "session_id": "session_1",
  "sequence_number": 1
}
```

Or use the DSL directly:

```json
{
  "tool": "search",
  "args": {
    "dsl": "(search :query \"quantum physics\" :rerank (hybrid :weights (sem 0.7 bm25 0.3)) :final-top-k 5)"
  },
  "session_id": "session_1",
  "sequence_number": 1
}
```

---

## Configuration

### Environment Variables

The server reads configuration from environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | — | PostgreSQL connection string |
| `QDRANT_URL` | — | Qdrant gRPC endpoint |
| `QDRANT_COLLECTION` | — | Qdrant collection name |
| `MCP_BIND_ADDR` | `0.0.0.0:8080` | MCP server listen address |
| `MAX_CONNECTIONS` | `5` | PostgreSQL connection pool size |

### Database Configuration

**PostgreSQL** — Stores BM25 scores and document metadata:

```sql
CREATE TABLE documents (
    id BIGSERIAL PRIMARY KEY,
    text TEXT NOT NULL,
    bm25_score REAL NOT NULL DEFAULT 0.0
);
```

**Qdrant** — Stores vector embeddings. Configure via the Qdrant URI and collection name.

When no database is configured, the system runs in mock mode and returns synthetic data.

### Programmatic Configuration

Configure connection pools in Rust code:

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

---

## Project Structure

```
tensor-rag/
├── Cargo.toml
├── src/
│   ├── lib.rs              — Crate root, module declarations, integration tests
│   ├── mcp.rs              — MCP server, session management, SearchTool
│   ├── ir.rs               — IR types, Planner, OpCode enum, Program
│   ├── vm.rs               — Stack VM, all opcode implementations
│   ├── dsl.rs              — S-expression parser, all query forms
│   ├── arrow.rs            — Arrow schema, RecordBatch construction, score extraction
│   ├── db.rs               — Connection pools (PostgreSQL, Qdrant, ML servers)
│   ├── result.fbs          — FlatBuffer schema for search results
│   └── result_generated.rs — FlatBuffer generated Rust bindings
```

---

## Performance

Target non-functional requirements (measured on single-core CPU with AVX-512):

| Metric | Target | Current |
|--------|--------|---------|
| P99 latency (Top-1000, VM only) | < 5 ms | ~50 µs |
| Memory per 10k MCP sessions | < 500 MB | ~0.01 MB/session |
| Throughput | 50,000 req/s | — |

---

## Roadmap

1. **MVP Core** (✓) — MCP server, JSON/DSL → IR compiler, Tensor VM with 17 opcodes, direct Qdrant and PostgreSQL calls with mock fallback.
2. **Full DSL Integration** — `steel-scheme` runtime, macro-extensible rerank strategies, multi-hop and Self-RAG as macros.
3. **Zero-Copy** — Full Arrow RecordBatch pipeline, `mmap` regions for intra-process sharing, Arrow IPC for inter-process.
4. **Production** — GPU backend via WGPU, ColBERT late interaction, horizontal MCP scaling via Arrow Flight, OpenTelemetry observability.

---

## License

See LICENSE.
