//! Harmony Zed Extension
//!
//! WASM-compiled extension that provides:
//! - A project-local Harmony MCP context server
//! - The `/harmony-pulse` diagnostic slash command
//! - The `/harmony-dashboard` helper for opening the network dashboard

mod sidecar;
mod panels;
mod config;
mod ipc;

use zed_extension_api as zed;

struct HarmonyExtension;

#[derive(Default, serde::Deserialize)]
struct HarmonyContextServerSettings {
    db_path: Option<String>,
}

impl zed::Extension for HarmonyExtension {
    fn new() -> Self {
        HarmonyExtension
    }

    fn complete_slash_command_argument(
        &self,
        _command: zed::SlashCommand,
        _args: Vec<String>,
    ) -> Result<Vec<zed::SlashCommandArgumentCompletion>, String> {
        Ok(vec![])
    }

    fn run_slash_command(
        &self,
        command: zed::SlashCommand,
        args: Vec<String>,
        worktree: Option<&zed::Worktree>,
    ) -> Result<zed::SlashCommandOutput, String> {
        match command.name.as_str() {
            "harmony-pulse" => {
                let binary = Self::resolve_binary_path()?;
                let db_path = Self::resolve_db_path_for_worktree(worktree, &binary);
                let mut native_command = zed::Command::new(binary)
                    .arg("pulse")
                    .arg("--db-path")
                    .arg(db_path);
                Self::run_native_slash_command(&mut native_command, "Harmony Pulse")
            }
            "harmony-sync" => {
                let binary = Self::resolve_binary_path()?;
                let db_path = Self::resolve_db_path_for_worktree(worktree, &binary);
                let mut native_command = zed::Command::new(binary)
                    .arg("sync")
                    .arg("--db-path")
                    .arg(db_path)
                    .arg("--actor-id")
                    .arg("agent:zed-assistant");

                if args.is_empty() {
                    native_command = native_command.arg("--since-seconds").arg("900");
                } else {
                    for path in args {
                        native_command = native_command.arg("--file").arg(path);
                    }
                }

                Self::run_native_slash_command(&mut native_command, "Harmony Sync")
            }
            "harmony-dashboard" => {
                let url = args
                    .first()
                    .map(|value| Self::normalize_dashboard_url(value))
                    .unwrap_or_else(|| "http://localhost:4233".to_string());
                Self::open_dashboard(&url)
            }
            other => Err(format!("Unknown command: {other}")),
        }
    }

    fn context_server_command(
        &mut self,
        context_server_id: &zed::ContextServerId,
        project: &zed::Project,
    ) -> Result<zed::Command, String> {
        if context_server_id.as_ref() != "harmony-memory" {
            return Err(format!("Unknown context server: {}", context_server_id));
        }

        let binary = Self::resolve_binary_path()?;
        let db_path = Self::resolve_db_path_for_project(project)?;

        Ok(zed::Command::new(binary)
            .arg("--stdio-bridge")
            .arg("--db-path")
            .arg(db_path))
    }

    fn context_server_configuration(
        &mut self,
        context_server_id: &zed::ContextServerId,
        project: &zed::Project,
    ) -> Result<Option<zed::ContextServerConfiguration>, String> {
        if context_server_id.as_ref() != "harmony-memory" {
            return Ok(None);
        }

        let binary = Self::resolve_binary_path()?;
        let db_path = Self::resolve_db_path_for_project(project)?;
        let debug_log = Self::debug_log_path(&db_path);

        Ok(Some(zed::ContextServerConfiguration {
            installation_instructions: format!(
                "Build the native sidecar before enabling Harmony in Zed:\n\n```powershell\ncargo build --release -p harmony-mcp\n```\n\nZed now launches Harmony through the network bridge automatically when you click Configure Server.\n\nSame-network setup:\n1. On the host laptop, set `.harmony/config.toml` `[network].mode = \"host\"`\n2. On the client laptop, set `[network].mode = \"client\"` and `host_url = \"http://HOST_IP:4231\"`\n3. Click Configure Server in Zed on each machine\n4. Open the dashboard with `/harmony-dashboard`\n\nHarmony binary:\n{}\n\nHarmony database for this project:\n{}\n\nHarmony debug log:\n{}\n\nUseful slash commands after assistant edits:\n- `/harmony-sync`\n- `/harmony-sync path/to/file`\n- `/harmony-pulse`\n- `/harmony-dashboard`\n\nZed log:\nC:\\Users\\water\\AppData\\Local\\Zed\\logs\\Zed.log\n\nTip: after a failed Configure attempt, inspect the logs with:\n```powershell\nGet-Content -Tail 120 \"{}\"\nGet-Content -Tail 120 \"C:\\Users\\water\\AppData\\Local\\Zed\\logs\\Zed.log\"\n```",
                binary,
                db_path,
                debug_log,
                debug_log,
            ),
            default_settings: "{\"db_path\": \".harmony\\\\memory.db\"}".to_string(),
            settings_schema: "{\"type\":\"object\",\"properties\":{\"db_path\":{\"type\":\"string\",\"description\":\"Database path for this project. Relative paths are resolved from the opened project root.\"}}}".to_string(),
        }))
    }
}

impl HarmonyExtension {
    fn open_dashboard(url: &str) -> Result<zed::SlashCommandOutput, String> {
        let mut command = match zed::current_platform().0 {
            zed::Os::Windows => zed::Command::new(Self::cmd_binary())
                .arg("/c")
                .arg("start")
                .arg("")
                .arg(url),
            zed::Os::Mac => zed::Command::new("open").arg(url),
            _ => zed::Command::new("xdg-open").arg(url),
        };
        let output = command.output()?;
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

        if output.status != Some(0) {
            let details = if stderr.is_empty() {
                format!("Failed to open the Harmony dashboard at {url}.")
            } else {
                stderr
            };
            return Err(details);
        }

        let text = format!(
            "Opened Harmony Dashboard:\n{url}\n\nTip: you can also pass a host URL explicitly, for example `/harmony-dashboard http://192.168.1.10:4233`."
        );
        let end = text.len();
        Ok(zed::SlashCommandOutput {
            sections: vec![zed::SlashCommandOutputSection {
                range: (0..end).into(),
                label: "Harmony Dashboard".to_string(),
            }],
            text,
        })
    }

    fn run_native_slash_command(
        command: &mut zed::Command,
        label: &str,
    ) -> Result<zed::SlashCommandOutput, String> {
        let output = command.output()?;
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

        if output.status != Some(0) {
            let details = if stderr.is_empty() {
                "harmony-mcp exited with a non-zero status.".to_string()
            } else {
                stderr
            };
            return Err(format!("{label} failed: {details}"));
        }

        let text = if stdout.is_empty() {
            format!("{label} returned no output.")
        } else {
            stdout
        };
        let end = text.len();

        Ok(zed::SlashCommandOutput {
            sections: vec![zed::SlashCommandOutputSection {
                range: (0..end).into(),
                label: label.to_string(),
            }],
            text,
        })
    }

    fn resolve_binary_path() -> Result<String, String> {
        if let Some(path) = Self::binary_path_from_manifest() {
            return Ok(path);
        }

        if let Some(path) = Self::binary_path_from_env() {
            return Ok(path);
        }

        Ok(Self::compiled_binary_path())
    }

    fn binary_path_from_env() -> Option<String> {
        std::env::var("HARMONY_MCP_PATH")
            .ok()
            .filter(|path| !path.trim().is_empty())
    }

    fn binary_path_from_manifest() -> Option<String> {
        #[derive(serde::Deserialize)]
        struct ExtensionManifest {
            context_servers: Option<std::collections::BTreeMap<String, ContextServer>>,
        }

        #[derive(serde::Deserialize)]
        struct ContextServer {
            command: Option<String>,
        }

        let manifest: ExtensionManifest =
            toml::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/extension.toml"))).ok()?;
        manifest
            .context_servers?
            .get("harmony-memory")?
            .command
            .clone()
            .filter(|command| !command.trim().is_empty())
    }

    fn binary_name() -> String {
        match zed::current_platform().0 {
            zed::Os::Windows => "harmony-mcp.exe".to_string(),
            _ => "harmony-mcp".to_string(),
        }
    }

    fn compiled_repo_root() -> String {
        let manifest_dir = env!("CARGO_MANIFEST_DIR").replace('/', "\\");
        Self::strip_windows_suffix(&manifest_dir, "\\crates\\harmony-extension")
            .unwrap_or(manifest_dir)
    }

    fn compiled_binary_path() -> String {
        Self::join_windows(
            &Self::join_windows(&Self::compiled_repo_root(), "target\\release"),
            &Self::binary_name(),
        )
    }

    fn cmd_binary() -> String {
        std::env::var("ComSpec")
            .ok()
            .filter(|path| !path.trim().is_empty())
            .unwrap_or_else(|| "cmd.exe".to_string())
    }

    fn normalize_dashboard_url(value: &str) -> String {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return "http://localhost:4233".to_string();
        }

        let without_mcp = trimmed
            .strip_suffix("/mcp")
            .unwrap_or(trimmed)
            .to_string();
        if let Some(prefix) = without_mcp.strip_suffix(":4231") {
            return format!("{prefix}:4233");
        }

        without_mcp
    }

    fn resolve_db_path(binary: &str) -> String {
        if let Ok(path) = std::env::var("HARMONY_DB_PATH") {
            if !path.trim().is_empty() {
                return path;
            }
        }

        let repo_root = Self::repo_root_from_binary(binary).unwrap_or_else(Self::compiled_repo_root);
        Self::join_windows(&repo_root, ".harmony\\memory.db")
    }

    fn resolve_db_path_for_worktree(worktree: Option<&zed::Worktree>, binary: &str) -> String {
        if let Some(worktree) = worktree {
            return Self::join_windows(
                &Self::normalize_windows_path(&worktree.root_path()),
                ".harmony\\memory.db",
            );
        }

        Self::resolve_db_path(binary)
    }

    fn resolve_db_path_for_project(project: &zed::Project) -> Result<String, String> {
        if let Some(path) = Self::db_path_from_context_server_settings(project)? {
            return Ok(path);
        }

        Ok(".harmony\\memory.db".to_string())
    }

    fn db_path_from_context_server_settings(
        project: &zed::Project,
    ) -> Result<Option<String>, String> {
        let settings = zed::settings::ContextServerSettings::for_project("harmony-memory", project)
            .map_err(|error| format!("Failed to read Harmony settings: {error}"))?;
        let Some(settings_value) = settings.settings else {
            return Ok(None);
        };

        let parsed: HarmonyContextServerSettings = serde_json::from_value(settings_value)
            .map_err(|error| format!("Invalid Harmony settings: {error}"))?;
        Ok(parsed.db_path.filter(|path| !path.trim().is_empty()))
    }

    fn repo_root_from_binary(binary: &str) -> Option<String> {
        let normalized = binary.replace('/', "\\");
        let suffix = format!("\\target\\release\\{}", Self::binary_name());
        Self::strip_windows_suffix(&normalized, &suffix)
    }

    fn context_server_launcher_path(binary: &str) -> String {
        let root = Self::repo_root_from_binary(binary).unwrap_or_else(Self::compiled_repo_root);
        Self::join_windows(&root, "run-harmony-mcp.cmd")
    }

    fn strip_windows_suffix(path: &str, suffix: &str) -> Option<String> {
        let path_lower = path.to_ascii_lowercase();
        let suffix_lower = suffix.to_ascii_lowercase();
        path_lower
            .ends_with(&suffix_lower)
            .then(|| path[..path.len() - suffix.len()].to_string())
    }

    fn join_windows(base: &str, tail: &str) -> String {
        let trimmed_base = base.trim_end_matches(['\\', '/']);
        let trimmed_tail = tail.trim_start_matches(['\\', '/']);
        format!("{trimmed_base}\\{trimmed_tail}")
    }

    fn normalize_windows_path(path: &str) -> String {
        path.replace('/', "\\")
    }

    fn parent_windows(path: &str) -> Option<String> {
        let normalized = path.replace('/', "\\");
        normalized
            .rfind('\\')
            .map(|index| normalized[..index].to_string())
    }

    fn debug_log_path(db_path: &str) -> String {
        Self::parent_windows(db_path)
            .map(|dir| Self::join_windows(&dir, "mcp-debug.log"))
            .unwrap_or_else(|| ".harmony\\mcp-debug.log".to_string())
    }

    fn launch_log_path(db_path: &str) -> String {
        Self::parent_windows(db_path)
            .map(|dir| Self::join_windows(&dir, "context-server-launch.log"))
            .unwrap_or_else(|| ".harmony\\context-server-launch.log".to_string())
    }
}

zed::register_extension!(HarmonyExtension);
