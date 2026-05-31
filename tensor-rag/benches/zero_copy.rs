use criterion::{black_box, criterion_group, criterion_main, Criterion, BatchSize};
use tensor_rag::ir::OpCode;
use tensor_rag::vm::{Value, VM};
use arrow::array::{Float32Array, Array};
use std::sync::Arc;
use ndarray::Array1;

fn create_arrow_array(size: usize, val: f32) -> Value {
    let arr = Float32Array::from(vec![val; size]);
    Value::ArrowArray(Arc::new(arr) as Arc<dyn Array>)
}

fn create_tensor1d(size: usize, val: f32) -> Value {
    let arr = Array1::from(vec![val; size]);
    Value::Tensor1D(arr)
}

fn bench_vm_add(c: &mut Criterion) {
    let mut group = c.benchmark_group("VM_Add_1000_elements");

    group.bench_function("arrow", |b| {
        b.iter_batched(
            || {
                let mut vm = VM::new();
                vm.stack.push(create_arrow_array(1000, 1.0));
                vm.stack.push(create_arrow_array(1000, 2.0));
                vm
            },
            |mut vm| {
                vm.step(&OpCode::Add).unwrap();
                black_box(vm.stack)
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("tensor1d", |b| {
        b.iter_batched(
            || {
                let mut vm = VM::new();
                vm.stack.push(create_tensor1d(1000, 1.0));
                vm.stack.push(create_tensor1d(1000, 2.0));
                vm
            },
            |mut vm| {
                vm.step(&OpCode::Add).unwrap();
                black_box(vm.stack)
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_vm_mul(c: &mut Criterion) {
    let mut group = c.benchmark_group("VM_Mul_1000_elements");

    group.bench_function("arrow", |b| {
        b.iter_batched(
            || {
                let mut vm = VM::new();
                vm.stack.push(create_arrow_array(1000, 2.0));
                vm.stack.push(Value::Scalar(3.0));
                vm
            },
            |mut vm| {
                vm.step(&OpCode::Mul).unwrap();
                black_box(vm.stack)
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("tensor1d", |b| {
        b.iter_batched(
            || {
                let mut vm = VM::new();
                vm.stack.push(create_tensor1d(1000, 2.0));
                vm.stack.push(Value::Scalar(3.0));
                vm
            },
            |mut vm| {
                vm.step(&OpCode::Mul).unwrap();
                black_box(vm.stack)
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_vm_fusion(c: &mut Criterion) {
    let mut group = c.benchmark_group("VM_Fusion_1000_elements");

    group.bench_function("arrow", |b| {
        b.iter_batched(
            || {
                let mut vm = VM::new();
                vm.stack.push(create_arrow_array(1000, 0.5)); // t1
                vm.stack.push(create_arrow_array(1000, 0.8)); // t2
                vm.stack.push(Value::Scalar(1.0)); // w1
                vm.stack.push(Value::Scalar(2.0)); // w2
                vm
            },
            |mut vm| {
                vm.step(&OpCode::Fusion).unwrap();
                black_box(vm.stack)
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("tensor1d", |b| {
        b.iter_batched(
            || {
                let mut vm = VM::new();
                vm.stack.push(create_tensor1d(1000, 0.5)); // t1
                vm.stack.push(create_tensor1d(1000, 0.8)); // t2
                vm.stack.push(Value::Scalar(1.0)); // w1
                vm.stack.push(Value::Scalar(2.0)); // w2
                vm
            },
            |mut vm| {
                vm.step(&OpCode::Fusion).unwrap();
                black_box(vm.stack)
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_vm_topk(c: &mut Criterion) {
    let mut group = c.benchmark_group("VM_TopK_1000_elements");

    group.bench_function("arrow", |b| {
        b.iter_batched(
            || {
                let mut vm = VM::new();
                let mut vals = vec![0.0; 1000];
                vals[10] = 5.0;
                vals[20] = 10.0;
                let arr = Float32Array::from(vals);
                vm.stack.push(Value::ArrowArray(Arc::new(arr) as Arc<dyn Array>));
                vm
            },
            |mut vm| {
                vm.step(&OpCode::TopK(10)).unwrap();
                black_box(vm.stack)
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("tensor1d", |b| {
        b.iter_batched(
            || {
                let mut vm = VM::new();
                let mut vals = vec![0.0; 1000];
                vals[10] = 5.0;
                vals[20] = 10.0;
                let arr = Array1::from(vals);
                vm.stack.push(Value::Tensor1D(arr));
                vm
            },
            |mut vm| {
                vm.step(&OpCode::TopK(10)).unwrap();
                black_box(vm.stack)
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

criterion_group!(benches, bench_vm_add, bench_vm_mul, bench_vm_fusion, bench_vm_topk);
criterion_main!(benches);
