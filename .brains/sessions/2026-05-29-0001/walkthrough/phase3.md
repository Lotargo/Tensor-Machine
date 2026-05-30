Let me read the key source files for exact references.
```text
# Phase 3: Zero-Copy Implementation in tensor-rag

## Overview

Phase 3 migrates the Tensor VM from heap-allocated `ndarray::Array1<f32>` vectors to
Apache Arrow's columnar format, enabling zero-copy data sharing between the I/O layer
and the compute engine. The `arrow` crate (v51.0.0) serves as the in-memory representation
for all tensor data flowing through the system.

**Crate dependency** (`tensor-rag/Cargo.toml:15`):
```toml
arrow = "51.0.0"
```

The official `apache/arrow-rs` crate was chosen over `arrow2` to avoid maintenance risk
from the community fork's uncertain long-term support (see
`.dataset/lessons/arrow_integration.json`).

---

## RecordBatch Schema

Defined in `tensor-rag/src/arrow.rs:15-26`:

```
Column       │ Arrow Type                        │ Nullable │ Role
─────────────┼───────────────────────────────────┼──────────┼────────────────────
id           │ Int64                             │ no       │ Document ID
vector       │ FixedSizeList(Float32, 768)       │ no       │ 768-dim embedding
bm25_score   │ Float32                           │ no       │ Pre-computed BM25
text         │ Utf8                              │ no       │ Document text
```

The `FixedSizeList` layout stores all vectors in a contiguous `Float32Array` buffer,
which is the foundation for zero-copy slicing and SIMD-friendly iteration.

---

## Converters to RecordBatch

### Qdrant → RecordBatch (`arrow.rs:29-75`)

```rust
pub fn qdrant_to_record_batch(
    ids: &[usize],
    vectors: &[Array1<f32>],
) -> Result<RecordBatch, arrow::error::ArrowError>
```

- Validates `ids.len() == vectors.len()` and each vector has length exactly 768.
- Flattens all vectors into `Vec<f32>` (length = n × 768).
- Wraps in `FixedSizeListArray` with item size 768.
- Fills `bm25_score = 0.0` and `text = ""` (these are populated by the Postgres converter).
- Returns a `RecordBatch` matching the schema via `RecordBatch::try_new`.

**Invoked from** `mcp.rs:188` inside `execute_program`:
```rust
let batch = crate::arrow::qdrant_to_record_batch(&ids, &vectors)?;
vm.batch = Some(Arc::new(batch));
```

### PostgreSQL → RecordBatch (`arrow.rs:92-129`)

```rust
pub fn postgres_to_record_batch(
    ids: &[usize],
    bm25_scores: &[f32],
    texts: &[String],
) -> Result<RecordBatch, arrow::error::ArrowError>
```

- Fills the vector column with zeros (`vec![0.0; ids.len() * 768]`) since vectors
  originate from Qdrant, not Postgres.
- Builds `Float32Array` for BM25 scores and `StringArray` for text.
- Returns a complete `RecordBatch`.

### Helper Extractors

- **`extract_vector_scores`** (`arrow.rs:77-83`): Downcasts the `vector` column from
  `FixedSizeListArray` → inner `Float32Array`. Returns the buffer as a `Float32Array`
  (data remains in the Arrow buffer via `flat.values()` — a zero-copy read).
- **`extract_float_column`** (`arrow.rs:85-89`): Generic float column extraction.

---

## VM Integration: Zero-Copy on the Stack

### The `Value` Enum (`vm.rs:6-12`)

```rust
pub enum Value {
    Scalar(f32),
    Tensor1D(Array1<f32>),
    ArrowArray(Arc<dyn Array>),  // zero-copy Arrow
    Result(TopKResult),
}
```

`Value::ArrowArray(Arc<dyn arrow::array::Array>)` is the zero-copy conduit. Only the
`Arc` is cloned when values move through the stack — the underlying Arrow buffer data
is shared.

### VM State (`vm.rs:34-38`)

```rust
pub struct VM {
    pub stack: Vec<Value>,
    pub batch: Option<Arc<arrow::record_batch::RecordBatch>>,
    pub query_vector: Array1<f32>,
}
```

`batch: Arc<RecordBatch>` holds the full result set from Qdrant + Postgres behind a
shared reference. Every instruction that reads candidate vectors reads from this single
allocation.

### Dual-Path OpCodes

Every compute opcode has two implementation paths: one for `Value::Tensor1D` (ndarray,
backwards-compatible) and one for `Value::ArrowArray` (Arrow-native). The Arrow paths
use `arrow::compute::kernels::numeric` for element-wise operations:

| OpCode           | Arrow Kernel / Strategy                          | File:Line    |
|------------------|--------------------------------------------------|--------------|
| `Add`            | `arrow::compute::kernels::numeric::add`          | vm.rs:62-65  |
| `Mul`            | `arrow::compute::kernels::numeric::mul`          | vm.rs:77-81  |
| `Fusion`         | Two `mul` calls + one `add` call                 | vm.rs:96-107 |
| `Matmul`         | `mul` then `arrow::compute::sum`                 | vm.rs:124-131|
| `Div`            | `arrow::compute::kernels::numeric::div`          | vm.rs:142-145|
| `Threshold`      | Iterator with conditional logic                   | vm.rs:157-160|
| `Sort`           | Collect to Vec, sort descending                  | vm.rs:173-177|
| `Slice`          | `arr.slice(start, end)` — zero-copy view         | vm.rs:189-191|
| `TopK`           | Enumerate + sort + take k                        | vm.rs:208-221|
| `ComputeSemantic`| Direct `flat.values()` read + dot product loop   | vm.rs:229-248|
| `IfConfidence`   | Iterator over `t_f32.iter().flatten()`            | vm.rs:311-315|

### ComputeSemantic: Direct Buffer Read

`vm.rs:229-248` — the cornerstone of zero-copy tensor computation:

```rust
OpCode::ComputeSemantic => {
    let batch = self.batch.as_ref().ok_or("No batch found")?;
    let flat = crate::arrow::extract_vector_scores(batch)?;
    let values = flat.values();  // &[f32] — zero-copy read from Arrow buffer
    for i in 0..n {
        let chunk = &values[start..start + dim];
        let dot: f32 = chunk.iter().zip(query).map(|(a, b)| a * b).sum();
        scores.push(dot);
    }
    self.stack.push(Value::ArrowArray(Arc::new(Float32Array::from(scores))));
}
```

`flat.values()` returns a `&[f32]` slice directly into the Arrow `Float32Array` buffer.
No per-element copies; the dot product loop reads cache-friendly contiguous memory.

### Slice: Truly Zero-Copy

`vm.rs:189-191` — `arr.slice(*start, end - start)` returns an Arrow `ArrayRef` view
into the existing buffer without copying any data. This is the only opcode that is
genuinely zero-copy for both input and output.

---

## I/O Orchestration (`mcp.rs:173-242`)

The async `execute_program` function bridges external I/O and the VM:

1. **`RetrieveQdrant`** (line 181): Calls `db_pools.search_qdrant()`, converts the
   result via `qdrant_to_record_batch()`, stores as `Arc<RecordBatch>` in `vm.batch`.
2. **`RetrievePostgres`** (line 192): Reads IDs from the existing batch's `Int64Array`,
   fetches BM25 scores, pushes result as `Value::ArrowArray(Arc::new(Float32Array))`.
3. **`IfConfidence`** (line 204): Pops the top tensor, finds max score, recurses into
   the appropriate sub-program.
4. **Default** (line 234): Delegates pure-compute opcodes to `vm.step()`.

---

## Result Serialization

The final `TopKResult` (indices + scores) is serialized via **FlatBuffers**
(`result.fbs`, `result_generated.rs`) into a compact binary payload of a few dozen
bytes — the only data that crosses the process boundary back to the LLM agent.

---

## Current Zero-Copy Status

### Achieved
- `Arc<RecordBatch>` is shared between orchestrator and VM — no clone of the
  full result set.
- `Arc<dyn Array>` values on the VM stack are reference-counted, not copied.
- `arr.slice(start, end)` in `OpCode::Slice` returns an Arrow array view without
  copying data.
- `flat.values()` in `ComputeSemantic` reads directly from the Arrow buffer.

### Not Yet Achieved
- `qdrant_to_record_batch()` still receives `&[Array1<f32>]` (ndarray) rather than
  building directly from a `bytes::Bytes` gRPC buffer as envisioned in the
  architecture manifesto.
- `postgres_to_record_batch()` allocates `vec![0.0; ids.len() * 768]` for the
  vector column.
- `ComputeSemantic` allocates a new `Float32Array` for results (unavoidable for new
  computed values).
- mmap-based shared memory (`MmapRegion<T>`) and Arrow IPC inter-process protocol
  (architecture Tasks 8.1–8.3) remain unimplemented.
```
The documentation has been output to stdout above. It covers:

1. **Arrow crate integration** — `arrow = "51.0.0"` in `Cargo.toml:15`, rationale for choosing official `arrow-rs` over `arrow2`.
2. **RecordBatch schema** — four-column schema (`id`, `vector`, `bm25_score`, `text`) with `FixedSizeList<Float32>[768]` for embeddings.
3. **Converters** — `qdrant_to_record_batch` (flattens ndarray vectors into Arrow buffer) and `postgres_to_record_batch` (fills BM25 scores and text), both in `arrow.rs:29-129`.
4. **VM usage** — the `Value::ArrowArray(Arc<dyn Array>)` variant on the stack, the `batch: Arc<RecordBatch>` field, and dual-path opcodes that dispatch to `arrow::compute` kernels vs. ndarray. Key zero-copy operations: `flat.values()` in `ComputeSemantic` (direct buffer read), `arr.slice()` (view without copy), and `Arc` sharing everywhere.
5. **Status** — what is and isn't yet zero-copy (gRPC byte buffer bypass, mmap, Arrow IPC remain unimplemented).
