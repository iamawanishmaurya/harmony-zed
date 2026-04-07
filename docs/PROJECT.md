# Harmony - Project Overview

> Intelligent mediation for parallel human + AI development inside Zed.

---

## What Problem Harmony Solves

Harmony helps when a human and an AI assistant both work on the same project and risk silently editing the same file region.

Instead of waiting for that conflict to show up later in git, Harmony records edit provenance early:

- who changed a file
- which file and line range changed
- when it changed
- why it changed
- whether the new change overlaps an existing one

That makes overlap detection visible while work is still in progress.

---

## Current Practical Model

Today, Harmony works best as a project-local tracking layer for:

- Zed context-server startup
- assistant edit reporting
- human/agent overlap detection
- lightweight project memory

The current practical loop is:

```text
assistant edits file
-> /harmony-sync
-> Harmony records agent provenance
-> human edits same file region
-> Harmony records human provenance
-> harmony_pulse reports the overlap
```

The `sync` step exists because the current Zed Rust extension API does not expose a direct "assistant just edited this file" callback yet.

---

## Architecture

```text
Zed
  -> harmony-extension (WASM)
     -> context server configuration
     -> /harmony-pulse
     -> /harmony-sync
  -> harmony-mcp (native sidecar over stdio MCP)
     -> tool routing
     -> sync command
     -> doctor and pulse commands
     -> shared tracking logic
  -> harmony-core / harmony-memory / harmony-analyzer
     -> overlap detection
     -> persistence
     -> impact analysis
```

### Why a native sidecar exists

The extension runs as WASM, while Harmony still needs native capabilities such as:

- SQLite access
- local file scanning for sync
- Tree-sitter analysis
- richer project bootstrap logic

That work happens in `harmony-mcp`.

---

## Main Concepts

### Provenance tags

A provenance tag records:

- actor id, such as `human:water` or `agent:copilot`
- file path
- line range
- timestamp
- unified diff
- optional task prompt

### Overlap detection

Harmony checks whether two recent changes:

1. target the same file
2. come from different actors
3. touch overlapping lines
4. fall inside the configured overlap window

### Project-local memory

Harmony stores all of this in `.harmony\memory.db` inside the tracked project.

That means Harmony follows the project, not the editor installation.

---

## Current User-Facing Surfaces

### Zed slash commands

- `/harmony-pulse`
- `/harmony-sync`

### MCP tools

- `harmony_pulse`
- `report_file_edit`
- `report_change`
- `query_memory`
- `add_memory`
- `list_decisions`

### Native CLI

- `harmony-mcp serve`
- `harmony-mcp doctor`
- `harmony-mcp pulse`
- `harmony-mcp sync`

---

## What Harmony Does Well Right Now

- starts cleanly from Zed as a context server
- chooses the correct project-local database
- shows project/database status through Pulse
- records assistant-side edits through `report_file_edit` or `sync`
- detects real overlaps once both sides of the change are reported

---

## What Is Still In Progress

These ideas exist in the repository, but they are not the primary verified user path yet:

- live extension panels
- automatic ghost highlights in the editor
- zero-click assistant edit capture
- rich extension-side IPC-driven UI

The current docs and workflows should treat those as future-facing or scaffolded, not as the main shipped path.

---

## What Harmony Does Not Try To Be

- not a git replacement
- not a cloud sync service
- not a general-purpose chat app
- not a full autonomous swarm orchestrator

Harmony is a local collaboration safety layer for code edits.

---

## Repository Layout

| Path | Purpose |
|------|---------|
| `crates/harmony-core` | shared models and algorithms |
| `crates/harmony-memory` | SQLite store and memory retrieval |
| `crates/harmony-analyzer` | Tree-sitter and impact analysis |
| `crates/harmony-mcp` | native sidecar and CLI |
| `crates/harmony-extension` | Zed extension |
| `docs` | user and implementation docs |

---

*Last updated: 2026-04-07*
