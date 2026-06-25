# Skill-Routed Retrieval Architecture for Tensor Machine

Status: design note / implementation candidate  
Date: 2026-06-25  
Scope: Tensor Machine agent mode, MCP routing, retrieval planning, instruct-embedding projections

Implementation tracking: unchecked items are proposed work, checked items are already present or verified in the current codebase.

---

## 1. Purpose

This document captures a proposed extension for Tensor Machine: a skill-routed retrieval architecture where RAG is used not only to retrieve factual document chunks, but also to retrieve the best procedural instructions, routing policies, and embedding instructions for the current task.

The goal is to evolve Tensor Machine from a programmable RAG execution engine into a semantic execution planner that can choose how to search, how to reason, which retrieval pattern to apply, and which embedding projection should be used before vector search.

Important boundary:

```text
Skill routing improves retrieval decisions.
It does not by itself guarantee correct low-level tool-call protocol behavior.
```

For OpenAI-compatible models this distinction is often less visible because tool calls are returned in a structured `tool_calls` field. For DeepSeek-like or other partially compatible providers, the model may emit a tool call as plain text or provider-specific markup. That must be handled by the agent orchestration layer with parser, validator, retry, and final-answer guards. Skills can reduce the frequency of bad calls, but the runtime must still treat malformed tool calls as protocol events, not as user-facing answers.

Current Tensor Machine already has several important foundations:

- MCP-first agent integration.
- Mandatory preflight RAG search in agent mode.
- DSL to IR to bytecode to VM execution.
- Hybrid retrieval signals: dense, lexical/BM25, entity, structural, recency, and cluster.
- RSF, RRF, dynamic rerank strategies, ColBERT-style late interaction, and self/corrective RAG patterns.
- Default support for an instruct embedding model such as multilingual E5-large-instruct.

The proposed architecture adds a procedural layer above this existing retrieval and VM stack.

---

## 2. Core Idea

Traditional RAG answers this question:

```text
Which documents help answer the user?
```

Skill-routed RAG answers a different question first:

```text
Which procedures, skills, and retrieval modes should the agent use for this task?
```

Then an embedding-instruction layer answers:

```text
How should the query be projected into vector space for this task?
```

The full flow becomes:

```text
User request
  -> Agent Skill-RAG
  -> FSM / router state selection
  -> Embedding Instruction-RAG
  -> Multi-projection vector retrieval
  -> Candidate fusion
  -> Reranking / evidence check
  -> Answer or action
```

A compact formula:

```text
First RAG chooses how the agent should think.
Second RAG chooses how the embedding model should see the query.
The existing Tensor VM executes the selected retrieval plan.
```

---

## 3. Why This Fits Tensor Machine

Tensor Machine is already designed around programmable retrieval:

```text
DSL -> AstNode -> Program(OpCode[]) -> VM execution
```

The missing layer is not another retriever. The missing layer is a pre-planner that chooses the right retrieval program for the current task.

Today, a user or client can provide a DSL form such as:

```scheme
(search :query "semantic search architecture" :final-top-k 5)

(multi-hop :query "quantum error correction"
           :hops 3
           :rerank (hybrid :weights (sem 0.6 bm25 0.4))
           :final-top-k 5)

(corrective-rag :query "specific technical detail"
                :fallback-query "technical overview"
                :rerank (rrf)
                :final-top-k 5)
```

The proposed skill router allows the system to generate or select such plans automatically from a natural-language task.

Example:

```text
User: "After adding reranking, answers became worse even though top-k looks normal."
```

Instead of a single naive search, the system should select:

```text
Agent skill: rag_failure_diagnosis
Reasoning mode: hypothesis_tree
Embedding projections:
  - reranker failure modes
  - top-k recall loss
  - context packing after rerank
  - before/after evaluation evidence
Retrieval strategy:
  - dense + BM25 + structural
  - fusion through RRF or RSF
  - evidence check before answering
```

This is a natural extension of Tensor Machine's vector-relational execution model.

---

## 4. Layered Architecture

### L0. User Intent

Input is the raw user task, plus optional context such as selected document, conversation state, current tool mode, or explicit constraints.

Example:

```text
"Find where the documentation explains GPU fallback behavior and prove it with citations."
```

### L1. Agent Skill-RAG

This layer searches a skill registry for procedural skills.

It retrieves compact skill cards, not full long instructions at first.

Example skill card:

```yaml
id: rag.evidence_first_answering
kind: agent_skill
description: Use when the user asks for an answer grounded strictly in retrieved local sources with citations.
tags:
  - rag
  - evidence
  - citations
  - grounded-answering
positive_examples:
  - "prove it with citations"
  - "answer only from documents"
  - "do not use pretrained knowledge"
negative_triggers:
  - "creative writing"
  - "general brainstorming without document grounding"
mode: evidence_first
risk_level: low
priority: 70
```

The skill router should select one primary skill and at most a few auxiliary skills.

Good selection:

```text
Primary: evidence_first_answering
Auxiliary: exact_section_lookup
Auxiliary: local_context_insufficient_policy
```

Bad selection:

```text
Load 12 overlapping skills into context.
```

The router must avoid instruction noise.

### L2. FSM / Router State

The selected skills should map the request into an execution state.

Possible states:

```text
EXPLAIN_CONCEPT
EXACT_LOOKUP
SECTION_LOOKUP
COMPARE_OPTIONS
VERIFY_CLAIM
DIAGNOSE_RAG_FAILURE
DEBUG_IMPLEMENTATION
DESIGN_BENCHMARK
SUMMARIZE_DOCUMENTS
```

Each state can define defaults:

```yaml
state: DIAGNOSE_RAG_FAILURE
reasoning_mode: hypothesis_tree
max_agent_skills: 3
retrieval:
  dense_top_k: 80
  sparse_top_k: 80
  fusion: rrf
  rerank_top_k: 12
evidence_policy: diagnose_insufficient_evidence
```

The FSM does not replace the DSL or VM. It decides which retrieval plan should be compiled.

### L3. Embedding Instruction-RAG

This layer searches a separate library of embedding instructions.

These instructions are short, focused task prompts for an instruct-aware embedding model such as:

- multilingual-e5-large-instruct
- e5-large-instruct
- BGE-instruct variants
- GTE-instruct variants
- INSTRUCTOR-style models
- any compatible local embedding endpoint

Example instruction card:

```yaml
id: embed.rag_reranker_failure_modes
kind: embedding_instruction
description: Project a query for retrieving evidence about RAG reranker failure modes, recall loss, threshold issues, and context packing errors.
tags:
  - rag
  - reranker
  - failure-mode
  - retrieval-diagnosis
instruction: >
  Represent this query for retrieving technical evidence about RAG reranker failure modes,
  including recall loss, score distribution problems, top-k truncation, domain mismatch,
  and context packing errors.
```

The selected instruction is prepended or otherwise formatted for the query-side embedding model.

Example projected query:

```text
Instruction:
Represent this query for retrieving implementation details about GPU fallback behavior,
configuration flags, and backend selection.

Query:
Find where the documentation explains GPU fallback behavior.
```

### L4. Multi-Projection Retrieval

A single user query may produce multiple query vectors:

```text
q_general
q_exact_terms
q_implementation
q_evidence
q_failure_modes
```

Each vector can search the same Qdrant collection, or later, separate projection-specific collections.

Recommended MVP:

```text
Use one document index.
Encode 2-5 query projections.
Run vector search for each projection.
Run sparse/BM25 search for exact terms.
Fuse results with RRF or RSF.
Rerank or evidence-check before answering.
```

This avoids expensive reindexing while still allowing dynamic semantic projections.

---

## 5. Proposed File Structure

A minimal implementation can start with this structure:

```text
tensor-rag/src/skills/
  mod.rs
  types.rs
  registry.rs
  router.rs
  prompt.rs
  projections.rs

skills/
  agent/
    rag_failure_diagnosis.skill.yaml
    evidence_first_answering.skill.yaml
    exact_lookup.skill.yaml
    comparison.skill.yaml
    benchmark_design.skill.yaml
  embedding/
    definition.instruction.yaml
    implementation.instruction.yaml
    evidence.instruction.yaml
    failure_modes.instruction.yaml
    contradiction_search.instruction.yaml
```

The runtime should support loading skill cards from a directory first. Later, these cards can be indexed into PostgreSQL/Qdrant just like regular documents.

---

## 6. Skill Card vs Skill Body

Do not index long skill bodies as one giant text block.

Use two layers:

```text
Skill card: short metadata used for retrieval and routing.
Skill body: full instruction loaded only after selection.
```

Skill card fields:

```yaml
id: string
kind: agent_skill | embedding_instruction | tool_policy | evidence_policy
version: string
description: string
tags: string[]
positive_examples: string[]
negative_triggers: string[]
allowed_tools: string[]
risk_level: low | medium | high
priority: integer
body_path: string
```

Full skill body can be Markdown:

```md
# RAG Failure Diagnosis

Use this skill when a RAG system gives wrong, weak, hallucinated, or incomplete answers.

Do not assume the LLM is the first problem. Build a hypothesis tree:

1. Query formulation
2. Chunking
3. Dense recall
4. BM25 mismatch
5. Reranker cutoff
6. Context packing
7. Generation grounding

For each branch:
- list expected symptoms
- propose the smallest diagnostic test
- avoid rebuilding the whole system until local tests fail
```

---

## 7. Integration Point: `agent.rs`

Current agent mode already performs a mandatory preflight document search before answering.

The proposed change is to turn this into a richer preflight planner:

```rust
let plan = skill_preflight(
    db_pools,
    config,
    &latest_user_query,
    selected_doc_uuid,
    scope_to_selected_document,
).await?;
```

Possible return type:

```rust
pub struct PreflightPlan {
    pub selected_agent_skills: Vec<SelectedSkill>,
    pub selected_embedding_instructions: Vec<EmbeddingInstruction>,
    pub query_projections: Vec<QueryProjection>,
    pub retrieval_plan: Option<String>,
    pub evidence_policy: EvidencePolicy,
}
```

Then preflight should:

1. Select procedural skills.
2. Select embedding instructions.
3. Build 2-5 query projections.
4. Execute projected retrieval.
5. Fuse and deduplicate results.
6. Inject both sources and compact active skill summaries into the system context.

The system context should remain compact:

```text
Active retrieval mode: evidence_first_answering
Active projection modes: implementation, evidence
Evidence policy: strict local sources; report insufficient context if no source supports the answer.
```

Avoid injecting long skill bodies unless the task requires them.

---

## 8. DSL Extension Candidate

A future DSL form could expose projected retrieval explicitly:

```scheme
(projected-search
  :query "why did reranking make answers worse"
  :projections (
    (instruction "embed.rag_reranker_failure_modes")
    (instruction "embed.topk_recall_loss")
    (instruction "embed.context_packing_after_rerank"))
  :candidate-top-k 80
  :fusion (rrf :k 60)
  :final-top-k 8)
```

Another form could allow skill-routed execution:

```scheme
(skill-routed-search
  :query "prove where GPU fallback is documented"
  :agent-skill "rag.evidence_first_answering"
  :state "VERIFY_CLAIM"
  :final-top-k 6)
```

MVP can avoid DSL changes and build these plans internally. The DSL extension can come later once behavior is validated.

---

## 9. IR and OpCode Extension Candidate

Possible future additions:

```rust
pub enum AstNode {
    SkillRoutedSearch {
        query: String,
        skill_ids: Vec<String>,
        projections: Vec<QueryProjection>,
        final_top_k: usize,
    },
    VerifyClaim {
        claim: String,
        evidence_policy: EvidencePolicy,
    },
    DiagnoseRag {
        query: String,
        branches: Vec<DiagnosticBranch>,
    },
}
```

Possible opcodes:

```rust
pub enum OpCode {
    RetrieveSkillCards { query: String, top_k: usize },
    RerankSkills,
    LoadSkillPolicy { skill_id: String },
    BuildQueryProjection { instruction_id: String, query: String },
    RetrieveQdrantProjected { projection_id: String, top_k: usize },
    FuseProjections { method: FusionMethod },
    EvidenceCheck { policy: EvidencePolicy },
}
```

This should not be the first implementation step. The first implementation can live outside the VM and call existing search paths.

---

## 10. Retrieval Strategy Selection Examples

### Exact lookup

User:

```text
Where is MCP_SEARCH_DEFAULT_TOP_K documented?
```

Selected behavior:

```text
Agent skill: exact_lookup
Embedding instruction: implementation/configuration lookup
Retrieval order:
  1. BM25 / keyword
  2. structured heading / metadata
  3. dense fallback
  4. parent/neighbor expansion
```

### Evidence-first answer

User:

```text
Prove this from the local docs with citations.
```

Selected behavior:

```text
Agent skill: evidence_first_answering
Evidence policy: strict local only
Retrieval:
  - dense + BM25
  - rerank
  - cite exact chunks
  - if insufficient evidence, say so explicitly
```

### RAG failure diagnosis

User:

```text
After adding the reranker, answers became worse.
```

Selected behavior:

```text
Agent skill: rag_failure_diagnosis
Reasoning mode: hypothesis_tree
Embedding projections:
  - reranker failure modes
  - top-k recall loss
  - context packing after rerank
  - before/after evaluation evidence
Retrieval:
  - multi-projection dense
  - BM25 exact terms
  - structured/neighbor expansion
  - diagnostic summary
```

### Comparison

User:

```text
Compare Qdrant and Milvus for this project architecture.
```

Selected behavior:

```text
Agent skill: comparison
Embedding projections:
  - vector database architecture
  - scalability trade-offs
  - integration constraints
Output:
  - table of trade-offs
  - recommendation
  - assumptions and limits
```

---

## 11. Selection Scoring

Do not rely only on keyword triggers.

Recommended skill selection score:

```text
score =
  semantic_similarity
+ keyword_match
+ intent_match
+ tool_availability_match
+ project_context_match
+ user_preference_match
- negative_trigger_match
- conflict_penalty
- risk_penalty
```

A skill may be topically similar but not applicable.

Example:

```text
User: "What is a reranker?"
```

Relevant topic: reranker  
Correct mode: concept explanation  
Incorrect mode: RAG failure diagnosis

This is why intent matching is mandatory.

---

## 12. Conflict Resolution

Multiple skills can be selected, but they can conflict.

Example conflict:

```text
Skill A: create an issue after analysis.
Skill B: never create issues without explicit confirmation.
```

Recommended priority order:

```text
System policy
User instruction
Project instructions
Selected skills
Tool descriptions
Model defaults
```

For Tensor Machine, high-risk operations should never be triggered only by a retrieved skill.

Risk examples:

```text
write file
update index
delete indexed documents
execute DSL with side effects
create issue / PR
send external requests
```

Require explicit user confirmation or an existing trusted tool policy.

---

## 13. Evaluation Plan

This architecture must be evaluated, not assumed to be better.

Metrics:

```text
skill_selection_accuracy
wrong_skill_activation_rate
embedding_instruction_selection_accuracy
Recall@k before rerank
MRR / nDCG after rerank
answer_groundedness
citation_support_rate
latency_overhead
number_of_extra_retrieval_steps
insufficient_context_detection_rate
```

Important negative metric:

```text
wrong_skill_activation_rate
```

A wrong skill can be worse than no skill because it can push the agent into solving the wrong task.

Suggested test set:

```text
1. Short concept questions
2. Exact configuration lookups
3. Section/chapter lookups
4. Evidence-first questions
5. Multi-hop comparison questions
6. RAG failure diagnosis questions
7. Ambiguous queries
8. Queries with misleading keywords
9. Queries where no local evidence exists
10. Tool/action requests that require confirmation
```

---

## 14. Minimal MVP

MVP should avoid deep VM changes.

Implement:

```text
1. Skill card schema.
2. File-based skill registry.
3. Simple hybrid skill selector:
   - BM25 over name/tags/description/examples
   - optional dense search later
4. Intent classifier with a small enum.
5. Embedding instruction selector.
6. Query projection builder.
7. Preflight integration in agent mode.
8. Compact active-skill injection into system context.
9. Evaluation logs for selected skills and projections.
```

### 14.1 Implementation checklist

Core skill routing:

- [ ] Add skill card schema.
- [ ] Add file-based skill registry.
- [ ] Add short skill-card retrieval over name, tags, description, and examples.
- [ ] Add intent classifier with states such as `EXACT_LOOKUP`, `SECTION_LOOKUP`, `VERIFY_CLAIM`, `DIAGNOSE_RAG_FAILURE`, and `SUMMARIZE_DOCUMENTS`.
- [ ] Add conflict and negative-trigger handling for selected skills.
- [ ] Select one primary skill and at most a few auxiliary skills.
- [ ] Load full skill bodies only after selection.
- [ ] Inject compact active-skill summaries into system context.
- [ ] Add audit logs for selected skills, scores, reasons, and rejected candidates.

Embedding instruction routing:

- [ ] Add embedding instruction card schema.
- [ ] Add instruction selector for exact lookup, implementation lookup, evidence lookup, contradiction search, and failure diagnosis.
- [ ] Build 2-5 query projections from selected instructions.
- [ ] Run projected dense retrieval against the existing document index.
- [ ] Fuse projected retrieval with lexical, structural, entity, recency, and cluster signals.
- [ ] Deduplicate and rerank final candidates before answer generation.

Agent-mode preflight:

- [ ] Replace the current single mandatory preflight search with `skill_preflight`.
- [ ] Preserve the current mandatory local-library evidence policy.
- [ ] Keep the preflight compact enough to fit within context limits.
- [ ] Add structured evidence policy output: strict local evidence, insufficient context, or clarification needed.
- [ ] Add UI/debug visibility for selected mode, projections, and evidence policy.

Initial module sketch:

```text
tensor-rag/src/skills/types.rs
tensor-rag/src/skills/registry.rs
tensor-rag/src/skills/router.rs
tensor-rag/src/skills/projections.rs
tensor-rag/src/skills/prompt.rs
```

Initial function sketch:

```rust
pub async fn skill_preflight(
    db_pools: &DbPools,
    config: &AiConfig,
    query: &str,
    selected_doc_uuid: Option<&str>,
    scope_to_selected_document: bool,
) -> Result<PreflightPlan, String>;
```

---

## 15. Tool-Call Compatibility Corrections

Skill routing should be implemented together with a stricter tool-call orchestration layer. This is especially important for DeepSeek-like models or any provider that is only partially OpenAI-compatible.

Observed failure mode:

```text
The model emits a tool call as assistant text instead of returning a structured tool call.
Example: DSML-like markup appears in the chat UI as the final answer.
```

This is not primarily a retrieval-quality failure. It is a protocol boundary failure.

### 15.1 Provider-specific tool policy skills

Add compact tool policy skills or prompt snippets for models that need stricter instructions.

Recommended cards:

```text
tool_policy.openai_tool_calls
tool_policy.deepseek_dsml_tool_calls
tool_policy.strict_json_tool_decision
tool_policy.no_textual_tool_calls
```

Checklist:

- [ ] Add provider-specific tool policy skill cards.
- [ ] Add a policy telling models never to print tool calls as user-facing prose.
- [ ] Add a policy telling models to return either a tool call or a final answer, never both.
- [ ] Add examples for `search_text`, `get_document_metadata`, `read_document`, and `list_documents`.
- [ ] Add examples for chapter/section lookup: metadata first, then filtered search.
- [ ] Keep provider policies short; avoid loading long procedural bodies for every request.

These skills reduce bad behavior but should not be treated as a correctness guarantee.

### 15.2 Structured JSON tool-decision mode

For providers with unreliable native tool calls, use a strict structured decision format instead of free-form assistant text.

Example:

```json
{
  "type": "tool_call",
  "name": "search_text",
  "arguments": {
    "query": "Глава 23",
    "doc_uuid": "73ebd244-f30c-447f-9784-0096c2c1f2b3",
    "final_top_k": 5
  }
}
```

Final answer example:

```json
{
  "type": "final_answer",
  "answer": "..."
}
```

Clarification example:

```json
{
  "type": "ask_clarification",
  "question": "Which document should I search?"
}
```

Checklist:

- [ ] Add optional strict JSON tool-decision mode for providers that do not reliably emit native `tool_calls`.
- [ ] Validate model output against a small schema.
- [ ] Retry or repair when JSON is invalid.
- [ ] Reject mixed outputs that contain both final answer text and tool-call intent.
- [ ] Convert valid JSON tool decisions into the internal `ToolCall` structure.
- [ ] Keep native OpenAI `tool_calls` as the preferred path for providers that support it correctly.

### 15.3 DSML parser hardening

The current DSML parser should be treated as a compatibility adapter, not as the primary protocol.

Checklist:

- [ ] Accept known DSML marker variants used by DeepSeek-like models.
- [ ] Tolerate whitespace and minor formatting differences around tags and attributes.
- [ ] Parse primitive JSON values correctly, including booleans, numbers, arrays, and objects.
- [ ] Add tests for `search_text`, `get_document_metadata`, `list_documents`, and malformed DSML.
- [ ] Strip parsed DSML from assistant content before adding the assistant message to history.
- [ ] Do not show raw DSML to the user.

### 15.4 Final-answer guard

The final completion step must not leak tool-call markup to the UI.

Checklist:

- [ ] Inspect final assistant output for DSML, JSON tool-decision objects, or textual tool-call markers.
- [ ] If final output contains a valid tool call, execute it or continue the tool loop instead of returning it to the user.
- [ ] If final output contains an invalid tool call, retry with a protocol correction message.
- [ ] If retry budget is exhausted, return a controlled protocol error, not raw markup.
- [ ] Add tests for tool-call leakage after `AI_AGENT_MAX_TOOL_ROUNDS` is exhausted.

### 15.5 Tool-round budget

The default tool-round budget should support multi-hop, section lookup, and evidence-first workflows.

Current concern:

```text
Simple lookup may need 1-2 model tool rounds.
Chapter lookup may need list_documents/get_document_metadata/search_text/read_document.
Multi-hop or failure diagnosis may need more.
```

Recommended changes:

- Raise the default `AI_AGENT_MAX_TOOL_ROUNDS` from a small value such as `4` to `8`.
- Raise the hard clamp from `12` to `20` or `24`.
- Add a separate safeguard for repeated identical tool calls.
- Add a separate safeguard for total elapsed time and total tool calls.
- Prefer deterministic preflight execution for common chapter/section lookup paths so every step does not require another LLM round.

Checklist:

- [ ] Raise default tool rounds to a safer value for multi-hop workflows.
- [ ] Raise the hard maximum while keeping a sane cap.
- [ ] Detect repeated identical tool calls.
- [ ] Stop loops with a useful diagnostic message.
- [ ] Add metrics for number of tool rounds per request.
- [ ] Add evaluation cases for 1-hop, 2-hop, chapter lookup, and failure diagnosis.

---

## 16. Chapter, Section, and Anchor Lookup Corrections

Skill routing should make structured lookup easier for the model, but the runtime should also support deterministic fallbacks.

Recommended behavior:

```text
User asks about chapter/section/part
  -> identify selected document if available
  -> load metadata/anchors
  -> match requested heading
  -> search with doc_uuid and chapter/section filter
  -> expand to neighboring chunks when needed
  -> cite evidence or report insufficient local context
```

Checklist:

- [ ] Add `exact_section_lookup` or `chapter_section_lookup` agent skill.
- [ ] Add an embedding instruction for structured heading lookup.
- [ ] Add examples for Russian chapter names such as `Глава 23` and `Часть вторая`.
- [ ] Verify `get_document_metadata` returns enough anchor fields for the agent to select the right chapter.
- [ ] Verify `search_text` accepts chapter/section filters in tool schema.
- [ ] Verify filters apply after fusion/rerank, not only to the dense candidate path.
- [ ] Add deterministic fallback: if the query clearly names a chapter, infer `chapter_id` before retrieval.
- [ ] Add neighbor expansion around matched chapter chunks.
- [ ] Add tests using the current document with known anchors.

---

## 17. Evaluation And Regression Checklist

Evaluate this architecture before treating it as a quality improvement.

Checklist:

- [ ] Measure skill selection accuracy.
- [ ] Measure wrong skill activation rate.
- [ ] Measure embedding instruction selection accuracy.
- [ ] Measure Recall@k before rerank.
- [ ] Measure MRR or nDCG after rerank.
- [ ] Measure answer groundedness.
- [ ] Measure citation support rate.
- [ ] Measure insufficient-context detection rate.
- [ ] Measure latency overhead.
- [ ] Measure average and p95 tool rounds.
- [ ] Measure malformed tool-call rate by provider/model.
- [ ] Measure raw tool-call leakage rate in the UI.
- [ ] Add regression tests for DeepSeek-like DSML output.
- [ ] Add regression tests for strict JSON tool-decision mode.
- [ ] Add regression tests for chapter/section lookup.

---

## 18. Later Extensions

After MVP works:

1. Index skill cards into PostgreSQL and Qdrant.
2. Add cross-encoder or LLM-based skill applicability reranking.
3. Add DSL forms for projected search and skill-routed search.
4. Add VM-level opcodes for projections and evidence checks.
5. Support multiple document-side projection indexes.
6. Add audit logs for activated skills.
7. Add a skill governance model: trust level, source, version, risk, owner.
8. Add a UI/debug endpoint showing which skills were selected and why.

---

## 19. Positioning

Current positioning:

```text
Tensor Machine: Programmable Low-Latency RAG Engine
```

Possible future positioning:

```text
Tensor Machine: Skill-routed semantic execution engine for agentic RAG.
```

More conservative wording:

```text
A programmable RAG engine with procedural skill routing and instruction-aware retrieval planning.
```

The important distinction:

```text
Tensor Machine currently compiles retrieval strategies.
Skill-RAG lets it choose retrieval strategies.
Embedding Instruction-RAG lets it choose semantic projections before retrieval.
```

---

## 20. Summary

The proposed architecture adds procedural memory to Tensor Machine.

Normal RAG retrieves facts.

Skill-RAG retrieves competencies:

```text
how to search
how to reason
which tools to use
which retrieval strategy to compile
which embedding instruction to apply
when to stop
when to report insufficient evidence
```

For Tensor Machine, this is not an external wrapper. It is a natural control layer above the existing DSL, IR, VM, hybrid retrieval, and MCP agent mode.

The target architecture:

```text
User request
  -> Skill registry retrieval
  -> Agent skill selection
  -> FSM/router state
  -> Embedding instruction selection
  -> Query projection generation
  -> Tensor Machine retrieval execution
  -> Fusion/reranking/evidence check
  -> Grounded answer or controlled action
```

Short formula:

```text
Tensor Machine = VM for retrieval operations.
Skill-RAG = planner for retrieval behavior.
Embedding Instruction-RAG = planner for semantic projection.
Together = an agentic operating layer for RAG.
```
