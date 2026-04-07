use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use harmony_core::types::FileSyncEvent;
use harmony_memory::store::MemoryStore;
use tower_http::cors::{Any, CorsLayer};

use super::{emit_file_sync_event, emit_log, upsert_machine, AppState, MachineRegistrationPayload};
use crate::types::{RequestContext, MACHINE_IP_HEADER, MACHINE_NAME_HEADER};

pub async fn serve(
    addr: SocketAddr,
    state: AppState,
    store: Arc<Mutex<MemoryStore>>,
) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/mcp", post(handle_mcp))
        .route("/api/machines/register", post(handle_machine_register))
        .route("/api/machines/heartbeat", post(handle_machine_heartbeat))
        .route("/api/filesync/events", get(handle_file_sync_events))
        .route("/api/filesync/push", post(handle_file_sync_push))
        .with_state(HttpState { state, store })
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                .allow_headers(Any),
        );

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("HTTP MCP listening on http://{}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}

#[derive(Clone)]
struct HttpState {
    state: AppState,
    store: Arc<Mutex<MemoryStore>>,
}

#[derive(Debug, serde::Deserialize)]
struct FileSyncQuery {
    since_seq: Option<i64>,
    limit: Option<u32>,
}

#[derive(Debug, serde::Deserialize)]
struct FileSyncPushRequest {
    event: FileSyncEvent,
}

async fn handle_mcp(
    State(http_state): State<HttpState>,
    headers: HeaderMap,
    Json(request): Json<serde_json::Value>,
) -> Response {
    let request_context = RequestContext::new(
        header_value(&headers, MACHINE_NAME_HEADER)
            .unwrap_or_else(|| http_state.state.machine_name.clone()),
        header_value(&headers, MACHINE_IP_HEADER)
            .unwrap_or_else(|| http_state.state.machine_ip.clone()),
    );
    if let Some(tool_name) = request
        .get("params")
        .and_then(|params| params.get("name"))
        .and_then(|value| value.as_str())
    {
        emit_log(
            "INFO",
            "mcp",
            format!(
                "{} <- {} ({})",
                tool_name, request_context.machine_name, request_context.machine_ip
            ),
        );
    }

    let body = match serde_json::to_string(&request) {
        Ok(body) => body,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request.get("id").cloned().unwrap_or(serde_json::Value::Null),
                    "error": {
                        "code": -32700,
                        "message": format!("Invalid JSON payload: {error}")
                    }
                })),
            )
                .into_response();
        }
    };

    tracing::debug!(
        "HTTP MCP request from {} mode={:?}",
        request_context.machine_name,
        http_state.state.mode
    );

    match crate::transport::handle_request(&body, &http_state.store, &request_context) {
        Some(response) => (StatusCode::OK, Json(response)).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

async fn handle_machine_register(
    State(http_state): State<HttpState>,
    Json(payload): Json<MachineRegistrationPayload>,
) -> Response {
    let snapshot = upsert_machine(&http_state.state, payload).await;
    emit_log(
        "INFO",
        "network",
        format!("machine registered via mcp: {} ({})", snapshot.name, snapshot.ip),
    );
    (StatusCode::OK, Json(serde_json::json!({ "machine": snapshot }))).into_response()
}

async fn handle_machine_heartbeat(
    State(http_state): State<HttpState>,
    Json(payload): Json<MachineRegistrationPayload>,
) -> Response {
    let snapshot = upsert_machine(&http_state.state, payload).await;
    (StatusCode::OK, Json(serde_json::json!({ "machine": snapshot }))).into_response()
}

async fn handle_file_sync_events(
    State(http_state): State<HttpState>,
    Query(query): Query<FileSyncQuery>,
) -> Response {
    let since_seq = query.since_seq.unwrap_or(0);
    let limit = query.limit.unwrap_or(256).clamp(1, 1000);

    match http_state
        .store
        .lock()
        .unwrap()
        .get_file_sync_events_since(since_seq, limit)
    {
        Ok(events) => (StatusCode::OK, Json(serde_json::json!({ "events": events }))).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn handle_file_sync_push(
    State(http_state): State<HttpState>,
    Json(request): Json<FileSyncPushRequest>,
) -> Response {
    match http_state.store.lock().unwrap().insert_file_sync_event(&request.event) {
        Ok(event) => {
            emit_log(
                "INFO",
                "sync",
                format!(
                    "received remote {} {} from {}",
                    match &event.change_kind {
                        harmony_core::types::FileSyncChangeKind::Created => "create",
                        harmony_core::types::FileSyncChangeKind::Updated => "update",
                        harmony_core::types::FileSyncChangeKind::Deleted => "delete",
                    },
                    event.relative_path,
                    event.machine_name
                ),
            );
            emit_file_sync_event(&event);
            (StatusCode::OK, Json(serde_json::json!({ "event": event }))).into_response()
        }
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

fn header_value(headers: &HeaderMap, key: &str) -> Option<String> {
    headers
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
