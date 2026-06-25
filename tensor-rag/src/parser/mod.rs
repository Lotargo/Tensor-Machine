use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::parser_protocol_generated::tensor_rag::parser as fb;

pub const MAX_FRAME_BYTES: usize = 128 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ParserBackend {
    Unknown,
    DocxNative,
    MistralOcr,
    PdfNative,
    PlainText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BlockKind {
    Unknown,
    Paragraph,
    HeadingCandidate,
    Table,
    ListItem,
    Caption,
    PageBreak,
    HeaderFooter,
    Footnote,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ParseRequest {
    pub job_id: String,
    pub file_path: String,
    pub filename: String,
    pub mime_type: String,
    pub parser_hints: Vec<String>,
    pub requested_backends: Vec<ParserBackend>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DocumentMetadata {
    pub filename: String,
    pub mime_type: String,
    pub parser_name: String,
    pub parser_version: String,
    pub source_path: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DocumentBlock {
    pub block_id: String,
    pub text: String,
    pub kind: BlockKind,
    pub source_backend: ParserBackend,
    pub page_number: u32,
    pub order_index: u32,
    pub style_name: Option<String>,
    pub heading_level: Option<u32>,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ParsedDocument {
    pub job_id: String,
    pub metadata: DocumentMetadata,
    pub blocks: Vec<DocumentBlock>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ParseResponse {
    pub job_id: String,
    pub ok: bool,
    pub document: Option<ParsedDocument>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ParserClient {
    addr: String,
}

impl ParserClient {
    pub fn new(addr: impl Into<String>) -> Self {
        Self { addr: addr.into() }
    }

    pub async fn parse(&self, request: &ParseRequest) -> Result<ParseResponse, String> {
        let mut stream = TcpStream::connect(&self.addr)
            .await
            .map_err(|err| format!("failed to connect parser sidecar at {}: {err}", self.addr))?;
        let payload = encode_parse_request(request);
        write_frame(&mut stream, &payload)
            .await
            .map_err(|err| format!("failed to write parser request: {err}"))?;
        let response = read_frame(&mut stream)
            .await
            .map_err(|err| format!("failed to read parser response: {err}"))?;
        decode_parse_response(&response)
            .map_err(|err| format!("invalid parser response flatbuffer: {err}"))
    }
}

pub fn encode_parse_request(request: &ParseRequest) -> Vec<u8> {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();

    let job_id = fbb.create_string(&request.job_id);
    let file_path = fbb.create_string(&request.file_path);
    let filename = fbb.create_string(&request.filename);
    let mime_type = fbb.create_string(&request.mime_type);
    let hint_offsets = request
        .parser_hints
        .iter()
        .map(|hint| fbb.create_string(hint))
        .collect::<Vec<_>>();
    let parser_hints = fbb.create_vector(&hint_offsets);
    let backends = request
        .requested_backends
        .iter()
        .copied()
        .map(to_fb_backend)
        .collect::<Vec<_>>();
    let requested_backends = fbb.create_vector(&backends);

    let root = fb::ParseRequest::create(
        &mut fbb,
        &fb::ParseRequestArgs {
            job_id: Some(job_id),
            file_path: Some(file_path),
            filename: Some(filename),
            mime_type: Some(mime_type),
            parser_hints: Some(parser_hints),
            requested_backends: Some(requested_backends),
        },
    );
    fbb.finish(root, None);
    fbb.finished_data().to_vec()
}

pub fn decode_parse_request(bytes: &[u8]) -> Result<ParseRequest, flatbuffers::InvalidFlatbuffer> {
    let request = flatbuffers::root::<fb::ParseRequest>(bytes)?;
    Ok(ParseRequest {
        job_id: request.job_id().unwrap_or_default().to_string(),
        file_path: request.file_path().unwrap_or_default().to_string(),
        filename: request.filename().unwrap_or_default().to_string(),
        mime_type: request.mime_type().unwrap_or_default().to_string(),
        parser_hints: request
            .parser_hints()
            .map(|hints| {
                (0..hints.len())
                    .map(|idx| hints.get(idx).to_string())
                    .collect()
            })
            .unwrap_or_default(),
        requested_backends: request
            .requested_backends()
            .map(|backends| {
                (0..backends.len())
                    .map(|idx| from_fb_backend(backends.get(idx)))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

pub fn encode_parse_response(response: &ParseResponse) -> Vec<u8> {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();

    let job_id = fbb.create_string(&response.job_id);
    let error_code = response
        .error_code
        .as_ref()
        .map(|value| fbb.create_string(value));
    let error_message = response
        .error_message
        .as_ref()
        .map(|value| fbb.create_string(value));
    let document = response
        .document
        .as_ref()
        .map(|document| build_parsed_document(&mut fbb, document));

    let root = fb::ParseResponse::create(
        &mut fbb,
        &fb::ParseResponseArgs {
            job_id: Some(job_id),
            ok: response.ok,
            document,
            error_code,
            error_message,
        },
    );
    fb::finish_parse_response_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

pub fn decode_parse_response(
    bytes: &[u8],
) -> Result<ParseResponse, flatbuffers::InvalidFlatbuffer> {
    let response = fb::root_as_parse_response(bytes)?;
    Ok(ParseResponse {
        job_id: response.job_id().unwrap_or_default().to_string(),
        ok: response.ok(),
        document: response.document().map(decode_document),
        error_code: response.error_code().map(str::to_string),
        error_message: response.error_message().map(str::to_string),
    })
}

pub async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    payload: &[u8],
) -> std::io::Result<()> {
    if payload.len() > MAX_FRAME_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "parser frame exceeds max size",
        ));
    }
    writer
        .write_all(&(payload.len() as u32).to_le_bytes())
        .await?;
    writer.write_all(payload).await?;
    writer.flush().await
}

pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> std::io::Result<Vec<u8>> {
    let mut len_bytes = [0u8; 4];
    reader.read_exact(&mut len_bytes).await?;
    let len = u32::from_le_bytes(len_bytes) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "parser frame exceeds max size",
        ));
    }
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload).await?;
    Ok(payload)
}

fn build_parsed_document<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    document: &ParsedDocument,
) -> flatbuffers::WIPOffset<fb::ParsedDocument<'a>> {
    let job_id = fbb.create_string(&document.job_id);
    let filename = fbb.create_string(&document.metadata.filename);
    let mime_type = fbb.create_string(&document.metadata.mime_type);
    let parser_name = fbb.create_string(&document.metadata.parser_name);
    let parser_version = fbb.create_string(&document.metadata.parser_version);
    let source_path = fbb.create_string(&document.metadata.source_path);
    let metadata = fb::DocumentMetadata::create(
        fbb,
        &fb::DocumentMetadataArgs {
            filename: Some(filename),
            mime_type: Some(mime_type),
            parser_name: Some(parser_name),
            parser_version: Some(parser_version),
            source_path: Some(source_path),
        },
    );

    let block_offsets = document
        .blocks
        .iter()
        .map(|block| build_document_block(fbb, block))
        .collect::<Vec<_>>();
    let blocks = fbb.create_vector(&block_offsets);
    let warnings = fbb.create_vector::<flatbuffers::ForwardsUOffset<fb::ParserWarning>>(&[]);

    fb::ParsedDocument::create(
        fbb,
        &fb::ParsedDocumentArgs {
            job_id: Some(job_id),
            metadata: Some(metadata),
            blocks: Some(blocks),
            warnings: Some(warnings),
        },
    )
}

fn build_document_block<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    block: &DocumentBlock,
) -> flatbuffers::WIPOffset<fb::DocumentBlock<'a>> {
    let block_id = fbb.create_string(&block.block_id);
    let text = fbb.create_string(&block.text);
    let style_name = block
        .style_name
        .as_ref()
        .map(|value| fbb.create_string(value));
    let spans = fbb.create_vector::<flatbuffers::ForwardsUOffset<fb::TextSpan>>(&[]);
    let signals = fbb.create_vector::<flatbuffers::ForwardsUOffset<fb::ParserSignal>>(&[]);

    fb::DocumentBlock::create(
        fbb,
        &fb::DocumentBlockArgs {
            block_id: Some(block_id),
            text: Some(text),
            kind: to_fb_block_kind(block.kind),
            source_backend: to_fb_backend(block.source_backend),
            page_number: block.page_number,
            order_index: block.order_index,
            style_name,
            heading_level: block.heading_level.unwrap_or(0),
            confidence: block.confidence,
            bbox: None,
            spans: Some(spans),
            table: None,
            signals: Some(signals),
        },
    )
}

fn decode_document(document: fb::ParsedDocument<'_>) -> ParsedDocument {
    let metadata = document.metadata();
    ParsedDocument {
        job_id: document.job_id().unwrap_or_default().to_string(),
        metadata: DocumentMetadata {
            filename: metadata
                .and_then(|m| m.filename())
                .unwrap_or_default()
                .to_string(),
            mime_type: metadata
                .and_then(|m| m.mime_type())
                .unwrap_or_default()
                .to_string(),
            parser_name: metadata
                .and_then(|m| m.parser_name())
                .unwrap_or_default()
                .to_string(),
            parser_version: metadata
                .and_then(|m| m.parser_version())
                .unwrap_or_default()
                .to_string(),
            source_path: metadata
                .and_then(|m| m.source_path())
                .unwrap_or_default()
                .to_string(),
        },
        blocks: document
            .blocks()
            .map(|blocks| {
                (0..blocks.len())
                    .map(|idx| decode_block(blocks.get(idx)))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn decode_block(block: fb::DocumentBlock<'_>) -> DocumentBlock {
    let heading_level = block.heading_level();
    DocumentBlock {
        block_id: block.block_id().unwrap_or_default().to_string(),
        text: block.text().unwrap_or_default().to_string(),
        kind: from_fb_block_kind(block.kind()),
        source_backend: from_fb_backend(block.source_backend()),
        page_number: block.page_number(),
        order_index: block.order_index(),
        style_name: block.style_name().map(str::to_string),
        heading_level: (heading_level > 0).then_some(heading_level),
        confidence: block.confidence(),
    }
}

fn to_fb_backend(backend: ParserBackend) -> fb::ParserBackend {
    match backend {
        ParserBackend::Unknown => fb::ParserBackend::Unknown,
        ParserBackend::DocxNative => fb::ParserBackend::DocxNative,
        ParserBackend::MistralOcr => fb::ParserBackend::MistralOcr,
        ParserBackend::PdfNative => fb::ParserBackend::PdfNative,
        ParserBackend::PlainText => fb::ParserBackend::PlainText,
    }
}

fn from_fb_backend(backend: fb::ParserBackend) -> ParserBackend {
    match backend {
        fb::ParserBackend::DocxNative => ParserBackend::DocxNative,
        fb::ParserBackend::MistralOcr => ParserBackend::MistralOcr,
        fb::ParserBackend::PdfNative => ParserBackend::PdfNative,
        fb::ParserBackend::PlainText => ParserBackend::PlainText,
        _ => ParserBackend::Unknown,
    }
}

fn to_fb_block_kind(kind: BlockKind) -> fb::BlockKind {
    match kind {
        BlockKind::Unknown => fb::BlockKind::Unknown,
        BlockKind::Paragraph => fb::BlockKind::Paragraph,
        BlockKind::HeadingCandidate => fb::BlockKind::HeadingCandidate,
        BlockKind::Table => fb::BlockKind::Table,
        BlockKind::ListItem => fb::BlockKind::ListItem,
        BlockKind::Caption => fb::BlockKind::Caption,
        BlockKind::PageBreak => fb::BlockKind::PageBreak,
        BlockKind::HeaderFooter => fb::BlockKind::HeaderFooter,
        BlockKind::Footnote => fb::BlockKind::Footnote,
    }
}

fn from_fb_block_kind(kind: fb::BlockKind) -> BlockKind {
    match kind {
        fb::BlockKind::Paragraph => BlockKind::Paragraph,
        fb::BlockKind::HeadingCandidate => BlockKind::HeadingCandidate,
        fb::BlockKind::Table => BlockKind::Table,
        fb::BlockKind::ListItem => BlockKind::ListItem,
        fb::BlockKind::Caption => BlockKind::Caption,
        fb::BlockKind::PageBreak => BlockKind::PageBreak,
        fb::BlockKind::HeaderFooter => BlockKind::HeaderFooter,
        fb::BlockKind::Footnote => BlockKind::Footnote,
        _ => BlockKind::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_request_round_trips_flatbuffer() {
        let request = ParseRequest {
            job_id: "job-1".to_string(),
            file_path: "F:/docs/sample.docx".to_string(),
            filename: "sample.docx".to_string(),
            mime_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                .to_string(),
            parser_hints: vec!["prefer_native".to_string()],
            requested_backends: vec![ParserBackend::DocxNative, ParserBackend::MistralOcr],
        };

        let bytes = encode_parse_request(&request);
        let decoded = decode_parse_request(&bytes).expect("valid parse request");

        assert_eq!(decoded, request);
    }

    #[test]
    fn parse_response_round_trips_flatbuffer() {
        let response = ParseResponse {
            job_id: "job-2".to_string(),
            ok: true,
            document: Some(ParsedDocument {
                job_id: "job-2".to_string(),
                metadata: DocumentMetadata {
                    filename: "sample.docx".to_string(),
                    mime_type:
                        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                            .to_string(),
                    parser_name: "python-docx-sidecar".to_string(),
                    parser_version: "0.1.0".to_string(),
                    source_path: "F:/docs/sample.docx".to_string(),
                },
                blocks: vec![DocumentBlock {
                    block_id: "b1".to_string(),
                    text: "ГЛАВА 1".to_string(),
                    kind: BlockKind::HeadingCandidate,
                    source_backend: ParserBackend::DocxNative,
                    page_number: 1,
                    order_index: 0,
                    style_name: Some("Heading 1".to_string()),
                    heading_level: Some(1),
                    confidence: 0.99,
                }],
            }),
            error_code: None,
            error_message: None,
        };

        let bytes = encode_parse_response(&response);
        let decoded = decode_parse_response(&bytes).expect("valid parse response");

        assert_eq!(decoded, response);
    }

    #[tokio::test]
    async fn frame_codec_reads_back_to_back_frames() {
        let (mut client, mut server) = tokio::io::duplex(1024);
        let first = b"first-frame".to_vec();
        let second = b"second-frame".to_vec();

        let writer = tokio::spawn(async move {
            write_frame(&mut client, &first).await.unwrap();
            write_frame(&mut client, &second).await.unwrap();
        });

        assert_eq!(read_frame(&mut server).await.unwrap(), b"first-frame");
        assert_eq!(read_frame(&mut server).await.unwrap(), b"second-frame");
        writer.await.unwrap();
    }

    #[tokio::test]
    async fn parser_client_sends_request_and_decodes_response() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let payload = read_frame(&mut socket).await.unwrap();
            let request = decode_parse_request(&payload).unwrap();
            assert_eq!(request.job_id, "job-client");

            let response = ParseResponse {
                job_id: request.job_id,
                ok: false,
                document: None,
                error_code: Some("NOT_IMPLEMENTED".to_string()),
                error_message: Some("parser backend missing".to_string()),
            };
            write_frame(&mut socket, &encode_parse_response(&response))
                .await
                .unwrap();
        });

        let client = ParserClient::new(addr.to_string());
        let response = client
            .parse(&ParseRequest {
                job_id: "job-client".to_string(),
                file_path: "F:/docs/sample.docx".to_string(),
                filename: "sample.docx".to_string(),
                mime_type:
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                        .to_string(),
                parser_hints: Vec::new(),
                requested_backends: vec![ParserBackend::DocxNative],
            })
            .await
            .unwrap();

        assert!(!response.ok);
        assert_eq!(response.error_code.as_deref(), Some("NOT_IMPLEMENTED"));
        server.await.unwrap();
    }
}
