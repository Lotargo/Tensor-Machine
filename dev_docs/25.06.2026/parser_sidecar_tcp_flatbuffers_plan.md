# Parser Sidecar over TCP + FlatBuffers

Status: draft decision checklist
Date: 2026-06-25
Scope: move document parsing away from Mistral OCR as the single source of truth, without introducing heavyweight local OCR stacks or gRPC transport.

---

## Decision

Use a long-lived Python parser sidecar for native document parsing, connected to Rust over local TCP with length-prefixed FlatBuffers frames.

Mistral OCR remains available, but becomes one parser signal source rather than the authoritative document structure source.

Do not implement:

- [ ] gRPC for the parser hot path.
- [ ] REST as the parser hot path.
- [ ] Tesseract integration.
- [ ] PaddleOCR integration.
- [ ] Docling integration.

Allowed:

- [ ] REST/debug endpoint later, only for diagnostics and manual inspection.
- [ ] Mistral OCR fallback for scanned or unsupported documents.
- [ ] Native parsers for structured formats, starting with DOCX.

---

## Target Architecture

```text
Rust ingestion
  -> ParserDispatcher
  -> async parse job
  -> Python parser sidecar over local TCP
  -> FlatBuffers ParsedDocument
  -> Rust StructureResolver
  -> policy-driven chunker
  -> embeddings/indexing
```

Principle:

```text
Python extracts parser signals.
Rust owns structure decisions, chunking, metadata, and indexing.
```

---

## Transport

- [ ] Use local TCP for the first implementation.
- [ ] Use length-prefixed frames: `[u32_le frame_len][flatbuffer_bytes]`.
- [ ] Keep the Python worker process long-lived.
- [ ] Do not spawn Python per document.
- [ ] Send file paths in parse requests instead of embedding whole DOCX/PDF bytes in the message.
- [ ] Return structured `ParsedDocument` FlatBuffers from Python.
- [ ] Add per-job timeout.
- [ ] Add cancellation support.
- [ ] Add parser worker health check.
- [ ] Add reconnect/restart behavior when the worker dies.

Rationale:

- TCP is simpler than gRPC for this local sidecar.
- FlatBuffers avoids JSON overhead for large block/span outputs.
- File-path handoff avoids copying large documents through IPC.
- The hot path remains binary and compact.

---

## FlatBuffers Contract

Create a dedicated schema, separate from retrieval result buffers:

- [ ] `parser_protocol.fbs`
- [ ] `ParseRequest`
- [ ] `ParseResponse`
- [ ] `ParsedDocument`
- [ ] `DocumentBlock`
- [ ] `TextSpan`
- [ ] `TableBlock`
- [ ] `ParserSignal`
- [ ] `ParserWarning`
- [ ] `DocumentMetadata`

Minimum `ParseRequest` fields:

- [ ] `job_id`
- [ ] `file_path`
- [ ] `filename`
- [ ] `mime_type`
- [ ] `parser_hints`
- [ ] `requested_backends`

Minimum `DocumentBlock` fields:

- [ ] `block_id`
- [ ] `text`
- [ ] `kind`
- [ ] `source_backend`
- [ ] `page_number`
- [ ] `order_index`
- [ ] `style_name`
- [ ] `heading_level`
- [ ] `confidence`
- [ ] `bbox`
- [ ] `spans`

Block kinds:

- [ ] `Paragraph`
- [ ] `HeadingCandidate`
- [ ] `Table`
- [ ] `ListItem`
- [ ] `Caption`
- [ ] `PageBreak`
- [ ] `HeaderFooter`
- [ ] `Footnote`
- [ ] `Unknown`

Parser signal kinds:

- [ ] `DocxStyle`
- [ ] `MarkdownHeading`
- [ ] `PdfLayout`
- [ ] `OcrMarkdown`
- [ ] `LexicalPattern`
- [ ] `Numbering`
- [ ] `TableStructure`

---

## Parser Backends

### Phase 1: DOCX Native Parser

- [ ] Implement Python DOCX parser sidecar backend.
- [ ] Read Word paragraph styles.
- [ ] Preserve `Heading 1`, `Heading 2`, etc.
- [ ] Preserve document order.
- [ ] Extract tables as structured blocks.
- [ ] Extract lists and numbering as signals where practical.
- [ ] Return style names and confidence values.
- [ ] Treat DOCX heading styles as high-confidence structure.

### Phase 2: Existing Mistral OCR Adapter

- [ ] Wrap current Mistral OCR markdown output into the same `ParsedDocument` model.
- [ ] Convert markdown headings into `HeadingCandidate` blocks.
- [ ] Mark all Mistral-derived structure as `source_backend = MistralOcr`.
- [ ] Assign lower authority than native DOCX styles when both are available.

### Explicitly Out of Scope

- [ ] No Tesseract.
- [ ] No PaddleOCR.
- [ ] No Docling.
- [ ] No local heavy OCR/layout framework in this phase.

---

## Rust Responsibilities

- [ ] Add `ParserDispatcher`.
- [ ] Add parser worker client with TCP frame codec.
- [ ] Add async parse job state.
- [ ] Add parser progress events.
- [ ] Add cancellation path wired into upload cancellation.
- [ ] Add `ParsedDocument` to internal ingestion flow.
- [ ] Add `StructureResolver`.
- [ ] Move heading decisions out of raw `chunker.rs`.
- [ ] Make chunker consume resolved document blocks instead of plain OCR markdown.

Structure resolution priority:

- [ ] Native format structure, e.g. DOCX styles.
- [ ] Parser layout signals.
- [ ] OCR markdown signals.
- [ ] Lexical fallback rules.
- [ ] Low-confidence candidates remain body text.

---

## Async Ingestion Flow

- [ ] Upload accepted.
- [ ] Create `job_id`.
- [ ] Queue parse job.
- [ ] Send parse request to parser sidecar.
- [ ] Receive `ParsedDocument`.
- [ ] Resolve section tree.
- [ ] Chunk by resolved structure and token limits.
- [ ] Embed chunks in batches.
- [ ] Store chunks, metadata, anchors, and relationships.
- [ ] Emit progress for each stage.

Failure behavior:

- [ ] If DOCX native parser fails, fall back to Mistral OCR when configured.
- [ ] If parser sidecar is unavailable, return a controlled ingestion error.
- [ ] If a parse job times out, cancel it and mark upload failed.
- [ ] Do not silently index plain text without parser provenance.

---

## Tests

- [ ] Unit test FlatBuffers encode/decode for `ParseRequest`.
- [ ] Unit test FlatBuffers encode/decode for `ParsedDocument`.
- [ ] TCP frame codec test with multiple back-to-back frames.
- [ ] Python sidecar smoke test on a small DOCX.
- [ ] DOCX heading style regression test.
- [ ] Regression test for body ALL CAPS text not becoming heading.
- [ ] Mistral markdown adapter regression test.
- [ ] Fallback test: DOCX native parser failure -> Mistral OCR.
- [ ] Timeout test.
- [ ] Worker restart/reconnect test.

---

## Migration Steps

- [ ] Define `parser_protocol.fbs`.
- [ ] Generate Rust FlatBuffers bindings.
- [ ] Generate Python FlatBuffers bindings.
- [ ] Implement local TCP frame codec in Rust.
- [ ] Implement Python sidecar TCP server.
- [ ] Implement DOCX parser backend in Python.
- [ ] Add Rust parser client.
- [ ] Add `ParsedDocument` ingestion path behind a feature/config flag.
- [ ] Add Mistral OCR adapter into `ParsedDocument`.
- [ ] Add `StructureResolver`.
- [ ] Refactor `HierarchicalChunker` to consume resolved blocks.
- [ ] Keep old Mistral markdown path as temporary fallback.
- [ ] Add regression suite for current documents.
- [ ] Switch DOCX ingestion default to native parser first.
- [ ] Remove domain-specific heading logic from the generic chunker.

---

## Open Questions

- [ ] Should the parser sidecar run as a child process managed by Rust or as an independently started service?
- [ ] Should TCP bind to localhost only or support container-to-container networking?
- [ ] Should large extracted tables eventually use Arrow IPC instead of FlatBuffers?
- [ ] Should parser results be cached by file hash?
- [ ] Should `ParsedDocument` be persisted for debugging and re-chunking?

