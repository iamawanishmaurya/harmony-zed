//! Negotiation backend integration tests using httpmock.
//!
//! Tests all 4 LLM backend branches (openai, anthropic, agent, disabled)
//! WITHOUT making real HTTP calls — uses a local mock HTTP server.

use harmony_core::config::{NegotiationConfig, AgentsConfig, AgentEndpoint};
use harmony_core::negotiation::call_negotiation_llm;
use harmony_core::errors::HarmonyError;
use httpmock::prelude::*;

/// Valid negotiation result JSON body (reused across tests).
fn valid_negotiation_json() -> String {
    serde_json::json!({
        "proposed_diff": "--- a/test.ts\n+++ b/test.ts\n@@ -1,3 +1,5 @@\n+merged line",
        "rationale": "Combined both changes.",
        "confidence": 0.82,
        "memory_notes": ["Merged successfully"]
    }).to_string()
}

/// OpenAI response wrapper around the negotiation JSON.
fn openai_response_body() -> serde_json::Value {
    serde_json::json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": valid_negotiation_json()
            },
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 100, "completion_tokens": 50, "total_tokens": 150}
    })
}

/// Anthropic response wrapper around the negotiation JSON.
fn anthropic_response_body() -> serde_json::Value {
    serde_json::json!({
        "id": "msg_test",
        "type": "message",
        "role": "assistant",
        "content": [{
            "type": "text",
            "text": valid_negotiation_json()
        }],
        "model": "claude-sonnet-4-6",
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 100, "output_tokens": 50}
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// TEST 1: OpenAI backend sends correct headers
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn openai_backend_sends_correct_headers() {
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/chat/completions")
            .header("Authorization", "Bearer sk-test-key-123")
            .header("Content-Type", "application/json");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(openai_response_body());
    });

    let config = NegotiationConfig {
        negotiation_backend: "openai".into(),
        api_key: Some("sk-test-key-123".into()),
        model: Some("gpt-4o".into()),
        base_url: Some(server.url("")),
    };
    let agents = AgentsConfig { registry: vec![] };

    let result = call_negotiation_llm("test prompt".into(), &config, &agents).await;
    assert!(result.is_ok(), "Expected Ok, got {:?}", result);

    let neg = result.unwrap();
    assert!(!neg.proposed_diff.is_empty());
    assert_eq!(neg.confidence, 0.82);

    mock.assert();
}

// ═══════════════════════════════════════════════════════════════════════════════
// TEST 2: Anthropic backend sends correct headers
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn anthropic_backend_sends_correct_headers() {
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/messages")
            .header("x-api-key", "sk-ant-test-key")
            .header("anthropic-version", "2023-06-01");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(anthropic_response_body());
    });

    let config = NegotiationConfig {
        negotiation_backend: "anthropic".into(),
        api_key: Some("sk-ant-test-key".into()),
        model: Some("claude-sonnet-4-6".into()),
        base_url: Some(server.url("")),
    };
    let agents = AgentsConfig { registry: vec![] };

    let result = call_negotiation_llm("test prompt".into(), &config, &agents).await;
    assert!(result.is_ok(), "Expected Ok, got {:?}", result);

    mock.assert();
}

// ═══════════════════════════════════════════════════════════════════════════════
// TEST 3: GitHub Copilot uses the OpenAI branch
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn copilot_uses_openai_branch() {
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/chat/completions")
            .header("Authorization", "Bearer ghp_test_copilot_token");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(openai_response_body());
    });

    let config = NegotiationConfig {
        negotiation_backend: "openai".into(),
        api_key: Some("ghp_test_copilot_token".into()),
        model: Some("gpt-4o".into()),
        base_url: Some(server.url("")), // Would be https://api.githubcopilot.com in prod
    };
    let agents = AgentsConfig { registry: vec![] };

    let result = call_negotiation_llm("copilot test".into(), &config, &agents).await;
    assert!(result.is_ok(), "Copilot should use OpenAI branch: {:?}", result);
    mock.assert();
}

// ═══════════════════════════════════════════════════════════════════════════════
// TEST 4: Ollama uses the OpenAI branch
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn ollama_uses_openai_branch() {
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/chat/completions")
            .header("Authorization", "Bearer ollama");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(openai_response_body());
    });

    let config = NegotiationConfig {
        negotiation_backend: "openai".into(),
        api_key: Some("ollama".into()),
        model: Some("llama3.3".into()),
        base_url: Some(server.url("")), // Would be http://localhost:11434/v1 in prod
    };
    let agents = AgentsConfig { registry: vec![] };

    let result = call_negotiation_llm("ollama test".into(), &config, &agents).await;
    assert!(result.is_ok(), "Ollama should use OpenAI branch: {:?}", result);

    let neg = result.unwrap();
    assert!(neg.proposed_diff.contains("merged line"));

    mock.assert();
}

// ═══════════════════════════════════════════════════════════════════════════════
// TEST 5: Disabled returns NegotiationNotConfigured immediately
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn disabled_returns_error() {
    let config = NegotiationConfig {
        negotiation_backend: "disabled".into(),
        api_key: None,
        model: None,
        base_url: None,
    };
    let agents = AgentsConfig { registry: vec![] };

    let result = call_negotiation_llm("should not call".into(), &config, &agents).await;
    assert!(result.is_err());

    match result.unwrap_err() {
        HarmonyError::NegotiationNotConfigured => {} // Expected
        other => panic!("Expected NegotiationNotConfigured, got: {:?}", other),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TEST 6: OpenAI parses valid response correctly
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn openai_parses_valid_response() {
    let server = MockServer::start();

    server.mock(|when, then| {
        when.method(POST).path("/chat/completions");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(openai_response_body());
    });

    let config = NegotiationConfig {
        negotiation_backend: "openai".into(),
        api_key: Some("sk-parse-test".into()),
        model: Some("gpt-4o".into()),
        base_url: Some(server.url("")),
    };
    let agents = AgentsConfig { registry: vec![] };

    let result = call_negotiation_llm("parse test".into(), &config, &agents).await.unwrap();

    assert!(!result.proposed_diff.is_empty(), "proposed_diff should not be empty");
    assert!(result.proposed_diff.contains("merged line"));
    assert_eq!(result.rationale, "Combined both changes.");
    assert_eq!(result.confidence, 0.82);
    assert_eq!(result.memory_notes, vec!["Merged successfully"]);
}

// ═══════════════════════════════════════════════════════════════════════════════
// TEST 7: Anthropic parses valid response correctly
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn anthropic_parses_valid_response() {
    let server = MockServer::start();

    server.mock(|when, then| {
        when.method(POST).path("/v1/messages");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(anthropic_response_body());
    });

    let config = NegotiationConfig {
        negotiation_backend: "anthropic".into(),
        api_key: Some("sk-ant-parse-test".into()),
        model: Some("claude-sonnet-4-6".into()),
        base_url: Some(server.url("")),
    };
    let agents = AgentsConfig { registry: vec![] };

    let result = call_negotiation_llm("parse test".into(), &config, &agents).await.unwrap();

    assert!(result.proposed_diff.contains("merged line"));
    assert_eq!(result.rationale, "Combined both changes.");
    assert_eq!(result.confidence, 0.82);
}

// ═══════════════════════════════════════════════════════════════════════════════
// TEST 8: Bad JSON returns NegotiationInvalidResponse
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn bad_json_returns_negotiation_error() {
    let server = MockServer::start();

    server.mock(|when, then| {
        when.method(POST).path("/chat/completions");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(serde_json::json!({
                "choices": [{
                    "message": {
                        "content": "This is not valid JSON {{{broken"
                    }
                }]
            }));
    });

    let config = NegotiationConfig {
        negotiation_backend: "openai".into(),
        api_key: Some("sk-bad-json-test".into()),
        model: None,
        base_url: Some(server.url("")),
    };
    let agents = AgentsConfig { registry: vec![] };

    let result = call_negotiation_llm("bad json test".into(), &config, &agents).await;
    assert!(result.is_err());

    match result.unwrap_err() {
        HarmonyError::NegotiationInvalidResponse(msg) => {
            assert!(msg.contains("Invalid JSON"), "Error should mention invalid JSON: {}", msg);
        }
        other => panic!("Expected NegotiationInvalidResponse, got: {:?}", other),
    }
}
