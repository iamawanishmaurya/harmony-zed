# Harmony — Testing Guide

> Cross-platform testing workflow for the Harmony v0.1.1 negotiation backends and IPC

---

## Phase 1 — Unit Tests (both platforms, same commands)

```bash
# Run all 8 new negotiation backend tests
cargo test -p harmony-core --test negotiation_tests -- --nocapture

# Run the full existing suite to confirm nothing broke
cargo test -p harmony-core -p harmony-memory -p harmony-analyzer
```

Expected: **8 passed** for negotiation, **95 total** for everything.

---

## Phase 2 — Cross-Platform Build Check

### On Arch Linux (building for both targets)

```bash
# Native Arch build
cargo build --release -p harmony-mcp

# Cross-compile Windows binary from Arch
sudo pacman -S mingw-w64-gcc
rustup target add x86_64-pc-windows-gnu
cargo build --release -p harmony-mcp --target x86_64-pc-windows-gnu

# Confirm the Windows .exe was produced
ls -lh target/x86_64-pc-windows-gnu/release/harmony-mcp.exe
```

### On Windows (PowerShell)

```powershell
# Build the native sidecar
cargo build --release -p harmony-mcp

# Confirm binary exists
Get-Item .\target\release\harmony-mcp.exe
```

---

## Phase 3 — IPC Smoke Test (platform-specific)

### Arch Linux — Unix Socket

```bash
mkdir -p /tmp/test-project/.harmony
./target/release/harmony-mcp --db-path /tmp/test-project/.harmony/memory.db &
sleep 1

# Ping test
echo '{"cmd":"ping"}' | socat - UNIX-CONNECT:/tmp/test-project/.harmony/harmony.sock
# Expected: {"result":"pong"}
```

### Windows — TCP Fallback

```powershell
# Start sidecar
Start-Process .\target\release\harmony-mcp.exe -ArgumentList "--db-path C:\tmp\test\.harmony\memory.db"
Start-Sleep -Seconds 2

# Check it's listening on the right port
netstat -ano | findstr "17432"

# Ping via TCP using PowerShell
$tcp = New-Object System.Net.Sockets.TcpClient("127.0.0.1", 17432)
$stream = $tcp.GetStream()
$msg = '{"cmd":"ping"}'
$bytes = [System.Text.Encoding]::UTF8.GetBytes($msg)
$stream.Write($bytes, 0, $bytes.Length)
# Read response and print it
```

---

## Phase 4 — Backend Smoke Tests (real API keys)

For each backend, set the config in `.harmony/config.toml` and verify the negotiation pipeline.

### OpenAI

```toml
[negotiation]
negotiation_backend = "openai"
api_key = "sk-..."
model = "gpt-4o"
```

```bash
# Check the negotiation produced a memory note
sqlite3 .harmony/memory.db \
  "SELECT content FROM memory_records WHERE namespace='shared' ORDER BY created_at DESC LIMIT 1;"
```

### GitHub Copilot

```toml
[negotiation]
negotiation_backend = "openai"
api_key = "ghp_..."
model = "gpt-4o"
base_url = "https://api.githubcopilot.com"
```

> If you see a `401`, your token is missing the `copilot` scope — regenerate at github.com/settings/tokens.

### Anthropic Claude

```toml
[negotiation]
negotiation_backend = "anthropic"
api_key = "sk-ant-..."
model = "claude-sonnet-4-6"
```

### Ollama (Arch Linux, no API key needed)

```bash
# Install and start Ollama
sudo pacman -S ollama
ollama serve &
ollama pull llama3.3
```

```toml
[negotiation]
negotiation_backend = "openai"
api_key = "ollama"
model = "llama3.3"
base_url = "http://localhost:11434/v1"
```

### LM Studio (Windows)

Download from lmstudio.ai → load any model → enable local server → then:

```toml
[negotiation]
negotiation_backend = "openai"
api_key = "lm-studio"
model = "local-model"
base_url = "http://localhost:1234/v1"
```

**Pass criteria for each backend**: The negotiation returns a `proposed_diff` that is a valid unified diff and a non-empty `rationale` string.

---

## Phase 5 — Reliability Check (both platforms)

### Linux/macOS

```bash
for i in 1 2 3; do
  time cargo test -p harmony-core --test e2e_golden_path
  echo "--- Run $i done ---"
done
```

### Windows (PowerShell)

```powershell
1..3 | ForEach-Object {
  Measure-Command { cargo test -p harmony-core --test e2e_golden_path }
  Write-Host "--- Run $_ done ---"
}
```

---

## Troubleshooting Quick Reference

| Symptom | Likely Cause | Fix |
|---------|-------------|-----|
| `NegotiationNotConfigured` | Backend key missing or typo | Check `.harmony/config.toml` spelling |
| `401 Unauthorized` from Copilot | Token missing `copilot` scope | Regenerate at github.com/settings/tokens |
| `Connection refused` on Windows | Sidecar not writing `.harmony/harmony.port` | Check `#[cfg(target_os = "windows")]` compiled |
| `NegotiationInvalidResponse` | LLM returned markdown not JSON | Template B already has `"Respond ONLY with valid JSON"` |
| Anthropic `401` | Missing header | Both `x-api-key` and `anthropic-version: 2023-06-01` required |
| Ollama timeout | Model not pulled | Run `ollama pull llama3.3` first |

---

*Harmony v0.1.1 — Awanish Maurya · XPWNIT LAB · April 2026*
