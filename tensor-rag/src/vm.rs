use crate::ir::{OpCode, Program};
use arrow::array::Array;
use ndarray::Array1;
use std::sync::Arc;
use tracing::info;

#[derive(Debug, Clone)]
pub enum Value {
    Scalar(f32),
    Tensor1D(Array1<f32>),
    Tensor2D(ndarray::Array2<f32>),
    ArrowArray(Arc<dyn Array>),
    Result(TopKResult),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Scalar(a), Value::Scalar(b)) => a == b,
            (Value::Tensor1D(a), Value::Tensor1D(b)) => a == b,
            (Value::Tensor2D(a), Value::Tensor2D(b)) => a == b,
            (Value::Result(a), Value::Result(b)) => a.indices == b.indices && a.scores == b.scores,
            // Simple reference equality for Arc<dyn Array> for tests,
            // or just false if we don't need strict equality on Arrays
            (Value::ArrowArray(a), Value::ArrowArray(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TopKResult {
    pub indices: Vec<usize>,
    pub scores: Vec<f32>,
}


#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum IpcValue {
    Scalar(f32),
    Tensor1D(Vec<f32>),
    Tensor2D(Vec<Vec<f32>>),
    FlatArray(Vec<f32>),
    Result(TopKResult),
}

impl From<&Value> for IpcValue {
    fn from(value: &Value) -> Self {
        match value {
            Value::Scalar(s) => IpcValue::Scalar(*s),
            Value::Tensor1D(t) => IpcValue::Tensor1D(t.to_vec()),
            Value::Tensor2D(t) => {
                let mut vec2d = Vec::with_capacity(t.nrows());
                for r in 0..t.nrows() {
                    vec2d.push(t.row(r).to_vec());
                }
                IpcValue::Tensor2D(vec2d)
            }
            Value::ArrowArray(arr) => {
                let f32_arr = arr.as_any().downcast_ref::<arrow::array::Float32Array>().unwrap();
                let mut vec = Vec::with_capacity(f32_arr.len());
                for i in 0..f32_arr.len() {
                    vec.push(f32_arr.value(i));
                }
                IpcValue::FlatArray(vec)
            }
            Value::Result(res) => IpcValue::Result(res.clone()),
        }
    }
}

impl From<IpcValue> for Value {
    fn from(ipc: IpcValue) -> Self {
        match ipc {
            IpcValue::Scalar(s) => Value::Scalar(s),
            IpcValue::Tensor1D(v) => Value::Tensor1D(Array1::from(v)),
            IpcValue::Tensor2D(v) => {
                let rows = v.len();
                let cols = if rows > 0 { v[0].len() } else { 0 };
                let flat: Vec<f32> = v.into_iter().flatten().collect();
                Value::Tensor2D(ndarray::Array2::from_shape_vec((rows, cols), flat).unwrap())
            }
            IpcValue::FlatArray(v) => Value::ArrowArray(Arc::new(arrow::array::Float32Array::from(v))),
            IpcValue::Result(res) => Value::Result(res),
        }
    }
}




#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum VmRequest {
    Execute {
        session_id: String,
        sequence: u64,
        shm_name: String,
    },
    Shutdown,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum VmResponse {
    Result {
        session_id: String,
        sequence: u64,
        shm_name: String,
    },
    Error {
        session_id: String,
        sequence: u64,
        message: String,
    },
    ShutdownAck,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct VmExecutionPayload {
    pub program: Program,
    pub query_vector: Vec<f32>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct VmResultPayload {
    pub value: Option<IpcValue>,
}

pub struct VM {
    pub stack: Vec<Value>,
    pub batch: Option<Arc<arrow::record_batch::RecordBatch>>,
    pub query_vector: Array1<f32>,
    pub gpu_context: Option<Arc<crate::gpu::GpuContext>>,
}

impl Default for VM {
    fn default() -> Self {
        Self::new()
    }
}

impl VM {
    pub fn new() -> Self {
        Self {
            stack: Vec::new(),
            batch: None,
            query_vector: Array1::zeros(768), // Default initialization, could be populated externally
            gpu_context: None,
        }
    }

    pub async fn init_gpu(&mut self) -> Result<(), String> {
        if let Some(ctx) = crate::gpu::GpuContext::new().await {
            self.gpu_context = Some(Arc::new(ctx));
        }
        Ok(())
    }

    /// Executes a single pure math operation.
    pub fn step(&mut self, op: &OpCode) -> Result<Option<Value>, String> {
        match op {
            OpCode::LoadConst(v) => {
                self.stack.push(Value::Scalar(*v));
            }
            OpCode::Add => {
                let b = self.stack.pop().ok_or("Stack underflow")?;
                let a = self.stack.pop().ok_or("Stack underflow")?;
                match (a, b) {
                    (Value::Tensor1D(t1), Value::Tensor1D(t2)) => {
                        if let Some(gpu) = &self.gpu_context {
                            if let Some(res) = gpu.add(t1.as_standard_layout().as_slice().unwrap(), t2.as_standard_layout().as_slice().unwrap()) {
                                self.stack.push(Value::Tensor1D(Array1::from(res)));
                                return Ok(None);
                            }
                        }
                        self.stack.push(Value::Tensor1D(t1 + t2));
                    }
                    (Value::ArrowArray(arr1), Value::ArrowArray(arr2)) => {
                        if let Some(gpu) = &self.gpu_context {
                            let t1_f32 = arr1
                                .as_any()
                                .downcast_ref::<arrow::array::Float32Array>()
                                .ok_or("Not Float32Array")?;
                            let t2_f32 = arr2
                                .as_any()
                                .downcast_ref::<arrow::array::Float32Array>()
                                .ok_or("Not Float32Array")?;

                            let s1: Vec<f32> = t1_f32.iter().map(|v| v.unwrap_or(0.0)).collect();
                            let s2: Vec<f32> = t2_f32.iter().map(|v| v.unwrap_or(0.0)).collect();

                            if let Some(res) = gpu.add(&s1, &s2) {
                                self.stack.push(Value::ArrowArray(Arc::new(arrow::array::Float32Array::from(res))));
                                return Ok(None);
                            }
                        }

                        let result = arrow::compute::kernels::numeric::add(
                            &arr1 as &dyn arrow::array::Datum,
                            &arr2 as &dyn arrow::array::Datum,
                        )
                        .map_err(|e| e.to_string())?;
                        self.stack.push(Value::ArrowArray(Arc::new(result)));
                    }
                    _ => return Err("Invalid types for Add".into()),
                }
            }
            OpCode::Mul => {
                let b = self.stack.pop().ok_or("Stack underflow")?;
                let a = self.stack.pop().ok_or("Stack underflow")?;
                match (a, b) {
                    (Value::Tensor1D(t), Value::Scalar(s)) => {
                        if let Some(gpu) = &self.gpu_context {
                            if let Some(res) = gpu.mul(t.as_standard_layout().as_slice().unwrap(), s) {
                                self.stack.push(Value::Tensor1D(Array1::from(res)));
                                return Ok(None);
                            }
                        }
                        self.stack.push(Value::Tensor1D(t * s));
                    }
                    (Value::ArrowArray(arr), Value::Scalar(s)) => {
                        if let Some(gpu) = &self.gpu_context {
                            let t_f32 = arr
                                .as_any()
                                .downcast_ref::<arrow::array::Float32Array>()
                                .ok_or("Not Float32Array")?;

                            let s_vec: Vec<f32> = t_f32.iter().map(|v| v.unwrap_or(0.0)).collect();
                            if let Some(res) = gpu.mul(&s_vec, s) {
                                self.stack.push(Value::ArrowArray(Arc::new(arrow::array::Float32Array::from(res))));
                                return Ok(None);
                            }
                        }

                        let s_arr = arrow::array::Float32Array::from(vec![s; arr.len()]);
                        let result = arrow::compute::kernels::numeric::mul(
                            &arr as &dyn arrow::array::Datum,
                            &s_arr as &dyn arrow::array::Datum,
                        )
                        .map_err(|e| e.to_string())?;
                        self.stack.push(Value::ArrowArray(Arc::new(result)));
                    }
                    _ => return Err("Invalid types for Mul".into()),
                }
            }
            OpCode::Fusion => {
                let w2 = self.stack.pop().ok_or("Stack underflow")?;
                let w1 = self.stack.pop().ok_or("Stack underflow")?;
                let b = self.stack.pop().ok_or("Stack underflow")?;
                let a = self.stack.pop().ok_or("Stack underflow")?;

                match (a, b, w1, w2) {
                    (
                        Value::Tensor1D(t1),
                        Value::Tensor1D(t2),
                        Value::Scalar(sw1),
                        Value::Scalar(sw2),
                    ) => {
                        if let Some(gpu) = &self.gpu_context {
                            if let Some(res) = gpu.fusion(t1.as_standard_layout().as_slice().unwrap(), t2.as_standard_layout().as_slice().unwrap(), sw1, sw2) {
                                self.stack.push(Value::Tensor1D(Array1::from(res)));
                                return Ok(None);
                            }
                        }
                        let res = (&t1 * sw1) + (&t2 * sw2);
                        self.stack.push(Value::Tensor1D(res));
                    }
                    (
                        Value::ArrowArray(arr1),
                        Value::ArrowArray(arr2),
                        Value::Scalar(sw1),
                        Value::Scalar(sw2),
                    ) => {
                        if let Some(gpu) = &self.gpu_context {
                            let t1_f32 = arr1.as_any().downcast_ref::<arrow::array::Float32Array>().ok_or("Not Float32Array")?;
                            let t2_f32 = arr2.as_any().downcast_ref::<arrow::array::Float32Array>().ok_or("Not Float32Array")?;

                            let s1: Vec<f32> = t1_f32.iter().map(|v| v.unwrap_or(0.0)).collect();
                            let s2: Vec<f32> = t2_f32.iter().map(|v| v.unwrap_or(0.0)).collect();

                            if let Some(res) = gpu.fusion(&s1, &s2, sw1, sw2) {
                                self.stack.push(Value::ArrowArray(Arc::new(arrow::array::Float32Array::from(res))));
                                return Ok(None);
                            }
                        }

                        let sw1_arr = arrow::array::Float32Array::from(vec![sw1; arr1.len()]);
                        let sw2_arr = arrow::array::Float32Array::from(vec![sw2; arr2.len()]);

                        let w1_res = arrow::compute::kernels::numeric::mul(
                            &arr1 as &dyn arrow::array::Datum,
                            &sw1_arr as &dyn arrow::array::Datum,
                        )
                        .map_err(|e| e.to_string())?;

                        let w2_res = arrow::compute::kernels::numeric::mul(
                            &arr2 as &dyn arrow::array::Datum,
                            &sw2_arr as &dyn arrow::array::Datum,
                        )
                        .map_err(|e| e.to_string())?;

                        let result = arrow::compute::kernels::numeric::add(
                            &w1_res as &dyn arrow::array::Datum,
                            &w2_res as &dyn arrow::array::Datum,
                        )
                        .map_err(|e| e.to_string())?;

                        self.stack.push(Value::ArrowArray(Arc::new(result)));
                    }
                    _ => return Err("Invalid types for Fusion".into()),
                }
            }
            OpCode::Div => {
                let b = self.stack.pop().ok_or("Stack underflow")?;
                let a = self.stack.pop().ok_or("Stack underflow")?;
                match (a, b) {
                    (Value::Tensor1D(t1), Value::Tensor1D(t2)) => {
                        self.stack.push(Value::Tensor1D(t1 / t2));
                    }
                    (Value::ArrowArray(arr1), Value::ArrowArray(arr2)) => {
                        let result = arrow::compute::kernels::numeric::div(
                            &arr1 as &dyn arrow::array::Datum,
                            &arr2 as &dyn arrow::array::Datum,
                        )
                        .map_err(|e| e.to_string())?;
                        self.stack.push(Value::ArrowArray(Arc::new(result)));
                    }
                    _ => return Err("Invalid types for Div".into()),
                }
            }
            OpCode::Threshold(thresh) => {
                let a = self.stack.pop().ok_or("Stack underflow")?;
                match a {
                    Value::Tensor1D(t) => {
                        let t_mapped = t.mapv(|x| if x < *thresh { 0.0 } else { x });
                        self.stack.push(Value::Tensor1D(t_mapped));
                    }
                    Value::ArrowArray(arr) => {
                        let t_f32 = arr
                            .as_any()
                            .downcast_ref::<arrow::array::Float32Array>()
                            .ok_or("Not Float32Array")?;
                        let vec: Vec<f32> = t_f32
                            .iter()
                            .map(|v| {
                                let val = v.unwrap_or(0.0);
                                if val < *thresh {
                                    0.0
                                } else {
                                    val
                                }
                            })
                            .collect();
                        self.stack.push(Value::ArrowArray(Arc::new(
                            arrow::array::Float32Array::from(vec),
                        )));
                    }
                    _ => return Err("Invalid types for Threshold".into()),
                }
            }
            OpCode::Sort => {
                let a = self.stack.pop().ok_or("Stack underflow")?;
                match a {
                    Value::Tensor1D(t) => {
                        let mut vec = t.into_raw_vec_and_offset().0;
                        vec.sort_by(|x, y| y.partial_cmp(x).unwrap_or(std::cmp::Ordering::Equal));
                        self.stack.push(Value::Tensor1D(Array1::from(vec)));
                    }
                    Value::ArrowArray(arr) => {
                        let t_f32 = arr
                            .as_any()
                            .downcast_ref::<arrow::array::Float32Array>()
                            .ok_or("Not Float32Array")?;
                        let mut vec: Vec<f32> = t_f32.iter().map(|v| v.unwrap_or(0.0)).collect();
                        vec.sort_by(|x, y| y.partial_cmp(x).unwrap_or(std::cmp::Ordering::Equal));
                        self.stack.push(Value::ArrowArray(Arc::new(
                            arrow::array::Float32Array::from(vec),
                        )));
                    }
                    _ => return Err("Invalid types for Sort".into()),
                }
            }
            OpCode::Slice(start, end) => {
                let a = self.stack.pop().ok_or("Stack underflow")?;
                match a {
                    Value::Tensor1D(t) => {
                        let slice = t.slice(ndarray::s![*start..*end]).to_owned();
                        self.stack.push(Value::Tensor1D(slice));
                    }
                    Value::ArrowArray(arr) => {
                        let sliced = arr.slice(*start, *end - *start);
                        self.stack.push(Value::ArrowArray(Arc::new(sliced)));
                    }
                    _ => return Err("Invalid types for Slice".into()),
                }
            }
            OpCode::Matmul => {
                let b = self.stack.pop().ok_or("Stack underflow")?;
                let a = self.stack.pop().ok_or("Stack underflow")?;
                match (a, b) {
                    (Value::Tensor1D(t1), Value::Tensor1D(t2)) => {
                        if let Some(gpu) = &self.gpu_context {
                            if let Some(res) = gpu.matmul(t1.as_standard_layout().as_slice().unwrap(), t2.as_standard_layout().as_slice().unwrap()) {
                                self.stack.push(Value::Scalar(res));
                                return Ok(None);
                            }
                        }
                        let res = t1.dot(&t2);
                        self.stack.push(Value::Scalar(res));
                    }
                    // For Arrow we could do dot product similarly, mocked here for brevity
                    (Value::ArrowArray(arr1), Value::ArrowArray(arr2)) => {
                        let t1_f32 = arr1
                            .as_any()
                            .downcast_ref::<arrow::array::Float32Array>()
                            .ok_or("Not Float32Array")?;
                        let t2_f32 = arr2
                            .as_any()
                            .downcast_ref::<arrow::array::Float32Array>()
                            .ok_or("Not Float32Array")?;

                        if let Some(gpu) = &self.gpu_context {
                            let s1: Vec<f32> = t1_f32.iter().map(|v| v.unwrap_or(0.0)).collect();
                            let s2: Vec<f32> = t2_f32.iter().map(|v| v.unwrap_or(0.0)).collect();
                            if let Some(res) = gpu.matmul(&s1, &s2) {
                                self.stack.push(Value::Scalar(res));
                                return Ok(None);
                            }
                        }

                        let mut sum = 0.0;
                        for (v1, v2) in t1_f32.iter().zip(t2_f32.iter()) {
                            sum += v1.unwrap_or(0.0) * v2.unwrap_or(0.0);
                        }
                        self.stack.push(Value::Scalar(sum));
                    }
                    _ => return Err("Invalid types for Matmul".into()),
                }
            }
            OpCode::TopK(k) => {
                let a = self.stack.pop().ok_or("Stack underflow")?;

                let (indices, scores) = match a {
                    Value::Tensor1D(t) => {
                        if let Some(gpu) = &self.gpu_context {
                            if let Some(res) = gpu.topk(t.as_standard_layout().as_slice().unwrap(), *k) {
                                self.stack.push(Value::Result(TopKResult { indices: res.0, scores: res.1 }));
                                return Ok(None);
                            }
                        }
                        let mut indexed: Vec<(usize, f32)> = t.into_iter().enumerate().collect();
                        indexed.sort_by(|x, y| {
                            y.1.partial_cmp(&x.1).unwrap_or(std::cmp::Ordering::Equal)
                        });
                        let top = indexed.into_iter().take(*k).collect::<Vec<_>>();
                        let indices = top.iter().map(|(i, _)| *i).collect();
                        let scores = top.iter().map(|(_, s)| *s).collect();
                        (indices, scores)
                    }
                    Value::ArrowArray(arr) => {
                        let t_f32 = arr
                            .as_any()
                            .downcast_ref::<arrow::array::Float32Array>()
                            .ok_or("Not Float32Array")?;

                        if let Some(gpu) = &self.gpu_context {
                            let s: Vec<f32> = t_f32.iter().map(|v| v.unwrap_or(f32::MIN)).collect();
                            if let Some(res) = gpu.topk(&s, *k) {
                                self.stack.push(Value::Result(TopKResult { indices: res.0, scores: res.1 }));
                                return Ok(None);
                            }
                        }

                        let mut indexed: Vec<(usize, f32)> = t_f32
                            .iter()
                            .enumerate()
                            .map(|(i, v)| (i, v.unwrap_or(0.0)))
                            .collect();
                        indexed.sort_by(|x, y| {
                            y.1.partial_cmp(&x.1).unwrap_or(std::cmp::Ordering::Equal)
                        });
                        let top = indexed.into_iter().take(*k).collect::<Vec<_>>();
                        let indices = top.iter().map(|(i, _)| *i).collect();
                        let scores = top.iter().map(|(_, s)| *s).collect();
                        (indices, scores)
                    }
                    _ => return Err("Invalid type for TopK".into()),
                };

                self.stack.push(Value::Result(TopKResult { indices, scores }));
            }
            OpCode::Mmr(diversity) => {
                let a = self.stack.pop().ok_or("Stack underflow")?;

                // Synthetic MMR logic for demonstration
                let (indices, scores) = match a {
                    Value::ArrowArray(arr) => {
                        let t_f32 = arr
                            .as_any()
                            .downcast_ref::<arrow::array::Float32Array>()
                            .ok_or("Not Float32Array")?;
                        let mut indexed: Vec<(usize, f32)> = t_f32
                            .iter()
                            .enumerate()
                            .map(|(i, v)| (i, v.unwrap_or(0.0) * (1.0 - diversity)))
                            .collect();
                        indexed.sort_by(|x, y| {
                            y.1.partial_cmp(&x.1).unwrap_or(std::cmp::Ordering::Equal)
                        });
                        let top = indexed.into_iter().take(10).collect::<Vec<_>>(); // Fixed top K for MMR mock
                        let indices = top.iter().map(|(i, _)| *i).collect();
                        let scores = top.iter().map(|(_, s)| *s).collect();
                        (indices, scores)
                    }
                    _ => return Err("Invalid type for MMR".into()),
                };
                self.stack.push(Value::Result(TopKResult { indices, scores }));
            }
            OpCode::Rrf(k) => {
                let _b = self.stack.pop().ok_or("Stack underflow")?;
                let _a = self.stack.pop().ok_or("Stack underflow")?;

                // Synthetic RRF mock returning fixed values for testing
                let indices = vec![0, 1, 2];
                let scores = vec![
                    1.0 / (60.0 + *k as f32),
                    1.0 / (61.0 + *k as f32),
                    1.0 / (62.0 + *k as f32),
                ];
                self.stack.push(Value::Result(TopKResult { indices, scores }));
            }
            OpCode::CallDynamic(name, _args) => {
                if name == "mock_dynamic" {
                    let indices = vec![100];
                    let scores = vec![0.99];
                    self.stack.push(Value::Result(TopKResult { indices, scores }));
                } else {
                    return Err(format!("Unknown dynamic strategy: {}", name));
                }
            }
            OpCode::Emit => {
                let res = self.stack.pop().ok_or("Stack underflow")?;
                return Ok(Some(res));
            }
            OpCode::ComputeSemantic => {
                let batch = self
                    .batch
                    .as_ref()
                    .ok_or("No batch found for ComputeSemantic")?;
                let vec_col = batch
                    .column_by_name("vector")
                    .ok_or("Missing vector column")?;

                // Assume vectors are List<Float32>
                let list_arr = vec_col
                    .as_any()
                    .downcast_ref::<arrow::array::FixedSizeListArray>()
                    .ok_or("Not FixedSizeListArray")?;

                let values = list_arr.values();
                let f32_values = values
                    .as_any()
                    .downcast_ref::<arrow::array::Float32Array>()
                    .ok_or("Not Float32Array in list")?;

                let dim = 768; // Based on setup
                let mut scores = Vec::with_capacity(list_arr.len());

                let query_slice = self.query_vector.as_slice().unwrap();

                for i in 0..list_arr.len() {
                    let start = i * dim;
                    let end = start + dim;
                    let vec_slice = &f32_values.values()[start..end];
                    let dot: f32 = query_slice
                        .iter()
                        .zip(vec_slice.iter())
                        .map(|(a, b)| a * b)
                        .sum();
                    scores.push(dot);
                }

                self.stack.push(Value::ArrowArray(Arc::new(
                    arrow::array::Float32Array::from(scores),
                )));
            }
            OpCode::LoadColbertQuery(_) => {
                // Mock creating a 10x128 query token embedding matrix
                use ndarray::Array2;
                let query_tokens = Array2::from_elem((10, crate::arrow::COLBERT_DIM), 0.1f32);
                self.stack.push(Value::Tensor2D(query_tokens));
            }
            OpCode::ColbertMaxSim => {
                let query_val = self.stack.pop().ok_or("Stack underflow")?;
                let query_tokens = match query_val {
                    Value::Tensor2D(t) => t,
                    _ => return Err("Expected Tensor2D for ColBERT query".to_string()),
                };

                // Clean up previous scores from docs plan
                self.stack.pop();

                // Get doc tokens from batch
                let batch = self.batch.as_ref().ok_or("No record batch loaded")?;
                let colbert_col = batch.column_by_name("colbert_tokens").ok_or("Missing colbert_tokens column")?;

                let list_arr = colbert_col.as_any().downcast_ref::<arrow::array::ListArray>().ok_or("colbert_tokens is not a ListArray")?;
                let num_docs = list_arr.len();

                if let Some(gpu) = &self.gpu_context {
                    // Collect all doc tokens into a single flat buffer and build offsets
                    let mut all_doc_tokens = Vec::new();
                    let mut colbert_offsets = Vec::new();

                    for i in 0..num_docs {
                         if list_arr.is_null(i) {
                             colbert_offsets.push(crate::gpu::ColbertOffsets {
                                 doc_start: (all_doc_tokens.len() / crate::arrow::COLBERT_DIM) as u32,
                                 num_tokens: 0,
                             });
                             continue;
                         }
                         let doc_arr = list_arr.value(i);
                         let doc_fixed = doc_arr.as_any().downcast_ref::<arrow::array::FixedSizeListArray>().ok_or("Not FixedSizeListArray")?;
                         let doc_inner = doc_fixed.values();
                         let doc_flat = doc_inner.as_any().downcast_ref::<arrow::array::Float32Array>().ok_or("Not Float32Array")?;
                         let offset = doc_fixed.value_offset(0) as usize;
                         let len = doc_fixed.len() * crate::arrow::COLBERT_DIM;
                         colbert_offsets.push(crate::gpu::ColbertOffsets {
                             doc_start: (all_doc_tokens.len() / crate::arrow::COLBERT_DIM) as u32,
                             num_tokens: doc_fixed.len() as u32,
                         });
                         all_doc_tokens.extend_from_slice(&doc_flat.values()[offset..offset+len]);
                    }

                    if let Some(res) = gpu.colbert_maxsim(
                        query_tokens.as_standard_layout().as_slice().unwrap(),
                        &all_doc_tokens,
                        num_docs as u32,
                        query_tokens.shape()[0] as u32,
                        &colbert_offsets,
                        crate::arrow::COLBERT_DIM as u32
                    ) {
                        self.stack.push(Value::Tensor1D(ndarray::Array1::from(res)));
                        return Ok(None);
                    }
                }

                let mut maxsim_scores = ndarray::Array1::<f32>::zeros(num_docs);

                for doc_idx in 0..num_docs {
                    if list_arr.is_null(doc_idx) {
                        continue;
                    }
                    let doc_arr = list_arr.value(doc_idx);
                    let doc_fixed = doc_arr.as_any().downcast_ref::<arrow::array::FixedSizeListArray>().ok_or("Not FixedSizeListArray")?;
                    let doc_inner = doc_fixed.values();
                    let doc_flat = doc_inner.as_any().downcast_ref::<arrow::array::Float32Array>().ok_or("Not Float32Array")?;
                    let offset = doc_fixed.value_offset(0) as usize; // Get offset for first token in this document

                    let num_doc_tokens = doc_fixed.len();
                    if num_doc_tokens == 0 {
                        continue;
                    }

                    // SIMD-optimized dot product using ndarray
                    // Extract the document's flat slice
                    let doc_slice_start = offset;
                    let doc_slice_end = offset + num_doc_tokens * crate::arrow::COLBERT_DIM;
                    let doc_slice = &doc_flat.values()[doc_slice_start..doc_slice_end];

                    // Create a 2D view: [num_doc_tokens, COLBERT_DIM]
                    let doc_view = ndarray::ArrayView2::from_shape(
                        (num_doc_tokens, crate::arrow::COLBERT_DIM),
                        doc_slice
                    ).unwrap();

                    // Compute sim matrix: query_tokens @ doc_view.T -> shape [num_query_tokens, num_doc_tokens]
                    // query_tokens shape is [num_query_tokens, COLBERT_DIM]
                    let sim_matrix = query_tokens.dot(&doc_view.t());

                    // Max pooling over the document tokens (axis 1), then sum over query tokens (axis 0)
                    let maxsim_per_query = sim_matrix.fold_axis(ndarray::Axis(1), f32::NEG_INFINITY, |&max, &x| {
                        if x > max { x } else { max }
                    });

                    let doc_score = maxsim_per_query.sum();
                    maxsim_scores[doc_idx] = doc_score;
                }

                self.stack.push(Value::Tensor1D(maxsim_scores));
            }
            OpCode::RetrieveQdrant { .. }
            | OpCode::RetrievePostgres
            | OpCode::IfConfidence(..) => {
                return Err(
                    "I/O operations and branching must be handled by the orchestrator".into(),
                );
            }
        }
        Ok(None)
    }

    /// Backwards compatibility run function for tests
    pub fn run(&mut self, program: Program, instruction_count: &mut usize, max_instructions: usize) -> Result<Option<Value>, String> {
        let _span = tracing::info_span!("vm_run", instructions = program.instructions.len()).entered();
        let start = std::time::Instant::now();
        let total = program.instructions.len();

        for instruction in program.instructions {
            if *instruction_count >= max_instructions {
                return Err(format!("Execution error: Instruction limit exceeded ({})", max_instructions));
            }
            *instruction_count += 1;

            match instruction.op {
                OpCode::RetrieveQdrant { .. } | OpCode::RetrievePostgres | OpCode::IfConfidence(..) => {
                    return Err("I/O operations and branching must be handled by the orchestrator".into());
                }
                _ => {
                    if let Some(res) = self.step(&instruction.op)? {
                        let elapsed = start.elapsed();
                        info!(elapsed = ?elapsed, total_instructions = total, "VM program completed");
                        return Ok(Some(res));
                    }
                }
            }
        }

        let elapsed = start.elapsed();
        info!(elapsed = ?elapsed, total_instructions = total, "VM program completed");
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_const() {
        let mut vm = VM::new();
        let mut program = Program::new();
        program.push(OpCode::LoadConst(42.0));
        program.push(OpCode::Emit);

        let res = vm.run(program, &mut 0, 100_000).unwrap().unwrap();
        assert_eq!(res, Value::Scalar(42.0));
    }

    #[test]
    fn test_add() {
        let mut vm = VM::new();
        vm.stack.push(Value::ArrowArray(Arc::new(
            arrow::array::Float32Array::from(vec![1.0, 2.0, 3.0]),
        )));
        vm.stack.push(Value::ArrowArray(Arc::new(
            arrow::array::Float32Array::from(vec![4.0, 5.0, 6.0]),
        )));

        let mut program = Program::new();
        program.push(OpCode::Add);
        program.push(OpCode::Emit);

        let res = vm.run(program, &mut 0, 100_000).unwrap().unwrap();
        if let Value::ArrowArray(arr) = res {
            let t_f32 = arr
                .as_any()
                .downcast_ref::<arrow::array::Float32Array>()
                .unwrap();
            let result_vec: Vec<f32> = t_f32.iter().map(|v| v.unwrap()).collect();
            assert_eq!(result_vec, vec![5.0, 7.0, 9.0]);
        } else {
            panic!("Expected ArrowArray");
        }
    }

    #[test]
    fn test_mul() {
        let mut vm = VM::new();
        vm.stack.push(Value::ArrowArray(Arc::new(
            arrow::array::Float32Array::from(vec![1.0, 2.0, 3.0]),
        )));
        vm.stack.push(Value::Scalar(2.0));

        let mut program = Program::new();
        program.push(OpCode::Mul);
        program.push(OpCode::Emit);

        let res = vm.run(program, &mut 0, 100_000).unwrap().unwrap();
        if let Value::ArrowArray(arr) = res {
            let t_f32 = arr
                .as_any()
                .downcast_ref::<arrow::array::Float32Array>()
                .unwrap();
            let result_vec: Vec<f32> = t_f32.iter().map(|v| v.unwrap()).collect();
            assert_eq!(result_vec, vec![2.0, 4.0, 6.0]);
        } else {
            panic!("Expected ArrowArray");
        }
    }

    #[test]
    fn test_fusion() {
        let mut vm = VM::new();
        // Stack order: t1, t2, w1, w2
        vm.stack.push(Value::ArrowArray(Arc::new(
            arrow::array::Float32Array::from(vec![1.0, 1.0, 1.0]),
        ))); // a (t1)
        vm.stack.push(Value::ArrowArray(Arc::new(
            arrow::array::Float32Array::from(vec![2.0, 2.0, 2.0]),
        ))); // b (t2)
        vm.stack.push(Value::Scalar(0.5)); // a_weight (w1)
        vm.stack.push(Value::Scalar(0.5)); // b_weight (w2)

        let mut program = Program::new();
        program.push(OpCode::Fusion);
        program.push(OpCode::Emit);

        let res = vm.run(program, &mut 0, 100_000).unwrap().unwrap();
        if let Value::ArrowArray(arr) = res {
            let t_f32 = arr
                .as_any()
                .downcast_ref::<arrow::array::Float32Array>()
                .unwrap();
            let result_vec: Vec<f32> = t_f32.iter().map(|v| v.unwrap()).collect();
            assert_eq!(result_vec, vec![1.5, 1.5, 1.5]); // 1*0.5 + 2*0.5 = 1.5
        } else {
            panic!("Expected ArrowArray");
        }
    }

    #[test]
    fn test_topk() {
        let mut vm = VM::new();
        vm.stack.push(Value::ArrowArray(Arc::new(
            arrow::array::Float32Array::from(vec![1.0, 5.0, 3.0, 9.0, 2.0]),
        )));

        let mut program = Program::new();
        program.push(OpCode::TopK(3));
        program.push(OpCode::Emit);

        let res = vm.run(program, &mut 0, 100_000).unwrap().unwrap();
        if let Value::Result(topk) = res {
            assert_eq!(topk.indices, vec![3, 1, 2]);
            assert_eq!(topk.scores, vec![9.0, 5.0, 3.0]);
        } else {
            panic!("Expected TopKResult");
        }
    }
}

#[cfg(test)]
mod additional_tests {
    use super::*;
    use ndarray::Array1;

    #[test]
    fn test_div() {
        let mut vm = VM::new();
        vm.stack.push(Value::ArrowArray(Arc::new(
            arrow::array::Float32Array::from(vec![10.0, 20.0, 30.0]),
        )));
        vm.stack.push(Value::ArrowArray(Arc::new(
            arrow::array::Float32Array::from(vec![2.0, 5.0, 3.0]),
        )));

        let mut program = Program::new();
        program.push(OpCode::Div);
        program.push(OpCode::Emit);

        let res = vm.run(program, &mut 0, 100_000).unwrap().unwrap();
        if let Value::ArrowArray(arr) = res {
            let t_f32 = arr
                .as_any()
                .downcast_ref::<arrow::array::Float32Array>()
                .unwrap();
            let result_vec: Vec<f32> = t_f32.iter().map(|v| v.unwrap()).collect();
            assert_eq!(result_vec, vec![5.0, 4.0, 10.0]);
        } else {
            panic!("Expected ArrowArray");
        }
    }

    #[test]
    fn test_threshold() {
        let mut vm = VM::new();
        vm.stack.push(Value::ArrowArray(Arc::new(
            arrow::array::Float32Array::from(vec![0.1, 0.5, 0.9, 0.2]),
        )));

        let mut program = Program::new();
        program.push(OpCode::Threshold(0.5));
        program.push(OpCode::Emit);

        let res = vm.run(program, &mut 0, 100_000).unwrap().unwrap();
        if let Value::ArrowArray(arr) = res {
            let t_f32 = arr
                .as_any()
                .downcast_ref::<arrow::array::Float32Array>()
                .unwrap();
            let result_vec: Vec<f32> = t_f32.iter().map(|v| v.unwrap()).collect();
            assert_eq!(result_vec, vec![0.0, 0.5, 0.9, 0.0]);
        } else {
            panic!("Expected ArrowArray");
        }
    }

    #[test]
    fn test_sort() {
        let mut vm = VM::new();
        vm.stack.push(Value::ArrowArray(Arc::new(
            arrow::array::Float32Array::from(vec![0.1, 0.9, 0.5, 0.2]),
        )));

        let mut program = Program::new();
        program.push(OpCode::Sort);
        program.push(OpCode::Emit);

        let res = vm.run(program, &mut 0, 100_000).unwrap().unwrap();
        if let Value::ArrowArray(arr) = res {
            let t_f32 = arr
                .as_any()
                .downcast_ref::<arrow::array::Float32Array>()
                .unwrap();
            let result_vec: Vec<f32> = t_f32.iter().map(|v| v.unwrap()).collect();
            assert_eq!(result_vec, vec![0.9, 0.5, 0.2, 0.1]);
        } else {
            panic!("Expected ArrowArray");
        }
    }

    #[test]
    fn test_slice() {
        let mut vm = VM::new();
        vm.stack.push(Value::ArrowArray(Arc::new(
            arrow::array::Float32Array::from(vec![1.0, 2.0, 3.0, 4.0, 5.0]),
        )));

        let mut program = Program::new();
        program.push(OpCode::Slice(1, 4));
        program.push(OpCode::Emit);

        let res = vm.run(program, &mut 0, 100_000).unwrap().unwrap();
        if let Value::ArrowArray(arr) = res {
            let t_f32 = arr
                .as_any()
                .downcast_ref::<arrow::array::Float32Array>()
                .unwrap();
            let result_vec: Vec<f32> = t_f32.iter().map(|v| v.unwrap()).collect();
            assert_eq!(result_vec, vec![2.0, 3.0, 4.0]);
        } else {
            panic!("Expected ArrowArray");
        }
    }

    #[test]
    fn test_matmul_1d_dot_product() {
        let mut vm = VM::new();
        vm.stack
            .push(Value::Tensor1D(Array1::from(vec![1.0, 2.0, 3.0])));
        vm.stack
            .push(Value::Tensor1D(Array1::from(vec![4.0, 5.0, 6.0])));

        let mut program = Program::new();
        program.push(OpCode::Matmul);
        program.push(OpCode::Emit);

        let res = vm.run(program, &mut 0, 100_000).unwrap().unwrap();
        if let Value::Scalar(s) = res {
            assert_eq!(s, 32.0); // 1*4 + 2*5 + 3*6 = 4 + 10 + 18 = 32
        } else {
            panic!("Expected Scalar");
        }
    }

    #[test]
    fn test_instruction_limit() {
        let mut vm = VM::new();
        let mut program = Program::new();
        for _ in 0..10 {
            program.push(OpCode::LoadConst(42.0));
        }

        let mut instruction_count = 0;
        let max_instructions = 5;

        let res = vm.run(program, &mut instruction_count, max_instructions);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), "Execution error: Instruction limit exceeded (5)");
        assert_eq!(instruction_count, 5);
    }
}

#[cfg(test)]
mod io_rejection_tests {
    use super::*;

    #[test]
    fn test_vm_run_rejects_io_opcodes() {
        let mut vm = VM::new();

        let mut program = Program::new();
        program.push(OpCode::RetrievePostgres);

        let res = vm.run(program, &mut 0, 10);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), "I/O operations and branching must be handled by the orchestrator");
    }
}
