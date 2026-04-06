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
        // Extension initialization — sidecar will be spawned on first workspace open
        HarmonyExtension {
            sidecar_pid: None,
            ipc_connected: false,
        }
    }
}

zed::register_extension!(HarmonyExtension);
