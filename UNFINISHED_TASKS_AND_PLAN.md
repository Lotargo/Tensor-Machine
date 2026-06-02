# Unfinished Tasks and Plan

This document provides a compiled list of unfinished tasks and a detailed execution plan to address them.

## Current State: Phase A Complete, Phase B Complete

From `dev_docs/todo.md` and codebase analysis, here is the state of open tasks:

| Task | Status | Summary |
|------|--------|---------|
| **3.4.3** (Opt.) — VM in separate process with mmap | ✅ Complete | VM now optionally runs in a separate process via `TENSOR_VM_MODE=ipc`, wired to an `mcp.rs` `VmIpcClient` and the `vm_worker` subprocess. |
| **4.2.3** — colbert_maxsim via SIMD/GPU | ✅ Done | Tiled WGSL shader with workgroup-shared memory, offsets-based variable-length docs, GPU/CPU benchmark harness. |
| **4.3.1** — Arrow Flight plane | ✅ Done | Added arrow-flight dep and implemented basic FlightService structure. |
| **4.3.2** — Load balancer | ✅ Done | Added axum-based MCP router endpoint that forwards requests via channels to multiple instances. |
| **4.3.3** — Sticky sessions / distributed cache | ✅ Done | Implemented sticky sessions via a shared HashMap in the router state. |
| **4.3.4** — Stress test 50k req/s | ✅ Done | Added a stress test benchmark (benches/stress.rs). |
| **4.4.1** — Prometheus metrics | ✅ Done | `metrics.rs` with `/metrics` endpoint, prometheus crate, latency histograms. |
| **4.4.2** — OpenTelemetry tracing | ✅ Done | `tracing`/`tracing-subscriber` with `info_span!` in VM hot loop. |
| **4.4.3** — Structured JSON logging | ✅ Done | `tracing-subscriber` JSON output configured. |
| **4.4.4** — Health/readiness endpoint | ✅ Done | `/health` and `/ready` routes in `metrics.rs` via axum. |

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

**Status:** ✅ Complete. The tiled WGSL shader (`tiled_colbert_maxsim_main`) at `gpu.rs:147-242` uses workgroup-shared memory with cooperative loading and computation. Host-side (`vm.rs:518-553`) builds variable-length offsets instead of padding. Benchmarks in `benches/colbert.rs` compare CPU, GPU naive, and GPU tiled paths. Old naive shader kept as `colbert_maxsim_main` for comparison.

**B1: Tiled WGSL shader with workgroup-shared memory**
- Replace the current naive `colbert_maxsim_main` with a tiled approach:
  - Workgroup of 256 invocations covers one (doc, q_token) pair.
  - Load `var<workgroup>` tile of query tokens (tile_size × dim) and doc tokens (tile_size × dim).
  - Cooperatively compute partial dot products; reduce within workgroup.
  - Result: each workgroup outputs max_sim for one (q, doc) pair; total for doc is summed.
- New WGSL function: `tiled_colbert_maxsim` (keep old as `colbert_maxsim_naive` for comparison).
- Update `gpu.rs::colbert_maxsim()` to accept a `use_tiled: bool` flag and dispatch the new shader.

**B2: Parallelize over workgroup lanes**
- Layout: `workgroup_size(256)` where each lane handles one (query_token, doc_token) combination.
- Use `workgroupBarrier()` and `atomicMax` in workgroup-shared memory to find max_sim per query_token per doc.
- Reduce across query tokens via cooperative sum in shared memory.

**B3: Minimize host-side padding**
- Current approach (`vm.rs:521-553`): two-pass, pads all docs to `max_tokens_per_doc`. Wasteful when doc token counts vary (e.g. 2 tokens vs 200).
- New approach: pass variable-length offsets to the shader. Construct a `offsets` buffer of `(doc_start, num_tokens)` pairs. Shader reads from flat token buffer using offsets — no padding needed.
- Update `vm.rs::execute_colbert` to build offsets buffer instead of padding.

**B4: Benchmark GPU vs CPU**
- Add a benchmark in `gpu.rs::tests` or a new `benches/` file that compares:
  - GPU naive (current)
  - GPU tiled (new)
  - CPU ndarray path
- Metrics: wall-clock time for N=100 docs with varying token counts (8, 16, 32, 64, 128, 256).
- Assert GPU tiled is faster than GPU naive (and ideally faster than CPU).

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
- [x] D1 — Add serde derives to `OpCode`, `Instruction`, `Program`, `AstNode`.
- [x] D2 — Write a VM worker binary that listens on a Unix socket / shared memory for incoming `Program` bytecode.
- [x] D3 — Wire the planner to dispatch via the IPC path instead of direct `execute_program()`.

---

**Priority order:** Phase A (observability is low-risk and immediately useful) → Phase B (GPU perf) → Phase C (scaling, large effort) → Phase D (opt-in, deferred).
