use crate::provider::{ProviderKind, UpstreamProvider};

pub struct AnthropicProvider;

impl UpstreamProvider for AnthropicProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Anthropic
    }

    fn rewrite_path(&self, incoming_path: &str, _model: &str) -> String {
        incoming_path.to_string()
    }

    fn apply_auth<'a>(
        &self,
        request: reqwest::RequestBuilder,
        api_key: &'a str,
    ) -> reqwest::RequestBuilder {
        request
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
    }

    fn extract_response_text(&self, body: &[u8]) -> Option<String> {
        // Try to parse as JSON; if invalid, return the raw string
        let json = match serde_json::from_slice::<serde_json::Value>(body) {
            Ok(v) => v,
            Err(_) => return Some(String::from_utf8_lossy(body).to_string()),
        };

        // Anthropic format: { "content": [{ "type": "text", "text": "..." }] }
        if let Some(content) = json.get("content").and_then(|c| c.as_array()) {
            let texts: Vec<&str> = content
                .iter()
                .filter_map(|block| {
                    if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                        block.get("text").and_then(|t| t.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            if !texts.is_empty() {
                return Some(texts.join("\n"));
            }
        }

        // Fallback: top-level "text" field
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

        if let Some(system) = json.get("system").and_then(|s| s.as_str()) {
            return Some(system.to_string());
        }

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

        None
    }

    fn extract_token_usage(&self, body: &[u8]) -> (Option<i32>, Option<i32>) {
        let json: serde_json::Value = match serde_json::from_slice(body) {
            Ok(v) => v,
            Err(_) => return (None, None),
        };

        let usage = &json["usage"];
        let input = usage["input_tokens"].as_i64().map(|v| v as i32);
        let output = usage["output_tokens"].as_i64().map(|v| v as i32);
        (input, output)
    }

    fn extract_model(&self, body: &[u8]) -> Option<String> {
        let json: serde_json::Value = serde_json::from_slice(body).ok()?;
        json.get("model").and_then(|m| m.as_str()).map(String::from)
    }
}
