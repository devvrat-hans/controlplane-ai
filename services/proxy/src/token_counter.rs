/// Extract token usage from an Anthropic API response body.
/// Returns (input_tokens, output_tokens) or (None, None) if not parseable.
pub fn extract_token_usage(body: &[u8]) -> (Option<i32>, Option<i32>) {
    let parsed: Result<serde_json::Value, _> = serde_json::from_slice(body);

    match parsed {
        Ok(json) => {
            let usage = &json["usage"];
            let input = usage["input_tokens"].as_i64().map(|v| v as i32);
            let output = usage["output_tokens"].as_i64().map(|v| v as i32);
            (input, output)
        }
        Err(_) => (None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_anthropic_usage() {
        let body = br#"{
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello!"}],
            "usage": {"input_tokens": 150, "output_tokens": 42}
        }"#;

        let (input, output) = extract_token_usage(body);
        assert_eq!(input, Some(150));
        assert_eq!(output, Some(42));
    }

    #[test]
    fn handles_missing_usage() {
        let body = br#"{"id": "msg_123", "content": "hello"}"#;
        let (input, output) = extract_token_usage(body);
        assert_eq!(input, None);
        assert_eq!(output, None);
    }

    #[test]
    fn handles_invalid_json() {
        let body = b"not json at all";
        let (input, output) = extract_token_usage(body);
        assert_eq!(input, None);
        assert_eq!(output, None);
    }
}
