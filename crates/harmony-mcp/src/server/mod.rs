pub mod ipc_tcp;
pub mod mcp_http;
pub mod web;

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use chrono::{DateTime, Utc};
use harmony_core::types::{Agent, MemoryRecord, OverlapEvent};
use harmony_memory::store::MemoryStore;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, RwLock};

const EVENT_BUFFER_SIZE: usize = 512;
const MACHINE_OFFLINE_AFTER_SECS: i64 = 10;
const HEARTBEAT_INTERVAL_SECS: u64 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkMode {
    Host,
    Client,
}

#[derive(Debug)]
pub struct RuntimeState {
    pub events: broadcast::Sender<serde_json::Value>,
    pub machines: RwLock<HashMap<String, ConnectedMachine>>,
}

impl RuntimeState {
    pub fn new() -> Arc<Self> {
        let (events, _) = broadcast::channel(EVENT_BUFFER_SIZE);
        Arc::new(Self {
            events,
            machines: RwLock::new(HashMap::new()),
        })
    }
}

#[derive(Clone)]
pub struct AppState {
    pub store: Option<Arc<Mutex<MemoryStore>>>,
    pub started_at: Instant,
    pub project_root: PathBuf,
    pub db_path: Option<PathBuf>,
    pub config_path: PathBuf,
    pub debug_log_path: PathBuf,
    pub machine_name: String,
    pub machine_ip: String,
    pub mode: NetworkMode,
    pub mcp_port: u16,
    pub ipc_port: u16,
    pub web_port: u16,
    pub host_url: Option<String>,
    pub runtime: Arc<RuntimeState>,
}

pub struct HostRuntimeConfig {
    pub state: AppState,
    pub store: Arc<Mutex<MemoryStore>>,
}

pub struct ClientRuntimeConfig {
    pub state: AppState,
    pub host_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineRegistrationPayload {
    pub machine_name: String,
    pub machine_ip: String,
    pub role: String,
    pub host_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectedMachine {
    pub name: String,
    pub ip: String,
    pub role: String,
    pub host_url: Option<String>,
    pub registered_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectedMachineSnapshot {
    pub name: String,
    pub ip: String,
    pub role: String,
    pub host_url: Option<String>,
    pub status: String,
    pub registered_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub agent_count: usize,
}

static GLOBAL_EVENT_SENDER: OnceLock<std::sync::Mutex<Option<broadcast::Sender<serde_json::Value>>>> =
    OnceLock::new();

pub async fn run_host(config: HostRuntimeConfig) -> anyhow::Result<()> {
    let mcp_addr = std::net::SocketAddr::from(([0, 0, 0, 0], config.state.mcp_port));
    let web_addr = std::net::SocketAddr::from(([0, 0, 0, 0], config.state.web_port));

    set_global_event_sender(config.state.runtime.events.clone());
    upsert_machine(
        &config.state,
        MachineRegistrationPayload {
            machine_name: config.state.machine_name.clone(),
            machine_ip: config.state.machine_ip.clone(),
            role: "host".to_string(),
            host_url: config.state.host_url.clone(),
        },
    )
    .await;

    tracing::info!(
        "Harmony host mode starting: mcp=http://{}:{}, ipc=127.0.0.1:{}, web=http://{}:{}",
        config.state.machine_ip,
        config.state.mcp_port,
        config.state.ipc_port,
        config.state.machine_ip,
        config.state.web_port
    );
    emit_log(
        "INFO",
        "network",
        format!(
            "host online at http://{}:{} (dashboard http://{}:{})",
            config.state.machine_ip,
            config.state.mcp_port,
            config.state.machine_ip,
            config.state.web_port
        ),
    );
    let state = config.state.clone();
    let store = config.store.clone();
    tokio::try_join!(
        mcp_http::serve(mcp_addr, state.clone(), store.clone()),
        ipc_tcp::serve_host(config.state.ipc_port, store),
        web::serve(web_addr, state),
    )?;

    clear_global_event_sender();
    Ok(())
}

pub async fn run_client(config: ClientRuntimeConfig) -> anyhow::Result<()> {
    tracing::info!(
        "Harmony client mode starting: host_url={}, ipc=127.0.0.1:{}",
        config.host_url,
        config.state.ipc_port
    );
    let heartbeat_state = config.state.clone();
    let heartbeat_url = config.host_url.clone();
    tokio::spawn(async move {
        let _ = heartbeat_loop(heartbeat_state, heartbeat_url).await;
    });

    ipc_tcp::serve_client_proxy(
        config.state.ipc_port,
        config.host_url,
        config.state.machine_name,
        config.state.machine_ip,
    )
    .await
}

pub fn detect_machine_ip() -> String {
    match local_ip_address::local_ip() {
        Ok(IpAddr::V4(ip)) => ip.to_string(),
        Ok(IpAddr::V6(ip)) => ip.to_string(),
        Err(_) => "127.0.0.1".to_string(),
    }
}

pub async fn upsert_machine(
    state: &AppState,
    payload: MachineRegistrationPayload,
) -> ConnectedMachineSnapshot {
    let now = Utc::now();
    let machine = {
        let mut machines = state.runtime.machines.write().await;
        let key = machine_key(&payload.machine_name, &payload.machine_ip);
        let entry = machines.entry(key).or_insert_with(|| ConnectedMachine {
            name: payload.machine_name.clone(),
            ip: payload.machine_ip.clone(),
            role: payload.role.clone(),
            host_url: payload.host_url.clone(),
            registered_at: now,
            last_seen: now,
        });
        entry.name = payload.machine_name;
        entry.ip = payload.machine_ip;
        entry.role = payload.role;
        entry.host_url = payload.host_url;
        entry.last_seen = now;
        entry.clone()
    };

    let snapshot = snapshot_machine(state, &machine, agent_count_for_machine(state, &machine));
    emit_machine_update(&snapshot);
    snapshot
}

pub async fn machine_snapshots(state: &AppState) -> Vec<ConnectedMachineSnapshot> {
    let machines = {
        let machines = state.runtime.machines.read().await;
        machines.values().cloned().collect::<Vec<_>>()
    };

    let mut snapshots = machines
        .into_iter()
        .map(|machine| {
            let agent_count = agent_count_for_machine(state, &machine);
            snapshot_machine(state, &machine, agent_count)
        })
        .collect::<Vec<_>>();

    snapshots.sort_by(|a, b| a.name.cmp(&b.name).then(a.ip.cmp(&b.ip)));
    snapshots
}

pub fn subscribe_events(state: &AppState) -> broadcast::Receiver<serde_json::Value> {
    state.runtime.events.subscribe()
}

pub fn emit_log(level: &str, module: &str, message: impl Into<String>) {
    broadcast_event(serde_json::json!({
        "type": "log",
        "level": level,
        "module": module,
        "msg": message.into(),
        "ts": Utc::now().format("%H:%M:%S").to_string(),
    }));
}

pub fn emit_overlap(overlap: &OverlapEvent) {
    broadcast_event(serde_json::json!({
        "type": "overlap",
        "data": overlap,
    }));
}

pub fn emit_agent_update(agent: &Agent) {
    broadcast_event(serde_json::json!({
        "type": "agent_update",
        "data": agent,
    }));
}

pub fn emit_memory_added(record: &MemoryRecord) {
    broadcast_event(serde_json::json!({
        "type": "memory_added",
        "data": record,
    }));
}

pub fn emit_machine_update(machine: &ConnectedMachineSnapshot) {
    broadcast_event(serde_json::json!({
        "type": "machine_update",
        "data": machine,
    }));
}

pub fn broadcast_event(event: serde_json::Value) {
    if let Some(sender) = global_event_sender() {
        let _ = sender.send(event);
    }
}

pub fn machine_key(name: &str, ip: &str) -> String {
    format!(
        "{}|{}",
        name.trim().to_ascii_lowercase(),
        ip.trim().to_ascii_lowercase()
    )
}

fn global_event_slot(
) -> &'static std::sync::Mutex<Option<broadcast::Sender<serde_json::Value>>> {
    GLOBAL_EVENT_SENDER.get_or_init(|| std::sync::Mutex::new(None))
}

fn set_global_event_sender(sender: broadcast::Sender<serde_json::Value>) {
    *global_event_slot().lock().unwrap() = Some(sender);
}

fn clear_global_event_sender() {
    *global_event_slot().lock().unwrap() = None;
}

fn global_event_sender() -> Option<broadcast::Sender<serde_json::Value>> {
    global_event_slot().lock().unwrap().as_ref().cloned()
}

fn snapshot_machine(
    _state: &AppState,
    machine: &ConnectedMachine,
    agent_count: usize,
) -> ConnectedMachineSnapshot {
    ConnectedMachineSnapshot {
        name: machine.name.clone(),
        ip: machine.ip.clone(),
        role: machine.role.clone(),
        host_url: machine.host_url.clone(),
        status: if machine_is_online(machine) {
            "online".to_string()
        } else {
            "offline".to_string()
        },
        registered_at: machine.registered_at,
        last_seen: machine.last_seen,
        agent_count,
    }
}

fn agent_count_for_machine(state: &AppState, machine: &ConnectedMachine) -> usize {
    state
        .store
        .as_ref()
        .and_then(|store| store.lock().ok())
        .and_then(|store| store.get_agents().ok())
        .map(|agents| {
            agents
                .into_iter()
                .filter(|agent| {
                    agent.machine_name.eq_ignore_ascii_case(&machine.name)
                        && agent.machine_ip == machine.ip
                })
                .count()
        })
        .unwrap_or(0)
}

fn machine_is_online(machine: &ConnectedMachine) -> bool {
    (Utc::now() - machine.last_seen).num_seconds() <= MACHINE_OFFLINE_AFTER_SECS
}

async fn heartbeat_loop(state: AppState, host_url: String) -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    let payload = MachineRegistrationPayload {
        machine_name: state.machine_name.clone(),
        machine_ip: state.machine_ip.clone(),
        role: "client".to_string(),
        host_url: Some(host_url.clone()),
    };
    let register_url = host_api_endpoint(&host_url, "/api/machines/register");
    let heartbeat_url = host_api_endpoint(&host_url, "/api/machines/heartbeat");
    let mut registered = false;

    loop {
        let target_url = if registered {
            heartbeat_url.as_str()
        } else {
            register_url.as_str()
        };

        match client.post(target_url).json(&payload).send().await {
            Ok(response) if response.status().is_success() => {
                if !registered {
                    tracing::info!(
                        "Registered Harmony client {} at {}",
                        state.machine_name,
                        host_url
                    );
                }
                registered = true;
            }
            Ok(response) => {
                registered = false;
                tracing::warn!(
                    "Harmony client heartbeat received HTTP {} from {}",
                    response.status(),
                    target_url
                );
            }
            Err(error) => {
                registered = false;
                tracing::warn!(
                    "Harmony client heartbeat failed against {}: {}",
                    target_url,
                    error
                );
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(HEARTBEAT_INTERVAL_SECS)).await;
    }
}

fn host_api_endpoint(host_url: &str, path: &str) -> String {
    let trimmed = host_url.trim_end_matches('/');
    let base = trimmed.strip_suffix("/mcp").unwrap_or(trimmed);
    format!("{base}{path}")
}
