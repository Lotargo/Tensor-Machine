use criterion::{Criterion, criterion_group, criterion_main};
use reqwest::Client;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::time::Instant;

// This stress test will start the mcp load balancer and send lots of requests
fn stress_test_router(c: &mut Criterion) {
    let mut group = c.benchmark_group("mcp_stress");
    group.sample_size(10); // Reduce from default 100 to 10
    group.measurement_time(std::time::Duration::from_secs(10));

    group.bench_function("1000_requests", |b| {
        // Run this in a tokio runtime
        let rt = tokio::runtime::Runtime::new().unwrap();

        // Start the server for the benchmark
        rt.block_on(async {
            let cancel_token = tokio_util::sync::CancellationToken::new();
            let db_pools = std::sync::Arc::new(tensor_rag::db::DbPools::new());
            let mut worker_senders = Vec::new();

            for _i in 0..4 {
                let (tx, rx) = tokio::sync::mpsc::channel(100);
                worker_senders.push(tx);
                let mut server = tensor_rag::mcp::McpServer::new(db_pools.clone(), None);
                tensor_rag::mcp::register_search_tool(&mut server);
                let token = cancel_token.clone();
                tokio::spawn(async move {
                    server.run_server(rx, token).await;
                });
            }

            let app_state = std::sync::Arc::new(tensor_rag::router::AppState {
                sessions: std::sync::Arc::new(std::sync::RwLock::new(
                    std::collections::HashMap::new(),
                )),
                workers: worker_senders,
                next_worker: std::sync::atomic::AtomicUsize::new(0),
            });
            let router = tensor_rag::router::mcp_router(app_state);
            let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 8080));
            let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
            tokio::spawn(async move {
                axum::serve(listener, router).await.unwrap();
            });
            // Small wait for server
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        });

        b.to_async(&rt).iter_custom(|iters| async move {
            let client = Client::new();
            // Start the total duration timer
            let start = Instant::now();

            for _ in 0..iters {
                let mut tasks = Vec::new();
                let success_count = Arc::new(AtomicUsize::new(0));

                let num_clients = 50;
                let reqs_per_client = 10; // Total 500 reqs

                for client_id in 0..num_clients {
                    let client = client.clone();
                    let success_count = success_count.clone();
                    let session_id = format!("session_stress_{}", client_id);

                    tasks.push(tokio::spawn(async move {
                        for seq in 0..reqs_per_client {
                            let req = serde_json::json!({
                                "tool": "search",
                                "args": {
                                    "query": "stress test",
                                    "filters": null,
                                    "rerank_strategy": "SemanticOnly",
                                    "final_top_k": 5
                                },
                                "session_id": session_id.clone(),
                                "sequence_number": seq,
                            });

                            // If the server is up and listening on 8080.
                            // We don't start it here since `cargo bench` tests the app performance
                            // but actually we need to ensure the server is running.
                            // Wait, if we start the server in `main.rs`, `cargo bench` won't run `main.rs`.
                            // So this bench function would fail connecting to 8080!

                            let res = client
                                .post("http://127.0.0.1:8080/mcp")
                                .json(&req)
                                .send()
                                .await;

                            if let Ok(r) = res {
                                if r.status().is_success() {
                                    success_count.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }
                    }));
                }

                futures::future::join_all(tasks).await;
            }
            start.elapsed()
        });
    });
    group.finish();
}

criterion_group!(benches, stress_test_router);
criterion_main!(benches);
