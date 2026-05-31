# Development Guide

## Project Structure

```
tensor-rag/
├── Cargo.toml              # Package manifest (edition 2024)
├── Cargo.lock              # Dependency lockfile
├── src/
│   ├── lib.rs              # Crate root, module declarations, integration tests
│   ├── mcp.rs              # MCP server, session management, SearchTool
│   ├── ir.rs               # IR types, Planner, OpCode enum, Program
│   ├── vm.rs               # Stack-based VM, all 17 opcode implementations
│   ├── dsl.rs              # S-expression parser, all query forms
│   ├── arrow.rs            # Arrow schema, RecordBatch construction, IPC
│   ├── db.rs               # Connection pools (PostgreSQL, Qdrant, ML servers)
│   ├── shared_memory.rs    # mmap-backed shared memory manager
│   ├── result.fbs          # FlatBuffer schema for search results
│   └── result_generated.rs # Auto-generated FlatBuffer Rust bindings
├── benches/
│   └── zero_copy.rs        # Criterion benchmarks
└── target/                 # Build artifacts
```

## Build Commands

```bash
# Build release binary
cargo build --release

# Build debug binary
cargo build

# Run all tests (unit + integration)
cargo test

# Run benchmarks (Criterion)
cargo bench --bench zero_copy

# Check for compilation errors without building
cargo check

# Run clippy for linting
cargo clippy

# Format code
cargo fmt
```

## Testing

The project includes three integration tests in `lib.rs`:

| Test | What it validates |
|------|-------------------|
| `test_e2e_pipeline` | Full DSL → Planner → VM → result correctness |
| `test_p99_latency` | Top-1000 processing completes in < 25ms |
| `test_memory_footprint` | 10k MCP sessions use < 500MB |

Run a specific test:

```bash
cargo test test_e2e_pipeline
```

### Benchmarks

Criterion benchmarks in `benches/zero_copy.rs` compare Arrow vs Tensor1D performance for:

- Element-wise addition
- Element-wise multiplication
- Weighted fusion
- TopK selection

```bash
cargo bench --bench zero_copy
```

## Module Responsibilities

| Module | Lines | Responsibility |
|--------|-------|----------------|
| `mcp.rs` | ~286 | MCP server, session management, `SearchTool`, async orchestrator |
| `ir.rs` | ~273 | Data model (`AstNode`, `QueryRequest`, `OpCode`, `Program`), `Planner::compile()` |
| `vm.rs` | ~640 | Stack-based VM, all 17 opcode implementations, dual-path (ndarray + Arrow) |
| `dsl.rs` | ~502 | S-expression parser, all query forms, rerank strategies |
| `arrow.rs` | ~231 | Arrow schema, RecordBatch converters, IPC shared-memory read/write |
| `db.rs` | ~303 | Connection pools for PostgreSQL, Qdrant, and ML servers |
| `shared_memory.rs` | ~157 | mmap-backed shared memory manager for zero-copy IPC |
| `lib.rs` | ~138 | Module declarations + integration tests |

## Code Conventions

- **No comments** in production code unless explicitly requested
- **Dual-path opcodes:** Every compute opcode handles both `Value::Tensor1D` and `Value::ArrowArray`
- **I/O separation:** I/O opcodes are rejected by the VM and handled by the async orchestrator in `mcp.rs`
- **Mock-first:** All database backends fall back to synthetic data when not configured
- **Error handling:** Use `Result<T>` types; propagate errors with `?`

## Architecture Patterns

### Planner/Executor Separation

The `Planner::compile()` step is deterministic and pure (AST in, bytecode out). The VM is a separate executor. This mirrors database query compilation.

### Session-Level Message Ordering

MCP sessions track `expected_sequence` and reject out-of-order messages, ensuring deterministic request processing per session.

### FlatBuffer Serialization

Results are serialized as compact FlatBuffers (~50 bytes) rather than JSON, minimizing data that crosses the process boundary.

## Adding a New Rerank Strategy

1. Add the strategy variant to `RerankStrategy` in `ir.rs`
2. Parse the DSL form in `dsl.rs`
3. Implement the VM opcode logic in `vm.rs` (if needed)
4. Add the strategy to the `CallDynamic` dispatch in `mcp.rs`

Target: < 1 hour for a new strategy via DSL macro.

## Development Workflow

This project uses a multi-agent development workflow:

- `.brains/sessions/` — Agent session tracking (plans, tasks, walkthroughs)
- `.dataset/lessons/` — Fine-tuning dataset entries from development insights
- `AGENTS.md` — Collaboration protocol for Jules + opencode agents
