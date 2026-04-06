//! Panel state and rendering for the Zed extension.
//!
//! §13, Tasks 11-14: Agent Team panel + Harmony Pulse panel + ghost highlights.
//!
//! NOTE: Actual Zed panel rendering uses the `zed_extension_api` Panel trait,
//! which requires WASM compilation. This module defines the state structures
//! and rendering logic that will be wired to the Zed API.

use serde::{Deserialize, Serialize};

// ── Agent Team Panel ──────────────────────────────────────────────────────────

/// Agent Team panel state (§13).
/// Keybinding: Cmd+Shift+T
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTeamPanel {
    pub agents: Vec<AgentCard>,
    pub spawn_input: String,
    pub is_spawning: bool,
}

/// Rendered agent card in the sidebar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCard {
    pub id: String,
    pub role_name: String,
    pub avatar_key: String,
    pub status: String,       // "idle", "working", "negotiating", "paused", "error"
    pub mode: String,         // "shadow" or "live"
    pub task_count: u32,
    pub status_color: String, // CSS hex color
    pub mode_color: String,   // CSS hex color for mode badge
}

/// Events emitted by the Agent Team panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum AgentTeamEvent {
    SpawnPressed { prompt: String },
    ToggleModePressed { agent_id: String },
    PausePressed { agent_id: String },
    RemovePressed { agent_id: String },
    ViewMemoryPressed { agent_id: String },
    SpawnInputChanged { text: String },
}

impl AgentTeamPanel {
    pub fn new() -> Self {
        Self {
            agents: Vec::new(),
            spawn_input: String::new(),
            is_spawning: false,
        }
    }

    /// Apply status coloring rules from §13.
    pub fn status_color(status: &str) -> &'static str {
        match status {
            "idle" => "#808080",       // gray
            "working" => "#22c55e",    // green (pulsing)
            "negotiating" => "#eab308",// yellow (pulsing)
            "paused" => "#808080",     // gray
            "error" => "#ef4444",      // red
            _ => "#808080",
        }
    }

    /// Apply mode badge coloring from §13.
    pub fn mode_color(mode: &str) -> &'static str {
        match mode {
            "live" => "#3b82f6",    // blue
            "shadow" => "#8b5cf6",  // purple
            _ => "#808080",
        }
    }
}

// ── Harmony Pulse Panel ───────────────────────────────────────────────────────

/// Harmony Pulse panel state (§13).
/// Keybinding: Cmd+Shift+H
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PulsePanel {
    pub overlaps: Vec<OverlapCard>,
    pub selected_overlap_id: Option<String>,
    pub is_loading_impact: bool,
    pub notification_count: u32,
}

/// Rendered overlap card in the Pulse panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlapCard {
    pub id: String,
    pub file_path: String,
    pub time_ago: String,           // "2 min ago"
    pub summary: String,            // Impact summary from Template A
    pub impact_description: String, // Detailed impact text
    pub border_color: String,       // Red/Yellow/Green per complexity
    pub complexity: String,         // "simple", "moderate", "complex"

    // Resolution state
    pub has_negotiation_result: bool,
    pub negotiated_diff: Option<String>,
    pub negotiation_rationale: Option<String>,
}

/// Events emitted by the Pulse panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum PulseEvent {
    AcceptMine { overlap_id: String },
    AcceptTheirs { overlap_id: String },
    StartNegotiation { overlap_id: String },
    ShowWhatIf { overlap_id: String },
    OpenManualEdit { overlap_id: String },
    AcceptNegotiated { overlap_id: String },
    RejectNegotiated { overlap_id: String },
    DismissOverlap { overlap_id: String },
    ClearAll,
}

impl PulsePanel {
    pub fn new() -> Self {
        Self {
            overlaps: Vec::new(),
            selected_overlap_id: None,
            is_loading_impact: false,
            notification_count: 0,
        }
    }

    /// Border color based on impact complexity (§13).
    pub fn complexity_border_color(complexity: &str) -> &'static str {
        match complexity {
            "complex" => "#ef4444",   // red
            "moderate" => "#f59e0b",  // yellow
            "simple" => "#22c55e",    // green
            _ => "#6b7280",           // gray
        }
    }
}

// ── Ghost Highlights ──────────────────────────────────────────────────────────

/// Ghost highlight state for shadow diff rendering in the editor (§14).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhostHighlight {
    pub file_path: String,
    pub line_start: u32,
    pub line_end: u32,
    pub kind: GhostKind,
    pub agent_name: String,
    pub diff_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GhostKind {
    Addition,
    Removal,
}

impl GhostHighlight {
    /// Get the display color for this ghost highlight (from §14 config).
    pub fn color<'a>(&self, add_color: &'a str, remove_color: &'a str) -> &'a str {
        match self.kind {
            GhostKind::Addition => add_color,
            GhostKind::Removal => remove_color,
        }
    }

    /// Parse shadow diffs into ghost highlights for a specific file.
    pub fn from_shadow_diff(
        file_path: &str,
        diff_unified: &str,
        agent_name: &str,
    ) -> Vec<GhostHighlight> {
        let mut highlights = Vec::new();
        let mut current_line: u32 = 0;

        for line in diff_unified.lines() {
            if line.starts_with("@@") {
                // Parse hunk header: @@ -a,b +c,d @@
                if let Some(new_start) = parse_hunk_start(line) {
                    current_line = new_start;
                }
            } else if line.starts_with('+') && !line.starts_with("+++") {
                highlights.push(GhostHighlight {
                    file_path: file_path.to_string(),
                    line_start: current_line,
                    line_end: current_line,
                    kind: GhostKind::Addition,
                    agent_name: agent_name.to_string(),
                    diff_text: line[1..].to_string(),
                });
                current_line += 1;
            } else if line.starts_with('-') && !line.starts_with("---") {
                highlights.push(GhostHighlight {
                    file_path: file_path.to_string(),
                    line_start: current_line,
                    line_end: current_line,
                    kind: GhostKind::Removal,
                    agent_name: agent_name.to_string(),
                    diff_text: line[1..].to_string(),
                });
                // Don't increment for removals (they don't exist in new file)
            } else if !line.starts_with("---") && !line.starts_with("+++") {
                current_line += 1;
            }
        }

        highlights
    }
}

fn parse_hunk_start(hunk_header: &str) -> Option<u32> {
    // @@ -a,b +c,d @@ → extract c
    let plus_part = hunk_header.split('+').nth(1)?;
    let num = plus_part.split(',').next()?.trim();
    num.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_status_colors() {
        assert_eq!(AgentTeamPanel::status_color("working"), "#22c55e");
        assert_eq!(AgentTeamPanel::status_color("error"), "#ef4444");
    }

    #[test]
    fn test_agent_mode_colors() {
        assert_eq!(AgentTeamPanel::mode_color("shadow"), "#8b5cf6");
        assert_eq!(AgentTeamPanel::mode_color("live"), "#3b82f6");
    }

    #[test]
    fn test_complexity_colors() {
        assert_eq!(PulsePanel::complexity_border_color("complex"), "#ef4444");
        assert_eq!(PulsePanel::complexity_border_color("simple"), "#22c55e");
    }

    #[test]
    fn test_ghost_highlight_from_diff() {
        let diff = "@@ -10,3 +10,5 @@\n context\n+added line 1\n+added line 2\n context";
        let highlights = GhostHighlight::from_shadow_diff("test.ts", diff, "Architect");
        assert_eq!(highlights.len(), 2);
        assert!(matches!(highlights[0].kind, GhostKind::Addition));
        assert_eq!(highlights[0].diff_text, "added line 1");
        assert_eq!(highlights[0].agent_name, "Architect");
    }

    #[test]
    fn test_parse_hunk_start() {
        assert_eq!(parse_hunk_start("@@ -44,5 +44,8 @@"), Some(44));
        assert_eq!(parse_hunk_start("@@ -10,3 +15,5 @@"), Some(15));
    }
}
