#[derive(Debug, Clone, serde::Serialize)]
pub struct StoredDocument {
    pub filename: String,
    pub doc_uuid: String,
    pub text: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChunkDetail {
    pub id: usize,
    pub text: String,
    pub filename: String,
    pub doc_uuid: String,
    pub chapter_id: String,
    pub chunk_index: i32,
    pub chunk_level: String,
    pub parent_chunk_index: Option<i32>,
    pub macro_index: Option<i32>,
    pub medium_index: Option<i32>,
    pub header_chain: Vec<String>,
    pub zone_keywords: Vec<String>,
    pub entities: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CollectionHealth {
    pub collection_name: String,
    pub exists: bool,
    pub configured_dim: usize,
    pub actual_dim: Option<usize>,
    pub points_count: Option<u64>,
    pub dimension_matches: bool,
}

pub struct EnrichedDocumentInsert<'a> {
    pub text: &'a str,
    pub vector: ndarray::Array1<f32>,
    pub filename: &'a str,
    pub doc_uuid: &'a str,
    pub chapter_id: &'a str,
    pub chunk_index: i32,
    pub chunk_level: &'a str,
    pub parent_chunk_index: Option<i32>,
    pub macro_index: Option<i32>,
    pub medium_index: Option<i32>,
    pub header_chain: &'a [String],
    pub zone_keywords: &'a [String],
    pub entities: &'a [String],
}
