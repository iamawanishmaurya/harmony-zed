# Harmony

**Intelligent mediation for parallel human + AI development.**

Harmony is a Zed editor extension that makes parallel human + AI-agent development safe and collaborative. It tracks every change with full provenance metadata, detects overlapping edits, runs semantic impact analysis, and surfaces non-intrusive resolution UI.

## Architecture

- **harmony-core** — Pure Rust types, overlap detection, shadow diffs, negotiation logic
- **harmony-analyzer** — Tree-sitter AST diffing + LSP dependency lookup
- **harmony-memory** — SQLite + embedding-based semantic memory store
- **harmony-mcp** — Standalone MCP server binary (sidecar)
- **harmony-extension** — Zed extension (compiles to WASM)

## Build

```bash
# Build native crates
cargo build --release -p harmony-core -p harmony-analyzer -p harmony-memory -p harmony-mcp

# Build WASM extension
rustup target add wasm32-wasip2
cargo build --release -p harmony-extension --target wasm32-wasip2
```

## Test

```bash
cargo test --workspace
```

## License

MIT — Awanish Maurya · XPWNIT LAB · April 2026
