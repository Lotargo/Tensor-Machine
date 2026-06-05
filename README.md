<div align="center">

![Tensor Machine Banner](./assets/hero-banner.svg)

# Tensor Machine — Low-Latency RAG Engine

### *Rust-powered hybrid semantic search with Tensor VM, GPU acceleration & LLM agent integration*

[![Rust Version](https://img.shields.io/badge/Rust-2024%2B-orange.svg?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![Build Status](https://img.shields.io/badge/Build-Passing-brightgreen.svg?style=for-the-badge&logo=github)](https://github.com/)
[![Vector Engine](https://img.shields.io/badge/Vector-Stack--VM%20%2F%20Arrow-blue.svg?style=for-the-badge)](https://github.com/)
[![License](https://img.shields.io/badge/License-MIT-green.svg?style=for-the-badge)](https://opensource.org/licenses/MIT)

</div>

---

## Overview

**Tensor Machine** is a next-generation RAG (Retrieval-Augmented Generation) system that treats semantic search as a **vector relational DBMS with a query compiler**. The entire production stack is written in Rust with an optional Lisp-like DSL for humans and agents.

The core design separates the system into a **Planner** (DSL → IR bytecode) and an **Executor** (stack-based Tensor VM), operating on zero-copy Apache Arrow data structures with sub-millisecond latency on hot paths.

### Why Tensor Machine?

| Problem | Solution |
|---------|----------|
| Python RAG frameworks suffer from GC pauses and memory copies | **Pure Rust** — no GC, compile-time safety, zero-copy Arrow pipeline |
| Adding new search strategies requires modifying production code | **Declarative DSL** — new strategies via macros, no core recompilation |
| High P99 latency from IPC and serialization overhead | **Single-process triad** — Control Plane, DSL, and VM in one Rust process |
| GPU acceleration requires complex CUDA setup | **WebGPU (wgpu)** — cross-platform GPU compute shaders (Vulkan/DX12/Metal) |

---

## Features

- **Hybrid Search** — semantic + BM25 retrieval with weighted fusion, MMR diversity, RRF
- **Tensor VM** — stack-based vector VM with 17+ opcodes, Arrow-native SIMD execution
- **Declarative DSL** — Lisp-style s-expressions for `search`, `multi-hop`, `self-rag`, `corrective-rag`
- **GPU Acceleration** — tiled WGSL compute shaders for ColBERT MaxSim late interaction
- **Zero-Copy Pipeline** — Apache Arrow + mmap shared memory + FlatBuffer serialization
- **MCP Protocol** — Model Context Protocol server for LLM agent integration (compatible with LM Studio, Claude, etc.)
- **AI Chat** — multi-provider chat (OpenAI, Gemini, OpenRouter, Ollama) with RAG context and agent mode
- **Knowledge Graph** — entity extraction, document neighborhood traversal, graph edges
- **Multi-Channel Ranking (RSF)** — combine dense, BM25, entity, structural, recency, and cluster signals
- **Token-Accurate Chunking** — semantic chunking via `chunkedrs` with `o200k_harmony` (GPT-4o tokenizer)
- **Arrow Flight** — distributed data plane for horizontal scaling across nodes
- **Observability** — Prometheus metrics, OpenTelemetry tracing, structured JSON logging, health check endpoints
- **Production Hardening** — circuit breakers, rate limiting, graceful shutdown, configurable instruction limits

---

## Quick Start

### Prerequisites

- Rust 2024 edition (MSRV 1.75+)
- Docker (for PostgreSQL, Qdrant, Ollama)

### 1. Start Infrastructure

```bash
docker compose up -d
```

This starts PostgreSQL 15, Qdrant, and Ollama (with `multilingual-e5-large` model auto-pulled).

### 2. Build & Run

```bash
cargo build --release -p tensor-rag
cargo run --release -p tensor-rag
```

The server starts on `0.0.0.0:8080` by default. Without databases configured, it runs in **mock mode** with synthetic data.

### 3. Send a Query

```bash
curl -X POST http://localhost:8080/mcp \
  -H "Content-Type: application/json" \
  -d '{
    "tool": "search",
    "args": {
      "dsl": "(search :query \"quantum physics\" :final-top-k 5)"
    },
    "session_id": "session_1",
    "sequence_number": 1
  }'
```

### 4. Connect via LM Studio

Configure the MCP bridge in LM Studio:

```json
{
  "mcpServers": {
    "tensor-rag": {
      "command": "node",
      "args": ["/path/to/APL-RAG/mcp-bridge.js"]
    }
  }
}
```

### Windows / Linux Launcher

```bash
# Windows
.\run.ps1

# Linux
./run.sh
```

---

## Architecture

```
LLM Agent (MCP Client)
        │ MCP (JSON/FlatBuffer)
        ▼
┌──────────────────────────────────────┐
│ Layer 1: Control Plane & Transport   │
│  MCP Server · Session Manager        │
│  DbPools (PG, Qdrant, ML)           │
│  Shared Memory Manager · Predicate   │
│  Pushdown                            │
└──────────────┬───────────────────────┘
               │ AstNode / QueryRequest
               ▼
┌──────────────────────────────────────┐
│ Layer 2: Declarative DSL             │
│  S-Expression Parser (lexpr)         │
│  search · multi-hop · self-rag       │
│  corrective-rag                      │
│  Planner::compile() → IR bytecode    │
└──────────────┬───────────────────────┘
               │ Program (bytecode)
               ▼
┌──────────────────────────────────────┐
│ Layer 3: Tensor Execution Engine     │
│  Stack-Based VM (17 opcodes)        │
│  Arrow-native · SIMD-friendly        │
│  GPU backend via wgpu                │
└──────────────┬───────────────────────┘
               │ TopKResult → FlatBuffer
               ▼
          McpResponse
```

### Key Modules

| Module | Purpose |
|--------|---------|
| `mcp/` | MCP server, session management, tool handlers (`search`, `read_document`, `list_documents`, `get_document_metadata`, `exec`, `help`) |
| `ir.rs` | IR types, `Planner::compile()`, `OpCode` enum, `Program` |
| `vm.rs` | Stack-based VM with all opcodes, dual-path (ndarray + Arrow) |
| `dsl.rs` | S-expression parser for all query forms and rerank strategies |
| `arrow.rs` | Arrow schema, RecordBatch construction, IPC shared-memory read/write |
| `db/` | Connection pools (PostgreSQL, Qdrant), circuit breakers, document/anchors/edges CRUD |
| `gpu.rs` | wgpu context, WGSL compute shaders (add, mul, fusion, colbert MaxSim tiled) |
| `flight.rs` | Arrow Flight gRPC service for distributed data exchange |
| `ai/` | Multi-provider chat, streaming, agent mode with MCP tool calling |
| `router.rs` | Axum HTTP router with load balancing and sticky sessions |
| `metrics.rs` | Prometheus metrics, OpenTelemetry tracing, health endpoints |
| `chunker.rs` | Token-accurate semantic chunking (paragraph, recursive, sentence) |
| `graph.rs` | Knowledge graph neighborhood traversal via recursive CTE |
| `entity.rs` | Named entity extraction (Russian + English, regex-based) |
| `rsf.rs` | Reciprocal Signal Fusion — multi-channel ranking |
| `normalizer.rs` | Text normalization, stemming (Russian + English), stopword removal |
| `clustering.rs` | K-means clustering with PCA fallback reducer |
| `shared_memory.rs` | mmap-backed regions for zero-copy IPC |
| `config.rs` | Environment variable configuration (50+ settings) |

---

## Configuration

Key environment variables (full list in `.env.example`):

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | — | PostgreSQL connection string |
| `QDRANT_URL` | — | Qdrant gRPC endpoint |
| `QDRANT_COLLECTION` | `default` | Qdrant collection name |
| `MCP_BIND_ADDR` | `0.0.0.0:8080` | HTTP router listen address |
| `TENSOR_VM_MODE` | `in_process` | VM execution mode (`in_process` / `ipc`) |
| `TENSOR_VM_BACKEND` | `simd` | Compute backend (`simd` / `gpu`) |
| `AI_PROVIDER` | `openai` | AI chat provider |
| `EMBEDDING_SERVER_URL` | — | OpenAI-compatible embeddings endpoint |
| `EMBEDDING_DIM` | `768` | Embedding vector dimension |
| `MCP_RATE_LIMIT` | `100` | Requests per second limit |
| `CB_FAILURE_THRESHOLD` | `5` | Circuit breaker failure threshold |

The system runs in **mock mode** when no databases are configured — no external infrastructure needed for testing.

---

## Infrastructure

```yaml
# docker-compose.yml
services:
  postgres:   # Document storage, BM25 scores, graph data
  qdrant:     # Vector storage, ANN search
  ollama:     # Local embeddings (multilingual-e5-large)
```

Included tools:
- `check_len.js` — Qdrant vector length diagnostics
- `mcp-bridge.js` — Node.js MCP bridge for LM Studio
- `postgres-init/init.sql` — Database schema initialization

---

## MCP Tools

| Tool | Description |
|------|-------------|
| `search` | Primary hybrid semantic/BM25 retrieval with DSL or JSON |
| `read_document` | Retrieve full document text by UUID |
| `list_documents` | List all indexed documents with filenames and UUIDs |
| `get_document_metadata` | Get metadata, anchors (headings), and graph edges |
| `exec` | Execute DSL commands for side-effects (add metadata, anchors, edges) |
| `help` | Comprehensive DSL documentation and examples |

---

## DSL Examples

```scheme
; Simple search
(search :query "quantum physics" :final-top-k 5)

; Hybrid search with filters
(search :query "machine learning"
        :filters (>= year 2023)
        :rerank (hybrid :weights (sem 0.7 bm25 0.3))
        :final-top-k 5)

; Multi-hop retrieval
(multi-hop :query "quantum error correction"
           :hops 3
           :rerank (hybrid :weights (sem 0.6 bm25 0.4))
           :final-top-k 5)

; Self-RAG with confidence threshold
(self-rag :query "latest AI research"
          :threshold 0.8
          :low-confidence (search :query "AI research survey" :final-top-k 10)
          :high-confidence (search :query "latest AI research" :final-top-k 5)
          :final-top-k 5)

; Corrective RAG with fallback
(corrective-rag :query "specific technical detail"
                :fallback-query "technical overview"
                :rerank (rrf)
                :final-top-k 5)
```

---

## Performance Targets

| Metric | Target | Current |
|--------|--------|---------|
| P99 latency (Top-1000, VM only) | < 5 ms | ~50 µs |
| Memory per 10k MCP sessions | < 500 MB | ~0.01 MB/session |
| Throughput | 50,000 req/s | — |
| New rerank strategy | < 1 hour | DSL macro |

---

## Project Structure

```
APL-RAG/
├── tensor-rag/              # Rust crate (core engine)
│   ├── src/
│   │   ├── main.rs          # Server entry point
│   │   ├── lib.rs           # Crate root, integration tests
│   │   ├── mcp/             # MCP server, executor, tools, IPC
│   │   ├── db/              # Database pools, CRUD, circuit breakers
│   │   ├── ai/              # Chat agent, streaming, context management
│   │   ├── vm.rs            # Stack-based Tensor VM
│   │   ├── ir.rs            # IR types, Planner, OpCodes
│   │   ├── dsl.rs           # S-expression parser
│   │   ├── arrow.rs         # Apache Arrow integration
│   │   ├── gpu.rs           # WebGPU compute shaders
│   │   ├── flight.rs        # Arrow Flight gRPC service
│   │   ├── router.rs        # Axum HTTP router
│   │   ├── metrics.rs       # Prometheus / OpenTelemetry
│   │   ├── graph.rs         # Knowledge graph
│   │   ├── entity.rs        # Named entity extraction
│   │   ├── chunker.rs       # Token-accurate chunking
│   │   ├── rsf.rs           # Reciprocal Signal Fusion
│   │   ├── normalizer.rs    # Text normalization & stemming
│   │   ├── clustering.rs    # K-means clustering
│   │   ├── config.rs        # Environment configuration
│   │   └── shared_memory.rs # mmap shared memory manager
│   └── benches/             # Criterion benchmarks
├── mcp-bridge.js            # Node.js MCP bridge (LM Studio)
├── docker-compose.yml       # PostgreSQL + Qdrant + Ollama
├── postgres-init/           # Database schema
├── docs/                    # Architecture, API, configuration guides
├── dev_docs/                # Development planning & roadmap
├── assets/                  # SVG banners
├── .brains/                 # Agent session tracking
├── .dataset/                # Fine-tuning dataset entries
├── run.ps1                  # Windows launcher
└── run.sh                   # Linux launcher
```

---

## Development

```bash
# Build
cargo build --release -p tensor-rag

# Test
cargo test -p tensor-rag

# Benchmarks
cargo bench --bench colbert   # ColBERT performance (CPU vs GPU)
cargo bench --bench stress    # High-concurrency throughput
cargo bench --bench zero_copy # Arrow vs Tensor1D comparison

# Lint
cargo clippy -p tensor-rag
cargo fmt -p tensor-rag
```

---

## Performance Benchmarks

```bash
# ColBERT late interaction: GPU naive vs GPU tiled vs CPU SIMD
cargo bench --bench colbert

# Stress test: 50k concurrent requests
cargo bench --bench stress

# Zero-copy comparison: Arrow vs ndarray
cargo bench --bench zero_copy
```

---

## Roadmap

- **Phase 1 (MVP)**: MCP server, JSON → IR, Tensor VM (5 opcodes), direct DB calls
- **Phase 2 (DSL)**: S-expression parser, hybrid/multi-hop/self-rag/corrective-rag macros
- **Phase 3 (Zero-Copy)**: Apache Arrow pipeline, FlatBuffer results, mmap regions
- **Phase 4 (Production)**: GPU backend (wgpu), ColBERT, horizontal scaling, observability
- **Phase 5 (Tech Debt)**: I/O separation, dynamic strategies, missing opcodes, latency hardening
- **Phase 6 (Hardening)**: Real MMR/RRF, Arrow Flight, ML embedding, ColBERT SIMD, DSL panics fix

All phases are structurally complete. See `dev_docs/` for detailed planning.

---

## License

MIT
