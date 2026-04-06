//! End-to-end integration test (§17 Task 16).
//!
//! Full golden path: creates a temp project with TypeScript files,
//! opens MemoryStore, reports two changes on the same file region
//! with different actors, and verifies:
//!   - OverlapEvent is created
//!   - ImpactGraph is generated
//!   - Summary string is sensible
//!   - Memory store persists correctly

use harmony_core::types::*;
use harmony_core::overlap::detect_overlaps;
use harmony_core::shadow::{compute_unified_diff, apply_shadow_diff, content_hash};
use harmony_core::negotiation::{build_negotiation_prompt, parse_negotiation_result};
use harmony_core::config::HarmonyConfig;
use harmony_core::sandbox::{detect_test_command, run_sandbox};
use harmony_memory::store::MemoryStore;
use harmony_analyzer::treesitter::{TreeSitterAnalyzer, SupportedLanguage};
use harmony_analyzer::impact::ImpactAnalyzer;
use chrono::Utc;
use uuid::Uuid;
use tempfile::TempDir;
use std::path::Path;

/// Set up a realistic temp project with TypeScript files.
fn setup_project() -> (TempDir, MemoryStore) {
    let tmp = TempDir::new().unwrap();

    // Create source files
    let src_dir = tmp.path().join("src").join("middleware");
    std::fs::create_dir_all(&src_dir).unwrap();

    // auth.ts — the file both actors will edit
    std::fs::write(src_dir.join("auth.ts"), r#"import { Request, Response, NextFunction } from 'express';
import jwt from 'jsonwebtoken';

const SECRET = process.env.JWT_SECRET || 'dev-secret';

export function validateJWT(req: Request, res: Response, next: NextFunction) {
  const header = req.headers.authorization;
  if (!header) {
    return res.status(401).json({ error: 'Authorization required' });
  }

  const token = header.replace('Bearer ', '');
  try {
    const payload = jwt.verify(token, SECRET);
    req.user = payload;
    next();
  } catch (err) {
    return res.status(401).json({ error: 'Invalid token' });
  }
}

export function requireRole(role: string) {
  return (req: Request, res: Response, next: NextFunction) => {
    if (req.user?.role !== role) {
      return res.status(403).json({ error: 'Insufficient permissions' });
    }
    next();
  };
}

export function logout(req: Request, res: Response) {
  // Clear session
  req.session?.destroy();
  res.status(200).json({ message: 'Logged out' });
}
"#).unwrap();

    // routes.ts — a second file for cross-file testing
    std::fs::write(src_dir.join("routes.ts"), r#"import { Router } from 'express';
import { validateJWT, requireRole } from './auth';

const router = Router();

router.get('/api/profile', validateJWT, (req, res) => {
  res.json(req.user);
});

router.get('/api/admin', validateJWT, requireRole('admin'), (req, res) => {
  res.json({ admin: true });
});

export default router;
"#).unwrap();

    // Create .harmony directory and open store
    let harmony_dir = tmp.path().join(".harmony");
    std::fs::create_dir_all(&harmony_dir).unwrap();
    let db_path = harmony_dir.join("memory.db");
    let store = MemoryStore::open(&db_path).unwrap();

    (tmp, store)
}

/// Simulate a human editing validateJWT (lines 5-20)
fn human_edit() -> (String, ProvenanceTag) {
    let diff = r#"@@ -5,8 +5,10 @@
 export function validateJWT(req: Request, res: Response, next: NextFunction) {
   const header = req.headers.authorization;
-  if (!header) {
-    return res.status(401).json({ error: 'Authorization required' });
+  if (!header || header.trim() === '') {
+    return res.status(401).json({
+      error: 'Authorization header required',
+      code: 'AUTH_MISSING'
+    });
   }
"#.to_string();

    let tag = ProvenanceTag {
        id: Uuid::new_v4(),
        actor_id: ActorId("human:awanish".to_string()),
        actor_kind: ActorKind::Human,
        task_id: None,
        task_prompt: Some("Improve auth error messages".to_string()),
        timestamp: Utc::now(),
        file_path: "src/middleware/auth.ts".to_string(),
        region: TextRange { start_line: 5, end_line: 20, start_col: 0, end_col: 0 },
        mode: AgentMode::Shadow,
        diff_unified: diff.clone(),
        session_id: Uuid::new_v4(),
    };

    (diff, tag)
}

/// Simulate an agent adding Redis caching to validateJWT (lines 11-18)
fn agent_edit() -> (String, ProvenanceTag) {
    let diff = r#"@@ -11,4 +11,8 @@
   const token = header.replace('Bearer ', '');
+  // Check Redis cache first
+  const cached = await redis.get(`jwt:${token}`);
+  if (cached) {
+    req.user = JSON.parse(cached);
+    return next();
+  }
   try {
"#.to_string();

    let tag = ProvenanceTag {
        id: Uuid::new_v4(),
        actor_id: ActorId("agent:architect-01".to_string()),
        actor_kind: ActorKind::Agent,
        task_id: Some(Uuid::new_v4()),
        task_prompt: Some("Add Redis caching to JWT validation for performance".to_string()),
        timestamp: Utc::now(),
        file_path: "src/middleware/auth.ts".to_string(),
        region: TextRange { start_line: 11, end_line: 18, start_col: 0, end_col: 0 },
        mode: AgentMode::Shadow,
        diff_unified: diff.clone(),
        session_id: Uuid::new_v4(),
    };

    (diff, tag)
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn e2e_golden_path() {
    let (_tmp, store) = setup_project();

    // Step 1: Human reports a change
    let (_human_diff, human_tag) = human_edit();
    store.insert_provenance_tag(&human_tag).unwrap();

    // Step 2: Agent reports a change
    let (_agent_diff, agent_tag) = agent_edit();
    store.insert_provenance_tag(&agent_tag).unwrap();

    // Step 3: Detect overlaps
    let recent = store.get_recent_tags_for_file("src/middleware/auth.ts", 30).unwrap();
    assert_eq!(recent.len(), 2, "Both tags should be stored");

    let overlaps = detect_overlaps(&agent_tag, &recent, 30);
    assert_eq!(overlaps.len(), 1, "Exactly one overlap should be detected");

    let overlap = &overlaps[0];
    assert_eq!(overlap.file_path, "src/middleware/auth.ts");

    // Store the overlap
    store.insert_overlap_event(overlap).unwrap();

    // Step 4: Run impact analysis
    let auth_content = std::fs::read_to_string(
        _tmp.path().join("src/middleware/auth.ts")
    ).unwrap();

    let mut impact_analyzer = ImpactAnalyzer::new_without_lsp();
    let impact = impact_analyzer.analyze(overlap, &auth_content, &auth_content);

    assert!(!impact.summary.is_empty(), "Impact summary should not be empty");
    assert_eq!(impact.overlap_id, overlap.id);

    // Step 5: Summary should mention both actors
    let summary = &impact.summary;
    assert!(
        summary.contains("awanish") || summary.contains("human"),
        "Summary should mention human actor. Got: {}", summary
    );
    assert!(
        summary.contains("Architect") || summary.contains("architect"),
        "Summary should mention agent actor. Got: {}", summary
    );

    // Step 6: Store a memory note about this overlap
    let memory_id = store.add_memory(
        "Overlap detected: human improved auth errors while agent added Redis caching to validateJWT",
        vec!["overlap".into(), "auth".into(), "redis".into()],
        MemoryNamespace::Shared,
        None,
        vec![],
    ).unwrap();
    assert_ne!(memory_id, Uuid::nil());

    // Step 7: Query memory should find the note
    let results = store.query_memory("redis auth overlap", MemoryNamespace::Shared, 5).unwrap();
    assert!(!results.is_empty(), "Memory query should return results");
    assert!(results[0].0.content.contains("validateJWT"));

    // Step 8: Build negotiation prompt
    let prompt = build_negotiation_prompt(overlap, &impact, &results);
    assert!(prompt.contains("src/middleware/auth.ts"));
    assert!(prompt.contains("proposed_diff"));

    // Step 9: Parse a mock negotiation result
    let mock_response = r#"{
        "proposed_diff": "--- a/src/middleware/auth.ts\n+++ b/src/middleware/auth.ts\n@@ merged @@",
        "rationale": "Combined auth error improvements with Redis caching.",
        "confidence": 0.82,
        "memory_notes": ["Merged auth error messages with Redis cache layer"]
    }"#;
    let neg_result = parse_negotiation_result(overlap.id, mock_response).unwrap();
    assert_eq!(neg_result.confidence, 0.82);
    assert!(neg_result.proposed_diff.contains("merged"));
}

#[test]
fn e2e_shadow_diff_round_trip() {
    let original = r#"export function validateJWT(req: Request) {
  const token = req.headers.authorization;
  return jwt.verify(token, SECRET);
}
"#;
    let modified = r#"export function validateJWT(req: Request) {
  const token = req.headers.authorization;
  if (!token) return null;
  const cached = cache.get(token);
  if (cached) return cached;
  return jwt.verify(token, SECRET);
}
"#;
    let diff = compute_unified_diff(original, modified, "auth.ts");
    let result = apply_shadow_diff(original, &diff).unwrap();
    assert_eq!(result, modified, "Round-trip diff/apply should produce identical output");

    // Hash should differ
    assert_ne!(content_hash(original), content_hash(modified));
}

#[test]
fn e2e_config_loads_defaults() {
    let tmp = TempDir::new().unwrap();
    let config = HarmonyConfig::load(tmp.path()).unwrap();
    assert_eq!(config.general.overlap_window_minutes, 30);
    assert_eq!(config.analysis.lsp_mode, "auto");
    assert_eq!(config.negotiation.negotiation_backend, "agent");
    assert_eq!(config.agents.registry.len(), 2);

    // Config file should have been created
    let config_file = tmp.path().join(".harmony").join("config.toml");
    assert!(config_file.exists(), "Config file should be auto-created");
}

#[test]
fn e2e_memory_similarity_ranking() {
    let (_tmp, store) = setup_project();

    // Add several memories
    store.add_memory(
        "We rejected Redis for session caching because of operational complexity",
        vec!["decision".into(), "rejected".into(), "redis".into()],
        MemoryNamespace::Shared, None, vec![],
    ).unwrap();

    store.add_memory(
        "Added logging middleware using winston with structured JSON output",
        vec!["implementation".into(), "logging".into()],
        MemoryNamespace::Shared, None, vec![],
    ).unwrap();

    store.add_memory(
        "Chose PostgreSQL over MySQL for better JSON column support",
        vec!["decision".into(), "database".into()],
        MemoryNamespace::Shared, None, vec![],
    ).unwrap();

    // Query for redis — should rank the redis note highest
    let results = store.query_memory("why was redis rejected", MemoryNamespace::Shared, 5).unwrap();
    assert!(!results.is_empty());
    assert!(
        results[0].0.content.contains("Redis") || results[0].0.content.contains("redis"),
        "Top result should be about Redis. Got: '{}'",
        results[0].0.content
    );
}

#[test]
fn e2e_tree_sitter_analysis() {
    let mut analyzer = TreeSitterAnalyzer::new();

    let ts_content = r#"export function validateJWT(req: Request, res: Response) {
  const token = req.headers.authorization;
  if (!token) {
    return res.status(401).json({ error: 'No token' });
  }
  const payload = jwt.verify(token, SECRET);
  req.user = payload;
}

export function logout() {
  session.destroy();
}
"#;

    let region = TextRange { start_line: 0, end_line: 8, start_col: 0, end_col: 0 };
    let symbols = analyzer.extract_symbols_in_range(ts_content, SupportedLanguage::TypeScript, &region);

    let has_validate = symbols.iter().any(|s| s.name == "validateJWT");
    assert!(has_validate, "Should extract validateJWT. Got: {:?}",
        symbols.iter().map(|s| &s.name).collect::<Vec<_>>());
}
