# APL-RAG: Tensor Machine Project Summary

## Overview

APL-RAG is a next-generation RAG (Retrieval-Augmented Generation) system that treats semantic search as a vector relational DBMS with a query compiler. The architecture is defined in `APL_RAG_ARCHITECTURE.md` and implemented in the `tensor-rag/` Rust crate.

The core insight: separate the system into a **Planner** (DSL → IR) and an **Executor** (Tensor VM), keeping the entire production stack in a single language (Rust) with an optional Lisp-like DSL for humans and agents.

## Architectural Triad

### 1. Rust Control Plane & Transport (`mcp.rs`, `db.rs`)

Handles networking, memory management, and safety.

- **MCP Server** (`mcp.rs`): Entry point for LLM agents via Model Context Protocol. Supports long-lived sessions with guaranteed message ordering. Tools are registered via the `ToolHandler` trait.
- **Connection Pools** (`db.rs`): Async pools for Qdrant (gRPC via `tonic`), PostgreSQL (`sqlx`), and external ML servers.
- **Predicate Pushdown**: Filters (date, source, BM25 threshold) are applied before vector retrieval, minimizing data transfer.

### 2. Lisp-like DSL (`dsl.rs`)

An s-expression-based language parsed via `lexpr` that compiles queries into IR.

Supported forms:

| Form | Description |
|------|-------------|
| `(search ...)` | Single search with filters, rerank strategy, top-k |
| `(multi-hop ...)` | Iterative multi-hop retrieval |
| `(self-rag ...)` | Conditional branching based on confidence threshold |
| `(corrective-rag ...)` | Fallback query on failure |

Rerank strategies: `semantic`, `bm25`, `hybrid`, `mmr`, `rrf`.

JSON input is also supported as a fallback (via `serde_json` deserialization into `QueryRequest`).

### 3. Tensor Execution Engine (`ir.rs`, `vm.rs`)

A minimal stack-based vector VM with Arrow-native computation.

**IR Instructions** (`ir.rs`):

| OpCode | Description |
|--------|-------------|
| `RetrieveQdrant` | Fetch candidate IDs and vectors from Qdrant |
| `RetrievePostgres` | Fetch BM25 scores and metadata from PostgreSQL |
| `ComputeSemantic` | Dot product of query vector with retrieved vectors |
| `LoadConst` | Push a scalar constant |
| `Add` | Element-wise tensor addition |
| `Mul` | Element-wise tensor-scalar multiplication |
| `Fusion` | Weighted blending of two tensors |
| `TopK` | Select top-K elements with indices |
| `Mmr` | Maximum Marginal Relevance (diversity) |
| `Rrf` | Reciprocal Rank Fusion |
| `IfConfidence` | Conditional branch based on max score |

**Stack-based VM** (`vm.rs`):
- Values are `Scalar(f32)`, `Tensor1D(Array1<f32>)`, `ArrowArray(ArrayRef)`, or `Result(TopKResult)`.
- Operates on Arrow `Float32Array` for SIMD-friendly computation via `arrow::compute`.
- Supports hybrid fusion, MMR, RRF, and conditional branching (Self-RAG).

## Data Flow (Arrow Pipeline)

The `arrow.rs` module defines a zero-copy pipeline:

1. **Qdrant** returns binary ANN results via gRPC → `bytes::Bytes` buffer.
2. **Arrow RecordBatch** is constructed with schema: `id: Int64`, `vector: FixedSizeList<Float32>[768]`, `bm25_score: Float32`, `text: Utf8`.
3. **Tensor VM** receives `Arc<RecordBatch>` and operates on columns directly.
4. **Result** is returned as a small JSON payload (indices + scores).

BM25 scores are pre-computed at indexing time and fetched from PostgreSQL as a raw `Float32Array`.

## Query Lifecycle Example

```
LLM Agent → "Find top-5 quantum physics articles from 2023+"
  ↓
1. MCP receives tool call → DSL or JSON parsed into AstNode
2. Planner compiles AstNode → Program (linear IR bytecode)
3. Fetching (async parallel):
   - Qdrant: Top-1000 vectors with filter year >= 2023
   - PostgreSQL: BM25 scores for same IDs
4. Binding: Data merged into Arrow RecordBatch
5. VM execution: mul(0.7) + mul(0.3) + add + topk(5)
6. Resolution: Top-5 texts returned via MCP
```

## Current Implementation Status

| Module | Lines | Status |
|--------|-------|--------|
| `mcp.rs` | 152 | MCP server with session management, `SearchTool`, sequence ordering |
| `ir.rs` | 231 | IR definitions, `Planner::compile` for all AST node types |
| `vm.rs` | 353 | Stack VM with all opcodes, Arrow-native compute, `IfConfidence` branching |
| `dsl.rs` | 391 | S-expression parser with `lexpr`, all query forms and rerank strategies |
| `arrow.rs` | 159 | Arrow schema, `qdrant_to_record_batch`, `postgres_to_record_batch`, score extraction |
| `db.rs` | 75 | Connection pool manager (PgPool, Qdrant gRPC channels), mock search/scores |
| `lib.rs` | 109 | Module declarations, E2E test, P99 latency benchmark, 10k session memory test |

## Non-Functional Requirements (Targets)

| Metric | Target | Tested |
|--------|--------|--------|
| P99 latency (Top-1000, VM only) | < 5 ms | ~50 µs in CI |
| Memory per 10k MCP sessions | < 500 MB | Test passes |
| Add new rerank strategy | < 1 hour (DSL macro) | |
| Throughput | 50,000 req/s per server | |
| Memory safety | Compile-time guaranteed (Rust) | |

## Roadmap (from APL_RAG_ARCHITECTURE.md)

1. **MVP Core** (current): MCP server, JSON parser → IR, Tensor VM (5+ instructions), direct Qdrant/PostgreSQL calls.
2. **DSL Layer**: Full `steel-scheme` integration, macros for hybrid rerank, multi-hop, Self-RAG.
3. **Zero-Copy**: Arrow RecordBatch full integration, mmap regions for intra-process sharing.
4. **Production**: GPU backend (WGPU), ColBERT late interaction, horizontal scaling (Arrow Flight), observability (OpenTelemetry).

## Key Design Decisions

- **Why Rust + Arrow?** Eliminates GC pauses, enables zero-copy data sharing, and provides SIMD-vectorized compute via `arrow::compute`.
- **Why s-expressions for DSL?** Nested logic and rerank function composition map naturally to s-expressions; `lexpr` provides a ready-made parser.
- **Why stack-based VM?** Simpler representation for a linear sequence of tensor operations; avoids register allocation complexity.
- **BM25 pre-computed**: BM25 scores are computed at indexing time (in PostgreSQL or Rust), not at query time. The VM only sees numeric arrays.

## Project Structure

```
tensor-rag/
  src/
    lib.rs       — Crate root, module declarations, integration tests
    mcp.rs       — MCP server, session management, SearchTool
    ir.rs        — IR types, Planner, OpCode enum, Program
    vm.rs        — Stack VM, all opcode implementations, TopKResult
    dsl.rs       — S-expression parser, all query forms
    arrow.rs     — Arrow schema, RecordBatch construction, score extraction
    db.rs        — Connection pools (PostgreSQL, Qdrant, ML servers)
```
