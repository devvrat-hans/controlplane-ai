use crate::provider::{ProviderKind, UpstreamProvider};

/// OpenCode Zen API provider (Ox Alpha Free).
///
/// This provider uses the OpenAI-compatible `/v1/chat/completions` format.
/// No authentication is required.
pub struct OpenCodeProvider;

impl UpstreamProvider for OpenCodeProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::OpenCode
    }

    /// OpenCode uses the standard OpenAI path format.
    /// `/v1/chat/completions` is the target endpoint.
    fn rewrite_path(&self, incoming_path: &str, _model: &str) -> String {
        // The OpenCode endpoint is at /zen/v1/chat/completions.
        // We rewrite to that path regardless of incoming path.
        let path = incoming_path.trim_start_matches('/');
        if path.contains("chat/completions") {
            // Already has chat/completions, use as-is under /zen
            format!("/zen/{path}")
        } else {
            // Generic v1 path — rewrite to chat/completions under /zen
            "/zen/v1/chat/completions".to_string()
        }
    }

    /// OpenCode Zen API uses no authentication (free model).
    fn apply_auth(
        &self,
        request: reqwest::RequestBuilder,
        _api_key: &str,
    ) -> reqwest::RequestBuilder {
        // No auth required for Ox Alpha Free
        request
    }

    fn extract_response_text(&self, body: &[u8]) -> Option<String> {
        // OpenAI-compatible format:
        // { "choices": [{ "message": { "content": "..." } }] }
        let json = match serde_json::from_slice::<serde_json::Value>(body) {
            Ok(v) => v,
            Err(_) => return Some(String::from_utf8_lossy(body).to_string()),
        };

        if let Some(choices) = json.get("choices").and_then(|c| c.as_array()) {
            if let Some(first) = choices.first() {
                if let Some(message) = first.get("message") {
                    if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
                        return Some(content.to_string());
                    }
                }
            }
        }

        // Fallback: top-level "text" or "content" field
        if let Some(text) = json.get("text").and_then(|t| t.as_str()) {
            return Some(text.to_string());
        }

        Some(json.to_string())
    }

    fn extract_request_prompt(&self, body: &[u8]) -> Option<String> {
        // OpenAI format: { "messages": [{ "role": "user", "content": "..." }] }
        let json: serde_json::Value = serde_json::from_slice(body).ok()?;

        if let Some(messages) = json.get("messages").and_then(|m| m.as_array()) {
            let user_msgs: Vec<&str> = messages
                .iter()
                .filter_map(|msg| {
                    if msg.get("role").and_then(|r| r.as_str()) == Some("user") {
                        msg.get("content").and_then(|c| c.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            if !user_msgs.is_empty() {
                return Some(user_msgs.join("\n"));
            }
        }

        None
    }

    fn extract_context(&self, body: &[u8]) -> Option<String> {
        let json: serde_json::Value = serde_json::from_slice(body).ok()?;

        // Check for "system" role messages
        if let Some(messages) = json.get("messages").and_then(|m| m.as_array()) {
            let system_msgs: Vec<&str> = messages
                .iter()
                .filter_map(|msg| {
                    if msg.get("role").and_then(|r| r.as_str()) == Some("system") {
                        msg.get("content").and_then(|c| c.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            if !system_msgs.is_empty() {
                return Some(system_msgs.join("\n"));
            }
        }

        // Also check top-level "system" field (some OpenAI-compatible APIs)
        if let Some(system) = json.get("system").and_then(|s| s.as_str()) {
            return Some(system.to_string());
        }

        None
    }

    fn extract_token_usage(&self, body: &[u8]) -> (Option<i32>, Option<i32>) {
        let json: serde_json::Value = match serde_json::from_slice(body) {
            Ok(v) => v,
            Err(_) => return (None, None),
        };

        // OpenAI format: { "usage": { "prompt_tokens": N, "completion_tokens": N } }
        let usage = &json["usage"];
        let input = usage["prompt_tokens"]
            .as_i64()
            .or_else(|| usage["input_tokens"].as_i64())
            .map(|v| v as i32);
        let output = usage["completion_tokens"]
            .as_i64()
            .or_else(|| usage["output_tokens"].as_i64())
            .map(|v| v as i32);
        (input, output)
    }

    fn extract_model(&self, body: &[u8]) -> Option<String> {
        let json: serde_json::Value = serde_json::from_slice(body).ok()?;
        json.get("model")
            .and_then(|m| m.as_str())
            .map(String::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rewrite_path_passthrough() {
        let provider = OpenCodeProvider;
        assert_eq!(
            provider.rewrite_path("/v1/chat/completions", "mimo-v2.5-free"),
            "/zen/v1/chat/completions"
        );
    }

    #[test]
    fn test_rewrite_path_generic() {
        let provider = OpenCodeProvider;
        assert_eq!(
            provider.rewrite_path("/v1/messages", "mimo-v2.5-free"),
            "/zen/v1/chat/completions"
        );
    }

    #[test]
    fn test_rewrite_path_with_leading_slash() {
        let provider = OpenCodeProvider;
        assert_eq!(
            provider.rewrite_path("/chat/completions", "mimo-v2.5-free"),
            "/zen/chat/completions"
        );
    }

    #[test]
    fn test_extract_model() {
        let provider = OpenCodeProvider;
        let body = r#"{"model": "mimo-v2.5-free"}"#;
        assert_eq!(
            provider.extract_model(body.as_bytes()),
            Some("mimo-v2.5-free".to_string())
        );
    }

    #[test]
    fn test_extract_response_text() {
        let provider = OpenCodeProvider;
        let body = r#"{"choices":[{"message":{"content":"Hello world"}}]}"#;
        assert_eq!(
            provider.extract_response_text(body.as_bytes()),
            Some("Hello world".to_string())
        );
    }

    #[test]
    fn test_extract_prompt() {
        let provider = OpenCodeProvider;
        let body = r#"{"messages":[{"role":"user","content":"What is 2+2?"}]}"#;
        assert_eq!(
            provider.extract_request_prompt(body.as_bytes()),
            Some("What is 2+2?".to_string())
        );
    }

    #[test]
    fn test_extract_context() {
        let provider = OpenCodeProvider;
        let body = r#"{"messages":[{"role":"system","content":"You are helpful"},{"role":"user","content":"Hi"}]}"#;
        assert_eq!(
            provider.extract_context(body.as_bytes()),
            Some("You are helpful".to_string())
        );
    }

    #[test]
    fn test_extract_token_usage() {
        let provider = OpenCodeProvider;
        let body = r#"{"usage":{"prompt_tokens":10,"completion_tokens":20}}"#;
        assert_eq!(
            provider.extract_token_usage(body.as_bytes()),
            (Some(10), Some(20))
        );
    }

    #[test]
    fn test_extract_token_usage_alt_fields() {
        let provider = OpenCodeProvider;
        let body = r#"{"usage":{"input_tokens":15,"output_tokens":25}}"#;
        assert_eq!(
            provider.extract_token_usage(body.as_bytes()),
            (Some(15), Some(25))
        );
    }

    #[test]
    fn test_no_auth_applied() {
        let provider = OpenCodeProvider;
        let client = reqwest::Client::new();
        let req = client.get("https://example.com");
        // Should not panic or add auth headers
        let _ = provider.apply_auth(req, "test-key");
    }

    #[test]
    fn test_kind() {
        let provider = OpenCodeProvider;
        assert_eq!(provider.kind(), ProviderKind::OpenCode);
    }
}
