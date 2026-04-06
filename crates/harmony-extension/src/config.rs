//! Extension-side config reader.
//!
//! Reads `.harmony/config.toml` using the Zed extension filesystem API.
//! Falls back to defaults if the file doesn't exist.

use serde::{Deserialize, Serialize};

/// Minimal subset of HarmonyConfig that the WASM extension needs.
/// (Full config is parsed by the native sidecar.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionConfig {
    /// Ghost highlight colors
    pub ghost_add_color: String,
    pub ghost_remove_color: String,
    /// Notification auto-dismiss duration
    pub notification_duration_seconds: u32,
    /// Human display name
    pub username: String,
}

impl Default for ExtensionConfig {
    fn default() -> Self {
        Self {
            ghost_add_color: "#7ee8a280".into(),
            ghost_remove_color: "#f0606060".into(),
            notification_duration_seconds: 8,
            username: "developer".into(),
        }
    }
}

impl ExtensionConfig {
    /// Parse from TOML content (used when reading from Zed's sandboxed FS).
    pub fn from_toml(content: &str) -> Self {
        #[derive(Deserialize)]
        struct PartialConfig {
            ui: Option<UiSection>,
            human: Option<HumanSection>,
        }

        #[derive(Deserialize)]
        struct UiSection {
            ghost_add_color: Option<String>,
            ghost_remove_color: Option<String>,
            notification_duration_seconds: Option<u32>,
        }

        #[derive(Deserialize)]
        struct HumanSection {
            username: Option<String>,
        }

        let defaults = ExtensionConfig::default();

        match toml::from_str::<PartialConfig>(content) {
            Ok(parsed) => {
                let ui = parsed.ui.unwrap_or(UiSection {
                    ghost_add_color: None,
                    ghost_remove_color: None,
                    notification_duration_seconds: None,
                });
                let human = parsed.human.unwrap_or(HumanSection { username: None });

                ExtensionConfig {
                    ghost_add_color: ui.ghost_add_color.unwrap_or(defaults.ghost_add_color),
                    ghost_remove_color: ui.ghost_remove_color.unwrap_or(defaults.ghost_remove_color),
                    notification_duration_seconds: ui.notification_duration_seconds
                        .unwrap_or(defaults.notification_duration_seconds),
                    username: human.username.unwrap_or(defaults.username),
                }
            }
            Err(_) => defaults,
        }
    }
}
