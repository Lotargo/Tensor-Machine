<div align="center">

![Tensor Machine Hero Banner](./assets/hero-banner.svg)

# Tensor Machine: Programmable Low-Latency RAG Engine

### _Rust-powered hybrid retrieval with Tensor VM, GPU acceleration & MCP agent integration_

[![Rust Version](https://img.shields.io/badge/Rust-2024%2B-orange.svg?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![Build Status](https://img.shields.io/badge/Build-Passing-brightgreen.svg?style=for-the-badge&logo=github)](https://github.com/)
[![Vector Engine](https://img.shields.io/badge/Vector-Stack--VM%20%2F%20Arrow-blue.svg?style=for-the-badge)](https://github.com/)
[![License](https://img.shields.io/badge/License-GPLv3-blue.svg?style=for-the-badge)](https://www.gnu.org/licenses/gpl-3.0)

</div>

---

<table>
<tr>
<td>

## <img src="./assets/architecture.svg" width="24" height="24" /> 1. Abstract & Introduction

### Abstract

Modern **Retrieval-Augmented Generation (RAG)** systems have moved far beyond the simple `embed → top-k → prompt` pattern. Production retrieval pipelines often include hybrid search, reranking, filtering, graph relationships, caching, and agent-driven workflows. As these systems grow, retrieval logic can become a dense layer of connected services, heuristics, and application code where latency, repeatability, and strategy changes are difficult to control.

**Tensor Machine** is an experimental high-performance RAG engine written in Rust and designed as a vector-relational execution system: a declarative DSL, an intermediate representation, bytecode compilation, and an isolatable Tensor VM. The project is not positioned as a replacement for every existing RAG framework. Instead, it focuses on scenarios where retrieval must be programmable, measurable, and ready for agentic operation through MCP.

The architecture separates planning from execution, uses Apache Arrow and FlatBuffer for compact data representation, and supports several execution paths: CPU/SIMD, WebGPU, and optional VM isolation through IPC.

### Why Tensor Machine?

| Limitation in common RAG implementations                                                 | Tensor Machine approach                                                                             |
| ---------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| Search logic often lives in application code and grows with the product                  | **Declarative DSL → IR → bytecode → VM** for retrieval strategies without rewriting the core engine |
| Hybrid retrieval, reranking, and fallback behavior may become ad-hoc heuristic chains    | **Planned strategies**: `search`, `multi-hop`, `self-rag`, `corrective-rag`, ColBERT rerank         |
| Moving documents and vectors across components may add copies and serialization overhead | **Arrow-native pipeline** and compact FlatBuffer responses on the hot path                          |
| GPU acceleration for retrieval is often tied to CUDA or a specific platform              | **WebGPU (wgpu)**: Vulkan / DirectX 12 / Metal with CPU fallback                                    |
| Agent mode is often added as an external REST wrapper                                    | **MCP-first interface**: tool calling, sessions, streaming SSE, and an LM Studio bridge             |
| Simple length-based chunking is not enough for large technical corpora                   | **Token-aware structural chunking**: headings, parent links, anchors, metadata, and graph edges     |

### Design Goals

- **Programmability:** retrieval strategies are described declaratively and compiled into executable programs.
- **Low Latency:** unnecessary copies are minimized; hot paths are controlled; CPU/SIMD and GPU backends are selectable.
- **Agent Readiness:** MCP is part of the architecture rather than an external REST wrapper.
- **Hybrid Retrieval:** dense, lexical, entity, structural, recency, and cluster signals can participate in a unified fusion layer.
- **Operational Control:** circuit breakers, rate limiting, instruction limits, health checks, and readiness endpoints are built in.

### MCP Tooling Surface

| Tool / Variable                  | Description                                       |
| -------------------------------- | ------------------------------------------------- |
| `search`                         | Hybrid semantic/BM25 retrieval with DSL support   |
| `search_text`                    | Text-oriented search entry point                  |
| `read_document`                  | Retrieves a document by `doc_uuid`                |
| `list_documents`                 | Lists indexed documents                           |
| `get_document_metadata`          | Retrieves metadata, anchors, and graph edges      |
| `exec`                           | Executes DSL commands for controlled side effects |
| `help`                           | Returns DSL documentation and examples            |
| `MCP_SEARCH_DEFAULT_TOP_K`       | Default `top-k` for MCP search                    |
| `MCP_SEARCH_MAX_TOP_K`           | Upper bound for `top-k`                           |
| `MCP_SEARCH_MAX_RESPONSE_TOKENS` | Maximum token budget in search responses          |
| `MCP_READ_DOCUMENT_MAX_TOKENS`   | Maximum token budget when reading a document      |
| `TOKENIZER_MODEL`                | Tokenizer model, for example `o200k_harmony`      |
| `TENSOR_VM_SOCKET_PATH`          | IPC socket path for Tensor VM mode                |

### LM Studio MCP Example

To integrate the MCP bridge with **LM Studio**, configure the `mcpServers` object as follows:

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

This configuration tells LM Studio to launch the bridge via Node.js, allowing the editor to forward MCP requests to the Tensor Machine backend.

</td>
</tr>
</table>

---

<table>
<tr>
<td>

## <img src="./assets/architecture.svg" width="24" height="24" /> 2. System Architecture

Tensor Machine is organized as three connected layers: transport and control, declarative planning, and tensor execution. In local mode, these layers can run inside a single Rust process, reducing network overhead. For stricter isolation, the VM can be moved into a separate worker process through IPC.

```text
LLM Agent / MCP Client
        │ MCP (JSON / FlatBuffer)
        ▼
┌──────────────────────────────────────────────┐
│ Layer 1: Control Plane & Transport           │
│  MCP Server · Session Manager                │
│  DbPools (PostgreSQL, Qdrant, ML Servers)    │
│  Predicate Pushdown · Rate Limiting          │
│  Token-Accurate Chunking                     │
└──────────────────┬───────────────────────────┘
                   │ AstNode / QueryRequest
                   ▼
┌──────────────────────────────────────────────┐
│ Layer 2: Declarative DSL                     │
│  S-Expression Parser (lexpr)                 │
│  search · multi-hop · self-rag               │
│  corrective-rag · colbert-retrieval          │
│  Planner::compile() → IR bytecode            │
└──────────────────┬───────────────────────────┘
                   │ Program (Vec<OpCode>)
                   ▼
┌──────────────────────────────────────────────┐
│ Layer 3: Tensor Execution Engine             │
│  Stack-Based VM (17 opcodes)                 │
│  Arrow-native · SIMD-friendly                │
│  Tensor1D + ArrowArray execution paths       │
│  GPU backend via wgpu                        │
└──────────────────┬───────────────────────────┘
                   │ TopKResult → FlatBuffer
                   ▼
              McpResponse
```

### VM Execution Modes

- **In-Process Mode:** the VM runs inside the main MCP server process, minimizing latency for local workloads.
- **IPC Mode (`TENSOR_VM_MODE=ipc`):** the VM runs in a separate worker process and exchanges data through IPC / mmap.

### Layer 1: Control Plane & Transport

- **MCP Server** — entry point for LLM agents, long-lived sessions, and ordered messages.
- **Connection Pools** — PostgreSQL (`sqlx`), Qdrant (gRPC/tonic), ML servers, and embedding endpoints.
- **Predicate Pushdown** — filters are applied before expensive retrieval operations.
- **Rate Limiting & Sessions** — per-session governor, sequence numbers, and cancellation flow.
- **Token-Accurate Chunking** — chunking that respects tokenizer boundaries and document structure.

### Layer 2: Declarative DSL

- S-expressions powered by `lexpr`.
- Supported forms: `search`, `multi-hop`, `self-rag`, `corrective-rag`, `colbert-retrieval`.
- Compilation path: DSL → `AstNode` → `Planner::compile()` → `Program`.
- JSON can be used as a fallback format for clients without a DSL layer.

### Layer 3: Tensor Execution Engine

- Stack-based Tensor VM with 17 opcodes.
- Direct work with Apache Arrow arrays and custom `Tensor1D` structures.
- CPU/SIMD path for stable fallback execution.
- GPU path through `wgpu` and WGSL compute shaders.
- FlatBuffer serialization for compact top-k responses.

</td>
</tr>
</table>

---

<table>
<tr>
<td>

## <img src="./assets/tech_stack.svg" width="24" height="24" /> 3. Features & Capabilities

### Programmable Retrieval

Retrieval strategies do not have to be hardcoded into production code. They can be described through the DSL, compiled into bytecode, and executed by the VM:

```text
(scheme-form) → AstNode → Program (OpCode[]) → VM Execution
```

Supported retrieval patterns:

- `(search ...)` — hybrid search with filters, fusion, and reranking.
- `(multi-hop ...)` — iterative retrieval with query refinement.
- `(self-rag ...)` — confidence-threshold branching.
- `(corrective-rag ...)` — fallback search when relevant results are insufficient.
- `(colbert-retrieval ...)` — late-interaction reranking through ColBERT MaxSim.

### Hybrid Search & RSF

Tensor Machine supports several independent relevance signals that can be combined through RSF, RRF, or weighted fusion depending on the selected strategy.

| Signal     | What it captures              |
| ---------- | ----------------------------- |
| Dense      | Embedding similarity          |
| BM25       | Lexical term matching         |
| Entity     | Named entity overlap          |
| Structural | Heading and section alignment |
| Recency    | Time-decay relevance          |
| Cluster    | Semantic cluster cohesion     |

### GPU Acceleration

- WebGPU (`wgpu`) compute backend.
- WGSL shaders for tensor add, mul, matmul, cosine similarity, fusion, and top-K stages.
- **ColBERT MaxSim** with a tiled shader and cooperative workgroup memory.
- Backend selector: CPU/SIMD or GPU.
- Cross-platform compute through Vulkan, DirectX 12, and Metal.

### Zero-Copy Pipeline

```text
Qdrant (gRPC) → bytes::Bytes → Arrow RecordBatch → Arc<RecordBatch> → VM → FlatBuffer
```

- Apache Arrow for tabular representation of retrieval results.
- `Arc<RecordBatch>` for cheap data sharing between layers.
- mmap regions for IPC mode.
- FlatBuffer for compact top-k result serialization.

### AI Chat Integration

- Multi-provider setup: OpenAI, Gemini, OpenRouter, Ollama/local, and custom endpoints.
- **RAG mode:** answers are built from retrieved sources and citations.
- **Agent mode:** the LLM calls MCP tools, reads documents, refines search, and assembles an evidence chain.
- Streaming SSE, context management, and configurable system prompts.

### Knowledge Graph & Entity Extraction

- Named entity extraction for English and Russian text.
- Anchors: headings and named entry points into documents.
- Typed edges between documents, chunks, and entities.
- Recursive neighborhood traversal through PostgreSQL recursive CTE.

### Structural Chunking

- Hierarchical chunk levels: macro → medium → micro.
- `header_chain` for preserving section context.
- Parent links between chunks.
- `zone_keywords`, entities, and metadata at the chunk level.

### Observability & Hardening

- Prometheus metrics: latency, throughput, and backend states.
- OpenTelemetry tracing for the hot path and VM execution.
- Structured JSON logging.
- Circuit breakers for Qdrant, PostgreSQL, and ML servers.
- Graceful shutdown with `CancellationToken`.
- Configurable VM instruction limit (`VM_MAX_INSTRUCTIONS`).

</td>
</tr>
</table>

---

<table>
<tr>
<td>

## <img src="./assets/patterns.svg" width="24" height="24" /> 4. Network Scaling & Observability

### 4.1 Arrow Flight Data Plane

**Arrow Flight** over gRPC is used as a data plane for moving `RecordBatch` values between cluster nodes without unnecessary serialization. Supported operations include `do_get`, `do_put`, `do_exchange`, `get_flight_info`, and `get_schema`.

### 4.2 Axum MCP Router & Sticky Sessions

MCP requests are routed through Axum. For agent sessions, sticky routing by `session_id` can be used to preserve message ordering and execution state.

### 4.3 Metrics, Tracing & Health Checks

- Prometheus endpoint: `/metrics`.
- Health and readiness endpoints: `/health`, `/ready`.
- OpenTelemetry spans for planning, retrieval, VM execution, and serialization.
- Structured logs for ELK / Grafana Loki.

</td>
</tr>
</table>

---

<table>
<tr>
<td>

## <img src="./assets/agents.svg" width="24" height="24" /> 5. Repository Structure

```text
APL-RAG/
├── tensor-rag/                  # Rust crate: core engine
│   ├── Cargo.toml               # Package manifest, Rust 2024 edition
│   ├── src/
│   │   ├── main.rs              # Server entry point and wiring
│   │   ├── lib.rs               # Crate root and integration tests
│   │   ├── mcp/                 # MCP server, executor, tools, IPC
│   │   │   ├── server.rs        # Session management, run_server
│   │   │   ├── executor.rs      # Program execution orchestrator
│   │   │   ├── ipc.rs           # IPC mode support
│   │   │   ├── types.rs         # McpRequest, McpResponse, RateLimiter
│   │   │   └── tools/           # Tool handlers: search, read, list, metadata
│   │   ├── db/                  # Database pools, CRUD, circuit breakers
│   │   │   ├── pool.rs          # DbPools: PostgreSQL, Qdrant, ML servers
│   │   │   ├── search.rs        # Search and retrieval logic
│   │   │   ├── document.rs      # Document CRUD
│   │   │   ├── anchors.rs       # Anchors / headings management
│   │   │   ├── edges.rs         # Graph edges
│   │   │   ├── metadata.rs      # Document metadata
│   │   │   ├── circuit_breaker.rs
│   │   │   ├── helpers.rs       # DB helper utilities
│   │   │   └── types.rs         # StoredDocument, ChunkDetail, etc.
│   │   ├── ai/                  # Chat agent, streaming, context
│   │   │   ├── mod.rs           # run_chat, stream_chat, AiConfig
│   │   │   ├── agent.rs         # Agent mode with tool calling
│   │   │   └── context.rs       # Context building and overflow strategies
│   │   ├── vm.rs                # Stack-based Tensor VM
│   │   ├── ir.rs                # IR types, Planner, OpCode enum
│   │   ├── dsl.rs               # S-expression parser
│   │   ├── arrow.rs             # Arrow schema and RecordBatch converters
│   │   ├── gpu.rs               # WebGPU context and WGSL compute shaders
│   │   ├── flight.rs            # Arrow Flight gRPC service
│   │   ├── router.rs            # Axum HTTP router and load balancing
│   │   ├── metrics.rs           # Prometheus and health endpoints
│   │   ├── graph.rs             # Knowledge graph traversal
│   │   ├── entity.rs            # Named entity extraction
│   │   ├── chunker.rs           # Token-aware structural chunking
│   │   ├── rsf.rs               # Reciprocal Signal Fusion
│   │   ├── normalizer.rs        # Text normalization and stemming
│   │   ├── clustering.rs        # Semantic clustering
│   │   ├── config.rs            # Environment configuration
│   │   ├── shared_memory.rs     # mmap-backed shared memory
│   │   ├── response.rs          # Response formatting and deduplication
│   │   ├── result.fbs           # FlatBuffer schema
│   │   └── result_generated.rs  # Generated FlatBuffer bindings
│   └── benches/
│       ├── colbert.rs           # ColBERT CPU vs GPU benchmark
│       ├── stress.rs            # High-concurrency throughput
│       └── zero_copy.rs         # Arrow vs Tensor1D comparison
├── mcp-bridge.js                # Node.js MCP bridge for LM Studio
├── docker-compose.yml           # PostgreSQL + Qdrant + Ollama
├── postgres-init/               # Database schema initialization
├── docs/                        # Architecture, API, and configuration guides
├── dev_docs/                    # Development planning and roadmap
├── assets/                      # SVG banners, icons, and datasets
│   ├── hero-banner.svg
│   ├── architecture.svg
│   ├── tech_stack.svg
│   ├── patterns.svg
│   ├── agents.svg
│   ├── setup.svg
│   └── datasets/
├── run.ps1                      # Windows launcher
└── run.sh                       # Linux launcher
```

</td>
</tr>
</table>

---

<table>
<tr>
<td>

## <img src="./assets/setup.svg" width="24" height="24" /> 6. Build, Run & Configuration

### Environment Requirements

- Rust 2024 edition.
- C compiler for building selected dependencies.
- Docker / Docker Compose for PostgreSQL, Qdrant, and Ollama.
- WebGPU-compatible GPU for GPU mode: Vulkan, DirectX 12, or Metal.
- Node.js for `mcp-bridge.js` when integrating with LM Studio.

### Quick Start

```bash
# 1. Start infrastructure
docker compose up -d

# 2. Build the project
cargo build --release -p tensor-rag

# 3. Run the server
cargo run --release -p tensor-rag
```

The server starts on `0.0.0.0:8080`. Without configured external databases, the engine can run in mock mode with synthetic data.

### First MCP Request

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

### Launcher Scripts

```bash
# Windows
.\run.ps1

# Linux
./run.sh
```

### Testing

```bash
cargo test -p tensor-rag
```

### Benchmarks

```bash
cargo bench --bench colbert    # ColBERT CPU vs GPU
cargo bench --bench stress     # High-concurrency throughput
cargo bench --bench zero_copy  # Arrow vs Tensor1D
```

### Environment Variables

| Variable                | Default                                              | Description                           |
| ----------------------- | ---------------------------------------------------- | ------------------------------------- |
| `DATABASE_URL`          | —                                                    | PostgreSQL connection string          |
| `QDRANT_URL`            | —                                                    | Qdrant gRPC endpoint                  |
| `QDRANT_COLLECTION`     | `default`                                            | Qdrant collection name                |
| `MCP_BIND_ADDR`         | `0.0.0.0:8080`                                       | HTTP router listen address            |
| `TENSOR_VM_MODE`        | `in_process`                                         | VM mode: `in_process` / `ipc`         |
| `TENSOR_VM_BACKEND`     | `simd`                                               | Compute backend: `simd` / `gpu`       |
| `AI_PROVIDER`           | `openai`                                             | AI chat provider                      |
| `AI_MODEL`              | `gpt-4.1-mini`                                       | LLM model                             |
| `EMBEDDING_SERVER_URL`  | —                                                    | OpenAI-compatible embeddings endpoint |
| `EMBEDDING_MODEL_NAME`  | `jeffh/intfloat-multilingual-e5-large-instruct:q8_0` | Embedding model                       |
| `EMBEDDING_DIM`         | `1024`                                               | Embedding vector dimension            |
| `MCP_RATE_LIMIT`        | `100`                                                | Requests per second                   |
| `CB_FAILURE_THRESHOLD`  | `5`                                                  | Circuit breaker threshold             |
| `VM_MAX_INSTRUCTIONS`   | `100000`                                             | Max VM instruction count              |
| `RSF_DENSE_WEIGHT`      | `1.0`                                                | Dense signal weight                   |
| `RSF_BM25_WEIGHT`       | `0.8`                                                | BM25 signal weight                    |
| `RSF_ENTITY_WEIGHT`     | `0.5`                                                | Entity signal weight                  |
| `RSF_STRUCTURAL_WEIGHT` | `0.3`                                                | Structural signal weight              |
| `RSF_RECENCY_WEIGHT`    | `0.2`                                                | Recency signal weight                 |
| `RSF_CLUSTER_WEIGHT`    | `0.3`                                                | Cluster signal weight                 |

The full configuration list is available in `.env.example` and `docs/configuration.md`.

### DSL Examples

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

; Self-RAG with confidence branching
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

; ColBERT late-interaction rerank
(colbert-retrieval :query "semantic search architecture"
                   :candidate-top-k 100
                   :final-top-k 10)
```

</td>
</tr>
</table>

---

<table>
<tr>
<td>

## License

**GPLv3** — see [LICENSE](./LICENSE) for details.

</td>
</tr>
</table>
