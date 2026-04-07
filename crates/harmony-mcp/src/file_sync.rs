use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use anyhow::Context;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use chrono::Utc;
use harmony_core::types::{ActorId, FileSyncChangeKind, FileSyncEntryKind, FileSyncEvent};
use harmony_core::HarmonyConfig;
use harmony_memory::store::MemoryStore;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::server::{emit_file_sync_event, emit_log, AppState};

const AUTO_SYNC_STATE_VERSION: u32 = 1;
const AUTO_SYNC_FETCH_LIMIT: u32 = 256;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AutoSyncState {
    version: u32,
    last_remote_seq: i64,
    entries: BTreeMap<String, LocalEntryState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct LocalEntryState {
    entry_kind: FileSyncEntryKind,
    modified_unix_ms: u64,
    size_bytes: u64,
}

enum SyncBackend {
    Host {
        store: Arc<Mutex<MemoryStore>>,
    },
    Client {
        client: Client,
        host_url: String,
    },
}

pub(crate) fn spawn_host_auto_sync(
    state: AppState,
    store: Arc<Mutex<MemoryStore>>,
    config: HarmonyConfig,
) {
    if !config.network.auto_sync {
        emit_log("INFO", "sync", "automatic file sync disabled in config");
        return;
    }

    tokio::spawn(async move {
        run_sync_loop(
            state,
            SyncBackend::Host { store },
            config.network.sync_interval_seconds.max(1),
            config.network.max_sync_file_bytes.max(1024),
            config.human.actor_id,
        )
        .await;
    });
}

pub(crate) fn spawn_client_auto_sync(
    state: AppState,
    host_url: String,
    config: HarmonyConfig,
) {
    if !config.network.auto_sync {
        emit_log("INFO", "sync", "automatic file sync disabled in config");
        return;
    }

    tokio::spawn(async move {
        run_sync_loop(
            state,
            SyncBackend::Client {
                client: Client::new(),
                host_url,
            },
            config.network.sync_interval_seconds.max(1),
            config.network.max_sync_file_bytes.max(1024),
            config.human.actor_id,
        )
        .await;
    });
}

async fn run_sync_loop(
    state: AppState,
    backend: SyncBackend,
    interval_seconds: u64,
    max_sync_file_bytes: u64,
    actor_id: String,
) {
    let state_path = state
        .project_root
        .join(".harmony")
        .join("network-sync-state.json");
    let mut sync_state = match AutoSyncState::load(&state_path) {
        Ok(state) => state,
        Err(error) => {
            emit_log(
                "WARN",
                "sync",
                format!("failed to load auto sync state: {error}; starting fresh"),
            );
            AutoSyncState::default()
        }
    };

    emit_log(
        "INFO",
        "sync",
        format!(
            "automatic file sync enabled (interval={}s, max_file={} bytes)",
            interval_seconds, max_sync_file_bytes
        ),
    );

    loop {
        if let Err(error) = sync_once(
            &state,
            &backend,
            &actor_id,
            max_sync_file_bytes,
            &mut sync_state,
            &state_path,
        )
        .await
        {
            emit_log("WARN", "sync", format!("auto sync cycle failed: {error}"));
        }

        tokio::time::sleep(Duration::from_secs(interval_seconds)).await;
    }
}

async fn sync_once(
    state: &AppState,
    backend: &SyncBackend,
    actor_id: &str,
    max_sync_file_bytes: u64,
    sync_state: &mut AutoSyncState,
    state_path: &Path,
) -> anyhow::Result<()> {
    let current_entries = collect_project_entries(&state.project_root)?;
    let local_events = build_local_events(
        &state.project_root,
        &sync_state.entries,
        &current_entries,
        actor_id,
        &state.machine_name,
        &state.machine_ip,
        max_sync_file_bytes,
    )?;

    for event in local_events {
        let inserted = push_local_event(backend, event).await?;
        sync_state.last_remote_seq = sync_state.last_remote_seq.max(inserted.seq);
    }

    sync_state.entries = current_entries;

    let remote_events = fetch_remote_events(backend, sync_state.last_remote_seq).await?;
    for event in remote_events {
        sync_state.last_remote_seq = sync_state.last_remote_seq.max(event.seq);
        if is_self_event(state, &event) {
            continue;
        }

        apply_remote_event(&state.project_root, &event, &mut sync_state.entries)
            .with_context(|| format!("failed to apply remote change for {}", event.relative_path))?;
    }

    sync_state.save(state_path)?;
    Ok(())
}

fn is_self_event(state: &AppState, event: &FileSyncEvent) -> bool {
    event.machine_name.eq_ignore_ascii_case(&state.machine_name) && event.machine_ip == state.machine_ip
}

async fn push_local_event(
    backend: &SyncBackend,
    event: FileSyncEvent,
) -> anyhow::Result<FileSyncEvent> {
    match backend {
        SyncBackend::Host { store } => {
            let inserted = {
                let store = store.lock().unwrap();
                store.insert_file_sync_event(&event)?
            };
            emit_file_sync_event(&inserted);
            emit_log(
                "INFO",
                "sync",
                format!(
                    "{} {} {}",
                    change_label(&inserted.change_kind),
                    kind_label(&inserted.entry_kind),
                    inserted.relative_path
                ),
            );
            Ok(inserted)
        }
        SyncBackend::Client { client, host_url } => {
            let url = host_api_endpoint(host_url, "/api/filesync/push");
            let response = client
                .post(url)
                .json(&serde_json::json!({ "event": event }))
                .send()
                .await?;
            if !response.status().is_success() {
                anyhow::bail!("host rejected file sync push with HTTP {}", response.status());
            }
            let payload: serde_json::Value = response.json().await?;
            let event = payload
                .get("event")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("missing event in sync push response"))?;
            Ok(serde_json::from_value(event)?)
        }
    }
}

async fn fetch_remote_events(
    backend: &SyncBackend,
    since_seq: i64,
) -> anyhow::Result<Vec<FileSyncEvent>> {
    match backend {
        SyncBackend::Host { store } => {
            let store = store.lock().unwrap();
            store.get_file_sync_events_since(since_seq, AUTO_SYNC_FETCH_LIMIT)
        }
        SyncBackend::Client { client, host_url } => {
            let url = format!(
                "{}?since_seq={}&limit={}",
                host_api_endpoint(host_url, "/api/filesync/events"),
                since_seq,
                AUTO_SYNC_FETCH_LIMIT
            );
            let response = client.get(url).send().await?;
            if !response.status().is_success() {
                anyhow::bail!("host rejected file sync fetch with HTTP {}", response.status());
            }
            let payload: serde_json::Value = response.json().await?;
            let events = payload
                .get("events")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([]));
            Ok(serde_json::from_value(events)?)
        }
    }
}

fn build_local_events(
    project_root: &Path,
    previous: &BTreeMap<String, LocalEntryState>,
    current: &BTreeMap<String, LocalEntryState>,
    actor_id: &str,
    machine_name: &str,
    machine_ip: &str,
    max_sync_file_bytes: u64,
) -> anyhow::Result<Vec<FileSyncEvent>> {
    let all_paths = previous
        .keys()
        .chain(current.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut events = Vec::new();

    for relative_path in all_paths {
        let old_entry = previous.get(&relative_path);
        let new_entry = current.get(&relative_path);

        match (old_entry, new_entry) {
            (None, Some(entry)) => {
                events.push(build_event_for_current_path(
                    project_root,
                    &relative_path,
                    entry,
                    FileSyncChangeKind::Created,
                    actor_id,
                    machine_name,
                    machine_ip,
                    max_sync_file_bytes,
                )?);
            }
            (Some(entry), None) => {
                events.push(FileSyncEvent {
                    seq: 0,
                    id: Uuid::new_v4(),
                    relative_path: relative_path.clone(),
                    entry_kind: entry.entry_kind.clone(),
                    change_kind: FileSyncChangeKind::Deleted,
                    content_base64: None,
                    content_sha256: None,
                    size_bytes: 0,
                    actor_id: ActorId(actor_id.to_string()),
                    machine_name: machine_name.to_string(),
                    machine_ip: machine_ip.to_string(),
                    detected_at: Utc::now(),
                    impact_summary: impact_summary(
                        &relative_path,
                        &entry.entry_kind,
                        &FileSyncChangeKind::Deleted,
                    ),
                });
            }
            (Some(old_entry), Some(new_entry)) if old_entry.entry_kind != new_entry.entry_kind => {
                events.push(FileSyncEvent {
                    seq: 0,
                    id: Uuid::new_v4(),
                    relative_path: relative_path.clone(),
                    entry_kind: old_entry.entry_kind.clone(),
                    change_kind: FileSyncChangeKind::Deleted,
                    content_base64: None,
                    content_sha256: None,
                    size_bytes: 0,
                    actor_id: ActorId(actor_id.to_string()),
                    machine_name: machine_name.to_string(),
                    machine_ip: machine_ip.to_string(),
                    detected_at: Utc::now(),
                    impact_summary: impact_summary(
                        &relative_path,
                        &old_entry.entry_kind,
                        &FileSyncChangeKind::Deleted,
                    ),
                });
                events.push(build_event_for_current_path(
                    project_root,
                    &relative_path,
                    new_entry,
                    FileSyncChangeKind::Created,
                    actor_id,
                    machine_name,
                    machine_ip,
                    max_sync_file_bytes,
                )?);
            }
            (Some(old_entry), Some(new_entry)) => {
                if old_entry != new_entry && new_entry.entry_kind == FileSyncEntryKind::File {
                    events.push(build_event_for_current_path(
                        project_root,
                        &relative_path,
                        new_entry,
                        FileSyncChangeKind::Updated,
                        actor_id,
                        machine_name,
                        machine_ip,
                        max_sync_file_bytes,
                    )?);
                }
            }
            (None, None) => {}
        }
    }

    Ok(events)
}

fn build_event_for_current_path(
    project_root: &Path,
    relative_path: &str,
    entry: &LocalEntryState,
    change_kind: FileSyncChangeKind,
    actor_id: &str,
    machine_name: &str,
    machine_ip: &str,
    max_sync_file_bytes: u64,
) -> anyhow::Result<FileSyncEvent> {
    let mut content_base64 = None;
    let mut content_sha256 = None;
    let mut size_bytes = entry.size_bytes;

    if entry.entry_kind == FileSyncEntryKind::File {
        let full_path = to_project_path(project_root, relative_path);
        let bytes = fs::read(&full_path)?;
        size_bytes = bytes.len() as u64;
        if size_bytes > max_sync_file_bytes {
            anyhow::bail!(
                "file {} exceeds max_sync_file_bytes ({} > {})",
                relative_path,
                size_bytes,
                max_sync_file_bytes
            );
        }

        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        content_sha256 = Some(format!("{:x}", hasher.finalize()));
        content_base64 = Some(BASE64.encode(bytes));
    }

    Ok(FileSyncEvent {
        seq: 0,
        id: Uuid::new_v4(),
        relative_path: relative_path.to_string(),
        entry_kind: entry.entry_kind.clone(),
        change_kind: change_kind.clone(),
        content_base64,
        content_sha256,
        size_bytes,
        actor_id: ActorId(actor_id.to_string()),
        machine_name: machine_name.to_string(),
        machine_ip: machine_ip.to_string(),
        detected_at: Utc::now(),
        impact_summary: impact_summary(relative_path, &entry.entry_kind, &change_kind),
    })
}

fn collect_project_entries(project_root: &Path) -> anyhow::Result<BTreeMap<String, LocalEntryState>> {
    let mut entries = BTreeMap::new();
    collect_entries_recursive(project_root, project_root, &mut entries)?;
    Ok(entries)
}

fn collect_entries_recursive(
    project_root: &Path,
    current_dir: &Path,
    entries: &mut BTreeMap<String, LocalEntryState>,
) -> anyhow::Result<()> {
    for entry in fs::read_dir(current_dir)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = entry.metadata()?;
        let file_name = entry.file_name().to_string_lossy().to_string();

        if metadata.is_dir() {
            if should_skip_directory(&file_name) {
                continue;
            }

            let relative = relative_project_path(project_root, &path)?;
            if !relative.is_empty() {
                entries.insert(
                    relative,
                    LocalEntryState {
                        entry_kind: FileSyncEntryKind::Directory,
                        modified_unix_ms: modified_unix_ms(&metadata)?,
                        size_bytes: 0,
                    },
                );
            }
            collect_entries_recursive(project_root, &path, entries)?;
            continue;
        }

        if !metadata.is_file() {
            continue;
        }

        let relative = relative_project_path(project_root, &path)?;
        if should_skip_path(&relative) {
            continue;
        }

        entries.insert(
            relative,
            LocalEntryState {
                entry_kind: FileSyncEntryKind::File,
                modified_unix_ms: modified_unix_ms(&metadata)?,
                size_bytes: metadata.len(),
            },
        );
    }

    Ok(())
}

fn apply_remote_event(
    project_root: &Path,
    event: &FileSyncEvent,
    entries: &mut BTreeMap<String, LocalEntryState>,
) -> anyhow::Result<()> {
    if should_skip_path(&event.relative_path) {
        return Ok(());
    }

    let full_path = to_project_path(project_root, &event.relative_path);
    match event.change_kind {
        FileSyncChangeKind::Created | FileSyncChangeKind::Updated => match event.entry_kind {
            FileSyncEntryKind::Directory => {
                fs::create_dir_all(&full_path)?;
                let metadata = fs::metadata(&full_path)?;
                entries.insert(
                    event.relative_path.clone(),
                    LocalEntryState {
                        entry_kind: FileSyncEntryKind::Directory,
                        modified_unix_ms: modified_unix_ms(&metadata)?,
                        size_bytes: 0,
                    },
                );
            }
            FileSyncEntryKind::File => {
                if let Some(parent) = full_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let content = event
                    .content_base64
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("missing file content for {}", event.relative_path))?;
                let bytes = BASE64
                    .decode(content)
                    .with_context(|| format!("invalid base64 content for {}", event.relative_path))?;
                fs::write(&full_path, bytes)?;
                let metadata = fs::metadata(&full_path)?;
                entries.insert(
                    event.relative_path.clone(),
                    LocalEntryState {
                        entry_kind: FileSyncEntryKind::File,
                        modified_unix_ms: modified_unix_ms(&metadata)?,
                        size_bytes: metadata.len(),
                    },
                );
            }
        },
        FileSyncChangeKind::Deleted => match event.entry_kind {
            FileSyncEntryKind::File => {
                if full_path.exists() {
                    let _ = fs::remove_file(&full_path);
                }
                entries.remove(&event.relative_path);
            }
            FileSyncEntryKind::Directory => {
                if full_path.exists() {
                    let _ = fs::remove_dir_all(&full_path);
                }
                let prefix = format!("{}/", event.relative_path.trim_end_matches('/'));
                entries.retain(|path, _| path != &event.relative_path && !path.starts_with(&prefix));
            }
        },
    }

    Ok(())
}

fn relative_project_path(project_root: &Path, full_path: &Path) -> anyhow::Result<String> {
    let absolute_root = absolutize(project_root);
    let absolute_path = absolutize(full_path);
    let relative = absolute_path
        .strip_prefix(&absolute_root)
        .map_err(|_| anyhow::anyhow!("path is outside the project root"))?;
    Ok(normalize_relative_path(relative))
}

fn normalize_relative_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn to_project_path(project_root: &Path, relative_path: &str) -> PathBuf {
    let mut full = absolutize(project_root);
    for part in relative_path.split('/') {
        if !part.is_empty() {
            full.push(part);
        }
    }
    full
}

fn should_skip_directory(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".harmony"
            | "target"
            | "node_modules"
            | "dist"
            | "build"
            | "coverage"
            | ".next"
            | ".zed"
            | ".idea"
            | "out"
    )
}

fn should_skip_path(relative_path: &str) -> bool {
    let normalized = relative_path.replace('\\', "/");
    matches!(normalized.as_str(), "" | ".harmony" | ".git")
        || normalized.starts_with(".harmony/")
        || normalized.starts_with(".git/")
}

fn modified_unix_ms(metadata: &fs::Metadata) -> anyhow::Result<u64> {
    let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
    Ok(modified
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64)
}

fn impact_summary(
    relative_path: &str,
    entry_kind: &FileSyncEntryKind,
    change_kind: &FileSyncChangeKind,
) -> String {
    let area = if relative_path.starts_with("src/") || relative_path.starts_with("app/") {
        "application code"
    } else if relative_path.starts_with("tests/") || relative_path.starts_with("test/") {
        "test coverage"
    } else if relative_path.starts_with("docs/") || relative_path.eq_ignore_ascii_case("README.md")
    {
        "project documentation"
    } else {
        "the shared project"
    };

    match (entry_kind, change_kind) {
        (FileSyncEntryKind::Directory, FileSyncChangeKind::Created) => format!(
            "Creates a new folder in {area}. Connected laptops will receive the directory automatically."
        ),
        (FileSyncEntryKind::Directory, FileSyncChangeKind::Deleted) => format!(
            "Removes a shared folder from {area}. Connected laptops will mirror that deletion."
        ),
        (FileSyncEntryKind::Directory, FileSyncChangeKind::Updated) => format!(
            "Refreshes a shared folder entry in {area} so connected laptops stay aligned."
        ),
        (FileSyncEntryKind::File, FileSyncChangeKind::Created) => format!(
            "Adds a new file in {area}. Connected laptops will receive the new content automatically."
        ),
        (FileSyncEntryKind::File, FileSyncChangeKind::Updated) => format!(
            "Updates a shared file in {area}. Connected laptops will pick up the latest content automatically."
        ),
        (FileSyncEntryKind::File, FileSyncChangeKind::Deleted) => format!(
            "Removes a shared file from {area}. Connected laptops will mirror that deletion."
        ),
    }
}

fn change_label(change_kind: &FileSyncChangeKind) -> &'static str {
    match change_kind {
        FileSyncChangeKind::Created => "created",
        FileSyncChangeKind::Updated => "updated",
        FileSyncChangeKind::Deleted => "deleted",
    }
}

fn kind_label(entry_kind: &FileSyncEntryKind) -> &'static str {
    match entry_kind {
        FileSyncEntryKind::File => "file",
        FileSyncEntryKind::Directory => "folder",
    }
}

fn host_api_endpoint(host_url: &str, path: &str) -> String {
    let trimmed = host_url.trim_end_matches('/');
    let base = trimmed.strip_suffix("/mcp").unwrap_or(trimmed);
    format!("{base}{path}")
}

fn absolutize(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

impl Default for AutoSyncState {
    fn default() -> Self {
        Self {
            version: AUTO_SYNC_STATE_VERSION,
            last_remote_seq: 0,
            entries: BTreeMap::new(),
        }
    }
}

impl AutoSyncState {
    fn load(state_path: &Path) -> anyhow::Result<Self> {
        if !state_path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(state_path)?;
        let mut state: AutoSyncState = serde_json::from_str(&content)?;
        if state.version == 0 {
            state.version = AUTO_SYNC_STATE_VERSION;
        }
        Ok(state)
    }

    fn save(&self, state_path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = state_path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(state_path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impact_summary_mentions_created_folder() {
        let summary = impact_summary(
            "src/components",
            &FileSyncEntryKind::Directory,
            &FileSyncChangeKind::Created,
        );
        assert!(summary.contains("folder"));
        assert!(summary.contains("Connected laptops"));
    }

    #[test]
    fn build_local_events_detects_created_directory_and_file() {
        let previous = BTreeMap::new();
        let mut current = BTreeMap::new();
        current.insert(
            "src".to_string(),
            LocalEntryState {
                entry_kind: FileSyncEntryKind::Directory,
                modified_unix_ms: 1,
                size_bytes: 0,
            },
        );
        current.insert(
            "src/demo.txt".to_string(),
            LocalEntryState {
                entry_kind: FileSyncEntryKind::File,
                modified_unix_ms: 2,
                size_bytes: 5,
            },
        );

        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("src")).unwrap();
        fs::write(temp.path().join("src").join("demo.txt"), b"hello").unwrap();

        let events = build_local_events(
            temp.path(),
            &previous,
            &current,
            "human:water",
            "water",
            "127.0.0.1",
            1024,
        )
        .unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].change_kind, FileSyncChangeKind::Created);
        assert_eq!(events[1].entry_kind, FileSyncEntryKind::File);
    }
}
