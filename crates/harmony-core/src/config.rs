//! Configuration loader for `.harmony/config.toml` (§14).
//!
//! Auto-creates with defaults on first run. All fields have sensible defaults
//! so missing keys are tolerated.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Complete Harmony config matching §14 schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarmonyConfig {
    #[serde(default = "GeneralConfig::default")]
    pub general: GeneralConfig,

    #[serde(default = "HumanConfig::default")]
    pub human: HumanConfig,

    #[serde(default = "NetworkConfig::default")]
    pub network: NetworkConfig,

    #[serde(default = "AnalysisConfig::default")]
    pub analysis: AnalysisConfig,

    #[serde(default = "MemoryConfig::default")]
    pub memory: MemoryConfig,

    #[serde(default = "NegotiationConfig::default")]
    pub negotiation: NegotiationConfig,

    #[serde(default = "AgentsConfig::default")]
    pub agents: AgentsConfig,

    #[serde(default = "UiConfig::default")]
    pub ui: UiConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    #[serde(default = "default_session_id")]
    pub session_id: String,

    #[serde(default = "default_overlap_window")]
    pub overlap_window_minutes: u32,

    #[serde(default = "default_max_recent_tags")]
    pub max_recent_tags: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanConfig {
    #[serde(default = "default_username")]
    pub username: String,

    #[serde(default = "default_actor_id")]
    pub actor_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    #[serde(default = "default_network_mode")]
    pub mode: String,

    #[serde(default = "default_mcp_port")]
    pub mcp_port: u16,

    #[serde(default = "default_ipc_port")]
    pub ipc_port: u16,

    #[serde(default = "default_web_port")]
    pub web_port: u16,

    #[serde(default)]
    pub host_url: Option<String>,

    #[serde(default = "default_true")]
    pub auto_sync: bool,

    #[serde(default = "default_sync_interval_seconds")]
    pub sync_interval_seconds: u64,

    #[serde(default = "default_max_sync_file_bytes")]
    pub max_sync_file_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisConfig {
    #[serde(default = "default_true")]
    pub treesitter_enabled: bool,

    #[serde(default = "default_lsp_mode")]
    pub lsp_mode: String,

    #[serde(default = "default_sandbox_mode")]
    pub sandbox_mode: String,

    #[serde(default = "default_sandbox_timeout")]
    pub sandbox_timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    #[serde(default = "default_embedding_model")]
    pub embedding_model: String,

    #[serde(default)]
    pub model_cache_dir: String,

    #[serde(default = "default_max_records")]
    pub max_records_in_memory: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NegotiationConfig {
    #[serde(default = "default_negotiation_backend")]
    pub negotiation_backend: String,

    #[serde(default)]
    pub api_key: Option<String>,

    #[serde(default)]
    pub model: Option<String>,

    #[serde(default)]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentsConfig {
    #[serde(default)]
    pub registry: Vec<AgentEndpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEndpoint {
    pub name: String,
    pub endpoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default = "default_ghost_add")]
    pub ghost_add_color: String,

    #[serde(default = "default_ghost_remove")]
    pub ghost_remove_color: String,

    #[serde(default = "default_notification_duration")]
    pub notification_duration_seconds: u32,
}

// ── Default impls ─────────────────────────────────────────────────────────────

impl Default for HarmonyConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            human: HumanConfig::default(),
            network: NetworkConfig::default(),
            analysis: AnalysisConfig::default(),
            memory: MemoryConfig::default(),
            negotiation: NegotiationConfig::default(),
            agents: AgentsConfig::default(),
            ui: UiConfig::default(),
        }
    }
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            session_id: Uuid::new_v4().to_string(),
            overlap_window_minutes: 30,
            max_recent_tags: 500,
        }
    }
}

impl Default for HumanConfig {
    fn default() -> Self {
        Self {
            username: whoami::fallible::username().unwrap_or_else(|_| "developer".into()),
            actor_id: format!("human:{}", whoami::fallible::username().unwrap_or_else(|_| "developer".into())),
        }
    }
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            treesitter_enabled: true,
            lsp_mode: "auto".into(),
            sandbox_mode: "complex_only".into(),
            sandbox_timeout_seconds: 60,
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            mode: "host".into(),
            mcp_port: 4231,
            ipc_port: 4232,
            web_port: 4233,
            host_url: None,
            auto_sync: true,
            sync_interval_seconds: 3,
            max_sync_file_bytes: 1024 * 1024,
        }
    }
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            embedding_model: "bge-small-en-v1.5".into(),
            model_cache_dir: String::new(),
            max_records_in_memory: 10000,
        }
    }
}

impl Default for NegotiationConfig {
    fn default() -> Self {
        Self {
            negotiation_backend: "agent".into(),
            api_key: None,
            model: None,
            base_url: None,
        }
    }
}

impl Default for AgentsConfig {
    fn default() -> Self {
        Self {
            registry: vec![
                AgentEndpoint { name: "opencode".into(), endpoint: "http://localhost:4231".into() },
                AgentEndpoint { name: "gemini-cli".into(), endpoint: "http://localhost:4232".into() },
            ],
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            ghost_add_color: "#7ee8a280".into(),
            ghost_remove_color: "#f0606060".into(),
            notification_duration_seconds: 8,
        }
    }
}

fn default_session_id() -> String { Uuid::new_v4().to_string() }
fn default_overlap_window() -> u32 { 30 }
fn default_max_recent_tags() -> u32 { 500 }
fn default_username() -> String { "developer".into() }
fn default_actor_id() -> String { "human:developer".into() }
fn default_network_mode() -> String { "host".into() }
fn default_mcp_port() -> u16 { 4231 }
fn default_ipc_port() -> u16 { 4232 }
fn default_web_port() -> u16 { 4233 }
fn default_sync_interval_seconds() -> u64 { 3 }
fn default_max_sync_file_bytes() -> u64 { 1024 * 1024 }
fn default_true() -> bool { true }
fn default_lsp_mode() -> String { "auto".into() }
fn default_sandbox_mode() -> String { "complex_only".into() }
fn default_sandbox_timeout() -> u64 { 60 }
fn default_embedding_model() -> String { "bge-small-en-v1.5".into() }
fn default_max_records() -> u32 { 10000 }
fn default_negotiation_backend() -> String { "agent".into() }
fn default_ghost_add() -> String { "#7ee8a280".into() }
fn default_ghost_remove() -> String { "#f0606060".into() }
fn default_notification_duration() -> u32 { 8 }

// ── Load/Save ─────────────────────────────────────────────────────────────────

impl HarmonyConfig {
    /// Load config from `.harmony/config.toml` relative to project root.
    /// Creates with defaults if missing.
    pub fn load(project_root: &Path) -> anyhow::Result<Self> {
        let config_path = project_root.join(".harmony").join("config.toml");
        Self::load_from_path(&config_path)
    }

    /// Load from an explicit path.
    pub fn load_from_path(path: &Path) -> anyhow::Result<Self> {
        if path.exists() {
            let content = std::fs::read_to_string(path)?;
            let config: HarmonyConfig = toml::from_str(&content)
                .map_err(|e| anyhow::anyhow!("Config parse error: {}", e))?;
            Ok(config)
        } else {
            // Auto-create with commented template
            let config = HarmonyConfig::default();
            config.save_with_template(path)?;
            Ok(config)
        }
    }

    /// Save current config to path using toml::to_string_pretty.
    pub fn save_to_path(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)
            .map_err(|e| anyhow::anyhow!("Config serialize error: {}", e))?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Save with the rich commented template (used on first-run auto-create).
    pub fn save_with_template(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let username = &self.human.username;
        let session_id = &self.general.session_id;
        let content = format!(r##"# Harmony Configuration
# Auto-generated on first run. Edit to customize.

[general]
# Session identifier (auto-generated, do not edit)
session_id = "{session_id}"

# How long (minutes) to look back when checking for overlapping changes
overlap_window_minutes = 30

# How many recent tags to keep in memory (older ones stay in DB only)
max_recent_tags = 500

[human]
# Display name for the human participant
username = "{username}"
actor_id = "human:{username}"

[network]
# "host" runs the shared Harmony network servers on this machine.
# "client" connects local Harmony tooling to another host's MCP server.
mode = "host"

# Ports used when running in host mode.
mcp_port = 4231
ipc_port = 4232
web_port = 4233

# In client mode, point this to the host machine's MCP base URL.
# host_url = "http://192.168.1.10:4231"

# Automatically replicate project files/folders through the Harmony host.
auto_sync = true

# How often each connected machine scans for local project changes.
sync_interval_seconds = 3

# Files larger than this are skipped by automatic replication.
max_sync_file_bytes = 1048576

[analysis]
# Tree-sitter analysis always enabled
treesitter_enabled = true

# LSP analysis: "auto" tries to find LSP, falls back gracefully if not found
# Options: "auto", "disabled"
lsp_mode = "auto"

# Complexity threshold for sandbox escalation
# Options: "always", "complex_only" (default), "never"
sandbox_mode = "complex_only"

# Maximum time (seconds) for sandbox test run before timeout
sandbox_timeout_seconds = 60

[memory]
# Embedding model. Currently only "bge-small-en-v1.5" supported.
embedding_model = "bge-small-en-v1.5"

# Path to fastembed model cache (default: ~/.cache/fastembed)
# Leave empty for default
model_cache_dir = ""

# Max memory records to load into RAM for similarity search
max_records_in_memory = 10000

[negotiation]
# Options: "agent" (default, no API key needed) | "openai" | "anthropic" | "disabled"
negotiation_backend = "agent"

# --- Uncomment ONE block below to use a specific backend ---

# OpenAI / GPT
# negotiation_backend = "openai"
# api_key = "sk-..."
# model = "gpt-4o"
# base_url = "https://api.openai.com/v1"

# GitHub Copilot (OpenAI-compatible)
# negotiation_backend = "openai"
# api_key = "ghp_..."
# model = "gpt-4o"
# base_url = "https://api.githubcopilot.com"

# Anthropic Claude
# negotiation_backend = "anthropic"
# api_key = "sk-ant-..."
# model = "claude-sonnet-4-6"

# Ollama (local)
# negotiation_backend = "openai"
# api_key = "ollama"
# model = "llama3.3"
# base_url = "http://localhost:11434/v1"

# LM Studio (local)
# negotiation_backend = "openai"
# api_key = "lm-studio"
# model = "local-model"
# base_url = "http://localhost:1234/v1"

# Disabled
# negotiation_backend = "disabled"

[agents]
# Known ACP agent endpoints. Harmony will try these on spawn.

[[agents.registry]]
name = "opencode"
endpoint = "http://localhost:4231"

[[agents.registry]]
name = "gemini-cli"
endpoint = "http://localhost:4232"

[ui]
# Ghost highlight colors (hex, with alpha)
ghost_add_color = "#7ee8a280"
ghost_remove_color = "#f0606060"

# Pulse notification duration before auto-dismiss (seconds)
notification_duration_seconds = 8
"##);
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Get the database path relative to project root.
    pub fn db_path(project_root: &Path) -> PathBuf {
        project_root.join(".harmony").join("memory.db")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = HarmonyConfig::default();
        assert_eq!(config.general.overlap_window_minutes, 30);
        assert_eq!(config.analysis.lsp_mode, "auto");
        assert_eq!(config.negotiation.negotiation_backend, "agent");
        assert_eq!(config.ui.ghost_add_color, "#7ee8a280");
        assert_eq!(config.network.mode, "host");
        assert_eq!(config.network.mcp_port, 4231);
        assert_eq!(config.agents.registry.len(), 2);
    }

    #[test]
    fn test_load_creates_default() {
        let tmp = TempDir::new().unwrap();
        let config = HarmonyConfig::load(tmp.path()).unwrap();
        assert_eq!(config.general.overlap_window_minutes, 30);

        // File should now exist
        let config_path = tmp.path().join(".harmony").join("config.toml");
        assert!(config_path.exists());
    }

    #[test]
    fn test_round_trip_save_load() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");

        let mut config = HarmonyConfig::default();
        config.human.username = "testuser".into();
        config.general.overlap_window_minutes = 60;
        config.save_to_path(&path).unwrap();

        let loaded = HarmonyConfig::load_from_path(&path).unwrap();
        assert_eq!(loaded.human.username, "testuser");
        assert_eq!(loaded.general.overlap_window_minutes, 60);
    }

    #[test]
    fn test_partial_toml_uses_defaults() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, r#"
[human]
username = "awanish"

[network]
host_url = "http://192.168.1.10:4231"
"#).unwrap();

        let config = HarmonyConfig::load_from_path(&path).unwrap();
        assert_eq!(config.human.username, "awanish");
        assert_eq!(config.network.mode, "host");
        assert_eq!(
            config.network.host_url.as_deref(),
            Some("http://192.168.1.10:4231")
        );
        // Everything else should be default
        assert_eq!(config.general.overlap_window_minutes, 30);
        assert_eq!(config.analysis.lsp_mode, "auto");
    }

    #[test]
    fn test_db_path() {
        let root = Path::new("/project");
        let db = HarmonyConfig::db_path(root);
        assert!(db.ends_with(".harmony/memory.db") || db.ends_with(".harmony\\memory.db"));
    }
}
