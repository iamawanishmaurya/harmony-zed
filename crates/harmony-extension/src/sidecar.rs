//! Sidecar lifecycle management.
//!
//! §13, Task 13: Extension spawns `harmony-mcp` as a child process.
//! Auto-restarts with exponential backoff on crash.
//! Clean shutdown on extension deactivation.

/// Configuration for the sidecar process.
pub struct SidecarConfig {
    /// Path to the harmony-mcp binary (resolved at runtime).
    pub binary_path: String,
    /// Path to the project database.
    pub db_path: String,
    /// Maximum restart attempts before giving up.
    pub max_restarts: u32,
    /// Current restart count.
    pub restart_count: u32,
}

impl SidecarConfig {
    pub fn new(binary_path: String, db_path: String) -> Self {
        Self {
            binary_path,
            db_path,
            max_restarts: 5,
            restart_count: 0,
        }
    }

    /// Get the backoff delay for the current restart attempt.
    pub fn backoff_ms(&self) -> u64 {
        // Exponential backoff: 1s, 2s, 4s, 8s, 16s
        1000 * (1u64 << self.restart_count.min(4))
    }

    /// Record a restart and return whether we should keep trying.
    pub fn record_restart(&mut self) -> bool {
        self.restart_count += 1;
        self.restart_count <= self.max_restarts
    }

    /// Reset restart count (called on successful connection).
    pub fn reset_restarts(&mut self) {
        self.restart_count = 0;
    }
}

/// Status of the sidecar process.
#[derive(Debug, Clone, PartialEq)]
pub enum SidecarStatus {
    /// Not yet started.
    NotStarted,
    /// Running and connected.
    Running,
    /// Disconnected, will retry.
    Disconnected { retry_in_ms: u64 },
    /// Permanently failed after max retries.
    Failed { message: String },
}

pub struct SidecarHandle {
    pub config: SidecarConfig,
    pub status: SidecarStatus,
    pub pid: Option<u32>,
}

impl SidecarHandle {
    pub fn new(config: SidecarConfig) -> Self {
        Self {
            config,
            status: SidecarStatus::NotStarted,
            pid: None,
        }
    }

    /// Get the CLI arguments for spawning the sidecar.
    pub fn spawn_args(&self) -> Vec<String> {
        vec![
            "--stdio-bridge".to_string(),
            "--db-path".to_string(),
            self.config.db_path.clone(),
        ]
    }

    /// Record that sidecar started successfully.
    pub fn mark_running(&mut self, pid: u32) {
        self.pid = Some(pid);
        self.status = SidecarStatus::Running;
        self.config.reset_restarts();
    }

    /// Record that sidecar disconnected.
    pub fn mark_disconnected(&mut self) {
        self.pid = None;
        if self.config.record_restart() {
            let delay = self.config.backoff_ms();
            self.status = SidecarStatus::Disconnected { retry_in_ms: delay };
        } else {
            self.status = SidecarStatus::Failed {
                message: format!("Sidecar failed after {} restart attempts", self.config.max_restarts),
            };
        }
    }
}
