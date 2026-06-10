# APL-π-φ Bridge Design Document

## Connecting formulas.py to the Hybrid v7+v10 Engine via TCBP

---

## 1. Architecture Overview

```
┌─────────────────────────────────────────────────────┐
│  Python Bridge Layer                                │
│                                                      │
│  ┌─────────────┐  ┌──────────────┐  ┌────────────┐ │
│  │ tcbp.py     │  │ client.py    │  │formula_     │ │
│  │ (binary     │→│ (high-level  │→│orchestrator │ │
│  │  protocol)  │  │  numpy API)  │  │ (formulas   │ │
│  └──────┬──────┘  └──────┬───────┘  │  bridge)    │ │
│         │               │           └──────┬───────┘ │
│         │               │                  │         │
│         └───────────────┼──────────────────┘         │
│                         │ workflow.py                │
│                         │ (orchestration)            │
│                         └───────────────────────────┘
│                                        │
└────────────────────────────────────────┼────────────────
                                         │ TCP (port 8443)
                                         ▼
┌───────────────────────────────────────────────────────────┐
│  v10 Lisp Router (TCBP Server)                           │
│                                                           │
│  ┌──────────────┐   ┌───────────────┐   ┌──────────────┐ │
│  │ Safe Eval    │   │ Prolog Pool   │   │ DSP / QSim   │ │
│  │ (Lisp)       │   │ (SWI-Prolog)  │   │ (Lisp)       │ │
│  └──────────────┘   └───────┬───────┘   └──────────────┘ │
│                             │                             │
│  ┌──────────────────────────┴──────────────────────────┐ │
│  │  Opcode Dispatcher (process-request)                 │ │
│  │  0x01=Eval  0x02=Matrix  0x03=Solve  0x04=Sym       │ │
│  │  0x05=Graph  0x06=DSP    0x07=QSim   0x08=CSP       │ │
│  │  0x09=Evolve 0x0A=Censor 0x0B=Formula (v7 ext.)     │ │
│  └──────────────────────┬───────────────────────────────┘ │
└─────────────────────────┼─────────────────────────────────┘
                          │ port 8444
                          ▼
┌──────────────────────────────────────────────────────────┐
│  APL Coprocessor (GNU APL) — matrix multiply via +.×     │
└──────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────┐
│  v7 Cognitive Engine (Lisp + Prolog + Sonata AVX2)       │
│  Future: TCBP server on port 8445                        │
│  Handles: AST evolution, censor checks, formula-guided   │
│           evolution, regression (AVX2 via Sonata)         │
└──────────────────────────────────────────────────────────┘
```

---

## 2. Module Signatures

### 2.1 `bridge/tcbp.py` — Binary TCBP Client

```python
def crc16_ccitt_false(data: bytes) -> int
    """CRC-16/CCITT-FALSE: poly=0x1021, init=0xFFFF, no reflec, no final xor."""

class Opcode:
    EVAL    = 0x01   # Safe arithmetic eval
    MATRIX  = 0x02   # Tensor/matrix operations → APL
    SOLVE   = 0x03   # Prolog CLP(FD) solver
    SYM     = 0x04   # Symbolic differentiation
    GRAPH   = 0x05   # Dijkstra on CSR graphs
    DSP     = 0x06   # DFT/FFT on complex vector
    QSIM    = 0x07   # Quantum state simulation
    CSP     = 0x08   # Prolog constraint satisfaction
    EVOLVE  = 0x09   # v7 AST evolution
    CENSOR  = 0x0A   # v7 Prolog censor check
    FORMULA = 0x0B   # formulas.py eval on server

class TcbpResponse:
    opcode: int
    payload: bytes
    def as_text(self) -> str
    def as_float64(self) -> float
    def as_matrix(self) -> tuple[list[int], list[float]]
    def as_complex(self) -> tuple[list[float], list[float]]
    def as_evolve(self) -> dict

class TcbpClient:
    def __init__(self, host='127.0.0.1', port=8443, connect_timeout=5.0)
    def connect(self)
    def close(self)
    def send_frame(self, opcode: int, payload: bytes) -> TcbpResponse
    def call_eval(self, expression: str) -> str
    def call_matrix(self, shape: list[int], data: list[float]) -> tuple
    def call_solve(self, equations: str) -> str
    def call_sym(self, function: str, variable: str) -> str
    def call_graph(self, num_rows, num_cols, row_ptrs, col_indices, data) -> tuple
    def call_dsp(self, real: list[float], imag: list[float]) -> tuple
    def call_qsim(self, real: list[float], imag: list[float]) -> tuple
    def call_csp(self, constraints: str) -> str
    def call_evolve(self, domain: str, ast_str: str, iterations=500, flags=0) -> dict
    def call_censor(self, domain: str, ast_str: str) -> float
```

### 2.2 `bridge/client.py` — High-Level NumPy API

```python
class ApiClient:
    def __init__(self, host='127.0.0.1', port=8443, v7_port=None)
    def close(self)

    # Core math operations → v10 router
    def eval_expr(self, expression: str) -> str
    def matrix_multiply(self, A: np.ndarray, B: np.ndarray) -> np.ndarray
    def solve_constraints(self, equations: str) -> str
    def qsim_circuit(self, state_vector: np.ndarray) -> np.ndarray
    def dsp_fft(self, signal: np.ndarray) -> np.ndarray
    def symbolic_diff(self, func: str, var: str) -> str
    def shortest_path(self, adj_matrix: np.ndarray, source=0) -> tuple

    # v7 evolution operations → v7 engine over TCBP
    def evolve_ast(self, domain: str, seed_ast: str,
                   iterations=500, flags=0) -> dict
    def censor_ast(self, domain: str, ast: str) -> float
```

### 2.3 `bridge/formula_orchestrator.py` — Formulas ↔ v7 Bridge

```python
class FormulaBridge:
    def __init__(self, client: ApiClient, formulas_module=None)

    # Censor wrappers
    def born_censor(self, ast, query, docs) -> float
    def weak_measure_censor(self, ast, rho, projector, lam=0.17) -> float
    def entropy_censor(self, ast, vectors, weights=None) -> float
    def gleason_censor(self, ast, rho, target) -> float

    # Hybrid evolution
    def evolve_with_scoring(self, domain, seed_ast, scoring_fn, iterations=500) -> dict
    def evolve_with_born(self, domain, seed_ast, query, docs, iterations=500) -> dict
    def evolve_with_weak_measure(self, domain, seed_ast, rho, projector, lam=0.17, iterations=500) -> dict
    def verify_result(self, ast, domain, domain_info=None) -> dict
```

### 2.4 `bridge/workflow.py` — Orchestration Scripts

```python
def workflow_born_v7(query=None, docs=None, host='127.0.0.1', port=8443)
    """Born probability → TCBP → v7 evolution → verify."""

def workflow_weak_measure_v7(host='127.0.0.1', port=8443)
    """weak_measurement as alternative censor."""

def workflow_full_roundtrip(host='127.0.0.1', port=8443)
    """Complete: formulas → TCBP → Prolog → evolution → verify."""

def workflow_censor_compare(host='127.0.0.1', port=8443)
    """Compare Prolog vs formulas.py censor outputs."""
```

---

## 3. TCBP Binary Frame Format

### 3.1 Generic Frame

| Offset | Size | Field | Encoding |
|--------|------|-------|----------|
| `0x00` | 1 | Opcode | `UInt8` |
| `0x01` | 4 | Payload Length N | `UInt32` Little-Endian |
| `0x05` | N | Payload | See §3.2–3.10 |
| `0x05+N` | 2 | CRC16 | `UInt16` LE, CCITT-FALSE |

**CRC16-CCITT (CCITT-FALSE):**
- Polynomial: `0x1021` (x¹⁶ + x¹² + x⁵ + 1)
- Initial value: `0xFFFF`
- No reflection on input or output
- No final XOR
- Computed over `[opcode(1) + payload_len(4) + payload(N)]` — i.e. header+payload, NOT including the crc field itself.
- Reference: matches `crc16` in `v10/router/protocol.lisp`

### 3.2 Matrix Payload (Opcode 0x02)

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
┌─────┬─────────────────────────────────────────────────────────────┐
│Ndims│ Dim[0] (U32 LE)           │ Dim[1] (U32 LE)          ...   │
├─────┴─────────────────────────────────────────────────────────────┤
│ ... │ Data[0] (Float64 LE)                                       │
├─────┬─────────────────────────────────────────────────────────────┤
│ Data[1] (Float64 LE)                    ...                       │
└───────────────────────────────────────────────────────────────────┘
```

- `Ndims`: 1 byte, number of dimensions
- `Dim[i]`: 4 bytes each, `UInt32` LE, size of each dimension
- `Data`: `∏ Dim[i] × 8` bytes, row-major `Float64` LE

**Example:** A 3×4 matrix = 1 byte ndims + 2×4 bytes dims + 12×8 bytes data = 105 bytes payload.

### 3.3 Complex/Quantum Payload (Opcodes 0x06 DSP, 0x07 QSim)

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
┌───────────────────────────────────────────────────────────────────┐
│ NumElems (U32 LE)                                                 │
├───────────────────────────────────────────────────────────────────┤
│ Re[0] (Float64 LE)                                                │
├───────────────────────────────────────────────────────────────────┤
│ Im[0] (Float64 LE)                                                │
├───────────────────────────────────────────────────────────────────┤
│ Re[1] (Float64 LE)                                                │
├───────────────────────────────────────────────────────────────────┤
│ Im[1] (Float64 LE)                                                │
├───────────────────────────────────────────────────────────────────┤
│ ...                                                               │
└───────────────────────────────────────────────────────────────────┘
```

- `NumElems`: 4 bytes, `UInt32` LE
- `Re[i]`, `Im[i]`: 8 bytes each, `Float64` LE
- Total payload: `4 + N × 16` bytes

### 3.4 CSR Sparse Matrix Payload (Opcode 0x05 Graph)

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
┌───────────────────────────────────────────────────────────────────┐
│ NumRows (U32 LE)                                                  │
├───────────────────────────────────────────────────────────────────┤
│ NumCols (U32 LE)                                                  │
├───────────────────────────────────────────────────────────────────┤
│ RowPtr[0] (U32 LE)                                                │
├───────────────────────────────────────────────────────────────────┤
│ RowPtr[1] (U32 LE)                                                │
├───────────────────────────────────────────────────────────────────┤
│ ... (total NumRows+1 entries)                                     │
├───────────────────────────────────────────────────────────────────┤
│ ColIdx[0] (U32 LE)                                                │
├───────────────────────────────────────────────────────────────────┤
│ ... (total NNZ = RowPtr[NumRows] entries)                         │
├───────────────────────────────────────────────────────────────────┤
│ Data[0] (Float64 LE)                                              │
├───────────────────────────────────────────────────────────────────┤
│ ... (total NNZ entries)                                           │
└───────────────────────────────────────────────────────────────────┘
```

### 3.5 Graph Response Payload (Opcode 0x05 Response)

```
┌───────────────────────────────────────────────────────────────────┐
│ NumElems (U32 LE)                                                 │
├───────────────────────────────────────────────────────────────────┤
│ Dist[0] (Float64 LE)                                              │
├───────────────────────────────────────────────────────────────────┤
│ Pred[0] (Float64 LE — cast from integer, lossy)                   │
├───────────────────────────────────────────────────────────────────┤
│ Dist[1] (Float64 LE)                                              │
├───────────────────────────────────────────────────────────────────┤
│ ...                                                               │
└───────────────────────────────────────────────────────────────────┘
```

**Note:** `predecessors` are integers but serialised as doubles (`protocol.lisp:204`). This is a known precision issue in v10.

### 3.6 Eval / Solve / Sym / CSP Payloads (Opcodes 0x01, 0x03, 0x04, 0x08)

Payload is a plain UTF-8 string. For `Sym`, format is `"function,variable"` (comma-separated, per `server.lisp:172-173`). Response is also UTF-8 string.

### 3.7 v7 Evolve Payload (Opcode 0x09)

```
┌───────────────────────────────────────────────────────────────────┐
│ Version (U32 LE) = 1                                              │
├───────────────────────────────────────────────────────────────────┤
│ DomainLen (U8)                                                    │
├───────────────────────────────────────────────────────────────────┤
│ Domain (UTF-8, DomainLen bytes)                                   │
├───────────────────────────────────────────────────────────────────┤
│ ASTLen (U16 LE)                                                   │
├───────────────────────────────────────────────────────────────────┤
│ AST (UTF-8, ASTLen bytes)                                         │
├───────────────────────────────────────────────────────────────────┤
│ Iterations (U32 LE)                                               │
├───────────────────────────────────────────────────────────────────┤
│ Flags (U16 LE)                                                    │
└───────────────────────────────────────────────────────────────────┘
```

**Flags:**
- Bit 0 (0x01): Enable novelty search mode
- Bit 1 (0x02): Enable Sonata AVX2 acceleration (regression)
- Bit 2 (0x04): Enable Python-side formula scoring callback

**Response:**

```
┌───────────────────────────────────────────────────────────────────┐
│ Version (U32 LE) = 1                                              │
├───────────────────────────────────────────────────────────────────┤
│ BestASTLen (U16 LE)                                               │
├───────────────────────────────────────────────────────────────────┤
│ BestAST (UTF-8, BestASTLen bytes)                                 │
├───────────────────────────────────────────────────────────────────┤
│ BestPenalty (Float64 LE)                                          │
├───────────────────────────────────────────────────────────────────┤
│ TotalEvaluations (U32 LE)                                         │
├───────────────────────────────────────────────────────────────────┤
│ ElapsedSeconds (Float64 LE)                                       │
└───────────────────────────────────────────────────────────────────┘
```

### 3.8 v7 Censor Payload (Opcode 0x0A)

**Request:**

```
┌───────────────────────────────────────────────────────────────────┐
│ Version (U32 LE) = 1                                              │
├───────────────────────────────────────────────────────────────────┤
│ DomainLen (U8)                                                    │
├───────────────────────────────────────────────────────────────────┤
│ Domain (UTF-8, DomainLen bytes)                                   │
├───────────────────────────────────────────────────────────────────┤
│ ASTLen (U16 LE)                                                   │
├───────────────────────────────────────────────────────────────────┤
│ AST (UTF-8, ASTLen bytes)                                         │
└───────────────────────────────────────────────────────────────────┘
```

**Response:** Single `Float64` LE (8 bytes) = penalty value.

### 3.9 Opcode Summary

| Opcode | Name | Payload In | Payload Out | Target |
|--------|------|------------|-------------|--------|
| 0x01 | Eval | UTF-8 string | UTF-8 string | Lisp safe-eval |
| 0x02 | Matrix | Matrix binary (Ndims+Dims+Data) | Matrix binary | APL coprocessor |
| 0x03 | Solve | UTF-8 equations | UTF-8 result | Prolog CLP(FD) |
| 0x04 | Sym | UTF-8 "func,var" | UTF-8 derivative | Lisp |
| 0x05 | Graph | CSR binary | Dist+Pred binary | Lisp Dijkstra |
| 0x06 | DSP | Complex binary | Complex binary | Lisp DFT/FFT |
| 0x07 | QSim | Complex binary | Complex binary | Lisp simulator |
| 0x08 | CSP | UTF-8 constraints | UTF-8 result | Prolog CLP(FD) |
| **0x09** | **Evolve** | **Version+Domain+AST+Iter+Flags** | **Version+BestAST+Penalty+Evals+Time** | **v7 engine** |
| **0x0A** | **Censor** | **Version+Domain+AST** | **Float64 penalty** | **v7 Prolog** |
| 0x0B | Formula | (reserved for server-side formulas.py) | TBD | — |

---

## 4. Formulas.py → TCBP Opcode Mapping

| formulas.py function | TCBP Opcode | Notes |
|----------------------|-------------|-------|
| `inner_product(a,b)` | → locally (or 0x02 Matrix) | Dot product is a special case of matrix multiply |
| `cosine_sim(a,b)` | → locally (or 0x02 Matrix post-process) | Normalise + dot |
| `born_probability(q,d)` | 0x0A Censor | Can be a custom censor using formula values |
| `qprp_interference(probs,phase)` | → locally | Multi-step: matrix + local interference |
| `weak_measurement(rho,P,lam)` | 0x0A Censor | Alternative quantum censor |
| `mrl_progressive_measurement(q,corpus,...)` | → locally | Python loop, no remote needed |
| `density_matrix(vecs,wts)` | → locally | Simple outer-product sum |
| `von_neumann_entropy(rho)` | → locally | Eigenvalue-based |
| `gleason_score(rho,doc)` | → locally | Trace-based |
| `kraus_channel(rho,ops)` | → locally | Matrix operations |
| `two_block_collisions(M)` | → locally | π digits via collision count |
| `phi_hierarchy(start,depth)` | → locally | φ-based dimension sequence |
| `kahan_sum(values)` | → locally | Compensated summation |
| `acos_stable(cosθ)` | → locally | Haversine-stabilised acos |
| `convert_space(vec,from,to)` | → locally | Vector space projection |
| `dim_mutate(vec,target_dim)` | → locally | Dimension phase mutation |
| `check_associative(op,a,b,c)` | → locally | Symbolic verification |
| `check_distributive(mul,add,a,b,c)` | → locally | Symbolic verification |
| **Matrix multiply** | **0x02** | Only operation that truly needs APL |
| **Solve constraints** | **0x03/0x08** | Delegate to Prolog CLP(FD) |
| **FFT** | **0x06** | Delegate to Lisp DFT |
| **QSim circuit** | **0x07** | Delegate to Lisp qsim |
| **Evolve AST** | **0x09** | Delegate to v7 engine |
| **Censor AST** | **0x0A** | Delegate to v7 Prolog |

**Design principle:** Only vector/matrix operations that benefit from APL's array acceleration or Prolog's logical inference are sent over TCBP. Most formulas.py functions run locally in Python — they are domain-specific formulas, not general-purpose math.

---

## 5. Formula Bridge: formulas.py as a v7 Censor

### 5.1 The Problem

v7's censors are SWI-Prolog rules. They check:
- Quantum: unitarity, fidelity, Grover target
- Arithmetic: Occam's razor, algebraic simplification
- Relativity: v < c, Lorentz factor bounds
- Thermodynamics: T > 0, Carnot efficiency
- Holography: Beckenstein bound S ≤ A/4

What if we want to use **Born probability**, **weak measurement**, or **von Neumann entropy** as the censor instead of (or in addition to) Prolog's own rules?

### 5.2 The Solution: FormulaBridge

```
  v7 Engine                    FormulaBridge (Python)
 ┌──────────┐    AST string    ┌─────────────────────┐
 │ Mutator  │────────────────→│ born_censor(ast)     │
 │ (Lisp)   │                 │   → parse AST        │
 └──────────┘                 │   → extract state     │
      ↑                       │   → P(q|d) = cos²    │
      │ penalty (float)       │   → (1-maxP)*1000    │
      └───────────────────────│───────────────────────┘
```

The bridge:
1. Receives the AST as a string from v7 (via TCBP opcode 0x09)
2. Optionally evaluates it using formulas.py
3. Returns a penalty float (0.0 = perfect, 1000.0 = reject)

### 5.3 Use Cases

| Use Case | Python Function | What It Replaces |
|----------|----------------|------------------|
| Quantum retrieval scoring | `born_probability` | Prolog `quantum_censor` fidelity metric |
| Partial-collapse censor | `weak_measurement` | Prolog unitary check |
| Query ambiguity metric | `von_neumann_entropy` | Prolog size-based penalty |
| Document-projector scoring | `gleason_score` | Prolog trace-based check |
| Progressive measurement | `mrl_progressive_measurement` | Multi-phase Prolog evaluation |
| Channel-based reranking | `kraus_channel` | — (new) |

### 5.4 Integration Path

**Phase 1 (Current):** Python calls both v7 (via TCBP) and formulas.py locally. FormulaBridge runs AFTER the v7 evolution as a verification layer.

**Phase 2 (Planned):** Extend the v10 Lisp router to forward censor requests to Python via a callback socket (opcode 0x09 flag bit 2). The v7 server calls Python's formula bridge in the censor loop.

**Phase 3 (Future):** Embed selected formulas.py functions as native Prolog rules (translated to `quantum.pl` syntax) for sub-millisecond censor latency.

---

## 6. Complete Round-Trip Example

### Scenario: Quantum Bell-State Discovery with Born-Rule Verification

```
Step 1 [Python]: Compute Born probabilities for query vs docs
       formula: born_probability(query=[1,0,0,0], doc=[0.707,0.707,0,0])
       result: P = 0.50

Step 2 [Python]: Build TCBP frame for v7 evolution
       opcode: 0x09 (EVOLVE)
       payload:
         version=1
         domain="quantum"
         seed_ast="(seq (h q1) (cnot q1 q2))"
         iterations=1000
         flags=0
       CRC = crc16_ccitt_false(frame without CRC)

Step 3 [Python → Lisp Router]: Send on port 8443
       [0x09][0xE8 0x03 0x00 0x00][payload...][0xXX 0xXX]

Step 4 [Lisp Router]: process-request
       - Read header (5 bytes)
       - Read payload (payload_len bytes)
       - Verify CRC16
       - Dispatch by opcode = 0x09 → v7 evolve handler

Step 5 [Lisp → v7 Engine]: 
       - Lisp spawns v7 worker thread (or connects to v7 daemon)
       - v7 sets domain='quantum', seed=AST
       - v7 mutator generates candidate ASTs
       - Each candidate is sent to Prolog censor via pipe
       - Prolog validates unitarity, computes fidelity
       - Results fed back to mutation loop

Step 6 [v7 Engine → Lisp → Python]:
       - v7 converges on best AST
       - Lisp packs evolve response:
         [best_ast_len 2B][best_ast][penalty 8B][evals 4B][elapsed 8B]
       - Pack TCBP frame: [opcode][len][response][CRC16]
       - Send to Python

Step 7 [Python]: Receive and decode
       - Verify CRC
       - Parse response
       - best_ast = "(seq (h q1) (cnot q1 q2))"
       - best_penalty = 0.0

Step 8 [Python]: Verify with formulas.py
       - Build density matrix from result
       - Compute von Neumann entropy
       - Compute Gleason score against target
       - Report verification pass/fail

Total wire bytes (approximate):
       Request:  1 + 4 + (4+1+7+2+36+4+2) + 2 = 63 bytes
       Response: 1 + 4 + (4+2+36+8+4+8) + 2 = 69 bytes
```

---

## 7. Endianness, CRC, and Wire Format Summary

| Property | Value |
|----------|-------|
| **Byte order** | Little-Endian (all multi-byte fields) |
| **CRC polynomial** | `0x1021` (CCITT) |
| **CRC init** | `0xFFFF` |
| **CRC refin** | false |
| **CRC refout** | false |
| **CRC xorout** | `0x0000` |
| **CRC check** | `crc16(b"123456789")` = `0x29B1` |
| **TCBP port (v10)** | 8443 |
| **TCBP port (v7 future)** | 8445 |
| **APL coprocessor port** | 8444 |
| **Float encoding** | IEEE 754 binary64 (Float64) |
| **Integer encoding** | UInt8 / UInt16 / UInt32 LE |
| **String encoding** | UTF-8 |
| **Max payload** | 10 MiB (enforced by v10 server.lisp `*max-payload-bytes*`) |

### CRC Python Implementation

```python
def crc16_ccitt_false(data: bytes) -> int:
    """CRC-16/CCITT-FALSE -- matches protocol.lisp exactly."""
    crc = 0xFFFF
    for byte in data:
        crc ^= (byte << 8)
        for _ in range(8):
            if crc & 0x8000:
                crc = ((crc << 1) ^ 0x1021) & 0xFFFF
            else:
                crc = (crc << 1) & 0xFFFF
    return crc
```

---

## 8. v7 TCBP Integration Path

### Current State (v7 engine.lisp)

v7 uses **OS pipes** to communicate with Prolog censors:

```lisp
;; engine.lisp:442-467
(let ((query (format nil "censor(~A, ~A, ~A, Penalty)." mode domain prolog-term)))
  (format (uiop:process-info-input proc) "~A~%" query)
  (force-output ...)
  (read-line (uiop:process-info-output proc)))
```

### Proposed TCBP Wrapper for v7

Instead of pipes, v7 should expose a TCBP server on port 8445:

```lisp
;; In a future v7-tcbp-server.lisp:
(defun start-v7-tcbp-server (&optional (port 8445))
  (let ((socket (make-instance 'inet-socket ...)))
    (socket-bind socket #(127 0 0 1) port)
    (socket-listen socket 5)
    (loop (let ((client (socket-accept socket)))
            (sb-thread:make-thread
              (lambda ()
                (handle-v7-request client)))))))

(defun handle-v7-request (stream)
  ;; Read TCBP frame
  ;; Dispatch to v7 engine (run-engine-parallel or check-with-prolog)
  ;; Pack result as TCBP response
  )
```

The v10 Lisp router would proxy opcodes 0x09 and 0x0A to this v7 TCBP server (analogous to how opcode 0x02 is proxied to APL on port 8444).

---

## 9. File Map

| File | Purpose |
|------|---------|
| `playground/bridge/__init__.py` | Package exports |
| `playground/bridge/tcbp.py` | TCBP binary protocol: CRC, pack/unpack, sockets |
| `playground/bridge/client.py` | High-level numpy API over TCBP |
| `playground/bridge/formula_orchestrator.py` | Formulas.py as v7 censor/scoring bridge |
| `playground/bridge/workflow.py` | Canned orchestration scripts |
| `playground/formulas.py` | All APL-π-φ formulas (existing, not modified) |
| `v10/router/protocol.lisp` | TCBP protocol (reference) |
| `v10/router/server.lisp` | TCBP server (reference) |
| `v7/engine/engine.lisp` | v7 evolution loop (future TCBP target) |
| `v7/engine/quantum.pl` | Quantum Prolog censor (alternative to formula bridge) |
