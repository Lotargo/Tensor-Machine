Now I have all the context needed. Here is the documentation:

---

# Phase 2 DSL: S-Expression Query Language for tensor-rag

## Overview

The DSL is a Lisp-style S-expression language that expresses RAG search strategies declaratively. It flows through three layers:

1. **Parse** (`dsl.rs`) — S-expression → `AstNode`
2. **Compile** (`ir.rs::Planner`) — `AstNode` → `Program` (linear `Vec<OpCode>`)
3. **Execute** (`vm.rs` + `mcp.rs`) — `Program` → stack-based VM with I/O orchestration

---

## 1. S-Expression Parsing with `lexpr`

All DSL forms use `lexpr::from_str` to tokenize and parse raw text into `lexpr::Value`. The entry point is `dsl::parse_dsl`:

- Calls `lexpr::from_str(input)` which returns a `Value::Cons` (linked list cell).
- Extracts the head symbol via `cell.car().as_symbol()` and dispatches on it.
- Keyword arguments (`:query`, `:filters`, `:rerank`, etc.) are walked sequentially by iterating over the cons cells: each keyword is followed by its value in the next cell.
- Sub-parsers (`parse_search_args`, `parse_multi_hop_args`, `parse_self_rag_args`, `parse_corrective_rag_args`, `parse_filters`, `parse_rerank`) each follow the same cons-cell walking pattern.

Nested S-expressions for branch plans (Self-RAG) are handled by `parse_inner_node`, which recursively calls the same dispatching logic on an already-parsed `Value` rather than a string.

---

## 2. Surface Forms (Search Strategies)

### 2.1 `search` — Flat retrieval with reranking

```
(search :query <string>
        :filters? (<op> <field> <value>)
        :rerank? (<strategy> ...)
        :final-top-k? <int>)
```

`:filters` parses a single filter triplet like `(>= year 2023)` into a `Filter { field, operator, value }`.

`:rerank` dispatches on the strategy name:

| Form | `RerankStrategy` variant | IR produced |
|---|---|---|
| `(semantic)` | `SemanticOnly` | RetrieveQdrant, ComputeSemantic, TopK, Emit |
| `(bm25)` | `Bm25Only` | +RetrievePostgres before TopK |
| `(hybrid :weights (sem <f> bm25 <f>))` | `Hybrid { sem, bm25 }` | +LoadConst(sem), LoadConst(bm25), Fusion |
| `(mmr :diversity <f>)` | `Mmr { diversity }` | Mmr(diversity), TopK, Emit |
| `(rrf :k <int>)` | `Rrf { k }` | +RetrievePostgres, Rrf(k), TopK, Emit |
| `(any-other :key val)` | `Dynamic { name, args }` | CallDynamic(name, args), TopK, Emit |

### 2.2 `multi-hop` — Iterative retrieval

```
(multi-hop :query <string> :hops <int>)
```

Emits N `RetrieveQdrant` instructions (one per hop), then a single `ComputeSemantic`, `TopK`, `Emit`.

### 2.3 `self-rag` — Confidence-gated branching

```
(self-rag :query <string>
          :threshold <float>
          :low-confidence (<form> ...)
          :high-confidence (<form> ...))
```

The `:low-confidence` and `:high-confidence` values are inner S-expressions parsed by `parse_inner_node`. The planner recursively compiles both sub-plans and emits:

```
RetrieveQdrant, ComputeSemantic, IfConfidence(threshold, low_prog, high_prog)
```

The VM's `run` method evaluates the max score against the threshold; if `max_score >= threshold` it runs `high_prog`, otherwise `low_prog`.

### 2.4 `corrective-rag` — Fallback query re-route

```
(corrective-rag :query <string> :fallback-query <string>)
```

Emits `RetrieveQdrant` with the fallback query (ignoring the primary query entirely in the IR, modeling a simple corrective fallback), then `ComputeSemantic`, `TopK`, `Emit`.

---

## 3. AST (`ir.rs::AstNode`)

The parsed output is one of four enum variants:

```
AstNode::Search(QueryRequest)
AstNode::MultiHop { query, hops }
AstNode::SelfRag { query, threshold, low_plan: Box<AstNode>, high_plan: Box<AstNode> }
AstNode::CorrectiveRag { query, fallback_query }
```

`QueryRequest` bundles `query`, `filters: Option<Vec<Filter>>`, `rerank_strategy: RerankStrategy`, and `final_top_k: usize`. It also derives `Deserialize` so the same type can be constructed directly from JSON (the fallback path in `mcp.rs`).

---

## 4. IR — OpCode Bytecode (`ir.rs::OpCode`)

`Planner::compile(ast: AstNode) -> Program` lowers the AST into a linear `Vec<Instruction>` where each instruction wraps an `OpCode`:

| OpCode | Purpose |
|---|---|
| `RetrieveQdrant { query, filters, top_k }` | Fetch candidate vectors from Qdrant |
| `RetrievePostgres` | Fetch BM25 scores from Postgres |
| `ComputeSemantic` | Dot-product query vector against all candidate vectors |
| `LoadConst(f32)` | Push scalar onto VM stack |
| `Add` / `Mul` / `Div` | Element-wise tensor arithmetic (stack-based) |
| `Fusion` | Weighted blend: `a*w1 + b*w2` (pops 4 stack values) |
| `Matmul` | Dot product of two 1D tensors → Scalar |
| `TopK(usize)` | Sort and select top K indices/scores |
| `Mmr(f32)` | Maximum Marginal Relevance (mock) |
| `Rrf(usize)` | Reciprocal Rank Fusion (mock) |
| `Threshold(f32)` | Zero values below threshold |
| `Sort` | Sort descending |
| `Slice(usize, usize)` | Extract sub-range |
| `IfConfidence(f32, Program, Program)` | Branch on max score |
| `CallDynamic(String, Value)` | Runtime-dispatchable strategy |
| `Emit` | Return top-of-stack as result |

**Compilation patterns by strategy:**

| Strategy | Instruction sequence |
|---|---|
| SemanticOnly | RetrieveQdrant, ComputeSemantic, TopK(N), Emit |
| Bm25Only | +RetrievePostgres, TopK(N), Emit |
| Hybrid | +RetrievePostgres, LoadConst(w_sem), LoadConst(w_bm25), Fusion, TopK(N), Emit |
| Mmr | Mmr(d), TopK(N), Emit |
| Rrf | +RetrievePostgres, Rrf(k), TopK(N), Emit |
| Dynamic | CallDynamic(name, args), TopK(N), Emit |

---

## 5. Execution: Orchestrator + VM

The `mcp.rs` orchestrator (`execute_program`) handles I/O ops (`RetrieveQdrant`, `RetrievePostgres`) and delegates pure compute to `vm.step()`. The `vm.run()` method provides a back-to-back test path that also handles I/O and `IfConfidence` branching.

**VM stack values:** `Scalar(f32)`, `Tensor1D(Array1<f32>)`, `ArrowArray(Arc<dyn Array>)`, `Result(TopKResult { indices, scores })`.

The `ComputeSemantic` opcode extracts the fixed-size list column from the Arrow `RecordBatch`, iterates row by row computing dot products with the query vector, and pushes a `Float32Array` of scores onto the stack.

---

## 6. Dual Entry in `mcp.rs`

The MCP `SearchTool` accepts either:
- A `"dsl"` key with an S-expression string → `dsl::parse_dsl(s)`
- A JSON object directly → `serde_json::from_value` into `QueryRequest` → wrapped as `AstNode::Search`

Both paths converge at `Planner::compile`, then `execute_program`.
