//! Analyzer integration tests.
//! Tests Tree-sitter analysis on real TypeScript/Rust content.

use harmony_analyzer::treesitter::{TreeSitterAnalyzer, SupportedLanguage};
use harmony_core::types::*;

#[test]
fn test_typescript_analysis_golden_path() {
    let mut analyzer = TreeSitterAnalyzer::new();
    let content = r#"import { Request, Response } from 'express';

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

    let region = TextRange { start_line: 2, end_line: 10, start_col: 0, end_col: 0 };
    let symbols = analyzer.extract_symbols_in_range(content, SupportedLanguage::TypeScript, &region);

    let has_validate = symbols.iter().any(|s| s.name == "validateJWT");
    assert!(has_validate, "Should find validateJWT. Found: {:?}",
        symbols.iter().map(|s| &s.name).collect::<Vec<_>>());
}

#[test]
fn test_rust_analysis() {
    let mut analyzer = TreeSitterAnalyzer::new();
    let content = r#"pub fn detect_overlaps(new_tag: &Tag, recent: &[Tag]) -> Vec<Overlap> {
    let mut overlaps = Vec::new();
    for tag in recent {
        if tag.overlaps(new_tag) {
            overlaps.push(Overlap::new(new_tag, tag));
        }
    }
    overlaps
}

fn helper() -> bool {
    true
}
"#;

    let region = TextRange { start_line: 0, end_line: 8, start_col: 0, end_col: 0 };
    let symbols = analyzer.extract_symbols_in_range(content, SupportedLanguage::Rust, &region);
    let has_detect = symbols.iter().any(|s| s.name == "detect_overlaps");
    assert!(has_detect, "Should find detect_overlaps. Found: {:?}",
        symbols.iter().map(|s| &s.name).collect::<Vec<_>>());
}

#[test]
fn test_language_detection() {
    assert!(matches!(TreeSitterAnalyzer::detect_language("src/auth.ts"), Some(SupportedLanguage::TypeScript)));
    assert!(matches!(TreeSitterAnalyzer::detect_language("main.rs"), Some(SupportedLanguage::Rust)));
    assert!(matches!(TreeSitterAnalyzer::detect_language("index.js"), Some(SupportedLanguage::JavaScript)));
    assert!(TreeSitterAnalyzer::detect_language("script.py").is_none());
}
