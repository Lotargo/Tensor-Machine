# Knowledge Graph Visual Design Notes

Date: 2026-06-07

This document tracks the intended visual direction for the knowledge graph UI. It is separate from the iterative bug report because these notes describe target design and interaction principles, not only defects.

## Goal

Make the graph feel like a readable knowledge map, not a dense hierarchy preview. The user should be able to understand document structure, entity relationships, and local neighborhoods without fighting visual noise or animation freezes.

## References And Direction

- Obsidian graph view: sparse constellation feel, focus around selected nodes, readable local neighborhoods.
- NotebookLM style: knowledge exploration rather than raw data dump, clean highlights, low-friction navigation.
- Neo4j Bloom / graph investigation tools: selected node as a working center, direct neighbors emphasized, distant context de-emphasized.
- Scientific console aesthetic: restrained, dark-friendly, aligned with the current app themes.

The target style is a "knowledge space": clusters, local focus, soft depth, and calm accents.

## Visual Problems To Avoid

- Concentric ring or "donut" layouts that look decorative but do not explain relationships.
- Too many visible labels at once.
- Bright saturated colors that fight the existing dark/light themes.
- Thick edge bundles that turn into a gray web.
- Duplicate rendering in inline and fullscreen modes.
- Floating controls that look detached from the graph surface.

## Layout Model

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

## 3D Style

- Use matte spheres or low-gloss points instead of shiny balls.
- Keep node geometry small and readable.
- Add subtle depth fog so far edges fade.
- Reduce edge opacity by default.
- Highlight only the selected node, direct neighbors, and active links.
- Show labels for the selected node, important section nodes, and hovered nodes only.
- Avoid permanent labels for dense medium/micro/entity sets.

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

## 2D And 3D Combined Architecture

The desired long-term architecture is a shared graph model with interchangeable renderers.

- Graphology can become the canonical graph data/model layer.
- Sigma can render the fast 2D view for large graphs and overview/focus work.
- Three.js can render the 3D view using the same filtered graph data.
- Both renderers should consume the same normalized graph view: nodes, edges, focus state, selection, color tokens, and layout hints.
- Backend or worker-side preprocessing should prepare graph neighborhoods before the frontend renders them.

This lets 2D stay practical for huge graphs while 3D becomes a richer exploration mode instead of the only way to inspect the graph.

## Semantic Clustering Layer

UMAP + HDBSCAN should be treated as a backend/data-layer feature, not only as a visual layout trick.

Current state:

- The Rust codebase has clustering traits and fallback implementations.
- `PcaFallbackReducer` is only a simple dimensionality fallback.
- `RadiusClusterer` is only a lightweight similarity-threshold clusterer.
- The current search VM can use cluster diversity as an RSF channel, but it does not yet use full manifold reduction or density clustering.

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

Open design questions:

- Should cluster ids be persisted per document version, per library, or per query result set?
- Should graph layout use global library clusters or local per-document/per-focus clusters?
- How often should clusters be recomputed after ingestion, delete, reindex, or entity enrichment?
- What metrics should decide whether a cluster is good enough to influence search ranking?

## Implementation Plan

1. Fix immediate visual regressions in the current graph surface: toolbar centering, spacing, labels, and palette.
2. Add profiling with Selenium/CDP before moving more graph computation to backend.
3. Define a semantic clustering contract for graph/search payloads.
4. Prototype UMAP/HDBSCAN or an equivalent density-clustering path on stored embeddings.
5. Persist cluster metadata and expose it through backend/BFF graph endpoints.
6. Replace artificial frontend layout hints with cluster-aware layout hints from backend.
7. Add explicit 2D/3D renderer boundary before introducing Graphology/Sigma.
8. Move heavy graph filtering/layout preparation to backend or worker after metrics show the true bottlenecks.

## MVP Acceptance Criteria

- Default graph overview is readable without fullscreen.
- Focus mode clearly shows why the selected node matters.
- Fullscreen mode does not render a duplicate inline graph behind it.
- Graph stays smooth enough for large documents after caps and focus filtering.
- Colors feel native to both app themes.
- Controls look like part of the product UI, not a debug overlay.
