use crate::provider::{ProviderKind, UpstreamProvider};

pub struct GeminiProvider;

impl UpstreamProvider for GeminiProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Gemini
    }

    fn rewrite_path(&self, _incoming_path: &str, model: &str) -> String {
        let model = if model.is_empty() {
            "gemini-2.0-flash"
        } else {
            model
        };
        format!("/v1beta/models/{model}:generateContent")
    }

    fn apply_auth(
        &self,
        request: reqwest::RequestBuilder,
        api_key: &str,
    ) -> reqwest::RequestBuilder {
        request.query(&[("key", api_key)])
    }

    fn extract_response_text(&self, body: &[u8]) -> Option<String> {
        // Try to parse as JSON; if invalid, return the raw string
        let json = match serde_json::from_slice::<serde_json::Value>(body) {
            Ok(v) => v,
            Err(_) => return Some(String::from_utf8_lossy(body).to_string()),
        };

        // Gemini format: { "candidates": [{ "content": { "parts": [{ "text": "..." }] } }] }
        if let Some(candidates) = json.get("candidates").and_then(|c| c.as_array()) {
            if let Some(first) = candidates.first() {
                if let Some(content) = first.get("content") {
                    if let Some(parts) = content.get("parts").and_then(|p| p.as_array()) {
                        let texts: Vec<&str> = parts
                            .iter()
                            .filter_map(|part| part.get("text").and_then(|t| t.as_str()))
                            .collect();
                        if !texts.is_empty() {
                            return Some(texts.join("\n"));
                        }
                    }
                }
            }
        }

        if let Some(text) = json.get("text").and_then(|t| t.as_str()) {
            return Some(text.to_string());
        }

        Some(json.to_string())
    }

    fn extract_request_prompt(&self, body: &[u8]) -> Option<String> {
        let json: serde_json::Value = serde_json::from_slice(body).ok()?;

        // Gemini format: { "contents": [{ "role": "user", "parts": [{ "text": "..." }] }] }
        if let Some(contents) = json.get("contents").and_then(|c| c.as_array()) {
            let user_texts: Vec<String> = contents
                .iter()
                .filter_map(|content| {
                    if content.get("role").and_then(|r| r.as_str()) == Some("user") {
                        let parts = content.get("parts").and_then(|p| p.as_array())?;
                        let texts: Vec<&str> = parts
                            .iter()
                            .filter_map(|part| part.get("text").and_then(|t| t.as_str()))
                            .collect();
                        if texts.is_empty() {
                            None
                        } else {
                            Some(texts.join("\n"))
                        }
                    } else {
                        None
                    }
                })
                .collect();
            if !user_texts.is_empty() {
                return Some(user_texts.join("\n"));
            }
        }

        None
    }

    fn extract_context(&self, body: &[u8]) -> Option<String> {
        let json: serde_json::Value = serde_json::from_slice(body).ok()?;

        // systemInstruction: { "parts": [{ "text": "..." }] }
        if let Some(system_instruction) = json.get("systemInstruction") {
            if let Some(parts) = system_instruction.get("parts").and_then(|p| p.as_array()) {
                let texts: Vec<&str> = parts
                    .iter()
                    .filter_map(|part| part.get("text").and_then(|t| t.as_str()))
                    .collect();
                if !texts.is_empty() {
                    return Some(texts.join("\n"));
                }
            }
        }

        // Also check contents for "system" role (older format)
        if let Some(contents) = json.get("contents").and_then(|c| c.as_array()) {
            let system_texts: Vec<String> = contents
                .iter()
                .filter_map(|content| {
                    if content.get("role").and_then(|r| r.as_str()) == Some("system") {
                        let parts = content.get("parts").and_then(|p| p.as_array())?;
                        let texts: Vec<&str> = parts
                            .iter()
                            .filter_map(|part| part.get("text").and_then(|t| t.as_str()))
                            .collect();
                        if texts.is_empty() {
                            None
                        } else {
                            Some(texts.join("\n"))
                        }
                    } else {
                        None
                    }
                })
                .collect();
            if !system_texts.is_empty() {
                return Some(system_texts.join("\n"));
            }
        }

        None
    }

    fn extract_token_usage(&self, body: &[u8]) -> (Option<i32>, Option<i32>) {
        let json: serde_json::Value = match serde_json::from_slice(body) {
            Ok(v) => v,
            Err(_) => return (None, None),
        };

        // { "usageMetadata": { "promptTokenCount": N, "candidatesTokenCount": N } }
        let usage = &json["usageMetadata"];
        let input = usage["promptTokenCount"].as_i64().map(|v| v as i32);
        let output = usage["candidatesTokenCount"].as_i64().map(|v| v as i32);
        (input, output)
    }

    fn extract_model(&self, body: &[u8]) -> Option<String> {
        let json: serde_json::Value = serde_json::from_slice(body).ok()?;
        json.get("model")
            .and_then(|m| m.as_str())
            .map(|s| s.strip_prefix("models/").unwrap_or(s).to_string())
    }
}
