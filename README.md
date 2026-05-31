# tensor-rag

A next-generation RAG (Retrieval-Augmented Generation) system built entirely in Rust. Tensor-rag treats semantic search as a vector relational DBMS with a query compiler, separating planning from execution for predictable latency and memory safety.

---

## Architecture

Tensor-rag is organized into three layers that run in a single Rust process, eliminating IPC overhead and multi-runtime coordination.

1. **Control Plane & Transport** — MCP server, connection pools (Qdrant, PostgreSQL), shared memory manager, predicate pushdown
2. **Declarative DSL** — Lisp-style s-expressions compiled into linear IR bytecode via `lexpr`
3. **Tensor Execution Engine** — Stack-based vector VM with 17 opcodes operating on Apache Arrow arrays

> See [Architecture](docs/architecture.md) for the full system design, data flow diagrams, and design decisions.

---

## Getting Started

```bash
git clone <repository>
cd tensor-rag
cargo build --release
cargo test
cargo run --release
```

No external databases required — the system runs in mock mode by default.

> See [Getting Started](docs/getting_started.md) for the full setup guide, first query walkthrough, and testing instructions.

---

## Configuration

Environment variables: `DATABASE_URL`, `QDRANT_URL`, `QDRANT_COLLECTION`, `MCP_BIND_ADDR`, `MAX_CONNECTIONS`.

> See [Configuration](docs/configuration.md) for full reference, database setup SQL, and programmatic configuration.

---

## Project Structure

```
tensor-rag/src/
├── lib.rs              — Crate root, module declarations, integration tests
├── mcp.rs              — MCP server, session management, SearchTool
├── ir.rs               — IR types, Planner, OpCode enum, Program
├── vm.rs               — Stack VM, all opcode implementations
├── dsl.rs              — S-expression parser, all query forms
├── arrow.rs            — Arrow schema, RecordBatch construction, score extraction
├── db.rs               — Connection pools (PostgreSQL, Qdrant, ML servers)
├── shared_memory.rs    — mmap-backed shared memory manager
├── result.fbs          — FlatBuffer schema for search results
└── result_generated.rs — FlatBuffer generated Rust bindings
```

> See [Development Guide](docs/development.md) for build commands, testing, code conventions, and contributing.

## Documentation

| Document | Description |
|----------|-------------|
| [Architecture](docs/architecture.md) | Three-layer design, data flow, VM opcodes, design decisions |
| [Getting Started](docs/getting_started.md) | Setup, first query, testing |
| [API Reference](docs/api.md) | MCP protocol, DSL syntax, VM opcodes, Rust API |
| [Configuration](docs/configuration.md) | Environment variables, database setup, programmatic config |
| [Development Guide](docs/development.md) | Build commands, testing, code conventions |
| [Architecture Manifesto](docs/APL_RAG_ARCHITECTURE.md) | Original architecture document (Russian) |
| [Project Summary](docs/project_summary.md) | English summary of architecture and implementation status |

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
