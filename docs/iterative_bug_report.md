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
6. Local model/world knowledge can mask retrieval failures during manual QA; answers must be checked against retrieved passages, not only against apparent correctness.
7. Configuration drift can silently put the server into mock mode or lower-throughput defaults even when Docker dependencies are running correctly.
8. UI phase indicators need explicit backend progress events; frontend-only guesses mix ingestion steps and mislead validation.
9. The current knowledge graph visualization is not readable at real document scale; the graph view can contain hundreds of nodes/edges but still fail to communicate structure.
10. Frontend rendering and animation performance remain a product-level issue; large derived views must not be fully computed or rendered on the main UI thread.
11. Persisted theme preferences are applied too late during page load, causing a visible light/dark flash when the saved theme differs from the default.

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

### Later correction for Alice validation

- `confirmed` A later query against `Alice_in_Wonderland_Zakhoder.docx` exposed a false positive validation risk.
  - Query: `какую песню поёт герцогиня?`
  - The assistant produced an answer, but the answer was likely supplied from model knowledge rather than retrieved RAG evidence.
  - The retrieval layer did not reliably surface the relevant passage.
- Lesson:
  - Manual validation must inspect retrieved sources/passages, not only the final answer.
  - Grounding prompts must forbid filling gaps from outside knowledge for local-document questions.
  - Retrieval fixes must be general; document-specific lexical hardcode is not acceptable.

---

## Validation dataset

- Local regression / manual validation documents are stored in `F:\projects\APL-RAG\doc_for_tests`.
- The folder contains a small document pack covering multiple scenarios:
  - small documents
  - large documents
  - OCR-heavy documents
  - mixed formatting cases
- Use this folder for future ingestion, retrieval, viewer, chapter/heading search, and UI regression checks before treating related fixes as verified.

### War and Peace volume 1 retrieval regression set

Document:
- `F:\projects\APL-RAG\doc_for_tests\War_and_Peace_Tom_1_LibRu.docx`

Purpose:
- Validate that retrieval finds the supporting passages, not merely that the final LLM answer is factually correct from pretrained knowledge.
- For each question, inspect retrieved sources/passages before accepting the answer.

Questions:

1. Fact + detail: exact context lookup
   - `С чего начинается роман? В чьем петербургском салоне происходит действие первых глав, в каком месяце и году? Какую главную политическую тему обсуждают гости?`
   - Expected retrieval behavior: surface the opening Anna Pavlovna Scherer salon passage, including July 1805 and the Napoleon / Genoa / Lucca political context.

2. Plot + intrigue: causal chain
   - `Из-за какого предмета развернулась скрытая борьба между князем Василием Курагиным и Анной Михайловной Друбецкой у постели умирающего графа Кирилла Владимировича Безухова? Чем закончилась эта борьба?`
   - Expected retrieval behavior: retrieve the deathbed/inheritance scene around the portfolio or papers/will, not only generic Bezukhov-family context.

3. Local episode + characterization
   - `Как проявил себя капитан Тушин в Шенграбенском сражении? Почему после боя его отчитали, и кто вступился за него перед Багратионом?`
   - Expected retrieval behavior: retrieve the Shengraben/Tushin battery episode and the after-action scene with Bagration and Prince Andrei.

4. Character relationships: multi-person aggregation
   - `Почему расстроилось сватовство Анатоля Курагина к княжне Марье Болконской в Лысых Горах? Какую роль в этом сыграла мадемуазель Бурьен?`
   - Expected retrieval behavior: aggregate the Anatole / Princess Mary / Mademoiselle Bourienne episode at Bald Hills without drifting to unrelated Kuragin scenes.

5. Philosophy + key moment
   - `Что увидел и понял князь Андрей Болконский, когда лежал тяжело раненый на Праценской горе во время Аустерлицкого сражения? Как изменилось его отношение к Наполеону в этот момент?`
   - Expected retrieval behavior: retrieve the Pratzen/Austerlitz sky passage and the subsequent shift in Andrei's perception of Napoleon.

### War and Peace validation pass after grounding/search fixes

Status: `validated`

Reviewer setup:
- The test questions were generated by Gemini 3.1 Pro.
- Gemini 3.1 Pro also acted as an external reviewer.
- The reviewer received both final assistant answers and the retrieved RAG passages, not only the answers.

Observed improvements:
- The earlier "model memory beats retrieval" failure was reduced significantly.
  - Initially, the assistant could answer famous-literature questions from pretrained knowledge even when retrieval was weak.
  - After stricter grounding instructions and mandatory agent preflight search, answers were much more consistently tied to retrieved passages.
- The agent became more willing to perform iterative tool use.
  - In later tests, it called search multiple times to gather missing context instead of stopping after one weak retrieval pass.
- Retrieval quality was strong on dense narrative scenes with names/actions:
  - The struggle between Prince Vasily Kuragin and Anna Mikhailovna Drubetskaya over the mosaic portfolio / inheritance papers near Count Kirill Bezukhov's deathbed was retrieved accurately.
  - Captain Tushin's Shengraben episode and the after-action scene with Bagration / Prince Andrei were retrieved accurately.
- Answer synthesis quality was strong once the right passages were retrieved:
  - Answers were structured, concise, and factually aligned with the supplied passages.
  - No factual distortion was observed in the reviewed successful cases.

Remaining risk:
- Positional/meta questions remain a known weak point for vector retrieval.
  - Examples: "С чего начинается роман?", "первая страница", "первые главы", date/location facts near the very beginning of a document.
  - Dense retrieval alone is not reliable for these; positional/structured retrieval and opening-chunk candidate injection are required.
- Continue validating that the final answer is supported by visible retrieved sources, especially for famous texts where LLM pretrained knowledge can mask RAG failures.

Current conclusion:
- The retrieval + agent architecture is now behaving like a usable grounded RAG pipeline for this literary QA test set.
- The system is not "done", but the critical failure mode where the agent confidently answers from memory without reliable local evidence has been materially reduced.

---

## Already fixed

### Ingestion / backend

- `fixed` Added request timeouts and phase logging for:
  - Mistral file upload
  - Mistral OCR
  - embedding server requests

- `fixed` Added retry and better diagnostics for Mistral OCR network calls.
  - `/v1/files` upload and `/v1/ocr` now retry transient send failures.
  - Errors include timeout/connect/request classification and source chain details.
  - Added `MISTRAL_BASE_URL`, `MISTRAL_UPLOAD_TIMEOUT_SECS`, `MISTRAL_OCR_TIMEOUT_SECS`, `MISTRAL_RETRY_ATTEMPTS`, and `MISTRAL_RETRY_DELAY_MS`.

- `fixed` Added race protection when switching documents in the frontend viewer.

- `fixed` Added cleanup of `anchors` / `edges` and anchor embeddings when deleting or clearing documents.

- `fixed` Switched main chunk embedding during ingestion from single-item requests to batched requests.

- `fixed` Restored visible batching and GPU saturation settings after switching embedding models.
  - `EMBEDDING_BATCH_SIZE` is now present in env examples and local config.
  - Ollama compose config now exposes `OLLAMA_NUM_PARALLEL`, `OLLAMA_MAX_LOADED_MODELS`, `OLLAMA_MAX_QUEUE`, `OLLAMA_KEEP_ALIVE`, and `OLLAMA_FLASH_ATTENTION`.
  - Current direction is one loaded embedding model with parallel contexts, not multiple duplicate model copies on the same GPU.

- `fixed` Added defensive limits for entity extraction by chunk size / entity count to avoid runaway CPU work.

- `fixed` Batched anchor embeddings instead of embedding anchors one by one at the tail of ingestion.

- `fixed` Batched Qdrant upserts for chunk and anchor embeddings during ingestion, and made Qdrant upsert waiting configurable via `QDRANT_UPSERT_WAIT`.

- `fixed` Replaced the `/documents` raw chunk listing with grouped document metadata so UI chunk counts are no longer truncated by the old `LIMIT 1000` path.

- `fixed` Prevented the graph integration test from using the normal application `DATABASE_URL`; it previously deleted real documents and left `doc-a`...`doc-d` mock rows in the live library.

- `fixed` Made `.env` loading failures visible and less dependent on current working directory.
  - The server now searches for `.env` in the current directory and parent directories.
  - Startup logs now report whether an environment file was loaded or why it failed.
  - `DATABASE_URL` logging is redacted.
  - PostgreSQL initialization retries were added for dependency startup races.
  - This fixed the observed banner state where both PostgreSQL and Qdrant appeared as `MOCK MODE` even though `.env` contained the URLs.
  - Operational note: current `docker-compose.yml` starts dependencies only; it does not rebuild or restart the Rust application binary.

- `fixed` Improved mock-mode API behavior.
  - `/documents` now returns an empty document list plus a warning when PostgreSQL is not connected instead of failing with a generic 500.
  - reset/reindex/graph endpoints now return structured JSON errors and `503` for missing PostgreSQL.

- `fixed` Removed legacy retrieval response artifacts:
  - synthetic summary injection
  - internal `=== ДОКУМЕНТЫ В ПОИСКЕ ===` TOC block
  - user-facing `Proximity Gap` markers

- `fixed` Replaced document-specific lexical hardcode with a configurable query expansion path.
  - The initial song/lullaby expansion was rejected as hidden document/domain hardcode.
  - Query expansion is now disabled by default and can be enabled with explicit dictionary files via `QUERY_EXPANSION_MODE=dictionary`.
  - Supported dictionary formats include JSON objects, JSON synonym groups, and text synonym groups.

- `fixed` Switched default embeddings to `jeffh/intfloat-multilingual-e5-large-instruct:q8_0`.
  - Default embedding dimension changed to 1024.
  - Query embeddings use an instruction template via `EMBEDDING_QUERY_PREFIX`.
  - Passage prefix defaults to empty for this Ollama model.
  - Qdrant collection recreation and reindexing are required after this change.

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
- Ollama runtime tuning is explicit in compose (`OLLAMA_NUM_PARALLEL`, queue size, keep-alive, flash attention), and application batch size is explicit in env config.

Follow-up work:
- Validate ingestion throughput against a large real document with Qdrant enabled.
- Introduce pipelining between:
  - chunk preparation
  - embedding requests
  - storage writes
- Profile whether `EMBEDDING_BATCH_SIZE=64` and `OLLAMA_NUM_PARALLEL=4` are optimal for the current GPU/VRAM; do not assume these are universal defaults.

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
- RSF is now the default rerank strategy, with BM25 weighted above dense similarity for exact textual matches.
- Search response shaping now limits ordinary QA context volume by narrowing fallback candidates, avoiding broad macro expansion unless explicitly requested, capping per-document chunks, and reducing the default MCP search response budget.
- The assistant prompt now explicitly forbids answering local-document questions from outside knowledge when retrieved passages are insufficient.
- The embedding backend now uses an instruct multilingual E5 model and an explicit query instruction template, replacing the older non-instruct/default configuration.

Remaining hypotheses:
- Qdrant/dense retrieval is not suitable as the only mechanism for precise chapter, heading, line, or paragraph lookup; PostgreSQL-backed lexical/structured search should be expanded for exact navigation and extraction tasks.
- Short queries (< 5 words) suffer from weak dense and BM25 signals because fewer terms = less matching surface area. The fixed RSF weights (dense 0.30, bm25 0.35) were tuned for medium/long queries and underperform on short factual questions.

Follow-up work:
- `fixed` Adaptive RSF weights based on query length.
  - Short queries (<= 5 words) now get boosted dense weight (+0.15) and reduced BM25 weight (-0.10) so semantic similarity can compensate for weak lexical signal.
  - Long queries (>= 6 words) keep the default balanced weights.
  - Implemented via `adaptive_config()` in `rsf.rs` and `compute_rsf_with_query()`.
- `fixed` Lowered entity search threshold from `>= 2` to `>= 1` entities.
  - Single-entity queries like "как звали мать Арсения?" now trigger entity co-occurrence search.
  - Previously blocked because only one entity ("Арсения") was extracted.
- `fixed` Increased keyword fallback limit for short queries.
  - Queries with <= 4 words now get 2x the base fallback limit (up to 60 candidates).
  - Compensates for weaker BM25 ranking on short queries by casting a wider net.
- `fixed` Added RSF channel debug logging in VM.
  - Logs per-candidate channel max scores, weights, and top-3 fusion results.
  - Enables diagnosis of retrieval quality issues per query.
- Remaining: validate these fixes against the War and Peace regression set and "как звали мать Арсения?" test case.
- `fixed` Re-check exact-match retrieval path — root cause identified: default `RerankStrategy::SemanticOnly` skipped BM25/exact-match scoring entirely.
  - `fixed` Changed default strategy from `SemanticOnly` to `Hybrid { semantic_weight: 0.7, bm25_weight: 0.3 }` in `ir.rs` and `dsl.rs`.
  - `fixed` Increased exact-match boost in `fetch_bm25_scores()` from 5.0 to 10.0.
- `fixed` Validate exact-match behavior against known failing examples.
  - Document: `Утро_ч.1_в.1.docx` (~190k tokens, single mention of `Лиза`).
  - Query 1: `расскажи что упоминалось про Лизу` → retrieved correct chunk, identified Лиза as Arseniy's mother.
  - Query 2: `как звали мать Арсения?` → correct exact answer with citation, fresh context.
- `fixed` Revisit RSF / BM25 weighting and exact-match boosts.
  - `fixed` Rebalanced RSF default weights in `rsf.rs`: dense 0.50→0.30, bm25 0.15→0.35, structural 0.05→0.10, so exact textual matches no longer dominated by dense embeddings after per-channel minmax normalization.
  - `fixed` Made RSF the default rerank strategy for JSON and DSL search requests, replacing the simpler Hybrid default.
  - `fixed` Reduced context flooding by narrowing PostgreSQL fallback candidate limits, disabling macro-span expansion for ordinary QA queries, capping per-document chunks in search responses, and lowering the default MCP search response budget from 10k to 5k tokens.
  - `fixed` Verified with `cargo test`.
- `in_progress` Add chapter/heading search that can locate a chapter or heading and retrieve its full text span.
  - `fixed` Added structured candidate lookup for `chapter_id` and `header_chain` so navigation queries are not dense-only.
  - `fixed` Search responses now expand explicit chapter/heading hits into their macro-level span before response assembly, instead of returning only the individual matched chunks.
  - Remaining: validate full chapter/section reconstruction against large real OCR markdown documents and tune span size if macro chunks are too broad or too narrow.
- Add precise lookup for a specific line/string and paragraph within headings/sections.
  - `fixed` Added quoted/line/phrase target extraction for PostgreSQL exact matching, so commands like `find line "..."` or `найди строку «...»` search the target text instead of the whole user instruction.
  - `fixed` Search responses for precise line/phrase lookup now return the matching paragraph or a small local excerpt instead of the full chunk.
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

Status: `in_progress`

Symptoms:
- The graph often shows only the active document node.
- No meaningful structure is visible.
- The 2D canvas gives weak navigation value.
- The viewport is too small.
- The `Граф связей` tab visualization is still not resolved and should not be treated as product-ready.
- On real data the graph can become a dense 2D hairball: node labels overlap, edges cover the canvas, and clusters cannot be understood visually.
- Hundreds of section/subsection/entity nodes are drawn at once without useful level-of-detail, clustering, filtering, or progressive disclosure.
- The current rendering does not communicate semantic layers such as document -> section -> subsection -> entity -> relation.
- The current graph rendering path has accumulated too many partial repairs around layout, buttons, controls, and canvas behavior; continuing to patch the same method may be more expensive than replacing it.
- Fullscreen graph stats can report that the graph data exists (for example hundreds of sections/subsections/entities and more than a thousand links), while the visible canvas/WebGL scene shows mostly entity-colored nodes around the document node.
- Section/subsection nodes and hierarchy links may be counted but not visibly rendered, or may be outside the camera/frustum, hidden by fog/depth/material opacity, overwhelmed by entity nodes, or filtered out after graph data construction.
- This creates a misleading state where the top stats say relationships were built, but the actual visualization looks incomplete.

Impact:
- The feature exists visually but does not help analysis.
- The user cannot reliably answer basic questions from the visualization: what clusters exist, which entities are central, which parts of the document are connected, and which relationships are important.
- The graph also contributes to frontend performance problems because layout/rendering work is pushed into the browser for a large, dense dataset.

Decision:
- Treat the current 2D graph as a temporary debugging view, not as the final knowledge graph UX.
- Prefer a clean graph visualization refactor/rewrite if the current renderer cannot be simplified safely. The goal is to redesign the graph surface around clear data contracts and interaction requirements, not to preserve the existing method structure.
- Investigate a 3D graph view for real-scale exploration, with camera controls, depth separation, semantic clustering, and level-of-detail.
- Re-evaluate the rendering engine instead of assuming the current canvas approach is mandatory. Candidate directions may include Three.js/WebGL, a specialized graph engine, or a hybrid where 2D remains only for small/debug graphs.
- Move expensive graph preparation out of the frontend where practical: backend or BFF should produce pre-filtered, clustered, positioned, and paginated graph payloads rather than forcing the browser to derive everything from raw nodes/edges.
- The frontend should remain responsible for interactive presentation, selection, camera/navigation state, and small incremental updates, not heavyweight graph analytics or full-layout recomputation.

Follow-up work:
- `fixed` Graph auto-linking now groups shared entities by hot-path normalized keys instead of raw labels, so inflected RU/EN forms can create `shares_entities` edges.
- `fixed` Added zoom/pan with mouse wheel and background drag.
- `fixed` Added zoom control buttons (+, -, fit-to-screen).
- `fixed` Added graph legend (active document, related document, shared entities).
- `fixed` Added graph statistics bar (node/edge counts).
- `fixed` Added fullscreen modal graph view.
- `fixed` Added semantic zoom for node labels (labels scale and truncate based on zoom level).
- `fixed` Added glow effect for active/hovered nodes.
- `fixed` Node labels now dynamically truncate based on zoom level.
- `fixed` Added empty-graph overlay with auto-link prompt when no edges exist.
- `fixed` Added "Авто-линковка" button in graph controls and empty overlay, calling `POST /documents/auto-link?threshold=3`.
- `in_progress` Added a separate Three.js/WebGL 3D graph renderer prototype with fallback to the existing 2D canvas renderer.
- `in_progress` Vendored Three.js locally under `static/vendor/three/` so the 3D graph no longer depends on CDN availability.
- `fixed` Added backend static routing for `/vendor/*path`, so the locally vendored Three.js module is served by the Rust UI server instead of returning 404.
- `fixed` Prevented the old 2D canvas context from blocking WebGL creation by replacing graph canvas elements before starting the 3D renderer.
- `in_progress` Changed the graph payload used by the UI toward an Obsidian/NotebookLM-style overview: document node, macro sections, a capped set of medium sections, top entities, capped entity edges, and no micro nodes in the default overview.
- `fixed` Fullscreen graph now stops the inline preview renderer before starting the modal renderer, avoiding doubled rendering load.
- `in_progress` Added basic 3D graph interactions: wheel zoom, scene rotation, node selection, chunk inspector, related-document navigation, entity search, stats, and legend.
- Verify graph data population against real documents in the running app.
- Add auto-link trigger from UI (call `POST /documents/auto-link`).
- Audit the existing graph implementation and decide whether to keep any reusable pieces or start a new visualization module from a minimal interface.
- Define the graph view contract first: input payload shape, selected/focused node behavior, filters, clustering, controls, fullscreen behavior, and performance budget.
- Reconsider layout and navigation model around semantic layers, clustering, filters, and "focus neighborhood" exploration.
- Prototype a 3D knowledge graph view and compare it against the current 2D canvas on a large real document.
- Add renderer diagnostics that compare graph data counts by type with actual rendered meshes/objects by type.
- Verify whether section/subsection nodes and hierarchy edges are outside the camera view, too faint, too small, occluded, fogged out, or filtered before mesh creation.
- Add a reliable "fit all visible graph" routine based on the actual rendered bounding box.
- Ensure the visible graph matches the stats bar: if stats report sections, subsections, entities, and links, each type must be visibly represented or explicitly hidden by a user-controlled filter.
- Compare graph engine options before committing to the rewrite, including:
  - Three.js/WebGL for custom 3D rendering;
  - a mature graph visualization library with force simulation and level-of-detail support;
  - keeping a simplified 2D debug renderer only for small graphs and tests.
- Add backend/BFF graph endpoints that can return:
  - graph summary statistics;
  - clusters/communities;
  - focus neighborhoods around a selected node;
  - paginated node/edge slices;
  - optional precomputed coordinates/layout hints;
  - level-of-detail payloads for zoomed-out and zoomed-in states.
- Avoid rendering every node, edge, and label at once; add label culling, edge bundling/filtering, and progressive reveal.
- Define performance budgets for graph load, pan/zoom, node selection, and animation smoothness.

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
- Animations are not consistently smooth; layout and rendering work visibly competes with user interaction.
- Heavy document-derived views can make otherwise simple UI actions feel delayed.

Likely causes:
- Large DOM trees rendered eagerly.
- Full rerendering of heavy panels on every tab switch.
- No virtualization / pagination for large entity lists and large hierarchical views.
- Expensive repeated parsing/rendering during chat updates.
- Some views likely rebuild from raw data too often instead of reusing cached derived structures.
- Graph layout/rendering, markdown/document rendering, hierarchy building, entity clouds, and chat/source panels are not yet separated into explicit performance tiers.
- Too much derived presentation data is computed in the browser instead of being pre-aggregated, paginated, or cached by the backend/BFF.

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
- Before starting broad optimization, run an instrumented Selenium + Chrome DevTools Protocol profiling pass on real MVP scenarios and use the results to choose optimization targets.
- Capture objective frontend metrics via CDP rather than relying only on visual impressions:
  - long tasks and main-thread blocking time;
  - scripting, rendering, layout, paint, and compositing time;
  - frame rate / dropped frames during graph pan/zoom and tab switching;
  - DOM node counts and style/layout invalidations;
  - JS heap growth and detached nodes after repeated tab switches;
  - network payload sizes and API response timings for document details, graph data, chat sources, and settings;
  - first paint / first contentful paint / theme bootstrap behavior.
- Profile at least these scenarios:
  - initial load with saved dark theme;
  - selecting a large document;
  - switching `Текст` -> `Иерархия` -> `Сущности` -> `Граф связей`;
  - opening fullscreen graph and interacting with pan/zoom;
  - streaming an agent/RAG response while sources are produced;
  - opening and editing AI settings, including secret field reveal.
- Use the Selenium/CDP report to decide what must move to backend/BFF computation, for example graph clustering/layout hints, hierarchy slices, entity buckets, source summaries, or document view-model aggregation.
- Add view-level profiling for tab switches and chat rendering.
- Introduce virtualization or pagination for large lists and clouds.
- Cache derived structures for hierarchy/entities/graph inputs.
- Avoid full rerender of inactive tabs.
- Consider incremental rendering for large panels.
- Use memoization for stable derived data and event handlers in the remaining frontend code paths.
- Add request/response contracts that let the frontend ask for slices of data rather than full document-scale payloads when only a small viewport is visible.
- Move heavy rendering preparation to backend/BFF where it has clear product value:
  - graph clustering/layout hints;
  - hierarchy slices;
  - entity buckets;
  - search/source summaries;
  - precomputed counts and facets.
- Keep browser work bounded with pagination, virtualization, memoization, throttled animation frames, and cancellation of stale renders.
- Define measurable budgets for tab switch latency, chat streaming rerender cadence, graph frame time, and large document viewer responsiveness.

Notes:
- A BFF/backend view-model layer is now a likely part of the solution for graph and document-scale views, but should be introduced around stable contracts instead of becoming an unstructured duplicate frontend.
- First priority is to eliminate obvious expensive rerenders and heavy client-side rebuilding, then move expensive derivations behind explicit backend APIs.

---

### 10b. Saved theme is applied too late and causes a visible flash on load

Status: `in_progress`

Symptoms:
- The default UI theme is light.
- If the user previously selected the dark theme, the page can still render briefly in the light theme during load before JavaScript applies the saved preference.
- The resulting flash is visually jarring and makes the app feel unstable.

Likely causes:
- Theme preference is read from `localStorage` only after the main frontend bundle starts.
- The initial HTML/CSS does not receive the resolved `data-theme` before first paint.
- There is no early inline theme bootstrap or server-rendered theme attribute.

Impact:
- Users who choose dark mode see a repeated theme-switch glitch on page load.
- This is especially noticeable because the app has large full-screen panels with strong background colors.

Follow-up work:
- `fixed` Add a small early theme bootstrap script in `index.html` before stylesheet-dependent paint, reading `tensor-rag-ui-theme` and setting `document.documentElement.dataset.theme`.
- `fixed` Keep the bootstrap script minimal and dependency-free so it runs before the app bundle.
- `fixed` Ensure `system` mode resolves against `prefers-color-scheme` before first paint.
- `fixed` Add `color-scheme` CSS metadata so native controls and scrollbars match the resolved theme.
- Verify page load visually for:
  - saved `dark`;
  - saved `light`;
  - saved `system` on dark OS theme;
  - saved `system` on light OS theme.

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

### 15c. Agent loses conversation context in directed topical dialog

Status: `open`

Symptoms:
- When a user maintains a conversation about a specific document, story, or topic without explicitly re-stating the subject in every message, the agent loses the thread.
- The logical connection between messages is clear to a human reader, but the agent begins searching in unrelated documents or irrelevant parts of the document set.
- The retrieval layer does not receive the implied context from prior messages, so it fetches passages unrelated to the ongoing topic.
- The agent does not maintain an implicit "active document" or "active topic" state across turns.

Impact:
- Directed multi-turn conversations degrade rapidly.
- The user must repeat the document/topic name in every message, which defeats the purpose of a conversational interface.
- Retrieval quality and user trust both suffer because the assistant appears to "forget" what is being discussed.

**Required: Before designing a fix, investigation is required to determine the root cause.**
Specifically:
1. Analyse whether the agent prompt/scoring passes conversation history to the retrieval query builder, or whether each turn builds a retrieval query from only the latest user message.
2. Determine whether the reranker and search scope drift because conversation context is summarised or discarded between turns.
3. Identify whether the agent chooses the search document scope solely from explicit document names in the latest message, or whether prior-message topic signals are used.
4. Determine if a conversation-level "active scope" (document set, topic cluster) should be maintained as explicit state.

Follow-up work (contingent on investigation results):
- Pass compressed conversation context into the retrieval query for each turn.
- Maintain a lightweight "active document" or "active topic" signal across turns.
- Scope search to the active document(s) when the conversation is clearly about a specific source, even if the user does not re-state the name.
- Validate with a multi-turn dialog where the document/story is named only in the first message.

Notes:
- Do not implement any fix for this item before the investigation phase is complete and documented.
- This item may overlap with item 2 (retrieval quality) and item 15a (RAG source overlap); the investigation should clarify whether the root cause is in retrieval scoping, agent context handling, or both.

### 15d. Agent can answer document questions without guaranteed RAG evidence

Status: `in_progress`

Symptoms:
- On famous literature such as `War_and_Peace_Tom_1_LibRu.docx`, the agent sometimes answers from pretrained knowledge instead of first grounding itself in retrieved passages.
- Some questions have to be repeated before the agent uses the search tool.
- The UI may show tool activity, but the final answer can still have no visible retrieved sources.
- It is unclear during manual QA whether:
  - the agent did not call `search_text`;
  - the agent called `search_text` and got empty results;
  - the agent called another tool such as full document reading;
  - sources were produced internally but not attached to the final UI message.

Impact:
- Manual validation becomes ambiguous.
- Correct answers can still be RAG failures.
- The assistant may ask for clarification or answer from memory when selected-document retrieval should be mandatory.

Completed in this area:
- `fixed` Agent instructions were tightened: local-document questions must not be answered from pretrained knowledge.
- `fixed` Agent mode now performs a mandatory preflight `search_text` when retrieval/tools are enabled, injects the raw search result into the model context, and streams sources to the UI before optional additional tool rounds.

Follow-up work:
- Add explicit UI diagnostics for empty search results vs no tool call vs unsupported/failed tool call.
- Consider a "strict grounded mode" toggle where document questions fail closed unless at least one source is available.
- Validate with the War and Peace regression questions and inspect whether sources appear for every local-document answer.

### 15b. AI provider network failures are not retried and overflow the notice modal

Status: `fixed`

Symptoms:
- Intermittent internet/provider failures produced a hard chat error such as:
  - `Failed to call AI provider: error sending request for url (...)`
- Long provider URLs overflowed the notice modal and visually escaped the card bounds.

Impact:
- Bad connections caused avoidable chat failures.
- Error display looked broken exactly when the user needed clear diagnostics.

Resolution:
- Added retry for AI provider chat-completion requests on network/send errors, `429`, and `5xx` responses.
- Added configurable `AI_REQUEST_TIMEOUT_SECS`, `AI_RETRY_ATTEMPTS`, and `AI_RETRY_DELAY_MS`.
- Error diagnostics now include timeout/connect/request classification and source-chain details.
- Notice modal text now wraps long URLs and scrolls inside the dialog instead of overflowing.

Remaining caveat:
- If streaming has already begun and then fails mid-answer, automatic retry is not yet enabled because replaying the request could duplicate partial output. This should be handled separately if mid-stream failures become common.

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
- `fixed` Added backend-driven upload progress events with SSE.
  - The frontend no longer marks OCR, chunking, embedding, and DB indexing as active immediately after request start.
  - The upload banner now receives explicit `active`, `completed`, and `failed` events from the backend pipeline.
  - The UI waits briefly for the SSE channel before starting `/upload` to avoid losing initial progress events.
- Restore a clearer animated upload/indexing progress indicator.
- Ensure the status bar transitions through upload, OCR, tokenization/chunking, embedding, and DB indexing without requiring a page refresh.
  - `fixed` The main phase-transition mechanism now uses backend SSE progress events.
  - Remaining: visually validate the banner on long OCR and long embedding runs, especially failure cases and reconnect behavior.
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

Status: `in_progress`

Symptoms:
- Arrow Flight service (`flight.rs`) has all 10 `FlightService` methods returning `Status::unimplemented` — zero working implementation.
- Without Arrow Flight, horizontal scaling (tasks 4.3.1–4.3.4) is impossible.
- Observability (tasks 4.4.1–4.4.4) is not started: code uses `println!` instead of structured tracing, no `/metrics` or `/health` endpoints expose real data, circuit breakers exist but are not instrumented.

Impact:
- Arrow Flight distributed data exchange cannot be tested or used.
- No way to monitor system health or performance in production-like conditions.

Follow-up work:
- `fixed` Replaced synthetic embedding with real OpenAI-compatible `/v1/embeddings` HTTP calls when an embedding server is configured.
- `fixed` Added batch embedding requests for ingestion and anchor embeddings.
- `fixed` Updated default embedding config to `jeffh/intfloat-multilingual-e5-large-instruct:q8_0` with 1024 dimensions.
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

## 2026-06-08 UI/maintenance closure pass

Status: `implemented_pending_manual_verification`

Closed in this pass:
- Upload/indexing UX now exposes the current backend stage as readable text in addition to the stepper, ETA, and cancel state.
- Chat now preserves visible tool-call activity and warns when a RAG/search tool was used but no answer sources are displayed.
- Source cards can send their excerpt to the document viewer search, making it possible to verify whether the cited fragment is present in the opened document.
- Document viewer now has inline search with match count, next/previous navigation, clear action, and active-match highlighting.
- Graph renderer now scales the 3D layout to the visible viewport, renders links with stronger visibility, and reports how many nodes/links were actually drawable.
- Settings now include a manual library optimization workflow with backend SSE progress and cancellation. The job checks indexed documents, fetches existing Qdrant vectors, recomputes per-document semantic layout, updates Postgres layout fields, and runs `ANALYZE documents`.

Manual verification required before marking as fully closed:
- Upload 2-3 large documents after clearing/reindexing the DB and confirm the status indicator reaches upload, OCR, chunking, embedding, DB indexing, and semantic clustering without hanging.
- Start upload cancellation during embedding/indexing and confirm partial vectors/rows for that document are removed.
- Ask the War and Peace regression questions and confirm the chat shows tool activity, visible sources where RAG returns them, and a no-sources warning when the agent/tool path yields no displayed citations.
- Click "Find in viewer" on at least three source cards and confirm the viewer finds/highlights the fragment in the selected document.
- Open graph tab and fullscreen for a large document and confirm the stats line shows nonzero visible nodes/links when counters claim links exist.
- Run Settings -> Optimize library, observe progress, cancel once, then run to completion; confirm documents checked/reclustered/rows updated summary is plausible and no server restart is required.

Verification commands already run:
- `node --check tensor-rag/static/app/library.js`
- `node --check tensor-rag/static/app/chat.js`
- `node --check tensor-rag/static/app/settings.js`
- `node --check tensor-rag/static/app/graph3d.js`
- `cargo check` from `tensor-rag/`

### Phase 1
- Continue ingestion performance work:
  - batch vector writes
  - reduce serial waits
  - improve token-accurate document metadata in UI

### Phase 2
- Continue retrieval correctness work:
  - validate exact match behavior
  - tune fusion / rerank weights
  - `fixed` reduce remaining context flooding in the default QA path

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
- Rework graph visualization as a real analysis surface:
  - allow a clean rewrite of the graph visualization module instead of preserving the current patched canvas method
  - evaluate graph rendering engines and keep only the parts of the current implementation that clearly reduce risk
  - prototype 3D graph navigation for large document-scale graphs
  - add clustering, focus neighborhoods, filtering, and level-of-detail
  - move graph preparation/layout hints and paginated graph payloads to backend/BFF APIs
  - keep the frontend bounded to interaction, camera state, and incremental rendering

### Phase 5
- Fix product-level frontend performance and visual stability:
  - run Selenium + CDP profiling first and create a metrics report before choosing optimization work
  - remove the saved-theme load flash before broader visual polish
  - add profiling budgets for tabs, chat streaming, graph interaction, and large document views
  - apply pagination, virtualization, memoization, throttled animation frames, and stale-render cancellation
  - move expensive document/graph/entity view-model preparation behind explicit backend/BFF contracts
- Polish chat/agent UX and visual design after performance budgets are met

---

## Notes for future passes

- Do not treat the current graph UI as product-complete until the underlying graph data is meaningful.
- Do not treat raw hot-path entity candidates as final semantic entities; they are allowed to be useful immediately, but cold enrichment should own final cleanup/canonicalization.
- Do not spend excessive time preserving the current graph renderer if a clean module rewrite gives a simpler, faster, and more capable visualization path.
- Do not treat 3D graph rendering as a pure frontend skin; real graph UX needs backend/BFF support for clustering, focus neighborhoods, paging, and level-of-detail.
- Do not begin broad UI optimization without Selenium + CDP metrics from real MVP scenarios; use the report to decide what stays in the browser and what moves to backend/BFF computation.
- Do not allow heavy document-scale frontend panels to render or recompute unbounded data on the main UI thread.
- Do not ship theme selection with a first-paint flash; the saved theme must be applied before the app visibly renders.
- Do not invest in superficial UI polish before retrieval and document structure are trustworthy.
- Do not reintroduce legacy Mongo-era response artifacts into the new PostgreSQL-backed system.
- A future knowledge graph layer has been captured as a concept in `docs/knowledge_graph_concept.md`; it requires web research and architecture review before implementation.
