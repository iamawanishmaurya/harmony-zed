use similar::{ChangeTag, TextDiff};
use sha2::{Sha256, Digest};

/// Apply a shadow diff (unified format) to the file contents in-memory.
/// Returns the new file content string if patch applies cleanly, or error.
pub fn apply_shadow_diff(
    original_content: &str,
    diff_unified: &str,
) -> Result<String, ShadowError> {
    let mut result_lines: Vec<String> = original_content.lines().map(|l| l.to_string()).collect();
    let hunks = parse_unified_diff(diff_unified)?;

    // Apply hunks in reverse order so line numbers remain valid
    let mut sorted_hunks = hunks;
    sorted_hunks.sort_by(|a, b| b.old_start.cmp(&a.old_start));

    for hunk in sorted_hunks {
        let start = hunk.old_start.saturating_sub(1) as usize; // unified diff is 1-indexed
        let end = start + hunk.old_lines as usize;

        if end > result_lines.len() {
            return Err(ShadowError::PatchFailed(format!(
                "hunk @@ -{},{} exceeds file length {}",
                hunk.old_start, hunk.old_lines, result_lines.len()
            )));
        }

        // Verify context lines match
        let mut old_idx = start;
        for line in &hunk.lines {
            match line {
                DiffLine::Context(text) => {
                    if old_idx >= result_lines.len() || result_lines[old_idx] != *text {
                        return Err(ShadowError::PatchFailed(format!(
                            "context mismatch at line {}: expected '{}', found '{}'",
                            old_idx + 1,
                            text,
                            result_lines.get(old_idx).unwrap_or(&"<EOF>".to_string())
                        )));
                    }
                    old_idx += 1;
                }
                DiffLine::Remove(_) => {
                    old_idx += 1;
                }
                DiffLine::Add(_) => {}
            }
        }

        // Build replacement lines
        let mut new_lines: Vec<String> = Vec::new();
        for line in &hunk.lines {
            match line {
                DiffLine::Context(text) => new_lines.push(text.clone()),
                DiffLine::Add(text) => new_lines.push(text.clone()),
                DiffLine::Remove(_) => {} // skip removed lines
            }
        }

        // Splice in the replacement
        result_lines.splice(start..end, new_lines);
    }

    // Join with newline; preserve trailing newline if original had one
    let mut result = result_lines.join("\n");
    if original_content.ends_with('\n') {
        result.push('\n');
    }
    Ok(result)
}

/// Compute a unified diff between original and modified content.
/// Uses the `similar` crate.
pub fn compute_unified_diff(
    original: &str,
    modified: &str,
    file_path: &str,
) -> String {
    let diff = TextDiff::from_lines(original, modified);
    diff.unified_diff()
        .context_radius(3)
        .header(&format!("a/{}", file_path), &format!("b/{}", file_path))
        .to_string()
}

/// Compute SHA256 of file content (for base_hash in ShadowDiff).
pub fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Returns true if a ShadowDiff can still be applied to current file content
/// (i.e., base_hash still matches current file hash — no intervening save).
pub fn is_diff_applicable(diff: &crate::types::ShadowDiff, current_content: &str) -> bool {
    content_hash(current_content) == diff.base_hash
}

// ─── Diff Parsing ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct ParsedHunk {
    old_start: u32,
    old_lines: u32,
    lines: Vec<DiffLine>,
}

#[derive(Debug, Clone)]
enum DiffLine {
    Context(String),
    Add(String),
    Remove(String),
}

fn parse_unified_diff(diff: &str) -> Result<Vec<ParsedHunk>, ShadowError> {
    let mut hunks = Vec::new();
    let mut current_hunk: Option<ParsedHunk> = None;
    let mut in_header = true;

    for line in diff.lines() {
        if line.starts_with("@@") {
            in_header = false;
            // Parse hunk header: @@ -old_start,old_lines +new_start,new_lines @@
            if let Some(hunk) = current_hunk.take() {
                hunks.push(hunk);
            }
            let (old_start, old_lines) = parse_hunk_header(line)?;
            current_hunk = Some(ParsedHunk {
                old_start,
                old_lines,
                lines: Vec::new(),
            });
        } else if in_header {
            // Skip --- and +++ and other header lines
            continue;
        } else if let Some(ref mut hunk) = current_hunk {
            if let Some(text) = line.strip_prefix('+') {
                hunk.lines.push(DiffLine::Add(text.to_string()));
            } else if let Some(text) = line.strip_prefix('-') {
                hunk.lines.push(DiffLine::Remove(text.to_string()));
            } else if let Some(text) = line.strip_prefix(' ') {
                hunk.lines.push(DiffLine::Context(text.to_string()));
            } else if line.is_empty() {
                // Empty context line
                hunk.lines.push(DiffLine::Context(String::new()));
            }
        }
    }

    if let Some(hunk) = current_hunk {
        hunks.push(hunk);
    }

    Ok(hunks)
}

fn parse_hunk_header(line: &str) -> Result<(u32, u32), ShadowError> {
    // Format: @@ -old_start,old_lines +new_start,new_lines @@
    // or:     @@ -old_start +new_start @@  (if lines=1)
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 {
        return Err(ShadowError::PatchFailed(format!(
            "invalid hunk header: {}",
            line
        )));
    }

    let old_part = parts[1]; // e.g. "-44,8"
    let old_part = old_part.trim_start_matches('-');
    let (old_start, old_lines) = if let Some((start, lines)) = old_part.split_once(',') {
        (
            start.parse::<u32>().map_err(|e| ShadowError::PatchFailed(e.to_string()))?,
            lines.parse::<u32>().map_err(|e| ShadowError::PatchFailed(e.to_string()))?,
        )
    } else {
        (
            old_part.parse::<u32>().map_err(|e| ShadowError::PatchFailed(e.to_string()))?,
            1,
        )
    };

    Ok((old_start, old_lines))
}

#[derive(thiserror::Error, Debug)]
pub enum ShadowError {
    #[error("patch does not apply cleanly: {0}")]
    PatchFailed(String),
    #[error("base hash mismatch: diff is stale")]
    StaleBase,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    #[test]
    fn test_compute_unified_diff() {
        let original = "function hello() {\n  console.log('hello');\n}\n";
        let modified = "function hello() {\n  console.log('hello world');\n  return true;\n}\n";
        let diff = compute_unified_diff(original, modified, "src/hello.ts");
        assert!(diff.contains("--- a/src/hello.ts"));
        assert!(diff.contains("+++ b/src/hello.ts"));
        assert!(diff.contains("@@"));
    }

    #[test]
    fn test_content_hash() {
        let content = "hello world";
        let hash = content_hash(content);
        assert_eq!(hash.len(), 64); // SHA256 hex is 64 chars
        // Same content should produce same hash
        assert_eq!(hash, content_hash(content));
    }

    #[test]
    fn test_content_hash_different() {
        let hash1 = content_hash("hello");
        let hash2 = content_hash("world");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_round_trip_diff() {
        let original = "line 1\nline 2\nline 3\nline 4\nline 5\n";
        let modified = "line 1\nline 2 modified\nline 3\nnew line\nline 4\nline 5\n";

        let diff = compute_unified_diff(original, modified, "test.txt");
        let result = apply_shadow_diff(original, &diff).unwrap();
        assert_eq!(result, modified);
    }

    #[test]
    fn test_round_trip_typescript() {
        let original = r#"import { Request, Response } from 'express';

export function validateJWT(req: Request, res: Response) {
  const token = req.headers.authorization;
  if (!token) {
    return res.status(401).json({ error: 'No token' });
  }
  const payload = jwt.verify(token, SECRET);
  req.user = payload;
}

export function logout(req: Request, res: Response) {
  req.session.destroy();
  res.status(200).json({ ok: true });
}
"#;

        let modified = r#"import { Request, Response } from 'express';
import { redis } from '../cache';

export function validateJWT(req: Request, res: Response) {
  const token = req.headers.authorization;
  if (!token) {
    return res.status(401).json({ error: 'No token' });
  }
  const cached = await redis.get(`jwt:${token}`);
  if (cached) return JSON.parse(cached);
  const payload = jwt.verify(token, SECRET);
  await redis.set(`jwt:${token}`, JSON.stringify(payload), 'EX', 3600);
  req.user = payload;
}

export function logout(req: Request, res: Response) {
  req.session.destroy();
  res.status(200).json({ ok: true });
}
"#;

        let diff = compute_unified_diff(original, modified, "src/middleware/auth.ts");
        let result = apply_shadow_diff(original, &diff).unwrap();
        assert_eq!(result, modified);
    }

    #[test]
    fn test_is_diff_applicable() {
        let content = "hello world\n";
        let base_hash = content_hash(content);

        let diff = ShadowDiff {
            id: uuid::Uuid::new_v4(),
            agent_id: uuid::Uuid::new_v4(),
            file_path: "test.txt".to_string(),
            diff_unified: String::new(),
            base_hash: base_hash.clone(),
            created_at: chrono::Utc::now(),
            status: ShadowDiffStatus::Pending,
        };

        assert!(is_diff_applicable(&diff, content));
        assert!(!is_diff_applicable(&diff, "different content\n"));
    }
}
