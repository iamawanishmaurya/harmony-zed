# Harmony - Implementation Overview

> Current repository status as of 2026-04-07.

---

## Current Status

The repository is in a working state for the core Zed + MCP workflow:

- `harmony-mcp` builds and serves MCP over stdio
- the Zed extension can configure and launch the Harmony context server
- project-local `.harmony\memory.db` selection works
- `harmony_pulse` works from MCP tool calls
- `/harmony-pulse` works from the extension
- `report_file_edit` works for agent-side edit reporting
- `/harmony-sync` and `harmony-mcp sync` provide the current extension-side edit sync path

Latest full automated run:

- **118 passed**
- **0 failed**
- **1 ignored** (`live_github_models_openai_smoke`)

Command used:

```powershell
cargo test -p harmony-core -p harmony-memory -p harmony-analyzer -p harmony-mcp -p harmony-extension
```

---

## What Is Fully Working

### Sidecar and MCP

- `harmony-mcp serve` starts the stdio MCP server
- `harmony-mcp doctor` verifies project setup and paths
- `harmony-mcp pulse` prints a one-shot Harmony summary
- `harmony-mcp sync` registers recent or explicit files as assistant edits
- the stdio transport handles both Content-Length framing and raw JSON-line requests

### Zed integration

- install the dev extension from `crates/harmony-extension`
- configure the `harmony-memory` context server from Zed
- let Zed launch `harmony-mcp` automatically
- run `/harmony-pulse`
- run `/harmony-sync` after assistant file edits

### Edit reporting

Harmony now supports three practical reporting paths:

1. `report_file_edit`
2. `report_change`
3. `sync` / `/harmony-sync`

The shared recording logic lives in `crates/harmony-mcp/src/tracking.rs`, so these paths behave consistently.

---

## What Is Not Fully Wired Yet

These pieces are still scaffolded or partial:

- automatic zero-click capture of assistant file edits inside Zed
- visual panels backed by live extension state
- ghost highlights rendered in the real editor UI
- extension-side direct IPC usage for panels and live state updates

The repository still contains panel and sidecar-lifecycle scaffolding, but the current verified Zed flow is based on:

- context server launch
- slash commands
- MCP tools

---

## Crate Summary

| Crate | Role | Current notes |
|------|------|---------------|
| `harmony-core` | Shared models and logic | overlap detection, shadow diffs, negotiation, config, sandbox |
| `harmony-memory` | SQLite persistence | provenance, agents, overlaps, memory storage, embeddings |
| `harmony-analyzer` | Code analysis | Tree-sitter parsing and impact analysis |
| `harmony-mcp` | Native sidecar | MCP stdio server, CLI commands, tool routing, sync logic |
| `harmony-extension` | Zed extension | context server config plus `/harmony-pulse` and `/harmony-sync` |

---

## Key Files

### `harmony-mcp`

| File | Purpose |
|------|---------|
| `crates/harmony-mcp/src/main.rs` | CLI parsing and `serve`, `doctor`, `pulse`, `sync` subcommands |
| `crates/harmony-mcp/src/transport.rs` | MCP stdio transport |
| `crates/harmony-mcp/src/tools.rs` | MCP tools including `harmony_pulse`, `report_file_edit`, and `report_change` |
| `crates/harmony-mcp/src/tracking.rs` | shared change-recording logic used by tools and sync |
| `crates/harmony-mcp/src/types.rs` | JSON-RPC types |

### `harmony-extension`

| File | Purpose |
|------|---------|
| `crates/harmony-extension/src/lib.rs` | Zed extension entry, slash commands, context server config |
| `crates/harmony-extension/extension.toml` | extension manifest with context server and slash commands |
| `crates/harmony-extension/src/ipc.rs` | placeholder IPC abstractions for future extension-side features |
| `crates/harmony-extension/src/panels.rs` | panel and highlight data scaffolding |

---

## Current Working Flow

```text
1. Open a project in Zed
2. Configure the Harmony context server
3. Zed launches harmony-mcp over stdio
4. Run harmony_pulse or /harmony-pulse
5. Ask the assistant to edit a file
6. Run /harmony-sync
7. Harmony stores an agent provenance tag
8. Run harmony_pulse again
9. Registered agents and overlaps now reflect the synced edit history
```

If you need raw tool-level reporting instead of slash-command sync:

```text
assistant edit -> report_file_edit -> harmony_pulse
```

---

## Tool Surface

Current MCP tools:

- `harmony_pulse`
- `report_file_edit`
- `query_memory`
- `add_memory`
- `report_change`
- `list_decisions`

Current extension slash commands:

- `/harmony-pulse`
- `/harmony-sync`

---

## Build and Verification Commands

```powershell
# Run the full suite
cargo test -p harmony-core -p harmony-memory -p harmony-analyzer -p harmony-mcp -p harmony-extension

# Build the sidecar
cargo build --release -p harmony-mcp

# Build the extension
cargo build --release -p harmony-extension --target wasm32-wasip2

# Verify a project database
.\target\release\harmony-mcp.exe doctor --db-path C:\path\to\project\.harmony\memory.db

# Check one-shot project status
.\target\release\harmony-mcp.exe pulse --db-path C:\path\to\project\.harmony\memory.db

# Sync recent assistant edits into Harmony
.\target\release\harmony-mcp.exe sync --db-path C:\path\to\project\.harmony\memory.db
```

---

## Manual Zed Verification That Has Been Completed

The following workflow has been exercised successfully on Windows:

1. install the dev extension from `crates/harmony-extension`
2. configure Harmony from Zed
3. connect the MCP context server
4. run `harmony_pulse`
5. point Harmony at a project-local `.harmony\memory.db`
6. record assistant edits
7. surface those edits through `harmony_pulse`

---

## Known Gaps

- The extension does not yet automatically observe assistant-applied edits.
- `/harmony-sync` is the current extension-side workaround for that missing event hook.
- Some extension modules are still placeholder-heavy and generate warnings during builds.

Those warnings are non-fatal, but the docs should not treat those placeholder modules as fully user-visible features yet.

---

*Last updated: 2026-04-07*
