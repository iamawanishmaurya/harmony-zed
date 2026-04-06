use std::sync::{Arc, Mutex};
use std::io::{self, BufRead, Read, Write};
use harmony_memory::store::MemoryStore;
use crate::tools;

/// Run the MCP server on stdin/stdout using JSON-RPC 2.0 protocol.
pub async fn run_stdio_server(store: Arc<Mutex<MemoryStore>>) -> anyhow::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();

    let mut reader = stdin.lock();
    let mut writer = stdout.lock();

    loop {
        // Read Content-Length header
        let content_length = match read_content_length(&mut reader) {
            Ok(len) => len,
            Err(_) => break, // EOF or broken pipe
        };

        // Read the JSON body
        let mut body = vec![0u8; content_length];
        if reader.read_exact(&mut body).is_err() {
            break;
        }

        let body_str = String::from_utf8_lossy(&body).to_string();
        tracing::debug!("Received: {}", body_str);

        // Parse and handle the request
        let response = handle_request(&body_str, &store);

        if let Some(response) = response {
            let response_str = serde_json::to_string(&response)?;
            let header = format!("Content-Length: {}\r\n\r\n", response_str.len());
            writer.write_all(header.as_bytes())?;
            writer.write_all(response_str.as_bytes())?;
            writer.flush()?;
            tracing::debug!("Sent: {}", response_str);
        }
    }

    Ok(())
}

// ── TCP IPC Server (Windows fallback) ─────────────────────────────────────────

/// Start a TCP IPC server on 127.0.0.1:17432 for Windows.
///
/// On Windows, Unix sockets are not available, so the sidecar listens
/// on a local TCP port. The port is written to `.harmony/harmony.port`
/// so the extension can discover it.
#[cfg(target_os = "windows")]
pub async fn start_ipc_server(
    project_root: &std::path::Path,
    store: Arc<Mutex<MemoryStore>>,
) -> anyhow::Result<()> {
    use tokio::net::TcpListener;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

    let addr = "127.0.0.1:17432";
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("IPC TCP server listening on {}", addr);

    // Write the port file so the extension can find us
    let port_file = project_root.join(".harmony").join("harmony.port");
    if let Some(parent) = port_file.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&port_file, addr).await?;
    tracing::info!("Port file written to {:?}", port_file);

    loop {
        let (mut socket, peer) = listener.accept().await?;
        let store = store.clone();
        tracing::debug!("IPC client connected: {}", peer);

        tokio::spawn(async move {
            let (reader_half, mut writer_half) = socket.split();
            let mut reader = BufReader::new(reader_half);

            loop {
                // Read Content-Length header
                let mut header_line = String::new();
                let content_length = loop {
                    header_line.clear();
                    match reader.read_line(&mut header_line).await {
                        Ok(0) => return, // Connection closed
                        Ok(_) => {}
                        Err(_) => return,
                    }
                    let trimmed = header_line.trim();
                    if trimmed.is_empty() { continue; }
                    if let Some(len_str) = trimmed.strip_prefix("Content-Length: ") {
                        if let Ok(len) = len_str.parse::<usize>() {
                            // Read empty line after header
                            let mut empty = String::new();
                            let _ = reader.read_line(&mut empty).await;
                            break len;
                        }
                    }
                };

                // Read JSON body
                let mut body = vec![0u8; content_length];
                if reader.read_exact(&mut body).await.is_err() {
                    return;
                }

                let body_str = String::from_utf8_lossy(&body).to_string();
                tracing::debug!("TCP received: {}", body_str);

                let response = handle_request(&body_str, &store);
                if let Some(response) = response {
                    if let Ok(response_str) = serde_json::to_string(&response) {
                        let header = format!("Content-Length: {}\r\n\r\n", response_str.len());
                        if writer_half.write_all(header.as_bytes()).await.is_err() { return; }
                        if writer_half.write_all(response_str.as_bytes()).await.is_err() { return; }
                        if writer_half.flush().await.is_err() { return; }
                    }
                }
            }
        });
    }
}

/// Start a Unix socket IPC server (Linux/macOS).
#[cfg(not(target_os = "windows"))]
pub async fn start_ipc_server(
    project_root: &std::path::Path,
    store: Arc<Mutex<MemoryStore>>,
) -> anyhow::Result<()> {
    use tokio::net::UnixListener;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

    let socket_path = project_root.join(".harmony").join("harmony.sock");
    if let Some(parent) = socket_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Remove stale socket file
    let _ = tokio::fs::remove_file(&socket_path).await;

    let listener = UnixListener::bind(&socket_path)?;
    tracing::info!("IPC Unix socket listening at {:?}", socket_path);

    loop {
        let (mut socket, _) = listener.accept().await?;
        let store = store.clone();

        tokio::spawn(async move {
            let (reader_half, mut writer_half) = socket.split();
            let mut reader = BufReader::new(reader_half);

            loop {
                // Read Content-Length header
                let mut header_line = String::new();
                let content_length = loop {
                    header_line.clear();
                    match reader.read_line(&mut header_line).await {
                        Ok(0) => return,
                        Ok(_) => {}
                        Err(_) => return,
                    }
                    let trimmed = header_line.trim();
                    if trimmed.is_empty() { continue; }
                    if let Some(len_str) = trimmed.strip_prefix("Content-Length: ") {
                        if let Ok(len) = len_str.parse::<usize>() {
                            let mut empty = String::new();
                            let _ = reader.read_line(&mut empty).await;
                            break len;
                        }
                    }
                };

                let mut body = vec![0u8; content_length];
                if reader.read_exact(&mut body).await.is_err() { return; }

                let body_str = String::from_utf8_lossy(&body).to_string();
                tracing::debug!("Unix received: {}", body_str);

                let response = handle_request(&body_str, &store);
                if let Some(response) = response {
                    if let Ok(response_str) = serde_json::to_string(&response) {
                        let header = format!("Content-Length: {}\r\n\r\n", response_str.len());
                        if writer_half.write_all(header.as_bytes()).await.is_err() { return; }
                        if writer_half.write_all(response_str.as_bytes()).await.is_err() { return; }
                        if writer_half.flush().await.is_err() { return; }
                    }
                }
            }
        });
    }
}

// ── Shared Protocol ───────────────────────────────────────────────────────────

fn read_content_length<R: BufRead>(reader: &mut R) -> anyhow::Result<usize> {
    let mut header_line = String::new();
    loop {
        header_line.clear();
        let bytes_read = reader.read_line(&mut header_line)?;
        if bytes_read == 0 {
            return Err(anyhow::anyhow!("EOF reached"));
        }
        let trimmed = header_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(len_str) = trimmed.strip_prefix("Content-Length: ") {
            let len: usize = len_str.parse()?;
            // Read the empty line after headers
            let mut empty = String::new();
            reader.read_line(&mut empty)?;
            return Ok(len);
        }
    }
}

fn handle_request(body: &str, store: &Arc<Mutex<MemoryStore>>) -> Option<serde_json::Value> {
    let request: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => {
            return Some(serde_json::json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": {
                    "code": -32700,
                    "message": format!("Parse error: {}", e)
                }
            }));
        }
    };

    let method = request.get("method")?.as_str()?;
    let id = request.get("id").cloned();
    let params = request.get("params").cloned().unwrap_or(serde_json::json!({}));

    let result = match method {
        "initialize" => {
            Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "harmony-memory",
                    "version": "0.1.0"
                }
            }))
        }
        "initialized" => {
            // Notification, no response needed
            return None;
        }
        "tools/list" => {
            Some(tools::list_tools())
        }
        "tools/call" => {
            let tool_name = params.get("name")?.as_str()?;
            let arguments = params.get("arguments").cloned().unwrap_or(serde_json::json!({}));
            Some(tools::call_tool(tool_name, &arguments, store))
        }
        "shutdown" => {
            Some(serde_json::json!(null))
        }
        _ => {
            Some(serde_json::json!({
                "error": {
                    "code": -32601,
                    "message": format!("Method not found: {}", method)
                }
            }))
        }
    };

    result.map(|r| {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": r
        })
    })
}
