# Harmony — User Guide

> How to set up, use, and troubleshoot Harmony for your projects.

---

## Table of Contents

1. [Quick Start](#quick-start)
2. [Setup](#setup)
3. [Configuration](#configuration)
4. [Using Harmony](#using-harmony)
5. [LLM Backend Setup](#llm-backend-setup)
6. [Common Workflows](#common-workflows)
7. [Troubleshooting](#troubleshooting)
8. [How to Report a Bug to Me](#how-to-report-a-bug-to-me)

---

## Quick Start

```bash
# 1. Clone and build
git clone <your-repo-url>
cd Harmony
cargo build --release -p harmony-mcp

# 2. Run in any project
cd /path/to/your/project
/path/to/harmony-mcp --db-path .harmony/memory.db

# 3. A config file is auto-created at .harmony/config.toml
#    Edit it to set your preferred LLM backend
```

That's it. Harmony is now tracking changes in your project.

---

## Setup

### Prerequisites

| Tool | Version | Install |
|------|---------|---------|
| **Rust** | 1.80+ | `winget install Rustlang.Rust.MSVC` (Windows) or `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` (Linux) |
| **Git** | any | Already installed |
| **SQLite** | bundled | Comes with `rusqlite` — no install needed |

### Build from Source

```bash
# Clone
git clone <repo-url> Harmony
cd Harmony

# Build the sidecar binary (the only thing you need to run)
cargo build --release -p harmony-mcp

# The binary is at:
#   Windows: target\release\harmony-mcp.exe
#   Linux:   target/release/harmony-mcp
```

### Verify Installation

```bash
# Run all tests to make sure everything works
cargo test -p harmony-core -p harmony-memory -p harmony-analyzer

# Expected: 95 passed, 0 failed
```

---

## Configuration

### Auto-Generated Config

The first time Harmony runs in a project, it creates `.harmony/config.toml` with sensible defaults and commented examples for every LLM backend.

```
your-project/
├── .harmony/
│   ├── config.toml    ← Your settings (auto-created)
│   ├── memory.db      ← SQLite database (auto-created)
│   ├── harmony.port   ← TCP port file (Windows only)
│   └── harmony.sock   ← Unix socket (Linux/macOS only)
├── src/
└── ...
```

### Key Settings

Open `.harmony/config.toml` and edit these sections:

#### Your Identity

```toml
[human]
username = "awanish"           # Your display name
actor_id = "human:awanish"     # Used in provenance tags
```

#### Overlap Detection Window

```toml
[general]
overlap_window_minutes = 30    # How far back to check for conflicts
```

#### LLM Backend (most important)

```toml
[negotiation]
negotiation_backend = "agent"  # Change to "openai", "anthropic", or "disabled"
# See "LLM Backend Setup" section below for full examples
```

#### Ghost Highlight Colors

```toml
[ui]
ghost_add_color = "#7ee8a280"      # Green glow for agent additions
ghost_remove_color = "#f0606060"   # Red glow for agent removals
```

---

## Using Harmony

### What Harmony Tracks

Every code change made by any participant (human or AI agent) is stored as a **provenance tag** containing:
- **Who** changed it (e.g., `human:awanish` or `agent:architect-01`)
- **What** changed (unified diff)
- **Where** (file path + line range)
- **When** (timestamp)
- **Why** (the task prompt that motivated the change)

### What Happens When Changes Overlap

```
You edit auth.ts lines 10–25        Agent edits auth.ts lines 15–30
         │                                    │
         └────────── OVERLAP! ────────────────┘
                      │
              Harmony detects it
                      │
         ┌────────────┼────────────┐
         │            │            │
    [Accept Mine] [Accept Theirs] [Negotiate ✨]
```

1. **Accept Mine** — Keep your changes, discard the agent's
2. **Accept Theirs** — Keep the agent's changes, discard yours
3. **Negotiate** — Send both diffs to the LLM, get a merged version

### MCP Tools Available

The sidecar exposes these tools via JSON-RPC:

| Tool | What It Does |
|------|-------------|
| `report_change` | Record a new code change (triggers overlap detection) |
| `query_memory` | Search team memory by keyword/semantic similarity |
| `add_memory` | Add a note to team memory |
| `list_decisions` | List past negotiation decisions |

### Example: Report a Change

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "report_change",
    "arguments": {
      "actor_id": "human:awanish",
      "file_path": "src/auth.ts",
      "start_line": 10,
      "end_line": 25,
      "diff": "@@ -10,5 +10,8 @@\n+const validated = true;"
    }
  }
}
```

### Example: Query Team Memory

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/call",
  "params": {
    "name": "query_memory",
    "arguments": {
      "query": "why was Redis rejected?"
    }
  }
}
```

---

## LLM Backend Setup

### Option 1: GitHub Models (Recommended for Testing)

Free with a GitHub PAT. No credit card needed.

```toml
[negotiation]
negotiation_backend = "openai"
api_key = "github_pat_..."
model = "gpt-4o-mini"
base_url = "https://models.inference.ai.azure.com"
```

**Setup:**
1. Go to [github.com/settings/tokens](https://github.com/settings/tokens)
2. Create a fine-grained PAT with AI model access
3. Set it as an env var: `$env:HARMONY_GITHUB_TOKEN = "github_pat_..."`
4. Or paste it directly in `config.toml` (less secure)

### Option 2: OpenAI

```toml
[negotiation]
negotiation_backend = "openai"
api_key = "sk-..."
model = "gpt-4o"
base_url = "https://api.openai.com/v1"
```

### Option 3: Anthropic Claude

```toml
[negotiation]
negotiation_backend = "anthropic"
api_key = "sk-ant-..."
model = "claude-sonnet-4-6"
```

### Option 4: Ollama (100% Local, Free)

```bash
# Install Ollama
# Windows: winget install Ollama.Ollama
# Linux:   curl -fsSL https://ollama.ai/install.sh | sh

# Pull a model
ollama pull llama3.3

# Start the server
ollama serve
```

```toml
[negotiation]
negotiation_backend = "openai"
api_key = "ollama"
model = "llama3.3"
base_url = "http://localhost:11434/v1"
```

### Option 5: LM Studio (Local, GUI)

1. Download from [lmstudio.ai](https://lmstudio.ai)
2. Load any model → enable "Local Server"
3. Configure:

```toml
[negotiation]
negotiation_backend = "openai"
api_key = "lm-studio"
model = "local-model"
base_url = "http://localhost:1234/v1"
```

### Option 6: Agent Delegation (Default)

No API key needed. Delegates negotiation to a locally registered ACP agent.

```toml
[negotiation]
negotiation_backend = "agent"

[agents]
[[agents.registry]]
name = "opencode"
endpoint = "http://localhost:4231"
```

### Option 7: Disabled

```toml
[negotiation]
negotiation_backend = "disabled"
```

Overlaps are still detected, but the "Negotiate" button does nothing.

---

## Common Workflows

### Workflow 1: Check That Harmony Is Working

```bash
# Run all tests
cargo test -p harmony-core -p harmony-memory -p harmony-analyzer
# Expected: 95 passed

# Run the E2E golden path test
cargo test -p harmony-core --test e2e_golden_path -- --nocapture
# Expected: 5 passed

# Run a live API test (needs HARMONY_GITHUB_TOKEN env var)
cargo test -p harmony-core --test live_backend_smoke -- --ignored --nocapture
# Expected: 1 passed, valid NegotiationResult printed
```

### Workflow 2: Check Config Is Valid

```bash
# This test creates a temp config and verifies it loads correctly
cargo test -p harmony-core -- test_load_creates_default --nocapture
```

### Workflow 3: View the Auto-Generated Config

```bash
# Delete existing config to regenerate
rm .harmony/config.toml

# Run harmony-mcp — it will auto-create the config
./target/release/harmony-mcp --db-path .harmony/memory.db

# Check the generated file
cat .harmony/config.toml
```

### Workflow 4: Check Memory Database

```bash
# View all stored provenance tags
sqlite3 .harmony/memory.db "SELECT actor_id, file_path, datetime(timestamp) FROM provenance_tags ORDER BY timestamp DESC LIMIT 10;"

# View all memory notes
sqlite3 .harmony/memory.db "SELECT namespace, content, datetime(created_at) FROM memory_records ORDER BY created_at DESC LIMIT 10;"

# View detected overlaps
sqlite3 .harmony/memory.db "SELECT file_path, status, datetime(detected_at) FROM overlap_events ORDER BY detected_at DESC LIMIT 10;"
```

---

## Troubleshooting

### Build Issues

| Problem | Solution |
|---------|----------|
| `cargo build` fails with missing target | Run `rustup target add wasm32-wasip2` for extension builds |
| Linker error on Windows | Install Visual Studio Build Tools: `winget install Microsoft.VisualStudio.2022.BuildTools` |
| `rusqlite` linking error | SQLite is bundled via `bundled` feature — should just work. If not, install `libsqlite3-dev` on Linux |

### Config Issues

| Problem | Solution |
|---------|----------|
| Config not found | Harmony auto-creates `.harmony/config.toml` on first run |
| Config parse error | Check for TOML syntax errors. Run `toml-lint .harmony/config.toml` or paste contents into [toml-lint.com](https://www.toml-lint.com) |
| Wrong username | Edit `[human].username` in `.harmony/config.toml` |

### LLM Backend Issues

| Problem | Solution |
|---------|----------|
| `NegotiationNotConfigured` | The `negotiation_backend` key is missing, misspelled, or set to `"disabled"` |
| `401 Unauthorized` (OpenAI) | Your `api_key` is invalid or expired. Regenerate at [platform.openai.com/api-keys](https://platform.openai.com/api-keys) |
| `401 Unauthorized` (Anthropic) | Missing `anthropic-version` header. This is a code bug — report it |
| `401 Unauthorized` (GitHub) | Your PAT doesn't have AI model access. Regenerate at [github.com/settings/tokens](https://github.com/settings/tokens) |
| `NegotiationInvalidResponse` | The LLM returned non-JSON output. Try a different model (gpt-4o is more reliable than gpt-3.5) |
| `Connection refused` (Ollama) | Ollama isn't running. Start it with `ollama serve` |
| `Connection refused` (LM Studio) | LM Studio's local server isn't enabled. Click "Start Server" in the UI |
| Timeout | The model is too slow. Try `gpt-4o-mini` instead of `gpt-4o`, or use a smaller Ollama model |

### IPC Issues (Sidecar Connection)

| Problem | Solution |
|---------|----------|
| Extension can't find sidecar (Windows) | Check `.harmony/harmony.port` exists and contains `127.0.0.1:17432` |
| Extension can't find sidecar (Linux) | Check `.harmony/harmony.sock` exists: `ls -la .harmony/harmony.sock` |
| Port 17432 already in use | Another instance of harmony-mcp is running. Kill it: `taskkill /IM harmony-mcp.exe /F` (Windows) or `pkill harmony-mcp` (Linux) |

---

## How to Report a Bug to Me

When you hit an issue and want me to help debug it, copy-paste this template into your message:

---

### 🐛 Bug Report Template

```
**What I was trying to do:**
[Describe the action, e.g., "Run the live backend smoke test with Ollama"]

**What happened:**
[Describe the error, e.g., "Got NegotiationInvalidResponse"]

**Error output (copy-paste the full output):**
```
[Paste the FULL terminal output here, including any error messages]
```

**My config (paste .harmony/config.toml negotiation section):**
```toml
[negotiation]
negotiation_backend = "..."
api_key = "REDACTED"
model = "..."
base_url = "..."
```

**My platform:**
- OS: [Windows 11 / Arch Linux / macOS]
- Rust version: [output of `rustc --version`]

**Commands I ran:**
```bash
[Paste the exact commands you ran]
```
```

---

### Quick Debug Commands (run these before reporting)

These commands help me understand the state of your system:

```bash
# 1. Check Rust version
rustc --version

# 2. Check if tests still pass
cargo test -p harmony-core --test negotiation_tests 2>&1 | Select-String "test result"

# 3. Check if the binary builds
cargo build -p harmony-mcp 2>&1 | Select-String "error|Finished"

# 4. Check your config loads properly
cargo test -p harmony-core -- test_default_config --nocapture 2>&1 | Select-String "test |ok|FAILED"

# 5. Check memory database integrity
sqlite3 .harmony/memory.db "PRAGMA integrity_check;"
# Expected: "ok"

# 6. Show your config (REDACT api_key before sharing!)
Get-Content .harmony\config.toml | Select-String -NotMatch "api_key"

# 7. Check if the LLM endpoint is reachable
# For GitHub Models:
Invoke-WebRequest -Uri "https://models.inference.ai.azure.com" -Method HEAD
# For Ollama:
Invoke-WebRequest -Uri "http://localhost:11434/api/version" -Method GET
```

### What I Need to See

When you paste a bug report, I will:

1. **Read the error message** to identify the error variant (`NegotiationInvalidResponse`, `NegotiationNotConfigured`, etc.)
2. **Check your config** to see if the backend/api_key/base_url are correct
3. **Run the relevant test** to reproduce the issue
4. **Look at the specific code path** that produced the error
5. **Fix and verify** with a test

### Example Bug Report

```
**What I was trying to do:**
Run the live backend test with Ollama locally

**What happened:**
Test passed but the proposed_diff was empty

**Error output:**
✅ SUCCESS!
  proposed_diff: 0 chars
  rationale: I cannot produce a diff without seeing the actual code.
  confidence: 0.3
  memory_notes: []

**My config:**
[negotiation]
negotiation_backend = "openai"
api_key = "ollama"
model = "llama3.2:1b"
base_url = "http://localhost:11434/v1"

**My platform:**
- OS: Arch Linux
- Rust: rustc 1.82.0
```

**My diagnosis for this example:** The model (`llama3.2:1b`) is too small to produce valid JSON diffs. Switch to `llama3.3` or `codellama:13b`.

---

## File Reference

| File | What It Is |
|------|-----------|
| `.harmony/config.toml` | Your project settings (human identity, LLM backend, UI colors) |
| `.harmony/memory.db` | SQLite database with all provenance tags, memory notes, overlaps |
| `.harmony/harmony.port` | TCP port the sidecar is listening on (Windows only) |
| `.harmony/harmony.sock` | Unix socket the sidecar is listening on (Linux/macOS only) |
| `target/release/harmony-mcp` | The compiled sidecar binary |
| `docs/PROJECT.md` | Architecture and key concepts |
| `docs/OVERVIEW.md` | Implementation status and module descriptions |
| `docs/TESTING.md` | Testing workflow (5 phases) |
| `docs/USAGE.md` | This file |

---

## Security Notes

- **Never share API keys in chat.** Use environment variables: `$env:MY_KEY = "sk-..."`
- **Never commit `.harmony/config.toml` if it contains real API keys.** Add `.harmony/` to `.gitignore`
- **Revoke any token that's been exposed** (even in an AI conversation)
- **The memory database is local.** Nothing is sent to the cloud unless you configure an LLM backend

### Recommended .gitignore Addition

```gitignore
# Harmony
.harmony/
```

---

*Harmony v0.1.1 — Awanish Maurya · XPWNIT LAB · April 2026*
