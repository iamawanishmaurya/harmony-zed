# Harmony

**Intelligent mediation for parallel human + AI development.**

Harmony is a fast, local-first Zed editor extension that makes parallel human + AI-agent development safe and collaborative. It tracks every change with full provenance metadata, detects overlapping edits, runs semantic impact analysis, and surfaces non-intrusive resolution UI to securely merge code.

```
       Human edit             AI Agent edit
            │                       │
            └────── OVERLAP! ───────┘
                    │
            ┌───────┴───────┐
      [Accept Mine]   [Negotiate ✨]
```

## Features

- **Full Provenance Tracking**: Tracks *who* made every change (Human, Agent) via an MCP sidecar.
- **Overlap Detection Engine**: Detects code collisions inside configurable time windows.
- **Tree-Sitter Impact Analyzer**: Computes AST complexity and logical disruption of overlaps.
- **Shadow Diffing**: Applies AI fixes silently in the background, only surfacing them as "Ghost Highlights".
- **Multi-Backend LLM Negotiation**: Combine colliding edits automatically using OpenAI, Anthropic, GitHub Models, Ollama, LM Studio, or local ACP agents.
- **Semantic Memory**: SQLite vector store with local `fastembed` retrieval.

---

## 🛠️ Prerequisites

1. **Rust (1.80+)**
   - **Windows:** `winget install Rustlang.Rust.MSVC`
   - **Linux/macOS:** `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
2. **Git**
3. **Zed Editor** *(Required to run the frontend WASM extension)*

---

## 🚀 Installation & Build

### On Linux / macOS

```bash
# 1. Clone the repository
git clone https://github.com/iamawanishmaurya/harmony-zed.git
cd harmony-zed

# 2. Build the MCP sidecar binary 
cargo build --release -p harmony-mcp

# 3. Add the WebAssembly target and build the Zed Extension
rustup target add wasm32-wasip2
cargo build --release -p harmony-extension --target wasm32-wasip2
```

### On Windows

```powershell
# 1. Clone the repository
git clone https://github.com/iamawanishmaurya/harmony-zed.git
cd harmony-zed

# 2. Build the MCP sidecar binary (requires MSVC Build Tools)
cargo build --release -p harmony-mcp

# 3. Add WASM target and build the Zed Extension
rustup target add wasm32-wasip2
cargo build --release -p harmony-extension --target wasm32-wasip2
```

---

## ⚙️ Configuration

Harmony is designed to be zero-config. The first time you run the sidecar in your project, it automatically provisions a SQLite database and creates a config file template.

To initialize Harmony in your current project directory:

**Windows:**
```powershell
# Starts the sidecar in the current folder
\path\to\harmony-zed\target\release\harmony-mcp.exe --db-path .harmony/memory.db
```

**Linux/macOS:**
```bash
# Starts the sidecar in the current folder
/path/to/harmony-zed/target/release/harmony-mcp --db-path .harmony/memory.db
```

### Loading into Zed

Zed workspaces require you to point the extension loader directly to the crate where the `extension.toml` and `Cargo.toml` reside alongside each other. Note that you CANNOT select the root folder of the repo, because it is a cargo `[workspace]`.

1. Open Zed's **Command Palette** (`Ctrl + Shift + P` or `Cmd + Shift + P`)
2. Type and select **`zed: install dev extension`**
3. Select the nested directory: `/path/to/harmony-zed/crates/harmony-extension`
4. Zed will mount the `extension.toml` and automatically start tracking your file changes!

### Setting up the LLM Backend

Once started, Harmony generates a rich configuration file at `.harmony/config.toml`. Open this file to customize your AI model settings. 

By default, Harmony delegates negotiation to local Agents. You can switch this to use any popular API or local runner:

**Option 1: Free GitHub Models (Recommended)**
```toml
[negotiation]
negotiation_backend = "openai"
api_key = "github_pat_YOUR_TOKEN" # Must have 'copilot' / models scope
model = "gpt-4o-mini"
base_url = "https://models.inference.ai.azure.com"
```

**Option 2: 100% Local (Ollama)**
```toml
[negotiation]
negotiation_backend = "openai"
api_key = "ollama"
model = "llama3.3"
base_url = "http://localhost:11434/v1"
```

*See the `USAGE.md` doc for OpenAI, Anthropic, LM Studio, and more setup examples.*

---

## 💻 How to Use

Once the MCP sidecar is running in the background and the extension is loaded into Zed:

1. **Make Edits Ordinarily:** You code like normal. The extension captures file diffs and passes them down to the sidecar as `report_change` events.
2. **AI Agents Code Parallelly:** An external AI (working via Copilot, Cursor, native Zed tasks, etc.) edits code.
3. **Collision Detection:** If the system detects both of you edited the same function within the `overlap_window_minutes` limit, the UI triggers a **Pulse Panel notification**.
4. **Negotiation:** Click **✨ Negotiate**. Harmony packages both AST chunks, hits the configured LLM Backend, and returns a cleanly merged unified diff for you to selectively apply.

### Where is the Data Stored?
All events, overlaps, and provenance tags are stored locally in `.harmony/memory.db`. No code is beamed to the cloud unless you explicitly utilize cloud LLM backends for negotiation.

---

## 🧪 Testing

Want to contribute or verify your build? Harmony has an extensive test suite checking all platforms and LLM paths:

```bash
# Run the entire test suite (95 tests)
cargo test --workspace

# Run just the backend integration tests
cargo test -p harmony-core --test negotiation_tests

# Run E2E logic golden path
cargo test -p harmony-core --test e2e_golden_path
```

## 📚 Documentation

For an in-depth breakdown of system behavior, refer to the documentation in `/docs`:
- [`docs/PROJECT.md`](./docs/PROJECT.md) — What Harmony is, Key Concepts, and Architecture
- [`docs/USAGE.md`](./docs/USAGE.md) — Exhaustive guide on setting up different Agent Backends, memory DB, and reporting bugs
- [`docs/TESTING.md`](./docs/TESTING.md) — 5-phase testing guide
- [`docs/OVERVIEW.md`](./docs/OVERVIEW.md) — Development tracker & module breakdown

---

## License

MIT — Awanish Maurya · XPWNIT LAB · April 2026
