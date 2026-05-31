# Getting Started

This guide walks you through building, running, and sending your first query to tensor-rag.

## Prerequisites

- **Rust 2024 edition** (nightly or stable with `edition = "2024"` support)
- **Cargo** (ships with Rust)
- No external databases required — the system runs in mock mode by default

## Building

```bash
git clone <repository>
cd tensor-rag

# Build the release binary
cargo build --release

# Run all tests (unit + integration)
cargo test
```

## Running the MCP Server

The MCP server is the primary entry point. It accepts tool calls from LLM agents via the Model Context Protocol.

```bash
# Run with default mock I/O (no external databases required)
cargo run --release

# Or run the binary directly after build
./target/release/tensor-rag
```

The server starts on `0.0.0.0:8080` by default. When running with mock I/O, it returns synthetic results for all queries — suitable for testing the full Planner → VM pipeline without external infrastructure.

## Sending Your First Query

### Option 1: JSON Request

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

### Option 2: DSL Request

Use the Lisp-style DSL directly:

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

### Using curl

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

## Connecting from an LLM Agent

The MCP server implements the [Model Context Protocol](https://spec.modelcontextprotocol.io), so any MCP-compatible client can connect. The server:

1. Accepts `McpRequest` payloads with a `tool`, `args`, `session_id`, and `sequence_number`
2. Processes the query through the Planner → VM pipeline
3. Returns an `McpResponse` with the result serialized as a FlatBuffer

### Session Management

- Each session is identified by `session_id`
- Messages within a session must be sent in order (`sequence_number` increments sequentially)
- Out-of-order messages are rejected

## Running Tests

```bash
# Run all tests (unit + integration)
cargo test

# Run benchmarks (Criterion)
cargo bench --bench zero_copy
```

The test suite includes:

- **E2E pipeline test** — Validates the full DSL → Planner → VM → result flow
- **P99 latency benchmark** — Ensures Top-1000 processing stays under 25ms
- **Memory footprint test** — Validates 10k sessions stay under 500MB

## Next Steps

- [Architecture](architecture.md) — Understand the three-layer design
- [API Reference](api.md) — Full DSL syntax, VM opcodes, and MCP protocol
- [Configuration](configuration.md) — Connect to real PostgreSQL and Qdrant instances
- [Development Guide](development.md) — Project conventions and contributing
