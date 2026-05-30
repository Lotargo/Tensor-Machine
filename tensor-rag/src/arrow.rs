use arrow::{
    array::{ArrayRef, Float32Array, Int64Array, StringArray, FixedSizeListArray},
    datatypes::{DataType, Field, Schema},
    record_batch::RecordBatch,
};
use ndarray::Array1;
use std::sync::Arc;

/// Defines the schema of our RecordBatch
/// Columns:
/// - id: Int64
/// - vector: FixedSizeList(Float32, 768)
/// - bm25_score: Float32
/// - text: Utf8
pub fn build_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new(
            "vector",
            DataType::FixedSizeList(Arc::new(Field::new("item", DataType::Float32, true)), 768),
            false,
        ),
        Field::new("bm25_score", DataType::Float32, false),
        Field::new("text", DataType::Utf8, false),
    ]))
}

/// Converts Qdrant results (ids, vectors) to a RecordBatch.
pub fn qdrant_to_record_batch(
    ids: &[usize],
    vectors: &[Array1<f32>],
) -> Result<RecordBatch, arrow::error::ArrowError> {
    if ids.len() != vectors.len() {
        return Err(arrow::error::ArrowError::InvalidArgumentError(
            "ids and vectors length mismatch".to_string(),
        ));
    }

    let id_array = Int64Array::from(ids.iter().map(|&id| id as i64).collect::<Vec<_>>());

    let mut flat_vectors: Vec<f32> = Vec::with_capacity(ids.len() * 768);
    for vec in vectors {
        if vec.len() != 768 {
            return Err(arrow::error::ArrowError::InvalidArgumentError(
                "Vector length must be exactly 768".to_string(),
            ));
        }
        flat_vectors.extend(vec.iter().copied());
    }

    let inner_array = Float32Array::from(flat_vectors);

    let vector_array = FixedSizeListArray::new(
        Arc::new(Field::new("item", DataType::Float32, true)),
        768,
        Arc::new(inner_array) as ArrayRef,
        None,
    );

    // Default bm25_score to 0.0
    let bm25_array = Float32Array::from(vec![0.0; ids.len()]);

    // Default text to empty string
    let text_array = StringArray::from(vec![""; ids.len()]);

    RecordBatch::try_new(
        build_schema(),
        vec![
            Arc::new(id_array) as ArrayRef,
            Arc::new(vector_array) as ArrayRef,
            Arc::new(bm25_array) as ArrayRef,
            Arc::new(text_array) as ArrayRef,
        ],
    )
}

/// Converts PostgreSQL results (ids, bm25_scores, texts) to a RecordBatch.
pub fn postgres_to_record_batch(
    ids: &[usize],
    bm25_scores: &[f32],
    texts: &[String],
) -> Result<RecordBatch, arrow::error::ArrowError> {
    if ids.len() != bm25_scores.len() || ids.len() != texts.len() {
        return Err(arrow::error::ArrowError::InvalidArgumentError(
            "Length mismatch between ids, bm25_scores, and texts".to_string(),
        ));
    }

    let id_array = Int64Array::from(ids.iter().map(|&id| id as i64).collect::<Vec<_>>());

    // Default vectors to zeros
    let flat_vectors = vec![0.0f32; ids.len() * 768];
    let inner_array = Float32Array::from(flat_vectors);
    let vector_array = FixedSizeListArray::new(
        Arc::new(Field::new("item", DataType::Float32, true)),
        768,
        Arc::new(inner_array) as ArrayRef,
        None,
    );

    let bm25_array = Float32Array::from(bm25_scores.to_vec());

    let texts_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
    let text_array = StringArray::from(texts_refs);

    RecordBatch::try_new(
        build_schema(),
        vec![
            Arc::new(id_array) as ArrayRef,
            Arc::new(vector_array) as ArrayRef,
            Arc::new(bm25_array) as ArrayRef,
            Arc::new(text_array) as ArrayRef,
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array1;

    #[test]
    fn test_qdrant_to_record_batch() {
        let ids = vec![1, 2];
        let vectors = vec![
            Array1::from_elem(768, 0.1),
            Array1::from_elem(768, 0.2),
        ];

        let batch = qdrant_to_record_batch(&ids, &vectors).unwrap();
        assert_eq!(batch.num_columns(), 4);
        assert_eq!(batch.num_rows(), 2);
    }

    #[test]
    fn test_postgres_to_record_batch() {
        let ids = vec![1, 2];
        let scores = vec![1.5, 2.5];
        let texts = vec!["text1".to_string(), "text2".to_string()];

        let batch = postgres_to_record_batch(&ids, &scores, &texts).unwrap();
        assert_eq!(batch.num_columns(), 4);
        assert_eq!(batch.num_rows(), 2);
    }
}
