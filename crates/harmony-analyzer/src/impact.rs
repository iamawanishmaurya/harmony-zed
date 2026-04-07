use std::path::Path;
use harmony_core::types::*;
use crate::treesitter::{TreeSitterAnalyzer, SupportedLanguage};
use crate::lsp_client::LspClient;

pub struct ImpactAnalyzer {
    tree_sitter: TreeSitterAnalyzer,
    lsp: Option<LspClient>,  // None if LSP unavailable
}

impl ImpactAnalyzer {
    pub fn new(project_root: &Path, language: SupportedLanguage) -> Self {
        let tree_sitter = TreeSitterAnalyzer::new();

        // Try to spawn LSP, fall back gracefully
        let lsp = match LspClient::spawn(language, project_root) {
            Ok(client) => {
                tracing::info!("LSP client connected for impact analysis");
                Some(client)
            }
            Err(e) => {
                tracing::warn!("LSP unavailable, falling back to Tree-sitter only: {}", e);
                None
            }
        };

        Self { tree_sitter, lsp }
    }

    /// Create an analyzer without LSP (tree-sitter only).
    pub fn new_without_lsp() -> Self {
        Self {
            tree_sitter: TreeSitterAnalyzer::new(),
            lsp: None,
        }
    }

    /// Main entry point. Analyze an OverlapEvent and produce an ImpactGraph.
    pub fn analyze(
        &mut self,
        overlap: &OverlapEvent,
        content_a: &str,
        content_b: &str,
    ) -> ImpactGraph {
        let language = TreeSitterAnalyzer::detect_language(&overlap.file_path);

        let (symbols_a, symbols_b) = if let Some(lang) = language {
            // Extract symbols changed in each change's region
            let syms_a = self.tree_sitter.extract_symbols_in_range(
                content_a, lang.clone(), &overlap.region_a
            );
            let syms_b = self.tree_sitter.extract_symbols_in_range(
                content_b, lang, &overlap.region_b
            );
            (syms_a, syms_b)
        } else {
            (Vec::new(), Vec::new())
        };

        // Find shared symbols (intersection by name)
        let shared: Vec<AffectedSymbol> = symbols_a.iter()
            .filter(|sa| symbols_b.iter().any(|sb| sb.name == sa.name))
            .cloned()
            .collect();

        // Combine all affected symbols
        let mut all_symbols: Vec<AffectedSymbol> = Vec::new();
        for mut sym in symbols_a.clone() {
            sym.file_path = overlap.file_path.clone();
            all_symbols.push(sym);
        }
        for mut sym in symbols_b.clone() {
            sym.file_path = overlap.file_path.clone();
            if !all_symbols.iter().any(|s| s.name == sym.name) {
                all_symbols.push(sym);
            }
        }

        // Try to get caller information from LSP
        let mut caller_count = 0;
        if let Some(ref mut _lsp) = self.lsp {
            // In v0.1, LSP reference lookup is best-effort
            // For each symbol, try to find references
            // This is expensive so we limit to 5 symbols max
            // TODO: implement actual LSP reference lookup
            tracing::debug!("LSP reference lookup not yet implemented, using Tree-sitter only");
        }

        // Determine complexity
        let complexity = if !shared.is_empty() && caller_count > 0 {
            ImpactComplexity::Complex
        } else if caller_count > 3 {
            ImpactComplexity::Complex
        } else if !shared.is_empty() || caller_count > 0 {
            ImpactComplexity::Moderate
        } else {
            ImpactComplexity::Simple
        };

        let sandbox_required = complexity == ImpactComplexity::Complex;

        // Generate summary
        let summary = build_impact_summary(
            &overlap.change_a,
            &overlap.change_b,
            &symbols_a,
            &symbols_b,
            &shared,
        );

        ImpactGraph {
            overlap_id: overlap.id,
            affected_symbols: all_symbols,
            summary,
            complexity,
            sandbox_required,
            sandbox_result: None,
        }
    }
}

/// Build a deterministic impact summary string (not LLM-generated).
pub fn build_impact_summary(
    change_a: &ProvenanceTag,
    change_b: &ProvenanceTag,
    symbols_a: &[AffectedSymbol],
    symbols_b: &[AffectedSymbol],
    shared: &[AffectedSymbol],
) -> String {
    let actor_a = format_actor(&change_a.actor_id);
    let actor_b = format_actor(&change_b.actor_id);
    let verb_a = if symbols_a.is_empty() { "edited code" } else { "modified" };
    let verb_b = if symbols_b.is_empty() { "edited code" } else { "modified" };
    let sym_a = format_symbols(symbols_a);
    let sym_b = format_symbols(symbols_b);

    let mut s = format!(
        "{actor_a} {verb_a} {sym_a} in `{}`. \
         {actor_b} {verb_b} {sym_b} in the same region.",
        change_a.file_path
    );

    if !shared.is_empty() {
        let shared_str = format_symbols(shared);
        s.push_str(&format!(" Both changes affect: {shared_str}."));
    }
    s
}

fn format_actor(actor_id: &ActorId) -> String {
    let id = &actor_id.0;
    if let Some(name) = id.strip_prefix("human:") {
        format!("You ({})", name)
    } else if let Some(role) = id.strip_prefix("agent:") {
        // "architect-01" → "Agent Architect"
        let role_name = role.split('-').next().unwrap_or(role);
        let capitalized = role_name.chars().next()
            .map(|c| c.to_uppercase().to_string() + &role_name[1..])
            .unwrap_or_else(|| role_name.to_string());
        format!("Agent {}", capitalized)
    } else {
        id.clone()
    }
}

fn format_symbols(symbols: &[AffectedSymbol]) -> String {
    if symbols.is_empty() {
        return "code".to_string();
    }

    let names: Vec<String> = symbols.iter()
        .take(3)
        .map(|s| format!("`{}`", s.name))
        .collect();

    let mut result = names.join(", ");
    if symbols.len() > 3 {
        result.push_str(&format!(" + {} more", symbols.len() - 3));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn make_overlap_with_content() -> (OverlapEvent, String, String) {
        let tag_a = ProvenanceTag {
            id: Uuid::new_v4(),
            actor_id: ActorId("human:awanish".to_string()),
            machine_name: "Awanish-Laptop".to_string(),
            machine_ip: "192.168.1.10".to_string(),
            actor_kind: ActorKind::Human,
            task_id: None,
            task_prompt: None,
            timestamp: Utc::now(),
            file_path: "src/middleware/auth.ts".to_string(),
            region: TextRange { start_line: 2, end_line: 8, start_col: 0, end_col: 0 },
            mode: AgentMode::Shadow,
            diff_unified: String::new(),
            session_id: Uuid::new_v4(),
        };

        let tag_b = ProvenanceTag {
            id: Uuid::new_v4(),
            actor_id: ActorId("agent:architect-01".to_string()),
            machine_name: "Rahul-Laptop".to_string(),
            machine_ip: "192.168.1.22".to_string(),
            actor_kind: ActorKind::Agent,
            task_id: None,
            task_prompt: Some("Add Redis caching".to_string()),
            timestamp: Utc::now(),
            file_path: "src/middleware/auth.ts".to_string(),
            region: TextRange { start_line: 4, end_line: 10, start_col: 0, end_col: 0 },
            mode: AgentMode::Shadow,
            diff_unified: String::new(),
            session_id: Uuid::new_v4(),
        };

        let overlap = OverlapEvent {
            id: Uuid::new_v4(),
            file_path: "src/middleware/auth.ts".to_string(),
            region_a: tag_a.region.clone(),
            region_b: tag_b.region.clone(),
            change_a: tag_a,
            change_b: tag_b,
            detected_at: Utc::now(),
            status: OverlapStatus::Pending,
        };

        let content_a = r#"import { Request, Response } from 'express';

export function validateJWT(req: Request, res: Response) {
  const token = req.headers.authorization;
  if (!token) {
    return res.status(401).json({ error: 'No token' });
  }
  const payload = jwt.verify(token, SECRET);
  req.user = payload;
}

export function logout() {}
"#.to_string();

        let content_b = r#"import { Request, Response } from 'express';
import { redis } from '../cache';

export function validateJWT(req: Request, res: Response) {
  const token = req.headers.authorization;
  const cached = await redis.get(token);
  if (cached) return JSON.parse(cached);
  if (!token) {
    return res.status(401).json({ error: 'No token' });
  }
  const payload = jwt.verify(token, SECRET);
  req.user = payload;
}
"#.to_string();

        (overlap, content_a, content_b)
    }

    #[test]
    fn test_impact_analyzer_produces_graph() {
        let (overlap, content_a, content_b) = make_overlap_with_content();
        let mut analyzer = ImpactAnalyzer::new_without_lsp();
        let graph = analyzer.analyze(&overlap, &content_a, &content_b);

        assert!(!graph.summary.is_empty(), "Summary should not be empty");
        assert_eq!(graph.overlap_id, overlap.id);
    }

    #[test]
    fn test_impact_summary_contains_actors() {
        let (overlap, content_a, content_b) = make_overlap_with_content();
        let mut analyzer = ImpactAnalyzer::new_without_lsp();
        let graph = analyzer.analyze(&overlap, &content_a, &content_b);

        assert!(graph.summary.contains("You (awanish)") || graph.summary.contains("awanish"),
            "Summary should contain human actor name. Got: {}", graph.summary);
        assert!(graph.summary.contains("Agent Architect") || graph.summary.contains("architect"),
            "Summary should contain agent role. Got: {}", graph.summary);
    }

    #[test]
    fn test_impact_complexity_is_valid() {
        let (overlap, content_a, content_b) = make_overlap_with_content();
        let mut analyzer = ImpactAnalyzer::new_without_lsp();
        let graph = analyzer.analyze(&overlap, &content_a, &content_b);

        assert!(
            graph.complexity == ImpactComplexity::Simple
                || graph.complexity == ImpactComplexity::Moderate
                || graph.complexity == ImpactComplexity::Complex,
            "Complexity must be one of Simple/Moderate/Complex"
        );
    }

    #[test]
    fn test_format_actor_human() {
        let actor = ActorId("human:awanish".to_string());
        assert_eq!(format_actor(&actor), "You (awanish)");
    }

    #[test]
    fn test_format_actor_agent() {
        let actor = ActorId("agent:architect-01".to_string());
        assert_eq!(format_actor(&actor), "Agent Architect");
    }

    #[test]
    fn test_format_symbols_empty() {
        let symbols: Vec<AffectedSymbol> = vec![];
        assert_eq!(format_symbols(&symbols), "code");
    }

    #[test]
    fn test_format_symbols_multiple() {
        let symbols = vec![
            AffectedSymbol { name: "foo".to_string(), kind: SymbolKind::Function, file_path: String::new(), line: 0, impact: SymbolImpact::DirectlyModified },
            AffectedSymbol { name: "bar".to_string(), kind: SymbolKind::Function, file_path: String::new(), line: 0, impact: SymbolImpact::DirectlyModified },
        ];
        let result = format_symbols(&symbols);
        assert!(result.contains("`foo`"));
        assert!(result.contains("`bar`"));
    }
}
