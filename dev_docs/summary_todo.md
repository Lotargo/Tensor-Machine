# Summary of Remaining TODO Items Investigation

This document summarizes the findings of three independent agents investigating the remaining tasks in `dev_docs/todo.md`.

## 1. Extract VM into Separate Process (Task 3.4.3)
**Status:** Deferred / Optional.
- **Infrastructure:** Mostly in place. The `SharedMemoryManager` (`shared_memory.rs`) and Arrow IPC (`arrow.rs`) are ready. The VM and orchestrator are cleanly separated (`mcp.rs`).
- **Risks:** The architecture specifies a single-process system to avoid IPC overhead (`architecture.md`). Introducing IPC latency could jeopardize the tight <5ms P99 target (each query requires multiple ops, and IPC could add 1–10µs per op). Furthermore, GPU contexts would need duplication, and recursive AST/Program types lack `serde` support currently.
- **Recommendation:** Keep in-process for now unless a measured bottleneck demands extraction.

## 2. Colbert_maxsim via SIMD/GPU (Task 4.2.3)
**Status:** Incomplete; currently missing SIMD path.
- **Infrastructure:** The `colbert_maxsim` logic exists as a naive CPU implementation (triple loop) and a GPU implementation (via `wgpu` shaders).
- **Risks:** The CPU implementation is extremely slow O(Q·D·dim). The GPU implementation suffers from padding waste (all docs padded to max length) and suboptimal parallelism (1 thread per doc). Also, query token count is hardcoded to 10.
- **Recommendation:** Before attempting further GPU optimizations, an explicit SIMD path should be added using `ndarray` matrix operations (matmul query_tokens @ doc_tokens.T, then max over dim).

## 3. Horizontal Scaling (Tasks 4.3.1 - 4.3.4)
**Status:** Not started.
- **Infrastructure:** The codebase is entirely single-node, single-threaded (`mcp.rs`, `main.rs`). `MCP_BIND_ADDR` is documented but unimplemented (everything is in-process `mpsc`).
- **Risks:**
  - **No Network Transport:** Need an HTTP/gRPC/WebSocket server.
  - **Session State:** Stored locally in a `HashMap`. Cannot survive worker restarts or redirection.
  - **Single-threaded Execution:** `run_server` processes requests serially.
  - **Routing/Discovery:** No worker IDs in `FlatBuffer` responses, no service discovery, no distributed rate limiting.
- **Positive Foundations:** `DbPools` uses named backends; `Planner::compile()` generates pure, serializable bytecode; zero-copy shared memory exists.
- **Recommendation:** Significant architectural work is required to implement network listeners, a distributed session store (or sticky routing), and an Arrow Flight plane.

## 4. Monitoring and Observability (Tasks 4.4.1 - 4.4.4)
**Status:** Not started.
- **Infrastructure:** Code currently uses `println!` instead of structured logging. Circuit breakers exist in `db.rs` but are not exposed as metrics.
- **Risks:**
  - Instrumentation overhead could impact the 5ms P99 latency target (especially in the hot VM loop).
  - High cardinality metrics (e.g., per session) could cause memory blowups.
  - GPU timestamp queries require enabling `wgpu::Features::TIMESTAMP_QUERY`.
- **Recommendation:**
  1. Replace `println!` with `tracing` and structured JSON logs.
  2. Implement a `/health` and `/metrics` endpoint using an HTTP server (e.g., `axum`) spawned on the main `tokio` runtime.
  3. Integrate `prometheus` for histograms/gauges and `opentelemetry` for tracing the MCP request lifecycle.
  4. Expose circuit breaker states and per-tool usage as metrics.
