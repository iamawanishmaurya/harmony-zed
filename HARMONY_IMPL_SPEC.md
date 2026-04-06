# HARMONY — Complete Implementation Specification
> **For AI coding agents (Codex, Cursor, Claude Code, etc.)**
> Read this entire document before writing a single line of code.
> Every architectural decision is made here. Do not invent what is not specified.

**Version:** 0.1.0-impl  
**Date:** April 2026  
**Author:** Awanish Maurya — XPWNIT LAB  
**Repo slug:** `harmony-zed`  
**License:** MIT

---

## TABLE OF CONTENTS

1. [What You Are Building](#1-what-you-are-building)
2. [Prerequisites & Environment](#2-prerequisites--environment)
3. [Workspace Layout — Every File, Every Directory](#3-workspace-layout--every-file-every-directory)
4. [Cargo.toml — Exact Dependencies & Versions](#4-cargotoml--exact-dependencies--versions)
5. [Zed Extension API — Reality Check](#5-zed-extension-api--reality-check)
6. [Data Models — Every Struct, Every Field, Every Type](#6-data-models--every-struct-every-field-every-type)
7. [SQLite Schema — Every Table, Column, Index](#7-sqlite-schema--every-table-column-index)
8. [Module Specs — Full Signatures & Logic](#8-module-specs--full-signatures--logic)
9. [MCP Memory Server — Full Implementation](#9-mcp-memory-server--full-implementation)
10. [ACP Agent Protocol — Message Formats](#10-acp-agent-protocol--message-formats)
11. [Algorithms — Step-by-Step](#11-algorithms--step-by-step)
12. [LLM Prompt Templates — Exact Text](#12-llm-prompt-templates--exact-text)
13. [UI Panel Specs — Layout, State, Events](#13-ui-panel-specs--layout-state-events)
14. [Config File Format](#14-config-file-format)
15. [Error Handling — Every Error Type](#15-error-handling--every-error-type)
16. [Build, Run & Test Commands](#16-build-run--test-commands)
17. [Implementation Order — Exact Sequence with Acceptance Criteria](#17-implementation-order--exact-sequence-with-acceptance-criteria)
18. [Known Constraints & Explicit Workarounds](#18-known-constraints--explicit-workarounds)
19. [What NOT to Build in v0.1](#19-what-not-to-build-in-v01)

---

## 1. WHAT YOU ARE BUILDING

**Harmony** is a Zed editor extension that makes parallel human + AI-agent development safe and collaborative.

### The core loop in one paragraph
When multiple agents and humans edit the same codebase simultaneously, Harmony (a) tracks every change with full provenance metadata, (b) detects when two participants' edits overlap in the same file region, (c) runs a fast semantic impact analysis (Tree-sitter AST diff + LSP dependency lookup), (d) surfaces a non-intrusive "Harmony Pulse" notification, (e) lets the user review a plain-English summary + resolution UI, and (f) optionally allows agents to auto-negotiate and propose a merged diff. Agents work in "shadow mode" by default — their proposed edits are stored privately and rendered as ghost highlights, never applied to the live file until the human accepts.

### What this is NOT
- Not a git replacement. Provenance is tracked in SQLite, not git objects.
- Not a cloud service. 100% local in v0.1.
- Not a chat interface. Task assignment is a single text prompt per agent; there is no back-and-forth dialogue UI.
- Not a swarm AI system. Agents work independently on assigned tasks; "negotiation" is a single-round LLM merge call, not a multi-turn conversation.

---

## 2. PREREQUISITES & ENVIRONMENT

### Required toolchain
```
rustup target add wasm32-wasip2       # Zed extensions compile to WASM
cargo install wasm-opt                # WASM optimization
cargo install zed-extension           # Zed extension CLI (if available) -- see §18
```

### Required Zed version
- Zed `0.150.x` or later (first version with stable Panel API + ACP registry support)
- Check: `zed --version`

### Required system tools
```
sqlite3 >= 3.39    # bundled via rusqlite, no system install needed
```

### Rust edition & MSRV
- Edition: `2021`
- MSRV: `1.78.0`

### Dev environment
- OS: Linux or macOS (Windows not tested in v0.1)
- RAM: minimum 4 GB (local embedding model needs ~500 MB)
- Disk: ~2 GB for model weights + build artifacts

---

## 3. WORKSPACE LAYOUT — EVERY FILE, EVERY DIRECTORY

```
harmony-zed/
│
├── Cargo.toml                        # workspace root — members listed here
├── extension.toml                    # Zed extension manifest
├── .gitignore
├── README.md
├── LICENSE
│
├── crates/
│   │
│   ├── harmony-core/                 # Pure Rust logic, no Zed/WASM deps
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── types.rs              # ALL shared data types (ProvenanceTag, Agent, etc.)
│   │       ├── provenance.rs         # Provenance tracking logic
│   │       ├── overlap.rs            # Overlap detection algorithm
│   │       ├── shadow.rs             # Shadow workspace state machine
│   │       ├── negotiation.rs        # Agent negotiation round-trip logic
│   │       └── errors.rs             # HarmonyError enum
│   │
│   ├── harmony-analyzer/             # Tree-sitter + LSP analysis (pure Rust, no WASM)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── treesitter.rs         # Tree-sitter AST diffing
│   │       ├── lsp_client.rs         # LSP JSON-RPC client (stdio)
│   │       └── impact.rs             # Build ImpactGraph from AST + LSP results
│   │
│   ├── harmony-memory/               # SQLite + embedding store (pure Rust, no WASM)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── store.rs              # SQLite read/write
│   │       ├── embeddings.rs         # Local embedding model wrapper
│   │       └── schema.rs             # SQL migration strings (const str, run on init)
│   │
│   ├── harmony-mcp/                  # Standalone MCP server binary
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs               # stdio MCP server entry point
│   │       ├── tools.rs              # Tool handler implementations
│   │       ├── transport.rs          # JSON-RPC over stdio
│   │       └── types.rs              # MCP-specific request/response types
│   │
│   └── harmony-extension/            # Zed extension crate (compiles to WASM)
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs                # Extension entry: register panels + commands
│           ├── agent_team.rs         # Agent Team sidebar — state + render
│           ├── pulse.rs              # Harmony Pulse panel — state + render
│           ├── ghost.rs              # Ghost highlight inline decorations
│           ├── hotkeys.rs            # Keybinding registration
│           ├── acp.rs                # ACP agent client (HTTP)
│           └── ipc.rs                # IPC bridge: extension → harmony-mcp process
│
├── assets/
│   ├── icon.svg                      # Extension icon (32×32, single color)
│   └── avatars/
│       ├── agent-architect.svg
│       ├── agent-coder.svg
│       ├── agent-tester.svg
│       └── agent-default.svg
│
└── tests/
    ├── overlap_tests.rs
    ├── shadow_tests.rs
    ├── memory_tests.rs
    └── analyzer_tests.rs
```

### Key architectural decision: Why split crates?
- `harmony-core`, `harmony-analyzer`, `harmony-memory`: compile to native. Run as a sidecar process spawned by the extension.
- `harmony-extension`: compiles to `wasm32-wasip2`. This is the actual Zed extension.
- `harmony-mcp`: compiles to native. Runs as a separate process, communicates via stdio JSON-RPC (MCP protocol).

**The extension itself does minimal logic.** It renders UI, handles user input, and communicates with the native sidecar (which does all heavy lifting: SQLite, embeddings, Tree-sitter, LSP). This is necessary because WASM in Zed has no filesystem access and no subprocess spawning. The sidecar is spawned by the extension using `zed::command` on startup.

---

## 4. CARGO.TOML — EXACT DEPENDENCIES & VERSIONS

### Workspace root: `Cargo.toml`
```toml
[workspace]
members = [
    "crates/harmony-core",
    "crates/harmony-analyzer",
    "crates/harmony-memory",
    "crates/harmony-mcp",
    "crates/harmony-extension",
]
resolver = "2"

[workspace.dependencies]
# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# Async runtime (native crates only — NOT in WASM extension)
tokio = { version = "1.37", features = ["full"] }

# Error handling
thiserror = "1.0"
anyhow = "1.0"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# UUID
uuid = { version = "1.8", features = ["v4", "serde"] }

# Time
chrono = { version = "0.4", features = ["serde"] }
```

### `crates/harmony-core/Cargo.toml`
```toml
[package]
name = "harmony-core"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }
tracing = { workspace = true }
```

### `crates/harmony-analyzer/Cargo.toml`
```toml
[package]
name = "harmony-analyzer"
version = "0.1.0"
edition = "2021"

[dependencies]
harmony-core = { path = "../harmony-core" }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }

# Tree-sitter
tree-sitter = "0.22"
tree-sitter-typescript = "0.21"
tree-sitter-javascript = "0.21"
tree-sitter-rust = "0.21"

# LSP client (JSON-RPC over stdio)
lsp-types = "0.95"
jsonrpc-core = "18.0"
```

### `crates/harmony-memory/Cargo.toml`
```toml
[package]
name = "harmony-memory"
version = "0.1.0"
edition = "2021"

[dependencies]
harmony-core = { path = "../harmony-core" }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }

# SQLite — bundled so no system sqlite3 required
rusqlite = { version = "0.31", features = ["bundled"] }

# Local embeddings — uses ONNX runtime, downloads model on first run
fastembed = "3.8"
# fastembed uses BAAI/bge-small-en-v1.5 by default (~130MB, cached in ~/.cache/fastembed)
```

### `crates/harmony-mcp/Cargo.toml`
```toml
[package]
name = "harmony-mcp"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "harmony-mcp"
path = "src/main.rs"

[dependencies]
harmony-core = { path = "../harmony-core" }
harmony-memory = { path = "../harmony-memory" }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
anyhow = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }

# MCP SDK (use rmcp — the official Rust MCP SDK)
rmcp = { version = "0.1", features = ["server", "transport-io"] }
```

### `crates/harmony-extension/Cargo.toml`
```toml
[package]
name = "harmony-extension"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
harmony-core = { path = "../harmony-core" }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }

# Zed extension SDK
zed_extension_api = "0.1"
# NOTE: zed_extension_api version must match your target Zed version.
# Check https://crates.io/crates/zed_extension_api for the latest.
# As of April 2026: 0.1.x series

[profile.release]
opt-level = "s"      # optimize for size (WASM)
lto = true
strip = true
```

---

## 5. ZED EXTENSION API — REALITY CHECK

**CRITICAL: Read this before touching `harmony-extension`.**

### What IS available in the Zed extension API (`zed_extension_api`)
As of Zed `0.150.x`:

| Feature | Status | How to use |
|---|---|---|
| Language servers | ✅ Stable | `zed_extension_api::LanguageServerBinary` |
| Slash commands | ✅ Stable | `zed_extension_api::SlashCommand` |
| Context servers (MCP) | ✅ Stable | `zed_extension_api::ContextServer` |
| Themes | ✅ Stable | theme files |
| Snippets | ✅ Stable | snippet files |
| **Panel API** | ⚠️ **Experimental** | `zed_extension_api::Panel` — may change |
| **Collaboration hooks** | ❌ **Not available** | Must use workaround (§18) |
| **File watchers** | ⚠️ **Limited** | Only workspace root watch |
| `process:exec` | ✅ via `zed_extension_api::Command` | Spawn native processes |

### The collaboration hooks problem and workaround
Zed's extension API does NOT expose collaboration events (other users' cursor positions, their edits, etc.) as of v0.150. 

**Workaround for v0.1:** Use a **file-system-based event bus**.
- The native sidecar watches the workspace using `notify` (Rust file watcher crate).
- Every participant (human or agent) writes a provenance-tagged change record to `.harmony/events/` directory in the project root when they save a file.
- The sidecar picks up these records and runs overlap detection.
- For the human participant, the extension hooks into Zed's `buffer_saved` event (available via slash command context) and writes the record.
- For agents, they write their own records via the MCP tool `report_change`.

This means overlap detection fires on **save**, not on every keystroke. This is acceptable for v0.1.

### How the extension spawns the sidecar
```rust
// In lib.rs extension init
fn init_sidecar(project_root: &Path) -> zed_extension_api::Result<()> {
    let sidecar_path = find_sidecar_binary()?; // bundled in extension assets
    zed_extension_api::Command::new(sidecar_path)
        .arg("--project-root")
        .arg(project_root.to_str().unwrap())
        .arg("--db-path")
        .arg(project_root.join(".harmony/memory.db").to_str().unwrap())
        .spawn()?;
    Ok(())
}
```

### How the extension communicates with the sidecar
IPC via a Unix socket at `.harmony/harmony.sock` in the project root.
- Extension sends JSON commands to the socket.
- Sidecar responds with JSON.
- Protocol is defined in §10.

**On Windows** (not supported in v0.1): fall back to TCP on `127.0.0.1:47823`. Add this TODO as a code comment.

---

## 6. DATA MODELS — EVERY STRUCT, EVERY FIELD, EVERY TYPE

All types live in `crates/harmony-core/src/types.rs`. Everything derives `Serialize, Deserialize, Debug, Clone`.

```rust
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
    Function, Method, Class, Interface, Variable, Import, Module,
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
```

---

## 7. SQLITE SCHEMA — EVERY TABLE, COLUMN, INDEX

File: `crates/harmony-memory/src/schema.rs`

All SQL migrations run in order on first open. Use a `schema_version` table to track which migrations have run.

```rust
pub const MIGRATIONS: &[(&str, &str)] = &[
    ("v001_init", V001_INIT),
    ("v002_shadow_diffs", V002_SHADOW_DIFFS),
    ("v003_memory_records", V003_MEMORY_RECORDS),
    ("v004_overlap_events", V004_OVERLAP_EVENTS),
    ("v005_tasks", V005_TASKS),
];

pub const SCHEMA_VERSION_TABLE: &str = "
CREATE TABLE IF NOT EXISTS schema_version (
    migration_id  TEXT PRIMARY KEY,
    applied_at    TEXT NOT NULL
);";

pub const V001_INIT: &str = "
CREATE TABLE IF NOT EXISTS agents (
    id            TEXT PRIMARY KEY,
    actor_id      TEXT NOT NULL UNIQUE,
    role_name     TEXT NOT NULL,
    role_avatar   TEXT NOT NULL,
    role_desc     TEXT NOT NULL DEFAULT '',
    status        TEXT NOT NULL DEFAULT 'idle',
    mode          TEXT NOT NULL DEFAULT 'shadow',
    task_prompt   TEXT,
    task_id       TEXT,
    memory_health TEXT NOT NULL DEFAULT 'good',
    spawned_at    TEXT NOT NULL,
    acp_endpoint  TEXT,
    session_id    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS provenance_tags (
    id            TEXT PRIMARY KEY,
    actor_id      TEXT NOT NULL,
    actor_kind    TEXT NOT NULL,
    task_id       TEXT,
    task_prompt   TEXT,
    timestamp     TEXT NOT NULL,
    file_path     TEXT NOT NULL,
    region_start_line  INTEGER NOT NULL,
    region_end_line    INTEGER NOT NULL,
    region_start_col   INTEGER NOT NULL DEFAULT 0,
    region_end_col     INTEGER NOT NULL DEFAULT 0,
    mode          TEXT NOT NULL DEFAULT 'shadow',
    diff_unified  TEXT NOT NULL DEFAULT '',
    session_id    TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_prov_file ON provenance_tags(file_path);
CREATE INDEX IF NOT EXISTS idx_prov_actor ON provenance_tags(actor_id);
CREATE INDEX IF NOT EXISTS idx_prov_ts ON provenance_tags(timestamp);
";

pub const V002_SHADOW_DIFFS: &str = "
CREATE TABLE IF NOT EXISTS shadow_diffs (
    id            TEXT PRIMARY KEY,
    agent_id      TEXT NOT NULL REFERENCES agents(id),
    file_path     TEXT NOT NULL,
    diff_unified  TEXT NOT NULL,
    base_hash     TEXT NOT NULL,
    created_at    TEXT NOT NULL,
    status        TEXT NOT NULL DEFAULT 'pending'
);

CREATE INDEX IF NOT EXISTS idx_shadow_agent ON shadow_diffs(agent_id);
CREATE INDEX IF NOT EXISTS idx_shadow_file ON shadow_diffs(file_path);
CREATE INDEX IF NOT EXISTS idx_shadow_status ON shadow_diffs(status);
";

pub const V003_MEMORY_RECORDS: &str = "
CREATE TABLE IF NOT EXISTS memory_records (
    id            TEXT PRIMARY KEY,
    content       TEXT NOT NULL,
    embedding     BLOB NOT NULL,     -- f32 array, little-endian, raw bytes
    namespace     TEXT NOT NULL,     -- 'shared' or 'agent:{uuid}'
    tags          TEXT NOT NULL,     -- JSON array of strings
    provenance_id TEXT,              -- optional FK to provenance_tags.id
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_mem_namespace ON memory_records(namespace);
-- Note: no vector index in SQLite; cosine similarity is computed in Rust
-- For v0.1 this is fine up to ~50k records. v0.2 can add usearch or hnswlib.
";

pub const V004_OVERLAP_EVENTS: &str = "
CREATE TABLE IF NOT EXISTS overlap_events (
    id               TEXT PRIMARY KEY,
    file_path        TEXT NOT NULL,
    region_a_start   INTEGER NOT NULL,
    region_a_end     INTEGER NOT NULL,
    region_b_start   INTEGER NOT NULL,
    region_b_end     INTEGER NOT NULL,
    change_a_id      TEXT NOT NULL REFERENCES provenance_tags(id),
    change_b_id      TEXT NOT NULL REFERENCES provenance_tags(id),
    detected_at      TEXT NOT NULL,
    status           TEXT NOT NULL DEFAULT 'pending',
    impact_summary   TEXT,
    impact_complexity TEXT,
    resolution_kind  TEXT,
    resolved_at      TEXT,
    session_id       TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_overlap_status ON overlap_events(status);
CREATE INDEX IF NOT EXISTS idx_overlap_file ON overlap_events(file_path);
";

pub const V005_TASKS: &str = "
CREATE TABLE IF NOT EXISTS tasks (
    id              TEXT PRIMARY KEY,
    prompt          TEXT NOT NULL,
    agent_ids       TEXT NOT NULL,   -- JSON array of agent UUIDs
    created_at      TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'queued',
    error_message   TEXT,
    session_id      TEXT NOT NULL
);
";
```

### SQLite connection settings
```rust
// In store.rs — apply these PRAGMA on every connection open
const PRAGMAS: &str = "
PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;
PRAGMA foreign_keys=ON;
PRAGMA temp_store=memory;
PRAGMA cache_size=-64000;  -- 64MB page cache
";
```

---

## 8. MODULE SPECS — FULL SIGNATURES & LOGIC

### `crates/harmony-core/src/overlap.rs`

```rust
use crate::types::*;

/// Main entry point for overlap detection.
/// Call this after every ProvenanceTag is written to the DB.
/// Returns all new OverlapEvents found (may be empty).
pub fn detect_overlaps(
    new_tag: &ProvenanceTag,
    recent_tags: &[ProvenanceTag],   // tags from same file in last N minutes
    window_minutes: u32,             // default: 30
) -> Vec<OverlapEvent>;

/// Returns true if two provenance tags constitute a real overlap.
/// Rules:
///   1. Must be same file_path.
///   2. Must be different actor_ids.
///   3. Regions must overlap (TextRange::overlaps).
///   4. Timestamps must be within window_minutes of each other.
///   5. Must not already have a resolved OverlapEvent for this pair.
fn is_real_overlap(a: &ProvenanceTag, b: &ProvenanceTag, window_minutes: u32) -> bool;
```

**Algorithm detail for `detect_overlaps`:**
```
1. Filter recent_tags: same file_path, within window_minutes of new_tag.timestamp
2. For each candidate in filtered:
     a. If candidate.actor_id == new_tag.actor_id → skip (same person)
     b. If !new_tag.region.overlaps(&candidate.region) → skip
     c. Build OverlapEvent {
          id: Uuid::new_v4(),
          file_path: new_tag.file_path.clone(),
          region_a: new_tag.region.clone(),
          region_b: candidate.region.clone(),
          change_a: new_tag.clone(),
          change_b: candidate.clone(),
          detected_at: Utc::now(),
          status: OverlapStatus::Pending,
        }
     d. Push to result vec
3. Return result vec
```

### `crates/harmony-core/src/shadow.rs`

```rust
use crate::types::*;

/// Apply a shadow diff to the file contents in-memory (does NOT write to disk).
/// Returns the new file content string if patch applies cleanly, or error.
pub fn apply_shadow_diff(
    original_content: &str,
    diff_unified: &str,
) -> Result<String, ShadowError>;

/// Compute a unified diff between original and modified content.
/// Uses the `similar` crate (add to analyzer deps: similar = "2.5").
pub fn compute_unified_diff(
    original: &str,
    modified: &str,
    file_path: &str,
) -> String;

/// Compute SHA256 of file content (for base_hash in ShadowDiff).
pub fn content_hash(content: &str) -> String;

/// Returns true if a ShadowDiff can still be applied to current file content
/// (i.e., base_hash still matches current file hash — no intervening save).
pub fn is_diff_applicable(diff: &ShadowDiff, current_content: &str) -> bool;

#[derive(thiserror::Error, Debug)]
pub enum ShadowError {
    #[error("patch does not apply cleanly: {0}")]
    PatchFailed(String),
    #[error("base hash mismatch: diff is stale")]
    StaleBase,
}
```

**Crate to add for shadow.rs:** `similar = "2.5"` in `harmony-core/Cargo.toml`.

### `crates/harmony-analyzer/src/treesitter.rs`

```rust
use harmony_core::types::*;
use tree_sitter::{Parser, Language, Node};

pub struct TreeSitterAnalyzer {
    parsers: HashMap<SupportedLanguage, Parser>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SupportedLanguage {
    TypeScript,
    JavaScript,
    Rust,
}

impl TreeSitterAnalyzer {
    pub fn new() -> Self;

    /// Detect language from file extension.
    /// .ts, .tsx → TypeScript
    /// .js, .jsx, .mjs, .cjs → JavaScript
    /// .rs → Rust
    /// Everything else → None (caller should skip Tree-sitter analysis)
    pub fn detect_language(file_path: &str) -> Option<SupportedLanguage>;

    /// Parse file content and extract all top-level symbols in the given TextRange.
    /// Returns names + kinds of AST nodes that overlap with the region.
    pub fn extract_symbols_in_range(
        &mut self,
        content: &str,
        language: SupportedLanguage,
        region: &TextRange,
    ) -> Vec<AffectedSymbol>;

    /// Diff two versions of a file (before/after a change).
    /// Returns symbols that changed.
    pub fn diff_symbols(
        &mut self,
        before: &str,
        after: &str,
        language: SupportedLanguage,
    ) -> Vec<AffectedSymbol>;
}
```

**Tree-sitter queries to use (TypeScript example):**
```
; Query to find function/method/class definitions in a range
(function_declaration name: (identifier) @name) @func
(method_definition name: (property_identifier) @name) @method
(class_declaration name: (type_identifier) @name) @class
(variable_declarator name: (identifier) @name) @var
(import_statement source: (string) @import_path) @import
```

### `crates/harmony-analyzer/src/lsp_client.rs`

```rust
use lsp_types::*;
use std::process::{Child, ChildStdin, ChildStdout};

pub struct LspClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    request_id: AtomicI32,
    project_root: PathBuf,
}

impl LspClient {
    /// Spawn the appropriate LSP server for the language.
    /// TypeScript/JavaScript: `typescript-language-server --stdio`
    /// Rust: `rust-analyzer`
    /// These must be installed on the host machine (not bundled).
    pub fn spawn(language: SupportedLanguage, project_root: &Path) -> Result<Self>;

    /// Send textDocument/definition for a symbol at position.
    pub fn find_definition(&mut self, file: &str, line: u32, col: u32)
        -> Result<Option<Location>>;

    /// Send textDocument/references for a symbol at position.
    pub fn find_references(&mut self, file: &str, line: u32, col: u32)
        -> Result<Vec<Location>>;

    /// Send shutdown + exit. Always call this on drop.
    pub fn shutdown(&mut self) -> Result<()>;
}

impl Drop for LspClient {
    fn drop(&mut self) { let _ = self.shutdown(); }
}
```

**Note:** If the LSP server binary is not installed (e.g., `typescript-language-server` missing), `LspClient::spawn` returns `Err`. The `ImpactGraph` builder must handle this gracefully — fall back to Tree-sitter-only analysis and set `complexity = ImpactComplexity::Moderate` (conservative estimate).

### `crates/harmony-analyzer/src/impact.rs`

```rust
use harmony_core::types::*;

pub struct ImpactAnalyzer {
    tree_sitter: TreeSitterAnalyzer,
    lsp: Option<LspClient>,  // None if LSP unavailable
}

impl ImpactAnalyzer {
    pub fn new(project_root: &Path, language: SupportedLanguage) -> Self;

    /// Main entry point. Analyze an OverlapEvent and produce an ImpactGraph.
    /// This is synchronous. The caller runs it in a tokio::task::spawn_blocking.
    pub fn analyze(&mut self, overlap: &OverlapEvent, 
                   content_a: &str, content_b: &str) -> ImpactGraph;
}
```

**`analyze` algorithm:**
```
1. Parse content_a and content_b with Tree-sitter.
2. Extract symbols changed in change_a's region → symbols_a
3. Extract symbols changed in change_b's region → symbols_b
4. Find intersection: symbols_shared = symbols_a ∩ symbols_b (by name)
5. If LSP available:
     For each symbol in symbols_a ∪ symbols_b:
       references = lsp.find_references(symbol)
       callers = references where file in project
       Add callers to affected_symbols list with impact=CallerOfModified
6. Determine complexity:
     Simple   → no shared symbols AND len(callers) == 0
     Moderate → shared symbols OR len(callers) <= 3
     Complex  → len(callers) > 3 OR (shared symbols AND callers > 0)
7. Set sandbox_required = complexity == Complex
8. Generate summary string (see §12 for template)
9. Return ImpactGraph
```

### `crates/harmony-memory/src/embeddings.rs`

```rust
use fastembed::{TextEmbedding, InitOptions, EmbeddingModel};

pub struct EmbeddingEngine {
    model: TextEmbedding,
}

impl EmbeddingEngine {
    /// Initialize. Downloads model on first run to ~/.cache/fastembed.
    /// Model: EmbeddingModel::BGESmallENV15 (130MB, good quality/speed tradeoff).
    /// This is slow (~3-10 seconds). Call once at sidecar startup.
    pub fn new() -> anyhow::Result<Self> {
        let model = TextEmbedding::try_new(InitOptions {
            model_name: EmbeddingModel::BGESmallENV15,
            show_download_progress: true,
            ..Default::default()
        })?;
        Ok(Self { model })
    }

    /// Embed a single text string. Returns 384-dimensional f32 vector.
    pub fn embed_one(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let mut results = self.model.embed(vec![text.to_string()], None)?;
        Ok(results.remove(0))
    }

    /// Embed multiple texts in a batch. More efficient than calling embed_one repeatedly.
    pub fn embed_batch(&self, texts: Vec<String>) -> anyhow::Result<Vec<Vec<f32>>>;

    /// Cosine similarity between two embedding vectors.
    /// Returns value in [-1.0, 1.0]. Values > 0.7 considered "relevant".
    pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let mag_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let mag_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        if mag_a == 0.0 || mag_b == 0.0 { return 0.0; }
        dot / (mag_a * mag_b)
    }
}
```

### `crates/harmony-memory/src/store.rs`

```rust
use rusqlite::{Connection, params};
use harmony_core::types::*;

pub struct MemoryStore {
    conn: Connection,
    embeddings: EmbeddingEngine,
    project_db_path: PathBuf,
}

impl MemoryStore {
    /// Open or create the SQLite DB at path. Runs all pending migrations.
    pub fn open(db_path: &Path) -> anyhow::Result<Self>;

    // ── Provenance ────────────────────────────────────────────────────────────

    pub fn insert_provenance_tag(&self, tag: &ProvenanceTag) -> anyhow::Result<()>;

    /// Fetch all provenance tags for a file, newer than `since`.
    pub fn get_recent_tags_for_file(
        &self,
        file_path: &str,
        since_minutes: u32,
    ) -> anyhow::Result<Vec<ProvenanceTag>>;

    // ── Agents ────────────────────────────────────────────────────────────────

    pub fn upsert_agent(&self, agent: &Agent) -> anyhow::Result<()>;
    pub fn get_agents(&self) -> anyhow::Result<Vec<Agent>>;
    pub fn get_agent(&self, id: Uuid) -> anyhow::Result<Option<Agent>>;
    pub fn delete_agent(&self, id: Uuid) -> anyhow::Result<()>;

    // ── Shadow Diffs ──────────────────────────────────────────────────────────

    pub fn insert_shadow_diff(&self, diff: &ShadowDiff) -> anyhow::Result<()>;
    pub fn update_shadow_diff_status(&self, id: Uuid, status: ShadowDiffStatus) -> anyhow::Result<()>;
    pub fn get_shadow_diffs_for_agent(&self, agent_id: Uuid) -> anyhow::Result<Vec<ShadowDiff>>;
    pub fn get_pending_shadow_diffs(&self) -> anyhow::Result<Vec<ShadowDiff>>;

    // ── Overlap Events ────────────────────────────────────────────────────────

    pub fn insert_overlap_event(&self, event: &OverlapEvent) -> anyhow::Result<()>;
    pub fn update_overlap_status(&self, id: Uuid, status: OverlapStatus) -> anyhow::Result<()>;
    pub fn get_pending_overlaps(&self) -> anyhow::Result<Vec<OverlapEvent>>;
    pub fn get_overlap(&self, id: Uuid) -> anyhow::Result<Option<OverlapEvent>>;

    // ── Memory Records ────────────────────────────────────────────────────────

    /// Add a new memory record. Embedding is computed here automatically.
    pub fn add_memory(
        &mut self,
        content: &str,
        tags: Vec<String>,
        namespace: MemoryNamespace,
        provenance_id: Option<Uuid>,
    ) -> anyhow::Result<Uuid>;

    /// Semantic search: embed query, compute cosine similarity against all records
    /// in namespace, return top `limit` results with similarity >= 0.5.
    pub fn query_memory(
        &mut self,
        query: &str,
        namespace: MemoryNamespace,
        limit: usize,
    ) -> anyhow::Result<Vec<(MemoryRecord, f32)>>;  // (record, similarity_score)

    // ── Helper ────────────────────────────────────────────────────────────────

    /// Serialize f32 vec to bytes for SQLite BLOB storage (little-endian IEEE 754).
    fn vec_to_bytes(v: &[f32]) -> Vec<u8>;
    /// Deserialize bytes from SQLite BLOB to f32 vec.
    fn bytes_to_vec(b: &[u8]) -> Vec<f32>;
}
```

---

## 9. MCP MEMORY SERVER — FULL IMPLEMENTATION

### Protocol
MCP over **stdio JSON-RPC 2.0**. The extension spawns `harmony-mcp` as a child process and communicates via stdin/stdout.

### Tool definitions (exactly as they must appear in the MCP server registration)

```rust
// In crates/harmony-mcp/src/tools.rs

// Tool 1: query_memory
// Input schema:
{
  "query": {
    "type": "string",
    "description": "Semantic search query. E.g. 'why did we reject Redis caching'"
  },
  "namespace": {
    "type": "string",
    "enum": ["shared", "agent:{uuid}"],
    "description": "Memory namespace. Use 'shared' for team memory.",
    "default": "shared"
  },
  "limit": {
    "type": "integer",
    "description": "Max results to return",
    "default": 5,
    "minimum": 1,
    "maximum": 20
  }
}
// Output: JSON array of { content: string, tags: string[], similarity: f32, created_at: string }

// Tool 2: add_memory
// Input schema:
{
  "content": {
    "type": "string",
    "description": "Memory content to store. Be specific and self-contained."
  },
  "tags": {
    "type": "array",
    "items": { "type": "string" },
    "description": "Tags for filtering. E.g. ['decision', 'rejected', 'auth', 'redis']"
  },
  "namespace": {
    "type": "string",
    "enum": ["shared", "agent:{uuid}"],
    "default": "shared"
  }
}
// Output: { "id": "{uuid}", "message": "Memory stored successfully" }

// Tool 3: report_change
// Agents call this to report their shadow changes (triggers overlap detection)
// Input schema:
{
  "actor_id": { "type": "string" },
  "file_path": { "type": "string" },
  "diff_unified": { "type": "string" },
  "start_line": { "type": "integer" },
  "end_line": { "type": "integer" },
  "task_id": { "type": "string", "description": "UUID of the task this change belongs to" },
  "task_prompt": { "type": "string" }
}
// Output: { "tag_id": "{uuid}", "overlaps_detected": ["{uuid}", ...] }

// Tool 4: list_decisions
// Input schema:
{
  "file_pattern": {
    "type": "string",
    "description": "Glob pattern to filter by file. E.g. 'src/auth/**'",
    "nullable": true
  },
  "since_days": {
    "type": "integer",
    "description": "Only decisions from the last N days",
    "default": 30
  },
  "limit": { "type": "integer", "default": 10 }
}
// Output: JSON array of { content: string, tags: string[], created_at: string }
// Filter: only memory_records with tag "decision" in them
```

### MCP server startup
```rust
// crates/harmony-mcp/src/main.rs
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse CLI args
    let args: Vec<String> = std::env::args().collect();
    let db_path = parse_arg(&args, "--db-path")
        .unwrap_or_else(|| ".harmony/memory.db".to_string());
    
    // Initialize tracing to stderr (stdout is reserved for MCP protocol)
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter("harmony_mcp=debug")
        .init();
    
    // Open memory store
    let store = MemoryStore::open(Path::new(&db_path))?;
    let store = Arc::new(Mutex::new(store));
    
    // Build MCP server with rmcp
    let server = HarmonyMcpServer { store };
    rmcp::serve_stdio(server).await?;
    Ok(())
}
```

---

## 10. ACP AGENT PROTOCOL — MESSAGE FORMATS

### What ACP is
ACP (Agent Communication Protocol) is Zed's standard for communicating with external AI agents. In v0.1, Harmony uses a simplified version: HTTP POST to the agent's endpoint.

### Agent task message (Harmony → Agent)
```json
POST {acp_endpoint}/task
Content-Type: application/json

{
  "task_id": "550e8400-e29b-41d4-a716-446655440000",
  "prompt": "Implement rate limiting middleware for the auth router. Max 100 req/min per IP.",
  "context": {
    "project_root": "/home/user/myproject",
    "relevant_files": ["src/middleware/auth.ts", "src/routes/auth.ts"],
    "memory_snapshot": [
      {
        "content": "We rejected Redis for caching in auth due to infra cost constraints - April 2026",
        "tags": ["decision", "rejected", "redis", "auth"],
        "similarity": 0.89
      }
    ],
    "harmony_mcp_endpoint": "stdio",
    "harmony_mcp_binary": "/path/to/harmony-mcp",
    "harmony_mcp_args": ["--db-path", "/home/user/myproject/.harmony/memory.db"]
  },
  "constraints": {
    "mode": "shadow",
    "report_changes_via_mcp": true,
    "mcp_tool": "report_change"
  }
}
```

### Agent response (Agent → Harmony)
```json
HTTP 200
{
  "task_id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "accepted",
  "agent_actor_id": "agent:coder-01"
}
```

Agents report their changes async via the `report_change` MCP tool. Harmony does NOT poll for changes — the MCP tool call triggers overlap detection synchronously.

### ACP agent registry (known agents in v0.1)
```rust
// crates/harmony-extension/src/acp.rs

pub struct KnownAgent {
    pub name: &'static str,
    pub default_endpoint: &'static str,
    pub role_hint: AgentRole,
}

pub const KNOWN_AGENTS: &[KnownAgent] = &[
    KnownAgent {
        name: "opencode",
        default_endpoint: "http://localhost:4231",
        role_hint: AgentRole { name: "Coder", avatar_key: "agent-coder", description: "Writes and edits code" },
    },
    KnownAgent {
        name: "gemini-cli",
        default_endpoint: "http://localhost:4232",
        role_hint: AgentRole { name: "Architect", avatar_key: "agent-architect", description: "Plans and architects solutions" },
    },
];
```

### If no ACP agent is running (for testing)
Use a **stub agent** that:
1. Receives the task
2. Waits 5 seconds
3. Calls `report_change` MCP tool with a dummy diff
4. Returns `{ status: "completed" }`

Implement the stub in `crates/harmony-mcp/src/stub_agent.rs`. Run it with `cargo run --bin harmony-mcp -- --stub-agent`.

---

## 11. ALGORITHMS — STEP-BY-STEP

### Algorithm A: Full overlap handling pipeline

```
TRIGGER: `report_change` MCP tool is called by any agent

INPUT: actor_id, file_path, diff_unified, start_line, end_line, task_id, task_prompt

STEP 1 — Write provenance tag
  tag = ProvenanceTag {
    id: Uuid::new_v4(),
    actor_id: input.actor_id,
    actor_kind: ActorKind::Agent,
    task_id: input.task_id,
    task_prompt: input.task_prompt,
    timestamp: Utc::now(),
    file_path: input.file_path (normalize: strip project root, forward slashes),
    region: TextRange { start_line, end_line, start_col: 0, end_col: 0 },
    mode: AgentMode::Shadow,
    diff_unified: input.diff_unified,
    session_id: CURRENT_SESSION_ID,
  }
  store.insert_provenance_tag(&tag)

STEP 2 — Detect overlaps
  recent = store.get_recent_tags_for_file(&tag.file_path, window_minutes=30)
  overlaps = detect_overlaps(&tag, &recent, 30)

STEP 3 — For each new overlap:
  store.insert_overlap_event(&overlap)

STEP 4 — Notify extension via IPC socket
  Send SidecarResponse::Overlaps([overlap]) to all connected IPC clients
  Extension receives this and shows Pulse notification badge

STEP 5 — Start background impact analysis (tokio::spawn)
  For each overlap:
    analyzer.analyze(&overlap, content_before, content_after)
    store.update_overlap with impact_summary and impact_complexity
    If sandbox_required:
      sandbox::run_tests(&overlap, project_root)
    Notify extension again with SidecarResponse::Impact(graph)

STEP 6 — Return to MCP caller
  { tag_id: tag.id, overlaps_detected: [overlap.id, ...] }
```

### Algorithm B: Agent auto-negotiation

```
TRIGGER: User clicks "Let Agents Negotiate" in Pulse panel

INPUT: overlap_id

STEP 1 — Load overlap + both provenance tags
  overlap = store.get_overlap(overlap_id)
  tag_a = overlap.change_a
  tag_b = overlap.change_b

STEP 2 — Load memory context
  memory = store.query_memory(
    query = "decisions about " + overlap.file_path,
    namespace = MemoryNamespace::Shared,
    limit = 5
  )

STEP 3 — Build negotiation prompt (see §12, Prompt Template B)

STEP 4 — Call LLM
  Use the agent_a's ACP endpoint if it supports direct completion.
  FALLBACK: If no direct LLM access, use agent_a's task endpoint with a special
  negotiation task prompt. Agent responds with JSON.
  FALLBACK 2: If no agent available, prompt the user to configure an LLM API key
  in harmony config. Use OpenAI-compatible API directly.

STEP 5 — Parse NegotiationResult from LLM response
  Validate: proposed_diff must be valid unified diff format
  Validate: proposed_diff applies cleanly to current file content

STEP 6 — Store result
  store.update_overlap_status(overlap_id, OverlapStatus::Negotiating)
  (after LLM responds)
  store.update_overlap_status(overlap_id, OverlapStatus::Resolved if auto-accepted, else Negotiating)

STEP 7 — Notify extension
  Send NegotiationResult via IPC
  Extension displays proposed merged diff in Pulse panel for human review
  Human accepts → call ResolveOverlap(Negotiated)

STEP 8 — On accept: apply diff to workspace file
  content = fs::read_to_string(file_path)
  new_content = apply_shadow_diff(content, result.proposed_diff)
  fs::write(file_path, new_content)
  Write memory: store.add_memory(content=rationale, tags=["decision", "merged"], ...)
```

### Algorithm C: Shadow diff → Ghost highlight mapping

```
TRIGGER: New ShadowDiff arrives at extension via IPC

STEP 1 — Read current file buffer content from Zed
  (via zed_extension_api's workspace buffer access)

STEP 2 — Check if diff is still applicable
  is_diff_applicable(&diff, &current_content)
  If false → mark diff as Superseded, skip

STEP 3 — Parse unified diff to extract line ranges
  For each hunk in diff:
    start_line = hunk.old_start  (0-indexed)
    end_line = hunk.old_start + hunk.old_lines - 1
    added_lines = hunk.new_lines that start with '+'

STEP 4 — Register inline decoration with Zed's buffer API
  For each added line ('+' prefix): show ghost text in muted green
  For each removed line ('-' prefix): show strikethrough on original line
  Decoration metadata includes diff_id for click handling

STEP 5 — User clicks a ghost highlight → opens Pulse panel filtered to that diff
```

### Algorithm D: Sandbox test execution

```
TRIGGER: ImpactComplexity::Complex detected during impact analysis

STEP 1 — Detect test command
  Check project root for (in order):
    package.json with "test" script → "npm test" or "pnpm test" or "yarn test"
    Cargo.toml → "cargo test"
    pyproject.toml → "pytest"
    If none found → skip sandbox, set sandbox_required=false with warning

STEP 2 — Create temp workspace copies (3 copies: apply A, apply B, apply merge if available)
  tmpdir_a = mktemp -d
  cp -r project_root tmpdir_a
  apply change_a diff to tmpdir_a/file_path
  Same for tmpdir_b with change_b

STEP 3 — Run tests in each (with 60 second timeout)
  Command::new(test_cmd).current_dir(tmpdir_a).timeout(60s)
  Parse output for pass/fail counts (regex: "X passed, Y failed" common patterns)

STEP 4 — Build SandboxResult
  delta = generate_delta_string(result_a, result_b)
  (e.g. "Change A: all 47 tests pass. Change B: 2 tests fail: auth.test.ts:12, auth.test.ts:89")

STEP 5 — Cleanup temp dirs

STEP 6 — Update ImpactGraph with SandboxResult
  Notify extension with updated graph
```

---

## 12. LLM PROMPT TEMPLATES — EXACT TEXT

### Template A: Impact summary generation (non-LLM, code-generated)

This summary is generated by code, not an LLM. It's a deterministic string built from ImpactGraph data.

```rust
pub fn build_impact_summary(
    change_a: &ProvenanceTag,
    change_b: &ProvenanceTag,
    symbols_a: &[AffectedSymbol],
    symbols_b: &[AffectedSymbol],
    shared: &[AffectedSymbol],
) -> String {
    // Template:
    // "{actor_a} {verb_a} {symbols_a_str} in {file}. {actor_b} {verb_b} {symbols_b_str} in the same region."
    // If shared: " Both changes affect: {shared_str}."
    
    let actor_a = format_actor(&change_a.actor_id);
    let actor_b = format_actor(&change_b.actor_id);
    let verb_a = if symbols_a.is_empty() { "edited code" } else { "modified" };
    let verb_b = if symbols_b.is_empty() { "edited code" } else { "modified" };
    let sym_a = format_symbols(symbols_a);
    let sym_b = format_symbols(symbols_b);
    
    let mut s = format!("{actor_a} {verb_a} {sym_a} in `{}`. \
                         {actor_b} {verb_b} {sym_b} in the same region.", 
                         change_a.file_path);
    
    if !shared.is_empty() {
        let shared_str = format_symbols(shared);
        s.push_str(&format!(" Both changes affect: {shared_str}."));
    }
    s
}

fn format_actor(actor_id: &ActorId) -> String {
    // "agent:architect-01" → "Agent Architect"
    // "human:awanish" → "You (awanish)"
    ...
}

fn format_symbols(symbols: &[AffectedSymbol]) -> String {
    // max 3 symbols listed, rest summarized as "+ N more"
    ...
}
```

### Template B: Agent negotiation LLM prompt

```rust
pub fn build_negotiation_prompt(
    overlap: &OverlapEvent,
    impact: &ImpactGraph,
    memory: &[(MemoryRecord, f32)],
) -> String {
    let memory_ctx = memory.iter()
        .map(|(r, score)| format!("- [relevance:{:.2}] {}", score, r.content))
        .collect::<Vec<_>>().join("\n");

    format!(r#"You are a code merge mediator for a software project.
Two changes were made to the same region of `{file}` simultaneously.
Your job is to produce a single merged change that preserves the intent of both.

## Change A
Author: {author_a}
Task: {task_a}
Diff:
```diff
{diff_a}
```

## Change B
Author: {author_b}
Task: {task_b}
Diff:
```diff
{diff_b}
```

## Impact Analysis
{impact_summary}
Affected symbols: {affected_symbols}

## Relevant Team Memory
{memory_ctx}

## Your Task
Produce a merged unified diff that:
1. Preserves the intent of BOTH changes
2. Does not break existing functionality
3. Follows the existing code style in the file
4. Is as minimal as possible

Respond ONLY with valid JSON in this exact format, no other text:
{{
  "proposed_diff": "--- a/{file}\n+++ b/{file}\n@@ ... @@\n...",
  "rationale": "One or two sentences explaining the merge decision.",
  "confidence": 0.85,
  "memory_notes": ["Short note to add to team memory about this decision"]
}}
"#,
        file = overlap.file_path,
        author_a = overlap.change_a.actor_id.0,
        task_a = overlap.change_a.task_prompt.as_deref().unwrap_or("(no task)"),
        diff_a = overlap.change_a.diff_unified,
        author_b = overlap.change_b.actor_id.0,
        task_b = overlap.change_b.task_prompt.as_deref().unwrap_or("(no task)"),
        diff_b = overlap.change_b.diff_unified,
        impact_summary = impact.summary,
        affected_symbols = impact.affected_symbols.iter()
            .map(|s| format!("{} ({})", s.name, format_impact(&s.impact)))
            .collect::<Vec<_>>().join(", "),
        memory_ctx = memory_ctx,
    )
}
```

### Template C: Agent spawn prompt decomposition

When user types a spawn prompt like "Build new auth flow with rate limiting", Harmony must decide which agents to spawn. Use this template to call an LLM (or use keyword matching in v0.1):

**v0.1 uses keyword matching, NOT LLM, for spawn decomposition** (simpler, no API key needed):
```rust
pub fn decompose_spawn_prompt(prompt: &str) -> Vec<AgentRole> {
    // Always spawn these 3 roles for any prompt in v0.1
    // v0.2 will use LLM to customize roles based on task
    vec![
        AgentRole { name: "Architect".into(), avatar_key: "agent-architect".into(), 
                    description: "Plans the implementation approach".into() },
        AgentRole { name: "Coder".into(), avatar_key: "agent-coder".into(),
                    description: "Writes the implementation code".into() },
        AgentRole { name: "Tester".into(), avatar_key: "agent-tester".into(),
                    description: "Writes and validates tests".into() },
    ]
}
```

---

## 13. UI PANEL SPECS — LAYOUT, STATE, EVENTS

### Agent Team Sidebar

**State struct:**
```rust
pub struct AgentTeamPanel {
    pub agents: Vec<Agent>,
    pub is_loading: bool,
    pub error: Option<String>,
    pub spawn_input: String,         // text in the prompt box
    pub is_spawning: bool,
    pub ipc: IpcClient,
}
```

**Layout (top to bottom):**
```
┌─────────────────────────────────────┐
│  [🎼] Agent Team            [+ Add] │  ← header with spawn button
├─────────────────────────────────────┤
│  ┌──────────────────────────────┐   │
│  │  [Avatar] Architect     ●●   │   │  ← agent card
│  │  Shadow · Working · 2 tasks  │   │
│  │  [Live] [Pause] [Memory] [✕] │   │
│  └──────────────────────────────┘   │
│  ┌──────────────────────────────┐   │
│  │  [Avatar] Coder          ●●  │   │
│  │  Shadow · Idle            │   │
│  │  [Live] [Pause] [Memory] [✕] │   │
│  └──────────────────────────────┘   │
├─────────────────────────────────────┤
│  > Spawn: [___________________] [→] │  ← prompt input
└─────────────────────────────────────┘
```

**Agent card colors:**
- Status Idle: text-dim, dot gray
- Status Working: text normal, dot green (pulsing)
- Status Negotiating: text normal, dot yellow (pulsing)
- Status Paused: text-dim, dot gray
- Status Error: text red, dot red
- Mode Live: mode badge blue
- Mode Shadow: mode badge purple

**Events:**
```rust
pub enum AgentTeamEvent {
    SpawnPressed(String),           // prompt text
    ToggleModePressed(Uuid),        // agent_id
    PausePressed(Uuid),
    RemovePressed(Uuid),
    ViewMemoryPressed(Uuid),
    SpawnInputChanged(String),
}
```

### Harmony Pulse Panel

**State struct:**
```rust
pub struct PulsePanel {
    pub overlaps: Vec<OverlapEvent>,
    pub selected_overlap: Option<Uuid>,
    pub impact_graphs: HashMap<Uuid, ImpactGraph>,
    pub negotiation_results: HashMap<Uuid, NegotiationResult>,
    pub preview_active: bool,
    pub is_loading_impact: bool,
    pub ipc: IpcClient,
}
```

**Layout:**
```
┌─────────────────────────────────────────────────────┐
│  ⚡ Harmony Pulse                        [Clear All] │
├─────────────────────────────────────────────────────┤
│  ┌─────────────────────────────────────────────┐    │
│  │ 🔴 middleware/auth.ts · 2 min ago           │    │  ← change card
│  │ ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ │    │
│  │ You modified `validateJWT()` on lines 44-67 │    │
│  │ Agent Architect added Redis cache on L52-71 │    │
│  │                                             │    │
│  │ Impact: Both affect `validateJWT`. 3 callers│    │
│  │ found: routes/auth.ts, middleware/csrf.ts   │    │
│  │                                             │    │
│  │ [Accept Mine] [Accept Theirs] [Negotiate ✨]│    │
│  │ [What-if Preview] [Manual Edit]             │    │
│  └─────────────────────────────────────────────┘    │
│                                                     │
│  ┌─── Negotiated Diff (pending review) ───────┐    │
│  │ @@ -44,8 +44,12 @@                         │    │  ← shown after negotiation
│  │ + const cached = await redis.get(token_key)│    │
│  │ + if (cached) return cached;               │    │
│  │   const payload = jwt.verify(...)          │    │
│  │ Rationale: Preserves JWT validation while  │    │
│  │ adding Redis cache layer before verify.    │    │
│  │                         [✓ Accept] [✗ Reject] │    │
│  └─────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────┘
```

**Change card color coding:**
- Red border: Complex impact (sandbox required)
- Yellow border: Moderate impact
- Green border: Simple impact (auto-resolvable)

**Events:**
```rust
pub enum PulseEvent {
    AcceptMine(Uuid),              // overlap_id
    AcceptTheirs(Uuid),
    StartNegotiation(Uuid),
    ShowWhatIf(Uuid),
    OpenManualEdit(Uuid),
    AcceptNegotiated(Uuid),
    RejectNegotiated(Uuid),
    DismissOverlap(Uuid),
    ClearAll,
}
```

### Notification badge
When an overlap is detected and the Pulse panel is closed:
- Show a red badge count on the Pulse panel tab
- Show a non-blocking notification: "⚡ 1 overlap in middleware/auth.ts — Cmd+Shift+H"
- The notification auto-dismisses after 8 seconds
- Do NOT show a modal or block user input at any point

---

## 14. CONFIG FILE FORMAT

Config lives at `.harmony/config.toml` in the project root. Auto-created with defaults on first run.

```toml
[general]
# Session identifier (auto-generated, do not edit)
session_id = "550e8400-e29b-41d4-a716-446655440000"

# How long (minutes) to look back when checking for overlapping changes
overlap_window_minutes = 30

# How many recent tags to keep in memory (older ones stay in DB only)
max_recent_tags = 500

[human]
# Display name for the human participant
username = "awanish"
actor_id = "human:awanish"

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
# Increase on machines with more RAM
max_records_in_memory = 10000

[negotiation]
# LLM backend for negotiation. Options:
#   "agent" - use one of the spawned agents' LLM (preferred, no extra API key)
#   "openai" - use OpenAI-compatible API
#   "anthropic" - use Anthropic API
#   "disabled" - no auto-negotiation
negotiation_backend = "agent"

# Only used if negotiation_backend = "openai" or "anthropic"
# api_key = ""
# model = "gpt-4o"  # or "claude-sonnet-4-6"
# base_url = "https://api.openai.com/v1"  # override for local models

[agents]
# Known ACP agent endpoints. Harmony will try these on spawn.
# Override these with actual endpoints.
[[agents.registry]]
name = "opencode"
endpoint = "http://localhost:4231"

[[agents.registry]]
name = "gemini-cli"
endpoint = "http://localhost:4232"

[ui]
# Ghost highlight colors (hex, with alpha)
ghost_add_color = "#7ee8a280"    # green with 50% alpha
ghost_remove_color = "#f0606060" # red with 37% alpha

# Pulse notification duration before auto-dismiss (seconds)
notification_duration_seconds = 8
```

---

## 15. ERROR HANDLING — EVERY ERROR TYPE

### `crates/harmony-core/src/errors.rs`

```rust
#[derive(thiserror::Error, Debug)]
pub enum HarmonyError {
    // Database
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    // Serialization
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    // Shadow diff
    #[error("shadow diff error: {0}")]
    Shadow(#[from] crate::shadow::ShadowError),

    // IPC
    #[error("IPC connection failed: {0}")]
    IpcConnection(String),
    #[error("IPC timeout after {timeout_ms}ms")]
    IpcTimeout { timeout_ms: u64 },
    #[error("IPC response parse error: {0}")]
    IpcParse(String),

    // Agent
    #[error("agent {agent_id} not found")]
    AgentNotFound { agent_id: uuid::Uuid },
    #[error("agent ACP endpoint unreachable: {endpoint}")]
    AgentUnreachable { endpoint: String },
    #[error("agent task rejected: {reason}")]
    AgentTaskRejected { reason: String },

    // Analysis
    #[error("Tree-sitter parse error for language {language}: {detail}")]
    TreeSitterParse { language: String, detail: String },
    #[error("LSP server not found for language {language}. Install {install_hint}.")]
    LspNotFound { language: String, install_hint: String },
    #[error("LSP request timed out")]
    LspTimeout,

    // Sandbox
    #[error("no test command found in project root")]
    NoTestCommand,
    #[error("sandbox test run timed out after {timeout_s}s")]
    SandboxTimeout { timeout_s: u64 },
    #[error("sandbox test run failed to start: {0}")]
    SandboxStartFailed(String),

    // Negotiation
    #[error("negotiation failed: LLM returned invalid JSON: {0}")]
    NegotiationInvalidResponse(String),
    #[error("negotiation failed: proposed diff does not apply cleanly")]
    NegotiationBadDiff,
    #[error("negotiation backend not configured")]
    NegotiationNotConfigured,

    // Memory
    #[error("embedding model failed to initialize: {0}")]
    EmbeddingInit(String),
    #[error("embedding computation failed: {0}")]
    EmbeddingFailed(String),

    // Config
    #[error("config file parse error: {0}")]
    ConfigParse(String),

    // Generic
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("unexpected error: {0}")]
    Internal(String),
}

/// IPC error codes for SidecarResponse::Error
pub mod error_codes {
    pub const AGENT_NOT_FOUND: u32 = 1001;
    pub const AGENT_UNREACHABLE: u32 = 1002;
    pub const OVERLAP_NOT_FOUND: u32 = 1003;
    pub const DIFF_NOT_APPLICABLE: u32 = 1004;
    pub const NEGOTIATION_FAILED: u32 = 1005;
    pub const ANALYSIS_FAILED: u32 = 1006;
    pub const DATABASE_ERROR: u32 = 2001;
    pub const INTERNAL: u32 = 9999;
}
```

### Error handling rules
1. **Never panic in production code.** All functions that can fail return `Result<T, HarmonyError>`.
2. **Never propagate LSP errors to the user.** If LSP fails, log a warning and continue with Tree-sitter-only analysis.
3. **Never block the extension UI on IPC errors.** If IPC times out, show "Sidecar disconnected — restart Zed" in the panel and attempt reconnect after 5 seconds.
4. **Log all errors with `tracing::error!` in the sidecar.** The extension only sees sanitized error messages.
5. **Sandbox failures are non-fatal.** If sandbox fails (timeout, missing test command), mark `sandbox_required=false` and proceed with just semantic analysis. Show warning in Pulse panel: "⚠ Sandbox analysis unavailable".

---

## 16. BUILD, RUN & TEST COMMANDS

### Building the native sidecar binaries
```bash
# Build all native crates (sidecar + MCP server)
cargo build --release -p harmony-core -p harmony-analyzer -p harmony-memory -p harmony-mcp

# Binary locations after build:
# ./target/release/harmony-mcp     ← MCP server binary
# (no separate sidecar binary — the MCP server IS the sidecar in v0.1)
```

### Building the Zed extension (WASM)
```bash
# Add WASM target first
rustup target add wasm32-wasip2

# Build extension crate
cargo build --release -p harmony-extension --target wasm32-wasip2

# Output: ./target/wasm32-wasip2/release/harmony_extension.wasm
```

### extension.toml — full manifest
```toml
[extension]
id = "harmony"
name = "Harmony"
version = "0.1.0"
schema_version = 1
description = "Intelligent mediation for parallel human + AI development"
authors = ["Awanish Maurya <awanish@xpwnit.me>"]
repository = "https://github.com/iamawanishmaurya/harmony-zed"

[lib]
path = "crates/harmony-extension"

[[context_servers]]
name = "harmony-memory"
binary = { name = "harmony-mcp", path = "crates/harmony-mcp" }
```

### Installing in Zed for development
```bash
# Method 1: Zed dev extension loading
# In Zed: Extensions → Install Dev Extension → select harmony-zed/ folder

# Method 2: Manual copy
cp -r harmony-zed/ ~/.config/zed/extensions/harmony/
# Then restart Zed
```

### Running tests
```bash
# All tests
cargo test --workspace

# Specific module tests
cargo test -p harmony-core -- overlap
cargo test -p harmony-memory -- store
cargo test -p harmony-analyzer -- treesitter

# Run with logs visible
RUST_LOG=debug cargo test -p harmony-core -- --nocapture
```

### Running the MCP server standalone (for debugging)
```bash
./target/release/harmony-mcp \
  --db-path /path/to/project/.harmony/memory.db
# Then send JSON-RPC messages to stdin
```

### Running the stub agent (for testing without a real ACP agent)
```bash
cargo run --bin harmony-mcp -- \
  --stub-agent \
  --stub-port 4231 \
  --db-path /path/to/project/.harmony/memory.db
```

### Directory setup (run once per project before testing)
```bash
# The sidecar creates this automatically, but you can pre-create it:
mkdir -p .harmony
touch .harmony/.gitkeep
echo '.harmony/memory.db' >> .gitignore
echo '.harmony/harmony.sock' >> .gitignore
```

---

## 17. IMPLEMENTATION ORDER — EXACT SEQUENCE WITH ACCEPTANCE CRITERIA

Follow this order exactly. Do not start a task until the previous one passes its acceptance criteria.

---

### TASK 01 — Workspace & Cargo scaffold
**Do:** Create the full directory structure from §3. Create all `Cargo.toml` files with exact deps from §4. Create empty `src/lib.rs` (with `// TODO`) for every crate. Create `extension.toml` from §16. Verify `cargo check --workspace` passes with zero errors.

**Acceptance criteria:**
- `cargo check --workspace` exits 0
- `cargo check -p harmony-extension --target wasm32-wasip2` exits 0
- All directories and files from §3 exist (even if empty)

---

### TASK 02 — Data models
**Do:** Implement ALL types from §6 in `crates/harmony-core/src/types.rs`. Derive `Serialize, Deserialize, Debug, Clone` on everything. Implement `TextRange::overlaps`. Add `similar = "2.5"` to harmony-core deps. Add all types to `lib.rs` as `pub mod types; pub use types::*;`.

**Acceptance criteria:**
- `cargo test -p harmony-core` passes
- Every type in §6 exists and compiles
- Can `serde_json::to_string(&ProvenanceTag {...})` without error

---

### TASK 03 — SQLite schema & store scaffold
**Do:** Implement `crates/harmony-memory/src/schema.rs` with all migrations from §7. Implement `MemoryStore::open` that creates the DB file, runs all pending migrations (check `schema_version` table before each), and applies PRAGMA settings. Implement `MemoryStore::insert_provenance_tag` and `get_recent_tags_for_file`.

**Acceptance criteria:**
- `cargo test -p harmony-memory` passes
- Test: create a `MemoryStore` in a temp dir, insert 3 provenance tags for the same file, `get_recent_tags_for_file` returns all 3

---

### TASK 04 — Overlap detection
**Do:** Implement `crates/harmony-core/src/overlap.rs` with `detect_overlaps` and `is_real_overlap` exactly as specified in §11 Algorithm A steps 1-2. Write unit tests.

**Acceptance criteria:**
- Test 1: Two tags same file, overlapping region, different actors → 1 OverlapEvent returned
- Test 2: Two tags same file, non-overlapping region, different actors → 0 events
- Test 3: Two tags same file, overlapping region, SAME actor → 0 events
- Test 4: Two tags different files → 0 events

---

### TASK 05 — Shadow diff
**Do:** Implement `crates/harmony-core/src/shadow.rs`: `apply_shadow_diff`, `compute_unified_diff`, `content_hash`, `is_diff_applicable`. Use `similar` crate for diffs. Write unit tests with realistic TypeScript content.

**Acceptance criteria:**
- `compute_unified_diff(before, after, "auth.ts")` returns valid unified diff string
- `apply_shadow_diff(original, diff)` returns modified content identical to `after`
- Round-trip: `apply(original, compute(original, modified)) == modified`
- Stale diff: modify original again → `is_diff_applicable` returns false

---

### TASK 06 — MCP server (tools only, no embedding yet)
**Do:** Implement the full `harmony-mcp` binary. Set up rmcp stdio server. Implement `report_change`, `add_memory` (store without embedding for now — store empty vec), `query_memory` (return all records matching namespace for now — no similarity), `list_decisions`. Wire all tools to `MemoryStore`.

**Acceptance criteria:**
- Binary compiles and starts without error
- Can send JSON-RPC `initialize` → receives capabilities response
- Can call `report_change` via JSON-RPC → new provenance tag in DB
- Can call `add_memory` → new record in DB
- Can call `query_memory` → returns records

---

### TASK 07 — Embedding engine
**Do:** Implement `crates/harmony-memory/src/embeddings.rs` with `EmbeddingEngine::new`, `embed_one`, `embed_batch`, `cosine_similarity`. Update `MemoryStore::add_memory` to compute and store actual embeddings. Update `MemoryStore::query_memory` to do cosine similarity ranking.

**Acceptance criteria:**
- `EmbeddingEngine::new()` succeeds (downloads model if needed — okay for CI to skip with env var `HARMONY_SKIP_EMBEDDING_TESTS=1`)
- `embed_one("hello world")` returns vec of 384 f32 values
- `cosine_similarity` of identical vecs = 1.0 (or very close)
- `query_memory("why did we reject redis")` ranks a record containing "rejected Redis due to cost" above one containing "added logging middleware"

---

### TASK 08 — Tree-sitter analyzer
**Do:** Implement `crates/harmony-analyzer/src/treesitter.rs` with `TreeSitterAnalyzer::new`, `detect_language`, `extract_symbols_in_range`. Use actual tree-sitter query strings from §11.

**Acceptance criteria:**
- `detect_language("src/auth.ts")` → TypeScript
- `detect_language("main.rs")` → Rust
- `detect_language("index.jsx")` → JavaScript
- Given TypeScript function definition in range → returns AffectedSymbol with kind=Function
- Given import statement in range → returns AffectedSymbol with kind=Import

---

### TASK 09 — Impact analyzer
**Do:** Implement `crates/harmony-analyzer/src/impact.rs` with `ImpactAnalyzer::new` and `analyze`. LSP is optional in this task — implement with `lsp: None` first. Wire Tree-sitter results into `ImpactGraph`. Implement `build_impact_summary` from §12 Template A.

**Acceptance criteria:**
- Given a mock OverlapEvent with realistic TypeScript content → returns ImpactGraph with non-empty summary string
- Summary string contains actor names from overlap
- Complexity is one of Simple/Moderate/Complex

---

### TASK 10 — Sidecar IPC server
**Do:** The harmony-mcp binary acts as the sidecar. Add a Unix socket IPC server alongside the MCP stdio server (run both in parallel with tokio). IPC server listens at `{project_root}/.harmony/harmony.sock`. Implement all `SidecarCommand` handlers from §6. Wire them to `MemoryStore`, `detect_overlaps`, `ImpactAnalyzer`.

**Acceptance criteria:**
- Can connect to socket with `socat - UNIX-CONNECT:.harmony/harmony.sock`
- Send `{"cmd":"ping"}` → receive `{"result":"pong"}`
- Send `{"cmd":"get_agents"}` → receive `{"result":"agents","data":[]}`
- Full Algorithm A from §11 runs end-to-end when `report_change` MCP tool is called and overlap is found → IPC clients receive overlap notification

---

### TASK 11 — Zed extension: IPC client + Agent Team panel (static)
**Do:** Implement `harmony-extension` with: IPC client (`ipc.rs`) that connects to `.harmony/harmony.sock` and sends/receives `SidecarCommand`/`SidecarResponse` JSON. Register Agent Team panel. Render static UI with hardcoded dummy agents using layout from §13.

**Acceptance criteria:**
- Extension loads in Zed without error
- `Cmd+Shift+T` opens Agent Team panel
- Panel renders dummy agent cards with avatars, status badges, action buttons
- No crashes

---

### TASK 12 — Zed extension: Harmony Pulse panel (static)
**Do:** Register Harmony Pulse panel. Render static UI with hardcoded dummy overlap event using layout from §13.

**Acceptance criteria:**
- `Cmd+Shift+H` opens Pulse panel
- Panel renders dummy change card with summary text, impact description, action buttons
- No crashes

---

### TASK 13 — Wire extension to live sidecar
**Do:** On extension init, spawn the `harmony-mcp` sidecar via `zed_extension_api::Command`. Connect IPC client. Replace all dummy data with live data from IPC. Handle IPC disconnection with retry.

**Acceptance criteria:**
- Extension spawns sidecar on Zed open (verify with `ps aux | grep harmony-mcp`)
- Agent Team panel shows actual agents from DB (empty on fresh project)
- Pulse panel shows actual overlaps (empty initially)
- If sidecar crashes, extension shows "Sidecar disconnected" message and retries

---

### TASK 14 — Ghost highlights
**Do:** Implement `ghost.rs`. On receiving `ShadowDiffs` via IPC, register inline ghost text decorations in the Zed buffer API for each pending diff.

**Acceptance criteria:**
- Add a shadow diff via MCP `report_change` with a diff that adds lines
- Ghost green text appears in Zed editor at the right lines
- Clicking ghost text opens Pulse panel

---

### TASK 15 — Negotiation round-trip
**Do:** Implement `crates/harmony-core/src/negotiation.rs`. Wire `StartNegotiation` IPC command. Implement prompt builder from §12 Template B. Call LLM (start with "openai" backend, load API key from config). Parse `NegotiationResult`. Write memory notes.

**Acceptance criteria:**
- With a valid OpenAI API key in config, clicking "Negotiate" on a real overlap → negotiation result appears in Pulse panel within 30 seconds
- Proposed diff is valid unified diff that applies cleanly
- Rationale string is non-empty
- Memory note is written to `shared` namespace in DB

---

### TASK 16 — End-to-end integration test
**Do:** Write a test in `tests/` that: creates a temp project with 2 TypeScript files, starts the sidecar, calls `report_change` twice on the same file region with different actor IDs, verifies an OverlapEvent is created, verifies ImpactGraph is generated, verifies the summary string is sensible.

**Acceptance criteria:**
- Test passes reliably 3 times in a row
- Test takes < 30 seconds total

---

## 18. KNOWN CONSTRAINTS & EXPLICIT WORKAROUNDS

| Constraint | Workaround in v0.1 |
|---|---|
| Zed extension API has no collaboration hooks | File-system event bus via `.harmony/events/` dir. Overlap detection fires on save. |
| WASM can't do SQLite or embeddings | All heavy logic runs in native sidecar (`harmony-mcp`). Extension only does UI + IPC. |
| WASM can't access filesystem directly | Extension reads `.harmony/config.toml` via `zed_extension_api::fs::read` (sandboxed). Sidecar has full FS access. |
| No `wasm32-wasip2` support for `tokio` | Extension is synchronous/event-driven. Only the sidecar uses tokio. |
| ACP registry may not include all agents | Ship with 2 known agents (OpenCode, Gemini CLI). User can add custom endpoints in config. |
| `fastembed` downloads model on first run | Show progress in panel: "Downloading embedding model (130MB, first run only)..." |
| LSP binaries may not be installed | Graceful fallback to Tree-sitter only. Show one-time tip: "Install typescript-language-server for better impact analysis." |
| Unix socket not available on Windows | TODO comment in `ipc.rs`. v0.2 can add TCP fallback. |
| `rmcp` crate may have API changes | Pin to exact version in Cargo.toml. Check https://github.com/modelcontextprotocol/rust-sdk |

---

## 19. WHAT NOT TO BUILD IN v0.1

The following features are explicitly **out of scope**. If you find yourself starting to implement any of these, stop and re-read this document.

- ❌ Git integration (no git hooks, no git object storage, no `git blame`)
- ❌ Cloud sync of any kind (no HTTP endpoints for memory, no websockets to external servers)
- ❌ Voice/audio features
- ❌ Agent-to-agent messaging (agents only communicate through the negotiation round, initiated by human)
- ❌ Multi-workspace federation (one project root at a time)
- ❌ Any UI beyond the two panels (Agent Team sidebar + Harmony Pulse). No settings GUI. Config is TOML file only.
- ❌ Streaming LLM responses (negotiation is a single request/response)
- ❌ Persistent agent sessions across machine restarts (agents must be re-spawned)
- ❌ Windows support (Unix socket only for IPC in v0.1)
- ❌ Language support beyond TypeScript, JavaScript, Rust (Tree-sitter grammars only for these 3)
- ❌ Custom Tree-sitter queries per-project (§11's queries are hardcoded in v0.1)
- ❌ Test suite parsing for languages other than those with output format: `X passed, Y failed`

---

## APPENDIX A — `.harmony/` directory contents

```
.harmony/
├── config.toml          # User config (committed to git if desired)
├── memory.db            # SQLite DB (gitignored)
├── harmony.sock         # Unix socket (gitignored, recreated on start)
└── events/              # Temporary event files (gitignored, auto-cleaned)
    └── .gitkeep
```

Add to project `.gitignore`:
```
.harmony/memory.db
.harmony/harmony.sock
.harmony/events/*.json
```

---

## APPENDIX B — Glossary

| Term | Definition |
|---|---|
| **Actor** | A participant in the project: a human or an AI agent |
| **Shadow mode** | Agent's edits are tracked privately, not applied to the live file. Shown as ghost highlights. |
| **Live mode** | Agent's edits are applied to the workspace in real-time (same as a human collaborator) |
| **Provenance tag** | Metadata record: who changed what, in which file/region, via which task, at what time |
| **Overlap event** | When two different actors' changes affect the same file region within the overlap window |
| **Harmony Pulse** | The mediator panel that surfaces overlap events with analysis and resolution UI |
| **Sidecar** | The native Rust process (`harmony-mcp`) spawned by the extension to do heavy lifting |
| **IPC** | Inter-process communication between the WASM extension and the native sidecar (via Unix socket) |
| **ACP** | Agent Communication Protocol — Zed's standard for talking to external AI agents |
| **MCP** | Model Context Protocol — standard for giving AI agents access to tools and resources |
| **Negotiation** | A single LLM call that takes both conflicting diffs and produces a merged diff |
| **Ghost highlight** | Inline editor decoration showing a shadow agent's proposed addition/removal (not yet applied) |

---

*End of Harmony Implementation Specification v0.1.0-impl*  
*Awanish Maurya · XPWNIT LAB · xpwnit.me · April 2026*
