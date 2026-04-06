use std::collections::HashMap;
use harmony_core::types::*;
use tree_sitter::{Parser, Language, Node, Query, QueryCursor};

pub struct TreeSitterAnalyzer {
    parsers: HashMap<SupportedLanguage, Parser>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SupportedLanguage {
    TypeScript,
    JavaScript,
    Rust,
}

impl TreeSitterAnalyzer {
    pub fn new() -> Self {
        let mut parsers = HashMap::new();

        // TypeScript parser
        let mut ts_parser = Parser::new();
        let _ = ts_parser.set_language(&tree_sitter_typescript::language_typescript());
        parsers.insert(SupportedLanguage::TypeScript, ts_parser);

        // JavaScript parser
        let mut js_parser = Parser::new();
        let _ = js_parser.set_language(&tree_sitter_javascript::language());
        parsers.insert(SupportedLanguage::JavaScript, js_parser);

        // Rust parser
        let mut rs_parser = Parser::new();
        let _ = rs_parser.set_language(&tree_sitter_rust::language());
        parsers.insert(SupportedLanguage::Rust, rs_parser);

        Self { parsers }
    }

    /// Detect language from file extension.
    pub fn detect_language(file_path: &str) -> Option<SupportedLanguage> {
        let ext = file_path.rsplit('.').next()?;
        match ext {
            "ts" | "tsx" => Some(SupportedLanguage::TypeScript),
            "js" | "jsx" | "mjs" | "cjs" => Some(SupportedLanguage::JavaScript),
            "rs" => Some(SupportedLanguage::Rust),
            _ => None,
        }
    }

    /// Parse file content and extract all top-level symbols in the given TextRange.
    pub fn extract_symbols_in_range(
        &mut self,
        content: &str,
        language: SupportedLanguage,
        region: &TextRange,
    ) -> Vec<AffectedSymbol> {
        let parser = match self.parsers.get_mut(&language) {
            Some(p) => p,
            None => return Vec::new(),
        };

        let tree = match parser.parse(content, None) {
            Some(t) => t,
            None => return Vec::new(),
        };

        let query_str = get_query_string(&language);
        let ts_lang = get_ts_language(&language);
        let query = match Query::new(&ts_lang, query_str) {
            Ok(q) => q,
            Err(e) => {
                tracing::warn!("Tree-sitter query error: {}", e);
                return Vec::new();
            }
        };

        let mut cursor = QueryCursor::new();
        let root = tree.root_node();
        let matches = cursor.matches(&query, root, content.as_bytes());

        let mut symbols = Vec::new();
        for m in matches {
            for capture in m.captures {
                let node = capture.node;
                let start_line = node.start_position().row as u32;
                let end_line = node.end_position().row as u32;

                // Check if this node overlaps with the region
                let node_range = TextRange {
                    start_line,
                    end_line,
                    start_col: 0,
                    end_col: 0,
                };

                if node_range.overlaps(region) {
                    let name = node.utf8_text(content.as_bytes()).unwrap_or("").to_string();
                    let capture_name = query.capture_names()[capture.index as usize];
                    let kind = capture_name_to_symbol_kind(capture_name);

                    // Only collect name captures (not the parent container nodes)
                    if capture_name == "name" || capture_name == "import_path" {
                        symbols.push(AffectedSymbol {
                            name,
                            kind,
                            file_path: String::new(), // caller fills this in
                            line: start_line,
                            impact: SymbolImpact::DirectlyModified,
                        });
                    }
                }
            }
        }

        symbols
    }

    /// Diff two versions of a file (before/after a change).
    /// Returns symbols that changed.
    pub fn diff_symbols(
        &mut self,
        before: &str,
        after: &str,
        language: SupportedLanguage,
    ) -> Vec<AffectedSymbol> {
        let full_range = TextRange {
            start_line: 0,
            end_line: u32::MAX,
            start_col: 0,
            end_col: 0,
        };

        let symbols_before = self.extract_symbols_in_range(before, language.clone(), &full_range);
        let symbols_after = self.extract_symbols_in_range(after, language, &full_range);

        let mut changed = Vec::new();

        // Find added or modified symbols
        for sym_after in &symbols_after {
            let found = symbols_before.iter().find(|s| s.name == sym_after.name);
            if found.is_none() {
                changed.push(sym_after.clone());
            } else if let Some(before_sym) = found {
                if before_sym.line != sym_after.line {
                    changed.push(sym_after.clone());
                }
            }
        }

        // Find removed symbols
        for sym_before in &symbols_before {
            let found = symbols_after.iter().find(|s| s.name == sym_before.name);
            if found.is_none() {
                let mut removed = sym_before.clone();
                removed.impact = SymbolImpact::DirectlyModified;
                changed.push(removed);
            }
        }

        changed
    }
}

/// Check if two symbol kinds are the same variant
fn symbol_kinds_match(a: &SymbolKind, b: &SymbolKind) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

fn get_ts_language(lang: &SupportedLanguage) -> Language {
    match lang {
        SupportedLanguage::TypeScript => tree_sitter_typescript::language_typescript(),
        SupportedLanguage::JavaScript => tree_sitter_javascript::language(),
        SupportedLanguage::Rust => tree_sitter_rust::language(),
    }
}

fn get_query_string(lang: &SupportedLanguage) -> &'static str {
    match lang {
        SupportedLanguage::TypeScript | SupportedLanguage::JavaScript => {
            r#"
            (function_declaration name: (identifier) @name) @func
            (method_definition name: (property_identifier) @name) @method
            (class_declaration name: (type_identifier) @name) @class
            (variable_declarator name: (identifier) @name) @var
            (import_statement source: (string) @import_path) @import
            "#
        }
        SupportedLanguage::Rust => {
            r#"
            (function_item name: (identifier) @name) @func
            (impl_item type: (type_identifier) @name) @class
            (struct_item name: (type_identifier) @name) @class
            (enum_item name: (type_identifier) @name) @class
            "#
        }
    }
}

fn capture_name_to_symbol_kind(capture_name: &str) -> SymbolKind {
    match capture_name {
        "func" => SymbolKind::Function,
        "method" => SymbolKind::Method,
        "class" => SymbolKind::Class,
        "var" | "name" => SymbolKind::Variable,
        "import" | "import_path" => SymbolKind::Import,
        _ => SymbolKind::Variable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_language_typescript() {
        assert_eq!(TreeSitterAnalyzer::detect_language("src/auth.ts"), Some(SupportedLanguage::TypeScript));
        assert_eq!(TreeSitterAnalyzer::detect_language("component.tsx"), Some(SupportedLanguage::TypeScript));
    }

    #[test]
    fn test_detect_language_javascript() {
        assert_eq!(TreeSitterAnalyzer::detect_language("index.js"), Some(SupportedLanguage::JavaScript));
        assert_eq!(TreeSitterAnalyzer::detect_language("index.jsx"), Some(SupportedLanguage::JavaScript));
    }

    #[test]
    fn test_detect_language_rust() {
        assert_eq!(TreeSitterAnalyzer::detect_language("main.rs"), Some(SupportedLanguage::Rust));
    }

    #[test]
    fn test_detect_language_unknown() {
        assert_eq!(TreeSitterAnalyzer::detect_language("file.py"), None);
    }

    #[test]
    fn test_extract_typescript_function() {
        let mut analyzer = TreeSitterAnalyzer::new();
        let content = r#"function validateJWT(token: string): boolean {
  const payload = jwt.verify(token, SECRET);
  return true;
}

function logout() {
  session.destroy();
}
"#;
        let region = TextRange { start_line: 0, end_line: 3, start_col: 0, end_col: 0 };
        let symbols = analyzer.extract_symbols_in_range(content, SupportedLanguage::TypeScript, &region);

        let has_validate = symbols.iter().any(|s| s.name == "validateJWT");
        assert!(has_validate, "Should find validateJWT function. Symbols: {:?}", symbols.iter().map(|s| &s.name).collect::<Vec<_>>());
    }

    #[test]
    fn test_extract_rust_function() {
        let mut analyzer = TreeSitterAnalyzer::new();
        let content = r#"fn main() {
    println!("Hello");
}

fn helper() -> bool {
    true
}
"#;
        let region = TextRange { start_line: 0, end_line: 2, start_col: 0, end_col: 0 };
        let symbols = analyzer.extract_symbols_in_range(content, SupportedLanguage::Rust, &region);
        let has_main = symbols.iter().any(|s| s.name == "main");
        assert!(has_main, "Should find main function. Symbols: {:?}", symbols.iter().map(|s| &s.name).collect::<Vec<_>>());
    }
}
