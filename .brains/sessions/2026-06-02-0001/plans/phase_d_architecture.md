# Phase D: VM Separate Process — Architectural Breakdown

**Date:** 2026-06-02
**Corresponds to:** Task 3.4.3 (deferred/optional from Phase 3)
**Goal:** Run the Tensor VM in a separate OS process with mmap-based IPC for fault isolation, without sacrificing latency.

---

## 1. Why Separate?

### Motivations
- **Fault isolation:** A crash in the VM (e.g., memory corruption in a WGSL shader, stack underflow in user-supplied bytecode) must not take down the MCP server.
- **Resource isolation:** GPU context, large Arrow batches, and tensor allocations live in the VM process. The planner/orchestrator stays lightweight.
- **Hot-restart:** A crashed VM can be resurrected (with state loss) without restarting the MCP server.

### Why It's Deferred/Optional
- IPC adds **1–10 µs per op** overhead (serialization + context switch). The `execute_program` hot loop is currently in-process, and a single query can execute 10–50 ops. At P99 <5ms, IPC could consume 2–20% of the budget.
- GPU context duplication across processes is non-trivial (WGPU handles are process-local).
- The current single-process architecture is already cleanly separated: `Planner` → `Program` → `execute_program()`. The seam is well-defined.

**Decision gate:** Only implement if measured P99 exceeds 3ms for single-process runs or if VM crashes are observed in production.

---

## 2. Current Architecture (Simplified)

```
MCP Server (main process)
  │
  ├── Planner::compile(ast) → Program (pure, no side effects)
  │
  └── execute_program(program, vm, db_pools)
        │
        ├── OpCode::RetrieveQdrant     → in-process call to db_pools
        ├── OpCode::RetrievePostgres   → in-process call to db_pools
        ├── OpCode::{math ops}         → vm.step()
        ├── OpCode::IfConfidence       → recursive execute_program
        └── OpCode::Emit               → return Value
```

D1 is already done: `OpCode`, `Instruction`, `Program`, `AstNode` all have `#[derive(Serialize, Deserialize)]`.

---

## 3. Target Architecture

```
MCP Server (process A)
  │
  ├── Planner::compile(ast) → Program
  │
  ├── DISPATCH MODE SELECTOR ─────────────────────────────┐
  │   │    (env: TENSOR_VM_MODE=inprocess|ipc)             │
  │   │                                                    │
  │   ▼ (in-process)                                      ▼ (IPC)
  │   execute_program()                         ┌──────────────────┐
  │                                              │  VM Worker       │
  │                                              │  (process B)     │
  │                                              │                  │
  │                                              │  Unix socket     │
  │                                              │  listener        │
  │                                              │                  │
  │                                              │  Shared memory   │
  │                                              │  (mmap)          │
  │                                              │                  │
  │                                              │  VM::step()      │
  │                                              │  GPU context     │
  │                                              └──────────────────┘
```

Two dispatch paths coexist:
- **In-process** (default): Same as today — zero-copy, zero-serialization.
- **IPC** (opt-in): Serialize `Program` → send over Unix socket → VM worker deserializes and executes → results back via shared memory.

---

## 4. IPC Protocol Design

### 4.1 Control Channel: Unix Socket (Seqpacket)

- **Socket type:** `tokio::net::UnixSeqpacket` (reliable, preserves message boundaries, supports SCM_CREDENTIALS for auth).
- **Protocol:** Simple binary framing — length-prefixed bincode-serialized messages.

**Messages:**

```rust
enum VmRequest {
    Execute {
        program: Program,
        session_id: String,
        sequence: u64,
    },
    Ping,
    Shutdown,
}

enum VmResponse {
    Result {
        value: Vec<u8>,        // bincode-serialized Value
        session_id: String,
        sequence: u64,
    },
    Error {
        message: String,
        session_id: String,
        sequence: u64,
    },
    Pong,
    ShutdownAck,
}
```

### 4.2 Data Channel: mmap Shared Memory

The Unix socket carries only control metadata and small scalars. Large payloads (Arrow `RecordBatch`, tensor buffers >64KB) go through mmap.

**Shared memory layout:**

```
[ 8 bytes  ] region_size
[ 4 bytes  ] version
[ 4 bytes  ] flags (ready/done/error)
[ 4 bytes  ] payload_len
[ 4 bytes  ] checksum (CRC32 of payload)
[ N bytes  ] payload (bincode-serialized Value or Program)
```

**Lifecycle per request:**
1. Planner writes serialized `Program` into shared memory region `vm_ipc_in_{session_id}`.
2. Planner sends `VmRequest::Execute` over Unix socket (includes region name).
3. VM worker reads `Program` from shared memory, executes, writes result back to region `vm_ipc_out_{session_id}`.
4. VM worker sends `VmResponse::Result` over Unix socket.
5. Planner reads result from shared memory.

### 4.3 Zero-Copy Option (Future)

If Arrow IPC is available, the `RecordBatch` can be sent via Arrow Flight (the Phase C component) and only a file descriptor is passed via SCM_RIGHTS over the Unix socket. This avoids any copying for large columnar data. However, this is **out of scope for Phase D** — Phase D uses bincode for simplicity.

---

## 5. Implementation Plan

### D2a — Create `vm_worker` Binary (`tensor-rag/src/bin/vm_worker.rs`)

**New file.** A standalone binary that:

1. **Initializes:**
   - Opens a `UnixListener` at a configurable path (e.g., `/tmp/tensor_rag_vm.sock`).
   - Creates a `VM` with optional GPU init.
   - Opens shared memory regions (via `SharedMemoryManager`).

2. **Main loop:**
   ```
   loop {
       accept connection → UnixSeqpacket
       spawn handler:
           loop {
               recv VmRequest
               match {
                   VmRequest::Execute{program, session_id, seq} →
                       read program from shared memory
                       execute in VM
                       write result to shared memory
                       send VmResponse::Result
                   VmRequest::Ping →
                       send VmResponse::Pong
                   VmRequest::Shutdown →
                       send VmResponse::ShutdownAck
                       break
               }
           }
   }
   ```

3. **GPU handling:**
   - `init_gpu()` is called once at startup. WGPU contexts are not shared across processes, so the VM worker has its own GPU context.
   - If GPU init fails, fall back to CPU-only mode.

4. **Instruction limits:**
   - Same `max_instructions` enforcement as the in-process path.
   - A watchdog timeout (e.g., 5s per `VmRequest::Execute`) prevents runaway programs.

**Cargo.toml changes:**
- Add `[[bin]]` entry for `vm_worker`.
- Add `bincode` dep for binary serialization (compact, fast, serde-compatible).

```toml
[[bin]]
name = "vm_worker"
path = "src/bin/vm_worker.rs"

[dependencies]
bincode = "1.3"
```

### D2b — Shared Memory Enhancements

The existing `SharedMemoryManager` in `shared_memory.rs` only provides `MmapRegion` (file handles pointing to tmpfs files). We need actual `mmap` syscall integration.

**Add to `shared_memory.rs`:**

```rust
pub struct MappedBuffer {
    ptr: *mut u8,
    len: usize,
    file: File,
}

impl MappedBuffer {
    pub fn map(region: &MmapRegion, size: usize) -> Result<Self, String>;
    pub fn write<T: Serialize>(&mut self, value: &T) -> Result<(), String>;
    pub fn read<T: DeserializeOwned>(&self) -> Result<T, String>;
    pub fn flush(&self) -> Result<(), String>;
    pub fn unmap(self) -> Result<(), String>;
}
```

Implementation uses `libc::mmap` with `PROT_READ | PROT_WRITE`, `MAP_SHARED`. Since the crate already has a `MmapRegion` abstraction over tmpfs files, this is a natural extension.

**New shared memory region naming convention:**
- `tensor_rag_vm_in_{session_id}` — planner writes program here
- `tensor_rag_vm_out_{session_id}` — VM worker writes result here

**Synchronization:**
- Use a simple atomic flag in shared memory header (not mutex — no cross-process robust mutex in std).
- Poll-based: planner polls `flags` field until `done` bit is set (with timeout).
- Or use the Unix socket as a completion notification (simpler: socket message signals "result ready", then planner reads from shm).

### D2c — Add `bincode` Serialization

Add `bincode = "1.3"` to `Cargo.toml`. All the necessary serde derives already exist on `OpCode`, `Instruction`, `Program`, `AstNode`, `Value` (note: `Value` does *not* currently derive `Serialize`/`Deserialize` — this must be added).

**Check `Value` enum in `vm.rs`:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]  // ← needs Serialize/Deserialize
pub enum Value {
    Scalar(f32),
    Tensor1D(Vec<f32>),     // changed from Array1<f32>
    Tensor2D(Vec<Vec<f32>>), // changed from Array2<f32>
    ArrowArray(...),         // problematic — Arc<dyn Array> is not serde
    Result(TopKResult),
}
```

**Problem:** `Value::ArrowArray(Arc<dyn Array>)` cannot be serialized. `Array1<f32>` and `Array2<f32>` also lack serde derives.

**Solution:** Use `Vec<f32>` and `Vec<Vec<f32>>` for the IPC representation. Convert to/from `ndarray` types on each end. For `ArrowArray`, fall back to writing the raw f32 slice as `Vec<f32>`.

**Create a new type for IPC:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcValue {
    Scalar(f32),
    Tensor1D(Vec<f32>),
    Tensor2D(Vec<Vec<f32>>),
    FlatArray(Vec<f32>),  // ArrowArray → Vec<f32>
    Result(TopKResult),
}

impl From<&Value> for IpcValue { ... }
impl From<IpcValue> for Value { ... }
```

### D3a — Dispatch Mode Selector (`mcp.rs`)

Add an `IpClient` that wraps the IPC path:

```rust
pub struct VmIpcClient {
    socket_path: PathBuf,
    conn: UnixSeqpacket,
    shm_manager: SharedMemoryManager,
}

impl VmIpcClient {
    pub async fn connect(socket_path: &str) -> Result<Self, String>;
    pub async fn execute_program(&self, program: &Program, session_id: &str, sequence: u64)
        -> Result<Option<Value>, String>;
    pub async fn shutdown(&self) -> Result<(), String>;
}
```

**In `execute_program`:** If `TENSOR_VM_MODE=ipc` is set, serialize the `Program` and dispatch to the VM worker instead of running locally. The `execute_program` signature stays the same — the IPC client handles the wire protocol internally.

**MCP server startup changes:**

```rust
// main.rs
let vm_mode = std::env::var("TENSOR_VM_MODE").unwrap_or_else(|_| "inprocess".to_string());
let vm_ipc_client = if vm_mode == "ipc" {
    let client = VmIpcClient::connect("/tmp/tensor_rag_vm.sock").await?;
    // Spawn VM worker if not already running
    let _vm_handle = tokio::process::Command::new("vm_worker")
        .arg("--socket=/tmp/tensor_rag_vm.sock")
        .spawn()?;
    Some(Arc::new(client))
} else {
    // Keep old direct VM
    None
};

// pass into SearchTool so it can choose dispatch path
```

### D3b — Session-aware IPC Routing

The VM worker needs to handle multiple concurrent sessions. Each `VmRequest::Execute` carries `session_id` and `sequence`. The VM worker maintains a `HashMap<session_id, VM>` so that state is isolated per session.

However, this raises the question: should the VM worker be session-scoped or per-connection? Two options:

**Option A: Stateless per request (simpler)**
- Each `VmRequest::Execute` contains the entire `Program` and all context (query vector, batch).
- VM is created per request, destroyed after.
- Pro: no session state to manage; crash in one request doesn't affect others.
- Con: no re-use of GPU context across requests from same session; higher latency.

**Option B: Session-scoped VM (more efficient)**
- VM Worker creates a `VM` instance per unique `session_id`.
- Pro: GPU context reused, batch cached across related requests.
- Con: state management, cleanup on disconnect, leak risk.

**Recommendation:** Option A for initial implementation. It's simpler and correct. GPU init overhead can be mitigated by keeping a single shared GPU context for all ephemeral VMs (the GPU context `Arc` is cloneable and stateless at the WGPU level).

### D3c — Startup Lifecycle

```
MCP Server starts
  │
  ├── TENSOR_VM_MODE=ipc ?
  │     YES:
  │       ├── Spawn vm_worker subprocess
  │       ├── Wait for Unix socket to appear (poll with timeout)
  │       ├── Create VmIpcClient
  │       └── Register shutdown handler (send VmRequest::Shutdown, wait, kill)
  │     NO:
  │       └── Use direct execute_program (current behavior)
  │
  ├── Register tools
  └── Start listening
```

Shutdown order:
1. MCP server stops accepting new requests.
2. `VmIpcClient::shutdown()` → sends `VmRequest::Shutdown`.
3. Wait for `VmResponse::ShutdownAck` (with 5s timeout).
4. `kill` the VM worker if still alive.
5. Clean up shared memory regions and socket file.

---

## 6. File Change Summary

| File | Action | Description |
|------|--------|-------------|
| `tensor-rag/src/bin/vm_worker.rs` | **CREATE** | New binary: Unix socket listener, shared memory I/O, VM execution loop |
| `tensor-rag/src/shared_memory.rs` | MODIFY | Add `MappedBuffer` with `mmap`/`munmap`, `read`/`write` serde helpers |
| `tensor-rag/src/mcp.rs` | MODIFY | Add `VmIpcClient` with `connect`, `execute_program`, `shutdown` methods |
| `tensor-rag/src/vm.rs` | MODIFY | Add `IpcValue` enum + conversions; add `Serialize`/`Deserialize` to `TopKResult`, `Value` (or add `IpcValue`) |
| `tensor-rag/src/ir.rs` | (no change) | Serde derives already present |
| `tensor-rag/Cargo.toml` | MODIFY | Add `bincode`, add `[[bin]]` section for `vm_worker` |
| `tensor-rag/src/main.rs` | MODIFY | Add IPC startup path, env var dispatch mode selection |
| `UNFINISHED_TASKS_AND_PLAN.md` | MODIFY | Mark D2/D3 progress |
| `/app/.brains/sessions/2026-06-02-0001/` | CREATE | Session tracking directory |

---

## 7. Testing Strategy

### Unit Tests

| Test | Location | What It Covers |
|------|----------|----------------|
| `shared_memory::test_mapped_buffer_write_read` | `shared_memory.rs` | `MappedBuffer` write + read round-trip with bincode |
| `shared_memory::test_cross_process_rw` | `shared_memory.rs` | Spawn child, write in parent, read in child (requires `#[cfg(test)]` subprocess) |
| `vm::test_ipc_value_roundtrip` | `vm.rs` | `IpcValue ↔ Value` conversion for all variants |
| `vm::test_ipc_value_arrow_array` | `vm.rs` | ArrowArray → FlatArray (Vec<f32>) conversion |
| `mcp::test_vm_ipc_client_connect` | `mcp.rs` | Connect to vm_worker socket, send ping, expect pong |
| `mcp::test_vm_ipc_client_execute` | `mcp.rs` | Full IPC round-trip: send Program, receive result |

### Integration Tests

| Test | Location | What It Covers |
|------|----------|----------------|
| `lib::test_e2e_pipeline_ipc` | `lib.rs` | Same as `test_e2e_pipeline` but with `TENSOR_VM_MODE=ipc` |
| `lib::test_vm_worker_crash_recovery` | `lib.rs` | Kill vm_worker mid-execution, verify MCP server survives and can restart worker |
| `lib::test_p99_latency_ipc` | `lib.rs` | Benchmark IPC path vs in-process; assert IPC overhead < 20% |

### How to Run

```bash
# Unit tests (no IPC needed)
cargo test -p tensor-rag --lib shared_memory::test
cargo test -p tensor-rag --lib vm::test
cargo test -p tensor-rag --lib mcp::test

# Integration tests (require vm_worker binary)
cargo test -p tensor-rag --test '*' -- --test-threads=1

# IPC overhead benchmark
TENSOR_VM_MODE=ipc cargo bench -- ipc_overhead
```

---

## 8. Dependencies Added

| Crate | Version | Reason |
|-------|---------|--------|
| `bincode` | 1.3 | Compact binary serde for IPC messages |
| `libc` | 0.2 | `mmap`/`munmap` syscall wrappers (if not doing it via `std::os::unix`) |

Note: `shared_memory.rs` could use the `memmap2` crate instead of raw `libc`, but the existing file-based approach works with `std::os::unix::io::AsRawFd` + `libc::mmap`. Adding `memmap2` would be cleaner.

---

## 9. Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| IPC latency exceeds P99 budget | User-facing slowdown | Keep in-process as default; IPC is opt-in via env var; benchmark gate in CI |
| GPU context not shareable across processes | No GPU acceleration in VM worker | Document limitation; CPU fallback; future: GPU context sharing via nvidia-peermem or CUDA IPC |
| Shared memory race condition | Data corruption | Single-threaded executor per connection; completion notification via socket before reading shm |
| VM worker crash during active request | Lost query | Return error to client; panic in IPC path is surfaced as `McpResponse::status = 500` |
| Recursive `IfConfidence` via IPC | Repeated serialization overhead | Flatten nested Programs in Planner (architectural change); or limit recursion depth |
| Orphaned shared memory on crash | Resource leak | Named regions in `/dev/shm` or tmpfs; cleanup on startup (remove stale files); watchdog in vm_worker to clean up on exit |

---

## 10. Future Extensions (Post-Phase D)

- **Arrow Flight integration:** Replace bincode for `RecordBatch` transfer with zero-copy Arrow IPC (Phase C dependency).
- **GPU context sharing:** Use `CUDA IPC` or `wgpu` external memory handles to share GPU buffers across processes.
- **Hot restart:** If vm_worker crashes, the MCP server detects the lost socket connection, cleans up, and spawns a new worker transparently.
- **Named worker pool:** Multiple VM worker processes with load-balanced dispatch for true parallelism.

---

## 11. Verification Checklist

- [ ] D2a — `vm_worker` binary compiles and accepts Unix socket connections
- [ ] D2b — `MappedBuffer` passes write/read round-trip test with bincode
- [ ] D2c — `IpcValue` conversion passes round-trip for all `Value` variants
- [ ] D3a — `VmIpcClient::execute_program` succeeds end-to-end with `vm_worker`
- [ ] D3b — Multiple sessions on same `vm_worker` do not interfere
- [ ] D3c — Shutdown sequence: graceful `Shutdown` → timeout → force kill
- [ ] Regression: `TENSOR_VM_MODE=inprocess` still works identically to today
- [ ] Performance: IPC path overhead measured and documented (target: <50µs per request)
- [ ] Integration: `test_e2e_pipeline` passes in both modes
- [ ] Integration: VM worker crash does not crash MCP server
