# Turbo-Calc: Comprehensive Remediation Plan

Generated from: `report.md` + `.brains/sessions/2026-05-29-0001/walkthrough/summary.md`

## Priority Legend

| Priority | Criteria |
|----------|----------|
| **P0** | Data loss, crash, or exploitable vulnerability |
| **P1** | Correctness bug or major regression |
| **P2** | Edge-case bug, missing validation, or meaningful gap |
| **P3** | Maintainability, precision, or minor issue |

---

## Phase 0: Critical Security Fixes (P0) — Do First

### 0.1 Enforce max payload size on server (`server.lisp:85-167`)

**Problem:** No limit on incoming payload size → arbitrary OOM via `(make-array N)`.

**Fix:**
- Add `*max-payload-bytes*` constant (e.g., 10 MB for graphs/matrices, 1 KB for eval strings).
- Check `payload-size` in `process-request` before dispatching to handlers.
- Reject oversized payloads with a protocol error response.

**Files to modify:** `router/server.lisp`
**Verification:** Send a 100 MB payload → expect immediate rejection + no heap spike.

### 0.2 Add socket I/O timeouts (`server.lisp:38-56`, `server.lisp:189-210`)

**Problem:** No timeouts on `socket-accept`, `socket-receive`, or the APL coprocessor proxy → indefinite block.

**Fix:**
- Set `sb-bsd-sockets:socket-receive-timeout` and `sb-bsd-sockets:socket-send-timeout` on all sockets (e.g., 30s default).
- Wrap `run-apl-coprocessor` in `with-timeout` from `sb-ext` (or a `handler-case` + `bt:make-thread` with timeout).
- Log and close hung connections.

**Files to modify:** `router/server.lisp`
**Verification:** Connect and send no data → connection times out in ≤30s.

### 0.3 Limit thread-per-connection (`server.lisp:189-210`)

**Problem:** Unbounded thread creation → OS resource exhaustion under load.

**Fix:**
- Introduce a bounded thread pool (e.g., `(lparallel:make-kernel N)`) or a simple semaphore with `*max-connections*`.
- Reject new connections when pool is exhausted.

**Files to modify:** `router/server.lisp`
**Verification:** Connect 1000 clients simultaneously → no more than `*max-connections*` threads active.

### 0.4 Harden safe-eval: remove `expt` and limit recursion (`safe_eval.lisp:15-28`)

**Problem:** `expt` in the whitelist enables easy DoS (`(expt 2 10000000)`). `read-from-string` allows circular structures.

**Fix:**
- Remove `expt` from `*math-functions*` whitelist.
- Replace `read-from-string` with a hand-written recursive-descent parser that rejects circular syntax (`#1=#1#`, `#n#`, etc.) and limits nesting depth (e.g., 64 levels).
- Add a time limit (via `sb-ext:with-timeout`) around the entire eval call.

**Files to modify:** `router/safe_eval.lisp`
**Verification:** `(expt 2 1000000)` fails with a clear error; `#1=(#1#)` does not hang.

### 0.5 Add max-size check on client response (`client/main.lua`)

**Problem:** `ffi.new` allocates based on server's declared payload size, unchecked → client-side OOM.

**Fix:**
- Before `ffi.new("double[?]", n)`, cap at `MAX_RESPONSE_SIZE` (e.g., 100 MB).
- Close connection and error if exceeded.

**Files to modify:** `client/main.lua`
**Verification:** Malicious server sends size=10⁹ → client rejects before allocation.

---

## Phase 1: Correctness Bugs (P1) — Fix Next

### 1.1 Fix `pack-graph-payload` predecessors (`protocol.lisp:195-206`)

**Problem:** Predecessor array is populated with `dist` values instead of actual predecessor nodes. Serialized as `double-to-bytes` which is lossy for integer data.

**Fix:**
- Return `predecessors` from `dijkstra` (see also 1.3) alongside `distances`.
- Serialize predecessors as `(integer-to-bytes ... n-bytes 4 :little-endian)` instead of `double-to-bytes`.

**Files to modify:** `router/protocol.lisp`, `router/graph.lisp`
**Verification:** Build a small graph, request shortest paths, verify predecessor array contains valid vertex IDs (not distances).

### 1.2 Verify CRC16 on client (`client/main.lua:90`)

**Problem:** CRC is read (as 32-bit LE with `string.unpack`) but never validated against computed CRC of payload.

**Fix:**
- After receiving response, compute CRC16-CCITT over the payload bytes.
- Compare against the received CRC (extracted as 16-bit LE).
- On mismatch, log error and optionally retry.

**Files to modify:** `client/main.lua`, `client/protocol.lua`
**Verification:** Corrupt a byte in a recorded response → client detects CRC mismatch.

### 1.3 Fix Dijkstra CSR end-index (`graph.lisp:64`)

**Problem:** `(1+ u)` used as end index for adjacency row works only for 0-indexed CSR with contiguous vertices. Isolated nodes cause incorrect access.

**Fix:**
- Use `(aref row-ptr (1+ u))` corrected logic: iterate `(aref row-ptr u)` to `(1- (aref row-ptr (1+ u)))` and handle `(= (aref row-ptr u) (aref row-ptr (1+ u)))` as empty row.

**Files to modify:** `router/graph.lisp`
**Verification:** Graph with isolated vertex 5 → query path to/from 5 returns "no path", not garbage.

### 1.4 Fix APL → Lisp protocol mismatch (`coprocessor/matrix.apl`, `server.lisp`)

**Problem:** Lisp sends raw binary TCBP payload; APL reads via `⎕FIO[3]` (text-oriented). Null bytes and newlines corrupt data. `process` returns `⍬` (stub).

**Fix:**
- Rewrite APL coprocessor to use `⎕FIO[26]` (binary read) and `⎕FIO[27]` (binary write).
- Implement proper TCBP message framing in APL: read 5-byte header (opcode+size), then N-byte payload, then 2-byte CRC.
- Replace the stub with actual `A +.× A` matrix multiply on decoded IEEE 754 doubles.
- Add `:Trap 0` error handling so invalid data doesn't crash APL.

**Files to modify:** `coprocessor/matrix.apl`
**Verification:** Send a 3×3 matrix from router → APL returns `A +.× A` correctly encoded as TCBP.

---

## Phase 2: Performance Regressions (P2) — Fix After Correctness

### 2.1 Implement binary heap Dijkstra (`graph.lisp`)

**Problem:** O(V²) naive Dijkstra blocks server thread on large graphs.

**Fix:**
- Replace unsorted-array distance scan with a binary heap (or use `priority-queue` from alexandria or cl-heap).
- Complexity: O((V+E) log V).

**Files to modify:** `router/graph.lisp`
**Verification:** 10K-node CSR graph returns in <1s (previously would take minutes).

### 2.2 Implement FFT for all sizes (`dsp.lisp`)

**Problem:** DFT is O(n²) for non-power-of-2 inputs → blocks on large arrays.

**Fix:**
- Implement Bluestein's algorithm for arbitrary-length FFT, or zero-pad to next power-of-2 and apply Cooley-Tukey.
- Benchmark: FFT length 1001 should not take 1000x longer than length 1024.

**Files to modify:** `router/dsp.lisp`
**Verification:** DFT of length 1001 completes in comparable time to length 1024 (not 1000x slower).

### 2.3 Add keep-alive to Lua client (`client/main.lua:41-96`)

**Problem:** New TCP connection per request → port exhaustion (TIME_WAIT) + ~1ms latency per request.

**Fix:**
- Reuse a single persistent TCP connection across requests.
- Add connection health check (send heartbeat or recover from closed socket by reconnecting).
- Close connection on client exit.

**Files to modify:** `client/main.lua`
**Verification:** 1000 sequential requests → only 1 TCP connection established (netstat shows 1 CLOSE_WAIT at end).

### 2.4 Fix O(n²) string concat in Lua client (`client/main.lua`)

**Problem:** `str = str .. chunk` in loops causes O(n²) memory churn.

**Fix:**
- Use `table.concat(buf)` for packing payload.
- Replace `string.sub(data, i, i+7)` loop with `ffi.copy(dest, data + offset, 8)` for binary unpacking.

**Files to modify:** `client/main.lua`, `client/protocol.lua`
**Verification:** Packing a 10K-node graph is <10ms (previously >1s).

### 2.5 Replace global Prolog mutex with pool (`server.lisp`)

**Problem:** Global mutex serializes all constraint-solving requests → bottleneck.

**Fix:**
- Pre-spawn N SWI-Prolog processes at startup (a process pool).
- Assign incoming solve requests to idle processes round-robin.
- Kill and restart a process if it hangs (with timeout).

**Files to modify:** `router/server.lisp`
**Verification:** 10 concurrent solve requests complete in ~same time as 1 (not 10x slower).

---

## Phase 3: Precision & Correctness (P2/P3) — Fix After Performance

### 3.1 Fix qsim `long-float` truncation (`protocol.lisp:229-230`)

**Problem:** Qsim computes in `long-float` (80-bit) but serializes as `double-float` (64-bit), discarding precision.

**Fix:**
- Serialize qsim results in `long-float` format (10 bytes IEEE 754 extended precision) or rational format.
- Update Lua client to deserialize `long-float` via FFI.
- Alternatively, compute qsim in `double-float` throughout (consistent, not silently lossy).

**Files to modify:** `router/protocol.lisp`, `router/qsim.lisp`
**Verification:** Qsim output matches input precision: 80-bit input → 80-bit output.

### 3.2 Validate matrix shape and tonumber in Lua (`client/main.lua`)

**Problem:** `tonumber` returning `nil` is silently replaced with `0.0`. Ragged matrix rows are silently truncated.

**Fix:**
- After `tonumber`, check for `nil` and raise a descriptive error (including row/col position).
- Before serializing, validate all rows have the same length; reject ragged matrices.

**Files to modify:** `client/main.lua`
**Verification:** Input `"[[1,2],[3]]"` → error "Row 2 has 1 columns, expected 2".

### 3.3 Validate response opcode in Lua (`client/main.lua`)

**Problem:** Client reads first byte but does not verify it matches expected opcode.

**Fix:**
- After receiving, compare response opcode byte against the request opcode.
- On mismatch, log and surface the server's error message.

**Files to modify:** `client/main.lua`
**Verification:** Send eval request, make server return matrix opcode → client logs protocol error.

### 3.4 Fix graph predecessor serialization type (`protocol.lisp:195-206`) — full fix

If predecessor values exceed 2^32-1, 4-byte LE is insufficient. Replace with variable-length encoding or always use 8-byte LE.

**Files to modify:** `router/protocol.lisp`

---

## Phase 4: Documentation & Maintainability (P3) — Fix Last

### 4.1 Correct README to match code capabilities

**Problem:** README claims ~40-50% fictional features (`diffeq`, `fem`, `tensor`, `stoch`, LLVM compilation, BLAS/LAPACK, HTML generation, sandboxing).

**Fix:**
- Remove or mark as "planned" all non-existent commands.
- Remove fabricated benchmarks (Section 9.1).
- Correct architecture claims (Section 1, 8, 9, 10, 13).
- Add accurate protocol table (remove opcodes 0x09-0x0B).

**Files to modify:** `README.md`

### 4.2 Add missing tests

**Test gaps identified:**
- CRC verification (client + server)
- Max payload size enforcement
- Timeout behavior
- APL coprocessor protocol (when fixed)
- Edge case: empty graph, 1-node graph, disconnected graph
- Edge case: qsim negative amplitudes
- DoS attempts (expt, oversized payloads)

**Files to create/modify:** `tests/`

### 4.3 Add English-language headers or docstrings to all source files

Code comments are in Russian; add English function-level docstrings for maintainability.

---

## Rollback Strategy

Before making changes:
1. Ensure the single commit is tagged or branch-created.
2. Each phase can be rolled back independently:
   - **Phase 0** changes are local to `server.lisp`, `safe_eval.lisp`, `client/main.lua` — revert with `git checkout`.
   - **Phase 1** involves APL and graph — verify with existing tests before proceeding.
   - **Phase 2** is pure performance — no functional change expected.
3. Run the test suite after each phase:
   ```bash
   sbcl --load tests/test_router.lisp
   luajit tests/test_client.lua
   python tests/ab_test_e2e.py
   ```

---

## Verification Checklist (Final)

| Check | Command/Procedure |
|-------|------------------|
| All tests pass | `sbcl --load tests/test_router.lisp && luajit tests/test_client.lua` |
| No OOM on 100MB payload | `python -c "import socket; s=socket.create_connection(('127.0.0.1',8443)); s.send(b'\x01' + (10**8).to_bytes(4,'little') + b'A'*10**8); print(s.recv(1024))"` → connection reset or error |
| No thread explosion | `for i in {1..1000}; do nc -z 127.0.0.1 8443 & done; sleep 1; ps -eLf | grep sbcl | wc -l` ≤ `*max-connections*` |
| E2E graph shortest path | Client sends graph → receives valid distances + predecessor array |
| CRC detection works | Corrupt one byte in a recorded TCBP response → Lua client rejects |
| APL multiply works | Router sends matrix → APL returns correct product |
| Keep-alive confirmed | `strace -e network luajit client/main.lua graph ... 2>&1 | grep connect` shows 1 connect call |
| No `expt` DoS | Client sends eval `(expt 2 1000000)` → error, no CPU spike |
