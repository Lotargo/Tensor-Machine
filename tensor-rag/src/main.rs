use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tensor_rag::mcp::{McpServer, McpRequest};
use tensor_rag::db::DbPools;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cancel_token = CancellationToken::new();
    let cloned_token = cancel_token.clone();

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
    let db_pools = Arc::new(DbPools::new());

    // Create the MCP server
    let mut server = McpServer::new(db_pools, Some(metrics_clone));

    // Create a channel for receiving requests (simulating network layer)
    let (_tx, rx) = mpsc::channel::<McpRequest>(100);

    info!("Starting MCP Server. Press Ctrl-C to stop.");

    // Run the server until the cancellation token is triggered
    server.run_server(rx, cancel_token).await;

    info!("Server shutdown complete.");

    Ok(())
}
