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

- [x] Use local TCP for the first implementation.
- [x] Use length-prefixed frames: `[u32_le frame_len][flatbuffer_bytes]`.
- [ ] Keep the Python worker process long-lived.
- [ ] Do not spawn Python per document.
- [ ] Send file paths in parse requests instead of embedding whole DOCX/PDF bytes in the message.
- [x] Return structured `ParsedDocument` FlatBuffers from Python.
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

- [x] `parser_protocol.fbs`
- [x] `ParseRequest`
- [x] `ParseResponse`
- [x] `ParsedDocument`
- [x] `DocumentBlock`
- [x] `TextSpan`
- [x] `TableBlock`
- [x] `ParserSignal`
- [x] `ParserWarning`
- [x] `DocumentMetadata`

Minimum `ParseRequest` fields:

- [x] `job_id`
- [x] `file_path`
- [x] `filename`
- [x] `mime_type`
- [x] `parser_hints`
- [x] `requested_backends`

Minimum `DocumentBlock` fields:

- [x] `block_id`
- [x] `text`
- [x] `kind`
- [x] `source_backend`
- [x] `page_number`
- [x] `order_index`
- [x] `style_name`
- [x] `heading_level`
- [x] `confidence`
- [x] `bbox`
- [x] `spans`

Block kinds:

- [x] `Paragraph`
- [x] `HeadingCandidate`
- [x] `Table`
- [x] `ListItem`
- [x] `Caption`
- [x] `PageBreak`
- [x] `HeaderFooter`
- [x] `Footnote`
- [x] `Unknown`

Parser signal kinds:

- [x] `DocxStyle`
- [x] `MarkdownHeading`
- [x] `PdfLayout`
- [x] `OcrMarkdown`
- [x] `LexicalPattern`
- [x] `Numbering`
- [x] `TableStructure`

Tooling:

- [x] Pin `flatc` version to `25.12.19`.
- [x] Vendor official `flatc` release archives for Windows, Linux, and macOS under `tools/flatc/v25.12.19`.
- [x] Add cross-platform `flatc` resolver scripts.
- [x] Add cross-platform binding generation scripts.
- [x] Keep extracted binaries out of git under `tools/flatc/.cache`.
- [x] Do not use Git LFS for `flatc` archives; every archive is below 3 MB.
- [ ] Add CI check that generated bindings match `*.fbs`.

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
- [x] Add parser worker client with TCP frame codec.
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

- [x] Unit test FlatBuffers encode/decode for `ParseRequest`.
- [x] Unit test FlatBuffers encode/decode for `ParsedDocument`.
- [x] TCP frame codec test with multiple back-to-back frames.
- [x] Python sidecar smoke test on a small DOCX.
- [ ] DOCX heading style regression test.
- [x] Regression test for body ALL CAPS text not becoming heading.
- [ ] Mistral markdown adapter regression test.
- [ ] Fallback test: DOCX native parser failure -> Mistral OCR.
- [ ] Timeout test.
- [ ] Worker restart/reconnect test.

---

## Migration Steps

- [x] Define `parser_protocol.fbs`.
- [x] Generate Rust FlatBuffers bindings.
- [x] Generate Python FlatBuffers bindings.
- [x] Implement local TCP frame codec in Rust.
- [x] Implement Python sidecar TCP server.
- [ ] Implement DOCX parser backend in Python.
- [x] Add Rust parser client.
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
