use std::sync::{Arc, Mutex};

use harmony_memory::store::MemoryStore;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use crate::types::{RequestContext, MACHINE_IP_HEADER, MACHINE_NAME_HEADER};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MessageFormat {
    ContentLength,
    JsonLine,
}

pub async fn serve_host(port: u16, store: Arc<Mutex<MemoryStore>>) -> anyhow::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    tracing::info!("Harmony TCP IPC (host) listening on 127.0.0.1:{}", port);

    loop {
        let (socket, peer) = listener.accept().await?;
        let store = store.clone();
        tracing::debug!("Host IPC client connected: {}", peer);

        tokio::spawn(async move {
            let _ = serve_local_client(socket, store).await;
        });
    }
}

pub async fn serve_client_proxy(
    port: u16,
    host_url: String,
    machine_name: String,
    machine_ip: String,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    let client = reqwest::Client::new();
    tracing::info!(
        "Harmony TCP IPC (client proxy) listening on 127.0.0.1:{} -> {}",
        port,
        host_url
    );

    loop {
        let (socket, peer) = listener.accept().await?;
        let client = client.clone();
        let host_mcp_url = host_mcp_endpoint(&host_url);
        let machine_name = machine_name.clone();
        let machine_ip = machine_ip.clone();
        tracing::debug!("Client IPC connection from {}", peer);

        tokio::spawn(async move {
            let _ = serve_proxy_client(socket, client, host_mcp_url, machine_name, machine_ip).await;
        });
    }
}

async fn serve_local_client(
    socket: tokio::net::TcpStream,
    store: Arc<Mutex<MemoryStore>>,
) -> anyhow::Result<()> {
    let (reader_half, mut writer_half) = socket.into_split();
    let mut reader = BufReader::new(reader_half);
    let request_context = RequestContext::local();

    loop {
        let Some((body, format)) = read_message(&mut reader).await? else {
            return Ok(());
        };

        if let Some(response) = crate::transport::handle_request(&body, &store, &request_context) {
            let response_body = serde_json::to_string(&response)?;
            write_message(&mut writer_half, format, &response_body).await?;
        }
    }
}

async fn serve_proxy_client(
    socket: tokio::net::TcpStream,
    client: reqwest::Client,
    host_mcp_url: String,
    machine_name: String,
    machine_ip: String,
) -> anyhow::Result<()> {
    let (reader_half, mut writer_half) = socket.into_split();
    let mut reader = BufReader::new(reader_half);

    loop {
        let Some((body, format)) = read_message(&mut reader).await? else {
            return Ok(());
        };

        let request_value: serde_json::Value = serde_json::from_str(&body)?;
        let response = client
            .post(&host_mcp_url)
            .header(MACHINE_NAME_HEADER, &machine_name)
            .header(MACHINE_IP_HEADER, &machine_ip)
            .json(&request_value)
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::NO_CONTENT {
            continue;
        }

        let response_value: serde_json::Value = response.json().await?;
        let response_body = serde_json::to_string(&response_value)?;
        write_message(&mut writer_half, format, &response_body).await?;
    }
}

async fn read_message<R>(
    reader: &mut BufReader<R>,
) -> anyhow::Result<Option<(String, MessageFormat)>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut first_line = String::new();
    let bytes_read = reader.read_line(&mut first_line).await?;
    if bytes_read == 0 {
        return Ok(None);
    }

    let trimmed = first_line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    if looks_like_json_message(trimmed) {
        return Ok(Some((trimmed.to_string(), MessageFormat::JsonLine)));
    }

    let mut content_length = parse_content_length(trimmed);
    let mut header_line = String::new();

    loop {
        header_line.clear();
        let bytes_read = reader.read_line(&mut header_line).await?;
        if bytes_read == 0 {
            return Err(anyhow::anyhow!("EOF while reading TCP IPC headers"));
        }

        let trimmed = header_line.trim();
        if trimmed.is_empty() {
            let length = content_length.ok_or_else(|| anyhow::anyhow!("Missing Content-Length"))?;
            let mut body = vec![0u8; length];
            reader.read_exact(&mut body).await?;
            let body = String::from_utf8(body)
                .map_err(|error| anyhow::anyhow!("invalid utf-8 body: {error}"))?;
            return Ok(Some((body, MessageFormat::ContentLength)));
        }

        if content_length.is_none() {
            content_length = parse_content_length(trimmed);
        }
    }
}

fn parse_content_length(line: &str) -> Option<usize> {
    let (name, value) = line.split_once(':')?;
    name.trim()
        .eq_ignore_ascii_case("Content-Length")
        .then(|| value.trim().parse().ok())
        .flatten()
}

async fn write_message<W>(
    writer: &mut W,
    format: MessageFormat,
    body: &str,
) -> anyhow::Result<()>
where
    W: AsyncWrite + Unpin,
{
    match format {
        MessageFormat::ContentLength => {
            let header = format!("Content-Length: {}\r\n\r\n", body.len());
            writer.write_all(header.as_bytes()).await?;
            writer.write_all(body.as_bytes()).await?;
        }
        MessageFormat::JsonLine => {
            writer.write_all(body.as_bytes()).await?;
            writer.write_all(b"\n").await?;
        }
    }

    writer.flush().await?;
    Ok(())
}

fn host_mcp_endpoint(host_url: &str) -> String {
    let trimmed = host_url.trim_end_matches('/');
    if trimmed.ends_with("/mcp") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/mcp")
    }
}

fn looks_like_json_message(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('{') && trimmed.ends_with('}')
}

#[cfg(test)]
mod tests {
    use super::{host_mcp_endpoint, parse_content_length};

    #[test]
    fn parses_content_length_header_case_insensitively() {
        assert_eq!(parse_content_length("Content-Length: 42"), Some(42));
        assert_eq!(parse_content_length("content-length: 12"), Some(12));
        assert_eq!(parse_content_length("Content-Type: application/json"), None);
    }

    #[test]
    fn host_mcp_endpoint_avoids_duplicate_suffix() {
        assert_eq!(
            host_mcp_endpoint("http://127.0.0.1:4231"),
            "http://127.0.0.1:4231/mcp"
        );
        assert_eq!(
            host_mcp_endpoint("http://127.0.0.1:4231/mcp"),
            "http://127.0.0.1:4231/mcp"
        );
    }
}
