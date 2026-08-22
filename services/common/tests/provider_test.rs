//! Comprehensive tests for the UpstreamProvider trait implementations.
//!
//! Tests cover:
//! - Anthropic: path rewriting, auth injection, text/prompt/context extraction, token usage, model extraction
//! - Gemini: path rewriting, auth injection, text/prompt/context extraction, token usage, model extraction
//! - ProviderKind: parsing from strings, display, serialization
//! - Factory: create_provider returns correct implementations

use controlplane_common::provider::{ProviderKind, UpstreamProvider};
use controlplane_common::provider_anthropic::AnthropicProvider;
use controlplane_common::provider_gemini::GeminiProvider;
use controlplane_common::providers::create_provider;

// =============================================================================
// ProviderKind tests
// =============================================================================

#[test]
fn provider_kind_from_str_anthropic() {
    assert_eq!("anthropic".parse::<ProviderKind>().unwrap(), ProviderKind::Anthropic);
    assert_eq!("Anthropic".parse::<ProviderKind>().unwrap(), ProviderKind::Anthropic);
    assert_eq!("ANTHROPIC".parse::<ProviderKind>().unwrap(), ProviderKind::Anthropic);
}

#[test]
fn provider_kind_from_str_gemini() {
    assert_eq!("gemini".parse::<ProviderKind>().unwrap(), ProviderKind::Gemini);
    assert_eq!("Gemini".parse::<ProviderKind>().unwrap(), ProviderKind::Gemini);
    assert_eq!("GEMINI".parse::<ProviderKind>().unwrap(), ProviderKind::Gemini);
}

#[test]
fn provider_kind_from_str_invalid() {
    assert!("openai".parse::<ProviderKind>().is_err());
    assert!("".parse::<ProviderKind>().is_err());
    assert!("llama".parse::<ProviderKind>().is_err());
}

#[test]
fn provider_kind_display() {
    assert_eq!(format!("{}", ProviderKind::Anthropic), "anthropic");
    assert_eq!(format!("{}", ProviderKind::Gemini), "gemini");
}

#[test]
fn provider_kind_serde_roundtrip() {
    let json = serde_json::to_string(&ProviderKind::Anthropic).unwrap();
    assert_eq!(json, "\"anthropic\"");
    let deserialized: ProviderKind = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, ProviderKind::Anthropic);

    let json = serde_json::to_string(&ProviderKind::Gemini).unwrap();
    assert_eq!(json, "\"gemini\"");
    let deserialized: ProviderKind = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, ProviderKind::Gemini);
}

// =============================================================================
// Factory tests
// =============================================================================

#[test]
fn create_provider_returns_anthropic() {
    let provider = create_provider(ProviderKind::Anthropic);
    assert_eq!(provider.kind(), ProviderKind::Anthropic);
}

#[test]
fn create_provider_returns_gemini() {
    let provider = create_provider(ProviderKind::Gemini);
    assert_eq!(provider.kind(), ProviderKind::Gemini);
}

// =============================================================================
// Anthropic Provider — Path Rewriting
// =============================================================================

#[test]
fn anthropic_rewrite_path_passthrough() {
    let p = AnthropicProvider;
    assert_eq!(p.rewrite_path("/v1/messages", ""), "/v1/messages");
    assert_eq!(p.rewrite_path("/v1/messages", "claude-3-sonnet"), "/v1/messages");
    assert_eq!(p.rewrite_path("/some/random/path", ""), "/some/random/path");
}

// =============================================================================
// Anthropic Provider — Auth
// =============================================================================

#[test]
fn anthropic_apply_auth_sets_headers() {
    let p = AnthropicProvider;
    let client = reqwest::Client::new();
    let req = client.post("https://example.com");
    let req = p.apply_auth(req, "sk-ant-test-key");

    let built = req.build().unwrap();
    assert_eq!(
        built.headers().get("x-api-key").unwrap(),
        "sk-ant-test-key"
    );
    assert_eq!(
        built.headers().get("anthropic-version").unwrap(),
        "2023-06-01"
    );
}

// =============================================================================
// Anthropic Provider — Response Text Extraction
// =============================================================================

#[test]
fn anthropic_extract_text_single_block() {
    let p = AnthropicProvider;
    let body = serde_json::json!({
        "content": [
            {"type": "text", "text": "Hello, world!"}
        ]
    });
    let text = p.extract_response_text(body.to_string().as_bytes()).unwrap();
    assert_eq!(text, "Hello, world!");
}

#[test]
fn anthropic_extract_text_multiple_blocks() {
    let p = AnthropicProvider;
    let body = serde_json::json!({
        "content": [
            {"type": "text", "text": "First part."},
            {"type": "text", "text": "Second part."}
        ]
    });
    let text = p.extract_response_text(body.to_string().as_bytes()).unwrap();
    assert!(text.contains("First part."));
    assert!(text.contains("Second part."));
}

#[test]
fn anthropic_extract_text_tool_use_ignored() {
    let p = AnthropicProvider;
    let body = serde_json::json!({
        "content": [
            {"type": "tool_use", "id": "123", "name": "search"},
            {"type": "text", "text": "Here are the results."}
        ]
    });
    let text = p.extract_response_text(body.to_string().as_bytes()).unwrap();
    assert_eq!(text, "Here are the results.");
    assert!(!text.contains("tool_use"));
}

#[test]
fn anthropic_extract_text_fallback_top_level_text() {
    let p = AnthropicProvider;
    let body = serde_json::json!({"text": "Simple text response"});
    let text = p.extract_response_text(body.to_string().as_bytes()).unwrap();
    assert_eq!(text, "Simple text response");
}

#[test]
fn anthropic_extract_text_invalid_json_returns_stringified() {
    let p = AnthropicProvider;
    let text = p.extract_response_text(b"not json").unwrap();
    assert!(text.contains("not json"));
}

// =============================================================================
// Anthropic Provider — Request Prompt Extraction
// =============================================================================

#[test]
fn anthropic_extract_prompt_single_user_message() {
    let p = AnthropicProvider;
    let body = serde_json::json!({
        "messages": [
            {"role": "user", "content": "What is Rust?"}
        ]
    });
    let prompt = p.extract_request_prompt(body.to_string().as_bytes()).unwrap();
    assert_eq!(prompt, "What is Rust?");
}

#[test]
fn anthropic_extract_prompt_multiple_user_messages() {
    let p = AnthropicProvider;
    let body = serde_json::json!({
        "messages": [
            {"role": "user", "content": "Hello"},
            {"role": "assistant", "content": "Hi there!"},
            {"role": "user", "content": "Tell me a joke"}
        ]
    });
    let prompt = p.extract_request_prompt(body.to_string().as_bytes()).unwrap();
    assert!(prompt.contains("Hello"));
    assert!(prompt.contains("Tell me a joke"));
    // Should NOT include assistant messages
    assert!(!prompt.contains("Hi there!"));
}

#[test]
fn anthropic_extract_prompt_no_user_messages() {
    let p = AnthropicProvider;
    let body = serde_json::json!({
        "messages": [
            {"role": "assistant", "content": "Only assistant messages"}
        ]
    });
    let prompt = p.extract_request_prompt(body.to_string().as_bytes());
    assert!(prompt.is_none());
}

// =============================================================================
// Anthropic Provider — Context Extraction
// =============================================================================

#[test]
fn anthropic_extract_context_system_field() {
    let p = AnthropicProvider;
    let body = serde_json::json!({
        "system": "You are a helpful coding assistant.",
        "messages": [{"role": "user", "content": "Help me"}]
    });
    let ctx = p.extract_context(body.to_string().as_bytes()).unwrap();
    assert!(ctx.contains("helpful coding assistant"));
}

#[test]
fn anthropic_extract_context_system_role_in_messages() {
    let p = AnthropicProvider;
    let body = serde_json::json!({
        "messages": [
            {"role": "system", "content": "Be concise."},
            {"role": "user", "content": "Hi"}
        ]
    });
    let ctx = p.extract_context(body.to_string().as_bytes()).unwrap();
    assert_eq!(ctx, "Be concise.");
}

#[test]
fn anthropic_extract_context_no_system() {
    let p = AnthropicProvider;
    let body = serde_json::json!({
        "messages": [{"role": "user", "content": "Hi"}]
    });
    let ctx = p.extract_context(body.to_string().as_bytes());
    assert!(ctx.is_none());
}

// =============================================================================
// Anthropic Provider — Token Usage
// =============================================================================

#[test]
fn anthropic_extract_token_usage_standard() {
    let p = AnthropicProvider;
    let body = serde_json::json!({
        "usage": {
            "input_tokens": 150,
            "output_tokens": 42
        }
    });
    let (input, output) = p.extract_token_usage(body.to_string().as_bytes());
    assert_eq!(input, Some(150));
    assert_eq!(output, Some(42));
}

#[test]
fn anthropic_extract_token_usage_missing() {
    let p = AnthropicProvider;
    let body = serde_json::json!({"id": "msg_123"});
    let (input, output) = p.extract_token_usage(body.to_string().as_bytes());
    assert_eq!(input, None);
    assert_eq!(output, None);
}

#[test]
fn anthropic_extract_token_usage_invalid_json() {
    let p = AnthropicProvider;
    let (input, output) = p.extract_token_usage(b"not json");
    assert_eq!(input, None);
    assert_eq!(output, None);
}

// =============================================================================
// Anthropic Provider — Model Extraction
// =============================================================================

#[test]
fn anthropic_extract_model() {
    let p = AnthropicProvider;
    let body = serde_json::json!({"model": "claude-3-sonnet-20240229"});
    let model = p.extract_model(body.to_string().as_bytes()).unwrap();
    assert_eq!(model, "claude-3-sonnet-20240229");
}

#[test]
fn anthropic_extract_model_missing() {
    let p = AnthropicProvider;
    let body = serde_json::json!({"content": []});
    let model = p.extract_model(body.to_string().as_bytes());
    assert!(model.is_none());
}

// =============================================================================
// Gemini Provider — Path Rewriting
// =============================================================================

#[test]
fn gemini_rewrite_path_with_model() {
    let p = GeminiProvider;
    let path = p.rewrite_path("/v1/messages", "gemini-2.0-flash");
    assert_eq!(
        path,
        "/v1beta/models/gemini-2.0-flash:generateContent"
    );
}

#[test]
fn gemini_rewrite_path_default_model() {
    let p = GeminiProvider;
    let path = p.rewrite_path("/v1/messages", "");
    assert_eq!(
        path,
        "/v1beta/models/gemini-2.0-flash:generateContent"
    );
}

#[test]
fn gemini_rewrite_path_pro_model() {
    let p = GeminiProvider;
    let path = p.rewrite_path("/v1/messages", "gemini-2.5-pro");
    assert_eq!(
        path,
        "/v1beta/models/gemini-2.5-pro:generateContent"
    );
}

// =============================================================================
// Gemini Provider — Auth
// =============================================================================

#[test]
fn gemini_apply_auth_sets_query_param() {
    let p = GeminiProvider;
    let client = reqwest::Client::new();
    let req = client.post("https://example.com");
    let req = p.apply_auth(req, "AIzaSyTestKey");

    let built = req.build().unwrap();
    let url = built.url();
    assert_eq!(url.query().unwrap(), "key=AIzaSyTestKey");

    // Should NOT have x-api-key header
    assert!(built.headers().get("x-api-key").is_none());
}

// =============================================================================
// Gemini Provider — Response Text Extraction
// =============================================================================

#[test]
fn gemini_extract_text_single_candidate() {
    let p = GeminiProvider;
    let body = serde_json::json!({
        "candidates": [{
            "content": {
                "parts": [{"text": "Hello from Gemini!"}]
            }
        }]
    });
    let text = p.extract_response_text(body.to_string().as_bytes()).unwrap();
    assert_eq!(text, "Hello from Gemini!");
}

#[test]
fn gemini_extract_text_multiple_parts() {
    let p = GeminiProvider;
    let body = serde_json::json!({
        "candidates": [{
            "content": {
                "parts": [
                    {"text": "Part one."},
                    {"text": "Part two."}
                ]
            }
        }]
    });
    let text = p.extract_response_text(body.to_string().as_bytes()).unwrap();
    assert!(text.contains("Part one."));
    assert!(text.contains("Part two."));
}

#[test]
fn gemini_extract_text_empty_candidates() {
    let p = GeminiProvider;
    let body = serde_json::json!({"candidates": []});
    let text = p.extract_response_text(body.to_string().as_bytes()).unwrap();
    // Should fall back to stringify
    assert!(text.contains("candidates"));
}

#[test]
fn gemini_extract_text_fallback_top_level_text() {
    let p = GeminiProvider;
    let body = serde_json::json!({"text": "Simple response"});
    let text = p.extract_response_text(body.to_string().as_bytes()).unwrap();
    assert_eq!(text, "Simple response");
}

// =============================================================================
// Gemini Provider — Request Prompt Extraction
// =============================================================================

#[test]
fn gemini_extract_prompt_single_user() {
    let p = GeminiProvider;
    let body = serde_json::json!({
        "contents": [
            {"role": "user", "parts": [{"text": "Explain quantum computing"}]}
        ]
    });
    let prompt = p.extract_request_prompt(body.to_string().as_bytes()).unwrap();
    assert_eq!(prompt, "Explain quantum computing");
}

#[test]
fn gemini_extract_prompt_multi_turn() {
    let p = GeminiProvider;
    let body = serde_json::json!({
        "contents": [
            {"role": "user", "parts": [{"text": "What is Rust?"}]},
            {"role": "model", "parts": [{"text": "A systems language."}]},
            {"role": "user", "parts": [{"text": "Why is it fast?"}]}
        ]
    });
    let prompt = p.extract_request_prompt(body.to_string().as_bytes()).unwrap();
    assert!(prompt.contains("What is Rust?"));
    assert!(prompt.contains("Why is it fast?"));
    // Should NOT include model messages
    assert!(!prompt.contains("A systems language."));
}

#[test]
fn gemini_extract_prompt_no_user() {
    let p = GeminiProvider;
    let body = serde_json::json!({
        "contents": [
            {"role": "model", "parts": [{"text": "Only model messages"}]}
        ]
    });
    let prompt = p.extract_request_prompt(body.to_string().as_bytes());
    assert!(prompt.is_none());
}

#[test]
fn gemini_extract_prompt_multiple_parts_per_turn() {
    let p = GeminiProvider;
    let body = serde_json::json!({
        "contents": [
            {"role": "user", "parts": [
                {"text": "First question"},
                {"text": "Second question"}
            ]}
        ]
    });
    let prompt = p.extract_request_prompt(body.to_string().as_bytes()).unwrap();
    assert!(prompt.contains("First question"));
    assert!(prompt.contains("Second question"));
}

// =============================================================================
// Gemini Provider — Context Extraction
// =============================================================================

#[test]
fn gemini_extract_context_system_instruction() {
    let p = GeminiProvider;
    let body = serde_json::json!({
        "systemInstruction": {
            "parts": [{"text": "You are a helpful assistant."}]
        },
        "contents": [{"role": "user", "parts": [{"text": "Hi"}]}]
    });
    let ctx = p.extract_context(body.to_string().as_bytes()).unwrap();
    assert_eq!(ctx, "You are a helpful assistant.");
}

#[test]
fn gemini_extract_context_system_instruction_multiple_parts() {
    let p = GeminiProvider;
    let body = serde_json::json!({
        "systemInstruction": {
            "parts": [
                {"text": "Rule 1: Be concise."},
                {"text": "Rule 2: Be accurate."}
            ]
        },
        "contents": []
    });
    let ctx = p.extract_context(body.to_string().as_bytes()).unwrap();
    assert!(ctx.contains("Rule 1"));
    assert!(ctx.contains("Rule 2"));
}

#[test]
fn gemini_extract_context_system_role_in_contents() {
    let p = GeminiProvider;
    let body = serde_json::json!({
        "contents": [
            {"role": "system", "parts": [{"text": "System prompt here"}]},
            {"role": "user", "parts": [{"text": "Hi"}]}
        ]
    });
    let ctx = p.extract_context(body.to_string().as_bytes()).unwrap();
    assert_eq!(ctx, "System prompt here");
}

#[test]
fn gemini_extract_context_no_context() {
    let p = GeminiProvider;
    let body = serde_json::json!({
        "contents": [{"role": "user", "parts": [{"text": "Hi"}]}]
    });
    let ctx = p.extract_context(body.to_string().as_bytes());
    assert!(ctx.is_none());
}

// =============================================================================
// Gemini Provider — Token Usage
// =============================================================================

#[test]
fn gemini_extract_token_usage_standard() {
    let p = GeminiProvider;
    let body = serde_json::json!({
        "usageMetadata": {
            "promptTokenCount": 150,
            "candidatesTokenCount": 42,
            "totalTokenCount": 192
        }
    });
    let (input, output) = p.extract_token_usage(body.to_string().as_bytes());
    assert_eq!(input, Some(150));
    assert_eq!(output, Some(42));
}

#[test]
fn gemini_extract_token_usage_missing() {
    let p = GeminiProvider;
    let body = serde_json::json!({"candidates": []});
    let (input, output) = p.extract_token_usage(body.to_string().as_bytes());
    assert_eq!(input, None);
    assert_eq!(output, None);
}

#[test]
fn gemini_extract_token_usage_zero_tokens() {
    let p = GeminiProvider;
    let body = serde_json::json!({
        "usageMetadata": {
            "promptTokenCount": 0,
            "candidatesTokenCount": 0
        }
    });
    let (input, output) = p.extract_token_usage(body.to_string().as_bytes());
    assert_eq!(input, Some(0));
    assert_eq!(output, Some(0));
}

// =============================================================================
// Gemini Provider — Model Extraction
// =============================================================================

#[test]
fn gemini_extract_model_plain() {
    let p = GeminiProvider;
    let body = serde_json::json!({"model": "gemini-2.0-flash"});
    let model = p.extract_model(body.to_string().as_bytes()).unwrap();
    assert_eq!(model, "gemini-2.0-flash");
}

#[test]
fn gemini_extract_model_with_prefix() {
    let p = GeminiProvider;
    let body = serde_json::json!({"model": "models/gemini-2.0-flash"});
    let model = p.extract_model(body.to_string().as_bytes()).unwrap();
    assert_eq!(model, "gemini-2.0-flash");
}

#[test]
fn gemini_extract_model_pro() {
    let p = GeminiProvider;
    let body = serde_json::json!({"model": "gemini-2.5-pro"});
    let model = p.extract_model(body.to_string().as_bytes()).unwrap();
    assert_eq!(model, "gemini-2.5-pro");
}

#[test]
fn gemini_extract_model_missing() {
    let p = GeminiProvider;
    let body = serde_json::json!({"contents": []});
    let model = p.extract_model(body.to_string().as_bytes());
    assert!(model.is_none());
}

// =============================================================================
// Gemini Real-World Response Formats
// =============================================================================

#[test]
fn gemini_real_generate_content_response() {
    let p = GeminiProvider;
    let body = serde_json::json!({
        "candidates": [{
            "content": {
                "parts": [{
                    "text": "Rust is a systems programming language focused on safety, speed, and concurrency. It achieves memory safety without garbage collection through its ownership system."
                }],
                "role": "model"
            },
            "finishReason": "STOP",
            "index": 0
        }],
        "usageMetadata": {
            "promptTokenCount": 12,
            "candidatesTokenCount": 35,
            "totalTokenCount": 47
        },
        "modelVersion": "gemini-2.0-flash"
    });
    let text = p.extract_response_text(body.to_string().as_bytes()).unwrap();
    assert!(text.contains("systems programming language"));
    let (input, output) = p.extract_token_usage(body.to_string().as_bytes());
    assert_eq!(input, Some(12));
    assert_eq!(output, Some(35));
}

#[test]
fn gemini_real_chat_request() {
    let p = GeminiProvider;
    let body = serde_json::json!({
        "contents": [
            {"role": "user", "parts": [{"text": "Hello"}]}
        ],
        "generationConfig": {
            "temperature": 0.7,
            "maxOutputTokens": 100
        }
    });
    let prompt = p.extract_request_prompt(body.to_string().as_bytes()).unwrap();
    assert_eq!(prompt, "Hello");
}

#[test]
fn gemini_real_chat_request_with_system() {
    let p = GeminiProvider;
    let body = serde_json::json!({
        "systemInstruction": {
            "parts": [{"text": "You are a helpful assistant that speaks like a pirate."}]
        },
        "contents": [
            {"role": "user", "parts": [{"text": "How are you?"}]},
            {"role": "model", "parts": [{"text": "Ahoy matey!"}]},
            {"role": "user", "parts": [{"text": "Tell me about treasure"}]}
        ]
    });
    let ctx = p.extract_context(body.to_string().as_bytes()).unwrap();
    assert!(ctx.contains("pirate"));
    let prompt = p.extract_request_prompt(body.to_string().as_bytes()).unwrap();
    assert!(prompt.contains("How are you?"));
    assert!(prompt.contains("Tell me about treasure"));
}

// =============================================================================
// Cross-Provider Consistency
// =============================================================================

#[test]
fn both_providers_return_same_kind() {
    let anthropic = create_provider(ProviderKind::Anthropic);
    let gemini = create_provider(ProviderKind::Gemini);
    assert_eq!(anthropic.kind(), ProviderKind::Anthropic);
    assert_eq!(gemini.kind(), ProviderKind::Gemini);
}

#[test]
fn both_providers_handle_invalid_json_gracefully() {
    let anthropic = create_provider(ProviderKind::Anthropic);
    let gemini = create_provider(ProviderKind::Gemini);

    // Response text
    assert!(anthropic.extract_response_text(b"not json").is_some());
    assert!(gemini.extract_response_text(b"not json").is_some());

    // Token usage
    assert_eq!(anthropic.extract_token_usage(b"not json"), (None, None));
    assert_eq!(gemini.extract_token_usage(b"not json"), (None, None));

    // Model
    assert!(anthropic.extract_model(b"not json").is_none());
    assert!(gemini.extract_model(b"not json").is_none());
}
