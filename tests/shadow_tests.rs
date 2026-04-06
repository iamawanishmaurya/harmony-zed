//! Shadow diff integration tests.
//! These tests verify the round-trip: original → compute_unified_diff → apply_shadow_diff → original matches modified.

use harmony_core::shadow::*;
use harmony_core::types::*;

#[test]
fn round_trip_simple() {
    let original = "line 1\nline 2\nline 3\n";
    let modified = "line 1\nline 2 modified\nline 3\n";
    let diff = compute_unified_diff(original, modified, "test.txt");
    let result = apply_shadow_diff(original, &diff).unwrap();
    assert_eq!(result, modified);
}

#[test]
fn round_trip_typescript_auth() {
    let original = r#"import { Request } from 'express';

export function validateJWT(req: Request) {
  const token = req.headers.authorization;
  if (!token) {
    return false;
  }
  return jwt.verify(token, SECRET);
}
"#;

    let modified = r#"import { Request } from 'express';
import { cache } from './cache';

export function validateJWT(req: Request) {
  const token = req.headers.authorization;
  const cached = cache.get(token);
  if (cached) return cached;
  if (!token) {
    return false;
  }
  return jwt.verify(token, SECRET);
}
"#;

    let diff = compute_unified_diff(original, modified, "src/auth.ts");
    let result = apply_shadow_diff(original, &diff).unwrap();
    assert_eq!(result, modified);
}

#[test]
fn hash_consistency() {
    let content = "hello world\n";
    let hash1 = content_hash(content);
    let hash2 = content_hash(content);
    assert_eq!(hash1, hash2);
    assert_eq!(hash1.len(), 64); // SHA-256 hex
}

#[test]
fn hash_differs_for_different_content() {
    let h1 = content_hash("foo\n");
    let h2 = content_hash("bar\n");
    assert_ne!(h1, h2);
}

#[test]
fn test_applicability_check() {
    let content = "function foo() {}\n";
    let base_hash = content_hash(content);

    let diff = ShadowDiff {
        id: uuid::Uuid::new_v4(),
        agent_id: uuid::Uuid::new_v4(),
        file_path: "test.ts".to_string(),
        diff_unified: String::new(),
        base_hash,
        created_at: chrono::Utc::now(),
        status: ShadowDiffStatus::Pending,
    };

    assert!(is_diff_applicable(&diff, content));
    assert!(!is_diff_applicable(&diff, "function bar() {}\n"));
}
