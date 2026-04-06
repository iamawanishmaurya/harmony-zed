mod tools;
mod transport;
mod types;

use std::path::Path;
use std::sync::{Arc, Mutex};
use harmony_memory::store::MemoryStore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse CLI args
    let args: Vec<String> = std::env::args().collect();
    let db_path = parse_arg(&args, "--db-path")
        .unwrap_or_else(|| ".harmony/memory.db".to_string());

    // Initialize tracing to stderr (stdout is reserved for MCP protocol)
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter("harmony_mcp=debug")
        .init();

    tracing::info!("Starting harmony-mcp server with db: {}", db_path);

    // Open memory store
    let store = MemoryStore::open(Path::new(&db_path))?;
    let store = Arc::new(Mutex::new(store));

    tracing::info!("Memory store opened successfully");

    // Run MCP server on stdin/stdout (JSON-RPC 2.0)
    transport::run_stdio_server(store).await?;

    Ok(())
}

fn parse_arg(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}
