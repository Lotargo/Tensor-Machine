use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tensor_rag::gpu::{ColbertOffsets, GpuContext};

const DIM: u32 = 128;
const NUM_Q: u32 = 32;
const NUM_DOCS: u32 = 100;

fn generate_varying_data() -> (Vec<f32>, Vec<f32>, Vec<ColbertOffsets>) {
    let dim = DIM as usize;
    let num_q = NUM_Q as usize;

    let query_tokens: Vec<f32> = (0..num_q * dim)
        .map(|i| ((i * 31) % 100) as f32 / 100.0)
        .collect();

    let token_lengths = [10, 50, 100, 200];
    let mut doc_tokens = Vec::new();
    let mut offsets = Vec::new();
    let mut doc_start = 0u32;

    for doc_idx in 0..NUM_DOCS as usize {
        let num_tokens = token_lengths[doc_idx % 4];
        let count = num_tokens * dim;
        for j in 0..count {
            doc_tokens.push(((j * 17 + doc_idx * 13) % 100) as f32 / 100.0);
        }
        offsets.push(ColbertOffsets {
            doc_start,
            num_tokens: num_tokens as u32,
        });
        doc_start += num_tokens as u32;
    }

    (query_tokens, doc_tokens, offsets)
}

fn cpu_colbert_maxsim(
    query_tokens: &[f32],
    doc_tokens: &[f32],
    num_docs: u32,
    num_q: u32,
    offsets: &[ColbertOffsets],
    dim: u32,
) -> Vec<f32> {
    let dim = dim as usize;
    let num_q = num_q as usize;
    let num_docs = num_docs as usize;

    let mut results = vec![0.0f32; num_docs];
    for doc_idx in 0..num_docs {
        let offs = &offsets[doc_idx];
        let num_d = offs.num_tokens as usize;
        let doc_start = offs.doc_start as usize;

        let mut doc_score = 0.0f32;
        for q in 0..num_q {
            let mut max_sim = -1e10f32;
            for d in 0..num_d {
                let mut sim = 0.0f32;
                for i in 0..dim {
                    sim += query_tokens[q * dim + i] * doc_tokens[(doc_start + d) * dim + i];
                }
                if sim > max_sim {
                    max_sim = sim;
                }
            }
            doc_score += max_sim;
        }
        results[doc_idx] = doc_score;
    }
    results
}

fn bench_colbert_maxsim(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let gpu = rt.block_on(GpuContext::new());
    let (query_tokens, doc_tokens, offsets) = generate_varying_data();

    let mut group = c.benchmark_group("colbert_maxsim_100docs");

    group.bench_function("cpu", |b| {
        b.iter(|| {
            black_box(cpu_colbert_maxsim(
                black_box(&query_tokens),
                black_box(&doc_tokens),
                black_box(NUM_DOCS),
                black_box(NUM_Q),
                black_box(&offsets),
                black_box(DIM),
            ))
        });
    });

    if let Some(ref gpu) = gpu {
        let _ = gpu.colbert_maxsim_variant(&query_tokens, &doc_tokens, NUM_DOCS, NUM_Q, &offsets, DIM, true);
        let _ = gpu.colbert_maxsim_variant(&query_tokens, &doc_tokens, NUM_DOCS, NUM_Q, &offsets, DIM, false);

        group.bench_function("gpu_tiled", |b| {
            b.iter(|| {
                black_box(
                    gpu.colbert_maxsim_variant(
                        black_box(&query_tokens),
                        black_box(&doc_tokens),
                        black_box(NUM_DOCS),
                        black_box(NUM_Q),
                        black_box(&offsets),
                        black_box(DIM),
                        black_box(true),
                    )
                    .unwrap(),
                )
            });
        });

        group.bench_function("gpu_naive", |b| {
            b.iter(|| {
                black_box(
                    gpu.colbert_maxsim_variant(
                        black_box(&query_tokens),
                        black_box(&doc_tokens),
                        black_box(NUM_DOCS),
                        black_box(NUM_Q),
                        black_box(&offsets),
                        black_box(DIM),
                        black_box(false),
                    )
                    .unwrap(),
                )
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_colbert_maxsim);
criterion_main!(benches);
