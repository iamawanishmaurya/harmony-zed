//! Live backend smoke test — requires HARMONY_GITHUB_TOKEN env var.
//! 
//! Run with: cargo test -p harmony-core --test live_backend_smoke -- --ignored --nocapture

use harmony_core::config::{NegotiationConfig, AgentsConfig};
use harmony_core::negotiation::call_negotiation_llm;

fn get_github_token() -> Option<String> {
    std::env::var("HARMONY_GITHUB_TOKEN").ok()
}

/// Smoke test: calls the GitHub Models API (OpenAI-compatible) with a
/// minimal negotiation prompt and verifies we get a valid NegotiationResult back.
#[tokio::test]
#[ignore] // Only run when explicitly requested with --ignored
async fn live_github_models_openai_smoke() {
    let token = match get_github_token() {
        Some(t) => t,
        None => {
            eprintln!("SKIPPED: HARMONY_GITHUB_TOKEN not set");
            return;
        }
    };

    println!("Using GitHub Models API (OpenAI-compatible)...");

    let config = NegotiationConfig {
        negotiation_backend: "openai".into(),
        api_key: Some(token),
        model: Some("gpt-4o-mini".into()),
        base_url: Some("https://models.inference.ai.azure.com".into()),
    };
    let agents = AgentsConfig { registry: vec![] };

    let prompt = r#"You are a code merge mediator. Two changes overlap on auth.ts:
Change A: Added JWT validation (human:awanish)  
Change B: Added Redis caching (agent:architect-01)

Respond ONLY with valid JSON:
{
  "proposed_diff": "--- a/auth.ts\n+++ b/auth.ts\n@@ -1,3 +1,5 @@\n+merged",
  "rationale": "Combined both changes.",
  "confidence": 0.8,
  "memory_notes": ["Merged JWT + Redis"]
}"#;

    let result = call_negotiation_llm(prompt.to_string(), &config, &agents).await;

    match &result {
        Ok(neg) => {
            println!("✅ SUCCESS!");
            println!("  proposed_diff: {} chars", neg.proposed_diff.len());
            println!("  rationale: {}", neg.rationale);
            println!("  confidence: {}", neg.confidence);
            println!("  memory_notes: {:?}", neg.memory_notes);
            assert!(!neg.proposed_diff.is_empty(), "proposed_diff should not be empty");
            assert!(!neg.rationale.is_empty(), "rationale should not be empty");
        }
        Err(e) => {
            eprintln!("❌ FAILED: {:?}", e);
            panic!("Live backend test failed: {}", e);
        }
    }
}
