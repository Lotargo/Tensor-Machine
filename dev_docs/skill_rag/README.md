# Skill-RAG for Agent Skills

## Status

Early architectural idea / development note.

This document captures the idea of using RAG not only as a knowledge retrieval layer, but as an adaptive skill retrieval layer for agents.

The goal is to avoid forcing a large static instruction bundle into every agent context. Instead, the agent can discover relevant skills during planning, inspect only lightweight manifests first, and lazily load the full instructions only when they are actually needed.

---

## Core idea

A planning agent receives a user task, but it may not have all task-specific operational skills in its current context.

Instead of preloading every possible instruction, the agent can call a Skill-RAG discovery tool:

```text
User task
  -> Planner Agent
  -> Skill Discovery Tool
  -> Candidate skill list
  -> Agent selects relevant skills
  -> Skill Loader Tool
  -> Lazy context injection
  -> Execution / verification
```

The important detail: retrieval is performed over the full skill content, but the agent initially sees only compact metadata:

- skill id
- title
- path
- category
- short summary
- tags
- confidence / relevance score
- optional reason for match

The full instruction is loaded only after the agent explicitly selects it.

---

## Why this matters

Traditional agent setups often suffer from two extremes:

1. **Too few instructions** — the agent lacks task-specific operating procedures.
2. **Too many instructions** — the context is overloaded and the agent becomes overconstrained, confused, or biased by irrelevant rules.

Skill-RAG gives a middle path:

- the agent can ask for relevant skills when needed;
- selection is guided by semantic retrieval;
- context remains small;
- instructions are not injected by force;
- the agent can explain which skills it wants to load and why.

This turns RAG into an adaptive operational memory for agents.

---

## Proposed MCP tool split

The Skill-RAG layer should probably not be merged into one monolithic "super RAG". A better architecture is a federation of specialized RAG/MCP servers with a shared core.

Recommended direction:

```text
APL RAG Core
  -> skills-rag MCP
  -> docs-rag MCP
  -> code-rag MCP
  -> project-memory MCP
  -> research-rag MCP
  -> router/gateway MCP
```

The shared APL RAG core can provide common retrieval, indexing, ranking, storage, and protocol primitives.

Each MCP server can expose task-specific tools and metadata schemas.

---

## Why not one super-RAG

A single RAG that tries to handle skills, docs, code, memory, and research at once will likely mix incompatible retrieval goals.

Different domains need different ranking criteria:

### Docs-RAG

- factual accuracy
- citations
- source locality
- current version relevance

### Code-RAG

- symbols
- file paths
- call graph
- AST-aware retrieval
- dependency relationships

### Skills-RAG

- applicability
- operational usefulness
- injection mode
- conflicts
- required prerequisites
- safety level

### Memory-RAG

- time relevance
- project state
- previous decisions
- consistency with current context

Mixing all of these into one index and one tool surface may produce noisy retrieval and poor routing.

The better pattern is:

```text
Not Super-RAG.
RAG Federation through MCP.
```

---

## Minimal MVP

Create a dedicated Skill-RAG variant first, for example:

```text
apl-rag-skills
```

MVP requirements:

1. Index Markdown skill instructions.
2. Retrieve skills by content using task description and optional categories.
3. Return only lightweight manifests during discovery.
4. Allow the agent to load selected skills later.
5. Support several injection modes:
   - `summary`
   - `section`
   - `full`
   - `handle_only`
6. Track conflicts and required dependencies between skills.

---

## Suggested MCP tools

```text
search_skills(task, categories, constraints)
get_skill_manifest(skill_id)
load_skill(skill_id, mode)
load_skill_sections(skill_id, sections)
list_skill_conflicts(skill_ids)
recommend_skill_pack(task)
```

### `search_skills`

Input example:

```json
{
  "task": "Create a RAG evaluation pipeline for opencode with synthetic QA generation",
  "categories": ["rag", "agents", "evaluation", "testing"],
  "constraints": {
    "max_results": 8,
    "mode": "planning"
  }
}
```

Output example:

```json
[
  {
    "id": "rag.eval.synthetic_qa",
    "title": "Synthetic QA Generation for RAG Evaluation",
    "path": "skills/rag/evaluation/synthetic_qa.md",
    "summary": "Generate questions from source spans and evaluate RAG answers against grounded evidence.",
    "tags": ["rag", "evaluation", "qa", "benchmark"],
    "score": 0.92,
    "match_reason": "The task asks for RAG evaluation and synthetic question generation."
  }
]
```

### `load_skill`

Input example:

```json
{
  "skill_id": "rag.eval.synthetic_qa",
  "mode": "summary"
}
```

Possible modes:

- `summary` — compact operational summary;
- `section` — selected sections only;
- `full` — full instruction text;
- `handle_only` — returns a reference/handle for later reading.

---

## Skill manifest format

Each skill should have a manifest next to the instruction file.

Example:

```yaml
id: rag.eval.synthetic_qa
title: Synthetic QA Generation for RAG Evaluation
category: rag/evaluation
tags:
  - rag
  - evaluation
  - qa
  - benchmark
applies_when:
  - A RAG pipeline needs quality evaluation.
  - Questions must be generated from source documents.
  - Retrieval errors must be separated from generation errors.
requires:
  - rag.source_grounding
conflicts_with:
  - rag.eval.manual_only
injection_modes:
  - summary
  - section
  - full
priority: recommended
safety_level: normal
```

A possible directory layout:

```text
skills/
  rag/
    evaluation/
      synthetic_qa.md
      synthetic_qa.skill.yaml
  agents/
    planning/
      subtask_decomposition.md
      subtask_decomposition.skill.yaml
```

---

## Lazy context injection

Skill loading should avoid dumping large instructions into the context by default.

Possible modes:

### Soft injection

Inject only a compact summary and key rules.

### Section injection

Inject only relevant sections from the selected skill.

### Full injection

Inject the entire skill when the task genuinely requires it.

### Handle-based injection

Give the agent a stable skill handle and let it request specific sections later.

### Tool-bound injection

Make the skill active only when calling a specific tool or executing a specific phase.

---

## Safety and control

The agent should not have unlimited freedom to load arbitrary instructions.

Recommended guardrails:

1. **Skill budget**
   - Limit loaded skills per planning cycle.
   - Example: 3 required + 5 optional.

2. **Required skills**
   - Some safety or integrity instructions should be loaded by policy, not agent choice.

3. **Conflict detection**
   - Detect contradictory skills before injection.
   - Example: one skill requires Next.js, another requires pure static HTML.

4. **Skill provenance**
   - Track where each skill came from, when it was updated, and whether it is trusted.

5. **Telemetry**
   - Track which skills were selected, loaded, used, ignored, or associated with failures.

---

## Skill packs

A useful extension is automatic skill pack recommendation.

Example:

```yaml
skill_pack:
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

The planner may request a recommended pack and then decide which optional skills to load.

---

## Relation to Tensor Machine

This pattern fits the Tensor Machine direction because it treats skills as structured operational memory rather than static prompt text.

RAG becomes not only a retrieval layer for knowledge, but a retrieval layer for agent capability.

Possible framing:

> RAG can be used not only as a knowledge retrieval layer, but as an adaptive skill acquisition layer, allowing agents to discover, select, and lazily load task-specific operational instructions during planning.

Russian framing:

> RAG можно использовать не только как слой поиска знаний, но и как слой адаптивного подбора навыков, где агент во время планирования сам находит релевантные инструкции, выбирает нужные и лениво подключает их в рабочий контекст.

---

## Recommended next step

Do not immediately build one universal RAG.

Start with a focused `apl-rag-skills` prototype:

```text
1. Reuse the existing APL RAG core.
2. Add skill manifests.
3. Implement skill discovery.
4. Implement lazy skill loading.
5. Add conflict/dependency metadata.
6. Expose the flow as a dedicated MCP server.
7. Later add a router MCP that can federate skills-rag, docs-rag, code-rag, memory-rag, and research-rag.
```

The long-term architecture should be a federation of specialized RAG servers, not a single overloaded RAG monolith.
