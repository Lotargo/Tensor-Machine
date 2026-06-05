<div align="center">

![Custom RAG Hero Banner](./assets/hero-banner.svg)

# Tensor Machine: Low-Latency RAG Engine
### *Rust-powered hybrid semantic search with Tensor VM, GPU acceleration & LLM agent integration*

[![Rust Version](https://img.shields.io/badge/Rust-2024%2B-orange.svg?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![Build Status](https://img.shields.io/badge/Build-Passing-brightgreen.svg?style=for-the-badge&logo=github)](https://github.com/)
[![Vector Engine](https://img.shields.io/badge/Vector-Stack--VM%20%2F%20Arrow-blue.svg?style=for-the-badge)](https://github.com/)
[![License](https://img.shields.io/badge/License-MIT-green.svg?style=for-the-badge)](https://opensource.org/licenses/MIT)

</div>

---

<table>
<tr>
<td>

## <img src="./assets/architecture.svg" width="24" height="24" /> 1. Аннотация и Введение (Abstract & Introduction)

### Аннотация
Современные архитектуры генерации ответов с дополнением знаниями (**Retrieval-Augmented Generation, RAG**) сталкиваются с барьером задержек (Latency) при передаче векторов и документов между процессами. Настоящий проект представляет **Tensor Machine** — высокопроизводительный распределенный RAG-движок на языке Rust, спроектированный как векторная реляционная СУБД с компилятором запросов и изолированной виртуальной машиной.

Архитектура Tensor Machine разделяет фазы планирования и выполнения, оперируя структурами данных Apache Arrow и обеспечивая субмиллисекундные задержки на горячих путях инференса и поиска.

### Why Tensor Machine?

| Проблема | Решение |
|----------|---------|
| Python RAG frameworks — GC pauses, memory copies | **Pure Rust** — no GC, compile-time safety, zero-copy Arrow |
| New search strategies require production code changes | **Declarative DSL** — macros, no core recompilation |
| High P99 latency from IPC and serialization | **Single-process triad** — Control Plane, DSL, VM in one process |
| GPU acceleration requires complex CUDA setup | **WebGPU (wgpu)** — cross-platform compute (Vulkan/DX12/Metal) |

### MCP Tooling Surface

| Variable | Description |
|---|---|
| `MCP_SEARCH_DEFAULT_TOP_K` | Default `top‑k` for MCP search |
| `MCP_SEARCH_MAX_TOP_K` | Upper bound for `top‑k`; clamped to default |
| `MCP_SEARCH_MAX_RESPONSE_TOKENS` | Maximum tokens in search response |
| `MCP_READ_DOCUMENT_MAX_TOKENS` | Maximum tokens when reading a document |
| `TOKENIZER_MODEL` | Tokenizer model (`o200k_harmony`) |
| `TENSOR_VM_SOCKET_PATH` | IPC socket path for the Tensor VM |
| `search` | Dynamic alpha‑blended hybrid semantic/BM25 retrieval |
| `read_document` | Retrieves a document by `doc_uuid` |
| `list_documents` | Lists all indexed documents |
| `get_document_metadata` | Retrieves metadata, anchors, and graph edges |
| `exec` | Executes DSL commands for side-effects |
| `help` | DSL documentation and examples |

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

## <img src="./assets/architecture.svg" width="24" height="24" /> 2. Трехуровневая архитектура (System Architecture)

Система организована в виде трех слоев, выполняющихся в рамках единого Rust-процесса (или изолированных процессов через IPC), что полностью устраняет накладные расходы на сетевой транспорт при локальной работе.

```
LLM Agent (MCP Client)
        │ MCP (JSON/FlatBuffer)
        ▼
┌──────────────────────────────────────────────┐
│ Layer 1: Control Plane & Transport           │
│  MCP Server · Session Manager                │
│  DbPools (PostgreSQL, Qdrant, ML Servers)   │
│  Shared Memory Manager · Predicate Pushdown  │
│  Token-Accurate Chunking (o200k_harmony)     │
└──────────────────┬───────────────────────────┘
                   │ AstNode / QueryRequest
                   ▼
┌──────────────────────────────────────────────┐
│ Layer 2: Declarative DSL                     │
│  S-Expression Parser (lexpr)                 │
│  search · multi-hop · self-rag               │
│  corrective-rag                              │
│  Planner::compile() → IR bytecode            │
└──────────────────┬───────────────────────────┘
                   │ Program (bytecode)
                   ▼
┌──────────────────────────────────────────────┐
│ Layer 3: Tensor Execution Engine             │
│  Stack-Based VM (17 opcodes)                │
│  Arrow-native · SIMD-friendly               │
│  Dual path: Tensor1D + ArrowArray           │
│  GPU backend via wgpu                        │
└──────────────────┬───────────────────────────┘
                   │ TopKResult → FlatBuffer
                   ▼
              McpResponse
```

### Режимы выполнения VM
- **In-Process Mode:** VM выполняется непосредственно в основном процессе MCP-сервера, минимизируя время отклика.
- **IPC Mode (`TENSOR_VM_MODE=ipc`):** VM запускается в изолированном рабочем процессе через mmap.

### Layer 1: Control Plane & Transport
- **MCP Server** — точка входа для LLM-агентов, долгоживущие сессии, гарантия порядка сообщений
- **Connection Pools** — PostgreSQL (`sqlx`), Qdrant (gRPC/tonic), ML-серверы (gRPC)
- **Predicate Pushdown** — фильтры применяются до векторного поиска
- **Shared Memory Manager** — mmap-регионы для zero-copy обмена
- **Token-Accurate Chunking** — семантический чанкер на базе `chunkedrs` + `o200k_harmony`

### Layer 2: Declarative DSL
- S-выражения на базе `lexpr`
- Формы: `search`, `multi-hop`, `self-rag`, `corrective-rag`
- Компиляция: DSL → `AstNode` → `Planner::compile()` → `Program` (bytecode)
- JSON-формат как fallback

### Layer 3: Tensor Execution Engine
- Стековая VM с 17 опкодами
- Прямая работа с Apache Arrow массивами
- Dual-path opcodes: `Tensor1D` (ndarray) + `ArrowArray` (zero-copy)
- FlatBuffer сериализация результатов (~50 байт)

</td>
</tr>
</table>

---

<table>
<tr>
<td>

## <img src="./assets/tech_stack.svg" width="24" height="24" /> 3. Features & Capabilities

### Hybrid Search
- Semantic (dot product) + BM25 lexical search
- Weighted fusion, MMR diversity, RRF ranking
- RSF (Reciprocal Signal Fusion) — multi-channel: dense, BM25, entity, structural, recency, cluster

### GPU Acceleration
- WebGPU (wgpu) compute shaders for add, mul, fusion
- **ColBERT MaxSim** — tiled WGSL shader with cooperative workgroup memory
- Zero-padding offsets for variable-length documents
- Backend selector: CPU (SIMD) vs GPU

### Zero-Copy Pipeline
```
Qdrant (gRPC) → bytes::Bytes → Arrow RecordBatch → Arc<RecordBatch> → VM → FlatBuffer
```
- Apache Arrow для zero-copy представления данных
- mmap-регионы для внутрипроцессного обмена
- FlatBuffer для компактной сериализации результатов

### AI Chat Integration
- Multi-provider: OpenAI, Gemini, OpenRouter, Ollama (local), custom
- RAG mode with source citation, Agent mode with MCP tool calling
- Streaming SSE, context management, configurable system prompts

### Knowledge Graph
- Named entity extraction (Russian + English, regex-based)
- Document neighborhood traversal via PostgreSQL recursive CTE
- Anchors (headings), metadata, typed edges between documents

### Observability
- Prometheus metrics (latency, throughput, circuit breaker states)
- OpenTelemetry tracing (tracing Spans)
- Structured JSON logging, health/readiness endpoints

### Production Hardening
- Circuit breakers for Qdrant, PostgreSQL, ML servers
- Rate limiting (per-session Governor)
- Graceful shutdown with CancellationToken
- Configurable VM instruction limit (default 100k)

</td>
</tr>
</table>

---

<table>
<tr>
<td>

## <img src="./assets/patterns.svg" width="24" height="24" /> 4. Сетевое масштабирование и Наблюдаемость

### 4.1 Arrow Flight Data Plane
Протокол **Arrow Flight** поверх gRPC для распределенного обмена RecordBatch между узлами кластера без сериализации. Включает `do_get`, `do_put`, `do_exchange`, `get_flight_info`, `get_schema`.

### 4.2 Axum MCP Router & Sticky Sessions
Маршрутизация MCP-запросов через Axum с консистентным хешированием по ID сессии.

### 4.3 Метрики и Трассировка
- Prometheus `/metrics`, health `/health` и `/ready`
- OpenTelemetry трассировка горячего цикла VM
- Структурированные JSON-логи для ELK/Grafana Loki

</td>
</tr>
</table>

---

<table>
<tr>
<td>

## <img src="./assets/agents.svg" width="24" height="24" /> 5. Структура Репозитория (Repository Structure)

```
APL-RAG/
├── tensor-rag/                  # Rust crate (core engine)
│   ├── Cargo.toml               # Package manifest (edition 2024)
│   ├── src/
│   │   ├── main.rs              # Server entry point, wiring
│   │   ├── lib.rs               # Crate root, integration tests
│   │   ├── mcp/                 # MCP server, executor, tools, IPC
│   │   │   ├── server.rs        # Session management, run_server
│   │   │   ├── executor.rs      # Program execution orchestrator
│   │   │   ├── ipc.rs           # IPC mode support
│   │   │   ├── types.rs         # McpRequest, McpResponse, RateLimiter
│   │   │   └── tools/           # Tool handlers (search, read, list, etc.)
│   │   ├── db/                  # Database pools, CRUD, circuit breakers
│   │   │   ├── pool.rs          # DbPools (PostgreSQL, Qdrant, ML)
│   │   │   ├── search.rs        # Search and retrieval logic
│   │   │   ├── document.rs      # Document CRUD
│   │   │   ├── anchors.rs       # Anchors (headings) management
│   │   │   ├── edges.rs         # Graph edges
│   │   │   ├── metadata.rs      # Document metadata
│   │   │   ├── circuit_breaker.rs
│   │   │   ├── helpers.rs       # DB helper utilities
│   │   │   └── types.rs         # StoredDocument, ChunkDetail, etc.
│   │   ├── ai/                  # Chat agent, streaming, context
│   │   │   ├── mod.rs           # run_chat, stream_chat, AiConfig
│   │   │   ├── agent.rs         # Agent mode with tool calling
│   │   │   └── context.rs       # Context building, overflow strategies
│   │   ├── vm.rs                # Stack-based Tensor VM (17 opcodes)
│   │   ├── ir.rs                # IR types, Planner, OpCode enum
│   │   ├── dsl.rs               # S-expression parser (lexpr)
│   │   ├── arrow.rs             # Arrow schema, RecordBatch converters
│   │   ├── gpu.rs               # WebGPU context, WGSL compute shaders
│   │   ├── flight.rs            # Arrow Flight gRPC service
│   │   ├── router.rs            # Axum HTTP router, load balancing
│   │   ├── metrics.rs           # Prometheus, health endpoints
│   │   ├── graph.rs             # Knowledge graph neighborhood
│   │   ├── entity.rs            # Named entity extraction
│   │   ├── chunker.rs           # Token-accurate chunking
│   │   ├── rsf.rs               # Reciprocal Signal Fusion
│   │   ├── normalizer.rs        # Text normalization, stemming
│   │   ├── clustering.rs        # K-means clustering
│   │   ├── config.rs            # Environment configuration (50+ vars)
│   │   ├── shared_memory.rs     # mmap-backed shared memory
│   │   ├── response.rs          # Response formatting, dedup
│   │   ├── result.fbs           # FlatBuffer schema
│   │   └── result_generated.rs  # Auto-generated FlatBuffer bindings
│   └── benches/
│       ├── colbert.rs           # ColBERT CPU vs GPU benchmark
│       ├── stress.rs            # High-concurrency throughput
│       └── zero_copy.rs         # Arrow vs Tensor1D comparison
├── mcp-bridge.js                # Node.js MCP bridge (LM Studio)
├── docker-compose.yml           # PostgreSQL + Qdrant + Ollama
├── postgres-init/               # Database schema initialization
├── docs/                        # Architecture, API, config guides
├── dev_docs/                    # Development planning & roadmap
├── assets/                      # SVG banners and icons
│   ├── hero-banner.svg
│   ├── architecture.svg
│   ├── tech_stack.svg
│   ├── patterns.svg
│   ├── agents.svg
│   ├── setup.svg
│   └── datasets/
├── .brains/                     # Agent session tracking
├── .dataset/                    # Fine-tuning dataset entries
├── check_len.js                 # Qdrant vector length diagnostics
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

## <img src="./assets/setup.svg" width="24" height="24" /> 6. Сборка, Запуск и Конфигурация

### Требования к окружению
- Rust 2024 edition (MSRV 1.75+)
- Компилятор C (для сборки библиотек-зависимостей)
- Docker (для PostgreSQL, Qdrant, Ollama)
- Совместимый графический ускоритель (Vulkan/DirectX12/Metal) для GPU-режима

### Быстрый старт

```bash
# 1. Запуск инфраструктуры
docker compose up -d

# 2. Сборка проекта
cargo build --release -p tensor-rag

# 3. Запуск сервера
cargo run --release -p tensor-rag
```

Сервер запускается на `0.0.0.0:8080`. Без настроенных БД работает в **mock mode** с синтетическими данными.

### Первый запрос

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

### Launcher-скрипты

```bash
# Windows
.\run.ps1

# Linux
./run.sh
```

### Тестирование

```bash
cargo test -p tensor-rag
```

### Бенчмарки

```bash
cargo bench --bench colbert    # ColBERT CPU vs GPU
cargo bench --bench stress     # High-concurrency throughput
cargo bench --bench zero_copy  # Arrow vs Tensor1D
```

### Переменные окружения

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | — | PostgreSQL connection string |
| `QDRANT_URL` | — | Qdrant gRPC endpoint |
| `QDRANT_COLLECTION` | `default` | Qdrant collection name |
| `MCP_BIND_ADDR` | `0.0.0.0:8080` | HTTP router listen address |
| `TENSOR_VM_MODE` | `in_process` | VM mode (`in_process` / `ipc`) |
| `TENSOR_VM_BACKEND` | `simd` | Compute backend (`simd` / `gpu`) |
| `AI_PROVIDER` | `openai` | AI chat provider |
| `AI_MODEL` | `gpt-4.1-mini` | LLM model |
| `EMBEDDING_SERVER_URL` | — | OpenAI-compatible embeddings endpoint |
| `EMBEDDING_MODEL_NAME` | `nomic-embed-text` | Embedding model |
| `EMBEDDING_DIM` | `768` | Embedding vector dimension |
| `MCP_RATE_LIMIT` | `100` | Requests per second |
| `CB_FAILURE_THRESHOLD` | `5` | Circuit breaker threshold |
| `VM_MAX_INSTRUCTIONS` | `100000` | Max VM instruction count |

Полный список — в `.env.example`.

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
```

### Performance Targets

| Metric | Target | Current |
|--------|--------|---------|
| P99 latency (Top-1000, VM only) | < 5 ms | ~50 µs |
| Memory per 10k MCP sessions | < 500 MB | ~0.01 MB/session |
| Throughput | 50,000 req/s | — |
| New rerank strategy | < 1 hour | DSL macro |

</td>
</tr>
</table>

---

<table>
<tr>
<td>

## <img src="./assets/agents.svg" width="24" height="24" /> 7. Roadmap

- **Phase 1 (MVP)**: MCP server, JSON → IR, Tensor VM (5 opcodes), direct DB calls
- **Phase 2 (DSL)**: S-expression parser, hybrid/multi-hop/self-rag/corrective-rag macros
- **Phase 3 (Zero-Copy)**: Apache Arrow pipeline, FlatBuffer results, mmap regions
- **Phase 4 (Production)**: GPU backend (wgpu), ColBERT, horizontal scaling, observability
- **Phase 5 (Tech Debt)**: I/O separation, dynamic strategies, missing opcodes, latency hardening
- **Phase 6 (Hardening)**: Real MMR/RRF, Arrow Flight, ML embedding, ColBERT SIMD, DSL panics fix

All phases structurally complete. See `dev_docs/` for detailed planning.

---

**License:** MIT

</td>
</tr>
</table>
