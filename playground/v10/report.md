# Project Audit Report: turbo-calc

**Repository**: `https://github.com/Lotargo/turbo-calc`
**Commit**: `3f6ca37` (grafted, single-commit history)
**Date**: 2026-06-02
**Analyst**: opencode (Builder)

---

## 1. Project Map

### 1.1 Directory Structure

```
/app/
├── .brains/sessions/2026-05-29-0001/   # Agent session tracking
│   ├── plans/plan.md                    # Execution plan for research agents
│   └── walkthrough/
│       ├── agent_fix_plan.md            # Remediation plan
│       └── summary.md                   # Bug/security analysis
├── .gitignore                           # Ignores apl-1.8/ and apl-1.8.tar.gz
├── AGENTS.md                            # OpenCode collaboration protocol
├── README.md                            # Russian-language architecture doc
├── client/
│   ├── main.lua                         # CLI client (LuaJIT, 273 lines)
│   └── protocol.lua                     # TCBP protocol implementation (Lua, 102 lines)
├── coprocessor/
│   └── matrix.apl                       # APL matrix coprocessor (GNU APL, 117 lines)
├── router/
│   ├── dsp.lisp                         # DFT/FFT implementation (70 lines)
│   ├── graph.lisp                       # Dijkstra on CSR graphs (74 lines)
│   ├── protocol.lisp                    # TCBP protocol in Lisp (233 lines)
│   ├── qsim.lisp                        # Naive quantum simulator (38 lines)
│   ├── safe_eval.lisp                   # Safe arithmetic evaluator (84 lines)
│   ├── server.lisp                      # TCP server & dispatcher (210 lines)
│   └── solver.pl                        # Prolog CLP(FD) solver (117 lines)
└── tests/
    ├── ab_test_e2e.py                   # Python A/B end-to-end tests (81 lines)
    ├── test_client.lua                  # Lua client unit tests (39 lines)
    ├── test_prolog.lisp                 # Prolog solver tests (27 lines)
    └── test_router.lisp                 # Lisp router tests (128 lines)
```

### 1.2 Architecture

```
┌──────────────┐     TCBP (binary)     ┌──────────────────┐
│  Lua Client  │──────port 8443───────→│  Lisp Router      │
│  (luajit)    │←──────────────────────│  + Prolog Solver  │
└──────────────┘                       └────────┬─────────┘
                                                │ TCBP binary
                                                │ port 8444
                                                ↓
                                       ┌──────────────────┐
                                       │  APL Coprocessor  │
                                       │  (GNU APL)        │
                                       └──────────────────┘
```

### 1.3 Components

| Component | Language | Lines | Role |
|-----------|----------|-------|------|
| **Client** | LuaJIT + FFI | 273 | CLI, parses user input, packs TCBP messages, sends/receives over TCP |
| **Client Protocol** | Lua | 102 | CRC16, LE pack/unpack, message framing |
| **Router Server** | Common Lisp (SBCL) | 210 | TCP server, opcode dispatch, APL proxy, thread-per-connection |
| **Router Protocol** | Common Lisp | 233 | TCBP: CRC16, payload serialization (matrix/CSR/complex/qsim) |
| **Safe Eval** | Common Lisp | 84 | Whitelist-based arithmetic evaluator + symbolic differentiation |
| **Graph** | Common Lisp | 74 | Dijkstra with binary heap on CSR graphs |
| **DSP** | Common Lisp | 70 | DFT (naive O(n²) for non-power-of-2, Cooley-Tukey FFT for power-of-2) |
| **QSim** | Common Lisp | 38 | Hadamard + CNOT gates on 1-2 qubit state vectors |
| **APL Coprocessor** | GNU APL | 117 | TCP server, IEEE 754 encode/decode, `A +.× A` matrix multiply |
| **Prolog Solver** | SWI-Prolog | 117 | Tokenizer, parser, CLP(FD) constraint solver |

### 1.4 Dependencies

- **Runtime:** SBCL, SWI-Prolog, GNU APL, LuaJIT 2.1+
- **Test (Python):** numpy, sympy, scipy
- **Network:** Raw TCP sockets only (no HTTP, no TLS)

### 1.5 TCBP Protocol (Custom Binary)

| Offset | Size | Field |
|--------|------|-------|
| 0x00 | 1B | Opcode |
| 0x01 | 4B LE | Payload Size N |
| 0x05 | N B | Payload |
| 0x05+N | 2B LE | CRC16 (CCITT-FALSE) |

Opcodes: `0x01` Eval, `0x02` Matrix, `0x03` Solve, `0x04` Sym, `0x05` Graph, `0x06` DSP, `0x07` QSim, `0x08` CSP.

---

## 2. Forgery & Deception Analysis

### 2.1 Malicious Code: **NONE FOUND**

After thorough review of every source file:

- **No obfuscated code, hidden payloads, or encoded strings** in any file.
- **No backdoors**: No reverse shell, no inbound/outbound connections to external hosts. All network connections are to `127.0.0.1` only.
- **No data exfiltration**: No HTTP requests, DNS queries, or file writes to external locations.
- **No credential theft**: No hardcoded passwords, API keys, or secret tokens.
- **No crypto miner or malware**: No CPU-intensive background loops, no cryptocurrency wallets or mining code.
- **No eval injection**: `safe_eval.lisp` uses `*read-eval* nil` and a whitelist of math functions. While not perfectly secure, it is not intentionally backdoored.

**Verdict**: No malicious code is present in this repository.

### 2.2 Misleading Documentation vs. Code Behavior: **SIGNIFICANT**

The README (`README.md`, 275 lines) makes numerous claims that do not match the implementation. These range from exaggeration to outright fiction.

#### 2.2.1 Non-Existent Commands Documented as Implemented

The README Section 12 ("Расширенная номенклатура операционных команд") documents commands with full syntax examples that **do not exist in the code**:

| Documented Command | Claimed Capability | `process-request` handler? |
|---|---|---|
| `diffeq` | ODE/PDE solving (Runge-Kutta, etc.) | **Missing** — no handler in `server.lisp:101-167` |
| `fem` | Finite Element Method (Navier-Stokes) | **Missing** |
| `tensor` | Tensor contraction, Christoffel symbols | **Missing** |
| `stoch` | Monte Carlo (10^8 iterations) | **Missing** |

The README gives specific syntax examples like:
```
turbo-calc stoch "monte_carlo(iterations=1e8, dist='log-normal', mu=0, sigma=1)"
```
This is entirely fictional — no such code exists.

#### 2.2.2 Fictional Architecture Claims

| README Claim | Reality |
|---|---|
| "Компиляция Lisp-макросов в LLVM" (Section 8) | No LLVM integration, no compilation pipeline. |
| "Генерация HTML из Lisp-макросов" (Section 1) | No HTTP server, no HTML generation in code. |
| "Миллионы параллельных сессий для Монте-Карло" (Section 13.1) | Server uses thread-per-connection; no pool, no connection limits. |
| "BLAS/LAPACK интегрированы в APL" (Section 9.3) | APL coprocessor uses `A +.× A` — no BLAS/LAPACK configured. |
| "On-the-fly CRC без буферизации пакета" (Section 9.1) | Code buffers entire message before CRC in `pack-message`. |
| "Стандартизированный бинарный пакет с кодом ошибки" (Section 10.2) | Returns plain error string in payload, no binary error code. |
| "Строгая изоляция/песочницы" (Section 10.1) | No sandboxing; `sb-ext:run-program` spawns SWI-Prolog without restrictions. |

#### 2.2.3 Protocol Table Mismatch

README Section 12 specifies opcodes `0x04`–`0x0B`. The code implements `0x04`–`0x08` but **not** `0x09`–`0x0B`. The READM explicitly lists types up to `0x0B` with assigned commands (`diffeq`=0x09, `tensor`=0x0A, `fem`=0x0B) that have no code backing them.

#### 2.2.4 Overstated SymCalc Capabilities

README Section 4 claims "символьное интегрирование, дифференцирование и разрешение дифференциальных уравнений." The code only implements basic differentiation of simple arithmetic expressions (+, -, *, /, sin, cos). There is no symbolic integration, no differential equation solver, no Taylor series, no Fourier series expansion — all claimed in Section 12.

#### 2.2.5 APL Performance Exaggeration

README Section 9.3 claims the APL coprocessor "автоматически определять симметричность или разреженность матриц, применяя специализированные алгоритмы." The APL code simply does `A +.× A` with no symmetry detection or specialized algorithms. The binary encode/decode functions in `coprocessor/matrix.apl` use element-by-element `:While` loops, not vectorized operations.

### 2.3 Deceptive Character of the README

The README is written in formal Russian technical style with sections on:
- Architectural topology
- Binary protocol specification
- Theoretical foundations (SLD resolution, CLP(FD), vector paradigm)
- Performance analysis with specific benchmarks
- Security and fault tolerance mechanisms
- Future development plans

This reads as a **convincing parody of enterprise technical documentation**. The level of detail in describing fictional features is high enough that a non-technical reader would believe the system is complete. Specific hallmarks of deception:

1. **Fabricated benchmarks**: Section 9.1 compares TCBP vs JSON with specific timing numbers (150-250ms for JSON vs <2ms for TCBP). These are not backed by any benchmarking code.

2. **Name-dropping**: References to LLVM, BLAS/LAPACK, MPI, SIMD, Runge-Kutta, Navier-Stokes — none of which are implemented.

3. **Theoretical pretension**: Section 11 (SLD-resolution, unification, CLP(FD)) describes standard Prolog theory as if it were a custom innovation.

4. **False concreteness**: Section 8 mentions "LLVM IR" and "C/C++ level performance" with no implementation.

### 2.4 Code Quality Issues (Not Malicious But Notable)

| Issue | Severity | Location | Detail |
|-------|----------|----------|--------|
| `read-from-string` in safe eval | P1 | `safe_eval.lisp:25-28` | Despite `*read-eval* nil`, reader macros can cause issues (e.g., `#1=#1#` infinite loop, sharp-equal, circle notation) |
| Thread-per-connection, no limits | P1 | `server.lisp:189-210` | Unbounded thread creation; any number of clients can exhaust resources |
| No timeouts on socket I/O | P1 | `server.lisp:38-56` | `run-apl-coprocessor` blocks indefinitely on slow/malicious APL |
| APL coprocessor protocol mismatch | P2 | `server.lisp → matrix.apl` | Lisp sends raw binary; APL reads via `⎕FIO[3]` — could corrupt on certain byte values |
| `pack-graph-payload` uses `predecessors` as double | P2 | `protocol.lisp:195-206` | Predecessor array is integer but serialized as `double-to-bytes` (lossy) |
| CRC verification bugs | P2 | `client/main.lua:90` | 16-bit CRC read with 32-bit unpack; padding with nulls |
| No keep-alive in client | P2 | `client/main.lua:41-96` | New TCP connection per request → TIME_WAIT exhaustion |
| `expt` in whitelist with weak limits | P2 | `safe_eval.lisp:15-21` | Limits exist (1M base, 10K power) but `expt` itself enables easy DoS |
| No max payload size on server | P2 | `server.lisp:85-167` | Server reads arbitrary-sized payloads from clients → OOM risk |
| Dijkstra uses `(1+ u)` for CSR end index | P3 | `graph.lisp:64` | Works only for 0-indexed CSR with contiguous rows; isolated nodes may cause issues |
| `long-float` → `double-float` truncation in qsim | P3 | `protocol.lisp:229-230` | Qsim computes in long-float but serializes as double-float |
| Code comments are in Russian but mixed | P1 | various | Major mismatch between documented claims and code capabilities |

---

## 3. Risk Assessment

| Risk | Probability | Impact | Detail |
|------|------------|--------|--------|
| **DoS via safe-eval** | High | Medium | `(expt 2 1000000)` in eval request consumes CPU/memory |
| **OOM via oversized input** | High | High | No payload size limits; attacker can send multi-GB payloads |
| **Resource exhaustion** | High | Medium | Thread-per-connection without limits |
| **APL coprocessor hangs** | Medium | Medium | No timeouts; a broken APL hangs the request indefinitely |
| **Data corruption (graph)** | Medium | Medium | Predecessors serialized as doubles → precision loss |
| **Reader macro attack** | Low | High | `*read-eval* nil` blocks `#.` but other reader tricks possible |
| **Data exfiltration** | None | — | All connections are localhost-only |

---

## 4. Summary

| Category | Finding |
|----------|---------|
| **Malicious code** | **None detected.** No backdoors, no obfuscation, no data theft, no crypto mining. |
| **Documentation deception** | **Severe.** README describes ~40-50% fictional capabilities. Non-existent commands (`diffeq`, `fem`, `tensor`, `stoch`) have syntax examples. Performance benchmarks are fabricated. Architecture claims (LLVM, BLAS, sandboxing) are entirely false. |
| **Security vulnerabilities** | **Several.** Unbounded thread creation, no payload size limits, no I/O timeouts, `read-from-string` vector in safe-eval, weak `expt` limits. |
| **Code quality** | **Mixed.** Some parts are reasonable (CRC16, LE pack/unpack, Dijkstra heap). Others have correctness bugs (graph predecessor serialization, CRC validation in client). |
| **Overall assessment** | The project appears to be a **satirical/parody implementation** or an **architectural mockup** — the documentation far exceeds the implementation. The code is functional for basic operations but does not deliver the advanced capabilities claimed in the README. No evidence of malicious intent. |

---

## 5. Git History Anomaly

The repository has a **single commit** (`3f6ca37`) with 5 grafted branches. All branches point to the same tree state. The `opencode` file in `.git/` and the `.brains/` session artifacts suggest this repository was created as an **agent evaluation benchmark** — consistent with the OpenCode collaboration protocol described in `AGENTS.md`. The `git config` shows a submodule directive `[submodule] active = .`, which is unusual.

## Мои собственные наблюдения
Агент успешно выявил, что проект представляет собой некую пародию или макет. Документация сильно преувеличивает возможности. Несуществующие команды задокументированы с синтаксисом. Кроме того, есть уязвимости (OOM-атаки, отсутствие таймаутов, потоки на каждое соединение и т.д.). Никакого скрытого майнера или трояна нет, но само по себе несоответствие README и кода — это и есть «декоративный подлог».
