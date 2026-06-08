# Knowledge Graph Visual Design Notes

Date: 2026-06-07
Last updated: 2026-06-08

This document tracks the intended visual direction for the knowledge graph UI. It is separate from the iterative bug report because these notes describe target design and interaction principles, not only defects.

## Status Convention

Throughout this document, the following markers track implementation progress:

| Marker | Meaning |
|--------|---------|
| `[x]` | Done |
| `[-]` | Partial / in progress |
| `[ ]` | Planned / not started |

These markers appear at section headings and individual checklist items. They reflect the state of the codebase, not ambition.

## [x] Goal (unchanging)

Make the graph feel like a readable knowledge map, not a dense hierarchy preview. The user should be able to understand document structure, entity relationships, and local neighborhoods without fighting visual noise or animation freezes.

## [x] References And Direction

- Obsidian graph view: sparse constellation feel, focus around selected nodes, readable local neighborhoods.
- NotebookLM style: knowledge exploration rather than raw data dump, clean highlights, low-friction navigation.
- Neo4j Bloom / graph investigation tools: selected node as a working center, direct neighbors emphasized, distant context de-emphasized.
- Scientific console aesthetic: restrained, dark-friendly, aligned with the current app themes.
- The old `Private--test--RAG3/calibre_converter/static/graph-visualizer.js` implementation: useful interaction model with force layout, semantic zoom, click-to-center, and a side inspector.
- The old `visualization_service.go` / RAG3 `VisualizationService`: useful backend contract for producing a graph-specific `nodes/links` payload instead of forcing the frontend to derive graph data from full document details.
- The user's RAG3 screenshots: target inspiration for an immersive 3D "document universe" with mouse-driven flight through graph space.

The target style is a "knowledge space": clusters, local focus, soft depth, and calm accents.

The product should not choose between 2D and 3D permanently. The intended direction is a dual-mode graph:

- 2D map mode for precise MVP work: fast overview, readable neighborhoods, stable labels, search-oriented inspection.
- 3D flight mode for immersive exploration: a navigable document universe where the user can fly through clusters and feel spatial structure.

## [x] Visual Problems To Avoid

- Concentric ring or "donut" layouts that look decorative but do not explain relationships.
- Too many visible labels at once.
- Bright saturated colors that fight the existing dark/light themes.
- Thick edge bundles that turn into a gray web.
- Duplicate rendering in inline and fullscreen modes.
- Floating controls that look detached from the graph surface.

## [-] Layout Model

### Implemented baseline

- `[x]` The graph uses a local Three.js renderer loaded from `/vendor/three/three.module.js`.
- `[x]` Inline and fullscreen renderers are isolated so fullscreen no longer keeps the inline WebGL scene rendering behind it.
- `[x]` Overview layout uses radial branches grouped by semantic cluster when cluster ids are available, otherwise by macro/parent structure (no longer a pure concentric ring/donut).
- `[x]` A static collision-relaxation pass separates node spheres after layout to reduce overlap without a per-frame physics simulation cost.
- `[x]` Fullscreen uses expanded graph caps; inline mode keeps smaller caps.

### Overview layout rules (current)

- `[x]` Document node near the center.
- `[x]` Macro sections as primary clusters around the document.
- `[-]` Medium and micro chunks near their parent section — implemented, but layout is still crowded on dense documents.
- `[-]` Entity nodes near the chunks they connect to — implemented, but placement is manual (entity adjacency, not semantic).
- `[ ]` Related documents on the outer context layer — not yet implemented; only the active document is rendered.

### Focus mode (click-to-center)

- `[x]` Clicking a node rebuilds the graph around it.
- `[x]` The selected node becomes the local center.
- `[-]` Direct neighbors and edges are emphasized — partially: halos and active edges exist, but no smooth fly-to animation.
- `[x]` Parents, children, siblings, shared entities shown with strict caps.
- `[x]` Distant context hidden or subdued.
- `[ ]` Clicking selected node again or root returns to overview — not implemented; requires a dedicated "back to overview" action.

### Known gaps

- `[-]` Focus mode is still a filtered hierarchy/entity neighborhood, not a truly semantic graph dependency view.
- `[-]` Layout needs more "air" for dense documents and better cluster separation after real UMAP coordinates.
- `[-]` Graph caps help, but huge documents still need progressive loading or renderer-level virtualization.

## [-] 3D Style

### Design principles

- Use matte spheres or low-gloss points instead of shiny balls.
- Keep node geometry small and readable.
- Add subtle depth fog so far edges fade.
- Reduce edge opacity by default.
- Highlight only the selected node, direct neighbors, and active links.
- Show labels for the selected node, important section nodes, and hovered nodes only.
- Avoid permanent labels for dense medium/micro/entity sets.

### Implementation status

- `[x]` Matte spheres with `roughness: 0.78`, zero `metalness`.
- `[x]` Fog enabled (`FogExp2`, color matches dark theme background).
- `[x]` Low-opacity default edges (0.16–0.26), higher for active edges (0.6).
- `[x]` Reduced permanent labels; most shown only on hover or selection.
- `[x]` Selected-node halo (18% opacity, 1.85× radius).
- `[x]` Click in fullscreen opens non-blocking side inspector with chunk text.
- `[x]` Hover tooltip with node label and type.
- `[-]` Active-neighbor highlighting — halos exist for selected node, but neighbor edges are only subtly brighter.
- `[-]` Depth ordering / cluster emphasis — needs improvement.
- `[ ]` Smooth fly-to animation on node selection.
- `[ ]` Edge/label fading by camera distance.

### Target 3D flight mode

Long-term vision, not yet implemented:

- Fullscreen-first experience, not a small embedded preview.
- Mouse-driven navigation should feel like flying through a document universe, not just rotating an object on a table.
- Controls should support orbit, pan, zoom, and eventually first-person/free-flight navigation.
- Spatial graph should have depth: clusters as constellations, entity bridges as filaments, important hubs as softly glowing anchors.
- Edge density must be aggressively controlled by distance, focus, opacity, and progressive reveal.
- Labels should appear through semantic zoom, hover, selection, and cluster focus, never as a permanent blanket over the scene.
- A selected node should become a navigation anchor: center/fly to it, open inspector, and highlight local neighborhood.
- This mode is allowed to be expressive, but it must remain useful: every visual effect should support orientation, selection, or comprehension.

### Risks (to solve before flight mode becomes default)

- Large graphs can overwhelm WebGL if every edge and label is rendered at once.
- Free-flight navigation needs stronger orientation aids: minimap, breadcrumbs, reset, and selected-node trail.
- Physics/layout should be prepared outside the render loop where possible.
- The current manual layout is not good enough for the target; it needs real semantic coordinates plus force/collision relaxation.

## [-] Color Palette

The palette MUST be theme-aware. Node colors are defined as CSS custom properties in `style.css` and consumed by the renderer. Hardcoded hex values in JS files (`graph3d.js`) are a known divergence to be fixed.

### Approach

- Each node type gets a semantic CSS variable (e.g. `--graph-node-doc`, `--graph-node-macro`).
- Variables are set per theme (light/dark) in `style.css` alongside `--color-primary`, `--color-success`, etc.
- The Three.js renderer reads these via `getComputedStyle` or receives theme-resolved colors through the graph data pipeline.
- Edge opacity and halo intensity also use CSS variables so theme switching does not require a re-render.

### Theme variable candidates (not yet in style.css)

| Token | Dark theme | Light theme | Role |
|-------|-----------|-------------|------|
| `--graph-node-doc` | muted mint `#68b8b1` (reuse `--color-primary`) | `--color-primary` | Document root node |
| `--graph-node-macro` | soft blue `#5b8def` | desaturated blue | Section header |
| `--graph-node-medium` | cyan teal `#27b7c6` | muted teal | Subsection chunk |
| `--graph-node-micro` | slate `#7c8a93` | gray-blue | Fine chunk |
| `--graph-node-entity` | warm amber `#d99a2b` | softened amber | Named entity |
| `--graph-node-related` | low-saturation purple | muted purple | Related document |
| `--graph-node-active` | pale aqua / off-white `#d9fff7` | deep aqua | Selected node |
| `--graph-edge-default` | `rgba(190, 210, 205, 0.16)` | `rgba(60, 80, 75, 0.2)` | Non-active edges |
| `--graph-edge-active` | `rgba(104, 184, 177, 0.55)` | `rgba(47, 119, 117, 0.5)` | Active/neighbor edges |
| `--graph-edge-hierarchy` | soft purple `rgba(180, 140, 220, 0.25)` | muted purple `rgba(100, 70, 140, 0.3)` | Contains edges |

### Current state in code

- `[ ]` CSS variables for graph node colors do not exist yet.
- `[ ]` The renderer does not read theme variables — it uses hardcoded hex values in `graph3d.js:3-10` (pink/purple/cyan tones) that do not match the intended palette.
- `[x]` The app already has robust light/dark theming via `style.css` `:root` / `[data-theme="dark"]` blocks — graph colors should follow the same pattern.
- `[ ]` Theme switching does not trigger a graph re-color.
- `[-]` Edge colors are partially controlled (default/hierarchy/active), but still hardcoded in JS.

### Migration plan

1. Add `--graph-node-*` and `--graph-edge-*` to `style.css` for both themes.
2. Create a small JS helper (`getGraphThemeColors()`) that reads these from `getComputedStyle(document.documentElement)`.
3. Update `graph3d.js` to call this helper on init and on theme change.
4. Remove hardcoded `NODE_COLORS` and `EDGE_COLORS` objects from `graph3d.js`.
5. Verify both themes render correctly without a full re-render.

## [-] Controls

Graph controls should become a compact toolbar rather than detached buttons.

- `[ ]` One consistent toolbar for inline and fullscreen modes.
- `[x]` Right-bottom placement with safe spacing from the graph border.
- `[x]` Stable square icon buttons, around 32-36 px.
- `[x]` Same visual language as the app: subtle panel background, restrained border, clear hover state.
- `[x]` First-pass controls: zoom in, zoom out, fit/reset view, fullscreen, refresh.
- `[ ]` Later controls: 2D/3D switch, labels toggle, edge filter, layout mode.
- `[-]` Inline and modal zoom controls both exist (`#graph-zoom-controls` and `#graph-modal-zoom-controls`) with consistent styling.

### Current state

- `[x]` Zoom controls are styled as stable square buttons with backdrop blur.
- `[x]` Controls use `color-mix` for theme-aware backgrounds (`var(--bg-card)` with transparency).
- `[-]` Controls are still separate from a unified graph toolbar model.
- `[ ]` 2D/3D switch, label toggle, edge filters, and layout mode remain unimplemented.
- `[ ]` Controls are hardcoded in HTML — should be dynamically built by a `GraphToolbar` module.

## [-] 2D And 3D Combined Architecture

The long-term architecture is a shared graph model with interchangeable renderers. This section distinguishes between the **target design** (described below) and the **current implementation** (marked with status).

### Target design (long-term)

- Graphology as the canonical graph data/model layer.
- Sigma for fast 2D view (large graphs, overview/focus work).
- Three.js for 3D view (same filtered graph data).
- Both renderers consume the same normalized `GraphViewModel`: nodes, edges, focus state, selection, color tokens, layout hints.
- Backend or worker-side preprocessing prepares graph neighborhoods.

### Current implementation

- `[-]` Graph data is built on the frontend in `library.js::buildDocumentGraphData()` — no shared `GraphViewModel` abstraction exists.
- `[-]` `graph3d.js` consumes graph data directly, mixing layout and rendering concerns.
- `[x]` Three.js is vendored locally (`/vendor/three/three.module.js`), no CDN dependency.
- `[ ]` No Graphology or Sigma dependency; 2D fallback is a simple canvas simulation in `library.js::createGraphSimulation()`.
- `[ ]` No `GraphViewModel`, `Graph2DRenderer`, `GraphInspector`, or `GraphToolbar` modules exist as reusable abstractions.
- `[ ]` The 2D canvas fallback has been preserved but is not actively maintained as a product surface.

### Renderer strategy (target)

| Module | Status | Notes |
|--------|--------|-------|
| `GraphViewModel` | `[ ]` | Normalized payload for all renderers |
| `Graph2DRenderer` | `[ ]` | Sigma/Graphology-based MVP mode |
| `Graph3DRenderer` | `[-]` | Exists as `graph3d.js` but tightly coupled to data pipeline |
| `GraphInspector` | `[-]` | Side panel exists inside graph3d.js, not extracted as shared module |
| `GraphToolbar` | `[ ]` | Controls still in HTML, not dynamically built |

### Lessons from the old RAG3 visualizer (reference)

- `[-]` Force-directed layout — not yet; current layout is radial-branch + collision relaxation.
- `[ ]` Per-type physics weights — not implemented.
- `[-]` Semantic zoom — partial: labels show on hover/selection, no distance-based reveal.
- `[-]` Click-to-center — implemented but without smooth fly-to animation.
- `[-]` Persistent side inspector — exists inside fullscreen, not in inline mode.
- `[-]` `ResizeObserver` behavior — graph canvas uses wrapper resize, but fullscreen/sidebar layout shifts can distort.
- `[x]` Stats and legend — lightweight, overlaid, non-interactive.

### Backend: current vs target

| Aspect | Current | Target |
|--------|---------|--------|
| Graph endpoint | `GET /documents/:doc_uuid/graph?depth=N` returns raw neighborhood | Dedicated `/documents/:doc_uuid/graph-view` with mode, focus, caps params |
| Graph data source | Frontend derives nodes/links from document details + edges | Backend builds typed nodes/links payload |
| Semantic data | `GET /documents/:uuid/details?semantic=true` (lazy, overloaded) | Separate endpoint or field in graph-view |
| Payload structure | `serde_json::Value` in `graph.rs` | Typed Rust structs (`GraphView`, `GraphNode`, `GraphEdge`) |
| Chunk level mapping | `root`, `doc`, `macro`, `medium`, `micro`, `entity` | Same, plus `related_document`, `cluster` |
| Entity aggregation | Frontend scans chunks (`collectGraphEntities`) | Backend aggregates entity counts |
| Heatmap / stats | Not supported | Backend includes hit counts when available |
| Tests | `graph_test.rs` (237 lines, integration) | Add unit tests for node/link construction |

### Rust adaptation tasks

- `[-]` `graph.rs` exists but returns `serde_json::Value`, not typed structs.
- `[-]` `GET /documents/:doc_uuid/graph` exists but is a raw neighborhood dump, not a graph-view payload.
- `[ ]` Need `mode=overview|focus`, `focus=<node_id>`, `renderer=2d|3d`, `semantic=true|false`, cap params.
- `[x]` PostgreSQL chunks/edges + Qdrant semantic data (no Mongo/Weaviate).
- `[x]` Chunk levels mapped: `root`, `macro`, `medium`, `micro`, `entity`.
- `[ ]` Node payload fields: `id`, `label`, `type`, `val/weight`, `content`, `count`, `hits`, graph metadata.
- `[ ]` `/documents/:uuid/details` should stay focused on text/details; graph belongs elsewhere.

## [x] Semantic Clustering Layer

UMAP + HDBSCAN should be treated as a backend/data-layer feature, not only as a visual layout trick.

### Current state

- `[x]` Clustering traits (`DimensionReducer`, `Clusterer`) and `HdbscanClusterer` exist in `clustering.rs`.
- `[x]` `PcaFallbackReducer` exists as a simple dimension reduction fallback (truncates to `components`).
- `[x]` Search VM uses `HdbscanClusterer` for RSF cluster diversity channel, with fallback for insufficient/invalid vectors.
- `[x]` Document details expose `cluster_id`, `cluster_distance`, `layout_hint` for chunks when `?semantic=true`.
- `[x]` Semantic cluster data is computed during document upload/indexing and stored on document chunks; `/documents/:uuid/details?semantic=true` now reads persisted cluster fields instead of recomputing Qdrant + UMAP/HDBSCAN on graph open.
- `[x]` Layout projection uses UMAP-based 3D coordinates via `UmapReducer` (umap-rs crate) — replaces the old normalized-prefix projection when sufficient data is available.
- `[x]` UMAP implemented via `umap-rs` crate in `clustering.rs::UmapReducer` with brute-force KNN, power-iteration PCA init, and graceful fallback to PcaFallbackReducer.
- `[-]` `SemanticClusterLayout`, `SemanticClusterPoint`, `SemanticClusterSummary` structs exist and are serializable.

### Target direction

- Use embeddings to build semantic neighborhoods for chunks, anchors, entities, and optionally document-level summaries.
- Reduce high-dimensional vectors into a layout-friendly manifold with UMAP or an equivalent reducer.
- Cluster the reduced vectors with HDBSCAN or a compatible density-based method.
- Store cluster ids, outlier/noise labels, representative chunks, centroid/medoid references, confidence metadata.
- Reuse same clusters for graph visualization, search diversity, query expansion, document/topic navigation.

### Expected wins

- Graph layout gets real semantic structure instead of artificial rings or hand-made clusters.
- Search can diversify results by topic cluster instead of returning many near-duplicate chunks.
- UI can show cluster-level summaries before expanding into individual nodes.
- Backend can precompute cluster payloads and keep frontend focused on rendering and interaction.

### Implementation options

| Path | Approach | Status |
|------|----------|--------|
| Pure Rust | Keep clustering inside `tensor-rag` | `[x]` HDBSCAN + UMAP (`umap-rs` crate) implemented |
| Python/service | Run UMAP/HDBSCAN in a local analysis worker | `[ ]` Feasibility not reviewed |
| Hybrid | Rust HDBSCAN + deferred UMAP | `[-]` Current approach; working but limited |

### UMAP-specific remaining work

- `[x]` Choose implementation path: Rust crate (`umap-rs` v0.4.5, brute-force KNN for document-scale datasets).
- `[x]` Define deterministic reducer parameters: `n_components=3`, `n_neighbors=15`, `min_dist=0.1`, `seed=42` — configurable via `UmapReducer` struct fields.
- `[x]` Decide scope: detailed UMAP + HDBSCAN runs per document during ingest; graph/details reads persisted per-chunk layout metadata.
- `[-]` Cache invalidation for reindex, delete, replacement, embedding model change, chunking schema change — upload/index writes fresh layout; full reindex refresh still needs semantic layout rebuild.
- `[ ]` Store reducer metadata alongside cluster metadata so old layouts can be recognized and rebuilt safely.
- `[x]` UMAP coordinates used as primary layout signal in `build_semantic_cluster_layout`; collision relaxation still applied in renderer. Very large documents use a bounded coarse projection fallback instead of blocking graph/details requests.

### Open design questions

- `[?]` Should cluster ids be persisted per document version, per library, or per query result set?
- `[?]` Should graph layout use global library clusters or local per-document/per-focus clusters?
- `[?]` How often should clusters be recomputed after ingestion, delete, reindex, or entity enrichment?
- `[?]` What metrics decide whether a cluster is good enough to influence search ranking?

## Implementation Plan

| # | Task | Status | Notes |
|---|------|--------|-------|
| 1 | Fix first-pass visual regressions: toolbar centering, spacing, labels, palette direction, isolated fullscreen render, click inspector | `[x]` | Applied in early graph3d commits |
| 2 | Define and expose semantic clustering payload (`cluster_id`, `cluster_distance`, `layout_hint`, `semantic_clusters`) | `[x]` | `SemanticClusterLayout`, `SemanticClusterPoint` structs exist |
| 3 | HDBSCAN-backed clustering in Rust with fallback | `[x]` | `HdbscanClusterer` + `PcaFallbackReducer` in `clustering.rs` |
| 4 | Lazy-load graph semantic details so normal doc viewing is not slowed | `[x]` | Via `?semantic=true` query param |
| 5 | Formalize `GraphViewModel` for shared 2D/3D consumption | `[ ]` | Graph data built ad-hoc in `library.js` |
| 6 | Extract shared `GraphInspector` and `GraphToolbar` from renderer code | `[ ]` | Inspector embedded in `graph3d.js` |
| 7 | Implement MVP 2D map mode (force layout, semantic zoom, click-to-center, side inspector) | `[ ]` | 2D fallback is simple canvas, not product-grade |
| 8 | Keep 3D available but relabel as experimental flight mode | `[-]` | 3D is default; no mode switch exists |
| 9 | Add profiling with Selenium/CDP before moving computation to backend | `[ ]` | Not started |
| 10 | Prototype UMAP or equivalent manifold reduction on stored embeddings | `[x]` | `UmapReducer` implemented via `umap-rs` crate with brute-force KNN and PCA init |
| 11 | Persist cluster/reducer metadata, expose through dedicated graph/BFF endpoint | `[-]` | Reducer runs per-request; no persistence yet |
| 12 | Replace frontend layout hints with backend-prepared UMAP/cluster coordinates | `[x]` | `build_semantic_cluster_layout` uses `UmapReducer` for 3D layout hints; projection field reports `umap-3d` |
| 13 | Move heavy graph filtering/layout to backend or worker after metrics | `[ ]` | Not started |
| 14 | Add manual library optimization / defragmentation task | `[ ]` | Settings action with progress modal, cancel support, consistency checks, and reclustering/backfill for existing documents |

## Phased Renderer Plan

### Phase 1: stabilize the graph product surface `[-]`

| Item | Status | Notes |
|------|--------|-------|
| Keep current graph tab + fullscreen modal, introduce renderer mode state | `[-]` | Tab and modal exist, no `2d-map`/`3d-flight` toggle |
| Make 2D map the default MVP mode once implemented | `[ ]` | 2D fallback exists but is not MVP-grade |
| Keep 3D flight as experimental mode | `[ ]` | No mode switch at all |
| Move inspector, toolbar, stats, legend, selection state out of renderer code | `[-]` | Stats/legend partially extracted, inspector and toolbar are not |

### Phase 2: build the 2D map mode `[ ]`

| Item | Status | Notes |
|------|--------|-------|
| Local graph rendering dependency (no CDN) | `[ ]` | Not evaluated |
| Force-directed layout with per-type charge and link distance | `[ ]` | Not implemented |
| Semantic zoom labels (root/macro always, others on zoom/hover/select) | `[-]` | Partial in 3D; not in 2D |
| Click-to-center and click-to-open-inspector | `[-]` | Exists in 3D only |
| Caps and focus neighborhoods | `[x]` | Implemented in `library.js::buildDocumentGraphData` |
| App palette (not old RAG3 saturated colors) | `[-]` | Documented target; graph3d.js still uses hardcoded pink/purple |

### Phase 3: rebuild the 3D flight mode around the same model `[ ]`

| Item | Status | Notes |
|------|--------|-------|
| Backend/worker layout coordinates + local collision relaxation | `[-]` | Collision relaxation exists; coordinates are frontend-only |
| Fly-to-node, orbit/free-flight toggle, reset, orientation helpers | `[-]` | Orbit via drag exists; no fly-to, no free-flight, no reset button |
| Distance-based label and edge fading | `[ ]` | Not implemented |
| Progressive edge/label rendering by camera distance | `[ ]` | Not implemented |
| "Document universe" feel | `[-]` | Partial: clusters as radial groups, but no constellations/filaments |

### Phase 4: backend graph payload and semantic layout `[-]`

- `[ ]` Dedicated graph endpoint for overview/focus payloads.
- `[-]` Compute caps, neighborhoods, cluster summaries, layout hints server-side — partial: UMAP layout hints computed in `build_semantic_cluster_layout`, but no dedicated graph endpoint.
- `[x]` Add UMAP or equivalent reducer — `UmapReducer` in `clustering.rs` using `umap-rs` crate.
- `[ ]` Persist reducer/cluster metadata with invalidation.
- `[ ]` Typed Rust structs for graph payload (not `serde_json::Value`).

### Phase 5: measurement and acceptance `[ ]`

- `[ ]` Profile first render, fullscreen open, node click, zoom/rotate/fly, inspector open, large-doc caps.
- `[ ]` Capture metrics before/after 2D mode and before/after 3D flight rebuild.
- `[ ]` Use screenshots + interaction traces as acceptance artifacts.

### Phase 6: library optimization / defragmentation `[ ]`

Manual maintenance action in settings. This should not run implicitly during search or graph opening.

- `[ ]` Add a settings action named "Optimize library" / "Оптимизация библиотеки".
- `[ ]` Show a dedicated modal with stage indicator, progress bar, current document name, elapsed time, and cancel button.
- `[ ]` Implement cancellable backend job state with SSE/progress events, similar to upload progress.
- `[ ]` Check Postgres/Qdrant consistency: missing Qdrant points, orphan Qdrant points, document rows without embeddings, and documents without persisted semantic layout.
- `[ ]` Recompute per-document semantic layout for existing indexed documents using stored Qdrant vectors and save `cluster_id`, `cluster_distance`, `layout_hint`, `clustering_algorithm`, and `clustering_projection` into Postgres.
- `[ ]` Keep detailed clustering scoped per document (`UMAP + HDBSCAN`) so large libraries do not merge unrelated documents into one shared cluster space.
- `[ ]` Later: build a separate global document-level UMAP projection from document centroids/summaries only, without HDBSCAN over all chunks.
- `[ ]` Add optional Postgres maintenance (`ANALYZE`, and possibly guarded `VACUUM`) after data repair/reclustering.
- `[ ]` Report a final summary: documents checked, documents reclustered, rows updated, missing/orphan vectors found, cancelled/completed status.

## MVP Acceptance Criteria

- `[-]` Default graph overview is readable without fullscreen — partially: works for small documents, dense docs still crowded.
- `[-]` Focus mode shows why the selected node matters — partially: highlights neighbors, but no semantic explanation.
- `[x]` Fullscreen does not render duplicate inline graph behind it.
- `[-]` Graph stays smooth for large documents after caps — mostly, but no progressive loading for very large docs.
- `[ ]` Colors feel native to both app themes — currently hardcoded pink/purple in JS, not theme-aware.
- `[-]` Controls look like part of the product UI — styling is consistent, but no unified toolbar.

## Remaining MVP Work

| # | Task | Priority | Status |
|---|------|----------|--------|
| 1 | Run Selenium/CDP profiling on current 3D graph interactions | high | `[ ]` |
| 2 | Verify graph behavior after full server restart + reindex | high | `[ ]` |
| 3 | Implement UMAP or equivalent reducer | high | `[x]` |
| 4 | Add dedicated graph payload endpoint (overview, focus, clusters, caps) | high | `[ ]` |
| 5 | Decide renderer split: 2D map as MVP default, 3D as experimental flight | medium | `[ ]` |
| 6 | Add progressive loading / virtualized graph levels for large docs | medium | `[ ]` |
| 7 | Improve fullscreen side inspector: entity detail, neighbor list, jump-to-text | medium | `[-]` |
| 8 | Clean up old 2D fallback code path and duplicate inspector/legend logic | low | `[ ]` |
| 9 | **Migrate graph colors from hardcoded JS hex to CSS theme variables** | high | `[ ]` |
| 10 | **Extract shared `GraphInspector` and `GraphToolbar` from `graph3d.js`** | medium | `[ ]` |
| 11 | **Add manual library optimization / defragmentation workflow** | high | `[ ]` |
