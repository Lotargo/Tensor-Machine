use arrow::ipc::reader::FileReader;
use arrow::ipc::writer::FileWriter;
use arrow::{
    array::{ArrayRef, FixedSizeListArray, Float32Array, Int64Array, ListArray, StringArray},
    datatypes::{DataType, Field, Schema},
    record_batch::RecordBatch,
};
use ndarray::Array1;
use std::sync::Arc;

pub const COLBERT_DIM: usize = 128;

/// Defines the schema of our RecordBatch
/// Columns:
/// - id: Int64
/// - vector: FixedSizeList(Float32, VECTOR_DIM)
/// - bm25_score: Float32
/// - text: Utf8
pub fn build_schema() -> Arc<Schema> {
    let embedding_dim = crate::config::get_embedding_dim();
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new(
            "vector",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                embedding_dim as i32,
            ),
            false,
        ),
        Field::new("bm25_score", DataType::Float32, false),
        Field::new("text", DataType::Utf8, false),
        Field::new(
            "colbert_tokens",
            DataType::List(Arc::new(Field::new(
                "item",
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, true)),
                    COLBERT_DIM as i32,
                ),
                true,
            ))),
            true,
        ),
    ]))
}

/// Converts Qdrant results (ids, vectors) to a RecordBatch.

fn empty_colbert_array(len: usize) -> ArrayRef {
    let offsets = arrow::buffer::OffsetBuffer::<i32>::from_lengths(vec![0; len]);
    let flat_colbert = Float32Array::from(vec![0.0f32; 0]);
    let colbert_fixed = FixedSizeListArray::new(
        Arc::new(Field::new("item", DataType::Float32, true)),
        COLBERT_DIM as i32,
        Arc::new(flat_colbert) as ArrayRef,
        None,
    );
    let colbert_array = ListArray::new(
        Arc::new(Field::new(
            "item",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                COLBERT_DIM as i32,
            ),
            true,
        )),
        offsets,
        Arc::new(colbert_fixed) as ArrayRef,
        None,
    );
    Arc::new(colbert_array) as ArrayRef
}

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

    let embedding_dim = crate::config::get_embedding_dim();
    let mut flat_vectors: Vec<f32> = Vec::with_capacity(ids.len() * embedding_dim);
    for vec in vectors {
        if vec.len() != embedding_dim {
            return Err(arrow::error::ArrowError::InvalidArgumentError(format!(
                "Vector length must be exactly {}",
                embedding_dim
            )));
        }
        flat_vectors.extend(vec.iter().copied());
    }

    let inner_array = Float32Array::from(flat_vectors);

    let vector_array = FixedSizeListArray::new(
        Arc::new(Field::new("item", DataType::Float32, true)),
        embedding_dim as i32,
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
            empty_colbert_array(ids.len()),
        ],
    )
}

pub fn extract_flat_vector_values(batch: &RecordBatch) -> Result<Float32Array, String> {
    let col = batch
        .column_by_name("vector")
        .ok_or("Missing vector column")?;
    let fixed = col
        .as_any()
        .downcast_ref::<FixedSizeListArray>()
        .ok_or("Not FixedSizeList")?;
    let inner = fixed.values();
    let flat = inner
        .as_any()
        .downcast_ref::<Float32Array>()
        .ok_or("Not Float32Array")?;
    Ok(flat.clone())
}

pub fn extract_float_column(batch: &RecordBatch, name: &str) -> Result<Float32Array, String> {
    let col = batch.column_by_name(name).ok_or("Missing column")?;
    let arr = col
        .as_any()
        .downcast_ref::<Float32Array>()
        .ok_or("Not Float32Array")?;
    Ok(arr.clone())
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

    let embedding_dim = crate::config::get_embedding_dim();
    // Default vectors to zeros
    let flat_vectors = vec![0.0f32; ids.len() * embedding_dim];
    let inner_array = Float32Array::from(flat_vectors);
    let vector_array = FixedSizeListArray::new(
        Arc::new(Field::new("item", DataType::Float32, true)),
        embedding_dim as i32,
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
            empty_colbert_array(ids.len()),
        ],
    )
}

pub fn write_batch_to_shm(
    manager: &crate::shared_memory::SharedMemoryManager,
    batch: &RecordBatch,
) -> Result<Arc<crate::shared_memory::MmapRegion>, String> {
    let region_name = format!("batch_{}", uuid::Uuid::new_v4());
    let region = manager.allocate(&region_name)?;

    let file = region.get_file_for_write()?;
    let mut writer = FileWriter::try_new(file, &batch.schema())
        .map_err(|e| format!("Failed to create IPC writer: {}", e))?;

    writer
        .write(batch)
        .map_err(|e| format!("Failed to write batch to IPC: {}", e))?;
    writer
        .finish()
        .map_err(|e| format!("Failed to finish IPC writing: {}", e))?;

    Ok(region)
}

pub fn read_batch_from_shm(
    manager: &crate::shared_memory::SharedMemoryManager,
    region_name: &str,
) -> Result<RecordBatch, String> {
    let region = manager.open(region_name)?;
    let file = region.get_file_for_read()?;

    let mut reader = FileReader::try_new(file, None)
        .map_err(|e| format!("Failed to create IPC reader: {}", e))?;

    let batch = reader
        .next()
        .ok_or_else(|| "No batch found in IPC file".to_string())?
        .map_err(|e| format!("Failed to read batch from IPC: {}", e))?;

    Ok(batch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array1;

    #[test]
    fn test_qdrant_to_record_batch() {
        let dim = crate::config::get_embedding_dim();
        let ids = vec![1, 2];
        let vectors = vec![
            Array1::from_elem(dim, 0.1),
            Array1::from_elem(dim, 0.2),
        ];

        let batch = qdrant_to_record_batch(&ids, &vectors).unwrap();
        assert_eq!(batch.num_columns(), 5);
        assert_eq!(batch.num_rows(), 2);
    }

    #[test]
    fn test_postgres_to_record_batch() {
        let ids = vec![1, 2];
        let scores = vec![1.5, 2.5];
        let texts = vec!["text1".to_string(), "text2".to_string()];

        let batch = postgres_to_record_batch(&ids, &scores, &texts).unwrap();
        assert_eq!(batch.num_columns(), 5);
        assert_eq!(batch.num_rows(), 2);
    }

    #[test]
    fn test_ipc_roundtrip() {
        let dim = crate::config::get_embedding_dim();
        let ids = vec![10, 20];
        let vectors = vec![
            Array1::from_elem(dim, 0.5),
            Array1::from_elem(dim, 0.8),
        ];
        let batch = qdrant_to_record_batch(&ids, &vectors).unwrap();

        let manager = crate::shared_memory::SharedMemoryManager::new();
        let region = write_batch_to_shm(&manager, &batch).expect("Failed to write to shm");

        let batch_read =
            read_batch_from_shm(&manager, &region.name).expect("Failed to read from shm");

        assert_eq!(batch.num_columns(), batch_read.num_columns());
        assert_eq!(batch.num_rows(), batch_read.num_rows());

        let original_flat = extract_flat_vector_values(&batch).unwrap();
        let read_flat = extract_flat_vector_values(&batch_read).unwrap();

        assert_eq!(original_flat.len(), read_flat.len());
        assert_eq!(original_flat.value(0), read_flat.value(0));
        assert_eq!(original_flat.value(dim), read_flat.value(dim));

        region.destroy().unwrap();
    }
}
