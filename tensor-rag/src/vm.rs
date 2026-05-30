use ndarray::Array1;
use arrow::array::ArrayRef;
use std::sync::Arc;
use crate::ir::{Program, OpCode};

#[derive(Debug, Clone, PartialEq)]
pub struct TopKResult {
    pub indices: Vec<usize>,
    pub scores: Vec<f32>,
}

#[derive(Debug, Clone)]
pub enum Value {
    Scalar(f32),
    Tensor1D(Array1<f32>),
    ArrowArray(ArrayRef),
    Result(TopKResult),
}

impl std::cmp::PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Scalar(a), Value::Scalar(b)) => a == b,
            (Value::Tensor1D(a), Value::Tensor1D(b)) => a == b,
            (Value::Result(a), Value::Result(b)) => a == b,
            (Value::ArrowArray(a), Value::ArrowArray(b)) => {
                if Arc::ptr_eq(a, b) {
                    return true;
                }
                let a_f32 = a.as_any().downcast_ref::<arrow::array::Float32Array>();
                let b_f32 = b.as_any().downcast_ref::<arrow::array::Float32Array>();
                match (a_f32, b_f32) {
                    (Some(a_arr), Some(b_arr)) => {
                        if a_arr.len() != b_arr.len() {
                            return false;
                        }
                        a_arr.iter().zip(b_arr.iter()).all(|(va, vb)| va == vb)
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }
}

use crate::arrow::qdrant_to_record_batch;
use arrow::record_batch::RecordBatch;

pub struct VM {
    pub stack: Vec<Value>,
    pub batch: Option<Arc<RecordBatch>>,
    pub query_vector: Array1<f32>,
}

impl VM {
    pub fn new() -> Self {
        Self {
            stack: Vec::new(),
            batch: None,
            query_vector: Array1::zeros(768), // Default initialization, could be populated externally
        }
    }

    pub fn run(&mut self, program: Program) -> Result<Option<Value>, String> {
        for instruction in program.instructions {
            match instruction.op {
                OpCode::LoadConst(val) => {
                    self.stack.push(Value::Scalar(val));
                }
                OpCode::Add => {
                    let b = self.stack.pop().ok_or("Stack underflow")?;
                    let a = self.stack.pop().ok_or("Stack underflow")?;

                    match (a, b) {
                        (Value::Tensor1D(t1), Value::Tensor1D(t2)) => {
                            self.stack.push(Value::Tensor1D(t1 + t2));
                        }
                        (Value::ArrowArray(a_arr), Value::ArrowArray(b_arr)) => {
                            let a_f32 = a_arr.as_any().downcast_ref::<arrow::array::Float32Array>().ok_or("Not Float32Array")?;
                            let b_f32 = b_arr.as_any().downcast_ref::<arrow::array::Float32Array>().ok_or("Not Float32Array")?;
                            let result = arrow::compute::kernels::numeric::add(a_f32, b_f32).map_err(|e| e.to_string())?;
                            self.stack.push(Value::ArrowArray(Arc::new(result)));
                        }
                        _ => return Err("Invalid types for Add".into()),
                    }
                }
                OpCode::Mul => {
                    let b = self.stack.pop().ok_or("Stack underflow")?;
                    let a = self.stack.pop().ok_or("Stack underflow")?;

                    match (a, b) {
                        (Value::Tensor1D(t), Value::Scalar(s)) | (Value::Scalar(s), Value::Tensor1D(t)) => {
                            self.stack.push(Value::Tensor1D(t * s));
                        }
                        (Value::ArrowArray(arr), Value::Scalar(s)) | (Value::Scalar(s), Value::ArrowArray(arr)) => {
                            let t_f32 = arr.as_any().downcast_ref::<arrow::array::Float32Array>().ok_or("Not Float32Array")?;
                            let result: arrow::array::Float32Array = t_f32.iter().map(|opt_v| opt_v.map(|v| v * s)).collect();
                            self.stack.push(Value::ArrowArray(Arc::new(result)));
                        }
                        _ => return Err("Invalid types for Mul".into()),
                    }
                }
                OpCode::Fusion => {
                    let b_weight = self.stack.pop().ok_or("Stack underflow")?;
                    let a_weight = self.stack.pop().ok_or("Stack underflow")?;
                    let b = self.stack.pop().ok_or("Stack underflow")?;
                    let a = self.stack.pop().ok_or("Stack underflow")?;

                    match (a, b, a_weight, b_weight) {
                        (Value::Tensor1D(t1), Value::Tensor1D(t2), Value::Scalar(w1), Value::Scalar(w2)) => {
                            self.stack.push(Value::Tensor1D((t1 * w1) + (t2 * w2)));
                        }
                        (Value::ArrowArray(a_arr), Value::ArrowArray(b_arr), Value::Scalar(w1), Value::Scalar(w2)) => {
                            let t1_f32 = a_arr.as_any().downcast_ref::<arrow::array::Float32Array>().ok_or("Not Float32Array")?;
                            let t2_f32 = b_arr.as_any().downcast_ref::<arrow::array::Float32Array>().ok_or("Not Float32Array")?;

                            // To minimize allocations, we compute w1*v1 + w2*v2 directly in one pass
                            let result: arrow::array::Float32Array = t1_f32.iter().zip(t2_f32.iter()).map(|(opt_v1, opt_v2)| {
                                match (opt_v1, opt_v2) {
                                    (Some(v1), Some(v2)) => Some(v1 * w1 + v2 * w2),
                                    _ => None,
                                }
                            }).collect();
                            self.stack.push(Value::ArrowArray(Arc::new(result)));
                        }
                        _ => return Err("Invalid types for Fusion".into()),
                    }
                }
                OpCode::TopK(k) => {
                    let a = self.stack.pop().ok_or("Stack underflow")?;
                    match a {
                        Value::Tensor1D(t) => {
                            let mut indexed_scores: Vec<(usize, f32)> = t.into_iter().enumerate().collect();
                            indexed_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

                            let top_k = indexed_scores.into_iter().take(k);
                            let (indices, scores): (Vec<usize>, Vec<f32>) = top_k.unzip();

                            self.stack.push(Value::Result(TopKResult { indices, scores }));
                        }
                        Value::ArrowArray(arr) => {
                            let t_f32 = arr.as_any().downcast_ref::<arrow::array::Float32Array>().ok_or("Not Float32Array")?;
                            let mut indexed_scores: Vec<(usize, f32)> = t_f32.iter()
                                .enumerate()
                                .filter_map(|(i, v)| v.map(|score| (i, score)))
                                .collect();

                            indexed_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

                            let top_k = indexed_scores.into_iter().take(k);
                            let (indices, scores): (Vec<usize>, Vec<f32>) = top_k.unzip();

                            self.stack.push(Value::Result(TopKResult { indices, scores }));
                        }
                        _ => return Err("Invalid types for TopK".into()),
                    }
                }
                OpCode::Emit => {
                    let res = self.stack.pop().ok_or("Stack underflow")?;
                    return Ok(Some(res));
                }
                OpCode::RetrieveQdrant { query: _, filters: _, top_k } => {
                    // Mock data retrieval (Task 1.4.1, 1.4.6)
                    let mut ids = Vec::new();
                    let mut vectors = Vec::new();
                    for i in 0..top_k {
                        ids.push(i);
                        vectors.push(Array1::from_elem(768, 0.1));
                    }
                    let batch = qdrant_to_record_batch(&ids, &vectors).map_err(|e| e.to_string())?;
                    self.batch = Some(Arc::new(batch));
                }
                OpCode::RetrievePostgres => {
                    // Mock retrieving BM25 scores (Task 1.4.2, 1.4.5 context)
                    let batch = self.batch.as_ref().ok_or("No batch found")?;
                    let n = batch.num_rows();

                    // In a real implementation we would fetch from Postgres and attach it.
                    // Here we just mock it using a Float32Array.
                    let scores = arrow::array::Float32Array::from(vec![1.0; n]);
                    self.stack.push(Value::ArrowArray(Arc::new(scores)));
                }
                OpCode::ComputeSemantic => {
                    // Compute dot product of query_vector with each candidate vector (Task 1.4.4)
                    let batch = self.batch.as_ref().ok_or("No batch found")?;
                    let flat = crate::arrow::extract_vector_scores(batch)?;

                    let dim = 768;
                    let n = flat.len() / dim;
                    let mut scores = Vec::with_capacity(n);

                    let query = self.query_vector.as_slice().ok_or("query not contiguous")?;
                    let values = flat.values();

                    for i in 0..n {
                        let start = i * dim;
                        let chunk = &values[start..start + dim];
                        let dot: f32 = chunk.iter().zip(query).map(|(a, b)| a * b).sum();
                        scores.push(dot);
                    }

                    self.stack.push(Value::ArrowArray(Arc::new(arrow::array::Float32Array::from(scores))));
                }
                OpCode::Mmr(_diversity) => {
                    // Mock MMR: return the tensor unchanged for now
                    // In a real implementation, this would perform Maximum Marginal Relevance
                    // to maximize diversity.
                }
                OpCode::Rrf(_k) => {
                    // Mock RRF: typically this combines multiple rankings.
                    // For now, we do nothing and let TopK handle the single score tensor.
                }
                OpCode::IfConfidence(threshold, low_plan, high_plan) => {
                    // Pop the scores tensor to check confidence
                    let a = self.stack.pop().ok_or("Stack underflow in IfConfidence")?;
                    let mut max_score = 0.0;
                    match a {
                        Value::Tensor1D(ref t) => {
                            for &score in t.iter() {
                                if score > max_score {
                                    max_score = score;
                                }
                            }
                        }
                        Value::ArrowArray(ref arr) => {
                            let t_f32 = arr.as_any().downcast_ref::<arrow::array::Float32Array>().ok_or("Not Float32Array")?;
                            for score in t_f32.iter().flatten() {
                                if score > max_score {
                                    max_score = score;
                                }
                            }
                        }
                        _ => return Err("Invalid type for IfConfidence condition".into()),
                    }

                    // Put it back in case the branch needs it
                    self.stack.push(a);

                    let branch_plan = if max_score >= threshold {
                        high_plan
                    } else {
                        low_plan
                    };

                    // Execute the chosen branch
                    if let Some(res) = self.run(branch_plan)? {
                        return Ok(Some(res));
                    }
                }
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;

    #[test]
    fn test_load_const() {
        let mut vm = VM::new();
        let mut program = Program::new();
        program.push(OpCode::LoadConst(42.0));
        program.push(OpCode::Emit);

        let res = vm.run(program).unwrap().unwrap();
        assert_eq!(res, Value::Scalar(42.0));
    }

    #[test]
    fn test_add() {
        let mut vm = VM::new();
        vm.stack.push(Value::ArrowArray(Arc::new(arrow::array::Float32Array::from(vec![1.0, 2.0, 3.0]))));
        vm.stack.push(Value::ArrowArray(Arc::new(arrow::array::Float32Array::from(vec![4.0, 5.0, 6.0]))));

        let mut program = Program::new();
        program.push(OpCode::Add);
        program.push(OpCode::Emit);

        let res = vm.run(program).unwrap().unwrap();
        if let Value::ArrowArray(arr) = res {
            let t_f32 = arr.as_any().downcast_ref::<arrow::array::Float32Array>().unwrap();
            let result_vec: Vec<f32> = t_f32.iter().map(|v| v.unwrap()).collect();
            assert_eq!(result_vec, vec![5.0, 7.0, 9.0]);
        } else {
            panic!("Expected ArrowArray");
        }
    }

    #[test]
    fn test_mul() {
        let mut vm = VM::new();
        vm.stack.push(Value::ArrowArray(Arc::new(arrow::array::Float32Array::from(vec![1.0, 2.0, 3.0]))));
        vm.stack.push(Value::Scalar(2.0));

        let mut program = Program::new();
        program.push(OpCode::Mul);
        program.push(OpCode::Emit);

        let res = vm.run(program).unwrap().unwrap();
        if let Value::ArrowArray(arr) = res {
            let t_f32 = arr.as_any().downcast_ref::<arrow::array::Float32Array>().unwrap();
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
        vm.stack.push(Value::ArrowArray(Arc::new(arrow::array::Float32Array::from(vec![1.0, 1.0, 1.0])))); // a (t1)
        vm.stack.push(Value::ArrowArray(Arc::new(arrow::array::Float32Array::from(vec![2.0, 2.0, 2.0])))); // b (t2)
        vm.stack.push(Value::Scalar(0.5));                     // a_weight (w1)
        vm.stack.push(Value::Scalar(0.5));                     // b_weight (w2)

        let mut program = Program::new();
        program.push(OpCode::Fusion);
        program.push(OpCode::Emit);

        let res = vm.run(program).unwrap().unwrap();
        if let Value::ArrowArray(arr) = res {
            let t_f32 = arr.as_any().downcast_ref::<arrow::array::Float32Array>().unwrap();
            let result_vec: Vec<f32> = t_f32.iter().map(|v| v.unwrap()).collect();
            assert_eq!(result_vec, vec![1.5, 1.5, 1.5]); // 1*0.5 + 2*0.5 = 1.5
        } else {
            panic!("Expected ArrowArray");
        }
    }

    #[test]
    fn test_topk() {
        let mut vm = VM::new();
        vm.stack.push(Value::ArrowArray(Arc::new(arrow::array::Float32Array::from(vec![1.0, 5.0, 3.0, 9.0, 2.0]))));

        let mut program = Program::new();
        program.push(OpCode::TopK(3));
        program.push(OpCode::Emit);

        let res = vm.run(program).unwrap().unwrap();
        if let Value::Result(topk) = res {
            assert_eq!(topk.indices, vec![3, 1, 2]);
            assert_eq!(topk.scores, vec![9.0, 5.0, 3.0]);
        } else {
            panic!("Expected TopKResult");
        }
    }
}
