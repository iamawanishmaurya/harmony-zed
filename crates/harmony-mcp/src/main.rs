mod tools;
mod transport;
mod types;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::{fs::OpenOptions, io::Write};
use harmony_core::HarmonyConfig;
use harmony_memory::store::MemoryStore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let cli = Cli::parse(&args)?;
    let bootstrap = bootstrap_project(&cli.db_path)?;

    match cli.mode {
        CommandMode::Serve => {
            maybe_init_tracing();
            install_file_trace_log(&bootstrap.db_path);
            tracing::info!(
                "Starting harmony-mcp server with db: {}",
                bootstrap.db_path.display()
            );

            let store = Arc::new(Mutex::new(bootstrap.store));
            transport::run_stdio_server(store).await?;
        }
        CommandMode::Pulse => {
            print!("{}", pulse_report(&bootstrap)?);
        }
        CommandMode::Doctor => {
            print!("{}", doctor_report(&bootstrap));
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
    Help,
}

#[derive(Debug, Clone)]
struct Cli {
    mode: CommandMode,
    db_path: PathBuf,
}

struct Bootstrap {
    db_path: PathBuf,
    project_root: PathBuf,
    config_path: PathBuf,
    config_created: bool,
    config_warning: Option<String>,
    store: MemoryStore,
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

        let db_path = parse_flag(args, "--db-path", start_index)
            .map(PathBuf::from)
            .unwrap_or_else(default_db_path);

        Ok(Self {
            mode,
            db_path: absolutize(&db_path),
        })
    }
}

fn bootstrap_project(db_path: &Path) -> anyhow::Result<Bootstrap> {
    let project_root = infer_project_root(db_path);
    let config_path = project_root.join(".harmony").join("config.toml");
    let config_created = !config_path.exists();

    let config_warning = match HarmonyConfig::load(&project_root) {
        Ok(_) => None,
        Err(err) if config_path.exists() => {
            let warning = format!(
                "Config warning: {err}. Continuing with defaults until {} is fixed.",
                config_path.display()
            );
            append_bootstrap_log(&project_root, &warning);
            Some(warning)
        }
        Err(err) => return Err(err),
    };
    let store = MemoryStore::open(db_path)?;

    Ok(Bootstrap {
        db_path: db_path.to_path_buf(),
        project_root,
        config_path,
        config_created,
        config_warning,
        store,
    })
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
        parent
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or(parent)
    } else {
        parent
    }
}

fn pulse_report(bootstrap: &Bootstrap) -> anyhow::Result<String> {
    let overlaps = bootstrap.store.get_pending_overlaps()?;
    let agents = bootstrap.store.get_agents()?;

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
                overlap.change_a.actor_id.0,
                overlap.change_b.actor_id.0,
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
    let overlaps = bootstrap.store.get_pending_overlaps().unwrap_or_default();
    let agents = bootstrap.store.get_agents().unwrap_or_default();
    let config_state = if bootstrap.config_created {
        "created"
    } else {
        "verified"
    };

    let mut lines = vec![
        "Harmony Doctor".to_string(),
        format!("Project: {}", bootstrap.project_root.display()),
        format!("Database: {} (ok)", bootstrap.db_path.display()),
        format!(
            "Config: {} ({config_state})",
            bootstrap.config_path.display()
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

fn parse_flag(args: &[String], flag: &str, start_index: usize) -> Option<String> {
    args.iter()
        .skip(start_index)
        .position(|arg| arg == flag)
        .and_then(|index| args.get(start_index + index + 1))
        .cloned()
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

    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        let _ = writeln!(file, "[bootstrap] {message}");
    }
}

fn help_text() -> &'static str {
    "Harmony MCP\n\
\n\
Usage:\n\
  harmony-mcp [--db-path PATH]\n\
  harmony-mcp serve [--db-path PATH]\n\
  harmony-mcp pulse [--db-path PATH]\n\
  harmony-mcp doctor [--db-path PATH]\n\
\n\
Commands:\n\
  serve   Start the stdio MCP server (default)\n\
  pulse   Print a one-shot overlap summary for the project database\n\
  doctor  Verify the local Harmony setup and print the resolved paths\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parse_defaults_to_serve_mode() {
        let cli = Cli::parse(&["harmony-mcp".to_string()]).unwrap();
        assert_eq!(cli.mode, CommandMode::Serve);
        assert!(cli.db_path.ends_with(".harmony\\memory.db") || cli.db_path.ends_with(".harmony/memory.db"));
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
        assert!(cli.db_path.ends_with("demo\\.harmony\\memory.db") || cli.db_path.ends_with("demo/.harmony/memory.db"));
    }

    #[test]
    fn infer_project_root_prefers_parent_of_harmony_dir() {
        let root = infer_project_root(Path::new("C:/repo/.harmony/memory.db"));
        assert!(root.ends_with("C:\\repo") || root.ends_with("C:/repo"));
    }

    #[test]
    fn infer_repo_root_from_release_binary() {
        let root = infer_repo_root_from_exe(Path::new("C:/repo/target/release/harmony-mcp.exe")).unwrap();
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

        let bootstrap = bootstrap_project(&harmony_dir.join("memory.db")).unwrap();
        assert!(bootstrap.config_warning.is_some());

        let debug_log = harmony_dir.join("mcp-debug.log");
        let log_text = std::fs::read_to_string(debug_log).unwrap();
        assert!(log_text.contains("Config warning:"));

        let _ = std::fs::remove_dir_all(root);
    }
}
