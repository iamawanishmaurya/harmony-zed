mod file_sync;
mod server;
mod tools;
mod tracking;
mod transport;
mod types;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{fs::OpenOptions, io::Write};

use harmony_core::HarmonyConfig;
use harmony_memory::store::MemoryStore;
use serde::{Deserialize, Serialize};

use crate::server::{
    detect_machine_ip, AppState, ClientRuntimeConfig, HostRuntimeConfig, NetworkMode,
    RuntimeState,
};
use crate::file_sync::{spawn_client_auto_sync, spawn_host_auto_sync};
use crate::tracking::{
    normalize_file_path, record_change, synthetic_diff_for_content, RecordChangeArgs,
};
use crate::types::RequestContext;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let cli = Cli::parse(&args)?;
    let bootstrap = bootstrap_project(
        cli.project_root.as_deref(),
        &cli.db_path,
        cli.requires_store(),
    )?;

    match cli.mode {
        CommandMode::Serve => run_serve_mode(&cli, bootstrap).await?,
        CommandMode::Pulse => {
            print!("{}", pulse_report(&bootstrap)?);
        }
        CommandMode::Doctor => {
            print!("{}", doctor_report(&bootstrap));
        }
        CommandMode::Sync => {
            let sync = cli
                .sync
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Sync options were not parsed"))?;
            print!("{}", sync_report(&bootstrap, sync)?);
        }
        CommandMode::Help => {
            print!("{}", help_text());
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandMode {
    Serve,
    Pulse,
    Doctor,
    Sync,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServeRuntimeMode {
    Stdio,
    Host,
    Client,
}

impl ServeRuntimeMode {
    fn parse(value: &str) -> anyhow::Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "stdio" => Ok(Self::Stdio),
            "host" => Ok(Self::Host),
            "client" => Ok(Self::Client),
            other => Err(anyhow::anyhow!(
                "Unknown serve mode: {other}. Expected one of: stdio, host, client."
            )),
        }
    }
}

#[derive(Debug, Clone)]
struct Cli {
    mode: CommandMode,
    db_path: PathBuf,
    project_root: Option<PathBuf>,
    serve: ServeOptions,
    sync: Option<SyncOptions>,
}

#[derive(Debug, Clone)]
struct ServeOptions {
    runtime_mode: ServeRuntimeMode,
    runtime_mode_explicit: bool,
    stdio_bridge: bool,
    host_name: Option<String>,
    host_url: Option<String>,
    mcp_port: Option<u16>,
    ipc_port: Option<u16>,
    web_port: Option<u16>,
}

#[derive(Debug, Clone)]
struct SyncOptions {
    files: Vec<String>,
    actor_id: String,
    task_prompt: Option<String>,
    since_seconds: u64,
}

struct Bootstrap {
    db_path: PathBuf,
    project_root: PathBuf,
    config_path: PathBuf,
    config_created: bool,
    config_warning: Option<String>,
    config: HarmonyConfig,
    store: Option<MemoryStore>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct SyncState {
    version: u32,
    files: BTreeMap<String, SyncedFileState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

impl Cli {
    fn parse(args: &[String]) -> anyhow::Result<Self> {
        let mut mode = CommandMode::Serve;
        let mut start_index = 1;

        if let Some(first) = args.get(1) {
            if !first.starts_with("--") {
                mode = match first.as_str() {
                    "serve" => CommandMode::Serve,
                    "pulse" => CommandMode::Pulse,
                    "doctor" => CommandMode::Doctor,
                    "sync" => CommandMode::Sync,
                    "help" => CommandMode::Help,
                    other => {
                        return Err(anyhow::anyhow!(
                            "Unknown subcommand: {other}\n\n{}",
                            help_text()
                        ));
                    }
                };
                start_index = 2;
            } else if first == "--help" || first == "-h" {
                mode = CommandMode::Help;
            }
        }

        let project_root = parse_flag(args, "--project-root", start_index).map(PathBuf::from);
        let db_path = parse_flag(args, "--db-path", start_index)
            .map(PathBuf::from)
            .or_else(|| {
                project_root
                    .as_ref()
                    .map(|root| root.join(".harmony").join("memory.db"))
            })
            .unwrap_or_else(default_db_path);
        let runtime_mode_raw = parse_flag(args, "--mode", start_index);
        let runtime_mode = runtime_mode_raw
            .as_deref()
            .map(ServeRuntimeMode::parse)
            .transpose()?
            .unwrap_or(ServeRuntimeMode::Stdio);
        let serve = ServeOptions {
            runtime_mode,
            runtime_mode_explicit: runtime_mode_raw.is_some(),
            stdio_bridge: has_flag(args, "--stdio-bridge", start_index),
            host_name: parse_flag(args, "--host-name", start_index)
                .filter(|value| !value.trim().is_empty()),
            host_url: parse_flag(args, "--host-url", start_index)
                .filter(|value| !value.trim().is_empty()),
            mcp_port: parse_u16_flag(args, "--mcp-port", start_index)?,
            ipc_port: parse_u16_flag(args, "--ipc-port", start_index)?,
            web_port: parse_u16_flag(args, "--web-port", start_index)?,
        };
        let sync = (mode == CommandMode::Sync).then(|| SyncOptions {
            files: parse_multi_flag(args, "--file", start_index),
            actor_id: parse_flag(args, "--actor-id", start_index)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "agent:zed-assistant".to_string()),
            task_prompt: parse_flag(args, "--task-prompt", start_index)
                .filter(|value| !value.trim().is_empty()),
            since_seconds: parse_flag(args, "--since-seconds", start_index)
                .and_then(|value| value.parse().ok())
                .unwrap_or(900),
        });

        Ok(Self {
            mode,
            db_path: absolutize(&db_path),
            project_root: project_root.map(|path| absolutize(&path)),
            serve,
            sync,
        })
    }

    fn requires_store(&self) -> bool {
        match self.mode {
            CommandMode::Help => false,
            CommandMode::Serve => false,
            CommandMode::Pulse | CommandMode::Doctor | CommandMode::Sync => true,
        }
    }
}

impl Bootstrap {
    fn store(&self) -> anyhow::Result<&MemoryStore> {
        self.store
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Harmony store is not available in this runtime mode"))
    }
}

fn bootstrap_project(
    project_root_override: Option<&Path>,
    db_path: &Path,
    open_store: bool,
) -> anyhow::Result<Bootstrap> {
    let project_root = project_root_override
        .map(absolutize)
        .unwrap_or_else(|| infer_project_root(db_path));
    let config_path = project_root.join(".harmony").join("config.toml");
    let config_created = !config_path.exists();

    let (config, config_warning) = match HarmonyConfig::load(&project_root) {
        Ok(config) => (config, None),
        Err(err) if config_path.exists() => {
            let warning = format!(
                "Config warning: {err}. Continuing with defaults until {} is fixed.",
                config_path.display()
            );
            append_bootstrap_log(&project_root, &warning);
            (HarmonyConfig::default(), Some(warning))
        }
        Err(err) => return Err(err),
    };
    let store = if open_store {
        Some(MemoryStore::open(db_path)?)
    } else {
        None
    };

    Ok(Bootstrap {
        db_path: db_path.to_path_buf(),
        project_root,
        config_path,
        config_created,
        config_warning,
        config,
        store,
    })
}

async fn run_serve_mode(cli: &Cli, bootstrap: Bootstrap) -> anyhow::Result<()> {
    maybe_init_tracing();
    install_file_trace_log(&bootstrap.db_path);
    let runtime_mode = resolve_runtime_mode(cli, &bootstrap)?;
    let machine_name = cli
        .serve
        .host_name
        .clone()
        .unwrap_or_else(|| bootstrap.config.human.username.clone());
    let machine_ip = detect_machine_ip();
    std::env::set_var("HARMONY_MACHINE_NAME", &machine_name);
    std::env::set_var("HARMONY_MACHINE_IP", &machine_ip);

    match runtime_mode {
        ServeRuntimeMode::Stdio => {
            tracing::info!(
                "Starting harmony-mcp stdio server with db: {}",
                bootstrap.db_path.display()
            );
            let store = MemoryStore::open(&bootstrap.db_path)?;
            transport::run_stdio_server(Arc::new(Mutex::new(store))).await
        }
        ServeRuntimeMode::Host => {
            let mcp_port = cli
                .serve
                .mcp_port
                .unwrap_or(bootstrap.config.network.mcp_port);
            let ipc_port = cli
                .serve
                .ipc_port
                .unwrap_or(bootstrap.config.network.ipc_port);
            let web_port = cli
                .serve
                .web_port
                .unwrap_or(bootstrap.config.network.web_port);
            let share_url = format!("http://{}:{}", machine_ip, mcp_port);
            let store = Arc::new(Mutex::new(MemoryStore::open(&bootstrap.db_path)?));
            let sync_config = bootstrap.config.clone();
            let state = AppState {
                store: Some(store.clone()),
                started_at: Instant::now(),
                project_root: bootstrap.project_root.clone(),
                db_path: Some(bootstrap.db_path.clone()),
                config_path: bootstrap.config_path.clone(),
                debug_log_path: bootstrap.project_root.join(".harmony").join("mcp-debug.log"),
                machine_name: machine_name.clone(),
                machine_ip: machine_ip.clone(),
                mode: NetworkMode::Host,
                mcp_port,
                ipc_port,
                web_port,
                host_url: Some(share_url),
                runtime: RuntimeState::new(),
            };
            spawn_host_auto_sync(state.clone(), store.clone(), sync_config);

            if cli.serve.stdio_bridge {
                let bridge_target = format!("http://127.0.0.1:{mcp_port}/mcp");
                let bridge_context = RequestContext::new(machine_name, machine_ip);
                let handle = tokio::spawn(server::run_host(HostRuntimeConfig { state, store }));
                tokio::time::sleep(std::time::Duration::from_millis(350)).await;
                let bridge_result =
                    transport::run_stdio_http_bridge(&bridge_target, &bridge_context).await;
                handle.abort();
                bridge_result
            } else {
                server::run_host(HostRuntimeConfig { state, store }).await
            }
        }
        ServeRuntimeMode::Client => {
            let host_url = cli
                .serve
                .host_url
                .clone()
                .or_else(|| bootstrap.config.network.host_url.clone())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Client mode requires --host-url or .harmony/config.toml [network].host_url"
                    )
                })?;
            let sync_config = bootstrap.config.clone();
            let state = AppState {
                store: None,
                started_at: Instant::now(),
                project_root: bootstrap.project_root.clone(),
                db_path: None,
                config_path: bootstrap.config_path.clone(),
                debug_log_path: bootstrap.project_root.join(".harmony").join("mcp-debug.log"),
                machine_name: machine_name.clone(),
                machine_ip: machine_ip.clone(),
                mode: NetworkMode::Client,
                mcp_port: cli
                    .serve
                    .mcp_port
                    .unwrap_or(bootstrap.config.network.mcp_port),
                ipc_port: cli
                    .serve
                    .ipc_port
                    .unwrap_or(bootstrap.config.network.ipc_port),
                web_port: cli
                    .serve
                    .web_port
                    .unwrap_or(bootstrap.config.network.web_port),
                host_url: Some(host_url.clone()),
                runtime: RuntimeState::new(),
            };
            spawn_client_auto_sync(state.clone(), host_url.clone(), sync_config);

            if cli.serve.stdio_bridge {
                let bridge_target = host_mcp_endpoint(&host_url);
                let bridge_context = RequestContext::new(machine_name, machine_ip);
                let handle = tokio::spawn(server::run_client(ClientRuntimeConfig { state, host_url }));
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                let bridge_result =
                    transport::run_stdio_http_bridge(&bridge_target, &bridge_context).await;
                handle.abort();
                bridge_result
            } else {
                server::run_client(ClientRuntimeConfig { state, host_url }).await
            }
        }
    }
}

fn infer_project_root(db_path: &Path) -> PathBuf {
    let parent = db_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    let is_harmony_dir = parent
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case(".harmony"))
        .unwrap_or(false);

    if is_harmony_dir {
        parent.parent().map(Path::to_path_buf).unwrap_or(parent)
    } else {
        parent
    }
}

fn pulse_report(bootstrap: &Bootstrap) -> anyhow::Result<String> {
    let overlaps = bootstrap.store()?.get_pending_overlaps()?;
    let agents = bootstrap.store()?.get_agents()?;

    let mut lines = vec![
        "Harmony Pulse".to_string(),
        format!("Project: {}", bootstrap.project_root.display()),
        format!("Database: {}", bootstrap.db_path.display()),
        format!("Registered agents: {}", agents.len()),
        format!("Pending overlaps: {}", overlaps.len()),
        String::new(),
    ];

    if overlaps.is_empty() {
        lines.push("No active overlaps found.".to_string());
        lines.push(
            "Next: keep harmony-mcp running, make overlapping human and agent edits in the same file, then run /harmony-pulse again."
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
                describe_actor(&overlap.change_a.actor_id.0, &overlap.change_a.machine_name),
                describe_actor(&overlap.change_b.actor_id.0, &overlap.change_b.machine_name),
            ));
        }

        if overlaps.len() > 5 {
            lines.push(format!("...and {} more overlap(s).", overlaps.len() - 5));
        }
    }

    lines.push(String::new());
    Ok(lines.join("\n"))
}

fn doctor_report(bootstrap: &Bootstrap) -> String {
    let overlaps = bootstrap
        .store
        .as_ref()
        .and_then(|store| store.get_pending_overlaps().ok())
        .unwrap_or_default();
    let agents = bootstrap
        .store
        .as_ref()
        .and_then(|store| store.get_agents().ok())
        .unwrap_or_default();
    let config_state = if bootstrap.config_created {
        "created"
    } else {
        "verified"
    };

    let mut lines = vec![
        "Harmony Doctor".to_string(),
        format!("Project: {}", bootstrap.project_root.display()),
        format!("Database: {} (ok)", bootstrap.db_path.display()),
        format!("Config: {} ({config_state})", bootstrap.config_path.display()),
        format!(
            "Network defaults: mode={} mcp={} ipc={} web={}",
            bootstrap.config.network.mode,
            bootstrap.config.network.mcp_port,
            bootstrap.config.network.ipc_port,
            bootstrap.config.network.web_port,
        ),
        format!("Registered agents: {}", agents.len()),
        format!("Pending overlaps: {}", overlaps.len()),
        "Status: ready".to_string(),
        String::new(),
    ];

    if let Some(warning) = &bootstrap.config_warning {
        lines.insert(4, warning.clone());
    }

    lines.join("\n")
}

fn sync_report(bootstrap: &Bootstrap, sync: &SyncOptions) -> anyhow::Result<String> {
    let state_path = bootstrap.project_root.join(".harmony").join("agent-sync-state.json");
    let mut state = SyncState::load(&state_path)?;
    let candidates = if sync.files.is_empty() {
        discover_recent_text_files(
            &bootstrap.project_root,
            Duration::from_secs(sync.since_seconds),
        )?
    } else {
        sync.files.clone()
    };

    let mut summary = SyncSummary {
        scanned_files: candidates.len(),
        ..SyncSummary::default()
    };

    for candidate in candidates {
        match sync_one_file(bootstrap, sync, &mut state, &candidate) {
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

    state.save(&state_path)?;

    let mut lines = vec![
        "Harmony Sync".to_string(),
        format!("Project: {}", bootstrap.project_root.display()),
        format!("Database: {}", bootstrap.db_path.display()),
        format!("Actor: {}", sync.actor_id),
        format!("Scanned files: {}", summary.scanned_files),
        format!("Synced files: {}", summary.synced_files.len()),
        format!("Overlaps detected: {}", summary.overlap_ids.len()),
        String::new(),
    ];

    if summary.synced_files.is_empty() {
        lines.push("No project files needed syncing.".to_string());
        if sync.files.is_empty() {
            lines.push(format!(
                "Tip: edit a file, then run /harmony-sync again within {} minutes.",
                (sync.since_seconds / 60).max(1)
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

    lines.push(String::new());
    Ok(lines.join("\n"))
}

fn parse_flag(args: &[String], flag: &str, start_index: usize) -> Option<String> {
    args.iter()
        .skip(start_index)
        .position(|arg| arg == flag)
        .and_then(|index| args.get(start_index + index + 1))
        .cloned()
}

fn parse_multi_flag(args: &[String], flag: &str, start_index: usize) -> Vec<String> {
    let mut values = Vec::new();
    let mut index = start_index;
    while index < args.len() {
        if args[index] == flag {
            if let Some(value) = args.get(index + 1) {
                values.push(value.clone());
                index += 2;
                continue;
            }
        }
        index += 1;
    }
    values
}

fn has_flag(args: &[String], flag: &str, start_index: usize) -> bool {
    args.iter().skip(start_index).any(|arg| arg == flag)
}

fn parse_u16_flag(args: &[String], flag: &str, start_index: usize) -> anyhow::Result<Option<u16>> {
    parse_flag(args, flag, start_index)
        .map(|value| {
            value
                .parse::<u16>()
                .map_err(|error| anyhow::anyhow!("Invalid value for {flag}: {value} ({error})"))
        })
        .transpose()
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

fn default_db_path() -> PathBuf {
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(repo_root) = infer_repo_root_from_exe(&exe_path) {
            return repo_root.join(".harmony").join("memory.db");
        }
    }

    PathBuf::from(".harmony").join("memory.db")
}

fn resolve_runtime_mode(cli: &Cli, bootstrap: &Bootstrap) -> anyhow::Result<ServeRuntimeMode> {
    if cli.serve.runtime_mode_explicit || !cli.serve.stdio_bridge {
        return Ok(cli.serve.runtime_mode);
    }

    ServeRuntimeMode::parse(&bootstrap.config.network.mode)
}

fn host_mcp_endpoint(host_url: &str) -> String {
    let trimmed = host_url.trim_end_matches('/');
    if trimmed.ends_with("/mcp") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/mcp")
    }
}

fn describe_actor(actor_id: &str, machine_name: &str) -> String {
    if actor_id.contains('@')
        || machine_name.trim().is_empty()
        || machine_name.eq_ignore_ascii_case("local")
    {
        actor_id.to_string()
    } else {
        format!("{actor_id}@{machine_name}")
    }
}

fn infer_repo_root_from_exe(exe_path: &Path) -> Option<PathBuf> {
    let binary_name = exe_path.file_name()?.to_str()?;
    let is_harmony_binary = matches!(binary_name, "harmony-mcp" | "harmony-mcp.exe");
    if !is_harmony_binary {
        return None;
    }

    let profile_dir = exe_path.parent()?;
    let target_dir = profile_dir.parent()?;
    let target_name = target_dir.file_name()?.to_str()?;
    if !target_name.eq_ignore_ascii_case("target") {
        return None;
    }

    target_dir.parent().map(Path::to_path_buf)
}

fn maybe_init_tracing() {
    let enabled = std::env::var("HARMONY_MCP_TRACE_STDERR")
        .ok()
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false);

    if !enabled {
        return;
    }

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter("harmony_mcp=debug")
        .init();
}

fn install_file_trace_log(db_path: &Path) {
    if let Some(parent) = db_path.parent() {
        let log_path = parent.join("mcp-debug.log");
        std::env::set_var("HARMONY_MCP_DEBUG_LOG", log_path);
    }
}

fn append_bootstrap_log(project_root: &Path, message: &str) {
    let log_path = project_root.join(".harmony").join("mcp-debug.log");
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&log_path) {
        let _ = writeln!(file, "[bootstrap] {message}");
    }
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
    bootstrap: &Bootstrap,
    sync: &SyncOptions,
    state: &mut SyncState,
    candidate: &str,
) -> anyhow::Result<Option<SyncFileResult>> {
    let full_path = resolve_candidate_path(&bootstrap.project_root, candidate)?;
    let metadata = fs::metadata(&full_path)?;
    if !metadata.is_file() {
        anyhow::bail!("not a file");
    }

    let relative_path = relative_project_path(&bootstrap.project_root, &full_path)?;
    if should_skip_path(&relative_path) {
        anyhow::bail!("ignored by Harmony sync rules");
    }

    let signature = file_signature(&metadata)?;
    if state.files.get(&relative_path) == Some(&signature) {
        return Ok(None);
    }

    let content = read_text_file(&full_path)?;
    let line_count = content.lines().count().max(1);
    let machine_name = bootstrap.config.human.username.clone();
    let machine_ip = detect_machine_ip();
    let task_prompt = sync.task_prompt.clone().or_else(|| {
        Some("Synced recent assistant edit from the Harmony Zed extension".to_string())
    });
    let result = record_change(
        bootstrap.store()?,
        RecordChangeArgs {
            actor_id: &sync.actor_id,
            file_path: &relative_path,
            diff_unified: &synthetic_diff_for_content(0, &content),
            start_line: 0,
            end_line: line_count.saturating_sub(1) as u32,
            task_id: None,
            task_prompt,
            machine_name: &machine_name,
            machine_ip: &machine_ip,
        },
    )?;

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
    Ok(absolutize(&resolved))
}

fn relative_project_path(project_root: &Path, full_path: &Path) -> anyhow::Result<String> {
    let absolute_root = absolutize(project_root);
    let absolute_path = absolutize(full_path);
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

fn help_text() -> &'static str {
    "Harmony MCP\n\
\n\
Usage:\n\
  harmony-mcp [--db-path PATH]\n\
  harmony-mcp --mode host|client [options]\n\
  harmony-mcp [--stdio-bridge] [options]\n\
  harmony-mcp serve [--db-path PATH] [--mode stdio|host|client]\n\
  harmony-mcp pulse [--db-path PATH]\n\
  harmony-mcp doctor [--db-path PATH]\n\
  harmony-mcp sync [--db-path PATH] [--actor-id ID] [--since-seconds N] [--file PATH ...]\n\
\n\
Commands:\n\
  serve   Start Harmony in stdio mode (default) or network host/client mode\n\
  pulse   Print a one-shot overlap summary for the project database\n\
  doctor  Verify the local Harmony setup and print the resolved paths\n\
  sync    Record recent or explicit project files as assistant edits in Harmony\n\
\n\
Network options:\n\
  --mode host|client|stdio  Choose runtime mode. Default is stdio unless --stdio-bridge defers to config.\n\
  --stdio-bridge           Run the host/client network path behind a stdio MCP bridge for Zed.\n\
  --project-root PATH       Override the project root used for config/log resolution.\n\
  --host-url URL            Host MCP base URL for client mode, for example http://192.168.1.10:4231\n\
  --host-name NAME          Display machine name for host/client status output.\n\
  --mcp-port PORT           HTTP MCP port in host mode (default: config or 4231)\n\
  --ipc-port PORT           Local TCP IPC port in host/client mode (default: config or 4232)\n\
  --web-port PORT           Dashboard port in host mode (default: config or 4233)\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parse_defaults_to_serve_mode() {
        let cli = Cli::parse(&["harmony-mcp".to_string()]).unwrap();
        assert_eq!(cli.mode, CommandMode::Serve);
        assert_eq!(cli.serve.runtime_mode, ServeRuntimeMode::Stdio);
        assert!(
            cli.db_path.ends_with(".harmony\\memory.db")
                || cli.db_path.ends_with(".harmony/memory.db")
        );
    }

    #[test]
    fn parse_supports_subcommands() {
        let cli = Cli::parse(&[
            "harmony-mcp".to_string(),
            "pulse".to_string(),
            "--db-path".to_string(),
            "demo/.harmony/memory.db".to_string(),
        ])
        .unwrap();

        assert_eq!(cli.mode, CommandMode::Pulse);
        assert!(
            cli.db_path.ends_with("demo\\.harmony\\memory.db")
                || cli.db_path.ends_with("demo/.harmony/memory.db")
        );
    }

    #[test]
    fn parse_supports_sync_flags() {
        let cli = Cli::parse(&[
            "harmony-mcp".to_string(),
            "sync".to_string(),
            "--db-path".to_string(),
            "demo/.harmony/memory.db".to_string(),
            "--actor-id".to_string(),
            "agent:copilot".to_string(),
            "--since-seconds".to_string(),
            "120".to_string(),
            "--file".to_string(),
            "src/app.ts".to_string(),
            "--file".to_string(),
            "README.md".to_string(),
        ])
        .unwrap();

        assert_eq!(cli.mode, CommandMode::Sync);
        let sync = cli.sync.expect("sync options");
        assert_eq!(sync.actor_id, "agent:copilot");
        assert_eq!(sync.since_seconds, 120);
        assert_eq!(
            sync.files,
            vec!["src/app.ts".to_string(), "README.md".to_string()]
        );
    }

    #[test]
    fn parse_supports_network_host_mode_flags() {
        let cli = Cli::parse(&[
            "harmony-mcp".to_string(),
            "--mode".to_string(),
            "host".to_string(),
            "--project-root".to_string(),
            "demo".to_string(),
            "--mcp-port".to_string(),
            "5001".to_string(),
            "--ipc-port".to_string(),
            "5002".to_string(),
            "--web-port".to_string(),
            "5003".to_string(),
            "--host-name".to_string(),
            "Awanish".to_string(),
        ])
        .unwrap();

        assert_eq!(cli.mode, CommandMode::Serve);
        assert_eq!(cli.serve.runtime_mode, ServeRuntimeMode::Host);
        assert_eq!(cli.serve.mcp_port, Some(5001));
        assert_eq!(cli.serve.ipc_port, Some(5002));
        assert_eq!(cli.serve.web_port, Some(5003));
        assert_eq!(cli.serve.host_name.as_deref(), Some("Awanish"));
        assert!(
            cli.db_path.ends_with("demo\\.harmony\\memory.db")
                || cli.db_path.ends_with("demo/.harmony/memory.db")
        );
    }

    #[test]
    fn infer_project_root_prefers_parent_of_harmony_dir() {
        let root = infer_project_root(Path::new("C:/repo/.harmony/memory.db"));
        assert!(root.ends_with("C:\\repo") || root.ends_with("C:/repo"));
    }

    #[test]
    fn infer_repo_root_from_release_binary() {
        let root =
            infer_repo_root_from_exe(Path::new("C:/repo/target/release/harmony-mcp.exe")).unwrap();
        assert!(root.ends_with("C:\\repo") || root.ends_with("C:/repo"));
    }

    #[test]
    fn bootstrap_tolerates_invalid_existing_config() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("harmony-mcp-invalid-config-{suffix}"));
        let harmony_dir = root.join(".harmony");
        std::fs::create_dir_all(&harmony_dir).unwrap();
        std::fs::write(
            harmony_dir.join("config.toml"),
            "[broken]\nenv = {\n  KEY = \"value\"\n}\n",
        )
        .unwrap();

        let bootstrap = bootstrap_project(None, &harmony_dir.join("memory.db"), true).unwrap();
        assert!(bootstrap.config_warning.is_some());

        let debug_log = harmony_dir.join("mcp-debug.log");
        let log_text = std::fs::read_to_string(debug_log).unwrap();
        assert!(log_text.contains("Config warning:"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn bootstrap_can_skip_opening_store() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("harmony-mcp-client-bootstrap-{suffix}"));
        let harmony_dir = root.join(".harmony");
        std::fs::create_dir_all(&harmony_dir).unwrap();

        let bootstrap =
            bootstrap_project(Some(&root), &harmony_dir.join("memory.db"), false).unwrap();
        assert!(bootstrap.store.is_none());
        assert_eq!(bootstrap.config.network.mode, "host");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn sync_report_records_agent_edit() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("harmony-mcp-sync-{suffix}"));
        let harmony_dir = root.join(".harmony");
        std::fs::create_dir_all(&harmony_dir).unwrap();
        std::fs::write(root.join("notes.txt"), "hello from sync\n").unwrap();

        let bootstrap = bootstrap_project(None, &harmony_dir.join("memory.db"), true).unwrap();
        let report = sync_report(
            &bootstrap,
            &SyncOptions {
                files: vec!["notes.txt".to_string()],
                actor_id: "agent:zed-assistant".to_string(),
                task_prompt: None,
                since_seconds: 60,
            },
        )
        .unwrap();

        assert!(report.contains("Synced files: 1"));
        assert!(report.contains("notes.txt"));

        let agents = bootstrap.store().unwrap().get_agents().unwrap();
        assert_eq!(agents.len(), 1);

        let _ = std::fs::remove_dir_all(root);
    }
}
