# Harmony - Testing Guide

> Verified test and smoke-check workflow for the current Harmony repository.

---

## Automated Test Baseline

### Platform note

- On Windows, use the PowerShell examples as written.
- On Linux, the same commands work in a shell, except the binary path is `./target/release/harmony-mcp` instead of `.\target\release\harmony-mcp.exe`.

Run the full suite:

```powershell
cargo test -p harmony-core -p harmony-memory -p harmony-analyzer -p harmony-mcp -p harmony-extension
```

Latest verified result on 2026-04-07:

- **118 passed**
- **0 failed**
- **1 ignored**

The ignored test is the live GitHub Models smoke test, which requires real credentials.

---

## Release Build Verification

### Build the native sidecar

```powershell
cargo build --release -p harmony-mcp
```

Linux:

```bash
cargo build --release -p harmony-mcp
```

### Build the Zed extension

```powershell
rustup target add wasm32-wasip2
cargo build --release -p harmony-extension --target wasm32-wasip2
```

Linux:

```bash
rustup target add wasm32-wasip2
cargo build --release -p harmony-extension --target wasm32-wasip2
```

---

## CLI Smoke Tests

### 1. Doctor

```powershell
.\target\release\harmony-mcp.exe doctor --db-path C:\path\to\project\.harmony\memory.db
```

Linux:

```bash
./target/release/harmony-mcp doctor --db-path /path/to/project/.harmony/memory.db
```

Expected:

- project path printed
- database path printed
- config path printed
- `Status: ready`

### 2. Pulse

```powershell
.\target\release\harmony-mcp.exe pulse --db-path C:\path\to\project\.harmony\memory.db
```

Linux:

```bash
./target/release/harmony-mcp pulse --db-path /path/to/project/.harmony/memory.db
```

Expected:

- project path
- database path
- registered agents count
- pending overlaps count

### 3. Sync

```powershell
.\target\release\harmony-mcp.exe sync --db-path C:\path\to\project\.harmony\memory.db
```

Linux:

```bash
./target/release/harmony-mcp sync --db-path /path/to/project/.harmony/memory.db
```

Expected:

- `Harmony Sync`
- scanned file count
- synced file count
- overlap count

Explicit single-file sync:

```powershell
.\target\release\harmony-mcp.exe sync --db-path C:\path\to\project\.harmony\memory.db --file src\app.ts --actor-id agent:zed-assistant
```

---

## Raw MCP Transport Smoke Test

This verifies the stdio MCP transport directly.

```powershell
@'
{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"manual","version":"1.0.0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized","params":null}
{"jsonrpc":"2.0","id":1,"method":"tools/list"}
'@ | .\target\release\harmony-mcp.exe --db-path C:\path\to\project\.harmony\memory.db
```

Expected:

- successful `initialize` response
- tool list containing `harmony_pulse`
- tool list containing `report_file_edit`

---

## Zed Manual Verification

### Install and connect

1. Build `harmony-mcp`.
2. Open Zed.
3. Run `zed: install dev extension`.
4. Select `<repo>\crates\harmony-extension`.
5. Open the project you want Harmony to track.
6. Configure the `harmony-memory` context server.

Expected:

- Harmony config panel opens
- `Configure Server` succeeds
- no timeout

### Pulse check

In a tool-enabled Agent chat:

```text
run the harmony_pulse tool
```

Or run:

```text
/harmony-pulse
```

Expected:

- project path matches the open project
- database path matches `<project>\.harmony\memory.db`

### Assistant edit sync check

1. Ask the assistant to create or edit a file.
2. Run `/harmony-sync`.
3. Run `harmony_pulse` again.

Expected:

- `Registered agents` is at least `1`
- the synced file appears in the sync output

### Overlap check

1. Sync an assistant edit for a file.
2. Record a human edit in the same file and overlapping line range.
3. Run `harmony_pulse`.

Expected:

- `Pending overlaps` increases
- Pulse shows the overlapping file and actors

### Linux manual verification notes

- Install the same dev extension from `crates/harmony-extension`.
- Use the same `Configure Server` flow in Zed.
- If you want CLI confirmation outside Zed, use the Linux commands above against the project's `.harmony/memory.db`.

---

## Targeted Test Commands

### `harmony-mcp`

```powershell
cargo test -p harmony-mcp
```

This includes:

- transport parsing tests
- `harmony_pulse` tool tests
- `report_file_edit` tests
- `sync` CLI tests

### `harmony-extension`

```powershell
cargo test -p harmony-extension
```

These are currently focused on extension-side helper logic and panel data structures.

---

## Logs to Inspect

### Harmony sidecar log

```powershell
Get-Content -Tail 120 .harmony\mcp-debug.log
```

Linux:

```bash
tail -n 120 .harmony/mcp-debug.log
```

### Zed log

```powershell
Get-Content -Tail 120 C:\Users\water\AppData\Local\Zed\logs\Zed.log
```

Linux:

```bash
tail -n 120 ~/.local/state/zed/logs/Zed.log
```

---

## Known Testing Limits

- There is no automated GUI test suite for Zed itself in this repo.
- Automatic assistant-edit capture is not implemented yet, so `/harmony-sync` is part of the verified workflow.
- Some extension scaffolding modules still generate warnings, but they do not block the current build or sync flow.

---

## Quick Troubleshooting Reference

| Symptom | Likely cause | Fix |
|---------|--------------|-----|
| Dev extension fails to install | wrong folder selected | install from `crates/harmony-extension` |
| Context server configure times out | stale or broken sidecar binary | rebuild `harmony-mcp`, reopen Zed, inspect logs |
| Pulse shows wrong project | Harmony attached to the wrong open folder | reopen the correct project and reconfigure |
| Agent count stays zero after assistant edit | edit was not reported | run `/harmony-sync` |
| Overlap count stays zero | changes were not overlapping or not both reported | confirm same file, same lines, and reported changes |

---

*Last updated: 2026-04-07*
