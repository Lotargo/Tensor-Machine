# Iterative Bug Report

Date: 2026-06-05

This document captures the currently confirmed bugs and architectural issues found during validation of the recent `rag3 -> tensor-rag` port and the new graph/UI work.

The goal is to fix the system iteratively instead of attempting a full rewrite in one pass.

---

## Status legend

- `open` - confirmed and not fixed
- `in_progress` - partially addressed, needs follow-up
- `fixed` - addressed and verified
- `deferred` - accepted, but intentionally postponed

---

## What is already confirmed

1. Mistral OCR is not the main ingestion bottleneck for large documents.
2. The main ingestion bottleneck was sequential per-chunk embedding requests.
3. Even after batching embeddings, the ingestion pipeline is still not fully pipelined:
   - GPU usage drops between batches
   - tail work still runs in smaller/single-item phases
   - database/vector writes remain too synchronous
4. Retrieval quality is currently too weak for exact phrase lookup and targeted QA.
5. The UI contains several legacy artifacts and multiple regressions in structure, scrolling, and chat presentation.

---

## Already fixed

### Ingestion / backend

- `fixed` Added request timeouts and phase logging for:
  - Mistral file upload
  - Mistral OCR
  - embedding server requests

- `fixed` Added race protection when switching documents in the frontend viewer.

- `fixed` Added cleanup of `anchors` / `edges` and anchor embeddings when deleting or clearing documents.

- `fixed` Switched main chunk embedding during ingestion from single-item requests to batched requests.

- `fixed` Added defensive limits for entity extraction by chunk size / entity count to avoid runaway CPU work.

---

## P0 - Critical issues

### 1. Ingestion throughput remains too low

Status: `in_progress`

Symptoms:
- GPU utilization rises during batch execution but drops hard between batches.
- The pipeline is not keeping the embedding server saturated.
- Large documents still take too long to index.

Confirmed causes:
- Embeddings were originally sent one chunk at a time.
- Even after embedding batching, the rest of the ingest pipeline still has serial phases.
- Tail work, including anchor embedding and writes, still degrades utilization.

Follow-up work:
- Batch anchor embeddings.
- Batch Qdrant upserts.
- Reduce synchronous `wait=true` behavior where safe.
- Introduce pipelining between:
  - chunk preparation
  - embedding requests
  - storage writes

Notes:
- `EMBEDDING_BATCH_SIZE` was introduced, but this is only the first step.

---

### 2. Retrieval quality is currently unacceptable

Status: `open`

Symptoms:
- Exact phrase lookup can fail even when the phrase is present in the document.
- Answers rely on huge context dumps instead of precise retrieval.
- Context overload reduces answer quality.

Examples:
- Query about a known mention of `Лиза` failed even though the document reportedly contains a direct match.
- The system often answers from broad nearby context rather than exact supporting spans.

Hypotheses:
- BM25 / dense / entity fusion is still poorly calibrated.
- Parent-context expansion is overwhelming precision.
- The current response builder passes too much text and too little ranking discipline.

Follow-up work:
- Re-check exact-match retrieval path.
- Tighten lexical search behavior.
- Reduce macro-context flooding.
- Revisit RSF / BM25 weighting and exact-match boosts.

---

## P1 - High-priority correctness and UX issues

### 3. Chunk count / indexing count mismatch in UI

Status: `open`

Symptoms:
- Real ingestion processed `3971` chunks.
- The frontend showed only `1000` segments.

Likely cause:
- `/documents` currently uses a hard limit and the frontend groups only what it receives.

Impact:
- The UI lies about document size.
- Viewer state is misleading.
- It becomes harder to reason about document completeness and order.

Follow-up work:
- Stop using the raw limited chunk list as the source of truth for document stats.
- Return grouped document metadata from the backend.

---

### 4. Token accounting and display are inconsistent

Status: `open`

Symptoms:
- Some UI/meta values are still based on `chars` while the project already uses `o200k_harmony`.
- This creates incorrect expectations about chunking and document size.

Impact:
- Misleading ingestion metrics.
- Misleading context budgeting expectations.

Follow-up work:
- Use tokenizer-based accounting wherever token semantics matter.
- Keep `chars` only as an auxiliary display metric.

---

### 5. Document structure is not preserved correctly in the viewer

Status: `open`

Symptoms:
- Document text appears as an unstructured plain text dump.
- Headings are not rendered with meaningful structure.
- Chapters/sections appear out of order.
- OCR markdown structure seems to be lost or flattened after processing.

Impact:
- Human validation becomes difficult.
- Hierarchy and TOC become unreliable.
- RAG debugging becomes harder.

Follow-up work:
- Trace where OCR markdown is flattened.
- Preserve heading order and structure into renderable document views.
- Add a real TOC path instead of relying on context hints.

---

### 6. Legacy summary injection should be removed

Status: `open`

Symptoms:
- Search responses still inject synthetic summary sections.
- This behavior comes from older assumptions and does not reflect real document summaries.

Impact:
- Pollutes context.
- Adds confusing fake structure.

Decision:
- Remove current summary injection.
- Do not replace it with BM25 / TF-IDF pseudo-summary generation.
- If summaries are needed later, use a real pipeline such as RAPTOR or model-generated hierarchy summaries.

---

### 7. Legacy TOC/search artifact is still present

Status: `open`

Symptoms:
- Search responses still build an internal `=== ДОКУМЕНТЫ В ПОИСКЕ ===` block.
- Chat/UI still has to filter around this artifact.

Impact:
- Pollutes the retrieval response format.
- Leaks implementation details into the UX.

Decision:
- Remove this compatibility artifact entirely.

---

### 8. Proximity gap artifact leaks into results

Status: `open`

Symptoms:
- Search/document output visibly contains `... [Proximity Gap] ...`

Impact:
- Bad UX.
- Distracts from content.
- Feels like a debugging marker rather than a product behavior.

Decision:
- Remove from user-facing output or replace with a non-intrusive structural rendering strategy.

---

### 9. Scrolling is broken or missing in multiple views

Status: `open`

Symptoms:
- Downward scrolling works reliably only in the modal text preview.
- Main tabs and content panels can trap the user visually.

Impact:
- Basic usability issue.

Follow-up work:
- Audit overflow containers.
- Ensure each main content region has proper vertical scrolling.

---

### 10. Graph visualization is too weak to be useful

Status: `open`

Symptoms:
- The graph often shows only the active document node.
- No meaningful structure is visible.
- The 2D canvas gives weak navigation value.
- The viewport is too small.

Impact:
- The feature exists visually but does not help analysis.

Follow-up work:
- Verify graph data population first.
- Add fullscreen / expanded graph mode.
- Reconsider layout and navigation model.
- Possibly move to a richer visualization approach once the backend graph is actually meaningful.

---

### 10a. Frontend becomes sluggish and freezes during tab switches and agent responses

Status: `open`

Symptoms:
- Noticeable UI freezes when switching between tabs such as:
  - `Текст`
  - `Иерархия`
  - `Сущности`
  - `Граф связей`
- Noticeable sluggishness while the agent is responding in chat.
- The interface feels too heavy once large documents and large derived views are loaded.

Likely causes:
- Large DOM trees rendered eagerly.
- Full rerendering of heavy panels on every tab switch.
- No virtualization / pagination for large entity lists and large hierarchical views.
- Expensive repeated parsing/rendering during chat updates.
- Some views likely rebuild from raw data too often instead of reusing cached derived structures.

Impact:
- The UI feels unstable and slow even when the backend is functioning.
- Large documents become difficult to inspect interactively.

Follow-up work:
- Add view-level profiling for tab switches and chat rendering.
- Introduce virtualization or pagination for large lists and clouds.
- Cache derived structures for hierarchy/entities/graph inputs.
- Avoid full rerender of inactive tabs.
- Consider incremental rendering for large panels.
- Evaluate whether a BFF-style layer is useful for pre-aggregated frontend payloads once the current data contracts are stabilized.

Notes:
- A BFF may help reduce frontend work, but should not be treated as the first fix.
- First priority is to eliminate obvious expensive rerenders and heavy client-side rebuilding.

---

## P1 - Entity pipeline issues

### 11. Entity extraction is too noisy

Status: `open`

Symptoms:
- Non-entities appear as entities.
- Pronouns, adjectives, filler words, and low-value tokens leak into the entity cloud.

Impact:
- Entities explorer becomes misleading.
- Entity-based search and graph edges lose value.

Notes:
- The current regex-based extractor is too weak as a semantic entity recognizer.

Follow-up work:
- Evaluate a better entity extraction library or model-backed NER.
- At minimum, strengthen heuristics to reject obvious junk.

---

### 12. Word forms are not normalized properly

Status: `open`

Symptoms:
- Inflected forms and endings remain fragmented.
- Similar concepts are counted separately.

Impact:
- Entity grouping is poor.
- Search and graph links fragment across grammatical forms.

Follow-up work:
- Introduce proper stemming / lemmatization.
- Snowball-style stemming was explicitly identified as a likely next step.

---

### 13. AI lookup from entity badges is not useful in the current design

Status: `open`

Symptoms:
- Clicking an entity and asking AI does not provide grounded understanding.
- The assistant sees scattered retrieved text but not enough coherent local context about the entity.

Impact:
- The feature exists, but does not produce reliable answers.

Decision:
- Rework before treating it as a stable feature.

---

## P1 - Agent/chat UX issues

### 14. Tool-use thinking indicator renders an artifact

Status: `open`

Symptoms:
- During MCP tool use, a small empty square/circle artifact appears in chat.

Impact:
- Visual glitch.

Follow-up work:
- Audit loading/thinking bubble rendering path in chat UI.

---

### 15. Tool/thinking text is not hidden appropriately

Status: `open`

Symptoms:
- Agent intermediate behavior is exposed in a rough way.
- The UX around tool-use state is unfinished.

Impact:
- Makes the chat feel noisy and unpolished.

---

## P2 - Medium-priority cleanup and evolution

### 16. Frontend asset quality regressed

Status: `deferred`

Symptoms:
- Custom PNG assets replaced higher-quality scalable iconography.
- Loading animation quality regressed.

Decision:
- Remove the custom PNG asset path.
- Restore SVG-based iconography.
- Restore or redesign modern loading animation after higher-priority backend fixes.

---

### 17. Parser/reader engine replacement discussion is deferred

Status: `deferred`

Notes:
- Replacing Mistral OCR with another parser/reader engine is not the current priority.
- Current evidence shows OCR is not the main bottleneck.
- Revisit only after ingestion and retrieval quality are stabilized.

---

## Proposed iterative order

### Phase 1
- Finish ingestion performance work:
  - batch anchor embeddings
  - batch vector writes
  - reduce serial waits
  - improve document metadata accuracy in UI

### Phase 2
- Fix retrieval correctness:
  - exact match behavior
  - reduce context flooding
  - remove fake summary / legacy TOC artifacts

### Phase 3
- Restore document structure:
  - ordering
  - headings
  - TOC
  - structured viewer rendering

### Phase 4
- Clean up entities and graph UX

### Phase 5
- Polish chat/agent UX and visual design

---

## Notes for future passes

- Do not treat the current graph UI as product-complete until the underlying graph data is meaningful.
- Do not invest in superficial UI polish before retrieval and document structure are trustworthy.
- Do not reintroduce legacy Mongo-era response artifacts into the new PostgreSQL-backed system.
