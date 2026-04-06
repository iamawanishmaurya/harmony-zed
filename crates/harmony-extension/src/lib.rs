//! Harmony Zed Extension
//!
//! WASM-compiled extension that provides:
//! - Agent Team sidebar panel (Cmd+Shift+T)
//! - Harmony Pulse panel (Cmd+Shift+H)
//! - Ghost highlight decorations for shadow diffs
//! - Sidecar lifecycle management (auto-start, restart, shutdown)

mod sidecar;
mod panels;
mod config;
mod ipc;

use zed_extension_api as zed;

struct HarmonyExtension {
    /// PID of the native sidecar process, if running.
    sidecar_pid: Option<u32>,
    /// IPC connection state.
    ipc_connected: bool,
}

impl zed::Extension for HarmonyExtension {
    fn new() -> Self {
        HarmonyExtension {
            sidecar_pid: None,
            ipc_connected: false,
        }
    }

    /// Implement slash commands for manual overlap UI triggers
    fn complete_slash_command_argument(
        &self,
        command: zed::SlashCommand,
        _args: Vec<String>,
    ) -> Result<Vec<zed::SlashCommandArgumentCompletion>, String> {
        Ok(vec![])
    }

    fn run_slash_command(
        &self,
        command: zed::SlashCommand,
        _args: Vec<String>,
        _worktree: Option<&zed::Worktree>,
    ) -> Result<zed::SlashCommandOutput, String> {
        if command.name == "harmony-pulse" {
            let text = "🔔 HARMONY PULSE 🔔\n\nNo active overlaps found. The sidecar memory.db is watching!".to_string();
            return Ok(zed::SlashCommandOutput {
                text,
                sections: vec![],
            });
        }
        
        Err(format!("Unknown command: {}", command.name))
    }
}

zed::register_extension!(HarmonyExtension);
