use ndarray::Array1;
use crate::ir::{Program, OpCode};

#[derive(Debug, Clone, PartialEq)]
pub struct TopKResult {
    pub indices: Vec<usize>,
    pub scores: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Scalar(f32),
    Tensor1D(Array1<f32>),
    Result(TopKResult),
}

pub struct VM {
    pub stack: Vec<Value>,
}

impl VM {
    pub fn new() -> Self {
        Self {
            stack: Vec::new(),
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

                    if let (Value::Tensor1D(t1), Value::Tensor1D(t2)) = (a, b) {
                        self.stack.push(Value::Tensor1D(t1 + t2));
                    } else {
                        return Err("Invalid types for Add".into());
                    }
                }
                OpCode::Mul => {
                    let b = self.stack.pop().ok_or("Stack underflow")?;
                    let a = self.stack.pop().ok_or("Stack underflow")?;

                    match (a, b) {
                        (Value::Tensor1D(t), Value::Scalar(s)) | (Value::Scalar(s), Value::Tensor1D(t)) => {
                            self.stack.push(Value::Tensor1D(t * s));
                        }
                        _ => return Err("Invalid types for Mul".into()),
                    }
                }
                OpCode::Fusion => {
                    let b_weight = self.stack.pop().ok_or("Stack underflow")?;
                    let a_weight = self.stack.pop().ok_or("Stack underflow")?;
                    let b = self.stack.pop().ok_or("Stack underflow")?;
                    let a = self.stack.pop().ok_or("Stack underflow")?;

                    if let (Value::Tensor1D(t1), Value::Tensor1D(t2), Value::Scalar(w1), Value::Scalar(w2)) = (a, b, a_weight, b_weight) {
                        self.stack.push(Value::Tensor1D((t1 * w1) + (t2 * w2)));
                    } else {
                        return Err("Invalid types for Fusion".into());
                    }
                }
                OpCode::TopK(k) => {
                    let a = self.stack.pop().ok_or("Stack underflow")?;
                    if let Value::Tensor1D(t) = a {
                        let mut indexed_scores: Vec<(usize, f32)> = t.into_iter().enumerate().collect();
                        indexed_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

                        let top_k = indexed_scores.into_iter().take(k);
                        let (indices, scores): (Vec<usize>, Vec<f32>) = top_k.unzip();

                        self.stack.push(Value::Result(TopKResult { indices, scores }));
                    } else {
                        return Err("Invalid types for TopK".into());
                    }
                }
                OpCode::Emit => {
                    let res = self.stack.pop().ok_or("Stack underflow")?;
                    return Ok(Some(res));
                }
                // Mock operations for unimplemented instructions
                OpCode::RetrieveQdrant { .. } => {
                    // In a real implementation this would fetch data
                }
                OpCode::RetrievePostgres => {
                    // In a real implementation this would fetch data
                }
                OpCode::ComputeSemantic => {
                    // In a real implementation this would compute scores
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
        vm.stack.push(Value::Tensor1D(array![1.0, 2.0, 3.0]));
        vm.stack.push(Value::Tensor1D(array![4.0, 5.0, 6.0]));

        let mut program = Program::new();
        program.push(OpCode::Add);
        program.push(OpCode::Emit);

        let res = vm.run(program).unwrap().unwrap();
        if let Value::Tensor1D(arr) = res {
            assert_eq!(arr, array![5.0, 7.0, 9.0]);
        } else {
            panic!("Expected Tensor1D");
        }
    }

    #[test]
    fn test_mul() {
        let mut vm = VM::new();
        vm.stack.push(Value::Tensor1D(array![1.0, 2.0, 3.0]));
        vm.stack.push(Value::Scalar(2.0));

        let mut program = Program::new();
        program.push(OpCode::Mul);
        program.push(OpCode::Emit);

        let res = vm.run(program).unwrap().unwrap();
        if let Value::Tensor1D(arr) = res {
            assert_eq!(arr, array![2.0, 4.0, 6.0]);
        } else {
            panic!("Expected Tensor1D");
        }
    }

    #[test]
    fn test_fusion() {
        let mut vm = VM::new();
        // Stack order: t1, t2, w1, w2
        vm.stack.push(Value::Tensor1D(array![1.0, 1.0, 1.0])); // a (t1)
        vm.stack.push(Value::Tensor1D(array![2.0, 2.0, 2.0])); // b (t2)
        vm.stack.push(Value::Scalar(0.5));                     // a_weight (w1)
        vm.stack.push(Value::Scalar(0.5));                     // b_weight (w2)

        let mut program = Program::new();
        program.push(OpCode::Fusion);
        program.push(OpCode::Emit);

        let res = vm.run(program).unwrap().unwrap();
        if let Value::Tensor1D(arr) = res {
            assert_eq!(arr, array![1.5, 1.5, 1.5]); // 1*0.5 + 2*0.5 = 1.5
        } else {
            panic!("Expected Tensor1D");
        }
    }

    #[test]
    fn test_topk() {
        let mut vm = VM::new();
        vm.stack.push(Value::Tensor1D(array![1.0, 5.0, 3.0, 9.0, 2.0]));

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
