use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use harmony_core::{HarmonyConfig, MemoryNamespace};
use harmony_core::types::ProvenanceTag;
use harmony_memory::store::MemoryStore;
use crate::server::{emit_agent_update, emit_log, emit_overlap};
use crate::tracking::{
    canonical_actor_id, default_machine_ip, default_machine_name, normalize_file_path,
    record_change, synthetic_diff_for_content, RecordChangeArgs,
};
use crate::types::RequestContext;
use uuid::Uuid;

/// List all available MCP tools.
pub fn list_tools() -> serde_json::Value {
    serde_json::json!({
        "tools": [
            {
                "name": "harmony_pulse",
                "description": "Return the current Harmony status for this project, including registered agents and pending overlaps.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "harmony_dashboard",
                "description": "Return the live Harmony dashboard URL for this project so you can open it in a browser from Zed Agent.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "harmony_sync",
                "description": "Register recent or explicit assistant-edited files with Harmony so Pulse can see agent activity and overlaps.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "files": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Optional project-relative file paths to sync. If omitted, Harmony scans recent text files."
                        },
                        "actor_id": {
                            "type": "string",
                            "description": "Actor identifier to register for the synced edits.",
                            "default": "agent:zed-assistant"
                        },
                        "since_seconds": {
                            "type": "integer",
                            "description": "How far back to scan when files are omitted.",
                            "default": 900
                        },
                        "task_prompt": {
                            "type": "string",
                            "description": "Optional task note stored with the synced edits."
                        }
                    }
                }
            },
            {
                "name": "report_file_edit",
                "description": "Record a file edit after an assistant changes a file. Call this immediately after creating or editing a file so Harmony can track the change and detect future overlaps.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "actor_id": {
                            "type": "string",
                            "description": "Actor identifier like 'agent:copilot' or 'human:water'."
                        },
                        "file_path": {
                            "type": "string",
                            "description": "Project-relative file path, for example 'src/app.ts' or 'test2.txt'."
                        },
                        "content": {
                            "type": "string",
                            "description": "The new file content or the edited snippet. Used to synthesize a basic diff when diff_unified is omitted."
                        },
                        "start_line": {
                            "type": "integer",
                            "description": "0-indexed starting line for the edit. Defaults to 0."
                        },
                        "end_line": {
                            "type": "integer",
                            "description": "0-indexed ending line for the edit. Defaults to cover the provided content."
                        },
                        "task_prompt": {
                            "type": "string",
                            "description": "Short note describing why the edit was made."
                        },
                        "diff_unified": {
                            "type": "string",
                            "description": "Optional unified diff. If omitted, Harmony builds a simple synthetic diff from content."
                        }
                    },
                    "required": ["actor_id", "file_path"]
                }
            },
            {
                "name": "query_memory",
                "description": "Semantic search for team memory. Returns relevant decisions, notes, and context.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Semantic search query. E.g. 'why did we reject Redis caching'"
                        },
                        "namespace": {
                            "type": "string",
                            "description": "Memory namespace. Use 'shared' for team memory.",
                            "default": "shared"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Max results to return",
                            "default": 5,
                            "minimum": 1,
                            "maximum": 20
                        }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "add_memory",
                "description": "Store a memory record for the team. Be specific and self-contained.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "content": {
                            "type": "string",
                            "description": "Memory content to store. Be specific and self-contained."
                        },
                        "tags": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Tags for filtering. E.g. ['decision', 'rejected', 'auth', 'redis']"
                        },
                        "namespace": {
                            "type": "string",
                            "default": "shared"
                        }
                    },
                    "required": ["content", "tags"]
                }
            },
            {
                "name": "report_change",
                "description": "Report a code change for overlap detection. Agents call this after modifying files.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "actor_id": { "type": "string" },
                        "file_path": { "type": "string" },
                        "diff_unified": { "type": "string" },
                        "start_line": { "type": "integer" },
                        "end_line": { "type": "integer" },
                        "task_id": { "type": "string", "description": "UUID of the task this change belongs to" },
                        "task_prompt": { "type": "string" }
                    },
                    "required": ["actor_id", "file_path", "diff_unified", "start_line", "end_line"]
                }
            },
            {
                "name": "list_decisions",
                "description": "List stored decisions filtered by file pattern and time range.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "file_pattern": {
                            "type": "string",
                            "description": "Glob pattern to filter by file. E.g. 'src/auth/**'"
                        },
                        "since_days": {
                            "type": "integer",
                            "description": "Only decisions from the last N days",
                            "default": 30
                        },
                        "limit": { "type": "integer", "default": 10 }
                    }
                }
            }
        ]
    })
}

/// Call a specific MCP tool.
pub fn call_tool(
    tool_name: &str,
    arguments: &serde_json::Value,
    store: &Arc<Mutex<MemoryStore>>,
    request_context: &RequestContext,
) -> serde_json::Value {
    match tool_name {
        "harmony_pulse" => handle_harmony_pulse(store),
        "harmony_dashboard" => handle_harmony_dashboard(store),
        "harmony_sync" => handle_harmony_sync(arguments, store, request_context),
        "report_file_edit" => handle_report_file_edit(arguments, store, request_context),
        "query_memory" => handle_query_memory(arguments, store),
        "add_memory" => handle_add_memory(arguments, store),
        "report_change" => handle_report_change(arguments, store, request_context),
        "list_decisions" => handle_list_decisions(arguments, store),
        _ => serde_json::json!({
            "content": [{
                "type": "text",
                "text": format!("Unknown tool: {}", tool_name)
            }],
            "isError": true
        }),
    }
}

fn handle_harmony_pulse(store: &Arc<Mutex<MemoryStore>>) -> serde_json::Value {
    let store = store.lock().unwrap();
    let db_path = store.db_path().display().to_string();
    let project_path = infer_project_path(&db_path);
    let overlaps = match store.get_pending_overlaps() {
        Ok(overlaps) => overlaps,
        Err(error) => {
            return serde_json::json!({
                "content": [{ "type": "text", "text": format!("Error reading pending overlaps: {}", error) }],
                "isError": true
            });
        }
    };
    let agents = match store.get_agents() {
        Ok(agents) => agents,
        Err(error) => {
            return serde_json::json!({
                "content": [{ "type": "text", "text": format!("Error reading registered agents: {}", error) }],
                "isError": true
            });
        }
    };

    let mut lines = vec![
        "Harmony Pulse".to_string(),
        format!("Project: {}", project_path),
        format!("Database: {}", db_path),
        format!("Registered agents: {}", agents.len()),
        format!("Pending overlaps: {}", overlaps.len()),
        String::new(),
    ];

    if overlaps.is_empty() {
        lines.push("No active overlaps found.".to_string());
        lines.push(
            "Next: keep Harmony connected, make overlapping human and agent edits in the same file, then run Harmony Pulse again."
                .to_string(),
        );
    } else {
        lines.push("Active overlaps:".to_string());
        for overlap in overlaps.iter().take(5) {
            lines.push(format!(
                "- {} lines {}-{}: {} vs {}",
                overlap.file_path,
                overlap.region_a.start_line + 1,
                overlap.region_a.end_line + 1,
                describe_actor(&overlap.change_a),
                describe_actor(&overlap.change_b),
            ));
        }

        if overlaps.len() > 5 {
            lines.push(format!("...and {} more overlap(s).", overlaps.len() - 5));
        }
    }

    serde_json::json!({
        "content": [{
            "type": "text",
            "text": lines.join("\n")
        }]
    })
}

fn handle_harmony_dashboard(store: &Arc<Mutex<MemoryStore>>) -> serde_json::Value {
    let store = store.lock().unwrap();
    let db_path = store.db_path().display().to_string();
    let project_path = infer_project_path(&db_path);
    let config = load_project_config(&project_path);
    let dashboard_url = dashboard_url_for_config(&config);

    let mut lines = vec![
        "Harmony Dashboard".to_string(),
        format!("Project: {}", project_path),
        format!("Database: {}", db_path),
        format!("URL: {}", dashboard_url),
        String::new(),
        "Open that URL in your browser. Zed Agent currently exposes Harmony MCP tools, not Harmony slash commands."
            .to_string(),
    ];

    if config.network.mode.eq_ignore_ascii_case("client") {
        lines.push(
            "This project is in client mode, so the dashboard is served by the host machine."
                .to_string(),
        );
    }

    serde_json::json!({
        "content": [{
            "type": "text",
            "text": lines.join("\n")
        }]
    })
}

fn handle_harmony_sync(
    args: &serde_json::Value,
    store: &Arc<Mutex<MemoryStore>>,
    request_context: &RequestContext,
) -> serde_json::Value {
    let (project_path, db_path_string) = {
        let store = store.lock().unwrap();
        let db_path = store.db_path().display().to_string();
        let project_path = infer_project_path(&db_path);
        (project_path, db_path)
    };

    let actor_id = args
        .get("actor_id")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("agent:zed-assistant");
    let since_seconds = args
        .get("since_seconds")
        .and_then(|value| value.as_u64())
        .unwrap_or(900);
    let task_prompt = args
        .get("task_prompt")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.to_string());
    let explicit_files = args
        .get("files")
        .and_then(|value| value.as_array())
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_str())
                .filter(|entry| !entry.trim().is_empty())
                .map(|entry| entry.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let project_root = PathBuf::from(&project_path);
    let state_path = project_root.join(".harmony").join("agent-sync-state.json");
    let mut sync_state = match SyncState::load(&state_path) {
        Ok(state) => state,
        Err(error) => {
            return serde_json::json!({
                "content": [{ "type": "text", "text": format!("Harmony Sync failed to load state: {}", error) }],
                "isError": true
            });
        }
    };

    let candidates = if explicit_files.is_empty() {
        match discover_recent_text_files(&project_root, Duration::from_secs(since_seconds)) {
            Ok(files) => files,
            Err(error) => {
                return serde_json::json!({
                    "content": [{ "type": "text", "text": format!("Harmony Sync failed to scan recent files: {}", error) }],
                    "isError": true
                });
            }
        }
    } else {
        explicit_files
    };

    let mut summary = SyncSummary {
        scanned_files: candidates.len(),
        ..SyncSummary::default()
    };

    for candidate in candidates {
        match sync_one_file(
            &project_root,
            store,
            actor_id,
            task_prompt.clone(),
            &mut sync_state,
            &candidate,
            request_context,
        ) {
            Ok(Some(result)) => {
                summary.synced_files.push(result.path);
                summary.overlap_ids.extend(result.overlap_ids);
            }
            Ok(None) => {}
            Err(error) => summary
                .skipped_files
                .push(format!("{candidate} ({error})")),
        }
    }

    if let Err(error) = sync_state.save(&state_path) {
        return serde_json::json!({
            "content": [{ "type": "text", "text": format!("Harmony Sync failed to save state: {}", error) }],
            "isError": true
        });
    }

    let mut lines = vec![
        "Harmony Sync".to_string(),
        format!("Project: {}", project_path),
        format!("Database: {}", db_path_string),
        format!("Actor: {}", actor_id),
        format!("Scanned files: {}", summary.scanned_files),
        format!("Synced files: {}", summary.synced_files.len()),
        format!("Overlaps detected: {}", summary.overlap_ids.len()),
        String::new(),
    ];

    if summary.synced_files.is_empty() {
        lines.push("No project files needed syncing.".to_string());
        if args.get("files").is_none() {
            lines.push(format!(
                "Tip: edit a file, then run harmony_sync again within {} minutes.",
                (since_seconds / 60).max(1)
            ));
        }
    } else {
        lines.push("Synced files:".to_string());
        for path in &summary.synced_files {
            lines.push(format!("- {}", path));
        }
    }

    if !summary.skipped_files.is_empty() {
        lines.push(String::new());
        lines.push("Skipped:".to_string());
        for skipped in summary.skipped_files.iter().take(5) {
            lines.push(format!("- {}", skipped));
        }
        if summary.skipped_files.len() > 5 {
            lines.push(format!(
                "...and {} more skipped item(s).",
                summary.skipped_files.len() - 5
            ));
        }
    }

    serde_json::json!({
        "content": [{
            "type": "text",
            "text": lines.join("\n")
        }]
    })
}

fn handle_report_file_edit(
    args: &serde_json::Value,
    store: &Arc<Mutex<MemoryStore>>,
    request_context: &RequestContext,
) -> serde_json::Value {
    let actor_id_str = match args.get("actor_id").and_then(|v| v.as_str()) {
        Some(actor_id) if !actor_id.trim().is_empty() => actor_id,
        _ => {
            return serde_json::json!({
                "content": [{ "type": "text", "text": "Missing required field: actor_id" }],
                "isError": true
            });
        }
    };

    let file_path = match args.get("file_path").and_then(|v| v.as_str()) {
        Some(file_path) if !file_path.trim().is_empty() => normalize_file_path(file_path),
        _ => {
            return serde_json::json!({
                "content": [{ "type": "text", "text": "Missing required field: file_path" }],
                "isError": true
            });
        }
    };

    let content = args
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let start_line = args
        .get("start_line")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let derived_end_line = start_line
        + content
            .lines()
            .count()
            .saturating_sub(1) as u32;
    let end_line = args
        .get("end_line")
        .and_then(|v| v.as_u64())
        .map(|value| value as u32)
        .unwrap_or(derived_end_line);
    let task_prompt = args
        .get("task_prompt")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let diff_unified = args
        .get("diff_unified")
        .and_then(|v| v.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.to_string())
        .unwrap_or_else(|| synthetic_diff_for_content(start_line, content));

    handle_record_change(
        actor_id_str,
        &file_path,
        &diff_unified,
        start_line,
        end_line,
        None,
        task_prompt,
        store,
        request_context,
    )
}

fn infer_project_path(db_path: &str) -> String {
    let db = Path::new(db_path);
    let parent = db.parent().unwrap_or(db);
    let project_root = parent
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case(".harmony"))
        .unwrap_or(false)
        .then(|| parent.parent().unwrap_or(parent))
        .unwrap_or(parent);

    project_root.display().to_string()
}

fn load_project_config(project_path: &str) -> HarmonyConfig {
    HarmonyConfig::load(Path::new(project_path)).unwrap_or_default()
}

fn dashboard_url_for_config(config: &HarmonyConfig) -> String {
    if config.network.mode.eq_ignore_ascii_case("client") {
        if let Some(host_url) = config.network.host_url.as_deref() {
            if let Ok(mut url) = reqwest::Url::parse(host_url) {
                let _ = url.set_port(Some(config.network.web_port));
                url.set_path("");
                url.set_query(None);
                url.set_fragment(None);
                return url.to_string().trim_end_matches('/').to_string();
            }
        }
    }

    format!("http://127.0.0.1:{}", config.network.web_port)
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
struct SyncState {
    version: u32,
    files: BTreeMap<String, SyncedFileState>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
struct SyncedFileState {
    modified_unix_ms: u64,
    size_bytes: u64,
}

#[derive(Debug, Default)]
struct SyncSummary {
    scanned_files: usize,
    synced_files: Vec<String>,
    skipped_files: Vec<String>,
    overlap_ids: Vec<String>,
}

struct SyncFileResult {
    path: String,
    overlap_ids: Vec<String>,
}

impl SyncState {
    fn load(state_path: &Path) -> anyhow::Result<Self> {
        if !state_path.exists() {
            return Ok(Self {
                version: 1,
                files: BTreeMap::new(),
            });
        }

        let content = fs::read_to_string(state_path)?;
        let mut state: SyncState = serde_json::from_str(&content)?;
        if state.version == 0 {
            state.version = 1;
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

fn sync_one_file(
    project_root: &Path,
    store: &Arc<Mutex<MemoryStore>>,
    actor_id: &str,
    task_prompt: Option<String>,
    state: &mut SyncState,
    candidate: &str,
    request_context: &RequestContext,
) -> anyhow::Result<Option<SyncFileResult>> {
    let full_path = resolve_candidate_path(project_root, candidate)?;
    let metadata = fs::metadata(&full_path)?;
    if !metadata.is_file() {
        anyhow::bail!("not a file");
    }

    let relative_path = relative_project_path(project_root, &full_path)?;
    if should_skip_path(&relative_path) {
        anyhow::bail!("ignored by Harmony sync rules");
    }

    let signature = file_signature(&metadata)?;
    if state.files.get(&relative_path) == Some(&signature) {
        return Ok(None);
    }

    let content = read_text_file(&full_path)?;
    let line_count = content.lines().count().max(1);
    let machine_name = if request_context.machine_name.trim().is_empty() {
        default_machine_name()
    } else {
        request_context.machine_name.clone()
    };
    let machine_ip = if request_context.machine_ip.trim().is_empty() {
        default_machine_ip()
    } else {
        request_context.machine_ip.clone()
    };

    let store = store.lock().unwrap();
    let result = record_change(
        &store,
        RecordChangeArgs {
            actor_id,
            file_path: &relative_path,
            diff_unified: &synthetic_diff_for_content(0, &content),
            start_line: 0,
            end_line: line_count.saturating_sub(1) as u32,
            task_id: None,
            task_prompt: task_prompt.clone().or_else(|| {
                Some("Synced recent assistant edit from the Harmony MCP tool".to_string())
            }),
            machine_name: &machine_name,
            machine_ip: &machine_ip,
        },
    )?;

    let canonical_actor = canonical_actor_id(actor_id, &machine_name);
    emit_log(
        "INFO",
        "sync",
        format!("sync <- {} {}", canonical_actor, relative_path),
    );

    if let Some(agent) = store
        .get_agents()
        .ok()
        .and_then(|agents| agents.into_iter().find(|agent| agent.actor_id.0 == canonical_actor))
    {
        emit_agent_update(&agent);
    }

    let overlaps = result
        .overlaps_detected
        .iter()
        .filter_map(|id| store.get_overlap(*id).ok().flatten())
        .collect::<Vec<_>>();
    for overlap in &overlaps {
        emit_log(
            "INFO",
            "overlap",
            format!(
                "detected: {} ({} ∩ {})",
                overlap.file_path,
                describe_actor(&overlap.change_a),
                describe_actor(&overlap.change_b)
            ),
        );
        emit_overlap(overlap);
    }

    state.files.insert(relative_path.clone(), signature);
    Ok(Some(SyncFileResult {
        path: relative_path,
        overlap_ids: result
            .overlaps_detected
            .into_iter()
            .map(|id| id.to_string())
            .collect(),
    }))
}

fn discover_recent_text_files(project_root: &Path, since: Duration) -> anyhow::Result<Vec<String>> {
    let cutoff = SystemTime::now().checked_sub(since).unwrap_or(UNIX_EPOCH);
    let mut results = Vec::new();
    collect_recent_text_files(project_root, project_root, cutoff, &mut results)?;
    results.sort();
    Ok(results)
}

fn collect_recent_text_files(
    project_root: &Path,
    current_dir: &Path,
    cutoff: SystemTime,
    results: &mut Vec<String>,
) -> anyhow::Result<()> {
    for entry in fs::read_dir(current_dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        let metadata = entry.metadata()?;

        if metadata.is_dir() {
            if should_skip_directory(&file_name) {
                continue;
            }
            collect_recent_text_files(project_root, &path, cutoff, results)?;
            continue;
        }

        if !metadata.is_file() {
            continue;
        }

        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        if modified < cutoff {
            continue;
        }

        let relative = relative_project_path(project_root, &path)?;
        if should_skip_path(&relative) {
            continue;
        }

        if is_probably_text_file(&path, &metadata)? {
            results.push(relative);
        }
    }

    Ok(())
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

fn resolve_candidate_path(project_root: &Path, candidate: &str) -> anyhow::Result<PathBuf> {
    let path = PathBuf::from(candidate);
    let resolved = if path.is_absolute() {
        path
    } else {
        project_root.join(path)
    };
    Ok(if resolved.is_absolute() {
        resolved
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(resolved)
    })
}

fn relative_project_path(project_root: &Path, full_path: &Path) -> anyhow::Result<String> {
    let absolute_root = if project_root.is_absolute() {
        project_root.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(project_root)
    };
    let absolute_path = if full_path.is_absolute() {
        full_path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(full_path)
    };
    let relative = absolute_path
        .strip_prefix(&absolute_root)
        .map_err(|_| anyhow::anyhow!("path is outside the project root"))?;
    Ok(normalize_file_path(&relative.to_string_lossy()))
}

fn file_signature(metadata: &fs::Metadata) -> anyhow::Result<SyncedFileState> {
    let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
    let modified_unix_ms = modified
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    Ok(SyncedFileState {
        modified_unix_ms,
        size_bytes: metadata.len(),
    })
}

fn is_probably_text_file(path: &Path, metadata: &fs::Metadata) -> anyhow::Result<bool> {
    if metadata.len() > 512 * 1024 {
        return Ok(false);
    }

    let bytes = fs::read(path)?;
    Ok(!bytes.contains(&0))
}

fn read_text_file(path: &Path) -> anyhow::Result<String> {
    let bytes = fs::read(path)?;
    if bytes.contains(&0) {
        anyhow::bail!("binary files are not supported");
    }

    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn handle_query_memory(
    args: &serde_json::Value,
    store: &Arc<Mutex<MemoryStore>>,
) -> serde_json::Value {
    let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
    let namespace_str = args.get("namespace").and_then(|v| v.as_str()).unwrap_or("shared");
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;

    let namespace = parse_namespace(namespace_str);

    let store = store.lock().unwrap();
    match store.query_memory(query, namespace, limit) {
        Ok(results) => {
            let records: Vec<serde_json::Value> = results.into_iter().map(|(record, similarity)| {
                serde_json::json!({
                    "content": record.content,
                    "tags": record.tags,
                    "similarity": similarity,
                    "created_at": record.created_at.to_rfc3339()
                })
            }).collect();

            serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": serde_json::to_string_pretty(&records).unwrap_or_default()
                }]
            })
        }
        Err(e) => serde_json::json!({
            "content": [{ "type": "text", "text": format!("Error: {}", e) }],
            "isError": true
        }),
    }
}

fn handle_add_memory(
    args: &serde_json::Value,
    store: &Arc<Mutex<MemoryStore>>,
) -> serde_json::Value {
    let content = match args.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return serde_json::json!({
            "content": [{ "type": "text", "text": "Missing required field: content" }],
            "isError": true
        }),
    };

    let tags: Vec<String> = args.get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    let namespace_str = args.get("namespace").and_then(|v| v.as_str()).unwrap_or("shared");
    let namespace = parse_namespace(namespace_str);

    let store = store.lock().unwrap();
    match store.add_memory(content, tags, namespace, None, vec![]) {
        Ok(id) => serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::json!({
                    "id": id.to_string(),
                    "message": "Memory stored successfully"
                }).to_string()
            }]
        }),
        Err(e) => serde_json::json!({
            "content": [{ "type": "text", "text": format!("Error: {}", e) }],
            "isError": true
        }),
    }
}

fn handle_report_change(
    args: &serde_json::Value,
    store: &Arc<Mutex<MemoryStore>>,
    request_context: &RequestContext,
) -> serde_json::Value {
    let actor_id_str = args.get("actor_id").and_then(|v| v.as_str()).unwrap_or("");
    let file_path = normalize_file_path(args.get("file_path").and_then(|v| v.as_str()).unwrap_or(""));
    let diff_unified = args.get("diff_unified").and_then(|v| v.as_str()).unwrap_or("");
    let start_line = args.get("start_line").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let end_line = args.get("end_line").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let task_id = args.get("task_id").and_then(|v| v.as_str()).and_then(|s| Uuid::parse_str(s).ok());
    let task_prompt = args.get("task_prompt").and_then(|v| v.as_str()).map(|s| s.to_string());

    handle_record_change(
        actor_id_str,
        &file_path,
        diff_unified,
        start_line,
        end_line,
        task_id,
        task_prompt,
        store,
        request_context,
    )
}

fn handle_record_change(
    actor_id_str: &str,
    file_path: &str,
    diff_unified: &str,
    start_line: u32,
    end_line: u32,
    task_id: Option<Uuid>,
    task_prompt: Option<String>,
    store: &Arc<Mutex<MemoryStore>>,
    request_context: &RequestContext,
) -> serde_json::Value {
    let store = store.lock().unwrap();
    let machine_name = request_context.machine_name.trim();
    let machine_ip = request_context.machine_ip.trim();
    let machine_name = if machine_name.is_empty() {
        default_machine_name()
    } else {
        machine_name.to_string()
    };
    let machine_ip = if machine_ip.is_empty() {
        default_machine_ip()
    } else {
        machine_ip.to_string()
    };
    let canonical_actor = canonical_actor_id(actor_id_str, &machine_name);
    let result = match record_change(
        &store,
        RecordChangeArgs {
            actor_id: actor_id_str,
            file_path,
            diff_unified,
            start_line,
            end_line,
            task_id,
            task_prompt,
            machine_name: &machine_name,
            machine_ip: &machine_ip,
        },
    ) {
        Ok(result) => result,
        Err(error) => {
            return serde_json::json!({
                "content": [{ "type": "text", "text": format!("Error storing change: {}", error) }],
                "isError": true
            });
        }
    };
    emit_log(
        "INFO",
        "mcp",
        format!(
            "report_change <- {} {} L{}-{}",
            canonical_actor,
            file_path,
            start_line + 1,
            end_line + 1
        ),
    );

    if let Some(agent) = store
        .get_agents()
        .ok()
        .and_then(|agents| {
            agents
                .into_iter()
                .find(|agent| agent.actor_id.0 == canonical_actor)
        })
    {
        emit_agent_update(&agent);
    }

    let overlaps = result
        .overlaps_detected
        .iter()
        .filter_map(|id| store.get_overlap(*id).ok().flatten())
        .collect::<Vec<_>>();
    for overlap in &overlaps {
        emit_log(
            "INFO",
            "overlap",
            format!(
                "detected: {} ({} ∩ {})",
                overlap.file_path,
                describe_actor(&overlap.change_a),
                describe_actor(&overlap.change_b)
            ),
        );
        emit_overlap(overlap);
    }

    serde_json::json!({
        "content": [{
            "type": "text",
            "text": serde_json::json!({
                "tag_id": result.tag_id.to_string(),
                "overlaps_detected": result.overlaps_detected
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>(),
                "agent_registered": result.agent_registered
            }).to_string()
        }]
    })
}

fn handle_list_decisions(
    args: &serde_json::Value,
    store: &Arc<Mutex<MemoryStore>>,
) -> serde_json::Value {
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

    let store = store.lock().unwrap();
    match store.query_memory_by_tag("decision", MemoryNamespace::Shared, limit) {
        Ok(records) => {
            let decisions: Vec<serde_json::Value> = records.into_iter().map(|record| {
                serde_json::json!({
                    "content": record.content,
                    "tags": record.tags,
                    "created_at": record.created_at.to_rfc3339()
                })
            }).collect();

            serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": serde_json::to_string_pretty(&decisions).unwrap_or_default()
                }]
            })
        }
        Err(e) => serde_json::json!({
            "content": [{ "type": "text", "text": format!("Error: {}", e) }],
            "isError": true
        }),
    }
}

fn describe_actor(tag: &ProvenanceTag) -> String {
    if tag.actor_id.0.contains('@')
        || tag.machine_name.trim().is_empty()
        || tag.machine_name.eq_ignore_ascii_case("local")
    {
        tag.actor_id.0.clone()
    } else {
        format!("{}@{}", tag.actor_id.0, tag.machine_name)
    }
}

fn parse_namespace(s: &str) -> MemoryNamespace {
    if s == "shared" {
        MemoryNamespace::Shared
    } else if let Some(uuid_str) = s.strip_prefix("agent:") {
        if let Ok(uuid) = Uuid::parse_str(uuid_str) {
            MemoryNamespace::Agent(uuid)
        } else {
            MemoryNamespace::Shared
        }
    } else {
        MemoryNamespace::Shared
    }
}

#[cfg(test)]
mod tests {
    use super::{call_tool, list_tools};
    use harmony_memory::store::MemoryStore;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use crate::types::RequestContext;

    fn test_store() -> Arc<Mutex<MemoryStore>> {
        Arc::new(Mutex::new(
            MemoryStore::open(Path::new(":memory:")).expect("memory store"),
        ))
    }

    fn test_request_context() -> RequestContext {
        RequestContext::new("local", "127.0.0.1")
    }

    #[test]
    fn list_tools_includes_harmony_pulse() {
        let tools = list_tools();
        let tool_names: Vec<&str> = tools["tools"]
            .as_array()
            .expect("tool list")
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();

        assert!(tool_names.contains(&"harmony_pulse"));
        assert!(tool_names.contains(&"harmony_dashboard"));
        assert!(tool_names.contains(&"harmony_sync"));
        assert!(tool_names.contains(&"report_file_edit"));
    }

    #[test]
    fn harmony_pulse_tool_returns_status_text() {
        let response = call_tool(
            "harmony_pulse",
            &serde_json::json!({}),
            &test_store(),
            &test_request_context(),
        );
        let text = response["content"][0]["text"]
            .as_str()
            .expect("text content");

        assert!(text.contains("Harmony Pulse"));
        assert!(text.contains("Database: :memory:"));
        assert!(text.contains("Registered agents: 0"));
        assert!(text.contains("Pending overlaps: 0"));
    }

    #[test]
    fn harmony_dashboard_tool_returns_local_dashboard_url() {
        let response = call_tool(
            "harmony_dashboard",
            &serde_json::json!({}),
            &test_store(),
            &test_request_context(),
        );
        let text = response["content"][0]["text"]
            .as_str()
            .expect("text content");

        assert!(text.contains("Harmony Dashboard"));
        assert!(text.contains("URL: http://127.0.0.1:4233"));
    }

    #[test]
    fn harmony_sync_tool_records_recent_file() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project_root = std::env::temp_dir().join(format!("harmony-tool-sync-{suffix}"));
        let harmony_dir = project_root.join(".harmony");
        std::fs::create_dir_all(&harmony_dir).unwrap();
        std::fs::write(project_root.join("notes.txt"), "hello from tool sync\n").unwrap();

        let store = Arc::new(Mutex::new(
            MemoryStore::open(&harmony_dir.join("memory.db")).expect("memory store"),
        ));
        let response = call_tool(
            "harmony_sync",
            &serde_json::json!({
                "files": ["notes.txt"]
            }),
            &store,
            &test_request_context(),
        );

        let text = response["content"][0]["text"]
            .as_str()
            .expect("text content");
        assert!(text.contains("Harmony Sync"));
        assert!(text.contains("Synced files: 1"));
        assert!(text.contains("notes.txt"));

        let agents = store.lock().unwrap().get_agents().expect("agents");
        assert_eq!(agents.len(), 1);

        let _ = std::fs::remove_dir_all(project_root);
    }

    #[test]
    fn report_file_edit_registers_agent_and_stores_change() {
        let store = test_store();
        let response = call_tool(
            "report_file_edit",
            &serde_json::json!({
                "actor_id": "agent:copilot",
                "file_path": "test2.txt",
                "content": "hello"
            }),
            &store,
            &test_request_context(),
        );

        let text = response["content"][0]["text"]
            .as_str()
            .expect("text content");
        assert!(text.contains("\"agent_registered\":true"));

        let agents = store.lock().unwrap().get_agents().expect("agents");
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].actor_id.0, "agent:copilot");
    }

    #[test]
    fn report_file_edit_can_create_overlap() {
        let store = test_store();

        let _ = call_tool(
            "report_file_edit",
            &serde_json::json!({
                "actor_id": "human:water",
                "file_path": "test2.txt",
                "content": "hello",
                "start_line": 0,
                "end_line": 0
            }),
            &store,
            &test_request_context(),
        );

        let _ = call_tool(
            "report_file_edit",
            &serde_json::json!({
                "actor_id": "agent:copilot",
                "file_path": "test2.txt",
                "content": "hello from agent",
                "start_line": 0,
                "end_line": 0
            }),
            &store,
            &test_request_context(),
        );

        let overlaps = store
            .lock()
            .unwrap()
            .get_pending_overlaps()
            .expect("overlaps");
        assert_eq!(overlaps.len(), 1);
    }
}
