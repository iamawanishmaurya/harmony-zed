use crate::types::*;
use crate::config::{NegotiationConfig, AgentsConfig};
use crate::errors::HarmonyError;

/// Build the negotiation prompt for the LLM (§12 Template B).
pub fn build_negotiation_prompt(
    overlap: &OverlapEvent,
    impact: &ImpactGraph,
    memory: &[(MemoryRecord, f32)],
) -> String {
    let memory_ctx = memory.iter()
        .map(|(r, score)| format!("- [relevance:{:.2}] {}", score, r.content))
        .collect::<Vec<_>>().join("\n");

    let affected_symbols = impact.affected_symbols.iter()
        .map(|s| format!("{} ({})", s.name, format_impact(&s.impact)))
        .collect::<Vec<_>>().join(", ");

    format!(r#"You are a code merge mediator for a software project.
Two changes were made to the same region of `{file}` simultaneously.
Your job is to produce a single merged change that preserves the intent of both.

## Change A
Author: {author_a}
Task: {task_a}
Diff:
```diff
{diff_a}
```

## Change B
Author: {author_b}
Task: {task_b}
Diff:
```diff
{diff_b}
```

## Impact Analysis
{impact_summary}
Affected symbols: {affected_symbols}

## Relevant Team Memory
{memory_ctx}

## Your Task
Produce a merged unified diff that:
1. Preserves the intent of BOTH changes
2. Does not break existing functionality
3. Follows the existing code style in the file
4. Is as minimal as possible

Respond ONLY with valid JSON in this exact format, no other text:
{{
  "proposed_diff": "--- a/{file}\n+++ b/{file}\n@@ ... @@\n...",
  "rationale": "One or two sentences explaining the merge decision.",
  "confidence": 0.85,
  "memory_notes": ["Short note to add to team memory about this decision"]
}}"#,
        file = overlap.file_path,
        author_a = overlap.change_a.actor_id.0,
        task_a = overlap.change_a.task_prompt.as_deref().unwrap_or("(no task)"),
        diff_a = overlap.change_a.diff_unified,
        author_b = overlap.change_b.actor_id.0,
        task_b = overlap.change_b.task_prompt.as_deref().unwrap_or("(no task)"),
        diff_b = overlap.change_b.diff_unified,
        impact_summary = impact.summary,
        affected_symbols = affected_symbols,
        memory_ctx = if memory_ctx.is_empty() { "(no relevant memory)".to_string() } else { memory_ctx },
    )
}

/// Parse a NegotiationResult from the LLM's JSON response.
pub fn parse_negotiation_result(
    overlap_id: uuid::Uuid,
    json_str: &str,
) -> Result<NegotiationResult, HarmonyError> {
    let value: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| HarmonyError::NegotiationInvalidResponse(
            format!("Invalid JSON: {}", e)
        ))?;

    let proposed_diff = value.get("proposed_diff")
        .and_then(|v| v.as_str())
        .ok_or_else(|| HarmonyError::NegotiationInvalidResponse(
            "Missing 'proposed_diff' field".to_string()
        ))?
        .to_string();

    let rationale = value.get("rationale")
        .and_then(|v| v.as_str())
        .unwrap_or("No rationale provided")
        .to_string();

    let confidence = value.get("confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.5) as f32;

    let memory_notes: Vec<String> = value.get("memory_notes")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    Ok(NegotiationResult {
        overlap_id,
        proposed_diff,
        rationale,
        confidence: confidence.clamp(0.0, 1.0),
        memory_notes,
    })
}

// ── Multi-Backend LLM Negotiation ─────────────────────────────────────────────

/// Call the configured LLM backend to negotiate an overlap resolution.
///
/// Routes to one of 4 backends based on `config.negotiation_backend`:
/// - `"openai"` — OpenAI-compatible API (also Copilot, Ollama, LM Studio)
/// - `"anthropic"` — Anthropic Claude API
/// - `"agent"` — Delegate to a spawned ACP agent
/// - `"disabled"` — Return NegotiationNotConfigured immediately
pub async fn call_negotiation_llm(
    prompt: String,
    config: &NegotiationConfig,
    agents: &AgentsConfig,
) -> Result<NegotiationResult, HarmonyError> {
    match config.negotiation_backend.as_str() {
        "openai" => call_openai_backend(&prompt, config).await,
        "anthropic" => call_anthropic_backend(&prompt, config).await,
        "agent" => call_agent_backend(&prompt, agents).await,
        "disabled" => Err(HarmonyError::NegotiationNotConfigured),
        other => Err(HarmonyError::NegotiationInvalidResponse(
            format!("Unknown negotiation backend: '{}'. Use 'openai', 'anthropic', 'agent', or 'disabled'.", other)
        )),
    }
}

/// OpenAI-compatible backend (also handles Copilot, Ollama, LM Studio).
async fn call_openai_backend(
    prompt: &str,
    config: &NegotiationConfig,
) -> Result<NegotiationResult, HarmonyError> {
    let base_url = config.base_url.as_deref().unwrap_or("https://api.openai.com/v1");
    let model = config.model.as_deref().unwrap_or("gpt-4o");
    let api_key = config.api_key.as_deref().ok_or(HarmonyError::NegotiationNotConfigured)?;

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0.3,
        "response_format": {"type": "json_object"}
    });

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| HarmonyError::NegotiationInvalidResponse(
            format!("HTTP request failed: {}", e)
        ))?;

    let status = response.status();
    let response_text = response.text().await
        .map_err(|e| HarmonyError::NegotiationInvalidResponse(
            format!("Failed to read response body: {}", e)
        ))?;

    if !status.is_success() {
        return Err(HarmonyError::NegotiationInvalidResponse(
            format!("OpenAI API returned {}: {}", status, response_text)
        ));
    }

    // Parse OpenAI response: .choices[0].message.content
    let resp_json: serde_json::Value = serde_json::from_str(&response_text)
        .map_err(|e| HarmonyError::NegotiationInvalidResponse(
            format!("Invalid response JSON: {}", e)
        ))?;

    let content = resp_json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| HarmonyError::NegotiationInvalidResponse(
            "Missing choices[0].message.content in OpenAI response".to_string()
        ))?;

    parse_negotiation_result(uuid::Uuid::new_v4(), content)
}

/// Anthropic Claude backend.
async fn call_anthropic_backend(
    prompt: &str,
    config: &NegotiationConfig,
) -> Result<NegotiationResult, HarmonyError> {
    let base_url = config.base_url.as_deref().unwrap_or("https://api.anthropic.com");
    let model = config.model.as_deref().unwrap_or("claude-sonnet-4-6");
    let api_key = config.api_key.as_deref().ok_or(HarmonyError::NegotiationNotConfigured)?;

    let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));

    let body = serde_json::json!({
        "model": model,
        "max_tokens": 2048,
        "messages": [{"role": "user", "content": prompt}]
    });

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| HarmonyError::NegotiationInvalidResponse(
            format!("HTTP request failed: {}", e)
        ))?;

    let status = response.status();
    let response_text = response.text().await
        .map_err(|e| HarmonyError::NegotiationInvalidResponse(
            format!("Failed to read response body: {}", e)
        ))?;

    if !status.is_success() {
        return Err(HarmonyError::NegotiationInvalidResponse(
            format!("Anthropic API returned {}: {}", status, response_text)
        ));
    }

    // Parse Anthropic response: .content[0].text
    let resp_json: serde_json::Value = serde_json::from_str(&response_text)
        .map_err(|e| HarmonyError::NegotiationInvalidResponse(
            format!("Invalid response JSON: {}", e)
        ))?;

    let content = resp_json
        .get("content")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("text"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| HarmonyError::NegotiationInvalidResponse(
            "Missing content[0].text in Anthropic response".to_string()
        ))?;

    parse_negotiation_result(uuid::Uuid::new_v4(), content)
}

/// Delegate negotiation to a registered ACP agent.
async fn call_agent_backend(
    prompt: &str,
    agents: &AgentsConfig,
) -> Result<NegotiationResult, HarmonyError> {
    let agent = agents.registry.first()
        .ok_or(HarmonyError::NegotiationNotConfigured)?;

    let url = format!("{}/negotiate", agent.endpoint.trim_end_matches('/'));

    let body = serde_json::json!({
        "prompt": prompt
    });

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| HarmonyError::NegotiationInvalidResponse(
            format!("Agent {} unreachable: {}", agent.name, e)
        ))?;

    let status = response.status();
    let response_text = response.text().await
        .map_err(|e| HarmonyError::NegotiationInvalidResponse(
            format!("Failed to read agent response: {}", e)
        ))?;

    if !status.is_success() {
        return Err(HarmonyError::NegotiationInvalidResponse(
            format!("Agent {} returned {}: {}", agent.name, status, response_text)
        ));
    }

    parse_negotiation_result(uuid::Uuid::new_v4(), &response_text)
}

/// Decompose a spawn prompt into agent roles.
/// v0.1 uses keyword matching, NOT LLM, for spawn decomposition.
pub fn decompose_spawn_prompt(_prompt: &str) -> Vec<AgentRole> {
    // Always spawn these 3 roles for any prompt in v0.1
    // v0.2 will use LLM to customize roles based on task
    vec![
        AgentRole {
            name: "Architect".into(),
            avatar_key: "agent-architect".into(),
            description: "Plans the implementation approach".into(),
        },
        AgentRole {
            name: "Coder".into(),
            avatar_key: "agent-coder".into(),
            description: "Writes the implementation code".into(),
        },
        AgentRole {
            name: "Tester".into(),
            avatar_key: "agent-tester".into(),
            description: "Writes and validates tests".into(),
        },
    ]
}

fn format_impact(impact: &SymbolImpact) -> &'static str {
    match impact {
        SymbolImpact::DirectlyModified => "directly modified",
        SymbolImpact::CallerOfModified => "caller",
        SymbolImpact::CalleeOfModified => "callee",
        SymbolImpact::SharedState => "shared state",
        SymbolImpact::ImportDependency => "import dep",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    #[test]
    fn test_build_negotiation_prompt() {
        let overlap = OverlapEvent {
            id: Uuid::new_v4(),
            file_path: "src/auth.ts".to_string(),
            region_a: TextRange { start_line: 44, end_line: 67, start_col: 0, end_col: 0 },
            region_b: TextRange { start_line: 52, end_line: 71, start_col: 0, end_col: 0 },
            change_a: ProvenanceTag {
                id: Uuid::new_v4(),
                actor_id: ActorId("human:awanish".to_string()),
                actor_kind: ActorKind::Human,
                task_id: None,
                task_prompt: Some("Fix JWT validation".to_string()),
                timestamp: Utc::now(),
                file_path: "src/auth.ts".to_string(),
                region: TextRange { start_line: 44, end_line: 67, start_col: 0, end_col: 0 },
                mode: AgentMode::Shadow,
                diff_unified: "@@ -44,5 +44,8 @@\n+const x = 1;".to_string(),
                session_id: Uuid::new_v4(),
            },
            change_b: ProvenanceTag {
                id: Uuid::new_v4(),
                actor_id: ActorId("agent:architect-01".to_string()),
                actor_kind: ActorKind::Agent,
                task_id: None,
                task_prompt: Some("Add Redis caching".to_string()),
                timestamp: Utc::now(),
                file_path: "src/auth.ts".to_string(),
                region: TextRange { start_line: 52, end_line: 71, start_col: 0, end_col: 0 },
                mode: AgentMode::Shadow,
                diff_unified: "@@ -52,3 +52,6 @@\n+const cache = {};".to_string(),
                session_id: Uuid::new_v4(),
            },
            detected_at: Utc::now(),
            status: OverlapStatus::Pending,
        };

        let impact = ImpactGraph {
            overlap_id: overlap.id,
            affected_symbols: vec![
                AffectedSymbol {
                    name: "validateJWT".to_string(),
                    kind: SymbolKind::Function,
                    file_path: "src/auth.ts".to_string(),
                    line: 44,
                    impact: SymbolImpact::DirectlyModified,
                },
            ],
            summary: "Both changes modify validateJWT".to_string(),
            complexity: ImpactComplexity::Moderate,
            sandbox_required: false,
            sandbox_result: None,
        };

        let prompt = build_negotiation_prompt(&overlap, &impact, &[]);
        assert!(prompt.contains("src/auth.ts"));
        assert!(prompt.contains("human:awanish"));
        assert!(prompt.contains("agent:architect-01"));
        assert!(prompt.contains("validateJWT"));
        assert!(prompt.contains("proposed_diff"));
    }

    #[test]
    fn test_parse_negotiation_result() {
        let json = r#"{
            "proposed_diff": "--- a/src/auth.ts\n+++ b/src/auth.ts\n@@ -44,5 +44,10 @@\n+merged code",
            "rationale": "Combined both changes preserving intent.",
            "confidence": 0.85,
            "memory_notes": ["Merged JWT fix with Redis caching"]
        }"#;

        let result = parse_negotiation_result(Uuid::new_v4(), json).unwrap();
        assert!(result.proposed_diff.contains("merged code"));
        assert_eq!(result.confidence, 0.85);
        assert_eq!(result.memory_notes.len(), 1);
    }

    #[test]
    fn test_parse_negotiation_result_invalid_json() {
        let result = parse_negotiation_result(Uuid::new_v4(), "not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_decompose_spawn_prompt() {
        let roles = decompose_spawn_prompt("Build auth flow with rate limiting");
        assert_eq!(roles.len(), 3);
        assert_eq!(roles[0].name, "Architect");
        assert_eq!(roles[1].name, "Coder");
        assert_eq!(roles[2].name, "Tester");
    }
}
