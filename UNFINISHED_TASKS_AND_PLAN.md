# Unfinished Tasks and Plan

This document provides a compiled list of unfinished tasks and a detailed execution plan to address them.

## Current State: All Unfinished Tasks

From `dev_docs/todo.md` and codebase analysis, here is the state of open tasks:

| Task | Status | Summary |
|------|--------|---------|
| **3.4.3** (Opt.) — VM in separate process with mmap | 🟡 Deferred | Infrastructure exists (shared_memory.rs, Arrow IPC), but `Program`/`OpCode` lack serde; no subprocess spawn. Kept in-process for latency. |
| **4.2.3** — colbert_maxsim via SIMD/GPU | 🟡 Partial | CPU path already uses ndarray (gets SIMD). GPU path is a naive triple-loop WGSL shader (no tiling, no shared mem). Needs GPU optimization. |
| **4.3.1** — Arrow Flight plane | 🔴 Not started | No arrow-flight dep, no network transport at all. |
| **4.3.2** — Load balancer | 🔴 Not started | Single in-process mpsc channel only. |
| **4.3.3** — Sticky sessions / distributed cache | 🔴 Not started | Sessions in local HashMap only. |
| **4.3.4** — Stress test 50k req/s | 🔴 Not started | No test exists. |
| **4.4.1** — Prometheus metrics | 🔴 Not started | Uses `println!` everywhere, no metric crates. |
| **4.4.2** — OpenTelemetry tracing | 🔴 Not started | No tracing/opentelemetry deps. |
| **4.4.3** — Structured JSON logging | 🔴 Not started | Only `println!`. |
| **4.4.4** — Health/readiness endpoint | 🔴 Not started | No HTTP server at all. |

---

## Execution Plan

### Phase A: Observability (Tasks 4.4.1–4.4.4)
**Files:** `Cargo.toml`, `main.rs`, `mcp.rs`, new `metrics.rs`
**Goal:** Replace `println!` with structured tracing, expose Prometheus metrics and health endpoint
- A1 — Add `tracing`, `tracing-subscriber`, `prometheus`, `axum`, `tokio` deps to Cargo.toml.
- A2 — Replace all `println!` with `tracing::info!`/`error!`/`warn!` (structured fields).
- A3 — Add `axum` HTTP server on a separate port serving `/metrics` (Prometheus) and `/health`/`/ready` endpoints.
- A4 — Instrument the VM hot loop and MCP request lifecycle with latency histograms and counters.
- A5 — Add OpenTelemetry tracing spans for request lifecycle (optional / configurable).

### Phase B: GPU Optimization — ColBERT MaxSIM (Task 4.2.3)
**Files:** `gpu.rs`, `vm.rs`
**Goal:** Optimize the GPU `colbert_maxsim` compute shader with shared memory tiling
- B1 — Rewrite the WGSL shader with workgroup-shared memory tiling (load chunks of query/doc tokens into `var<workgroup>` memory).
- B2 — Parallelize over workgroup lanes: each invocation handles one (query_token, doc_token) pair cooperatively.
- B3 — Ensure host-side padding/batching is minimal (current padding to `max_tokens_per_doc` is wasteful).
- B4 — Benchmark GPU path vs CPU path with representative data to ensure optimization is working.

### Phase C: Horizontal Scaling (Tasks 4.3.1–4.3.4)
**Files:** `Cargo.toml`, `mcp.rs`, `main.rs`, new `flight.rs`
**Goal:** Network-transparent multi-instance MCP with Arrow Flight data plane
- C1 — Add `arrow-flight` and `tonic` (already present) deps; implement an Arrow Flight service for data exchange.
- C2 — Add an HTTP/WebSocket gateway (axum) that accepts MCP requests and routes them to backends.
- C3 — Implement session affinity via consistent hashing (session_id → worker).
- C4 — Wire the distributed session store (Redis or in-memory with replication).
- C5 — Write stress test (50k req/s).

### Phase D: VM Separate Process (Task 3.4.3) — Optional / Deferred
**Goal:** Separate VM processing for fault isolation if required, without sacrificing latency.
- D1 — Add serde derives to `OpCode`, `Instruction`, `Program`, `AstNode`.
- D2 — Write a VM worker binary that listens on a Unix socket / shared memory for incoming `Program` bytecode.
- D3 — Wire the planner to dispatch via the IPC path instead of direct `execute_program()`.

---

**Priority order:** Phase A (observability is low-risk and immediately useful) → Phase B (GPU perf) → Phase C (scaling, large effort) → Phase D (opt-in, deferred).
