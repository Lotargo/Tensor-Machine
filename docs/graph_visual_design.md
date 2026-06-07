# Knowledge Graph Visual Design Notes

Date: 2026-06-07

This document tracks the intended visual direction for the knowledge graph UI. It is separate from the iterative bug report because these notes describe target design and interaction principles, not only defects.

Last updated: 2026-06-08, after reviewing the old RAG3 graph visualizer reference.

## Goal

Make the graph feel like a readable knowledge map, not a dense hierarchy preview. The user should be able to understand document structure, entity relationships, and local neighborhoods without fighting visual noise or animation freezes.

## References And Direction

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

## Visual Problems To Avoid

- Concentric ring or "donut" layouts that look decorative but do not explain relationships.
- Too many visible labels at once.
- Bright saturated colors that fight the existing dark/light themes.
- Thick edge bundles that turn into a gray web.
- Duplicate rendering in inline and fullscreen modes.
- Floating controls that look detached from the graph surface.

## Layout Model

Current implemented baseline:

- The graph now uses a local Three.js renderer loaded from `/vendor/three/three.module.js`.
- Inline and fullscreen renderers are isolated so fullscreen no longer keeps the inline WebGL scene rendering behind it.
- Overview layout is no longer a pure concentric ring/donut layout; it uses radial branches grouped by semantic cluster when cluster ids are available, otherwise by macro/parent structure.
- A static collision-relaxation pass separates node spheres after layout to reduce overlap without adding a per-frame physics simulation cost.
- Fullscreen uses expanded graph caps, while inline mode keeps smaller caps to protect the main UI.

Default overview:

- Document node near the center.
- Macro sections arranged as primary clusters around the document.
- Medium and micro chunks placed near their parent section, not in separate global rings.
- Entity nodes placed near the chunks they connect to.
- Related documents placed on the outer context layer.

Focus mode:

- Clicking a node rebuilds the graph around that node.
- The selected node becomes the local center.
- Direct neighbors and connecting edges are emphasized.
- Parents, children, siblings, and shared entities are shown with strict limits.
- Distant context remains hidden or visually subdued.
- Clicking the selected node again, or the root document, returns to overview.

Current gap:

- Focus mode exists, but it is still mostly a filtered hierarchy/entity neighborhood, not a truly semantic graph dependency view.
- The layout still needs more "air" for dense documents and better cluster separation after real UMAP coordinates are available.
- Some segment visibility is controlled by graph caps; fullscreen caps were increased, but huge documents still need progressive loading or renderer-level virtualization.

## 3D Style

- Use matte spheres or low-gloss points instead of shiny balls.
- Keep node geometry small and readable.
- Add subtle depth fog so far edges fade.
- Reduce edge opacity by default.
- Highlight only the selected node, direct neighbors, and active links.
- Show labels for the selected node, important section nodes, and hovered nodes only.
- Avoid permanent labels for dense medium/micro/entity sets.

Recent implementation notes:

- Current renderer uses matte spheres, reduced permanent labels, fog, low-opacity edges, and selected-node halos.
- Click selection in fullscreen opens a non-blocking side inspector with selected chunk text.
- Hover still uses a compact tooltip.
- Next visual pass should improve depth ordering, active-neighbor highlighting, and selected cluster emphasis.

Target 3D flight mode:

- Fullscreen-first experience, not a small embedded preview.
- Mouse-driven navigation should feel like flying through a document universe, not just rotating an object on a table.
- Controls should support orbit, pan, zoom, and eventually first-person/free-flight navigation.
- Spatial graph should have depth: clusters as constellations, entity bridges as filaments, important hubs as softly glowing anchors.
- Edge density must be aggressively controlled by distance, focus, opacity, and progressive reveal.
- Labels should appear through semantic zoom, hover, selection, and cluster focus, never as a permanent blanket over the scene.
- A selected node should become a navigation anchor: center/fly to it, open inspector, and highlight local neighborhood.
- This mode is allowed to be expressive, but it must remain useful: every visual effect should support orientation, selection, or comprehension.

3D risks to solve before it becomes default:

- Large graphs can overwhelm WebGL if every edge and label is rendered at once.
- Free-flight navigation needs stronger orientation aids: minimap, breadcrumbs, reset, and selected-node trail.
- Physics/layout should be prepared outside the render loop where possible.
- The current manual layout is not good enough for the target; it needs real semantic coordinates plus force/collision relaxation.

## Color Palette

The palette should be theme-aware and calmer than the current saturated graph colors.

- Background: deep green graphite, close to the app surface.
- Document: muted mint `#68b8b1`.
- Macro section: soft blue `#5b8def`.
- Medium subsection: cyan teal `#27b7c6`.
- Micro chunk: slate `#7c8a93`.
- Entity: warm amber `#d99a2b`.
- Selected node: pale aqua or off-white halo `#d9fff7`.
- Default edge: low-opacity neutral `rgba(190, 210, 205, 0.16)`.
- Active edge: mint accent `rgba(104, 184, 177, 0.55)`.

Light theme should use the same semantic colors with reduced saturation and darker edge contrast.

## Controls

Graph controls should become a compact toolbar rather than detached buttons.

- One consistent toolbar for inline and fullscreen modes.
- Right-bottom placement with safe spacing from the graph border.
- Stable square icon buttons, around 32-36 px.
- Same visual language as the app: subtle panel background, restrained border, clear hover state.
- Controls expected in the first pass: zoom in, zoom out, fit/reset view, fullscreen, refresh.
- Later controls: 2D/3D switch, labels toggle, edge filter, layout mode.

Current status:

- Zoom controls have been visually centered inside stable square buttons.
- Controls are still separate from a full graph toolbar model; the 2D/3D switch, label toggle, edge filters, and layout mode remain open.

## 2D And 3D Combined Architecture

The desired long-term architecture is a shared graph model with interchangeable renderers.

- Graphology can become the canonical graph data/model layer.
- Sigma can render the fast 2D view for large graphs and overview/focus work.
- Three.js can render the 3D view using the same filtered graph data.
- Both renderers should consume the same normalized graph view: nodes, edges, focus state, selection, color tokens, and layout hints.
- Backend or worker-side preprocessing should prepare graph neighborhoods before the frontend renders them.

This lets 2D stay practical for huge graphs while 3D becomes a richer exploration mode instead of the only way to inspect the graph.

Lessons from the old RAG3 visualizer:

- Use a force-directed layout model instead of hand-authored rings for the working graph.
- Use per-type physics: root/document has stronger repulsion and longer links, sections have medium repulsion, chunks/entities have shorter local distances.
- Use semantic zoom: labels are hidden by default and appear for important nodes or when zoomed in.
- Use click-to-center: selecting a node should smoothly center/zoom/fly to that node.
- Use a persistent side inspector instead of making the graph itself carry all text.
- Use `ResizeObserver` and explicit renderer resize behavior so fullscreen/sidebar changes do not distort the graph.
- Keep stats and legend lightweight, overlayed, and non-interactive.

Renderer strategy:

- `GraphViewModel`: one normalized payload for all renderers.
- `Graph2DRenderer`: MVP working mode, based on Graphology/Sigma or a local force-graph-compatible renderer.
- `Graph3DRenderer`: immersive mode, based on Three.js, consuming the same graph model and semantic layout hints.
- `GraphInspector`: shared side panel for selected node/chunk/entity details.
- `GraphToolbar`: shared controls for mode, labels, filters, reset, fullscreen, profiling/debug toggles.

Backend lessons from the old RAG3 `VisualizationService`:

- Use a dedicated visualization endpoint rather than overloading document details.
- Build graph payloads on the backend: root/document node, structure nodes, chunk nodes, entity nodes, and links.
- Include node payload fields directly needed by renderers and inspector: `id`, `label`, `type`, `val/weight`, `content`, `count`, `hits`, and graph-specific metadata.
- Include usage heatmap data when available; old RAG3 used chunk hit counts to size/color chunks.
- Aggregate entity counts server-side so renderers do not need to scan all chunks.
- Keep ownership/access checks in the graph endpoint, not only in document endpoints.
- Treat missing heatmap/stats as non-fatal; graph should still render.
- Add tests for node/link construction, especially entity links and structural links.

Adaptation for this Rust project:

- Create a Rust `graph_view` or `visualization` module that returns typed structs instead of `serde_json::Value` maps.
- Add a dedicated endpoint such as `/documents/:doc_uuid/graph-view`.
- Support query params: `mode=overview|focus`, `renderer=2d|3d`, `focus=<node_id>`, `semantic=true|false`, and caps.
- Use PostgreSQL chunks/edges plus Qdrant semantic layout data, not Mongo/Weaviate.
- Map our chunk levels to graph types: `root`, `macro`, `medium`, `micro`, `entity`, `related_document`, and later `cluster`.
- Return both `nodes/links` and renderer hints: physics weights, semantic coordinates, visibility caps, label priority, and inspector payload.
- Keep `/documents/:uuid/details` focused on document text/details; graph payload should become separate.

## Semantic Clustering Layer

UMAP + HDBSCAN should be treated as a backend/data-layer feature, not only as a visual layout trick.

Current state:

- The Rust codebase has clustering traits, fallback implementations, and an `HdbscanClusterer`.
- The search VM now uses `HdbscanClusterer` for the RSF cluster diversity channel, with fallback behavior for insufficient/invalid vectors.
- Document details can expose `cluster_id`, `cluster_distance`, and `layout_hint` for chunks when graph semantic details are requested.
- Semantic cluster data is now lazy-loaded for graph rendering through `/documents/:uuid/details?semantic=true`, so ordinary document opening does not pay the Qdrant + clustering cost.
- The current layout projection is still a normalized-prefix/simple projection used as a temporary layout hint.
- UMAP has not been implemented yet.

Target direction:

- Use embeddings to build semantic neighborhoods for chunks, anchors, entities, and optionally document-level summaries.
- Reduce high-dimensional vectors into a layout-friendly manifold with UMAP or an equivalent reducer.
- Cluster the reduced vectors with HDBSCAN or a compatible density-based method.
- Store cluster ids, outlier/noise labels, representative chunks, centroid/medoid references, and confidence metadata.
- Reuse the same semantic clusters for graph visualization, search diversity, query expansion, and document/topic navigation.

Expected wins:

- Graph layout gets real semantic structure instead of artificial rings or hand-made clusters.
- Search can diversify results by topic cluster instead of returning many near-duplicate chunks.
- The UI can show cluster-level summaries before expanding into individual nodes.
- Backend can precompute cluster payloads and keep the frontend focused on rendering and interaction.

Implementation options to evaluate:

- Pure Rust path: keep clustering inside `tensor-rag`, using Rust crates where mature enough.
- Python/service path: run UMAP/HDBSCAN in a local analysis worker if Rust libraries are not mature enough for production.
- Hybrid path: start with Rust DBSCAN/HDBSCAN-compatible clustering and add UMAP later once graph/search contracts are stable.

UMAP-specific remaining work:

- Choose implementation path: Rust crate, local Python worker, or a small local analysis service.
- Define deterministic reducer parameters for document-level graph layouts: neighbors, min distance, metric, random seed, and output dimensions.
- Decide whether UMAP runs per document, per focus neighborhood, per library, or as a cached multi-level embedding.
- Add cache invalidation for reindex, delete, document replacement, embedding model changes, and chunking schema changes.
- Store reducer metadata alongside cluster metadata so old layouts can be recognized and rebuilt safely.
- Use UMAP coordinates as the primary layout signal, then apply lightweight collision/label separation in the renderer.

Open design questions:

- Should cluster ids be persisted per document version, per library, or per query result set?
- Should graph layout use global library clusters or local per-document/per-focus clusters?
- How often should clusters be recomputed after ingestion, delete, reindex, or entity enrichment?
- What metrics should decide whether a cluster is good enough to influence search ranking?

## Implementation Plan

1. Done: fix first-pass visual regressions in the current graph surface: toolbar centering, spacing, labels, palette direction, isolated fullscreen render, and click inspector.
2. Done partially: define and expose a semantic clustering payload for graph/search use (`cluster_id`, `cluster_distance`, `layout_hint`, `semantic_clusters`).
3. Done partially: add HDBSCAN-backed clustering in Rust with fallback behavior.
4. Done partially: lazy-load graph semantic details so normal document viewing is not slowed by clustering.
5. Next: formalize `GraphViewModel` so both 2D map mode and 3D flight mode consume the same nodes, edges, types, clusters, selection, and caps.
6. Next: extract shared `GraphInspector` and `GraphToolbar` from renderer-specific code.
7. Next: implement MVP 2D map mode inspired by the old RAG3 visualizer: force layout, semantic zoom, click-to-center, stable side inspector, local library only.
8. Next: keep 3D available but relabel it as experimental flight/immersive mode until layout and performance are strong enough.
9. Next: add profiling with Selenium/CDP before moving more graph computation to backend.
10. Next: prototype UMAP or equivalent manifold reduction on stored embeddings.
11. Next: persist cluster/reducer metadata and expose it through a dedicated graph/BFF endpoint instead of overloading document details.
12. Next: replace temporary frontend layout hints with backend-prepared UMAP/cluster-aware layout coordinates.
13. Next: move heavy graph filtering/layout preparation to backend or worker after metrics show the true bottlenecks.

## Phased Renderer Plan

Phase 1: stabilize the graph product surface.

- Keep the current graph tab and fullscreen modal, but introduce renderer mode state: `2d-map` and `3d-flight`.
- Make 2D map the default MVP mode once implemented.
- Keep 3D flight accessible as an experimental mode, not removed.
- Move inspector, toolbar, stats, legend, and selected-node state out of renderer-specific code.

Phase 2: build the 2D map mode.

- Add a local graph rendering dependency or vendor bundle; no CDN.
- Use force-directed layout with per-type charge and link distance.
- Add semantic zoom labels: root/macro labels always allowed, medium/micro/entity labels only on zoom/hover/selection.
- Add click-to-center and click-to-open-inspector behavior.
- Support caps and focus neighborhoods so the graph does not dump every segment at once.
- Use the app palette, not the old RAG3 saturated colors.

Phase 3: rebuild the 3D flight mode around the same model.

- Replace manual ring/branch layout with backend/worker layout coordinates plus local collision relaxation.
- Add fly-to-node, orbit/free-flight toggle, reset, and orientation helpers.
- Add distance-based label and edge fading.
- Render edges/labels progressively based on camera distance and selected neighborhood.
- Keep the "document universe" feeling from the screenshots: clusters as constellations, entities as bridges, selected focus as an anchor.

Phase 4: backend graph payload and semantic layout.

- Add a dedicated graph endpoint for overview/focus payloads.
- Compute graph caps, neighborhoods, cluster summaries, and layout hints server-side or in a worker.
- Add UMAP or equivalent reducer; HDBSCAN alone is not enough for the target layout.
- Persist reducer/cluster metadata with invalidation on reindex, delete, embedding model changes, and chunking schema changes.
- Use the old RAG3 visualization service as a conceptual template, but implement typed Rust structs and our PostgreSQL/Qdrant-backed graph model.

Phase 5: measurement and acceptance.

- Profile first render, fullscreen open, node click, zoom/rotate/fly, side inspector open, and large-document caps.
- Capture metrics before/after 2D mode and before/after 3D flight rebuild.
- Use screenshots and interaction traces as acceptance artifacts, not only unit tests.

## MVP Acceptance Criteria

- Default graph overview is readable without fullscreen.
- Focus mode clearly shows why the selected node matters.
- Fullscreen mode does not render a duplicate inline graph behind it.
- Graph stays smooth enough for large documents after caps and focus filtering.
- Colors feel native to both app themes.
- Controls look like part of the product UI, not a debug overlay.

## Remaining MVP Work

- Run Selenium/CDP profiling on current 3D graph interactions: first render, fullscreen open, node click/focus, zoom, rotate, and large document caps.
- Verify graph behavior after a full server restart and document reindex, especially documents indexed before semantic cluster metadata existed.
- Implement UMAP or equivalent reducer; current HDBSCAN is useful, but layout coordinates are still temporary.
- Add a dedicated graph payload endpoint that can return overview, focus neighborhood, semantic clusters, and renderer caps without refetching full document details.
- Decide renderer split: 2D map as the MVP working default, Three.js 3D as immersive flight mode, both sharing the same normalized graph model.
- Add progressive loading/virtualized graph levels for very large documents so "all segments" can be explored without dumping every node at once.
- Improve fullscreen side inspector: entity detail mode, neighbor list, jump-to-text action, and close behavior that does not disrupt graph selection.
- Clean up the old 2D fallback code path and duplicate inspector/legend logic after the new renderer boundary is stable.
