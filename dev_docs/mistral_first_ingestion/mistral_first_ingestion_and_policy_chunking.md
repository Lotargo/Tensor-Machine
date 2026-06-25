# Mistral-First Ingestion and Policy-Driven Chunking

Status: design note / implementation candidate  
Date: 2026-06-25  
Scope: Tensor Machine document ingestion, OCR/parsing backends, chunking policy selection, metadata, and embedding text construction

---

## 1. Purpose

This document defines a Mistral-first ingestion strategy for Tensor Machine.

The main point is to separate three concerns that are often mixed together in RAG systems:

```text
Parsing backend
  != Chunking policy
  != Embedding template
```

Tensor Machine should not become a parser zoo during the first implementation stage. The MVP should use Mistral OCR as the primary parsing/OCR backend and treat its Markdown-like output as structured input for policy-driven chunking.

Future local parsing and heavy Docling-based parsing modes should be anticipated in the interfaces, but not implemented first.

---

## 2. High-Level Direction

MVP direction:

```text
Mistral OCR is the primary ingestion backend.
Mistral gives structured Markdown-like output and image/page information.
Tensor Machine controls chunking, metadata, embedding text, and indexing.
```

Non-MVP future directions:

```text
LocalLightBackend:
  lightweight local parsers, no guarantee of Markdown output

HeavyDoclingBackend:
  Docling + Tesseract/RapidOCR or similar heavy local layout/OCR stack
```

Short formula:

```text
Mistral gives structure.
Chunking policy preserves structure.
Embedding template explains structure to the vector model.
Metadata remembers structure for retrieval.
```

---

## 3. Why This Matters

Chunking is not just preprocessing. Chunking creates the memory layout of the RAG system.

Retrieval can compensate for weak chunking only to a limited degree. If the ingestion layer cuts a table away from its header, separates an image from its caption, splits a code block in the middle, or removes page/section context, later retrieval and reranking must work against damaged memory.

A better approach:

```text
Document
  -> parsing backend
  -> parsed document representation
  -> document profiler
  -> chunking policy selection
  -> deterministic policy-driven chunking
  -> policy-specific embedding_text
  -> metadata-rich indexing
```

This keeps the chunking core deterministic while allowing behavior to change based on document source, format, and structure.

---

## 4. Ingestion Backends

### 4.1 MVP Backend: Mistral OCR

Mistral OCR mode is the first and primary implementation target.

Expected input:

```text
PDFs
scans
images
OCR-heavy documents
layout-rich documents
```

Expected output:

```text
Markdown-like text
page structure when available
image references or extracted images when available
tables, lists, headings, captions, and layout hints when available
```

Tensor Machine should treat this output as structured Markdown, not as plain text.

Recommended mode name:

```rust
ParsingBackend::MistralOcr
```

### 4.2 Future Backend: LocalLight

This mode is not MVP.

Purpose:

```text
cheap local parsing without heavy layout conversion
```

Possible future tools:

```text
PDF text extraction libraries
DOCX zip/XML extraction
HTML parsing
plain Markdown/TXT direct loading
basic image extraction if cheap
```

Important constraint:

```text
LocalLightBackend does not have to produce Markdown.
```

It may return plain text plus minimal metadata.

Recommended mode name:

```rust
ParsingBackend::LocalLight
```

### 4.3 Future Backend: HeavyDocling

This mode is not MVP.

Purpose:

```text
expensive local fallback for offline/private/high-layout workloads
```

Possible future tools:

```text
Docling
Tesseract
RapidOCR
local layout extraction
local OCR pipelines
```

Use only when:

```text
Mistral is unavailable
privacy requires local processing
OCR quality must be inspected locally
a document requires heavy layout analysis
```

Recommended mode name:

```rust
ParsingBackend::HeavyDocling
```

---

## 5. Unified ParsedDocument Contract

All parsing backends should return the same internal representation.

```rust
pub enum ParsingBackend {
    MistralOcr,
    LocalLight,
    HeavyDocling,
}

pub struct ParsedDocument {
    pub source_path: String,
    pub source_format: SourceFormat,
    pub backend: ParsingBackend,
    pub markdown: Option<String>,
    pub text: String,
    pub pages: Vec<ParsedPage>,
    pub images: Vec<ParsedImage>,
    pub metadata: ParsedDocumentMetadata,
}

pub struct ParsedPage {
    pub page_number: u32,
    pub text: String,
    pub markdown: Option<String>,
    pub image_refs: Vec<String>,
}

pub struct ParsedImage {
    pub id: String,
    pub page_number: Option<u32>,
    pub path_or_url: String,
    pub alt_text: Option<String>,
    pub caption: Option<String>,
    pub nearby_heading: Option<String>,
    pub nearby_text: Option<String>,
}
```

For Mistral OCR mode:

```text
markdown = Some(...)
pages = best available page structure
images = extracted or referenced images
text = Markdown stripped or normalized text fallback
```

For LocalLight mode:

```text
markdown = None or simple generated Markdown
pages = minimal or empty
images = minimal or empty
text = extracted text
```

For HeavyDocling mode:

```text
markdown = Some(...) when conversion is available
pages = richer local layout metadata
images = extracted locally
```

---

## 6. Mistral OCR as Structured Markdown Input

Mistral OCR output should be treated as Markdown-like layout, not plain text.

The Mistral-aware chunker should preserve:

```text
headings
header chains
page boundaries
tables
image references
captions
lists
code blocks
paragraph boundaries
OCR-specific artifacts when useful
```

Bad ingestion:

```text
Mistral OCR -> plain text -> generic token splitter
```

Better ingestion:

```text
Mistral OCR -> Markdown/layout-aware spans -> policy-driven hierarchical chunks
```

The current hierarchical chunking model already has useful concepts:

```text
macro chunks
medium chunks
micro chunks
header_chain
parent links
macro_index / medium_index
zone_keywords
entities
embedding_text
```

The Mistral mode should extend this model with page and image awareness.

---

## 7. Chunking Policy Layer

Chunking policy should be selected after parsing and profiling.

Example policy:

```yaml
id: chunk.mistral_ocr_markdown
kind: chunking_policy
backend: mistral_ocr
description: Use for Markdown-like OCR output produced by Mistral OCR, including page structure, images, tables, headings, and OCR artifacts.

rules:
  input_format: markdown
  preserve_headings: true
  preserve_tables: true
  preserve_images_with_captions: true
  preserve_code_blocks: true
  preserve_lists: true
  preserve_page_boundaries: true
  attach_page_number: true
  attach_image_refs: true
  attach_header_chain: true
  attach_ocr_backend: true

tokens:
  macro_tokens: 2200
  macro_overlap: 350
  medium_tokens: 650
  medium_overlap: 120
  micro_tokens: 180
  micro_overlap: 40

embedding_template: embed_doc.mistral_page_aware
```

Chunking policy is not an LLM prompt. It is an executable ingestion policy.

---

## 8. Document Profiling

Before chunking, Tensor Machine should profile the parsed document.

Inputs:

```text
source extension
parsing backend
presence of Markdown headings
table density
image references
code fences
page markers
line length distribution
OCR markers
language hints
metadata from parser
```

Possible profile:

```rust
pub struct DocumentProfile {
    pub backend: ParsingBackend,
    pub source_format: SourceFormat,
    pub document_kind: DocumentKind,
    pub has_markdown: bool,
    pub has_tables: bool,
    pub has_images: bool,
    pub has_code_blocks: bool,
    pub has_page_boundaries: bool,
    pub language_hint: Option<String>,
    pub confidence: f32,
}
```

Possible document kinds:

```rust
pub enum DocumentKind {
    MistralMarkdownOcr,
    MarkdownTechnicalDoc,
    ResearchPaper,
    SourceCode,
    Logs,
    NarrativeBook,
    ConfigFile,
    ApiReference,
    Mixed,
}
```

MVP can use a simple rules-based profiler.

Example:

```text
backend = MistralOcr
markdown = Some(...)
headings/tables/images detected
=> DocumentKind::MistralMarkdownOcr
=> chunk.mistral_ocr_markdown
```

---

## 9. Policy-Driven Chunker

Recommended wrapper:

```rust
pub struct PolicyDrivenChunker {
    registry: ChunkingPolicyRegistry,
}

impl PolicyDrivenChunker {
    pub fn chunk(
        &self,
        parsed: &ParsedDocument,
    ) -> Result<Vec<HierarchicalChunkItem>, ChunkingError> {
        let profile = DocumentProfiler::profile(parsed);
        let policy = self.registry.select(&profile)?;
        HierarchicalChunker::from_policy(&policy).chunk_parsed(parsed, &policy)
    }
}
```

This keeps `HierarchicalChunker` as the deterministic execution engine.

The chunker should not know about Mistral API calls directly. It should know only the parsed document and the selected policy.

---

## 10. Mistral-Aware Span Model

The current span model can be extended.

Candidate structure:

```rust
pub enum SpanKind {
    Heading,
    Paragraph,
    Table,
    Image,
    Caption,
    CodeBlock,
    ListItem,
    PageBreak,
    Unknown,
}

pub struct LayoutSpan {
    pub kind: SpanKind,
    pub text: String,
    pub markdown: Option<String>,
    pub tokens: usize,
    pub page_number: Option<u32>,
    pub image_ref: Option<String>,
    pub is_new_paragraph: bool,
    pub header_level: Option<usize>,
}
```

Mistral Markdown should first be converted into `LayoutSpan` values.

Then chunks are assembled from spans.

Rules:

```text
Do not split inside small tables.
For large tables, repeat the header row in each table chunk.
Keep image references close to captions and nearby explanatory text.
Keep headings with their first content block when possible.
Do not split code blocks unless they exceed hard limits.
Preserve page numbers in chunk metadata.
```

---

## 11. Image Handling

Images must become first-class metadata.

Candidate chunk fields:

```rust
pub struct HierarchicalChunkItem {
    // existing fields...
    pub page_start: Option<u32>,
    pub page_end: Option<u32>,
    pub image_refs: Vec<String>,
    pub contains_table: bool,
    pub contains_image: bool,
    pub contains_code_block: bool,
    pub parsing_backend: ParsingBackend,
    pub chunking_policy_id: String,
    pub embedding_template_id: String,
}
```

Even if retrieval is initially text-only, storing image refs now prepares the system for future multimodal retrieval.

Example future query:

```text
Find the diagram that explains the indexing pipeline.
```

The system should be able to retrieve both the nearby textual chunk and the image reference.

---

## 12. Embedding Text Template for Mistral OCR

The embedding text should depend on the ingestion policy.

Candidate template:

```text
File: {filename}.
Backend: Mistral OCR.
Document kind: {document_kind}.
Page: {page_start}-{page_end}.
Level: {chunk_level}.
Section: {header_chain}.
Contains: {tables/images/code/lists}.
Image refs: {image_refs}.
Text:
{chunk.text}
```

This is better than embedding only raw chunk text because it gives the embedding model structured context.

For OCR documents, context is often carried by page, section, image, and table position.

---

## 13. Metadata Requirements

Every chunk produced from Mistral OCR should store at least:

```json
{
  "parsing_backend": "mistral_ocr",
  "parsing_backend_version": "mistral-ocr-current",
  "chunking_policy_id": "chunk.mistral_ocr_markdown",
  "chunking_policy_version": "1.0.0",
  "embedding_template_id": "embed_doc.mistral_page_aware",
  "source_format": "pdf",
  "intermediate_format": "markdown",
  "page_start": 3,
  "page_end": 4,
  "image_refs": ["img_003_01"],
  "contains_table": true,
  "contains_code_block": false,
  "contains_image": true,
  "ocr_confidence": null
}
```

Fields can be nullable at first. The schema should still reserve them.

This enables later diagnostics:

```text
Which chunking policy produced this chunk?
Was it created from Mistral OCR or local parsing?
Was retrieval failure caused by parser output, chunking policy, or embedding template?
```

---

## 14. Future LocalLight Mode

LocalLight mode should be planned as a cheap local fallback, but not implemented in MVP.

Expected behavior:

```text
Use local libraries.
Avoid heavy Markdown conversion.
Return plain text plus minimal structure.
Do not require Docling.
Do not require OCR unless explicitly configured.
```

Possible future policy:

```yaml
id: chunk.local_light_plaintext
kind: chunking_policy
backend: local_light
description: Use for locally extracted plain text or lightly structured documents.

rules:
  input_format: plain_text_or_minimal_structure
  preserve_headings_if_detected: true
  infer_paragraphs: true
  preserve_page_breaks_if_available: true
  attach_header_chain: best_effort

tokens:
  macro_tokens: 2000
  macro_overlap: 300
  medium_tokens: 550
  medium_overlap: 100
  micro_tokens: 160
  micro_overlap: 32

embedding_template: embed_doc.local_light_plaintext
```

---

## 15. Future HeavyDocling Mode

HeavyDocling mode should be treated as an expensive fallback.

It should not shape the MVP architecture.

Use cases:

```text
offline/private processing
bad OCR quality from external provider
need for local table/layout recovery
need for local OCR over scanned images
```

Possible future policy:

```yaml
id: chunk.heavy_docling_layout
kind: chunking_policy
backend: heavy_docling
description: Use for locally parsed layout-rich documents processed by Docling and OCR backends.

rules:
  input_format: rich_layout
  preserve_layout_blocks: true
  preserve_tables: true
  preserve_figures: true
  preserve_captions: true
  attach_coordinates_if_available: true
  attach_page_number: true

embedding_template: embed_doc.layout_aware
```

---

## 16. Suggested MVP Implementation Steps

MVP should be intentionally small.

Implement now:

```text
1. Define ParsingBackend enum.
2. Define ParsedDocument, ParsedPage, ParsedImage.
3. Implement MistralOcrBackend as the only active backend.
4. Define ChunkingPolicy and EmbeddingTemplate metadata.
5. Add chunk.mistral_ocr_markdown policy.
6. Add Mistral Markdown/Layout span extraction.
7. Add page/image/table/code metadata to chunks.
8. Add embed_doc.mistral_page_aware template.
9. Store parsing_backend, chunking_policy_id, and embedding_template_id in chunk metadata.
10. Leave LocalLightBackend and HeavyDoclingBackend as TODO interfaces.
```

Do not implement now:

```text
Docling integration
Tesseract integration
RapidOCR integration
local parser zoo
full multimodal retrieval
multiple document-side embedding indexes
```

---

## 17. Evaluation Plan

Test Mistral-first ingestion with documents that include:

```text
1. Normal text sections
2. Tables
3. Images with captions
4. Scanned/OCR-like pages
5. Mixed Russian/English text
6. Technical docs with code blocks
7. Long PDF reports
8. Documents with weak or inconsistent headings
```

Metrics:

```text
chunk_boundary_quality
table_preservation_rate
image_caption_link_rate
heading_chain_accuracy
page_metadata_accuracy
retrieval_recall_by_document_type
answer_groundedness
citation_support_rate
chunks_per_page
average_chunk_tokens
indexing_latency
```

Important failure diagnostics:

```text
Was the failure caused by OCR output?
Was the failure caused by chunking policy?
Was the failure caused by embedding template?
Was the failure caused by retrieval strategy?
```

---

## 18. Relationship to Skill-Routed Retrieval

This document complements the skill-routed retrieval architecture.

Skill-routed retrieval works at query time:

```text
User query
  -> select agent skill
  -> select embedding instruction
  -> retrieve/fuse/rerank
```

Mistral-first policy chunking works at ingestion time:

```text
Document
  -> select parsing backend
  -> select chunking policy
  -> build embedding text
  -> index metadata-rich chunks
```

Together:

```text
Ingestion policies create better memory.
Retrieval skills access that memory more intelligently.
```

Short formula:

```text
Chunking skills form memory.
Retrieval skills navigate memory.
Embedding instructions project queries into memory.
```

---

## 19. Final Summary

The first implementation should not attempt to solve all document parsing.

Recommended path:

```text
Mistral-first now.
Local-light later.
Docling-heavy last.
```

Tensor Machine should use Mistral OCR as a structured Markdown provider, then apply its own policy-driven chunking and metadata model.

This preserves the project's core identity:

```text
Tensor Machine is not an OCR framework.
Tensor Machine is a programmable memory formation and retrieval engine.
```

The ingestion layer should therefore focus on:

```text
stable backend contracts
Mistral-aware Markdown chunking
metadata-rich chunk records
policy-specific embedding text
future-ready backend interfaces
```

The result is a cleaner architecture:

```text
Parse with the best available backend.
Chunk according to document structure.
Embed with explicit context.
Retrieve with skill-routed plans.
```
