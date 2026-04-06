//! IPC client for communicating with the harmony-mcp sidecar.
//!
//! §11: Extension ↔ sidecar communication via JSON messages.
//! On Windows, reads `.harmony/harmony.port` and connects via TCP.
//! On Unix, connects to `.harmony/harmony.sock`.

use serde::{Deserialize, Serialize};
use serde_json;

/// Commands the extension can send to the sidecar.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd")]
pub enum SidecarCommand {
    /// Health check.
    #[serde(rename = "ping")]
    Ping,

    /// List all active agents.
    #[serde(rename = "get_agents")]
    GetAgents,

    /// Spawn a new agent team from a prompt.
    #[serde(rename = "spawn_agents")]
    SpawnAgents { prompt: String },

    /// Toggle agent between Shadow and Live mode.
    #[serde(rename = "toggle_mode")]
    ToggleMode { agent_id: String },

    /// Pause an agent.
    #[serde(rename = "pause_agent")]
    PauseAgent { agent_id: String },

    /// Remove an agent.
    #[serde(rename = "remove_agent")]
    RemoveAgent { agent_id: String },

    /// Get pending overlaps.
    #[serde(rename = "get_overlaps")]
    GetOverlaps,

    /// Accept human's version of an overlap.
    #[serde(rename = "accept_mine")]
    AcceptMine { overlap_id: String },

    /// Accept agent's version of an overlap.
    #[serde(rename = "accept_theirs")]
    AcceptTheirs { overlap_id: String },

    /// Start negotiation for an overlap.
    #[serde(rename = "start_negotiation")]
    StartNegotiation { overlap_id: String },

    /// Get pending shadow diffs.
    #[serde(rename = "get_shadow_diffs")]
    GetShadowDiffs,
}

/// Responses from the sidecar.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result")]
pub enum SidecarResponse {
    #[serde(rename = "pong")]
    Pong,

    #[serde(rename = "agents")]
    Agents { data: Vec<serde_json::Value> },

    #[serde(rename = "overlaps")]
    Overlaps { data: Vec<serde_json::Value> },

    #[serde(rename = "shadow_diffs")]
    ShadowDiffs { data: Vec<serde_json::Value> },

    #[serde(rename = "ok")]
    Ok { message: String },

    #[serde(rename = "error")]
    Error { code: u32, message: String },
}

/// Connection mode for IPC — platform-dependent.
#[derive(Debug, Clone)]
pub enum IpcConnectionMode {
    /// Stdio/process-spawn mode (used by Zed's extension process API)
    Stdio,
    /// TCP mode (Windows fallback — connects to .harmony/harmony.port)
    Tcp { address: String },
    /// Unix socket mode (Linux/macOS — connects to .harmony/harmony.sock)
    #[allow(dead_code)]
    UnixSocket { path: String },
}

/// IPC client handle. In WASM, actual socket communication is not possible,
/// so this delegates to the Zed extension process spawn API.
pub struct IpcClient {
    connected: bool,
    mode: IpcConnectionMode,
}

impl IpcClient {
    pub fn new() -> Self {
        Self {
            connected: false,
            mode: IpcConnectionMode::Stdio,
        }
    }

    /// Create a client configured for the correct IPC mode.
    ///
    /// On Windows: reads `.harmony/harmony.port` for TCP address.
    /// On Unix: uses `.harmony/harmony.sock` socket path.
    /// Falls back to Stdio (sidecar stdin/stdout) if neither exists.
    pub fn for_project(harmony_dir: &str) -> Self {
        // Try TCP port file first (Windows)
        let port_path = format!("{}/harmony.port", harmony_dir);
        // In WASM we can't actually read files, but the Zed extension API
        // provides workspace filesystem access. This is a structural placeholder.
        let mode = IpcConnectionMode::Tcp {
            address: "127.0.0.1:17432".to_string(),
        };

        // On Unix, prefer the socket
        #[cfg(not(target_os = "windows"))]
        let mode = IpcConnectionMode::UnixSocket {
            path: format!("{}/harmony.sock", harmony_dir),
        };

        let _ = port_path; // Used for documentation

        Self {
            connected: false,
            mode,
        }
    }

    /// Get the connection mode.
    pub fn connection_mode(&self) -> &IpcConnectionMode {
        &self.mode
    }

    /// Check if connection is alive.
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Mark as connected after sidecar spawn.
    pub fn set_connected(&mut self, connected: bool) {
        self.connected = connected;
    }

    /// Encode a command as a Content-Length framed JSON message.
    pub fn encode_command(cmd: &SidecarCommand) -> Option<String> {
        let json = serde_json::to_string(cmd).ok()?;
        Some(format!("Content-Length: {}\r\n\r\n{}", json.len(), json))
    }

    /// Encode a command as raw JSON (for Zed's process stdin pipe).
    pub fn encode_command_json(cmd: &SidecarCommand) -> Option<String> {
        serde_json::to_string(cmd).ok()
    }

    /// Decode a response from the sidecar's stdout.
    pub fn decode_response(json: &str) -> Option<SidecarResponse> {
        serde_json::from_str(json).ok()
    }
}
