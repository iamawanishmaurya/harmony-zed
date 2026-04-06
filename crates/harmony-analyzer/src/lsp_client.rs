use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::io::{BufReader, BufRead, Write};
use std::sync::atomic::{AtomicI32, Ordering};
use lsp_types::*;
use crate::treesitter::SupportedLanguage;

pub struct LspClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    request_id: AtomicI32,
    project_root: PathBuf,
}

impl LspClient {
    /// Spawn the appropriate LSP server for the language.
    /// TypeScript/JavaScript: `typescript-language-server --stdio`
    /// Rust: `rust-analyzer`
    /// These must be installed on the host machine (not bundled).
    pub fn spawn(language: SupportedLanguage, project_root: &Path) -> anyhow::Result<Self> {
        let (cmd, args): (&str, Vec<&str>) = match language {
            SupportedLanguage::TypeScript | SupportedLanguage::JavaScript => {
                ("typescript-language-server", vec!["--stdio"])
            }
            SupportedLanguage::Rust => {
                ("rust-analyzer", vec![])
            }
        };

        let mut child = Command::new(cmd)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .current_dir(project_root)
            .spawn()
            .map_err(|e| anyhow::anyhow!(
                "Failed to spawn LSP server '{}': {}. Install it to enable impact analysis.", cmd, e
            ))?;

        let stdin = child.stdin.take().ok_or_else(|| anyhow::anyhow!("Failed to get LSP stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow::anyhow!("Failed to get LSP stdout"))?;

        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            request_id: AtomicI32::new(0),
            project_root: project_root.to_path_buf(),
        })
    }

    fn next_id(&self) -> i32 {
        self.request_id.fetch_add(1, Ordering::Relaxed)
    }

    fn send_request(&mut self, method: &str, params: serde_json::Value) -> anyhow::Result<serde_json::Value> {
        let id = self.next_id();
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });

        let body = serde_json::to_string(&request)?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());

        self.stdin.write_all(header.as_bytes())?;
        self.stdin.write_all(body.as_bytes())?;
        self.stdin.flush()?;

        // Read response
        self.read_response()
    }

    fn read_response(&mut self) -> anyhow::Result<serde_json::Value> {
        // Read Content-Length header
        let mut header = String::new();
        loop {
            header.clear();
            self.stdout.read_line(&mut header)?;
            if header.trim().is_empty() {
                break;
            }
        }

        // Read body (simplified — in production, parse Content-Length properly)
        let mut body = String::new();
        self.stdout.read_line(&mut body)?;
        let response: serde_json::Value = serde_json::from_str(&body)?;
        Ok(response)
    }

    /// Send textDocument/definition for a symbol at position.
    pub fn find_definition(&mut self, file: &str, line: u32, col: u32)
        -> anyhow::Result<Option<Location>>
    {
        let uri = format!("file://{}", self.project_root.join(file).display());
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": col }
        });

        let response = self.send_request("textDocument/definition", params)?;
        // Parse Location from response
        if let Some(result) = response.get("result") {
            if let Ok(location) = serde_json::from_value::<Location>(result.clone()) {
                return Ok(Some(location));
            }
        }
        Ok(None)
    }

    /// Send textDocument/references for a symbol at position.
    pub fn find_references(&mut self, file: &str, line: u32, col: u32)
        -> anyhow::Result<Vec<Location>>
    {
        let uri = format!("file://{}", self.project_root.join(file).display());
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": col },
            "context": { "includeDeclaration": false }
        });

        let response = self.send_request("textDocument/references", params)?;
        if let Some(result) = response.get("result") {
            if let Ok(locations) = serde_json::from_value::<Vec<Location>>(result.clone()) {
                return Ok(locations);
            }
        }
        Ok(Vec::new())
    }

    /// Send shutdown + exit. Always call this on drop.
    pub fn shutdown(&mut self) -> anyhow::Result<()> {
        let _ = self.send_request("shutdown", serde_json::json!(null));
        let _ = self.stdin.write_all(b"Content-Length: 33\r\n\r\n{\"jsonrpc\":\"2.0\",\"method\":\"exit\"}");
        let _ = self.stdin.flush();
        let _ = self.child.wait();
        Ok(())
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
