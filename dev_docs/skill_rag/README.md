# Skill-RAG for Agent Skills

## Status

Architectural development note, updated after reviewing the current `Tensor-Machine` / `tensor-rag` codebase and recent merged work.

The idea is no longer just abstract. The current codebase already contains most of the infrastructure needed to support Skill-RAG:

- MCP server/tool layer;
- JSON request parsing into internal planning structures;
- IR / VM execution model;
- Qdrant, PostgreSQL, and ML embedding connection layer;
- reranking primitives such as MMR and RRF;
- ColBERT-style late-interaction path;
- CPU/SIMD/GPU backend direction;
- horizontal MCP routing and sticky sessions;
- Arrow Flight service for structured inter-process / inter-instance data exchange;
- metrics, rate limiting, graceful shutdown, and circuit breaker direction.

The missing part is not the retrieval core. The missing part is a dedicated **skill domain layer**: manifests, skill registry, lazy loading, dependency/conflict tracking, and MCP tools designed specifically for agent skill discovery.

---

## Core idea

Use RAG not only as a knowledge retrieval layer, but as an adaptive skill acquisition layer for agents.

A planning agent receives a user task. Instead of carrying every possible instruction in context, it asks the Skill-RAG layer which operational skills are relevant.

```text
User task
  -> Planner Agent
  -> Skill Discovery Tool
  -> Candidate skill manifests
  -> Agent selects relevant skills
  -> Skill Loader Tool
  -> Lazy context injection
  -> Execution / verification
```

The important rule:

> Retrieval should search over the full skill content, but the agent should initially see only lightweight metadata.

The full instruction is loaded only after explicit selection.

This avoids two common failure modes:

1. The agent has too few instructions and lacks task-specific procedures.
2. The agent has too many instructions and becomes overconstrained or distracted.

Skill-RAG creates a middle layer: the agent can discover relevant skills, but it does not drown in all available instructions.

---

## Updated conclusion after code review

The current `tensor-rag` codebase appears well suited as the retrieval/compute foundation for Skill-RAG.

Estimated architectural fit: **75-80% ready**.

What already fits:

- MCP server already provides a natural tool surface.
- The tool registry pattern can be extended with `search_skills`, `load_skill`, and related tools.
- The existing retrieval/reranking core can score and diversify candidate skills.
- Existing MMR/RRF direction is useful for choosing a compact but diverse skill set.
- The current separation between I/O orchestration and VM execution is aligned with Skill-RAG.
- Arrow Flight can later support a federation of specialized RAG/MCP servers.
- Router/load-balancer work makes a future multi-server design natural.

What is missing:

- Skill manifest schema.
- Skill-specific indexing pipeline.
- Skill registry and ID system.
- Discovery tool that returns manifests only.
- Lazy loader that returns summary/section/full content.
- Conflict/dependency checker.
- Skill pack recommendation layer.
- Telemetry for selected/loaded/useful/failed skills.

The recommendation is to **not** turn the existing RAG into one universal super-RAG. Instead, keep the existing Tensor Machine core as a shared retrieval and execution backend, then add a specialized `skills-mcp` server or module on top.

---

## Why Skill-RAG is different from normal document RAG

A normal RAG system usually retrieves evidence for answering questions.

Skill-RAG retrieves operational procedures for modifying agent behavior during planning and execution.

That changes the ranking goal.

### Normal Docs-RAG optimizes for

- factual grounding;
- citations;
- source locality;
- answer completeness;
- current version relevance.

### Code-RAG optimizes for

- exact file paths;
- symbols and definitions;
- dependency relationships;
- call graph / AST proximity;
- implementation locality.

### Skill-RAG optimizes for

- applicability to task;
- operational usefulness;
- phase relevance: planning, coding, testing, review, deployment;
- injection cost;
- dependency requirements;
- conflicts with other skills;
- safety level;
- whether the skill should be loaded as summary, section, full content, or handle only.

Therefore, Skill-RAG should not simply be another document collection in a generic RAG index. It needs its own metadata and tool contract.

---

## Fit with current Tensor Machine architecture

The current architecture can be mapped to Skill-RAG like this:

```text
Existing Tensor Machine / tensor-rag

MCP request layer
  -> tool registry / ToolHandler
  -> query parsing / planning
  -> retrieval from Qdrant/Postgres/ML embedding layer
  -> IR / VM execution
  -> reranking / top-k / MMR / RRF / ColBERT
  -> FlatBuffer / structured MCP response
```

Skill-RAG extension:

```text
skills-mcp
  -> search_skills
  -> get_skill_manifest
  -> load_skill
  -> load_skill_sections
  -> list_skill_conflicts
  -> recommend_skill_pack
  -> record_skill_feedback
```

The Skill-RAG layer should reuse the core where it is strong:

```text
Retrieval core       -> semantic search over skill content
MMR/RRF              -> diversified skill candidate ranking
IR/VM                -> scoring, fusion, top-k, future policy checks
MCP tool registry    -> agent-facing surface
Router               -> future federation of skill/docs/code/memory servers
Arrow Flight         -> structured exchange between specialized services
Metrics             -> usage and effectiveness telemetry
```

---

## Proposed repository layout

A low-risk integration can be added without disrupting the main RAG path.

Possible layout:

```text
tensor-rag/
  src/
    skills/
      mod.rs
      manifest.rs
      registry.rs
      indexer.rs
      loader.rs
      conflict.rs
      packs.rs
      telemetry.rs
    mcp_skills.rs

  skills/
    rag/
      evaluation/
        synthetic_qa.md
        synthetic_qa.skill.yaml
    agents/
      planning/
        subtask_decomposition.md
        subtask_decomposition.skill.yaml
    frontend/
      landing_pages/
        hero_section.md
        hero_section.skill.yaml
```

Alternative if keeping servers physically separated:

```text
servers/
  skills-mcp/
  docs-mcp/
  code-mcp/
  memory-mcp/
  router-mcp/

core/
  retrieval/
  ranking/
  storage/
  protocol/
```

Recommended first move: implement inside the current repo as a module/prototype, then split later only if it becomes large enough.

---

## Skill manifest schema

Each skill should have a manifest next to the Markdown instruction file.

Example:

```yaml
id: rag.eval.synthetic_qa
title: Synthetic QA Generation for RAG Evaluation
version: 0.1.0
status: draft
category: rag/evaluation
owner: local
trust_level: trusted

summary: >
  Generate questions from source spans and evaluate RAG answers against grounded evidence.

tags:
  - rag
  - evaluation
  - qa
  - benchmark
  - source-grounding

applies_when:
  - A RAG pipeline needs quality evaluation.
  - Questions must be generated from indexed documents.
  - Retrieval errors must be separated from generation errors.
  - The agent needs to create a reproducible evaluation report.

phase:
  - planning
  - testing
  - verification

requires:
  - rag.source_grounding

conflicts_with:
  - rag.eval.manual_only

usually_combined_with:
  - agents.subagent_roles
  - testing.mock_first
  - reporting.markdown_report

injection_modes:
  - summary
  - section
  - full
  - handle_only

preferred_injection_mode: summary
max_context_tokens: 1800
safety_level: normal
priority: recommended

entrypoints:
  summary: "# Summary"
  protocol: "# Protocol"
  failure_modes: "# Failure modes"
  report_format: "# Report format"
```

The manifest should be indexed together with the full Markdown body, but discovery should return only manifest-level information.

---

## Skill data model

Minimum internal records:

```text
SkillManifest
  id: String
  title: String
  version: String
  status: draft | active | deprecated
  category: String
  tags: Vec<String>
  summary: String
  applies_when: Vec<String>
  phase: Vec<planning|execution|testing|review|deployment|debugging>
  requires: Vec<SkillId>
  conflicts_with: Vec<SkillId>
  usually_combined_with: Vec<SkillId>
  injection_modes: Vec<InjectionMode>
  preferred_injection_mode: InjectionMode
  max_context_tokens: usize
  safety_level: SafetyLevel
  priority: Priority
  path: String
  content_hash: String
  updated_at: Timestamp
```

```text
SkillChunk
  skill_id: String
  section_id: String
  heading_path: Vec<String>
  text: String
  token_count: usize
  embedding: Vec<f32>
  tags: Vec<String>
```

```text
SkillUsageEvent
  task_hash: String
  skill_id: String
  action: discovered | selected | loaded | rejected | used | failed
  mode: summary | section | full | handle_only
  agent_id: Option<String>
  timestamp: Timestamp
  notes: Option<String>
```

---

## MCP tool contract

The Skill-RAG MCP surface should be explicit. Do not overload the generic search tool.

### `search_skills`

Purpose: find relevant skill candidates for a task, but do not return full instructions.

Input:

```json
{
  "task": "Create a RAG evaluation pipeline for opencode with synthetic QA generation",
  "categories": ["rag", "agents", "evaluation", "testing"],
  "phase": "planning",
  "constraints": {
    "max_results": 8,
    "max_total_context_tokens": 5000,
    "include_deprecated": false,
    "prefer_injection_mode": "summary"
  }
}
```

Output:

```json
{
  "candidates": [
    {
      "id": "rag.eval.synthetic_qa",
      "title": "Synthetic QA Generation for RAG Evaluation",
      "path": "skills/rag/evaluation/synthetic_qa.md",
      "summary": "Generate questions from source spans and evaluate RAG answers against grounded evidence.",
      "tags": ["rag", "evaluation", "qa", "benchmark"],
      "score": 0.92,
      "recommended_mode": "summary",
      "estimated_tokens": 900,
      "match_reason": "The task asks for RAG evaluation and synthetic question generation."
    }
  ]
}
```

### `get_skill_manifest`

Purpose: inspect one skill without loading instruction content.

```json
{
  "skill_id": "rag.eval.synthetic_qa"
}
```

### `load_skill`

Purpose: load selected skill content lazily.

```json
{
  "skill_id": "rag.eval.synthetic_qa",
  "mode": "summary"
}
```

Supported modes:

```text
summary      -> compact operational summary and key rules
section      -> one or more selected sections
full         -> full Markdown instruction
handle_only  -> return a stable handle for later section reads
```

### `load_skill_sections`

Purpose: load only specific sections from a skill.

```json
{
  "skill_id": "rag.eval.synthetic_qa",
  "sections": ["protocol", "failure_modes", "report_format"]
}
```

### `list_skill_conflicts`

Purpose: check whether selected skills contradict each other.

```json
{
  "skill_ids": [
    "frontend.nextjs.app_router",
    "frontend.static_html_only"
  ]
}
```

### `recommend_skill_pack`

Purpose: recommend a compact bundle of required/recommended/optional skills.

```json
{
  "task": "Build a RAG evaluation pipeline for opencode",
  "phase": "planning",
  "budget": {
    "max_required": 3,
    "max_recommended": 5,
    "max_optional": 4
  }
}
```

### `record_skill_feedback`

Purpose: save telemetry after use.

```json
{
  "task_hash": "sha256:...",
  "skill_id": "rag.eval.synthetic_qa",
  "action": "used",
  "outcome": "helpful",
  "notes": "Helped structure source-span QA generation."
}
```

---

## Ranking model

Skill-RAG should use hybrid ranking, not a single embedding score.

Recommended scoring:

```text
score =
  semantic_similarity(task, skill_content)          * w_semantic
+ manifest_tag_match(categories, tags)              * w_tags
+ phase_match(requested_phase, skill.phase)         * w_phase
+ applies_when_match(task, applies_when)            * w_applicability
+ trust_priority_boost(trust_level, priority)       * w_trust
- context_cost_penalty(estimated_tokens)            * w_cost
- conflict_penalty(selected_skills, conflicts_with) * w_conflict
```

Then apply MMR or similar diversification over the final candidate set.

Why MMR matters here:

A task like "build RAG evaluation pipeline" may retrieve many near-duplicate evaluation skills. MMR can diversify the final list so the agent receives:

- one evaluation skill;
- one source-grounding skill;
- one subagent planning skill;
- one report-writing skill;
- one mock-first testing skill.

RRF is useful when combining multiple retrieval channels:

```text
semantic embedding ranking
+ tag/category ranking
+ BM25 keyword ranking
+ historical usefulness ranking
= fused ranking through RRF
```

---

## Lazy context injection modes

Skill loading should avoid dumping full instructions into context by default.

### `summary`

Returns a compact operational summary:

```text
- When to use
- Required steps
- Constraints
- Failure modes
- Output format
```

Best default for planning.

### `section`

Returns only selected sections from the Markdown body.

Best when the agent knows which part it needs.

### `full`

Returns the full instruction.

Use only when:

- the skill is short;
- the task is highly sensitive to the exact instruction;
- the agent explicitly needs the full protocol.

### `handle_only`

Returns a stable skill handle and manifest. The agent can request sections later.

Best for long-running tasks.

### `tool_bound`

Skill is active only when using a specific tool or phase.

Example:

```yaml
active_when:
  tool: github.update_file
  phase: code_review
```

---

## Integration with current MCP server

The current MCP server already has a tool registry pattern. Skill-RAG can be added as new handlers rather than changing the existing search path.

Possible Rust-side shape:

```rust
pub struct SearchSkillsTool;
pub struct GetSkillManifestTool;
pub struct LoadSkillTool;
pub struct LoadSkillSectionsTool;
pub struct ListSkillConflictsTool;
pub struct RecommendSkillPackTool;
```

Registration sketch:

```rust
server.register_tool("search_skills", Box::new(SearchSkillsTool::new(skill_registry.clone())));
server.register_tool("get_skill_manifest", Box::new(GetSkillManifestTool::new(skill_registry.clone())));
server.register_tool("load_skill", Box::new(LoadSkillTool::new(skill_registry.clone())));
```

The key design choice:

> Do not overload the generic `SearchTool` response format. Skill discovery has a different contract from document retrieval.

---

## Integration with IR / VM

The VM should not load Markdown files or talk to the filesystem directly.

Recommended boundary:

```text
MCP/orchestrator layer:
  - reads skill manifests
  - indexes skill content
  - fetches candidate records
  - resolves dependencies/conflicts
  - loads selected skill content

VM / IR layer:
  - computes ranking/fusion/top-k
  - applies MMR/RRF
  - may run future policy/scoring programs
  - remains isolated from direct I/O
```

This keeps the existing architecture clean: I/O remains outside the VM, while the VM remains useful for vector/score computation.

Potential future IR operations:

```text
ScoreSkillApplicability
ApplySkillBudget
PenalizeConflicts
FuseSkillRankings
DiversifySkillSet
```

These do not need to exist in MVP. They are future extensions if skill ranking becomes complex enough.

---

## Storage strategy

### Qdrant

Use for skill chunks and manifest-enriched vectors.

Possible payload fields:

```json
{
  "kind": "skill_chunk",
  "skill_id": "rag.eval.synthetic_qa",
  "section_id": "failure_modes",
  "title": "Synthetic QA Generation for RAG Evaluation",
  "category": "rag/evaluation",
  "tags": ["rag", "evaluation", "qa"],
  "phase": ["planning", "testing"],
  "path": "skills/rag/evaluation/synthetic_qa.md",
  "content_hash": "sha256:...",
  "status": "active",
  "trust_level": "trusted"
}
```

### PostgreSQL

Use for normalized metadata and telemetry:

```sql
skills
skill_chunks
skill_dependencies
skill_conflicts
skill_packs
skill_usage_events
```

### Filesystem / Git

Use Markdown + YAML as the source of truth:

```text
skills/**/*.md
skills/**/*.skill.yaml
```

Indexing should be reproducible from repository contents.

---

## Suggested database tables

```sql
CREATE TABLE skills (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  version TEXT NOT NULL,
  status TEXT NOT NULL,
  category TEXT NOT NULL,
  summary TEXT NOT NULL,
  path TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  trust_level TEXT NOT NULL,
  priority TEXT NOT NULL,
  preferred_injection_mode TEXT NOT NULL,
  max_context_tokens INTEGER NOT NULL,
  updated_at TIMESTAMPTZ DEFAULT now()
);
```

```sql
CREATE TABLE skill_edges (
  from_skill_id TEXT NOT NULL,
  to_skill_id TEXT NOT NULL,
  edge_type TEXT NOT NULL, -- requires | conflicts_with | usually_combined_with
  PRIMARY KEY (from_skill_id, to_skill_id, edge_type)
);
```

```sql
CREATE TABLE skill_usage_events (
  id BIGSERIAL PRIMARY KEY,
  task_hash TEXT NOT NULL,
  skill_id TEXT NOT NULL,
  action TEXT NOT NULL,
  mode TEXT,
  outcome TEXT,
  notes TEXT,
  created_at TIMESTAMPTZ DEFAULT now()
);
```

---

## Federation model

Avoid one overloaded super-RAG.

Recommended long-term architecture:

```text
Agent
  -> router/gateway MCP
       -> skills-rag MCP
       -> docs-rag MCP
       -> code-rag MCP
       -> project-memory MCP
       -> research-rag MCP
       -> verifier MCP
```

`tensor-rag` can provide the shared retrieval/compute core.

Specialized servers expose specialized contracts:

```text
skills-rag MCP       -> operational skills and lazy instruction loading
docs-rag MCP         -> factual docs and citations
code-rag MCP         -> code navigation and symbol retrieval
project-memory MCP   -> project decisions and temporal state
research-rag MCP     -> papers, experiments, references
verifier MCP         -> checks, tests, validation reports
```

Arrow Flight can be used later for structured exchange between instances, especially when transferring `RecordBatch` data, candidate sets, or evaluation results.

---

## Skill packs

Skill packs are predefined bundles for common task families.

Example:

```yaml
id: rag.evaluation.pipeline
task_type: rag_evaluation
required:
  - safety.evaluation_integrity
  - rag.source_grounding
recommended:
  - rag.eval.synthetic_qa
  - agents.subagent_roles
  - testing.mock_first
optional:
  - reporting.markdown_report
  - visualization.metrics_table
```

The planner can request a skill pack, inspect it, and then load only the pieces that fit the current task.

Skill packs should not blindly inject all skills. They are recommendation bundles, not mandatory context dumps.

---

## Safety and control

The agent should not have unlimited freedom to load arbitrary instructions.

Recommended guardrails:

### Skill budget

Limit loaded skills per planning cycle.

Example:

```yaml
budget:
  required: 3
  recommended: 5
  optional: 3
  max_context_tokens: 8000
```

### Required skills

Some skills should be loaded by policy rather than agent choice.

Examples:

```text
safety.evaluation_integrity
security.no_secret_exfiltration
code_review.no_destructive_changes_without_confirmation
```

### Conflict detection

Detect contradictory skills before injection.

Examples:

```text
frontend.nextjs_app_router conflicts_with frontend.static_html_only
rag.eval.synthetic_qa conflicts_with rag.eval.manual_only
```

### Provenance

Track:

- source path;
- content hash;
- author/source;
- trust level;
- last update time;
- deprecation status.

### Telemetry

Track:

- discovered;
- selected;
- loaded;
- rejected;
- used;
- failed;
- helped;
- caused conflict.

This enables future learning: the system can recommend skills that historically helped similar tasks.

---

## MVP implementation plan

### Phase 0: Keep current RAG untouched

Do not modify the main document search behavior.

Add Skill-RAG as a separate module or MCP tool group.

### Phase 1: File format and registry

Implement:

```text
skills/**/*.md
skills/**/*.skill.yaml
```

Add parser and validator:

```text
skills/manifest.rs
skills/registry.rs
```

Acceptance:

- invalid manifest fails clearly;
- duplicate skill IDs are rejected;
- missing Markdown file is rejected;
- content hash is computed.

### Phase 2: Discovery-only tool

Implement:

```text
search_skills(task, categories, phase, constraints)
```

Return manifests only. No full content yet.

Acceptance:

- search uses full content/manifest text;
- response returns only title/path/summary/tags/score/recommended mode;
- max result limit works.

### Phase 3: Lazy loader

Implement:

```text
load_skill(skill_id, mode)
load_skill_sections(skill_id, sections)
```

Acceptance:

- summary mode returns compact summary;
- section mode returns only requested sections;
- full mode returns full content;
- handle-only mode returns stable handle.

### Phase 4: Conflict/dependency layer

Implement:

```text
list_skill_conflicts(skill_ids)
resolve_skill_dependencies(skill_ids)
```

Acceptance:

- missing dependencies are reported;
- conflicts are reported before injection;
- recommended alternatives can be returned.

### Phase 5: Skill packs

Implement:

```text
recommend_skill_pack(task, phase, budget)
```

Acceptance:

- returns required/recommended/optional groups;
- respects budget;
- does not force-load optional skills.

### Phase 6: Telemetry

Implement:

```text
record_skill_feedback(...)
```

Acceptance:

- event logging works;
- skill effectiveness can be summarized later;
- telemetry does not block the main path if storage fails.

---

## Minimal first skill set

Start with 10-15 skills, not hundreds.

Suggested starter set:

```text
skills/rag/evaluation/synthetic_qa.md
skills/rag/evaluation/source_grounding.md
skills/rag/evaluation/retrieval_vs_generation_error.md
skills/agents/planning/subtask_decomposition.md
skills/agents/planning/subagent_roles.md
skills/testing/mock_first.md
skills/testing/regression_report.md
skills/github/safe_file_update.md
skills/docs/markdown_architecture_note.md
skills/reporting/implementation_plan.md
```

This is enough to test the full mechanism.

---

## Example end-to-end flow

User task:

```text
Build a RAG evaluation pipeline for opencode using synthetic questions from source spans.
```

Agent calls:

```json
{
  "tool": "search_skills",
  "arguments": {
    "task": "Build a RAG evaluation pipeline for opencode using synthetic questions from source spans.",
    "categories": ["rag", "evaluation", "agents", "testing"],
    "phase": "planning",
    "constraints": {
      "max_results": 8,
      "prefer_injection_mode": "summary"
    }
  }
}
```

Skill-RAG returns:

```text
rag.eval.synthetic_qa
rag.source_grounding
agents.subagent_roles
testing.mock_first
reporting.markdown_report
```

Agent selects three:

```json
{
  "tool": "load_skill",
  "arguments": {
    "skill_id": "rag.eval.synthetic_qa",
    "mode": "summary"
  }
}
```

Then checks conflicts:

```json
{
  "tool": "list_skill_conflicts",
  "arguments": {
    "skill_ids": [
      "rag.eval.synthetic_qa",
      "rag.source_grounding",
      "testing.mock_first"
    ]
  }
}
```

Finally it executes the plan with only the loaded operational instructions.

---

## Relation to Tensor Machine

This pattern fits Tensor Machine because it treats skills as structured operational memory rather than static prompt text.

The architecture becomes:

```text
Tensor Machine = vector/IR/VM/retrieval compute substrate
Skill-RAG      = adaptive operational memory layer
MCP            = agent-facing tool protocol
```

RAG becomes not only a retrieval layer for knowledge, but a retrieval layer for agent capability.

Possible framing:

> RAG can be used not only as a knowledge retrieval layer, but as an adaptive skill acquisition layer, allowing agents to discover, select, and lazily load task-specific operational instructions during planning.

Russian framing:

> RAG можно использовать не только как слой поиска знаний, но и как слой адаптивного подбора навыков, где агент во время планирования сам находит релевантные инструкции, выбирает нужные и лениво подключает их в рабочий контекст.

---

## Recommended next step

Do not build one universal super-RAG.

Build a focused Skill-RAG MVP on top of the existing Tensor Machine core:

```text
1. Add `skills/` directory with Markdown + YAML manifests.
2. Implement manifest parsing and validation.
3. Add `skills` module and registry.
4. Add `search_skills` MCP tool that returns manifests only.
5. Add `load_skill` and `load_skill_sections`.
6. Add conflict/dependency checks.
7. Add skill packs.
8. Add telemetry.
9. Later split into `skills-rag MCP` as part of a RAG federation.
```

Long-term direction:

```text
Not Super-RAG.
RAG Federation through MCP.
Tensor Machine core as shared retrieval/compute substrate.
```
