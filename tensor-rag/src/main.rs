use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tensor_rag::mcp::{McpServer, McpRequest};
use tensor_rag::db::DbPools;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cancel_token = CancellationToken::new();
    let cloned_token = cancel_token.clone();

    // Spawn a task to listen for ctrl-c
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            println!("Received Ctrl-C, initiating graceful shutdown...");
            cloned_token.cancel();
        }
    });

    // Initialize the DB pools
    let db_pools = Arc::new(DbPools::new());

    // Create the MCP server
    let mut server = McpServer::new(db_pools);

    // Create a channel for receiving requests (simulating network layer)
    let (_tx, rx) = mpsc::channel::<McpRequest>(100);

    println!("Starting MCP Server. Press Ctrl-C to stop.");

    // Run the server until the cancellation token is triggered
    server.run_server(rx, cancel_token).await;

    println!("Server shutdown complete.");

    Ok(())
}
