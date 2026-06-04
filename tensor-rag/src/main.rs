use std::sync::Arc;
use tensor_rag::db::DbPools;
use tensor_rag::mcp::{McpRequest, McpServer};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::info;

fn print_banner_row(label: &str, value: &str) {
    println!("| {:<18} | {:<56} |", label, value);
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cancel_token = CancellationToken::new();
    let cloned_token = cancel_token.clone();
    let control = Arc::new(tensor_rag::router::ServerControl {
        cancel_token: cancel_token.clone(),
        restart_requested: std::sync::atomic::AtomicBool::new(false),
    });

    let metrics_clone = Arc::new(tensor_rag::metrics::Metrics::new());
    let metrics = metrics_clone.clone();

    // Start the metrics and health server
    let metrics_cancel = cancel_token.clone();
    tokio::spawn(async move {
        if let Err(e) = tensor_rag::metrics::serve(metrics, 9090, metrics_cancel).await {
            tracing::error!("Metrics server error: {}", e);
        }
    });

    // Spawn a task to listen for ctrl-c
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            info!("Received Ctrl-C, initiating graceful shutdown...");
            cloned_token.cancel();
        }
    });

    // Initialize the DB pools
    let mut pools = DbPools::new();
    if let Ok(url) = std::env::var("DATABASE_URL") {
        info!("Initializing PostgreSQL pool with URL: {}", url);
        if let Err(e) = pools.init_postgres(&url).await {
            tracing::error!("Failed to initialize PostgreSQL pool: {}", e);
        }
    } else {
        info!("DATABASE_URL not set, running PostgreSQL in mock mode.");
    }

    if let Ok(url) = std::env::var("QDRANT_URL") {
        info!("Initializing Qdrant client with URL: {}", url);
        if let Err(e) = pools.add_qdrant_node("default", &url).await {
            tracing::error!("Failed to initialize Qdrant node: {}", e);
        }
    } else {
        info!("QDRANT_URL not set, running Qdrant in mock mode.");
    }
    let db_pools = Arc::new(pools);

    // Start VM Worker process if TENSOR_VM_MODE is ipc
    let mut _vm_worker_process = None;
    if std::env::var("TENSOR_VM_MODE").unwrap_or_default() == "ipc" {
        info!("Starting VM Worker process in IPC mode...");
        let socket_path = tensor_rag::config::get_vm_socket_path();
        _vm_worker_process = Some(
            std::process::Command::new("cargo")
                .args([
                    "run",
                    "--bin",
                    "vm_worker",
                    "--",
                    &format!("--socket={}", socket_path),
                ])
                .spawn()
                .expect("Failed to spawn vm_worker process"),
        );
        // Wait briefly for the socket to bind
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    let num_workers = 4;
    let mut worker_senders = Vec::new();

    // Start Arrow Flight Service
    let flight_cancel = cancel_token.clone();
    tokio::spawn(async move {
        let flight_addr =
            std::env::var("ARROW_FLIGHT_ADDR").unwrap_or_else(|_| "0.0.0.0:50051".to_string());
        if let Ok(addr) = flight_addr.parse() {
            tracing::info!("Starting Arrow Flight Service on {}", addr);
            let service = tensor_rag::flight::TensorFlightService::new();
            if let Err(e) = tonic::transport::Server::builder()
                .add_service(arrow_flight::flight_service_server::FlightServiceServer::new(service))
                .serve_with_shutdown(addr, async {
                    flight_cancel.cancelled().await;
                })
                .await
            {
                tracing::error!("Flight server error: {}", e);
            }
        }
    });

    info!("Starting {} MCP Server instances.", num_workers);
    for i in 0..num_workers {
        let (tx, rx) = mpsc::channel::<McpRequest>(100);
        worker_senders.push(tx);

        let mut server = McpServer::new(db_pools.clone(), Some(metrics_clone.clone()));
        // Ensure default tools are registered
        tensor_rag::mcp::register_search_tool(&mut server);

        let token = cancel_token.clone();
        tokio::spawn(async move {
            info!("Worker {} starting", i);
            server.run_server(rx, token).await;
            info!("Worker {} shutdown", i);
        });
    }

    // Initialize the Load Balancer Router
    let app_state = Arc::new(tensor_rag::router::AppState {
        sessions: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
        workers: worker_senders,
        next_worker: std::sync::atomic::AtomicUsize::new(0),
        db_pools: db_pools.clone(),
        control: control.clone(),
    });

    let router = tensor_rag::router::mcp_router(app_state);

    // Bind and serve the router
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let mut listener_opt = None;
    for attempt in 1..=10 {
        match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => {
                listener_opt = Some(l);
                break;
            }
            Err(e) => {
                if attempt == 10 {
                    return Err(e.into());
                }
                tracing::warn!(
                    "Failed to bind to {} (attempt {}/10): {}. Retrying in 500ms...",
                    addr,
                    attempt,
                    e
                );
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
    }
    let listener = listener_opt.unwrap();
    let pg_status = if db_pools.postgres.is_some() {
        "ACTIVE (Pool)"
    } else {
        "MOCK MODE"
    };
    let qdrant_status = if db_pools.qdrant_clients.contains_key("default") {
        "ACTIVE (Client)"
    } else {
        "MOCK MODE"
    };
    let flight_addr =
        std::env::var("ARROW_FLIGHT_ADDR").unwrap_or_else(|_| "0.0.0.0:50051".to_string());
    let router_value = format!("http://{}", addr);
    let portal_value = "http://localhost:8080";

    println!("\n");
    println!("+--------------------+----------------------------------------------------------+");
    println!("|                    TENSOR MACHINE INITIALIZED SUCCESSFULLY                    |");
    println!("+--------------------+----------------------------------------------------------+");
    print_banner_row(
        "[DATABASE]",
        &format!("PostgreSQL Connection status : {}", pg_status),
    );
    print_banner_row(
        "[VECTOR DB]",
        &format!("Qdrant Vector DB status      : {}", qdrant_status),
    );
    print_banner_row(
        "[FLIGHT API]",
        &format!("Arrow Flight Plane listening : {}", flight_addr),
    );
    print_banner_row(
        "[MCP SERVER]",
        &format!("Active CPU Worker Instances  : {}", num_workers),
    );
    print_banner_row(
        "[MCP ROUTER]",
        &format!("Axum HTTP Router running on  : {}", router_value),
    );
    print_banner_row(
        "[PORTAL UI]",
        &format!("Document Portal URL          : {}", portal_value),
    );
    println!("+--------------------+----------------------------------------------------------+");
    println!("\n");

    let server = axum::serve(listener, router);

    tokio::select! {
        _ = server => {
            info!("Router server exited.");
        }
        _ = cancel_token.cancelled() => {
            info!("Router shutting down due to cancel token.");
        }
    }

    info!("Server shutdown complete.");

    if let Some(mut child) = _vm_worker_process {
        let _ = child.kill();
    }

    if control
        .restart_requested
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        if std::env::var("TENSOR_RAG_LAUNCHER").unwrap_or_default() == "true" {
            info!("Restart requested. Exiting with code 123 (handled by launcher).");
            std::process::exit(123);
        } else {
            info!("Restart requested. Spawning child process...");
            let current_exe = std::env::current_exe()?;
            let current_dir = std::env::current_dir()?;
            std::process::Command::new(current_exe)
                .current_dir(current_dir)
                .spawn()?;
        }
    }

    Ok(())
}
