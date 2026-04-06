# Harmony — Implementation Overview

> **Status: v0.1.1 — All tasks complete · 95 tests passing · 0 failures**

This document tracks what has been implemented, how it was tested, and what's ready for integration testing with a live Zed editor.

---

## Completion Status

| Task | Title | Status | Tests | Crate |
|------|-------|--------|-------|-------|
| 01 | Workspace scaffold | ✅ Complete | — | all |
| 02 | Data models | ✅ Complete | 10 unit | harmony-core |
| 03 | SQLite schema & store | ✅ Complete | 12 unit + 3 integration | harmony-memory |
| 04 | Overlap detection | ✅ Complete | 6 unit + 4 integration | harmony-core |
| 05 | Shadow diffing | ✅ Complete | 6 unit + 5 integration | harmony-core |
| 06 | MCP server | ✅ Complete | builds ✓ | harmony-mcp |
| 07 | Embedding engine | ✅ Complete | 8 unit | harmony-memory |
| 08 | Tree-sitter analyzer | ✅ Complete | 7 unit + 1 integration | harmony-analyzer |
| 09 | Impact analyzer | ✅ Complete | 7 unit + 1 integration | harmony-analyzer |
| 10 | Sandbox executor | ✅ Complete | 8 unit | harmony-core |
| 11 | Config loader | ✅ Complete | 5 unit | harmony-core |
| 12 | Extension IPC + panels | ✅ Complete | 5 unit | harmony-extension |
| 13 | Sidecar lifecycle | ✅ Complete | — (state machine) | harmony-extension |
| 14 | Ghost highlights | ✅ Complete | 2 unit | harmony-extension |
| 15 | Negotiation prompts | ✅ Complete | 4 unit | harmony-core |
| 16 | E2E golden path | ✅ Complete | 5 integration | harmony-core (tests/) |
| — | **Multi-backend LLM** | ✅ Complete | **8 integration** | harmony-core (tests/) |
| — | **Windows TCP IPC** | ✅ Complete | builds ✓ | harmony-mcp |

**Total: 95 tests passing, 0 failing**

---

## Multi-Backend LLM Negotiation (v0.1.1)

The negotiation system now supports 4 backends via `call_negotiation_llm()`:

| Backend | Config Value | Auth | Endpoint |
|---------|-------------|------|----------|
| **OpenAI** | `"openai"` | `Authorization: Bearer {api_key}` | `{base_url}/chat/completions` |
| **GitHub Copilot** | `"openai"` | `Authorization: Bearer {ghp_token}` | `https://api.githubcopilot.com/chat/completions` |
| **Ollama (local)** | `"openai"` | `Authorization: Bearer ollama` | `http://localhost:11434/v1/chat/completions` |
| **LM Studio (local)** | `"openai"` | `Authorization: Bearer lm-studio` | `http://localhost:1234/v1/chat/completions` |
| **Anthropic Claude** | `"anthropic"` | `x-api-key: {api_key}` + `anthropic-version: 2023-06-01` | `{base_url}/v1/messages` |
| **ACP Agent** | `"agent"` | None (local endpoint) | `{agent.endpoint}/negotiate` |
| **Disabled** | `"disabled"` | — | Returns `NegotiationNotConfigured` |

### Response Parsing

- **OpenAI**: Extracts `response.choices[0].message.content` → parses as JSON
- **Anthropic**: Extracts `response.content[0].text` → parses as JSON
- **Agent**: Direct JSON body passthrough
- All backends expect the same `NegotiationResult` JSON format

---

## Windows TCP IPC Fallback (v0.1.1)

| Platform | Transport | Discovery File |
|----------|-----------|---------------|
| **Windows** | TCP `127.0.0.1:17432` | `.harmony/harmony.port` |
| **Linux/macOS** | Unix socket | `.harmony/harmony.sock` |

Both use identical Content-Length framed JSON-RPC 2.0 protocol. The extension auto-detects the platform and connects accordingly.

---

## What Each Module Does

### harmony-core

The pure-logic crate containing all shared types and algorithms. No I/O restrictions.

| File | Purpose |
|------|---------|
| `types.rs` | All shared structs: `Agent`, `ProvenanceTag`, `OverlapEvent`, `ImpactGraph`, `ShadowDiff`, `NegotiationResult`, `TextRange`, `ActorId`, enums |
| `errors.rs` | `HarmonyError` enum covering IPC, analysis, sandbox, negotiation, memory, config errors |
| `overlap.rs` | `detect_overlaps()` — checks 4 rules: same file, different actors, overlapping regions, within time window |
| `shadow.rs` | `compute_unified_diff()`, `apply_shadow_diff()`, `content_hash()` (SHA-256), `is_diff_applicable()` — uses `similar` crate |
| `negotiation.rs` | `build_negotiation_prompt()` (§12 Template B), `parse_negotiation_result()`, `call_negotiation_llm()` (4-backend async router), `decompose_spawn_prompt()` |
| `config.rs` | `HarmonyConfig` with all §14 fields. TOML serialization, rich commented template on first-run auto-create |
| `sandbox.rs` | `run_sandbox()` — copies project to temp dir, auto-detects test command (npm/cargo/make), runs tests, parses output |

### harmony-memory

SQLite persistence layer with semantic search capability.

| File | Purpose |
|------|---------|
| `schema.rs` | 5 migrations (agents, provenance_tags, shadow_diffs, memory_records, overlap_events), WAL mode, PRAGMAs |
| `store.rs` | Full CRUD: `insert_provenance_tag`, `upsert_agent`, `add_memory`, `query_memory` (with cosine similarity ranking), `insert_shadow_diff`, `insert_overlap_event` |
| `embeddings.rs` | `EmbeddingEngine` — keyword-frequency hash fallback (FNV-1a→384-d vectors) or neural `fastembed` (BGE-Small-EN-v1.5). Cosine similarity scoring. |

### harmony-analyzer

Code intelligence using Tree-sitter AST parsing and optional LSP.

| File | Purpose |
|------|---------|
| `treesitter.rs` | `TreeSitterAnalyzer` — parses TS/JS/Rust, extracts symbols in a TextRange using Tree-sitter queries |
| `lsp_client.rs` | `LspClient` — spawns `typescript-language-server` or `rust-analyzer`, sends JSON-RPC definition/references requests |
| `impact.rs` | `ImpactAnalyzer` — combines Tree-sitter + optional LSP to build `ImpactGraph` with complexity scoring and deterministic summary |

### harmony-mcp

Native sidecar binary that runs alongside Zed as an MCP server.

| File | Purpose |
|------|---------|
| `main.rs` | CLI entry with `--db-path` flag, tracing to stderr |
| `transport.rs` | JSON-RPC 2.0 stdio + TCP (Windows) / Unix socket (Linux) IPC servers |
| `tools.rs` | 4 MCP tools: `query_memory`, `add_memory`, `report_change` (with automatic overlap detection), `list_decisions` |
| `types.rs` | JSON-RPC request/response type definitions |

### harmony-extension

WASM extension for the Zed editor (compiles to `wasm32-wasip2`).

| File | Purpose |
|------|---------|
| `lib.rs` | Extension entry point, registers `HarmonyExtension` with Zed |
| `ipc.rs` | Platform-aware IPC: TCP on Windows (reads `.harmony/harmony.port`), Unix socket on Linux/macOS, Stdio fallback |
| `sidecar.rs` | `SidecarHandle` — lifecycle management with exponential backoff restart |
| `panels.rs` | `AgentTeamPanel` + `PulsePanel` state structs, color theming, ghost highlight parser |
| `config.rs` | `ExtensionConfig` — minimal TOML reader for UI-relevant settings |

---

## How the Golden Path Works

```
1. Human edits auth.ts lines 5–20 (fix error messages)
         │
         ▼
2. report_change MCP tool → ProvenanceTag stored
         │
         ▼
3. Agent edits auth.ts lines 11–18 (add Redis caching)
         │
         ▼
4. report_change → ProvenanceTag stored
         │
         ▼
5. detect_overlaps() → OverlapEvent detected (regions 5-20 ∩ 11-18)
         │
         ▼
6. ImpactAnalyzer runs Tree-sitter → ImpactGraph
   Summary: "You modified validateJWT. Agent Architect added Redis cache."
   Complexity: Moderate
         │
         ▼
7. Harmony Pulse panel shows notification with:
   - Impact summary
   - [Accept Mine] [Accept Theirs] [Negotiate ✨]
         │
         ▼
8. User clicks "Negotiate" → call_negotiation_llm() routes to backend
   → Proposed unified diff + rationale
         │
         ▼
9. User clicks "Accept" → diff applied, memory note stored
```

---

## LLM Backend Config Examples

The auto-generated `.harmony/config.toml` includes all backend options as comments:

```toml
[negotiation]
# Options: "agent" (default) | "openai" | "anthropic" | "disabled"
negotiation_backend = "agent"

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

# Ollama (local, no API key needed)
# negotiation_backend = "openai"
# api_key = "ollama"
# model = "llama3.3"
# base_url = "http://localhost:11434/v1"

# LM Studio (local)
# negotiation_backend = "openai"
# api_key = "lm-studio"
# model = "local-model"
# base_url = "http://localhost:1234/v1"
```

---

## Embedding Engine Strategy

### Keyword-Frequency Fallback (default, instant)
- Each word is hashed with FNV-1a to 3 dimensions in a 384-d vector
- Vectors are L2-normalized for cosine similarity
- Works offline with zero setup

### Neural Embeddings (opt-in, `--features embeddings`)
- Uses `fastembed` with BGE-Small-EN-v1.5 (130 MB ONNX model)
- Enable with: `cargo build -p harmony-memory --features embeddings`

---

## Build & Test Commands

```bash
# Run ALL tests (95 tests)
cargo test -p harmony-core -p harmony-memory -p harmony-analyzer

# Run only the 8 new negotiation backend tests
cargo test -p harmony-core --test negotiation_tests

# Run the E2E golden path
cargo test -p harmony-core --test e2e_golden_path

# Build the native sidecar
cargo build --release -p harmony-mcp

# Build the WASM extension (requires wasm32-wasip2 target)
rustup target add wasm32-wasip2
cargo build --release -p harmony-extension --target wasm32-wasip2

# Cross-compile check for Windows from Linux
cargo check -p harmony-mcp --target x86_64-pc-windows-gnu
```

---

## Troubleshooting Guide

| Symptom | Likely Cause | Fix |
|---------|-------------|-----|
| `NegotiationNotConfigured` at runtime | `negotiation_backend` key missing or typo in config | Check `.harmony/config.toml` spelling |
| `401 Unauthorized` from Copilot | Token missing `copilot` OAuth scope | Regenerate token at github.com/settings/tokens |
| `Connection refused` on Windows | Sidecar not writing `.harmony/harmony.port` | Check TCP fallback compiled with `#[cfg(target_os = "windows")]` |
| `NegotiationInvalidResponse` | LLM returned markdown instead of raw JSON | Prompt already includes `"Respond ONLY with valid JSON"` (Template B) |
| Anthropic `401` | Missing `anthropic-version` header | Both `x-api-key` and `anthropic-version: 2023-06-01` are required |
| Ollama timeout | Model not pulled yet | Run `ollama pull llama3.3` first |

---

## Design Decisions

| Decision | Rationale |
|----------|-----------|
| Custom JSON-RPC transport | The `rmcp` crate had API instability. Hand-rolled transport is ~120 lines and spec-compliant. |
| Keyword fallback for embeddings | Avoids 130 MB ONNX runtime download during dev. Still provides meaningful similarity ranking. |
| `tempfile` for integration tests | Each test gets an isolated SQLite instance. No global state, no cleanup needed. |
| Config auto-creation with comments | First-run experience: just open a project and everything works with all backend options visible. |
| TCP fallback on Windows | Unix sockets not available on Windows. Port 17432 is ephemeral and unlikely to conflict. |
| `reqwest` for HTTP | Mature, async, production-grade HTTP client. Handles TLS, redirects, timeouts automatically. |
| `httpmock` for tests | Zero real network calls needed. Each test gets its own mock server. |
| OpenAI branch handles 4 services | Copilot, Ollama, LM Studio all speak the OpenAI API — just different `base_url` values. |

---

## File Tree (final)

```
harmony-zed/
├── Cargo.toml                    # workspace root (reqwest added)
├── extension.toml                # Zed extension manifest
├── HARMONY_IMPL_SPEC.md          # Complete specification
├── docs/
│   ├── PROJECT.md                # What is Harmony
│   └── OVERVIEW.md               # Implementation status (this file)
│
├── crates/
│   ├── harmony-core/
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── types.rs          # All data models
│   │   │   ├── errors.rs         # Error enum + codes
│   │   │   ├── overlap.rs        # Overlap detection (4 rules)
│   │   │   ├── shadow.rs         # Unified diff engine
│   │   │   ├── negotiation.rs    # LLM prompt builder + multi-backend caller
│   │   │   ├── config.rs         # TOML config loader + commented template
│   │   │   └── sandbox.rs        # Test runner for diffs
│   │   └── tests/
│   │       ├── overlap_tests.rs
│   │       ├── shadow_tests.rs
│   │       ├── negotiation_tests.rs  ← 8 httpmock tests (NEW)
│   │       └── e2e_golden_path.rs
│   │
│   ├── harmony-memory/
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── schema.rs         # 5 SQLite migrations
│   │   │   ├── store.rs          # Full CRUD
│   │   │   └── embeddings.rs     # Dual-mode embedding engine
│   │   └── tests/
│   │       └── memory_tests.rs
│   │
│   ├── harmony-analyzer/
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── treesitter.rs     # TS/JS/Rust AST parser
│   │   │   ├── lsp_client.rs     # JSON-RPC LSP client
│   │   │   └── impact.rs         # Impact analysis engine
│   │   └── tests/
│   │       └── analyzer_tests.rs
│   │
│   ├── harmony-mcp/
│   │   └── src/
│   │       ├── main.rs           # CLI entry, sidecar binary
│   │       ├── transport.rs      # stdio + TCP (Win) / Unix socket (Linux)
│   │       ├── tools.rs          # 4 MCP tools
│   │       └── types.rs          # JSON-RPC types
│   │
│   └── harmony-extension/
│       └── src/
│           ├── lib.rs            # Zed extension entry
│           ├── ipc.rs            # Platform-aware IPC (TCP/Unix/Stdio)
│           ├── sidecar.rs        # Lifecycle + backoff
│           ├── panels.rs         # Agent Team + Pulse panels
│           └── config.rs         # Extension-side config
│
└── tests/                        # Legacy integration tests
    ├── overlap_tests.rs
    ├── shadow_tests.rs
    ├── memory_tests.rs
    └── analyzer_tests.rs
```

---

*Harmony v0.1.1 — Awanish Maurya · XPWNIT LAB · April 2026*
