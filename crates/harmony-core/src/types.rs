use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─── Actor ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActorKind {
    Human,
    Agent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ActorId(pub String);
// Canonical formats:
//   Human: "human:awanish"
//   Agent: "agent:architect-01"

// ─── Agent ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    Working,
    Negotiating,
    Paused,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AgentMode {
    Live,    // edits applied in real-time to workspace
    Shadow,  // edits stored privately, shown as ghost highlights
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRole {
    pub name: String,          // "Architect", "Coder", "Tester"
    pub avatar_key: String,    // key into assets/avatars/
    pub description: String,   // shown in sidebar tooltip
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: Uuid,
    pub actor_id: ActorId,          // "agent:{role}-{short_id}"
    pub machine_name: String,       // "Awanish" or "Rahul"
    pub machine_ip: String,         // "192.168.1.10" or "192.168.1.22"
    pub role: AgentRole,
    pub status: AgentStatus,
    pub mode: AgentMode,
    pub task_prompt: Option<String>,
    pub task_id: Option<Uuid>,
    pub memory_health: MemoryHealth,
    pub spawned_at: DateTime<Utc>,
    pub acp_endpoint: Option<String>, // e.g. "http://localhost:4231"
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryHealth {
    Good,     // last query < 500ms, results relevant
    Degraded, // last query 500ms-2s, or low relevance scores
    Failed,   // last query error or > 2s
}

// ─── Text Range ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextRange {
    pub start_line: u32,   // 0-indexed
    pub end_line: u32,     // 0-indexed, inclusive
    pub start_col: u32,    // 0-indexed
    pub end_col: u32,      // 0-indexed, inclusive
}

impl TextRange {
    /// Returns true if self and other share any line overlap
    pub fn overlaps(&self, other: &TextRange) -> bool {
        self.start_line <= other.end_line && other.start_line <= self.end_line
    }
}

// ─── Provenance Tag ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceTag {
    pub id: Uuid,
    pub actor_id: ActorId,
    pub machine_name: String,       // "Awanish" or "Rahul"
    pub machine_ip: String,         // "192.168.1.10" or "192.168.1.22"
    pub actor_kind: ActorKind,
    pub task_id: Option<Uuid>,
    pub task_prompt: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub file_path: String,          // relative to project root, forward slashes
    pub region: TextRange,
    pub mode: AgentMode,
    pub diff_unified: String,       // unified diff format of the change
    pub session_id: Uuid,           // identifies this Zed session
}

// ─── Overlap Event ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlapEvent {
    pub id: Uuid,
    pub file_path: String,
    pub region_a: TextRange,
    pub region_b: TextRange,
    pub change_a: ProvenanceTag,
    pub change_b: ProvenanceTag,
    pub detected_at: DateTime<Utc>,
    pub status: OverlapStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OverlapStatus {
    Pending,           // waiting for analysis
    AnalyzedFast,      // Tree-sitter + LSP done, no sandbox
    AnalyzedSandbox,   // sandbox test run done
    Negotiating,       // agents in negotiation round
    Resolved(ResolutionKind),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionKind {
    AcceptAll,
    AcceptA,           // keep change_a, discard change_b
    AcceptB,           // keep change_b, discard change_a
    Negotiated,        // merged diff from negotiation accepted
    Manual,            // human edited manually
}

// ─── Impact Analysis ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactGraph {
    pub overlap_id: Uuid,
    pub affected_symbols: Vec<AffectedSymbol>,
    pub summary: String,              // plain English, 1-3 sentences
    pub complexity: ImpactComplexity,
    pub sandbox_required: bool,
    pub sandbox_result: Option<SandboxResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AffectedSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub file_path: String,
    pub line: u32,
    pub impact: SymbolImpact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Interface,
    Variable,
    Import,
    Module,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolImpact {
    DirectlyModified,
    CallerOfModified,
    CalleeOfModified,
    SharedState,
    ImportDependency,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ImpactComplexity {
    Simple,   // no shared state, no callers affected
    Moderate, // ≤3 callers or 1 shared state dependency
    Complex,  // >3 callers, or shared state + callers, → escalate to sandbox
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxResult {
    pub tests_before_a: TestSummary,  // only change_a applied
    pub tests_before_b: TestSummary,  // only change_b applied
    pub tests_merged: Option<TestSummary>, // negotiated merge applied
    pub delta: String,                // plain English delta description
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestSummary {
    pub total: u32,
    pub passed: u32,
    pub failed: u32,
    pub failed_names: Vec<String>,
}

// ─── Memory Record ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: Uuid,
    pub content: String,
    pub embedding: Vec<f32>,         // stored as BLOB in SQLite
    pub namespace: MemoryNamespace,
    pub tags: Vec<String>,
    pub provenance: Option<Uuid>,    // links to ProvenanceTag.id
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryNamespace {
    Shared,
    Agent(Uuid),  // Uuid of the Agent
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileSyncEntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileSyncChangeKind {
    Created,
    Updated,
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSyncEvent {
    pub seq: i64,
    pub id: Uuid,
    pub relative_path: String,
    pub entry_kind: FileSyncEntryKind,
    pub change_kind: FileSyncChangeKind,
    pub content_base64: Option<String>,
    pub content_sha256: Option<String>,
    pub size_bytes: u64,
    pub actor_id: ActorId,
    pub machine_name: String,
    pub machine_ip: String,
    pub detected_at: DateTime<Utc>,
    pub impact_summary: String,
}

// ─── Shadow Diff ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowDiff {
    pub id: Uuid,
    pub agent_id: Uuid,
    pub file_path: String,
    pub diff_unified: String,    // unified diff against current HEAD of file
    pub base_hash: String,       // SHA256 of file content when diff was made
    pub created_at: DateTime<Utc>,
    pub status: ShadowDiffStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ShadowDiffStatus {
    Pending,    // not yet surfaced to user
    Surfaced,   // shown as ghost highlight
    Accepted,   // merged into workspace
    Rejected,   // discarded
    Superseded, // replaced by a newer diff from same agent on same file
}

// ─── Task ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Uuid,
    pub prompt: String,
    pub assigned_agents: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
    pub status: TaskStatus,
    pub session_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Queued,
    Active,
    Completed,
    Failed(String),
}

// ─── Negotiation ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NegotiationResult {
    pub overlap_id: Uuid,
    pub proposed_diff: String,      // unified diff representing merged result
    pub rationale: String,          // why this merge was chosen (1-2 sentences)
    pub confidence: f32,            // 0.0 - 1.0
    pub memory_notes: Vec<String>,  // things to write to shared memory
}

// ─── IPC Message (extension ↔ sidecar) ───────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum SidecarCommand {
    GetAgents,
    SpawnAgent { prompt: String, roles: Vec<String> },
    ToggleAgentMode { agent_id: Uuid },
    PauseAgent { agent_id: Uuid },
    RemoveAgent { agent_id: Uuid },
    GetOverlaps { status_filter: Option<OverlapStatus> },
    GetImpact { overlap_id: Uuid },
    ResolveOverlap { overlap_id: Uuid, resolution: ResolutionKind },
    StartNegotiation { overlap_id: Uuid },
    GetShadowDiffs { agent_id: Option<Uuid> },
    AcceptShadowDiff { diff_id: Uuid },
    RejectShadowDiff { diff_id: Uuid },
    GetMemoryHealth,
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum SidecarResponse {
    Agents(Vec<Agent>),
    Overlaps(Vec<OverlapEvent>),
    Impact(ImpactGraph),
    ShadowDiffs(Vec<ShadowDiff>),
    NegotiationStarted { overlap_id: Uuid },
    NegotiationResult(NegotiationResult),
    MemoryHealth { status: MemoryHealth, record_count: u64 },
    Ok,
    Pong,
    Error { message: String, code: u32 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_range_overlaps() {
        let range_a = TextRange { start_line: 10, end_line: 20, start_col: 0, end_col: 0 };
        let range_b = TextRange { start_line: 15, end_line: 25, start_col: 0, end_col: 0 };
        assert!(range_a.overlaps(&range_b));
        assert!(range_b.overlaps(&range_a));
    }

    #[test]
    fn test_text_range_no_overlap() {
        let range_a = TextRange { start_line: 10, end_line: 20, start_col: 0, end_col: 0 };
        let range_b = TextRange { start_line: 21, end_line: 30, start_col: 0, end_col: 0 };
        assert!(!range_a.overlaps(&range_b));
        assert!(!range_b.overlaps(&range_a));
    }

    #[test]
    fn test_text_range_adjacent_no_overlap() {
        // line 20 and line 20 DO overlap (inclusive)
        let range_a = TextRange { start_line: 10, end_line: 20, start_col: 0, end_col: 0 };
        let range_b = TextRange { start_line: 20, end_line: 30, start_col: 0, end_col: 0 };
        assert!(range_a.overlaps(&range_b));
    }

    #[test]
    fn test_text_range_contained() {
        let range_a = TextRange { start_line: 10, end_line: 30, start_col: 0, end_col: 0 };
        let range_b = TextRange { start_line: 15, end_line: 25, start_col: 0, end_col: 0 };
        assert!(range_a.overlaps(&range_b));
        assert!(range_b.overlaps(&range_a));
    }

    #[test]
    fn test_provenance_tag_serialization() {
        let tag = ProvenanceTag {
            id: Uuid::new_v4(),
            actor_id: ActorId("agent:architect-01".to_string()),
            machine_name: "Awanish".to_string(),
            machine_ip: "192.168.1.10".to_string(),
            actor_kind: ActorKind::Agent,
            task_id: Some(Uuid::new_v4()),
            task_prompt: Some("Implement rate limiting".to_string()),
            timestamp: Utc::now(),
            file_path: "src/middleware/auth.ts".to_string(),
            region: TextRange { start_line: 44, end_line: 67, start_col: 0, end_col: 0 },
            mode: AgentMode::Shadow,
            diff_unified: "@@ -44,5 +44,8 @@\n+const cache = {};".to_string(),
            session_id: Uuid::new_v4(),
        };
        let json = serde_json::to_string(&tag).unwrap();
        let deserialized: ProvenanceTag = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.file_path, tag.file_path);
        assert_eq!(deserialized.actor_id, tag.actor_id);
    }

    #[test]
    fn test_agent_serialization() {
        let agent = Agent {
            id: Uuid::new_v4(),
            actor_id: ActorId("agent:coder-01".to_string()),
            machine_name: "Rahul".to_string(),
            machine_ip: "192.168.1.22".to_string(),
            role: AgentRole {
                name: "Coder".to_string(),
                avatar_key: "agent-coder".to_string(),
                description: "Writes and edits code".to_string(),
            },
            status: AgentStatus::Working,
            mode: AgentMode::Shadow,
            task_prompt: Some("Build auth flow".to_string()),
            task_id: Some(Uuid::new_v4()),
            memory_health: MemoryHealth::Good,
            spawned_at: Utc::now(),
            acp_endpoint: Some("http://localhost:4231".to_string()),
        };
        let json = serde_json::to_string(&agent).unwrap();
        let deserialized: Agent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.role.name, "Coder");
    }

    #[test]
    fn test_sidecar_command_serialization() {
        let cmd = SidecarCommand::Ping;
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("ping"));

        let cmd = SidecarCommand::GetOverlaps { status_filter: None };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("get_overlaps"));
    }

    #[test]
    fn test_sidecar_response_serialization() {
        let resp = SidecarResponse::Pong;
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("pong"));

        let resp = SidecarResponse::Error {
            message: "not found".to_string(),
            code: 1001,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("1001"));
    }

    #[test]
    fn test_overlap_status_serialization() {
        let status = OverlapStatus::Resolved(ResolutionKind::Negotiated);
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: OverlapStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, status);
    }

    #[test]
    fn test_memory_namespace_serialization() {
        let ns = MemoryNamespace::Shared;
        let json = serde_json::to_string(&ns).unwrap();
        let deserialized: MemoryNamespace = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, ns);

        let agent_id = Uuid::new_v4();
        let ns = MemoryNamespace::Agent(agent_id);
        let json = serde_json::to_string(&ns).unwrap();
        let deserialized: MemoryNamespace = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, ns);
    }

    #[test]
    fn test_file_sync_event_serialization() {
        let event = FileSyncEvent {
            seq: 7,
            id: Uuid::new_v4(),
            relative_path: "src/new-file.ts".to_string(),
            entry_kind: FileSyncEntryKind::File,
            change_kind: FileSyncChangeKind::Created,
            content_base64: Some("aGVsbG8=".to_string()),
            content_sha256: Some("abc123".to_string()),
            size_bytes: 5,
            actor_id: ActorId("human:water".to_string()),
            machine_name: "water".to_string(),
            machine_ip: "152.20.22.4".to_string(),
            detected_at: Utc::now(),
            impact_summary: "Adds a new project file that will sync to connected laptops."
                .to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: FileSyncEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.seq, 7);
        assert_eq!(deserialized.entry_kind, FileSyncEntryKind::File);
        assert_eq!(deserialized.change_kind, FileSyncChangeKind::Created);
        assert_eq!(deserialized.relative_path, "src/new-file.ts");
    }
}
