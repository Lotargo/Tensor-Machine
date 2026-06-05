# Knowledge Graph Concept

Status: concept only

Date: 2026-06-05

This document captures an early product and architecture concept for adding a dedicated knowledge graph layer to APL-RAG.

This is not an implementation plan yet. No engineering work should start from this document directly. Before implementation, the idea requires a separate research and design phase.

---

## Concept Summary

APL-RAG may add a dedicated knowledge graph mode inspired by systems such as LightRAG / GraphRAG, but adapted to this project instead of copied directly.

The graph should not replace the current document retrieval pipeline. It should act as a second layer that helps the agent navigate knowledge, relationships, decisions, notes, entities, events, and document references.

The initial intended scope is chat and notes, not automatic full-document summarization for every uploaded document.

---

## Core Idea

Add a special mode/tool where the AI can compress context into structured graph knowledge:

- important entities
- claims
- events
- relationships
- decisions
- open questions
- references to documents or sections
- links between chat context, notes, and indexed documents

This is conceptually similar to context compression, but instead of producing only a text summary, the system would produce a durable graph of knowledge and relationships.

The graph could later help the agent find relevant prior knowledge without rereading the entire chat or searching blindly across all documents.

---

## Important Scope Boundary

The graph must not become the source of truth by default.

For normal document QA:

- the existing RAG search remains the primary factual retrieval path
- the graph may suggest where to look
- final answers should still be grounded in retrieved document text, full document reads, or explicit user-provided notes

For documents:

- uploaded documents should not be automatically summarized into the graph by default
- graph nodes may reference documents, sections, chunks, headings, or anchors
- a separate background action may later allow the user to explicitly ingest a document into the knowledge graph

This distinction is critical to avoid a graph full of ungrounded summaries.

---

## Possible MCP Tool Shape

The system should avoid overwhelming the agent with too many graph tools.

A first tool could be something like:

- `record_context_graph`
- `compress_context_to_graph`
- `update_knowledge_graph`

The exact naming should be decided later.

The tool would accept structured content from the current chat/context and write/update graph nodes and edges.

Later tools may include:

- graph search
- neighborhood expansion
- evidence lookup
- graph-backed memory recall
- background document graph ingestion

These should not be designed until the research phase is complete.

---

## Required Research Phase Before Implementation

Before any implementation work starts, perform a dedicated web research pass.

Research goals:

- Study LightRAG, GraphRAG, and other knowledge-graph RAG systems.
- Identify which design choices worked well.
- Identify common failure modes and operational problems.
- Compare graph extraction approaches:
  - entity/relation extraction
  - claim extraction
  - event extraction
  - hierarchical summaries
  - document anchors
  - incremental memory graphs
- Compare storage approaches:
  - PostgreSQL tables
  - property graph databases
  - RDF/triples
  - hybrid PostgreSQL + vector store
- Study how other systems handle provenance, confidence, citations, and stale knowledge.
- Study how other systems avoid hallucinated or over-confident graph facts.
- Review how graph retrieval should interact with standard dense/BM25 retrieval.

The goal is not to copy another system blindly and not to reinvent the entire field from scratch.

The goal is to design a project-specific graph layer using lessons from existing systems and known mistakes.

---

## Risks To Avoid

### Ungrounded Knowledge

The graph must not store agent guesses as facts without marking them as inferred or low confidence.

Every meaningful node/edge should preserve provenance.

### Source Confusion

The system must distinguish between:

- document text
- user statements
- assistant conclusions
- inferred relationships
- generated summaries
- external knowledge

These should not collapse into the same truth category.

### Agent Overload

The graph layer should not expose too many tools too early.

The agent should have a small number of clear actions, otherwise tool use will become noisy and unreliable.

### Graph Bloat

Automatic graph ingestion can create too many low-value nodes and edges.

The initial implementation should prefer high-value, explicitly useful graph writes over exhaustive extraction.

### Copying Existing Systems Blindly

LightRAG/GraphRAG-style systems are useful references, but this project has its own constraints:

- local document library
- MCP tool integration
- PostgreSQL-backed metadata
- Qdrant vector search
- chat/agent UX
- exact retrieval requirements
- document viewer/debugger requirements

The final design should fit this system.

---

## Initial Design Hypothesis

A conservative first version could use PostgreSQL tables:

- `knowledge_nodes`
- `knowledge_edges`
- `knowledge_evidence`
- `knowledge_sessions`

Potential node types:

- `document`
- `section`
- `chat_note`
- `entity`
- `claim`
- `event`
- `topic`
- `decision`
- `question`

Potential edge types:

- `mentions`
- `supports`
- `contradicts`
- `caused_by`
- `leads_to`
- `related_to`
- `derived_from`
- `references`
- `answers`

Every edge should include provenance and confidence metadata.

This is only a hypothesis and must be validated by the research phase.

---

## Evidence / Provenance Requirements

Every non-trivial graph fact should be traceable.

Possible evidence fields:

- source type
- source document UUID
- source chunk IDs
- source heading/anchor
- source chat session
- source message IDs
- quote or text span
- extraction timestamp
- extraction model/tool
- confidence score
- relation type
- whether the relation is explicit or inferred

The graph should help the agent find evidence, not replace evidence.

---

## Possible Future Workflow

### Chat / Notes Mode

1. User and assistant discuss a topic.
2. The agent periodically calls a graph compression tool.
3. Important entities, decisions, claims, and links are stored.
4. Future chat turns can query the graph for relevant prior context.
5. The agent still verifies document facts through normal retrieval/read tools.

### Optional Document Graph Ingestion

1. User uploads a document.
2. Normal ingestion indexes it for search.
3. User optionally requests background graph ingestion.
4. A background worker analyzes the document structure and extracts high-value graph relationships.
5. Graph nodes reference document sections and chunks instead of replacing the document with a summary.

---

## Open Questions

- Should graph storage start in PostgreSQL or use a dedicated graph database?
- What should be the smallest useful MCP tool set?
- How should graph updates handle corrections and contradictions?
- How should stale or superseded knowledge be marked?
- Should user notes and document-derived facts live in the same graph with different provenance, or in separate graphs?
- How much automatic extraction is safe before the graph becomes noisy?
- What UI should expose graph knowledge without confusing it with document retrieval?

---

## Decision Gate

Before implementation, create a separate research document that summarizes:

- systems reviewed
- techniques compared
- common failure modes
- recommended architecture
- first milestone scope
- non-goals
- testing strategy

Only after that should this concept become an actionable roadmap.

