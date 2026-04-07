use std::io::{self, BufRead, Write};
use std::sync::{Arc, Mutex};

use harmony_memory::store::MemoryStore;
use reqwest::Client;

use crate::tools;
use crate::types::{RequestContext, MACHINE_IP_HEADER, MACHINE_NAME_HEADER};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StdioMessageFormat {
    ContentLength,
    JsonLine,
}

/// Run the MCP server on stdin/stdout using JSON-RPC 2.0 protocol.
pub async fn run_stdio_server(store: Arc<Mutex<MemoryStore>>) -> anyhow::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();

    let mut reader = stdin.lock();
    let mut writer = stdout.lock();
    let request_context = RequestContext::local();

    debug_log(&format!("stdio server started pid={}", std::process::id()));

    loop {
        let (body_str, message_format) = match read_stdio_message(&mut reader) {
            Ok(message) => message,
            Err(error) => {
                debug_log(&format!("read_stdio_message ended: {error}"));
                break;
            }
        };

        if let Some(response) = handle_request(&body_str, &store, &request_context) {
            let response_str = serde_json::to_string(&response)?;
            write_stdio_response(&mut writer, message_format, &response_str)?;
            writer.flush()?;
        }
    }

    debug_log("stdio server exiting");
    Ok(())
}

/// Proxy stdin/stdout MCP traffic to an HTTP MCP endpoint while preserving the caller's framing.
pub async fn run_stdio_http_bridge(
    target_mcp_url: &str,
    request_context: &RequestContext,
) -> anyhow::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let client = Client::new();

    let mut reader = stdin.lock();
    let mut writer = stdout.lock();

    debug_log(&format!(
        "stdio bridge started pid={} target={}",
        std::process::id(),
        target_mcp_url
    ));

    loop {
        let (body_str, message_format) = match read_stdio_message(&mut reader) {
            Ok(message) => message,
            Err(error) => {
                debug_log(&format!("stdio bridge ended: {error}"));
                break;
            }
        };

        let request_value: serde_json::Value = match serde_json::from_str(&body_str) {
            Ok(value) => value,
            Err(error) => {
                let response = jsonrpc_error(None, -32700, format!("Parse error: {error}"));
                let response_str = serde_json::to_string(&response)?;
                write_stdio_response(&mut writer, message_format, &response_str)?;
                writer.flush()?;
                continue;
            }
        };

        let response = client
            .post(target_mcp_url)
            .header(MACHINE_NAME_HEADER, &request_context.machine_name)
            .header(MACHINE_IP_HEADER, &request_context.machine_ip)
            .json(&request_value)
            .send()
            .await;

        let response = match response {
            Ok(response) => response,
            Err(error) => {
                let response = jsonrpc_error(
                    request_value.get("id").cloned(),
                    -32603,
                    format!("Bridge request failed: {error}"),
                );
                let response_str = serde_json::to_string(&response)?;
                write_stdio_response(&mut writer, message_format, &response_str)?;
                writer.flush()?;
                continue;
            }
        };

        if response.status() == reqwest::StatusCode::NO_CONTENT {
            continue;
        }

        let response_value: serde_json::Value = response.json().await?;
        let response_str = serde_json::to_string(&response_value)?;
        write_stdio_response(&mut writer, message_format, &response_str)?;
        writer.flush()?;
    }

    Ok(())
}

// TCP IPC Server (Windows fallback)

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
    use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;

    let addr = "127.0.0.1:17432";
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("IPC TCP server listening on {}", addr);

    let port_file = project_root.join(".harmony").join("harmony.port");
    if let Some(parent) = port_file.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&port_file, addr).await?;
    tracing::info!("Port file written to {:?}", port_file);

    loop {
        let (mut socket, peer) = listener.accept().await?;
        let store = store.clone();
        let request_context = RequestContext::local();
        tracing::debug!("IPC client connected: {}", peer);

        tokio::spawn(async move {
            let (reader_half, mut writer_half) = socket.split();
            let mut reader = BufReader::new(reader_half);

            loop {
                let mut header_line = String::new();
                let content_length = match read_async_content_length(&mut reader, &mut header_line).await {
                    Ok(len) => len,
                    Err(_) => return,
                };

                let mut body = vec![0u8; content_length];
                if reader.read_exact(&mut body).await.is_err() {
                    return;
                }

                let body_str = String::from_utf8_lossy(&body).to_string();
                if let Some(response) = handle_request(&body_str, &store, &request_context) {
                    if let Ok(response_str) = serde_json::to_string(&response) {
                        let header = format!("Content-Length: {}\r\n\r\n", response_str.len());
                        if writer_half.write_all(header.as_bytes()).await.is_err() {
                            return;
                        }
                        if writer_half.write_all(response_str.as_bytes()).await.is_err() {
                            return;
                        }
                        if writer_half.flush().await.is_err() {
                            return;
                        }
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;

    let socket_path = project_root.join(".harmony").join("harmony.sock");
    if let Some(parent) = socket_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let _ = tokio::fs::remove_file(&socket_path).await;

    let listener = UnixListener::bind(&socket_path)?;
    tracing::info!("IPC Unix socket listening at {:?}", socket_path);

    loop {
        let (mut socket, _) = listener.accept().await?;
        let store = store.clone();
        let request_context = RequestContext::local();

        tokio::spawn(async move {
            let (reader_half, mut writer_half) = socket.split();
            let mut reader = BufReader::new(reader_half);

            loop {
                let mut header_line = String::new();
                let content_length = match read_async_content_length(&mut reader, &mut header_line).await {
                    Ok(len) => len,
                    Err(_) => return,
                };

                let mut body = vec![0u8; content_length];
                if reader.read_exact(&mut body).await.is_err() {
                    return;
                }

                let body_str = String::from_utf8_lossy(&body).to_string();
                if let Some(response) = handle_request(&body_str, &store, &request_context) {
                    if let Ok(response_str) = serde_json::to_string(&response) {
                        let header = format!("Content-Length: {}\r\n\r\n", response_str.len());
                        if writer_half.write_all(header.as_bytes()).await.is_err() {
                            return;
                        }
                        if writer_half.write_all(response_str.as_bytes()).await.is_err() {
                            return;
                        }
                        if writer_half.flush().await.is_err() {
                            return;
                        }
                    }
                }
            }
        });
    }
}

fn read_stdio_message<R: BufRead>(reader: &mut R) -> anyhow::Result<(String, StdioMessageFormat)> {
    let mut header_line = String::new();
    let mut content_length = None;

    loop {
        header_line.clear();
        let bytes_read = reader.read_line(&mut header_line)?;
        if bytes_read == 0 {
            return Err(anyhow::anyhow!("EOF reached"));
        }

        let trimmed = header_line.trim();
        if trimmed.is_empty() {
            if let Some(length) = content_length {
                let mut body = vec![0u8; length];
                reader.read_exact(&mut body)?;
                let body_str = String::from_utf8(body)
                    .map_err(|error| anyhow::anyhow!("invalid utf-8 body: {error}"))?;
                return Ok((body_str, StdioMessageFormat::ContentLength));
            }
            continue;
        }

        if content_length.is_none() && looks_like_json_message(trimmed) {
            return Ok((trimmed.to_string(), StdioMessageFormat::JsonLine));
        }

        if let Some((name, value)) = trimmed.split_once(':') {
            if name.trim().eq_ignore_ascii_case("Content-Length") {
                let len: usize = value.trim().parse()?;
                content_length = Some(len);
            }
        }
    }
}

#[cfg(target_os = "windows")]
async fn read_async_content_length<R>(
    reader: &mut tokio::io::BufReader<R>,
    header_line: &mut String,
) -> anyhow::Result<usize>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncBufReadExt;

    let mut content_length = None;

    loop {
        header_line.clear();
        let bytes_read = reader.read_line(header_line).await?;
        if bytes_read == 0 {
            return match content_length {
                Some(length) => Ok(length),
                None => Err(anyhow::anyhow!("EOF reached")),
            };
        }

        let trimmed = header_line.trim();
        if trimmed.is_empty() {
            if let Some(length) = content_length {
                return Ok(length);
            }
            continue;
        }

        if let Some((name, value)) = trimmed.split_once(':') {
            if name.trim().eq_ignore_ascii_case("Content-Length") {
                let len: usize = value.trim().parse()?;
                content_length = Some(len);
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
async fn read_async_content_length<R>(
    reader: &mut tokio::io::BufReader<R>,
    header_line: &mut String,
) -> anyhow::Result<usize>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncBufReadExt;

    let mut content_length = None;

    loop {
        header_line.clear();
        let bytes_read = reader.read_line(header_line).await?;
        if bytes_read == 0 {
            return match content_length {
                Some(length) => Ok(length),
                None => Err(anyhow::anyhow!("EOF reached")),
            };
        }

        let trimmed = header_line.trim();
        if trimmed.is_empty() {
            if let Some(length) = content_length {
                return Ok(length);
            }
            continue;
        }

        if let Some((name, value)) = trimmed.split_once(':') {
            if name.trim().eq_ignore_ascii_case("Content-Length") {
                let len: usize = value.trim().parse()?;
                content_length = Some(len);
            }
        }
    }
}

pub(crate) fn handle_request(
    body: &str,
    store: &Arc<Mutex<MemoryStore>>,
    request_context: &RequestContext,
) -> Option<serde_json::Value> {
    let request: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => {
            return Some(jsonrpc_error(None, -32700, format!("Parse error: {}", e)));
        }
    };

    let method = request.get("method")?.as_str()?;
    let id = request.get("id").cloned();
    let params = request
        .get("params")
        .cloned()
        .unwrap_or(serde_json::json!({}));

    let result = match method {
        "initialize" => {
            let protocol_version = params
                .get("protocolVersion")
                .and_then(|value| value.as_str())
                .unwrap_or("2024-11-05");

            Some(serde_json::json!({
                "protocolVersion": protocol_version,
                "capabilities": {
                    "tools": {
                        "listChanged": false
                    }
                },
                "serverInfo": {
                    "name": "harmony-memory",
                    "version": "0.1.0"
                }
            }))
        }
        "notifications/initialized" | "initialized" => {
            return None;
        }
        "ping" => Some(serde_json::json!({})),
        "tools/list" => Some(tools::list_tools()),
        "tools/call" => {
            let tool_name = params.get("name")?.as_str()?;
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or(serde_json::json!({}));
            Some(tools::call_tool(tool_name, &arguments, store, request_context))
        }
        "shutdown" => Some(serde_json::json!(null)),
        _ => {
            if id.is_none() {
                return None;
            }

            return Some(jsonrpc_error(
                id,
                -32601,
                format!("Method not found: {}", method),
            ));
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

fn jsonrpc_error(id: Option<serde_json::Value>, code: i32, message: String) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    })
}

fn looks_like_json_message(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('{') && trimmed.ends_with('}')
}

fn write_stdio_response<W: Write>(
    writer: &mut W,
    message_format: StdioMessageFormat,
    response_str: &str,
) -> anyhow::Result<()> {
    match message_format {
        StdioMessageFormat::ContentLength => {
            let header = format!("Content-Length: {}\r\n\r\n", response_str.len());
            writer.write_all(header.as_bytes())?;
            writer.write_all(response_str.as_bytes())?;
        }
        StdioMessageFormat::JsonLine => {
            writer.write_all(response_str.as_bytes())?;
            writer.write_all(b"\n")?;
        }
    }

    Ok(())
}

fn debug_log(message: &str) {
    let Some(path) = std::env::var_os("HARMONY_MCP_DEBUG_LOG") else {
        return;
    };

    let timestamp = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => duration.as_millis(),
        Err(_) => 0,
    };

    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "[{timestamp}] {message}");
    }
}

#[cfg(test)]
mod tests {
    use super::{
        handle_request, looks_like_json_message, read_stdio_message, write_stdio_response,
        StdioMessageFormat,
    };
    use crate::types::RequestContext;
    use harmony_memory::store::MemoryStore;
    use std::io::Cursor;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    fn test_store() -> Arc<Mutex<MemoryStore>> {
        let store = MemoryStore::open(Path::new(":memory:")).expect("memory store");
        Arc::new(Mutex::new(store))
    }

    fn test_request_context() -> RequestContext {
        RequestContext::new("local", "127.0.0.1")
    }

    #[test]
    fn reads_content_length_with_extra_headers() {
        let body = "{\"jsonrpc\":\"2.0\",\"id\":0}";
        let payload = format!(
            "Content-Length: {}\r\nContent-Type: application/json\r\n\r\n{}",
            body.len(),
            body
        );
        let mut cursor = Cursor::new(payload.into_bytes());
        let (body, format) = read_stdio_message(&mut cursor).expect("content length");
        assert_eq!(format, StdioMessageFormat::ContentLength);
        assert_eq!(body, "{\"jsonrpc\":\"2.0\",\"id\":0}");
    }

    #[test]
    fn reads_content_length_when_header_order_varies() {
        let body = "{\"jsonrpc\":\"2.0\",\"id\":0,\"method\":\"ping\"}";
        let payload = format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let mut cursor = Cursor::new(payload.into_bytes());
        let (body, format) = read_stdio_message(&mut cursor).expect("content length");
        assert_eq!(format, StdioMessageFormat::ContentLength);
        assert_eq!(body, "{\"jsonrpc\":\"2.0\",\"id\":0,\"method\":\"ping\"}");
    }

    #[test]
    fn reads_json_line_messages() {
        let payload = b"{\"jsonrpc\":\"2.0\",\"id\":0,\"method\":\"initialize\"}\n";
        let mut cursor = Cursor::new(payload.as_slice());
        let (body, format) = read_stdio_message(&mut cursor).expect("json line");
        assert_eq!(format, StdioMessageFormat::JsonLine);
        assert_eq!(body, "{\"jsonrpc\":\"2.0\",\"id\":0,\"method\":\"initialize\"}");
    }

    #[test]
    fn writes_json_line_responses() {
        let mut output = Vec::new();
        write_stdio_response(&mut output, StdioMessageFormat::JsonLine, "{\"jsonrpc\":\"2.0\"}")
            .expect("json line response");
        assert_eq!(String::from_utf8(output).unwrap(), "{\"jsonrpc\":\"2.0\"}\n");
    }

    #[test]
    fn detects_json_messages() {
        assert!(looks_like_json_message("{\"jsonrpc\":\"2.0\"}"));
        assert!(!looks_like_json_message("Content-Length: 42"));
    }

    #[test]
    fn initialize_echoes_requested_protocol_version() {
        let response = handle_request(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"zed","version":"1.0.0"}}}"#,
            &test_store(),
            &test_request_context(),
        )
        .expect("response");

        assert_eq!(
            response["result"]["protocolVersion"].as_str(),
            Some("2025-06-18")
        );
        assert_eq!(
            response["result"]["capabilities"]["tools"]["listChanged"].as_bool(),
            Some(false)
        );
    }

    #[test]
    fn notifications_initialized_does_not_emit_response() {
        let response = handle_request(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#,
            &test_store(),
            &test_request_context(),
        );

        assert!(response.is_none());
    }
}
