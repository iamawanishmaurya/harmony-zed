# Harmony — Project Overview

> **Intelligent mediation for parallel human + AI development**

Harmony is a Zed editor extension that makes simultaneous editing by humans and AI agents safe, transparent, and collaborative. When multiple participants modify the same codebase, Harmony detects conflicts before they become merge nightmares, analyzes the semantic impact using AST parsing, and helps resolve overlaps through AI-assisted negotiation — all without leaving your editor.

---

## What Problem Does Harmony Solve?

Modern development increasingly involves AI agents (Copilot, Cursor, Claude Code, etc.) working alongside human developers. When a human is editing `auth.ts` line 44–67 to fix JWT validation while an AI agent simultaneously adds Redis caching at lines 52–71, **nobody knows about the conflict until a git merge fails hours later**.

Harmony solves this by:
1. **Tracking every change** with full provenance (who, what, where, when, why)
2. **Detecting overlaps in real-time** when two participants touch the same file region
3. **Analyzing impact** using Tree-sitter AST diffing + optional LSP dependency lookup
4. **Surfacing non-intrusive notifications** with plain-English summaries
5. **Enabling AI-assisted merge negotiation** that proposes unified diffs

---

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    Zed Editor                           │
│  ┌──────────────────────────────────────────────────┐   │
│  │        harmony-extension (WASM)                  │   │
│  │  • Agent Team Panel (Cmd+Shift+T)                │   │
│  │  • Harmony Pulse Panel (Cmd+Shift+H)             │   │
│  │  • Ghost highlight decorations                   │   │
│  └────────────────────┬─────────────────────────────┘   │
│                       │ stdin/stdout (JSON-RPC)         │
│  ┌────────────────────▼─────────────────────────────┐   │
│  │        harmony-mcp (native sidecar)              │   │
│  │  ┌──────────────────────────────────────────┐    │   │
│  │  │  harmony-core     │  harmony-analyzer    │    │   │
│  │  │  • Types          │  • Tree-sitter       │    │   │
│  │  │  • Overlap Detect │  • Impact Analysis   │    │   │
│  │  │  • Shadow Diff    │  • LSP Client        │    │   │
│  │  │  • Negotiation    │                      │    │   │
│  │  │  • Config Loader  │                      │    │   │
│  │  │  • Sandbox        │                      │    │   │
│  │  └──────────────────────────────────────────┘    │   │
│  │  ┌──────────────────────────────────────────┐    │   │
│  │  │  harmony-memory                          │    │   │
│  │  │  • SQLite Store (provenance, agents,     │    │   │
│  │  │    memory records, overlaps)             │    │   │
│  │  │  • Embedding Engine (keyword fallback    │    │   │
│  │  │    or neural BGE-Small-EN-v1.5)          │    │   │
│  │  └──────────────────────────────────────────┘    │   │
│  └──────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────┘
```

### Why a native sidecar?

WASM (used by Zed extensions) **cannot**:
- Access SQLite databases
- Run embedding models (ONNX runtime)
- Parse code with Tree-sitter
- Spawn LSP servers

So all heavy computation runs in a native Rust binary (`harmony-mcp`) that the WASM extension communicates with via JSON-RPC over stdin/stdout.

---

## The 5 Crates

| Crate | Type | Purpose |
|-------|------|---------|
| `harmony-core` | Library | Shared data models, overlap detection, shadow diffing, negotiation prompts, config loader, sandbox executor |
| `harmony-memory` | Library | SQLite persistence (all CRUD ops), embedding engine for semantic memory search |
| `harmony-analyzer` | Library | Tree-sitter code parsing, LSP client, impact analysis engine |
| `harmony-mcp` | Binary | Native sidecar — MCP server exposing tools over JSON-RPC stdio |
| `harmony-extension` | cdylib | WASM extension for Zed — panels, ghost highlights, sidecar lifecycle |

---

## Key Concepts

### Provenance Tags
Every code change is recorded as a `ProvenanceTag` containing:
- **Who**: Actor ID (`human:awanish` or `agent:architect-01`)
- **What**: Unified diff of the change
- **Where**: File path + specific line range (TextRange)
- **When**: Timestamp
- **Why**: Task prompt that motivated the change

### Overlap Detection
When a new change is reported, Harmony checks all recent tags for the same file and detects overlaps based on 4 rules:
1. Same file path
2. Different actors (same actor can't conflict with itself)
3. Overlapping line regions
4. Within time window (default: 30 minutes)

### Shadow Mode
Agents work in **shadow mode** by default — their edits are stored privately and rendered as translucent "ghost highlights" in the editor. The actual file is never modified until the human explicitly accepts the change. This prevents agents from accidentally breaking a file while the human is editing it.

### Impact Analysis
When an overlap is detected, Harmony runs:
1. **Tree-sitter AST analysis** — extracts affected symbols (functions, classes, imports) from both changes
2. **LSP reference lookup** (optional) — finds callers of modified functions
3. **Complexity scoring** — Simple / Moderate / Complex classification
4. **Impact summary** — Human-readable explanation (e.g., "You modified `validateJWT`. Agent Architect added Redis cache in the same function.")

### Negotiation
For complex overlaps, Harmony can call an LLM to produce a merged diff that preserves the intent of both changes. The LLM receives the two conflicting diffs, the impact analysis, and relevant team memory, then produces a unified diff with rationale.

### Team Memory
Harmony maintains a SQLite-backed "team memory" — structured notes about decisions, rejections, and context. Memory records are embedded using a keyword-frequency hash (or neural embeddings with `fastembed`) and retrieved by cosine similarity. Agents can query memory via MCP tools to avoid repeating past mistakes.

---

## Configuration

Config lives at `.harmony/config.toml` in your project root. Auto-created with sensible defaults on first run.

```toml
[general]
overlap_window_minutes = 30

[human]
username = "awanish"

[analysis]
lsp_mode = "auto"
sandbox_mode = "complex_only"

[memory]
embedding_model = "bge-small-en-v1.5"

[negotiation]
negotiation_backend = "agent"

[ui]
ghost_add_color = "#7ee8a280"
ghost_remove_color = "#f0606060"
```

See the [full config reference](../HARMONY_IMPL_SPEC.md#14-config-file-format) for all options.

---

## What Harmony Does NOT Do

- ❌ Replace git (provenance is tracked in SQLite, not git objects)
- ❌ Cloud sync (100% local in v0.1)
- ❌ Chat interface (task assignment is a single prompt per agent)
- ❌ Swarm AI (agents work independently; "negotiation" is a single LLM call)
- ❌ Windows support for IPC (Unix socket only; TCP fallback planned for v0.2)
- ❌ Languages beyond TypeScript, JavaScript, Rust (Tree-sitter grammars for these 3 only)

---

## License

MIT — Awanish Maurya · XPWNIT LAB · 2026
