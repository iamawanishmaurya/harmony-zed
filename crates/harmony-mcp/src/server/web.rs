use std::net::SocketAddr;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{Method, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use harmony_core::{MemoryNamespace, ResolutionKind};
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

use super::{
    emit_log, emit_memory_added, emit_overlap, machine_snapshots, subscribe_events, upsert_machine,
    AppState, MachineRegistrationPayload, NetworkMode,
};

const DASHBOARD_HTML: &str = include_str!("../../../../dashboard/index.html");
const DASHBOARD_CSS: &str = include_str!("../../../../dashboard/style.css");
const DASHBOARD_JS: &str = include_str!("../../../../dashboard/app.js");

pub async fn serve(addr: SocketAddr, state: AppState) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/", get(index))
        .route("/ws", get(ws_handler))
        .route("/api/status", get(api_status))
        .route("/api/agents", get(api_agents))
        .route("/api/overlaps", get(api_overlaps))
        .route("/api/files", get(api_files))
        .route("/api/memory", get(api_memory))
        .route("/api/logs", get(api_logs))
        .route("/api/config", get(api_config))
        .route("/api/resolve", post(api_resolve))
        .route("/api/machines/register", post(api_register_machine))
        .route("/api/machines/heartbeat", post(api_heartbeat_machine))
        .with_state(state)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                .allow_headers(Any),
        );

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Harmony dashboard listening on http://{}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}

#[derive(Debug, serde::Deserialize)]
struct ResolveRequest {
    overlap_id: String,
    resolution: String,
}

#[derive(Debug, serde::Deserialize)]
struct FileQuery {
    limit: Option<u32>,
    since_seq: Option<i64>,
}

#[derive(Debug, serde::Deserialize)]
struct WsClientMessage {
    #[serde(rename = "type")]
    message_type: String,
    overlap_id: Option<String>,
    resolution: Option<String>,
    content: Option<String>,
    tags: Option<Vec<String>>,
}

async fn index() -> Html<String> {
    Html(
        DASHBOARD_HTML
            .replace("{{CSS}}", DASHBOARD_CSS)
            .replace("{{JS}}", DASHBOARD_JS),
    )
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    let mut events = subscribe_events(&state);

    loop {
        tokio::select! {
            event = events.recv() => {
                match event {
                    Ok(event) => {
                        if socket.send(Message::Text(event.to_string())).await.is_err() {
                            return;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        return;
                    }
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if let Some(reply) = handle_ws_message(&state, &text).await {
                            if socket.send(Message::Text(reply.to_string())).await.is_err() {
                                return;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => return,
                    Some(Err(_)) => return,
                    _ => {}
                }
            }
        }
    }
}

async fn handle_ws_message(state: &AppState, text: &str) -> Option<serde_json::Value> {
    let message: WsClientMessage = serde_json::from_str(text).ok()?;
    match message.message_type.as_str() {
        "resolve" => {
            let overlap_id = message.overlap_id?;
            let resolution = message.resolution?;
            Some(resolve_overlap(state, &overlap_id, &resolution).await)
        }
        "add_memory" => {
            let content = message.content?;
            let tags = message.tags.unwrap_or_default();
            Some(add_memory_record(state, &content, tags).await)
        }
        other => Some(serde_json::json!({
            "type": "error",
            "message": format!("Unsupported dashboard action: {other}")
        })),
    }
}

async fn api_status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let machines = machine_snapshots(&state).await;
    Json(serde_json::json!({
        "mode": match state.mode {
            NetworkMode::Host => "host",
            NetworkMode::Client => "client",
        },
        "machine_name": state.machine_name,
        "machine_ip": state.machine_ip,
        "project_root": state.project_root,
        "db_path": state.db_path,
        "config_path": state.config_path,
        "debug_log_path": state.debug_log_path,
        "host_url": state.host_url,
        "ports": {
            "mcp": state.mcp_port,
            "ipc": state.ipc_port,
            "web": state.web_port,
        },
        "uptime_seconds": state.started_at.elapsed().as_secs(),
        "server_time": Utc::now(),
        "connected_machines": machines,
    }))
}

async fn api_agents(State(state): State<AppState>) -> impl IntoResponse {
    let Some(store) = &state.store else {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(serde_json::json!({
                "error": "Agent listing is only available on the host server."
            })),
        );
    };

    match store.lock().unwrap().get_agents() {
        Ok(agents) => (StatusCode::OK, Json(serde_json::json!({ "agents": agents }))),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        ),
    }
}

async fn api_overlaps(State(state): State<AppState>) -> impl IntoResponse {
    let Some(store) = &state.store else {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(serde_json::json!({
                "error": "Overlap listing is only available on the host server."
            })),
        );
    };

    match store.lock().unwrap().get_all_overlaps() {
        Ok(overlaps) => (StatusCode::OK, Json(serde_json::json!({ "overlaps": overlaps }))),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        ),
    }
}

async fn api_memory(State(state): State<AppState>) -> impl IntoResponse {
    let Some(store) = &state.store else {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(serde_json::json!({
                "error": "Memory listing is only available on the host server."
            })),
        );
    };

    match store
        .lock()
        .unwrap()
        .query_memory_by_tag("decision", MemoryNamespace::Shared, 50)
    {
        Ok(records) => (StatusCode::OK, Json(serde_json::json!({ "records": records }))),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        ),
    }
}

async fn api_files(
    State(state): State<AppState>,
    Query(query): Query<FileQuery>,
) -> impl IntoResponse {
    let Some(store) = &state.store else {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(serde_json::json!({
                "error": "File activity is only available on the host server."
            })),
        );
    };

    let limit = query.limit.unwrap_or(50).clamp(1, 500);
    let result = if let Some(since_seq) = query.since_seq {
        store
            .lock()
            .unwrap()
            .get_file_sync_events_since(since_seq, limit)
    } else {
        store.lock().unwrap().get_recent_file_sync_events(limit)
    };

    match result {
        Ok(events) => (StatusCode::OK, Json(serde_json::json!({ "events": events }))),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        ),
    }
}

async fn api_logs(State(state): State<AppState>) -> impl IntoResponse {
    let content = std::fs::read_to_string(&state.debug_log_path).unwrap_or_default();
    let lines = tail_lines(&content, 500);
    (StatusCode::OK, Json(serde_json::json!({ "lines": lines })))
}

async fn api_config(State(state): State<AppState>) -> impl IntoResponse {
    match std::fs::read_to_string(&state.config_path) {
        Ok(content) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "path": state.config_path,
                "content": content,
            })),
        ),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        ),
    }
}

async fn api_resolve(
    State(state): State<AppState>,
    Json(request): Json<ResolveRequest>,
) -> impl IntoResponse {
    let response = resolve_overlap(&state, &request.overlap_id, &request.resolution).await;
    let status = if response.get("error").is_some() {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::OK
    };
    (status, Json(response))
}

async fn api_register_machine(
    State(state): State<AppState>,
    Json(payload): Json<MachineRegistrationPayload>,
) -> impl IntoResponse {
    let machine = upsert_machine(&state, payload).await;
    emit_log(
        "INFO",
        "network",
        format!("machine registered: {} ({})", machine.name, machine.ip),
    );
    (StatusCode::OK, Json(serde_json::json!({ "machine": machine })))
}

async fn api_heartbeat_machine(
    State(state): State<AppState>,
    Json(payload): Json<MachineRegistrationPayload>,
) -> impl IntoResponse {
    let machine = upsert_machine(&state, payload).await;
    (StatusCode::OK, Json(serde_json::json!({ "machine": machine })))
}

async fn resolve_overlap(
    state: &AppState,
    overlap_id: &str,
    resolution: &str,
) -> serde_json::Value {
    let Some(store) = &state.store else {
        return serde_json::json!({ "error": "Host store unavailable" });
    };

    let overlap_id = match Uuid::parse_str(overlap_id) {
        Ok(id) => id,
        Err(error) => {
            return serde_json::json!({ "error": format!("Invalid overlap id: {error}") });
        }
    };
    let resolution_kind = match parse_resolution_kind(resolution) {
        Some(kind) => kind,
        None => {
            return serde_json::json!({
                "error": format!("Unknown resolution: {resolution}")
            });
        }
    };

    let overlap = {
        let store = store.lock().unwrap();
        if let Err(error) = store.update_overlap_status(
            overlap_id,
            harmony_core::OverlapStatus::Resolved(resolution_kind.clone()),
        ) {
            return serde_json::json!({ "error": error.to_string() });
        }
        store.get_overlap(overlap_id).ok().flatten()
    };

    if let Some(overlap) = overlap {
        emit_log(
            "INFO",
            "overlap",
            format!("resolved {} as {:?}", overlap.file_path, resolution_kind),
        );
        emit_overlap(&overlap);
        serde_json::json!({ "overlap": overlap })
    } else {
        serde_json::json!({ "error": "Overlap not found after update" })
    }
}

async fn add_memory_record(
    state: &AppState,
    content: &str,
    tags: Vec<String>,
) -> serde_json::Value {
    let Some(store) = &state.store else {
        return serde_json::json!({ "error": "Host store unavailable" });
    };

    let record = {
        let store = store.lock().unwrap();
        let id = match store.add_memory(content, tags.clone(), MemoryNamespace::Shared, None, vec![]) {
            Ok(id) => id,
            Err(error) => return serde_json::json!({ "error": error.to_string() }),
        };

        store
            .query_memory_by_tag(
                tags.first().map(String::as_str).unwrap_or("decision"),
                MemoryNamespace::Shared,
                1,
            )
            .ok()
            .and_then(|mut records| records.pop())
            .map(|record| (id, record))
    };

    match record {
        Some((_id, record)) => {
            emit_memory_added(&record);
            emit_log("INFO", "memory", "memory record added from dashboard");
            serde_json::json!({ "record": record })
        }
        None => serde_json::json!({ "ok": true }),
    }
}

fn parse_resolution_kind(value: &str) -> Option<ResolutionKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "accept_a" | "keep_a" | "mine" => Some(ResolutionKind::AcceptA),
        "accept_b" | "keep_b" | "theirs" => Some(ResolutionKind::AcceptB),
        "accept_all" | "all" => Some(ResolutionKind::AcceptAll),
        "manual" => Some(ResolutionKind::Manual),
        "negotiated" | "negotiate" => Some(ResolutionKind::Negotiated),
        _ => None,
    }
}

fn tail_lines(content: &str, max_lines: usize) -> Vec<String> {
    let mut lines: Vec<String> = content.lines().map(|line| line.to_string()).collect();
    if lines.len() > max_lines {
        lines.drain(0..lines.len() - max_lines);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::{parse_resolution_kind, tail_lines};
    use harmony_core::ResolutionKind;

    #[test]
    fn tail_lines_keeps_last_entries() {
        let content = "a\nb\nc\nd\n";
        assert_eq!(tail_lines(content, 2), vec!["c".to_string(), "d".to_string()]);
    }

    #[test]
    fn parse_resolution_supports_dashboard_actions() {
        assert_eq!(parse_resolution_kind("accept_a"), Some(ResolutionKind::AcceptA));
        assert_eq!(parse_resolution_kind("negotiate"), Some(ResolutionKind::Negotiated));
        assert_eq!(parse_resolution_kind("manual"), Some(ResolutionKind::Manual));
    }
}
