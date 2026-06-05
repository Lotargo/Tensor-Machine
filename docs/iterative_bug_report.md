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

## Positive validation notes

- `validated` Retrieval and answer quality showed a strong positive result on `Alice_in_Wonderland_Zakhoder.docx`.
  - The source DOCX is unusual: it contains embedded HTML markup, and after Mistral OCR the output became Markdown while the internal HTML-like structure still remained.
  - Query: `кому хозяин велел лезть в трубу?`
    - The assistant correctly answered that Bill was ordered to climb into the chimney/pipe, and connected it to the White Rabbit house scene.
  - Query: `кто сказал слова "НИ РАЖУ" ?`
    - The assistant correctly identified the King and provided the relevant courtroom/pun context.
  - The answers were manually verified against the retrieved content shown in chat.

This does not close retrieval quality work, but it is an important positive signal: the current retrieval pipeline can handle some targeted literary QA correctly even on a document with messy DOCX/HTML/Markdown structure.

---

## Validation dataset

- Local regression / manual validation documents are stored in `F:\doc_for_tests`.
- The folder contains a small document pack covering multiple scenarios:
  - small documents
  - large documents
  - OCR-heavy documents
  - mixed formatting cases
- Use this folder for future ingestion, retrieval, viewer, chapter/heading search, and UI regression checks before treating related fixes as verified.

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

- `fixed` Batched anchor embeddings instead of embedding anchors one by one at the tail of ingestion.

- `fixed` Batched Qdrant upserts for chunk and anchor embeddings during ingestion, and made Qdrant upsert waiting configurable via `QDRANT_UPSERT_WAIT`.

- `fixed` Replaced the `/documents` raw chunk listing with grouped document metadata so UI chunk counts are no longer truncated by the old `LIMIT 1000` path.

- `fixed` Prevented the graph integration test from using the normal application `DATABASE_URL`; it previously deleted real documents and left `doc-a`...`doc-d` mock rows in the live library.

- `fixed` Removed legacy retrieval response artifacts:
  - synthetic summary injection
  - internal `=== ДОКУМЕНТЫ В ПОИСКЕ ===` TOC block
  - user-facing `Proximity Gap` markers

- `in_progress` Tightened lexical retrieval with explicit exact/substring matching boosts and a stronger keyword candidate fallback path.

- `in_progress` Reduced frontend chat/tab rendering cost by:
  - throttling streaming chat rerenders
  - lazily building hierarchy nodes
  - capping rendered entity/keyword badges
  - removing the empty tool-use bubble artifact

- `fixed` Made the local portal URL more robust:
  - the server now prints `http://127.0.0.1:<port>` instead of hardcoded `http://localhost:8080`
  - the router bind address can be configured with `MCP_BIND_ADDR` / `TENSOR_RAG_BIND_ADDR`
  - when the default address is not explicitly configured and port `8080` is unavailable, the router tries fallback ports `8081..8090`
  - this avoids the observed Windows/Docker/WSL case where `localhost:8080` resolves to a different IPv6 listener and returns `ERR_EMPTY_RESPONSE` while the app is actually reachable over IPv4 loopback

---

## P0 - Critical issues

### 1. Ingestion throughput remains too low

Status: `in_progress`

Symptoms:
- GPU utilization rises during batch execution but drops hard between batches.
- The pipeline is not keeping the embedding server saturated.
- Large documents still take too long to index.
- After switching PostgreSQL writes inside each batch to multi-row inserts, a real document ingest still shows roughly a 2 second gap between embedding batches.

Confirmed causes:
- Embeddings were originally sent one chunk at a time.
- Even after embedding batching, the rest of the ingest pipeline still has serial phases.
- Tail work, especially storage writes, still degrades utilization.

Completed in this area:
- Main chunk embeddings are batched.
- Anchor embeddings are now batched too.
- Qdrant upserts for chunks and anchors are batched per embedding batch.
- Qdrant upsert waiting is configurable with `QDRANT_UPSERT_WAIT` and defaults to non-blocking writes.
- PostgreSQL chunk writes inside each embedding batch now use a single multi-row `INSERT ... RETURNING id` instead of one sequential insert per chunk.

Follow-up work:
- Validate ingestion throughput against a large real document with Qdrant enabled.
- Introduce pipelining between:
  - chunk preparation
  - embedding requests
  - storage writes

Notes:
- `EMBEDDING_BATCH_SIZE` was introduced, but this is only the first step.
- The remaining inter-batch delay needs a dedicated profiling pass; do not assume PostgreSQL row-by-row inserts were the only cause.

---

### 2. Retrieval quality is currently unacceptable

Status: `in_progress`

Symptoms:
- Exact phrase lookup can fail even when the phrase is present in the document.
- Answers rely on huge context dumps instead of precise retrieval.
- Context overload reduces answer quality.

Examples:
- Query about a known mention of `Лиза` failed even though the document reportedly contains a direct match.
- The system often answers from broad nearby context rather than exact supporting spans.

Current status:
- Exact/substring lexical scoring was strengthened.
- Keyword candidate fallback was strengthened.
- Structured PostgreSQL candidate fallback now adds chapter/header-chain matches before dense vector reranking.
- Legacy context-bloat from summary injection / TOC artifact / proximity markers was removed.

Remaining hypotheses:
- BM25 / dense / entity fusion is still poorly calibrated.
- `deferred` The current response builder could pass less text for targeted QA (~2-3k tokens observed), but reducing context flooding risks retrieval regressions. Deferred — revisit after knowledge graph integration when context shaping becomes more relevant.
- Qdrant/dense retrieval is not suitable as the only mechanism for precise chapter, heading, line, or paragraph lookup; PostgreSQL-backed lexical/structured search should be expanded for exact navigation and extraction tasks.

Follow-up work:
- `fixed` Re-check exact-match retrieval path — root cause identified: default `RerankStrategy::SemanticOnly` skipped BM25/exact-match scoring entirely.
  - `fixed` Changed default strategy from `SemanticOnly` to `Hybrid { semantic_weight: 0.7, bm25_weight: 0.3 }` in `ir.rs` and `dsl.rs`.
  - `fixed` Increased exact-match boost in `fetch_bm25_scores()` from 5.0 to 10.0.
- `fixed` Validate exact-match behavior against known failing examples.
  - Document: `Утро_ч.1_в.1.docx` (~190k tokens, single mention of `Лиза`).
  - Query 1: `расскажи что упоминалось про Лизу` → retrieved correct chunk, identified Лиза as Arseniy's mother.
  - Query 2: `как звали мать Арсения?` → correct exact answer with citation, fresh context.
- Revisit RSF / BM25 weighting and exact-match boosts.
- `in_progress` Add chapter/heading search that can locate a chapter or heading and retrieve its full text span.
  - `fixed` Added structured candidate lookup for `chapter_id` and `header_chain` so navigation queries are not dense-only.
  - `fixed` Search responses now expand explicit chapter/heading hits into their macro-level span before response assembly, instead of returning only the individual matched chunks.
  - Remaining: validate full chapter/section reconstruction against large real OCR markdown documents and tune span size if macro chunks are too broad or too narrow.
- Add precise lookup for a specific line/string and paragraph within headings/sections.
- Extend and test search methods so exact chapter/heading/paragraph retrieval can use PostgreSQL lexical/structured queries instead of relying on noisy dense vector search.

---

## P1 - High-priority correctness and UX issues

### 3. Chunk count / indexing count mismatch in UI

Status: `fixed`

Symptoms:
- Real ingestion processed `3971` chunks.
- The frontend showed only `1000` segments.

Resolution:
- `/documents` no longer exposes the raw limited chunk list as the source of truth.
- The backend now returns grouped document metadata and the frontend uses that directly.

Impact:
- The UI lies about document size.
- Viewer state is misleading.
- It becomes harder to reason about document completeness and order.

Follow-up work:
- Keep validating counts against very large documents after future pagination/virtualization work.

---

### 4. Token accounting and display are inconsistent

Status: `fixed`

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

Status: `in_progress`

Symptoms:
- Document text appears as an unstructured plain text dump.
- Headings are not rendered with meaningful structure.
- Chapters/sections appear out of order.
- OCR markdown structure seems to be lost or flattened after processing.

Impact:
- Human validation becomes difficult.
- Hierarchy and TOC become unreliable.
- RAG debugging becomes harder.

Completed in this area:
- Viewer text reconstruction no longer concatenates every hierarchy level (`macro` + `medium` + `micro`) into one repeated plain-text dump.
- `/documents` list metadata now uses the largest available display level for preview and character count while preserving the full indexed chunk count.

Follow-up work:
- Validate heading/order rendering against a large real OCR markdown document.
- Add a real TOC path instead of relying on context hints.

---

### 6. Legacy summary injection should be removed

Status: `fixed`

Symptoms:
- Search responses still inject synthetic summary sections.
- This behavior comes from older assumptions and does not reflect real document summaries.

Impact:
- Pollutes context.
- Adds confusing fake structure.

Resolution:
- Current summary injection was removed.

Decision:
- Do not replace it with BM25 / TF-IDF pseudo-summary generation.
- If summaries are needed later, use a real pipeline such as RAPTOR or model-generated hierarchy summaries.

---

### 7. Legacy TOC/search artifact is still present

Status: `fixed`

Symptoms:
- Search responses still build an internal `=== ДОКУМЕНТЫ В ПОИСКЕ ===` block.
- Chat/UI still has to filter around this artifact.

Impact:
- Pollutes the retrieval response format.
- Leaks implementation details into the UX.

Resolution:
- This compatibility artifact was removed from user-facing retrieval responses.

---

### 8. Proximity gap artifact leaks into results

Status: `fixed`

Symptoms:
- Search/document output visibly contains `... [Proximity Gap] ...`

Impact:
- Bad UX.
- Distracts from content.
- Feels like a debugging marker rather than a product behavior.

Resolution:
- Removed from user-facing retrieval output.

---

### 9. Scrolling is broken or missing in multiple views

Status: `in_progress`

Symptoms:
- Downward scrolling works reliably only in the modal text preview.
- Main tabs and content panels can trap the user visually.

Impact:
- Basic usability issue.

Completed in this area:
- Main viewer tabs now use flex-based content panes instead of block-level tab containers.
- Text, hierarchy, and entity panels now own their vertical scrolling instead of relying on clipped parent containers or fixed inline max-heights.

Follow-up work:
- Validate scrolling visually with a large real document in the running app.
- Continue auditing secondary panels and modals after graph/fullscreen work.

---

### 10. Graph visualization is too weak to be useful

Status: `open`

Symptoms:
- The graph often shows only the active document node.
- No meaningful structure is visible.
- The 2D canvas gives weak navigation value.
- The viewport is too small.
- The `Граф связей` tab visualization is still not resolved and should not be treated as product-ready.

Impact:
- The feature exists visually but does not help analysis.

Follow-up work:
- `fixed` Graph auto-linking now groups shared entities by hot-path normalized keys instead of raw labels, so inflected RU/EN forms can create `shares_entities` edges.
- Verify graph data population against real documents in the running app.
- Add fullscreen / expanded graph mode.
- Reconsider layout and navigation model.
- Possibly move to a richer visualization approach once the backend graph is actually meaningful.

---

### 10a. Frontend becomes sluggish and freezes during tab switches and agent responses

Status: `in_progress`

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

Completed in this area:
- Streaming chat updates are now throttled instead of forcing a full rerender on every token.
- The empty tool-use bubble artifact was removed.
- Hierarchy nodes are now built lazily on expand instead of eagerly.
- Entity/keyword badge rendering is capped to reduce heavy tab payloads.
- Heavy content, hierarchy, and entity tab renders are now cached per selected document details fetch instead of rebuilding on every tab switch.
- Main document tabs now use stable flex/scroll containers, reducing layout churn during tab switches.

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

### Entity pipeline direction: hot path + cold enrichment

Decision:
- Split entity processing into two phases instead of trying to solve all morphology, NER, aliasing, and domain cleanup in the synchronous ingestion path.

Hot path:
- Must stay fast, deterministic, and cheap enough to run during normal ingestion and search.
- Should use established lightweight libraries/frameworks where possible, not hand-written suffix lists or domain-specific hardcode.
- Responsibilities:
  - language detection when reliable enough;
  - tokenization and stopword filtering;
  - library-backed stemming or light normalization;
  - conservative regex/statistical entity candidates;
  - immediate searchable keys for retrieval and graph bootstrapping.
- Should not depend on LLM calls or heavy ML inference.
- Should avoid domain-specific rules in the shared normalizer. Domain dictionaries may exist later, but they should be explicit configuration/extensions rather than hidden generic logic.

Cold background phase:
- Runs asynchronously after the document is already indexed.
- May use local ML models, language-specific NER/lemmatization models, or an LLM provider.
- Should be configurable independently from the main chat/agent model.
- Responsibilities:
  - model-backed NER;
  - remove false-positive entities produced by the hot path;
  - lemmatize/canonicalize entity names;
  - merge aliases and inflected forms;
  - classify entity types such as person, organization, place, concept, document section, legal term, or domain-specific term;
  - persist canonical entities, aliases, mentions, confidence scores, and processing status;
  - trigger graph edge recalculation once enriched entities are available.

Configuration direction:
- Add separate settings for entity enrichment, for example:
  - `ENTITY_ENRICHMENT_ENABLED`
  - `ENTITY_ENRICHMENT_MODE=hot_only|local_ml|llm|hybrid`
  - `ENTITY_ENRICHMENT_PROVIDER=ollama|lmstudio|openai|mistral|custom`
  - `ENTITY_ENRICHMENT_MODEL`
  - `ENTITY_ENRICHMENT_LANGS`
  - `ENTITY_ENRICHMENT_BATCH_SIZE`
  - `ENTITY_ENRICHMENT_MAX_CONTEXT_TOKENS`
- Local ML should be selectable per supported language.
- LLM mode should work as a universal fallback or as a higher-quality enrichment mode when the user explicitly enables it.
- The enrichment model choice must be separate from the normal assistant/chat model so users can choose a small cheap model for background cleanup or a stronger model for high-quality canonicalization.

State model direction:
- The UI/backend should eventually expose enrichment state separately from base indexing, for example:
  - `indexed`
  - `entity_enrichment_pending`
  - `entity_enrichment_running`
  - `entity_enrichment_complete`
  - `entity_enrichment_failed`
- Retrieval should work immediately after hot indexing, then improve as cold enrichment finishes.
- Graph quality should be treated as provisional until cold entity enrichment has completed for the relevant documents.

### 11. Entity extraction is too noisy

Status: `in_progress`

Symptoms:
- Non-entities appear as entities.
- Pronouns, adjectives, filler words, and low-value tokens leak into the entity cloud.
- The hot regex-based phase can still produce false positives that require model-backed cleanup.

Impact:
- Entities explorer becomes misleading.
- Entity-based search and graph edges lose value.

Completed in this area:
- Added defensive limits for entity extraction by chunk size / entity count to avoid runaway CPU work.
- Strengthened simple heuristics enough to reduce the worst hot-path noise.

Notes:
- The current regex-based extractor is too weak as a semantic entity recognizer and should be treated as a candidate generator, not as final NER.
- Do not keep expanding generic hardcoded suffix/entity/domain rules in the hot path.

Follow-up work:
- Add a cold background NER/enrichment phase that can remove false positives from hot extraction.
- Evaluate local language-specific NER/lemmatization models.
- Add optional LLM-backed cleanup/canonicalization using the existing AI provider architecture but separate model settings.
- Store confidence/source metadata so UI and graph logic can distinguish raw candidates from enriched entities.

---

### 12. Word forms are not normalized properly

Status: `in_progress`

Symptoms:
- Inflected forms and endings remain fragmented.
- Similar concepts are counted separately.

Impact:
- Entity grouping is poor.
- Search and graph links fragment across grammatical forms.

Follow-up work:
- `fixed` Introduced library-backed hot-path stemming for Russian and English with `rust-stemmers`.
- `fixed` Entity retrieval boosting now compares stemmed entity keys instead of only raw lowercase labels.
- `fixed` Graph auto-linking now uses stemmed entity keys to reduce fragmentation across word forms.
- Do not rely on hand-written suffix lists as the long-term solution.
- Add cold canonicalization to merge inflected forms and aliases using local ML or LLM-backed enrichment.
- Store canonical entity IDs/keys separately from display labels and raw mentions.

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

Status: `fixed`

Symptoms:
- During MCP tool use, a small empty square/circle artifact appears in chat.

Impact:
- Visual glitch.

Resolution:
- The empty tool-use bubble artifact in chat was removed.

---

### 15. Tool/thinking text is not hidden appropriately

Status: `in_progress`

Symptoms:
- Agent intermediate behavior is exposed in a rough way.
- The UX around tool-use state is unfinished.

Impact:
- Makes the chat feel noisy and unpolished.

Completed in this area:
- Tool-use stream events now render a generic localized thinking label instead of exposing raw tool labels directly in chat.

Follow-up work:
- Validate agent-mode streaming with real MCP tool calls.
- Decide whether final UI should show a structured tool activity timeline, keep only generic thinking states, or hide tool activity entirely.

---

### 15a. RAG source results overlap with agent answer in chat

Status: `in_progress`

Symptoms:
- Search/RAG results appear in chat first.
- The agent answer is then rendered above or over the retrieval results, creating a visually confusing experience.
- Retrieved context is useful for verification, but it currently competes with the final answer.

Impact:
- The chat feels noisy and visually unstable.
- Good answers are harder to read because internal retrieval context is shown too early.

Follow-up work:
- Validate the new source layout visually while an agent/RAG response is streaming.
- Modernize the source/results presentation so verification remains available without dominating the chat.
- Rework chat answer/source layout as part of the broader UI/UX redesign.

Completed in this area:
- Retrieval/source results are no longer rendered while the assistant response is loading/thinking.
- Sources are shown only after the assistant answer is complete.
- Sources now render under a collapsed disclosure block so verification remains available without dominating the chat.

---

## P2 - Medium-priority cleanup and evolution

### 16. Frontend asset quality regressed

Status: `in_progress`

Symptoms:
- Custom PNG assets replaced higher-quality scalable iconography.
- Loading animation quality regressed.
- The upload/status bar looks worse after recent visual edits.
- Status icons are visually dull.
- The upload/status progress no longer updates reliably after document upload; refreshing the page is needed to see the final state.

Decision:
- Remove the custom PNG asset path.
- Restore SVG-based iconography.
- Restore or redesign modern loading animation after higher-priority backend fixes.

Follow-up work:
- `fixed` Removed custom PNG icons from the active UI path and restored Font Awesome / scalable icon rendering for shell, toolbar, chat, upload, and reset controls.
- `fixed` Restored stateful upload/status step icons for pending, active, completed, and failed states.
- `fixed` Moved the base UI palette away from the regressed brown/espresso look toward a more neutral operational palette with distinct status accents.
- Restore a clearer animated upload/indexing progress indicator.
- Ensure the status bar transitions through upload, OCR, tokenization/chunking, embedding, and DB indexing without requiring a page refresh.
  - Current frontend now updates final state and selected document after upload completion, but true live phase progress still needs backend progress events or polling; otherwise the frontend can only show server-processing state while `/upload` is pending.
- Revisit the broader visual design regression caused by the attempted UI refresh.

---

### 17. ColBERT SIMD path not exposed as a named, benchmarkable path

Status: `open`

Symptoms:
- CPU fallback for ColBERT MaxSim uses a naive triple loop (`O(Q·D·dim)`), which is extremely slow.
- GPU implementation suffers from padding waste (all docs padded to max length) and suboptimal parallelism (1 thread per doc).
- Query token count is hardcoded to 10.
- The existing ndarray `dot()` path is already SIMD-accelerated via BLAS but is not extracted as a standalone named function that can be benchmarked or compared against the GPU path.

Impact:
- ColBERT late interaction is effectively unusable on CPU for any real document set.
- No way to verify GPU results against a known-correct CPU reference.
- No benchmark harness to compare GPU naive vs GPU tiled vs CPU SIMD.

Follow-up work:
- Extract CPU ndarray-based ColBERT logic into a named `colbert_maxsim_simd()` function.
- Add `BackendSelector::Simd` variant to the existing CPU/GPU switch.
- Add criterion benchmark (`benches/colbert.rs`) with sparse/medium/dense/mixed scenarios.
- Compare GPU naive vs GPU tiled vs CPU SIMD for correctness and throughput.

---

### 18. External backends use synthetic stubs instead of real calls

Status: `open`

Symptoms:
- `get_embedding()` always returns `Array1::from_elem(768, 0.1)` regardless of ML server configuration.
- Arrow Flight service (`flight.rs`) has all 10 `FlightService` methods returning `Status::unimplemented` — zero working implementation.
- Without real embeddings or Arrow Flight, horizontal scaling (tasks 4.3.1–4.3.4) is impossible.
- Observability (tasks 4.4.1–4.4.4) is not started: code uses `println!` instead of structured tracing, no `/metrics` or `/health` endpoints expose real data, circuit breakers exist but are not instrumented.

Impact:
- Embedding always returns a constant dummy vector regardless of input text — any ML-server-dependent flow gives meaningless results.
- Arrow Flight distributed data exchange cannot be tested or used.
- No way to monitor system health or performance in production-like conditions.

Follow-up work:
- Replace synthetic embedding with real HTTP/gRPC call to configured ML server (OpenAI-compatible `/v1/embeddings` REST API).
- Implement Arrow Flight `do_put` / `do_get` / `do_exchange` with `RecordBatch` registry.
- Replace `println!` with `tracing` structured JSON logs.
- Implement `/health`, `/ready`, and `/metrics` HTTP endpoints.
- Expose circuit breaker states as Prometheus gauges.

---

### 19. Parser/reader engine replacement discussion is deferred

Status: `deferred`

Notes:
- Replacing Mistral OCR with another parser/reader engine is not the current priority.
- Current evidence shows OCR is not the main bottleneck.
- Revisit only after ingestion and retrieval quality are stabilized.

---

## Proposed iterative order

### Phase 1
- Continue ingestion performance work:
  - batch vector writes
  - reduce serial waits
  - improve token-accurate document metadata in UI

### Phase 2
- Continue retrieval correctness work:
  - validate exact match behavior
  - tune fusion / rerank weights
  - `deferred` reduce remaining context flooding where needed — revisit after knowledge graph integration

### Phase 3
- Restore document structure:
  - ordering
  - headings
  - TOC
  - structured viewer rendering

### Phase 4
- Clean up entities and graph foundation:
  - replace ad hoc hot-path stemming with library-backed normalization
  - add background entity enrichment / NER / canonicalization
  - persist canonical entities, aliases, mentions, confidence, and enrichment status
  - recalculate graph edges from enriched entities before investing heavily in graph UX

### Phase 5
- Polish chat/agent UX and visual design

---

## Notes for future passes

- Do not treat the current graph UI as product-complete until the underlying graph data is meaningful.
- Do not treat raw hot-path entity candidates as final semantic entities; they are allowed to be useful immediately, but cold enrichment should own final cleanup/canonicalization.
- Do not invest in superficial UI polish before retrieval and document structure are trustworthy.
- Do not reintroduce legacy Mongo-era response artifacts into the new PostgreSQL-backed system.
- A future knowledge graph layer has been captured as a concept in `docs/knowledge_graph_concept.md`; it requires web research and architecture review before implementation.
