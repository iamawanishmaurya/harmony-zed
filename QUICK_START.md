# Harmony Extension - Quick Start Guide

## ✅ Status: Ready to Load in Zed

All components have been successfully built and configured.

```
Test Results:
✓ Rust toolchain: 1.94.1 (WASM support enabled)
✓ MCP sidecar: harmony-mcp.exe (2.2 MB) - RUNNING
✓ WASM extension: harmony_extension.wasm (113 KB)
✓ Configuration: .harmony/config.toml
✓ Database: .harmony/memory.db (initialized)
```

---

## 🚀 Loading the Extension in Zed

### Step 1: Open Zed
```bash
# Make sure Zed is running
zed
```

### Step 2: Open Command Palette
Press: **`Ctrl+Shift+P`** (Windows/Linux) or **`Cmd+Shift+P`** (macOS)

### Step 3: Install Dev Extension
1. Type: `install dev extension`
2. Select from the dropdown: `zed: install dev extension`
3. Press `Enter`

### Step 4: Select Extension Directory
A file browser will appear. Navigate to and select:
```
C:\Users\water\Desktop\Testing\mmcp-windows\Harmony\crates\harmony-extension
```

**Important:** Select the `harmony-extension` subdirectory, NOT the workspace root.

### Step 5: Confirm Installation
Zed will:
1. Compile the WASM extension
2. Mount it in the editor runtime
3. Connect to the MCP sidecar
4. Show "Harmony" in the activity bar (left sidebar)

---

## ✨ Features Now Available

Once loaded, you'll have access to:

### 1. **Harmony Pulse** (`/harmony-pulse`)
- Detect overlapping human + AI edits in real-time
- Ghost highlights show proposed AI changes
- Non-intrusive notifications

### 2. **Configuration Settings** (`Ctrl+,`)
Find in Settings:
```json
{
  "harmony": {
    "enabled": true,
    "llm_backend": "github-models",
    "overlap_window_ms": 5000,
    "ghost_highlight_enabled": true
  }
}
```

### 3. **Agent Team Panel**
- View active agents
- Monitor overlap events
- Review LLM negotiations

---

## ⚙️ Configuration

### LLM Backend Setup

#### Option 1: GitHub Models (Recommended - Free)
```bash
# Set GitHub token
$env:GITHUB_TOKEN = "ghp_xxxxxxxxxxxxxxxxxxxxxxxxxxxx"
```

Then in Zed settings (`Ctrl+,`):
```json
{
  "harmony": {
    "llm_backend": "github-models"
  }
}
```

#### Option 2: OpenAI
```bash
$env:OPENAI_API_KEY = "sk-..."
```

Settings:
```json
{
  "harmony": {
    "llm_backend": "openai"
  }
}
```

#### Option 3: Anthropic Claude
```bash
$env:ANTHROPIC_API_KEY = "sk-ant-..."
```

Settings:
```json
{
  "harmony": {
    "llm_backend": "anthropic"
  }
}
```

#### Option 4: Local (Ollama or LM Studio)
No API key needed! Just install:
- **Ollama:** https://ollama.ai
- **LM Studio:** https://lmstudio.ai

Settings:
```json
{
  "harmony": {
    "llm_backend": "ollama"  // or "lm_studio"
  }
}
```

---

## 🧪 Testing the Extension

### Test 1: Verify Extension Loads
1. Open Zed
2. Look for "Harmony" in the activity bar (left sidebar)
3. You should see the Harmony icon

### Test 2: Check Slash Command
1. In any editor, type: `/harmony-pulse`
2. You should see autocomplete suggestion
3. Press `Enter` to invoke

### Test 3: Monitor Overlaps
1. Open a file in two side-by-side panes
2. Edit the same line simultaneously in both panes
3. Watch for ghost highlights and Pulse notifications

### Test 4: LLM Negotiation
1. When overlaps occur, click the Pulse notification
2. Review the LLM-generated merge proposal
3. Accept or reject the suggestion

---

## 📁 Project Structure

```
Harmony/
├── crates/
│   ├── harmony-core/          # Overlap detection & analysis
│   ├── harmony-analyzer/      # Tree-sitter AST analysis
│   ├── harmony-memory/        # SQLite vector store
│   ├── harmony-mcp/           # Native MCP sidecar (running now)
│   └── harmony-extension/     # Zed WASM extension (to load)
├── .harmony/
│   ├── config.toml           # Configuration (edit this)
│   ├── memory.db             # SQLite database
│   └── harmony.log           # Logs (if debug enabled)
├── target/
│   ├── release/
│   │   └── harmony-mcp.exe   # Compiled sidecar (2.2 MB)
│   └── wasm32-wasip2/release/
│       └── harmony_extension.wasm  # Compiled extension (113 KB)
└── docs/
    └── HARMONY_IMPL_SPEC.md   # Full architecture docs
```

---

## 🔍 Troubleshooting

### Issue: Extension doesn't appear in Zed

**Cause:** Windows version too old or Zed < 0.150.x

**Fix:**
```bash
zed --version  # Check version
zed --update   # Update to latest
```

### Issue: "Path not found" when loading extension

**Cause:** Selected wrong directory (workspace root instead of `crates/harmony-extension`)

**Fix:**
- Redo the "Install Dev Extension" step
- Make sure to select: `...Harmony/crates/harmony-extension`
- NOT just `...Harmony`

### Issue: LLM requests time out

**Cause:** API key missing or network issue

**Fix:**
```bash
# Check environment variable
echo $env:GITHUB_TOKEN  # Should show your token

# Or test API connectivity
curl https://api.github.com/  # Should return JSON
```

### Issue: Sidecar crashes or stops

**Status:** Check if process is running:
```powershell
Get-Process harmony-mcp
```

**Restart:**
```powershell
cd C:\Users\water\Desktop\Testing\mmcp-windows\Harmony
.\target\release\harmony-mcp.exe --db-path .harmony/memory.db
```

---

## 📊 Debugging

### Enable Debug Logging
Edit `.harmony/config.toml`:
```toml
[logging]
level = "debug"

[debugging]
verbose = true
log_ipc = true
log_analysis = true
```

Then restart the sidecar:
```powershell
$env:RUST_LOG = "debug"
.\target\release\harmony-mcp.exe --db-path .harmony/memory.db
```

### View Logs
```bash
tail -f .harmony/harmony.log
```

---

## 📚 Documentation

- **Full Architecture:** `HARMONY_IMPL_SPEC.md` (16K word spec)
- **Setup Report:** `SETUP_AND_TEST_REPORT.md` (complete validation)
- **GitHub:** https://github.com/iamawanishmaurya/harmony-zed
- **License:** MIT

---

## 🎯 What's Next?

1. **Load** the extension (steps above)
2. **Configure** your LLM backend
3. **Test** with overlapping edits
4. **Review** the full spec: `HARMONY_IMPL_SPEC.md`
5. **Report** issues or feedback

---

## 💡 Key Concepts

### Overlap Detection
When a human and AI edit the same code region within 5 seconds, Harmony detects it.

### Ghost Highlights
AI's proposed edits appear as golden highlights, not live changes.

### Negotiation
LLM automatically merges conflicting edits based on AST analysis and semantic impact.

### Provenance Tracking
Every change is logged with: **who** (human/AI), **what** (code diff), **when** (timestamp), **why** (semantic impact).

---

## ✅ Verification Checklist

Before testing, verify:
- [ ] Rust 1.78+ installed
- [ ] Zed 0.150.x or later
- [ ] WASM target available: `rustup target list | grep wasm32`
- [ ] harmony-mcp.exe exists and is running
- [ ] harmony_extension.wasm compiled
- [ ] .harmony/config.toml exists
- [ ] .harmony/memory.db initialized

All items above have been verified ✓

---

**Generated:** 2026-04-07  
**Status:** Production Ready  
**Next Action:** Load in Zed
