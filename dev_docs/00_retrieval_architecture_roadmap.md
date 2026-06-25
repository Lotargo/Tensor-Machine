# Retrieval Architecture Roadmap

Status: working roadmap  
Date: 2026-06-25  
Scope: short implementation order for ingestion, tool compatibility, skill-routed retrieval, disambiguation, and adaptive memory

---

## Principle

Build deterministic foundations before adaptive behavior.

```text
tool protocol safety
  -> structured ingestion
  -> narrow retrieval skills
  -> skill-routed preflight
  -> disambiguation
  -> adaptive memory
```

Do not enable a later adaptive layer until the earlier layer has regression tests and observable metrics.

---

## Document Order

Use numeric prefixes as the implementation/read order. Lower numbers are earlier or already foundational; higher numbers are later or more adaptive.

```text
00_retrieval_architecture_roadmap.md
10_tool_protocol_safety.md                  reserved
20_mistral_first_ingestion_and_policy_chunking.md
30_skill_routed_retrieval_architecture.md
50_active_disambiguation_mcp_skills.md
60_adaptive_cluster_memory.md
```

---

## Phase 1. Tool Protocol Safety

Goal: prevent model-specific tool-call failures from leaking into the UI.

Implement:

- [ ] Harden DSML/tool-call parsing for DeepSeek-like models.
- [ ] Add final-answer guard: never show raw tool-call markup to the user.
- [ ] Add optional strict JSON tool-decision mode.
- [ ] Increase `AI_AGENT_MAX_TOOL_ROUNDS` default and hard cap.
- [ ] Detect repeated identical tool calls and stop loops cleanly.

Must verify:

- [ ] Native OpenAI `tool_calls` still work.
- [ ] DeepSeek-like DSML output is parsed or rejected safely.
- [ ] Malformed tool calls do not appear in chat output.
- [ ] Tool-round exhaustion returns a controlled diagnostic, not raw markup.
- [ ] 1-hop and 2-hop tool workflows complete within the new round budget.

---

## Phase 2. Structured Ingestion

Goal: create metadata-rich memory that retrieval skills can navigate.

Implement:

- [ ] Add Mistral-first parsed document contract.
- [ ] Add policy-driven chunking interface.
- [ ] Add `chunk.mistral_ocr_markdown` policy.
- [ ] Store parsing backend, chunking policy, embedding template, pages, headings, tables, images, and anchors in metadata.
- [ ] Add page/header-aware embedding text.

Must verify:

- [ ] Headings and chapter anchors survive ingestion.
- [ ] Tables stay connected to headers.
- [ ] Images stay connected to captions and nearby text.
- [ ] Page metadata is present in retrieved chunks.
- [ ] Retrieval recall improves or at least does not regress on existing test documents.

---

## Phase 3. Narrow Retrieval Skills

Goal: add a small number of high-value skills before general skill routing.

Implement:

- [ ] `chapter_section_lookup`
- [ ] `exact_lookup`
- [ ] `evidence_first_answering`
- [ ] Provider-specific tool policy snippets.
- [ ] Compact skill-card registry.

Must verify:

- [ ] Chapter lookup uses metadata/anchors before filtered search.
- [ ] Exact config/API lookup prefers lexical and structural signals.
- [ ] Evidence-first answers cite retrieved local evidence only.
- [ ] Wrong skill activation rate is measured.
- [ ] Skills remain compact and do not flood the model context.

---

## Phase 4. Skill-Routed Preflight

Goal: replace single preflight search with a small planner.

Implement:

- [ ] Intent enum: `EXACT_LOOKUP`, `SECTION_LOOKUP`, `VERIFY_CLAIM`, `SUMMARIZE_DOCUMENTS`, `DIAGNOSE_RAG_FAILURE`.
- [ ] `skill_preflight` return type with selected skills, projections, retrieval plan, and evidence policy.
- [ ] Query projection builder for 2-5 projections.
- [ ] Fusion and deduplication across projected retrieval and existing retrieval channels.
- [ ] Debug output showing selected mode and projections.

Must verify:

- [ ] Simple questions do not over-trigger complex skills.
- [ ] Section lookup is better than baseline search.
- [ ] Evidence policy correctly reports insufficient local context.
- [ ] Latency overhead is acceptable.
- [ ] Retrieval quality is measured before and after skill routing.

---

## Phase 5. Active Disambiguation

Goal: ask one useful question when the query is genuinely ambiguous.

Implement:

- [ ] Ambiguity metadata: confidence, score gap, competing hypotheses, recommended action.
- [ ] `rag_disambiguation_skill` prompt/policy.
- [ ] Optional `analyze_ambiguity` and `refine_search` tools.
- [ ] Clarification outcome logging.

Must verify:

- [ ] The system asks at most one clarification question per ambiguous turn.
- [ ] It does not ask when confidence is high.
- [ ] Clarification improves retrieval on ambiguous test cases.
- [ ] User-facing questions avoid internal jargon.
- [ ] Feature can be disabled cleanly.

---

## Phase 6. Adaptive Retrieval Memory

Goal: let successful retrieval routes weakly influence future search.

Implement:

- [ ] Disabled-by-default smart-cache config.
- [ ] Retrieval trace data model.
- [ ] TTL, confidence, negative feedback, and quarantine.
- [ ] Weak RSF channel for smart-cache.
- [ ] Batch learned-cluster memory later.

Must verify:

- [ ] Memory never replaces source-grounded evidence.
- [ ] Memory weights are bounded and observable.
- [ ] Negative feedback removes or quarantines entries.
- [ ] Tenant isolation is enforced.
- [ ] Disabling the feature restores normal retrieval behavior.

---

## Release Gate

Before enabling any phase by default:

- [ ] Add regression tests.
- [ ] Add metrics or logs for the new behavior.
- [ ] Compare against baseline retrieval.
- [ ] Confirm no raw protocol artifacts reach the UI.
- [ ] Document the rollback switch.
