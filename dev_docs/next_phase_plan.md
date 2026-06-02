# Next Phase Execution Plan

## Status Summary

All Phases 1–5 scaffolding is structurally in place (IR, VM, Arrow, DSL, GPU, MCP server). However, several critical components remain **synthetic stubs** that must be replaced with real implementations. This plan covers those remaining items.

---

## Task 1: Real MMR Algorithm (`vm.rs`)

**Current:** Synthetic mock — scores scaled by `(1-diversity)`, sorted, top-10 returned. No similarity computation, no greedy selection.

**Implementation:**
1. Implement `mmr_rerank(batch: &RecordBatch, scores: &[f32], lambda: f32, top_k: usize) -> TopKResult`
   - Extract vectors from the `RecordBatch` column (already an `arrow::FixedSizeListArray` → `Array2<f32>`)
   - Compute pairwise cosine similarity matrix: `sim[i][j] = dot(vec_i, vec_j) / (norm_i * norm_j)`
   - Greedy selection loop:
     - Start with the highest-relevance item in set S
     - For each remaining candidate i, compute: `mmr_score = λ * rel(i) - (1-λ) * max_{j∈S} sim(i, j)`
     - Select argmax, add to S, repeat until `|S| = top_k`
2. Wire into `OpCode::Mmr(diversity)` handler, using `lambda = 1.0 - diversity` (matching DSL convention)
3. Update `planner.rs` or `ir.rs` if `diversity` needs to carry `top_k` parameter

**Files:** `src/vm.rs`
**Tests:** Unit test with 4 known vectors + scores; verify diversity produces different ordering
**Acceptance:** MMR with `diversity=0` returns pure relevance ranking; `diversity=1` returns pure diversity ranking

---

## Task 2: Real RRF Algorithm (`vm.rs`)

**Current:** Hardcoded mock — ignores both input rankings, returns `[0,1,2]` with synthetic scores.

**Implementation:**
1. Refine `TopKResult` to carry rank positions (currently has `indices` and `scores` — these imply rank order already by position in the array)
2. Implement `rrf_fusion(rankings: &[TopKResult], k: usize) -> TopKResult`
   - Build inverted index: `HashMap<usize, Vec<usize>>` mapping document ID → list of ranks across input rankings
   - For each document: `rrf_score = Σ 1/(k + rank_i(d))`
   - Sort by RRF score descending, take top-k
3. Wire into `OpCode::Rrf(k)` — pop N values from stack (support N≥2), pass to `rrf_fusion`

**Files:** `src/vm.rs`
**Tests:** Unit test with 2 known rankings; verify fused ordering is correct per RRF formula
**Acceptance:** RRF with `k=60` produces standard RRF weighting; ordering matches known examples

---

## Task 3: Arrow Flight Service (`flight.rs`)

**Current:** 100% unimplemented — all 10 `FlightService` methods return `Status::unimplemented`.

**Implementation (in order):**
1. **`do_put`** — receive stream of `FlightData`, reassemble into `RecordBatch`, store in a `Arc<RwLock<HashMap<FlightDescriptor, RecordBatch>>>` registry
2. **`do_get`** — accept `Ticket`, look up `RecordBatch` by ticket string, serialize to `FlightData` stream
3. **`do_exchange`** — bidirectional stream: accept query descriptor, return matching `RecordBatch` data (enables inter-instance exchange)
4. **`get_flight_info`** — return `FlightInfo` with available dataset endpoints from the registry
5. **`get_schema`** — return Arrow schema for a given `FlightDescriptor`
6. **`handshake`** — accept any handshake request, return empty `HandshakeResponse` (no auth)
7. **`list_flights`, `poll_flight_info`, `do_action`, `list_actions`** — minimal stubs returning empty responses
8. **Wire in `main.rs`** — start a second `tonic` gRPC server on `ARROW_FLIGHT_ADDR` (default `0.0.0.0:50051`) alongside the MCP server
9. **Integration test** — two clients: one does `do_put`, the other does `do_get`, verify roundtrip

**Files:** `src/flight.rs` (rewrite), `src/main.rs` (wire up), optionally `src/lib.rs` (export registry type)
**Tests:** Unit test for `do_get`/`do_put` roundtrip; integration test for cross-instance exchange
**Acceptance:** RecordBatch written via `do_put` is retrievable via `do_get` with same schema and data

---

## Task 4: Real ML Server Embedding (`db.rs`)

**Current:** `get_embedding()` always returns `Array1::from_elem(768, 0.1)` regardless of ML server configuration.

**Implementation:**
1. Define a proto for the embedding service (or use OpenAI-compatible `/v1/embeddings` JSON API via `reqwest`)
2. Add `embedding_dim: usize` and `api_base: String` to `DbPoolsConfig`
3. In `get_embedding()`:
   - If `ml_servers` has configured endpoints, make real gRPC/HTTP call to get embedding
   - Fall back to dummy `Array1::from_elem(embedding_dim, 0.1)` only when no servers configured
4. Remove hardcoded `768` → use `self.embedding_dim`
5. Wire the embedding result into the MCP `SearchTool` handler (`mcp.rs`)

**Files:** `src/db.rs`, `src/mcp.rs`
**Tests:** Mock HTTP server or gRPC test double; verify real call is made when server configured
**Acceptance:** When `ML_SERVER_URL` env var is set, embedding is fetched from that server; when unset, fallback dummy is used

---

## Task 5: ColBERT SIMD Path (`vm.rs`)

**Current:** CPU fallback uses ndarray `dot()` (already SIMD-accelerated via BLAS) but is not exposed as a named, benchmarkable path.

**Implementation:**
1. Extract the CPU ndarray-based ColBERT logic from `OpCode::ColbertMaxSim` handler into a standalone function:
   ```rust
   fn colbert_maxsim_simd(
       query_tokens: &Array2<f32>,
       doc_tokens: &Array2<f32>,
       num_docs: usize,
       colbert_offsets: &[ColbertOffsets],
   ) -> Array1<f32>
   ```
2. Add a `BackendSelector` enum variant `Simd` to the existing CPU/GPU switch (alongside `Cpu` and `Gpu`)
3. Add benchmarking criterion harness in `benches/colbert.rs` with scenarios: sparse (8 tok/doc), medium (32), dense (128), mixed
4. Compare: GPU naive vs GPU tiled vs CPU SIMD

**Files:** `src/vm.rs`, `benches/colbert.rs`
**Acceptance:** All three backends produce bitwise-identical (or within 1e-5) scores on the same inputs

---

## Task 6: Configuration Production-Hardening

### 6a: Rate Limit (`mcp.rs`)
- Move hardcoded `100` to `MCP_RATE_LIMIT` env var with `100` default
- Add per-session rate limiter (separate `Governor` per session ID)
- Add Prometheus counter `mcp_rate_limited_requests_total`

### 6b: Circuit Breaker (`db.rs`)
- Move thresholds to env vars: `CB_FAILURE_THRESHOLD` (default 5), `CB_COOLDOWN_SECS` (default 30)
- Expose state via Prometheus gauge `circuit_breaker_state{backend="qdrant|postgres|ml"}` (0=closed, 1=open, 2=half-open)

### 6c: Qdrant Collection Name (`db.rs`)
- Move `"default"` to `QDRANT_COLLECTION` env var
- Support per-request override via query filters

**Files:** `src/mcp.rs`, `src/db.rs`, `src/metrics.rs`
**Tests:** Verify env var overrides take effect

---

## Task 7: Fix DSL Panics (`dsl.rs`)

**Current:** 13 `panic!("Expected X")` calls in `dsl.rs` exhaustive match guards. Although structurally unreachable if the compiler is correct, a single logic bug crashes the server.

**Implementation:**
Replace each `_ => panic!(...)` with:
```rust
_ => return Err(format!("compile_{}: unexpected node {:?}", context, node))
```
All 13 sites follow the same pattern — convert to `Err(...)`.

**Files:** `src/dsl.rs`
**Tests:** Existing DSL tests should pass unchanged
**Acceptance:** No `panic!` calls remain in `dsl.rs`

---

## Execution Order

| Step | Task | Effort | Files | Verification |
|------|------|--------|-------|-------------|
| 1 | Fix DSL panics | Small (13× edit) | `dsl.rs` | `cargo test` passes, `grep panic! dsl.rs` = 0 |
| 2 | RRF Algorithm | Small (1 fn) | `vm.rs` | Unit test with known rankings |
| 3 | MMR Algorithm | Medium (1 fn + greedy loop) | `vm.rs` | Unit test with 4 vectors |
| 4 | ML Server Embedding | Medium (HTTP/gRPC client) | `db.rs`, `mcp.rs` | Mock server test |
| 5 | Config hardening | Small (env vars) | `mcp.rs`, `db.rs`, `metrics.rs` | Env override tests |
| 6 | ColBERT SIMD benchmark | Medium (function extraction + bench) | `vm.rs`, `benches/colbert.rs` | Benchmarks compile and run |
| 7 | Arrow Flight Service | Large (full protocol impl) | `flight.rs`, `main.rs` | Roundtrip integration test |

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| MMR O(n²) similarity matrix on 1000 docs → 1M entries (4 MB), acceptable | Low | Add note; optimize to dot-product only if needed |
| Arrow Flight `do_exchange` protocol complexity | Medium | Start with `do_get`/`do_put` only; `do_exchange` can be minimal |
| ML server proto mismatch with real server | Medium | Use OpenAI-compatible `/v1/embeddings` REST API as lowest common denominator |
| SIMD path produces slightly different float results vs GPU | Low | Accept 1e-5 tolerance; GPU uses fp32 same as CPU |
