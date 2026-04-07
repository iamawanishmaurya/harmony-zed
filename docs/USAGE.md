# Harmony - User Guide

> Current, verified workflow for running Harmony with Zed and a project-local `.harmony` database.

---

## Quick Start

### Platform note

- On Windows, use the PowerShell commands in this document.
- On Linux, use the same flow with the binary at `./target/release/harmony-mcp` and forward-slash paths such as `/path/to/project/.harmony/memory.db`.

### 1. Build the native sidecar

```powershell
cargo build --release -p harmony-mcp
```

Linux:

```bash
cargo build --release -p harmony-mcp
```

### 2. Install the dev extension in Zed

Use `zed: install dev extension` and select:

```text
<repo>\crates\harmony-extension
```

Do not select the workspace root.

### 3. Open the project you want Harmony to track

Harmony stores data in that project's own `.harmony` folder:

```text
your-project/
  .harmony/
    config.toml
    memory.db
    mcp-debug.log
    agent-sync-state.json
    network-sync-state.json
```

### 4. Configure Harmony in Zed

Open the Harmony context server and click `Configure Server`.

Zed starts `harmony-mcp` itself. You do not need to run a separate `.bat` file for normal editor use.

If `[network].auto_sync = true`, Harmony also keeps project files and folders synchronized across connected host/client laptops through the host server.

### 5. Verify the connection

In a tool-enabled Agent chat, run:

```text
run the harmony_pulse tool
```

Or use the slash command:

```text
/harmony-pulse
```

### 6. After assistant file edits, sync them into Harmony

Use:

```text
/harmony-sync
```

If you want to sync one file explicitly:

```text
/harmony-sync path/to/file
```

Then run `harmony_pulse` or `/harmony-pulse` again.

### 7. Watch the shared file timeline

Open the Harmony dashboard and use the new `Files` section to verify:

- newly created files
- newly created folders
- updated shared files
- deleted shared entries

Each file activity card now includes a short impact summary explaining how that change affects the shared project.

---

## What Works Today

Harmony currently supports these verified workflows:

- Zed context-server startup through `Configure Server`
- Project-local `.harmony\memory.db` selection based on the open project
- MCP tool `harmony_pulse`
- MCP tool `report_file_edit`
- Raw MCP tool `report_change`
- Slash commands `/harmony-pulse` and `/harmony-sync`
- Native CLI commands `doctor`, `pulse`, `sync`, and default `serve`
- Automatic host/client project file replication for created folders and files
- Dashboard `Files` timeline with project-impact summaries

The current Zed extension does not yet have a true "assistant just edited this file" event hook, so `/harmony-sync` is the reliable extension-side bridge after assistant edits.

---

## Prerequisites

| Tool | Notes |
|------|-------|
| Rust | Required to build `harmony-mcp` and the Zed extension |
| `wasm32-wasip2` target | Required for `harmony-extension` builds |
| Zed | Required for the extension workflow |

Install the WASM target once:

```powershell
rustup target add wasm32-wasip2
```

---

## Build Commands

### Native sidecar

```powershell
cargo build --release -p harmony-mcp
```

### Extension WASM

```powershell
cargo build --release -p harmony-extension --target wasm32-wasip2
```

### Verify the sidecar outside Zed

```powershell
.\target\release\harmony-mcp.exe doctor --db-path C:\path\to\project\.harmony\memory.db
```

Linux:

```bash
./target/release/harmony-mcp doctor --db-path /path/to/project/.harmony/memory.db
```

### Print a one-shot project status

```powershell
.\target\release\harmony-mcp.exe pulse --db-path C:\path\to\project\.harmony\memory.db
```

Linux:

```bash
./target/release/harmony-mcp pulse --db-path /path/to/project/.harmony/memory.db
```

### Sync recent or explicit files from the CLI

```powershell
.\target\release\harmony-mcp.exe sync --db-path C:\path\to\project\.harmony\memory.db
```

Linux:

```bash
./target/release/harmony-mcp sync --db-path /path/to/project/.harmony/memory.db
```

Explicit file sync:

```powershell
.\target\release\harmony-mcp.exe sync --db-path C:\path\to\project\.harmony\memory.db --file src\app.ts --actor-id agent:zed-assistant
```

---

## Harmony Files

Harmony creates and uses these project-local files:

| File | Purpose |
|------|---------|
| `.harmony/config.toml` | Project configuration |
| `.harmony/memory.db` | SQLite database with provenance, agents, overlaps, and memory |
| `.harmony/mcp-debug.log` | Sidecar debug log |
| `.harmony/agent-sync-state.json` | Tracks which files were already synced by `/harmony-sync` or `harmony-mcp sync` |
| `.harmony/network-sync-state.json` | Tracks automatic cross-laptop file replication state |

---

## MCP Tools

| Tool | Purpose |
|------|---------|
| `harmony_pulse` | Show project, database, registered agents, and pending overlaps |
| `report_file_edit` | Record a file edit without requiring a full handcrafted diff |
| `report_change` | Record a raw diff and line range directly |
| `query_memory` | Query shared memory |
| `add_memory` | Store a shared memory record |
| `list_decisions` | List stored decision records |

### When to use each edit-reporting tool

- Use `report_file_edit` when an agent changed a file and you want Harmony to track it quickly.
- Use `report_change` when you already have the exact unified diff and line range.
- Use `/harmony-sync` when you are inside Zed and want the extension to sweep recent assistant edits into Harmony.

---

## Slash Commands

| Slash command | Purpose |
|---------------|---------|
| `/harmony-pulse` | Run a one-shot status check through the native sidecar |
| `/harmony-sync` | Sync recently modified project files into Harmony as assistant edits |
| `/harmony-sync path/to/file` | Sync one explicit file |

### Important distinction

- `harmony_pulse` is the MCP tool used inside tool-enabled Agent chats.
- `/harmony-pulse` is the Zed slash command exposed by the extension.

They surface the same project status, but they are different integration surfaces.

---

## Common Zed Workflows

### Check that Harmony is connected

1. Open a project in Zed.
2. Configure the Harmony context server.
3. Run `harmony_pulse` in chat or `/harmony-pulse`.
4. Confirm the project path and database path point at the project you opened.

### Track an assistant edit

1. Ask the assistant to create or edit a file.
2. Run `/harmony-sync`.
3. Run `harmony_pulse`.
4. Confirm `Registered agents` is at least `1`.

### Verify automatic cross-laptop sync

1. Run one machine in `host` mode and one in `client` mode.
2. Create a new file or folder on either machine.
3. Wait a few seconds for the sync interval.
4. Confirm the same file or folder appears on the other machine.
5. Open the dashboard `Files` section and confirm the activity card appears there too.

### Create and detect an overlap

1. Let the assistant edit a file.
2. Run `/harmony-sync`.
3. Make a manual edit in the same file and same line area.
4. Record the human edit through your own integration or a raw `report_change` call.
5. Run `harmony_pulse`.
6. Confirm `Pending overlaps` increased.

---

## Example: `report_file_edit`

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "report_file_edit",
    "arguments": {
      "actor_id": "agent:copilot",
      "file_path": "src/app.ts",
      "content": "console.log('hello');\n",
      "task_prompt": "Created a small logging helper"
    }
  }
}
```

---

## Troubleshooting

| Problem | What to check |
|---------|---------------|
| `Failed to install dev extension` | Install from `crates/harmony-extension`, not the workspace root |
| `Context server request timeout` | Rebuild `harmony-mcp`, reopen Zed, and inspect `.harmony/mcp-debug.log` plus `C:\Users\water\AppData\Local\Zed\logs\Zed.log` |
| `harmony_pulse` points at the wrong project | Reopen the intended project folder and reconfigure the context server |
| `Registered agents: 0` after an assistant edit | Run `/harmony-sync` or call `report_file_edit` |
| `Pending overlaps: 0` after edits | Make sure both edits hit the same file and overlapping lines, and that both changes were reported into Harmony |
| Zed cannot build the extension | Run `rustup target add wasm32-wasip2` |

### Useful log commands

```powershell
Get-Content -Tail 120 .harmony\mcp-debug.log
Get-Content -Tail 120 C:\Users\water\AppData\Local\Zed\logs\Zed.log
```

Linux:

```bash
tail -n 120 .harmony/mcp-debug.log
tail -n 120 ~/.local/state/zed/logs/Zed.log
```

---

## Recommended Verification Sequence

```powershell
# 1. Run the full suite
cargo test -p harmony-core -p harmony-memory -p harmony-analyzer -p harmony-mcp -p harmony-extension

# 2. Build the sidecar
cargo build --release -p harmony-mcp

# 3. Build the extension
cargo build --release -p harmony-extension --target wasm32-wasip2

# 4. Verify a target project
.\target\release\harmony-mcp.exe doctor --db-path C:\path\to\project\.harmony\memory.db
```

---

## Security Notes

- Keep API keys out of git.
- Treat `.harmony/config.toml` as sensitive if it contains real credentials.
- Harmony data stays local unless you configure a remote negotiation backend.

---

*Last updated: 2026-04-07*
