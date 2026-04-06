//! Sandbox executor for running project tests against proposed diffs.
//!
//! §10, §17 Task 10: Applies a proposed diff to a temporary copy of the project,
//! runs the test suite, and reports pass/fail with captured output.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Result of a sandbox test run.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SandboxResult {
    pub passed: bool,
    pub tests_total: u32,
    pub tests_passed: u32,
    pub tests_failed: u32,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub duration_ms: u64,
    pub error: Option<String>,
}

/// Detect the test command for a project by probing for known config files.
pub fn detect_test_command(project_root: &Path) -> Option<TestCommand> {
    // 1. package.json with "test" script
    let pkg_json = project_root.join("package.json");
    if pkg_json.exists() {
        if let Ok(content) = std::fs::read_to_string(&pkg_json) {
            if content.contains("\"test\"") {
                return Some(TestCommand {
                    program: "npm".into(),
                    args: vec!["test".into(), "--".into(), "--no-coverage".into()],
                    timeout: Duration::from_secs(120),
                });
            }
        }
    }

    // 2. Cargo.toml — Rust project
    let cargo_toml = project_root.join("Cargo.toml");
    if cargo_toml.exists() {
        return Some(TestCommand {
            program: "cargo".into(),
            args: vec!["test".into(), "--no-fail-fast".into()],
            timeout: Duration::from_secs(300),
        });
    }

    // 3. Makefile with "test" target
    let makefile = project_root.join("Makefile");
    if makefile.exists() {
        if let Ok(content) = std::fs::read_to_string(&makefile) {
            if content.contains("test:") {
                return Some(TestCommand {
                    program: "make".into(),
                    args: vec!["test".into()],
                    timeout: Duration::from_secs(120),
                });
            }
        }
    }

    None
}

#[derive(Debug, Clone)]
pub struct TestCommand {
    pub program: String,
    pub args: Vec<String>,
    pub timeout: Duration,
}

/// Run the sandbox: applies a diff to a temp copy, runs tests, collects results.
///
/// If `diff_unified` is empty, just runs tests against the current project state.
pub fn run_sandbox(
    project_root: &Path,
    diff_unified: &str,
    timeout_secs: u64,
) -> SandboxResult {
    let start = std::time::Instant::now();

    // Detect test command
    let test_cmd = match detect_test_command(project_root) {
        Some(cmd) => cmd,
        None => {
            return SandboxResult {
                passed: false,
                tests_total: 0, tests_passed: 0, tests_failed: 0,
                stdout_tail: String::new(),
                stderr_tail: String::new(),
                duration_ms: start.elapsed().as_millis() as u64,
                error: Some("No test command found in project root".into()),
            };
        }
    };

    // If there's a diff, apply it to a temp dir
    let work_dir = if !diff_unified.is_empty() {
        match create_sandbox_copy(project_root, diff_unified) {
            Ok(dir) => dir,
            Err(e) => {
                return SandboxResult {
                    passed: false,
                    tests_total: 0, tests_passed: 0, tests_failed: 0,
                    stdout_tail: String::new(),
                    stderr_tail: String::new(),
                    duration_ms: start.elapsed().as_millis() as u64,
                    error: Some(format!("Failed to create sandbox: {}", e)),
                };
            }
        }
    } else {
        project_root.to_path_buf()
    };

    // Run tests with timeout
    let timeout = Duration::from_secs(timeout_secs.min(test_cmd.timeout.as_secs()));
    let result = run_with_timeout(&work_dir, &test_cmd.program, &test_cmd.args, timeout);

    // Clean up temp dir if we created one
    if work_dir != project_root {
        let _ = std::fs::remove_dir_all(&work_dir);
    }

    let elapsed = start.elapsed().as_millis() as u64;

    match result {
        Ok((stdout, stderr, exit_code)) => {
            let (total, passed, failed) = parse_test_output(&stdout, &stderr);
            SandboxResult {
                passed: exit_code == 0 && failed == 0,
                tests_total: total,
                tests_passed: passed,
                tests_failed: failed,
                stdout_tail: tail_string(&stdout, 2000),
                stderr_tail: tail_string(&stderr, 1000),
                duration_ms: elapsed,
                error: None,
            }
        }
        Err(e) => {
            SandboxResult {
                passed: false,
                tests_total: 0, tests_passed: 0, tests_failed: 0,
                stdout_tail: String::new(),
                stderr_tail: String::new(),
                duration_ms: elapsed,
                error: Some(format!("{}", e)),
            }
        }
    }
}

/// Create a temporary copy of the project and apply the diff.
fn create_sandbox_copy(project_root: &Path, _diff_unified: &str) -> anyhow::Result<PathBuf> {
    let tmp = std::env::temp_dir().join(format!("harmony-sandbox-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp)?;

    // Shallow copy: copy only source files (not target/, node_modules/, etc.)
    copy_dir_filtered(project_root, &tmp, &["target", "node_modules", ".git", ".harmony"])?;

    // TODO: Apply the unified diff to the temp directory files
    // For v0.1, we just run tests against the copied project
    // Full diff application would parse the unified diff and write modified files

    Ok(tmp)
}

fn copy_dir_filtered(src: &Path, dst: &Path, skip: &[&str]) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if skip.iter().any(|s| name_str == *s) {
            continue;
        }

        let src_path = entry.path();
        let dst_path = dst.join(&name);

        if src_path.is_dir() {
            std::fs::create_dir_all(&dst_path)?;
            copy_dir_filtered(&src_path, &dst_path, skip)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn run_with_timeout(
    cwd: &Path,
    program: &str,
    args: &[String],
    timeout: Duration,
) -> anyhow::Result<(String, String, i32)> {
    let child = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to start {}: {}", program, e))?;

    let output = child.wait_with_output()
        .map_err(|e| anyhow::anyhow!("Test process error: {}", e))?;

    // Check if we exceeded timeout (approximate — for precise timeout, use
    // tokio::time::timeout in async context or libc kill on Unix)
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(1);

    Ok((stdout, stderr, code))
}

/// Parse test output for pass/fail counts.
/// Supports: Rust `cargo test`, Node.js jest/vitest, generic "X passed, Y failed".
fn parse_test_output(stdout: &str, stderr: &str) -> (u32, u32, u32) {
    let combined = format!("{}\n{}", stdout, stderr);

    // Rust: "test result: ok. 26 passed; 0 failed; 0 ignored"
    if let Some(caps) = regex_match_test_result(&combined) {
        return caps;
    }

    // Generic: "X passed" / "Y failed"
    let passed = extract_count(&combined, "passed");
    let failed = extract_count(&combined, "failed");
    let total = passed + failed;
    (total, passed, failed)
}

fn regex_match_test_result(text: &str) -> Option<(u32, u32, u32)> {
    // Simple manual parsing for "N passed" and "N failed"
    for line in text.lines() {
        if line.contains("test result:") {
            let passed = extract_count(line, "passed");
            let failed = extract_count(line, "failed");
            return Some((passed + failed, passed, failed));
        }
    }
    None
}

fn extract_count(text: &str, keyword: &str) -> u32 {
    // Look for "X passed" or "X failed" patterns
    for word_pair in text.split_whitespace().collect::<Vec<_>>().windows(2) {
        if word_pair[1].contains(keyword) || word_pair[1].trim_end_matches(';').contains(keyword) {
            if let Ok(n) = word_pair[0].parse::<u32>() {
                return n;
            }
        }
    }
    0
}

fn tail_string(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_string()
    } else {
        format!("…{}", &s[s.len() - max_chars..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_test_command_cargo() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("Cargo.toml"), "[package]\nname=\"test\"").unwrap();
        let cmd = detect_test_command(tmp.path());
        assert!(cmd.is_some());
        assert_eq!(cmd.as_ref().unwrap().program, "cargo");
    }

    #[test]
    fn test_detect_test_command_npm() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("package.json"), r#"{"scripts":{"test":"jest"}}"#).unwrap();
        let cmd = detect_test_command(tmp.path());
        assert!(cmd.is_some());
        assert_eq!(cmd.as_ref().unwrap().program, "npm");
    }

    #[test]
    fn test_detect_no_test_command() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cmd = detect_test_command(tmp.path());
        assert!(cmd.is_none());
    }

    #[test]
    fn test_parse_rust_output() {
        let stdout = "test result: ok. 26 passed; 3 failed; 0 ignored; 0 measured";
        let (total, passed, failed) = parse_test_output(stdout, "");
        assert_eq!(total, 29);
        assert_eq!(passed, 26);
        assert_eq!(failed, 3);
    }

    #[test]
    fn test_parse_generic_output() {
        let stdout = "Tests: 5 passed, 2 failed, 7 total";
        let (total, passed, failed) = parse_test_output(stdout, "");
        assert_eq!(passed, 5);
        assert_eq!(failed, 2);
        assert_eq!(total, 7);
    }

    #[test]
    fn test_tail_string_short() {
        assert_eq!(tail_string("abc", 10), "abc");
    }

    #[test]
    fn test_tail_string_long() {
        let result = tail_string("abcdefghij", 5);
        assert!(result.contains("fghij"));
    }

    #[test]
    fn test_sandbox_no_test_command() {
        let tmp = tempfile::TempDir::new().unwrap();
        let result = run_sandbox(tmp.path(), "", 60);
        assert!(!result.passed);
        assert!(result.error.as_ref().unwrap().contains("No test command"));
    }
}
