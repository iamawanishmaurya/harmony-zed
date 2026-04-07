# Harmony

Harmony is a local-first Zed extension plus MCP sidecar for safer parallel human and AI development.

It records provenance for code changes, detects overlapping edits, exposes project status through MCP tools and slash commands, and serves a small dashboard for overlap review.

## What Is Verified In This Repo

As of 2026-04-07, this repository has been verified with:

- `cargo test -p harmony-core -p harmony-memory -p harmony-analyzer -p harmony-mcp -p harmony-extension`
- `cargo build --release -p harmony-mcp`
- `cargo build --release -p harmony-extension --target wasm32-wasip2`
- `powershell -NoProfile -ExecutionPolicy Bypass -File .\\smoke-network.ps1`

The one-laptop smoke test passed with:

- `BridgeSmoke: "ok"`
- `Pending overlaps: 1`
- `WebSocketEvents: 3`
- a resolved overlap status from the dashboard API

## Repository Layout

- `crates/harmony-mcp`: native MCP sidecar and network bridge
- `crates/harmony-extension`: Zed extension
- `crates/harmony-core`: shared domain logic
- `crates/harmony-memory`: SQLite-backed storage
- `dashboard/`: web dashboard served by `harmony-mcp`
- `docs/`: setup, usage, testing, and architecture notes

## Prerequisites

- Rust 1.80+
- Git
- Zed
- `wasm32-wasip2` target for the extension build

Install the WASM target once:

```bash
rustup target add wasm32-wasip2
```

## Build

### Windows

```powershell
git clone https://github.com/iamawanishmaurya/harmony-zed.git
cd harmony-zed

cargo build --release -p harmony-mcp
cargo build --release -p harmony-extension --target wasm32-wasip2
```

### Linux

```bash
git clone https://github.com/iamawanishmaurya/harmony-zed.git
cd harmony-zed

cargo build --release -p harmony-mcp
cargo build --release -p harmony-extension --target wasm32-wasip2
```

## Load The Extension In Zed

Use Zed's dev-extension installer and select:

```text
<repo>/crates/harmony-extension
```

Do not select the workspace root.

## Connect Harmony To A Project

1. Open the project you want Harmony to track in Zed.
2. Open the Harmony context server.
3. Click `Configure Server`.

Harmony creates a project-local `.harmony` folder when needed:

```text
your-project/
  .harmony/
    config.toml
    memory.db
    mcp-debug.log
    agent-sync-state.json
```

Zed starts `harmony-mcp` itself for normal use. You do not need to keep a separate terminal open for the editor workflow.

## Verify It Is Working

### In Zed

In a tool-enabled Agent chat, run:

```text
run the harmony_pulse tool
```

Or use the slash command:

```text
/harmony-pulse
```

Success looks like:

- the project path matches the open folder
- the database path points at `<project>/.harmony/memory.db`
- Harmony responds without a timeout

### After Assistant Edits

The current Zed extension does not yet have a direct assistant-edit event hook, so the reliable workflow is:

```text
/harmony-sync
```

Or for one file:

```text
/harmony-sync path/to/file
```

Then run `harmony_pulse` again.

### CLI Verification

#### Windows

```powershell
.\target\release\harmony-mcp.exe doctor --db-path C:\path\to\project\.harmony\memory.db
.\target\release\harmony-mcp.exe pulse --db-path C:\path\to\project\.harmony\memory.db
.\target\release\harmony-mcp.exe sync --db-path C:\path\to\project\.harmony\memory.db
```

#### Linux

```bash
./target/release/harmony-mcp doctor --db-path /path/to/project/.harmony/memory.db
./target/release/harmony-mcp pulse --db-path /path/to/project/.harmony/memory.db
./target/release/harmony-mcp sync --db-path /path/to/project/.harmony/memory.db
```

## One-Laptop Verification

### Windows

```powershell
cargo build --release -p harmony-mcp
powershell -NoProfile -ExecutionPolicy Bypass -File .\smoke-network.ps1
```

Treat the smoke test as successful when the JSON output includes:

- `BridgeSmoke: "ok"`
- `Pending overlaps: 1` inside `Pulse`
- `WebSocketEvents` greater than `0`
- a non-pending `ResolvedStatus`

### Linux

The repo currently ships the smoke test as `smoke-network.ps1`. On Linux, either:

- run it with PowerShell 7 (`pwsh`) if available
- or use the manual verification flow in [docs/TESTING.md](./docs/TESTING.md)

## Current User-Facing Surfaces

- MCP tool `harmony_pulse`
- MCP tool `harmony_sync`
- MCP tool `harmony_dashboard`
- MCP tool `report_file_edit`
- slash command `/harmony-pulse`
- slash command `/harmony-sync`

## Docs

- [docs/USAGE.md](./docs/USAGE.md)
- [docs/TESTING.md](./docs/TESTING.md)
- [docs/OVERVIEW.md](./docs/OVERVIEW.md)
- [docs/PROJECT.md](./docs/PROJECT.md)

## Notes

- Some scaffold modules still emit warnings, but they do not block the verified build, connect, sync, or smoke-test flow.
- Automatic human-edit capture is not fully wired yet, so overlap tests still require both sides to be reported into Harmony.

## License

MIT
