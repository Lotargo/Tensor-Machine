use ndarray::Array1;
use std::sync::Arc;
use crate::ir::{OpCode, Program};
use arrow::array::Array;

#[derive(Debug, Clone)]
pub enum Value {
    Scalar(f32),
    Tensor1D(Array1<f32>),
    ArrowArray(Arc<dyn Array>),
    Result(TopKResult),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Scalar(a), Value::Scalar(b)) => a == b,
            (Value::Tensor1D(a), Value::Tensor1D(b)) => a == b,
            (Value::Result(a), Value::Result(b)) => a.indices == b.indices && a.scores == b.scores,
            // Simple reference equality for Arc<dyn Array> for tests,
            // or just false if we don't need strict equality on Arrays
            (Value::ArrowArray(a), Value::ArrowArray(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TopKResult {
    pub indices: Vec<usize>,
    pub scores: Vec<f32>,
}

pub struct VM {
    pub stack: Vec<Value>,
    pub batch: Option<Arc<arrow::record_batch::RecordBatch>>,
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
                        self.stack.push(Value::Tensor1D(t1 + t2));
                    }
                    (Value::ArrowArray(arr1), Value::ArrowArray(arr2)) => {
                        let result = arrow::compute::kernels::numeric::add(&arr1 as &dyn arrow::array::Datum, &arr2 as &dyn arrow::array::Datum)
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
                        self.stack.push(Value::Tensor1D(t * s));
                    }
                    (Value::ArrowArray(arr), Value::Scalar(s)) => {
                        let s_arr = arrow::array::Float32Array::from(vec![s; arr.len()]);
                        let result = arrow::compute::kernels::numeric::mul(&arr as &dyn arrow::array::Datum, &s_arr as &dyn arrow::array::Datum)
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
                    (Value::Tensor1D(t1), Value::Tensor1D(t2), Value::Scalar(s1), Value::Scalar(s2)) => {
                        self.stack.push(Value::Tensor1D((t1 * s1) + (t2 * s2)));
                    }
                    (Value::ArrowArray(arr1), Value::ArrowArray(arr2), Value::Scalar(s1), Value::Scalar(s2)) => {
                        let s1_arr = arrow::array::Float32Array::from(vec![s1; arr1.len()]);
                        let s2_arr = arrow::array::Float32Array::from(vec![s2; arr2.len()]);

                        let part1 = arrow::compute::kernels::numeric::mul(&arr1 as &dyn arrow::array::Datum, &s1_arr as &dyn arrow::array::Datum).map_err(|e| e.to_string())?;
                        let part2 = arrow::compute::kernels::numeric::mul(&arr2 as &dyn arrow::array::Datum, &s2_arr as &dyn arrow::array::Datum).map_err(|e| e.to_string())?;

                        let result = arrow::compute::kernels::numeric::add(&part1 as &dyn arrow::array::Datum, &part2 as &dyn arrow::array::Datum).map_err(|e| e.to_string())?;

                        let t_f32 = result.as_any().downcast_ref::<arrow::array::Float32Array>().ok_or("Downcast failed")?;
                        let result: arrow::array::Float32Array = t_f32.iter().collect();
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

                        let top_k = indexed_scores.into_iter().take(*k);
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

                        let top_k = indexed_scores.into_iter().take(*k);
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
                // Mock MMR
            }
            OpCode::Rrf(_k) => {
                // Mock RRF
            }
            OpCode::RetrieveQdrant { .. } | OpCode::RetrievePostgres | OpCode::IfConfidence(..) => {
                return Err("I/O operations and branching must be handled by the orchestrator".into());
            }
        }
        Ok(None)
    }

    /// Backwards compatibility run function for tests
    pub fn run(&mut self, program: Program) -> Result<Option<Value>, String> {
        for instruction in program.instructions {
            match instruction.op {
                OpCode::RetrieveQdrant { query: _, filters: _, top_k } => {
                    let mut ids = Vec::new();
                    let mut vectors = Vec::new();
                    for i in 0..top_k {
                        ids.push(i);
                        vectors.push(Array1::from_elem(768, 0.1));
                    }
                    let batch = crate::arrow::qdrant_to_record_batch(&ids, &vectors).map_err(|e| e.to_string())?;
                    self.batch = Some(Arc::new(batch));
                }
                OpCode::RetrievePostgres => {
                    let batch = self.batch.as_ref().ok_or("No batch found")?;
                    let n = batch.num_rows();
                    let scores = arrow::array::Float32Array::from(vec![1.0; n]);
                    self.stack.push(Value::ArrowArray(Arc::new(scores)));
                }
                OpCode::IfConfidence(threshold, low_plan, high_plan) => {
                    let a = self.stack.pop().ok_or("Stack underflow in IfConfidence")?;
                    let mut max_score = 0.0;
                    match &a {
                        Value::Tensor1D(t) => {
                            for &score in t.iter() {
                                if score > max_score { max_score = score; }
                            }
                        }
                        Value::ArrowArray(arr) => {
                            let t_f32 = arr.as_any().downcast_ref::<arrow::array::Float32Array>().ok_or("Not Float32Array")?;
                            for score in t_f32.iter().flatten() {
                                if score > max_score { max_score = score; }
                            }
                        }
                        _ => return Err("Invalid type for IfConfidence condition".into()),
                    }
                    self.stack.push(a);

                    let branch_plan = if max_score >= threshold { high_plan } else { low_plan };
                    if let Some(res) = self.run(branch_plan)? { return Ok(Some(res)); }
                }
                _ => {
                    if let Some(res) = self.step(&instruction.op)? {
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
