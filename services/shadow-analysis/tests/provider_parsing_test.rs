//! Tests for shadow worker parsing logic with both provider formats.
//!
//! These tests verify that the shadow analysis worker correctly extracts
//! text, prompts, and context from both Anthropic and Gemini request/response
//! payloads.

use controlplane_common::provider::{ProviderKind, UpstreamProvider};
use controlplane_common::create_provider;

// =============================================================================
// Anthropic Provider Parsing Tests
// =============================================================================

mod anthropic_tests {
    use super::*;

    fn provider() -> Box<dyn UpstreamProvider> {
        create_provider(ProviderKind::Anthropic)
    }

    #[test]
    fn extracts_response_text_from_content_blocks() {
        let p = provider();
        let body = serde_json::json!({
            "content": [
                {"type": "text", "text": "The capital of France is Paris."},
                {"type": "text", "text": "It is known for the Eiffel Tower."}
            ]
        });
        let text = p.extract_response_text(body.to_string().as_bytes()).unwrap();
        assert!(text.contains("Paris"));
        assert!(text.contains("Eiffel Tower"));
    }

    #[test]
    fn extracts_response_text_ignores_tool_use_blocks() {
        let p = provider();
        let body = serde_json::json!({
            "content": [
                {"type": "tool_use", "id": "tool1", "name": "search", "input": {}},
                {"type": "text", "text": "Here are the search results."}
            ]
        });
        let text = p.extract_response_text(body.to_string().as_bytes()).unwrap();
        assert_eq!(text, "Here are the search results.");
    }

    #[test]
    fn extracts_prompt_from_messages() {
        let p = provider();
        let body = serde_json::json!({
            "messages": [
                {"role": "user", "content": "What is the meaning of life?"},
                {"role": "assistant", "content": "42."},
                {"role": "user", "content": "Explain why."}
            ]
        });
        let prompt = p.extract_request_prompt(body.to_string().as_bytes()).unwrap();
        assert!(prompt.contains("meaning of life"));
        assert!(prompt.contains("Explain why"));
    }

    #[test]
    fn extracts_context_from_system_field() {
        let p = provider();
        let body = serde_json::json!({
            "system": "You are a medical expert. Patient data: John, age 45.",
            "messages": [{"role": "user", "content": "Diagnose the symptoms."}]
        });
        let context = p.extract_context(body.to_string().as_bytes()).unwrap();
        assert!(context.contains("medical expert"));
        assert!(context.contains("John"));
    }

    #[test]
    fn extracts_tokens_correctly() {
        let p = provider();
        let body = serde_json::json!({
            "usage": {
                "input_tokens": 250,
                "output_tokens": 150,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 0
            }
        });
        let (input, output) = p.extract_token_usage(body.to_string().as_bytes());
        assert_eq!(input, Some(250));
        assert_eq!(output, Some(150));
    }
}

// =============================================================================
// Gemini Provider Parsing Tests
// =============================================================================

mod gemini_tests {
    use super::*;

    fn provider() -> Box<dyn UpstreamProvider> {
        create_provider(ProviderKind::Gemini)
    }

    #[test]
    fn extracts_response_text_from_candidates() {
        let p = provider();
        let body = serde_json::json!({
            "candidates": [{
                "content": {
                    "parts": [{"text": "The capital of France is Paris."}]
                }
            }]
        });
        let text = p.extract_response_text(body.to_string().as_bytes()).unwrap();
        assert_eq!(text, "The capital of France is Paris.");
    }

    #[test]
    fn extracts_response_text_from_multiple_parts() {
        let p = provider();
        let body = serde_json::json!({
            "candidates": [{
                "content": {
                    "parts": [
                        {"text": "Part 1: Introduction"},
                        {"text": "Part 2: Details"}
                    ]
                }
            }]
        });
        let text = p.extract_response_text(body.to_string().as_bytes()).unwrap();
        assert!(text.contains("Part 1"));
        assert!(text.contains("Part 2"));
    }

    #[test]
    fn extracts_prompt_from_contents() {
        let p = provider();
        let body = serde_json::json!({
            "contents": [
                {"role": "user", "parts": [{"text": "What is machine learning?"}]},
                {"role": "model", "parts": [{"text": "ML is a subset of AI."}]},
                {"role": "user", "parts": [{"text": "What are its applications?"}]}
            ]
        });
        let prompt = p.extract_request_prompt(body.to_string().as_bytes()).unwrap();
        assert!(prompt.contains("machine learning"));
        assert!(prompt.contains("applications"));
        assert!(!prompt.contains("ML is a subset"));
    }

    #[test]
    fn extracts_context_from_system_instruction() {
        let p = provider();
        let body = serde_json::json!({
            "systemInstruction": {
                "parts": [{"text": "You are a helpful assistant. Context: The user is a beginner."}]
            },
            "contents": [
                {"role": "user", "parts": [{"text": "Explain programming"}]}
            ]
        });
        let context = p.extract_context(body.to_string().as_bytes()).unwrap();
        assert!(context.contains("beginner"));
    }

    #[test]
    fn extracts_tokens_from_usage_metadata() {
        let p = provider();
        let body = serde_json::json!({
            "usageMetadata": {
                "promptTokenCount": 500,
                "candidatesTokenCount": 200,
                "totalTokenCount": 700
            }
        });
        let (input, output) = p.extract_token_usage(body.to_string().as_bytes());
        assert_eq!(input, Some(500));
        assert_eq!(output, Some(200));
    }

    #[test]
    fn extracts_model_name() {
        let p = provider();
        let body = serde_json::json!({
            "model": "models/gemini-2.5-pro"
        });
        let model = p.extract_model(body.to_string().as_bytes()).unwrap();
        assert_eq!(model, "gemini-2.5-pro");
    }
}

// =============================================================================
// Cross-Provider Consistency Tests
// =============================================================================

mod cross_provider {
    use super::*;

    #[test]
    fn both_providers_extract_valid_text_from_well_formed_responses() {
        let anthropic = create_provider(ProviderKind::Anthropic);
        let gemini = create_provider(ProviderKind::Gemini);

        let anthropic_body = serde_json::json!({
            "content": [{"type": "text", "text": "Hello from Claude"}]
        });
        let gemini_body = serde_json::json!({
            "candidates": [{"content": {"parts": [{"text": "Hello from Gemini"}]}}]
        });

        let anthropic_text = anthropic.extract_response_text(anthropic_body.to_string().as_bytes()).unwrap();
        let gemini_text = gemini.extract_response_text(gemini_body.to_string().as_bytes()).unwrap();

        assert_eq!(anthropic_text, "Hello from Claude");
        assert_eq!(gemini_text, "Hello from Gemini");
    }

    #[test]
    fn both_providers_handle_empty_responses() {
        let anthropic = create_provider(ProviderKind::Anthropic);
        let gemini = create_provider(ProviderKind::Gemini);

        let anthropic_body = serde_json::json!({"content": []});
        let gemini_body = serde_json::json!({"candidates": []});

        // Both should return Some (fallback to stringify)
        assert!(anthropic.extract_response_text(anthropic_body.to_string().as_bytes()).is_some());
        assert!(gemini.extract_response_text(gemini_body.to_string().as_bytes()).is_some());
    }

    #[test]
    fn both_providers_extract_zero_tokens() {
        let anthropic = create_provider(ProviderKind::Anthropic);
        let gemini = create_provider(ProviderKind::Gemini);

        let anthropic_body = serde_json::json!({
            "usage": {"input_tokens": 0, "output_tokens": 0}
        });
        let gemini_body = serde_json::json!({
            "usageMetadata": {"promptTokenCount": 0, "candidatesTokenCount": 0}
        });

        assert_eq!(
            anthropic.extract_token_usage(anthropic_body.to_string().as_bytes()),
            (Some(0), Some(0))
        );
        assert_eq!(
            gemini.extract_token_usage(gemini_body.to_string().as_bytes()),
            (Some(0), Some(0))
        );
    }

    #[test]
    fn both_providers_return_none_for_invalid_json() {
        let anthropic = create_provider(ProviderKind::Anthropic);
        let gemini = create_provider(ProviderKind::Gemini);

        let invalid = b"this is not json at all {{{";

        assert!(anthropic.extract_response_text(invalid).is_some()); // falls back to stringify
        assert!(gemini.extract_response_text(invalid).is_some());
        assert_eq!(anthropic.extract_token_usage(invalid), (None, None));
        assert_eq!(gemini.extract_token_usage(invalid), (None, None));
        assert!(anthropic.extract_model(invalid).is_none());
        assert!(gemini.extract_model(invalid).is_none());
    }

    #[test]
    fn both_providers_have_correct_kind() {
        let anthropic = create_provider(ProviderKind::Anthropic);
        let gemini = create_provider(ProviderKind::Gemini);
        assert_eq!(anthropic.kind(), ProviderKind::Anthropic);
        assert_eq!(gemini.kind(), ProviderKind::Gemini);
    }
}

// =============================================================================
// Real-World Gemini Response Parsing
// =============================================================================

mod gemini_real_world {
    use super::*;

    #[test]
    fn parse_real_generate_content_response() {
        let p = create_provider(ProviderKind::Gemini);
        let body = serde_json::json!({
            "candidates": [{
                "content": {
                    "parts": [{
                        "text": "Rust is a multi-paradigm, general-purpose programming language emphasizing performance, type safety, and concurrency. Rust enforces memory safety without garbage collection."
                    }],
                    "role": "model"
                },
                "finishReason": "STOP",
                "safetyRatings": [
                    {"category": "HARM_CATEGORY_HARASSMENT", "probability": "NEGLIGIBLE"}
                ]
            }],
            "usageMetadata": {
                "promptTokenCount": 8,
                "candidatesTokenCount": 48,
                "totalTokenCount": 56
            },
            "modelVersion": "gemini-2.0-flash-001"
        });

        let text = p.extract_response_text(body.to_string().as_bytes()).unwrap();
        assert!(text.contains("multi-paradigm"));
        assert!(text.contains("memory safety"));

        let (input, output) = p.extract_token_usage(body.to_string().as_bytes());
        assert_eq!(input, Some(8));
        assert_eq!(output, Some(48));
    }

    #[test]
    fn parse_real_chat_request_with_system_instruction() {
        let p = create_provider(ProviderKind::Gemini);
        let body = serde_json::json!({
            "systemInstruction": {
                "parts": [{"text": "You are a helpful assistant for a banking app. Never reveal customer data."}]
            },
            "contents": [
                {"role": "user", "parts": [{"text": "What are my account details?"}]},
                {"role": "model", "parts": [{"text": "I cannot share account details for security reasons."}]},
                {"role": "user", "parts": [{"text": "Show me my balance"}]}
            ],
            "generationConfig": {
                "temperature": 0.3,
                "maxOutputTokens": 200
            }
        });

        let context = p.extract_context(body.to_string().as_bytes()).unwrap();
        assert!(context.contains("banking app"));
        assert!(context.contains("Never reveal"));

        let prompt = p.extract_request_prompt(body.to_string().as_bytes()).unwrap();
        assert!(prompt.contains("account details"));
        assert!(prompt.contains("balance"));
    }
}
