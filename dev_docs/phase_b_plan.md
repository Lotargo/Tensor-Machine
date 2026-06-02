# Phase B: GPU ColBERT MaxSIM Optimization

**Goal:** Optimize the GPU `colbert_maxsim` compute shader with shared memory tiling.

**Current state:** Naive triple-loop WGSL shader (one invocation per doc, triple-nested loops over query tokens, doc tokens, dim). Host-side pads all docs to `max_tokens_per_doc`.

## B1 — Tiled WGSL shader with workgroup-shared memory

### Implementation
- Replace `colbert_maxsim_main` in `SHADER_SRC` (gpu.rs) with a tiled version `tiled_colbert_maxsim`
- Keep the old shader as `colbert_maxsim_naive` for A/B comparison
- New shader: workgroup of 256 invocations processes one (doc_idx, q_tile) pair
- Load `var<workgroup> query_tile[32][128]` and `var<workgroup> doc_tile[32][128]` (tile_size × dim, where dim ≤ 128)
- Cooperatively compute partial dot products; reduce within workgroup
- Each invocation: load one row, compute dot, contribute to workgroup-level max

### Key decisions
- Tile size: 32 × 128 (matches typical ColBERT dim=128, allows 32 tokens per tile)
- Workgroup size: 256 invocations = 8 rows × 32 dims per row
- Use `workgroupBarrier()` and `atomicMax` for reduction

### Files
- `gpu.rs`: Add new shader variant, update `GpuContext::colbert_maxsim()` with `use_tiled: bool` parameter

## B2 — Parallelize over workgroup lanes

### Strategy
- Layout: `@workgroup_size(256)` where 256 = tile_rows * (dim / 4), handling 4 floats at a time per lane
- Each lane handles one (q_token_idx, doc_token_idx, dim_chunk) combination
- Steps:
  1. Load query tile row: `var q_row = query_tile[q_local][dim_local];`
  2. Load doc tile row: `var d_row = doc_tile[d_local][dim_local];`
  3. Accumulate partial: `partial_dot += q_row * d_row;`
  4. Barrier, then reduce partials within workgroup using `workgroupAll` or shared atomics
  5. Track max per q_token for current doc
  6. Barrier, sum maxes across q_tokens → final score for doc

### Files
- `gpu.rs`: Shader rewrite with cooperative dispatch

## B3 — Minimize host-side padding

### Current waste
- `vm.rs:521-553`: Two-pass padding to `max_tokens_per_doc`. If one doc has 200 tokens and another has 2, GPU still allocates and computes on 200 for both.
- The naive shader assumes uniform `num_d` — any padded zeros waste cycles on dot product computation.

### New approach: Offset-based variable-length
- Instead of padding, build a `ColbertOffsets` buffer on the host:
  ```rust
  struct ColbertOffsets {
      doc_start: u32,   // flat index into all_doc_tokens
      num_tokens: u32,  // actual token count for this doc
  }
  ```
- Flatten all doc tokens contiguously (no padding between docs).
- Pass offset buffer to shader as a storage buffer.
- Each invocation reads offset for its doc_idx, iterates only `num_tokens` loop iterations.
- Results: zero wasted compute cycles, smaller GPU buffer allocations.

### Implementation steps
1. Add `ColbertOffsets` struct (Rust + WGSL mirror with `bytemuck`).
2. Build offset buffer in `vm.rs` instead of padding.
3. Update `colbert_maxsim()` to accept and bind the offsets buffer.
4. Update the shader to read `offsets[doc_idx].doc_start` and `offsets[doc_idx].num_tokens`.
5. Remove the `num_d` parameter (inferred from offsets).

### Files
- `gpu.rs`: `ColbertOffsets` struct, updated shader, updated `colbert_maxsim()` signature
- `vm.rs`: Replace padding logic with offset buffer construction

## B4 — Benchmark GPU vs CPU

### Approach
- Add new benchmark group in `benches/zero_copy.rs` or a new `benches/colbert.rs`
- Use criterion + `BatchSize::SmallInput` for warm GPU state
- Use `test_gpu_colbert_maxsim` as starting point, but wrap in criterion

### Scenarios
| Scenario | Docs | Avg tokens/doc | Dim |
|----------|------|---------------|-----|
| Sparse   | 100  | 8             | 128 |
| Medium   | 100  | 32            | 128 |
| Dense    | 100  | 128           | 128 |
| Mixed    | 100  | 8-128 uniform | 128 |

### Metrics
- Wall-clock time per invocation (mean, P50, P99 from criterion)
- Compare: GPU naive vs GPU tiled vs CPU ndarray
- Assert: GPU tiled >= 2x faster than GPU naive on dense scenario

### Files
- `benches/colbert.rs` (new)

## Dependencies / risk
- WGSL `var<workgroup>` arrays need constant size known at compile time — tile size must be fixed. Mixing docs with different token counts is fine since we iterate to `num_tokens` (from offsets), not to tile size.
- Workgroup size 256 may be device-limited (< 256 on some mobile GPUs). Make workgroup size configurable via `@workgroup_size` constant and fall back to 64 if needed.
- GPU context creation is async — benchmarks need `tokio::runtime::Runtime` or `futures::executor::block_on`.
