//! Redaction and payload bounding.
//!
//! Governance verdict reasons and upstream payloads can contain the very
//! secrets and PII the platform is there to catch. Everything that leaves the
//! MCP server passes through [`sanitize_text`] or [`sanitize_value`] first.
//! Redaction is intentionally conservative: it is better to over-redact an
//! error message than to leak a credential into an agent's context window.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::{Map, Value};

/// Maximum depth for JSON walking — guards against adversarial nested payloads.
const MAX_DEPTH: usize = 8;
/// Maximum number of array elements surfaced.
const MAX_ARRAY: usize = 200;

struct Patterns {
    aws_key: Regex,
    bearer: Regex,
    assignment: Regex,
    email: Regex,
    ssn: Regex,
    /// Candidate digit run for a payment card. Matches generously and is then
    /// validated in code (digit count + Luhn) so that it can never consume a
    /// UUID: a UUID contains at most 12 contiguous digits and its fragments are
    /// rejected by the length check.
    card_candidate: Regex,
}

fn patterns() -> &'static Patterns {
    static PATTERNS: OnceLock<Patterns> = OnceLock::new();
    PATTERNS.get_or_init(|| Patterns {
        aws_key: Regex::new(r"\bAKIA[0-9A-Z]{16}\b").expect("valid aws key regex"),
        bearer: Regex::new(r"(?i)\bbearer\s+[A-Za-z0-9\-._~+/]{16,}=*").expect("valid bearer regex"),
        assignment: Regex::new(
            r#"(?i)\b(api[_-]?key|token|secret|password|passwd|client[_-]?secret)\b\s*[:=]\s*["']?[A-Za-z0-9\-._~+/]{8,}"#,
        )
        .expect("valid assignment regex"),
        email: Regex::new(r"\b[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}\b")
            .expect("valid email regex"),
        ssn: Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").expect("valid ssn regex"),
        card_candidate: Regex::new(r"[0-9][0-9 \-]{11,}[0-9]").expect("valid card regex"),
    })
}

/// Luhn checksum, used to avoid redacting unrelated long digit runs.
fn luhn_valid(digits: &str) -> bool {
    let mut sum = 0u32;
    let mut double = false;
    for ch in digits.chars().rev() {
        let Some(mut d) = ch.to_digit(10) else {
            return false;
        };
        if double {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        sum += d;
        double = !double;
    }
    sum % 10 == 0
}

/// Redact payment-card-shaped substrings (13–19 digits, Luhn-valid).
///
/// This is a manual scan rather than a plain regex replacement because a naive
/// `\b(?:\d[ -]?){13,16}\b` also matches inside hyphenated identifiers such as
/// UUIDs, which would corrupt every id in the output.
fn redact_cards(input: &str) -> String {
    let re = &patterns().card_candidate;
    let mut out = String::with_capacity(input.len());
    let mut last = 0usize;
    for m in re.find_iter(input) {
        let digits: String = m.as_str().chars().filter(char::is_ascii_digit).collect();
        if !(13..=19).contains(&digits.len()) || !luhn_valid(&digits) {
            continue;
        }
        out.push_str(&input[last..m.start()]);
        out.push_str("[REDACTED:CARD]");
        last = m.end();
    }
    out.push_str(&input[last..]);
    out
}

/// Redact credentials, PII and secrets from a free-text string, then bound it.
pub fn sanitize_text(input: &str, max_len: usize) -> String {
    let p = patterns();
    let mut out = input.to_string();
    out = p
        .aws_key
        .replace_all(&out, "[REDACTED:AWS_KEY]")
        .into_owned();
    out = p
        .bearer
        .replace_all(&out, "Bearer [REDACTED:TOKEN]")
        .into_owned();
    out = p
        .assignment
        .replace_all(&out, "[REDACTED:CREDENTIAL]")
        .into_owned();
    out = p.email.replace_all(&out, "[REDACTED:EMAIL]").into_owned();
    out = p.ssn.replace_all(&out, "[REDACTED:SSN]").into_owned();
    out = redact_cards(&out);
    truncate(&out, max_len)
}

/// Truncate a string at a char boundary, appending a marker when cut.
pub fn truncate(input: &str, max_len: usize) -> String {
    if input.chars().count() <= max_len {
        return input.to_string();
    }
    let mut out: String = input.chars().take(max_len).collect();
    out.push_str("…[truncated]");
    out
}

/// Keys whose values are dropped entirely rather than redacted.
fn sensitive_key(key: &str) -> bool {
    let k = key.to_lowercase();
    matches!(
        k.as_str(),
        "password"
            | "password_hash"
            | "api_key"
            | "api_key_hash"
            | "apikey"
            | "token"
            | "secret"
            | "authorization"
            | "request_payload"
            | "response_payload"
    )
}

/// Recursively sanitize a JSON value.
///
/// Sensitive keys are replaced by `"[redacted]"`; all other strings are passed
/// through [`sanitize_text`]; depth, array length and string length are bounded.
pub fn sanitize_value(value: &Value, max_string_len: usize) -> Value {
    sanitize_inner(value, max_string_len, 0)
}

fn sanitize_inner(value: &Value, max_string_len: usize, depth: usize) -> Value {
    if depth >= MAX_DEPTH {
        return Value::String("[truncated:max-depth]".into());
    }
    match value {
        Value::String(s) => Value::String(sanitize_text(s, max_string_len)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .take(MAX_ARRAY)
                .map(|v| sanitize_inner(v, max_string_len, depth + 1))
                .collect(),
        ),
        Value::Object(map) => {
            let mut out = Map::with_capacity(map.len());
            for (key, val) in map {
                if sensitive_key(key) {
                    out.insert(key.clone(), Value::String("[redacted]".into()));
                } else {
                    out.insert(key.clone(), sanitize_inner(val, max_string_len, depth + 1));
                }
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redacts_aws_key() {
        let out = sanitize_text("key AKIAIOSFODNN7EXAMPLE leaked", 1000);
        assert!(!out.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(out.contains("[REDACTED:AWS_KEY]"));
    }

    #[test]
    fn redacts_assigned_credential() {
        let out = sanitize_text("password = hunter2hunter2", 1000);
        assert!(out.contains("[REDACTED:CREDENTIAL]"));
        assert!(!out.contains("hunter2hunter2"));
    }

    #[test]
    fn redacts_email_and_ssn() {
        let out = sanitize_text("contact a@b.com ssn 123-45-6789", 1000);
        assert!(!out.contains("a@b.com"));
        assert!(!out.contains("123-45-6789"));
    }

    #[test]
    fn truncates_long_text() {
        let out = sanitize_text(&"x".repeat(100), 10);
        assert!(out.ends_with("[truncated]"));
        assert!(out.chars().count() < 100);
    }

    #[test]
    fn drops_sensitive_json_keys() {
        let v = json!({ "api_key": "sk-secret", "reason": "hello a@b.com" });
        let out = sanitize_value(&v, 1000);
        assert_eq!(out["api_key"], "[redacted]");
        assert!(!out["reason"].as_str().unwrap().contains("a@b.com"));
    }

    #[test]
    fn preserves_iso_timestamps() {
        // Governance output is full of timestamps; mangling them would make the
        // data useless even though it would not leak anything.
        for ts in [
            "2026-09-26T10:00:00Z",
            "2026-09-26T10:00:00.123456Z",
            "2026-09-26",
            "Created at 2026-09-26T10:00:00Z by system",
        ] {
            assert_eq!(sanitize_text(ts, 1000), ts, "timestamp was altered: {ts}");
        }
    }

    #[test]
    fn preserves_uuids_and_model_names() {
        let uuid = "10000000-0000-0000-0000-000000000001";
        assert_eq!(sanitize_text(uuid, 1000), uuid);
        for model in [
            "qwen2.5:1.5b",
            "claude-sonnet-4-20250514",
            "gemini-2.0-flash",
        ] {
            assert_eq!(sanitize_text(model, 1000), model);
        }
    }

    #[test]
    fn redacts_luhn_valid_cards() {
        // 4111 1111 1111 1111 is a well-known Luhn-valid test number.
        assert!(sanitize_text("card 4111 1111 1111 1111", 1000).contains("[REDACTED:CARD]"));
        assert!(sanitize_text("card 4111-1111-1111-1111", 1000).contains("[REDACTED:CARD]"));
        assert!(sanitize_text("card 4111111111111111", 1000).contains("[REDACTED:CARD]"));
    }

    #[test]
    fn does_not_redact_non_luhn_digit_runs() {
        // Token counts, ids and timestamps must survive intact.
        assert_eq!(
            sanitize_text("tokens: 1234567890123456", 1000),
            "tokens: 1234567890123456"
        );
    }

    #[test]
    fn bounds_array_length() {
        let arr = Value::Array((0..500).map(|i| json!(i)).collect());
        let out = sanitize_value(&arr, 1000);
        assert_eq!(out.as_array().unwrap().len(), MAX_ARRAY);
    }
}
