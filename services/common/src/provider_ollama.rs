use crate::provider::{ProviderKind, UpstreamProvider};

/// Ollama local LLM provider.
///
/// Uses Ollama's OpenAI-compatible API at `http://localhost:11434/v1/chat/completions`.
/// No authentication required. Runs entirely on-device.
pub struct OllamaProvider;

impl UpstreamProvider for OllamaProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Ollama
    }

    /// Ollama exposes an OpenAI-compatible API.
    /// Rewrite any incoming path to `/v1/chat/completions`.
    fn rewrite_path(&self, incoming_path: &str, _model: &str) -> String {
        let path = incoming_path.trim_start_matches('/');
        if path.contains("chat/completions") {
            // Already correct — just use /v1/chat/completions
            "/v1/chat/completions".to_string()
        } else {
            "/v1/chat/completions".to_string()
        }
    }

    /// No authentication required for local Ollama.
    fn apply_auth(
        &self,
        request: reqwest::RequestBuilder,
        _api_key: &str,
    ) -> reqwest::RequestBuilder {
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

        // Fallback
        if let Some(text) = json.get("text").and_then(|t| t.as_str()) {
            return Some(text.to_string());
        }

        Some(json.to_string())
    }

    fn extract_request_prompt(&self, body: &[u8]) -> Option<String> {
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

        // Also check top-level "system" field
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

        // OpenAI-compatible format
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
    fn test_rewrite_path() {
        let provider = OllamaProvider;
        assert_eq!(
            provider.rewrite_path("/v1/chat/completions", "qwen2.5:1.5b"),
            "/v1/chat/completions"
        );
    }

    #[test]
    fn test_rewrite_path_messages() {
        let provider = OllamaProvider;
        assert_eq!(
            provider.rewrite_path("/v1/messages", "qwen2.5:1.5b"),
            "/v1/chat/completions"
        );
    }

    #[test]
    fn test_extract_response_text() {
        let provider = OllamaProvider;
        let body = r#"{"choices":[{"message":{"content":"Hello from Ollama"}}]}"#;
        assert_eq!(
            provider.extract_response_text(body.as_bytes()),
            Some("Hello from Ollama".to_string())
        );
    }

    #[test]
    fn test_extract_prompt() {
        let provider = OllamaProvider;
        let body = r#"{"messages":[{"role":"user","content":"What is 2+2?"}]}"#;
        assert_eq!(
            provider.extract_request_prompt(body.as_bytes()),
            Some("What is 2+2?".to_string())
        );
    }

    #[test]
    fn test_extract_context() {
        let provider = OllamaProvider;
        let body = r#"{"messages":[{"role":"system","content":"You are helpful"},{"role":"user","content":"Hi"}]}"#;
        assert_eq!(
            provider.extract_context(body.as_bytes()),
            Some("You are helpful".to_string())
        );
    }

    #[test]
    fn test_extract_token_usage() {
        let provider = OllamaProvider;
        let body = r#"{"usage":{"prompt_tokens":10,"completion_tokens":20}}"#;
        assert_eq!(
            provider.extract_token_usage(body.as_bytes()),
            (Some(10), Some(20))
        );
    }

    #[test]
    fn test_kind() {
        let provider = OllamaProvider;
        assert_eq!(provider.kind(), ProviderKind::Ollama);
    }

    #[test]
    fn test_no_auth_applied() {
        let provider = OllamaProvider;
        let client = reqwest::Client::new();
        let req = client.get("https://example.com");
        // Should not panic or add auth headers
        let _ = provider.apply_auth(req, "test-key");
    }
}
