use regex::Regex;
use std::sync::LazyLock;

use controlplane_common::types::{Axis, Outcome};

use crate::engine::{FastPathVerdict, ResponseEdit};

static AWS_KEY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(AKIA[0-9A-Z]{16})").unwrap()
});

static AWS_SECRET_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:aws.?secret.?access.?key|secret.?key)\s*[:=]\s*['"]?([A-Za-z0-9/+=]{40})['"]?"#).unwrap()
});

static GENERIC_API_KEY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:api[_-]?key|token|secret|password|auth[_-]?token)\s*[:=]\s*['"]?([A-Za-z0-9_\-]{20,64})['"]?"#).unwrap()
});

static CREDIT_CARD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(\d{4}[\s\-]?\d{4}[\s\-]?\d{4}[\s\-]?\d{4})\b").unwrap()
});

static SSN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(\d{3}-\d{2}-\d{4})\b").unwrap()
});

static EMAIL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b([A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,})\b").unwrap()
});

static AADHAAR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(\d{4}\s?\d{4}\s?\d{4})\b").unwrap()
});

static PHONE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(\+?\d{1,3}[-.\s]?\(?\d{2,4}\)?[-.\s]?\d{3,4}[-.\s]?\d{4})\b").unwrap()
});

#[derive(Clone)]
pub struct SecretDetector;

impl SecretDetector {
    pub fn new() -> Self {
        Self
    }

    pub fn check(&self, response_body: &str) -> SecretDetectionResult {
        let start = std::time::Instant::now();
        let mut findings: Vec<SecretFinding> = Vec::new();

        // AWS Access Key
        for cap in AWS_KEY_RE.captures_iter(response_body) {
            if let Some(m) = cap.get(1) {
                findings.push(SecretFinding {
                    matched_text: m.as_str().to_string(),
                    category: "AWS access key",
                    confidence: 0.98,
                });
            }
        }

        // AWS Secret Key
        for cap in AWS_SECRET_RE.captures_iter(response_body) {
            if let Some(m) = cap.get(1) {
                findings.push(SecretFinding {
                    matched_text: m.as_str().to_string(),
                    category: "AWS secret key",
                    confidence: 0.95,
                });
            }
        }

        // Generic API keys
        for cap in GENERIC_API_KEY_RE.captures_iter(response_body) {
            if let Some(m) = cap.get(1) {
                let value = m.as_str();
                let entropy = shannon_entropy(value);
                if entropy > 3.5 {
                    findings.push(SecretFinding {
                        matched_text: value.to_string(),
                        category: "API key/token",
                        confidence: (entropy / 5.0).min(0.95) as f32,
                    });
                }
            }
        }

        // Credit card numbers (with Luhn check)
        for cap in CREDIT_CARD_RE.captures_iter(response_body) {
            if let Some(m) = cap.get(1) {
                let digits: String = m.as_str().chars().filter(|c| c.is_ascii_digit()).collect();
                if digits.len() == 16 && luhn_check(&digits) {
                    findings.push(SecretFinding {
                        matched_text: m.as_str().to_string(),
                        category: "credit card number",
                        confidence: 0.97,
                    });
                }
            }
        }

        // SSN
        for cap in SSN_RE.captures_iter(response_body) {
            if let Some(m) = cap.get(1) {
                findings.push(SecretFinding {
                    matched_text: m.as_str().to_string(),
                    category: "SSN",
                    confidence: 0.90,
                });
            }
        }

        // Aadhaar numbers (12 digits)
        for cap in AADHAAR_RE.captures_iter(response_body) {
            if let Some(m) = cap.get(1) {
                let digits: String = m.as_str().chars().filter(|c| c.is_ascii_digit()).collect();
                if digits.len() == 12 && !digits.starts_with('0') && !digits.starts_with('1') {
                    findings.push(SecretFinding {
                        matched_text: m.as_str().to_string(),
                        category: "Aadhaar number",
                        confidence: 0.85,
                    });
                }
            }
        }

        // Email addresses
        for cap in EMAIL_RE.captures_iter(response_body) {
            if let Some(m) = cap.get(1) {
                findings.push(SecretFinding {
                    matched_text: m.as_str().to_string(),
                    category: "email address",
                    confidence: 0.80,
                });
            }
        }

        // Phone numbers
        for cap in PHONE_RE.captures_iter(response_body) {
            if let Some(m) = cap.get(1) {
                let digits: String = m.as_str().chars().filter(|c| c.is_ascii_digit()).collect();
                if digits.len() >= 10 {
                    findings.push(SecretFinding {
                        matched_text: m.as_str().to_string(),
                        category: "phone number",
                        confidence: 0.75,
                    });
                }
            }
        }

        let duration_ms = start.elapsed().as_millis() as u32;

        SecretDetectionResult { findings, duration_ms }
    }

    pub fn to_verdict_and_edits(&self, result: &SecretDetectionResult) -> (Option<FastPathVerdict>, Vec<ResponseEdit>) {
        if result.findings.is_empty() {
            return (None, Vec::new());
        }

        let max_confidence = result.findings.iter()
            .map(|f| f.confidence)
            .fold(0.0f32, f32::max);

        let categories: Vec<&str> = result.findings.iter()
            .map(|f| f.category)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        let reason = if categories.len() == 1 {
            format!("Detected potential {} in response — redacted", categories[0])
        } else {
            format!("Detected {} PII/secret types in response — redacted ({})",
                categories.len(), categories.join(", "))
        };

        let edits: Vec<ResponseEdit> = result.findings.iter().map(|f| {
            ResponseEdit {
                original: f.matched_text.clone(),
                replacement: format!("[REDACTED:{}]", f.category.to_uppercase().replace(' ', "_")),
                reason: format!("Redacted {}", f.category),
            }
        }).collect();

        let verdict = FastPathVerdict {
            axis: Axis::Responsibility,
            check_name: "secret_detection".to_string(),
            outcome: Outcome::Edit,
            confidence: max_confidence,
            reason,
            duration_ms: result.duration_ms,
        };

        (Some(verdict), edits)
    }
}

impl Default for SecretDetector {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SecretDetectionResult {
    pub findings: Vec<SecretFinding>,
    pub duration_ms: u32,
}

pub struct SecretFinding {
    pub matched_text: String,
    pub category: &'static str,
    pub confidence: f32,
}

/// Shannon entropy of a string (bits per character).
/// Higher entropy suggests random/secret data.
fn shannon_entropy(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }

    let mut freq = [0u32; 256];
    let len = s.len() as f64;

    for &b in s.as_bytes() {
        freq[b as usize] += 1;
    }

    freq.iter()
        .filter(|&&count| count > 0)
        .map(|&count| {
            let p = count as f64 / len;
            -p * p.log2()
        })
        .sum()
}

/// Luhn algorithm for credit card validation.
fn luhn_check(digits: &str) -> bool {
    let mut sum: u32 = 0;
    let mut alternate = false;

    for ch in digits.chars().rev() {
        let Some(mut n) = ch.to_digit(10) else {
            return false;
        };

        if alternate {
            n *= 2;
            if n > 9 {
                n -= 9;
            }
        }
        sum += n;
        alternate = !alternate;
    }

    sum % 10 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_aws_access_key() {
        let detector = SecretDetector::new();
        let body = "Here is the key: AKIAIOSFODNN7EXAMPLE for the account.";
        let result = detector.check(body);
        assert_eq!(result.findings.len(), 1);
        assert_eq!(result.findings[0].category, "AWS access key");
        assert!(result.findings[0].confidence > 0.9);
    }

    #[test]
    fn detects_credit_card_with_luhn() {
        let detector = SecretDetector::new();
        // Valid Luhn: 4532015112830366
        let body = "Card number is 4532 0151 1283 0366 for payment.";
        let result = detector.check(body);
        assert!(result.findings.iter().any(|f| f.category == "credit card number"));
    }

    #[test]
    fn rejects_invalid_credit_card() {
        let detector = SecretDetector::new();
        // Invalid Luhn
        let body = "Number 1234 5678 9012 3456 is not a card.";
        let result = detector.check(body);
        assert!(!result.findings.iter().any(|f| f.category == "credit card number"));
    }

    #[test]
    fn detects_ssn() {
        let detector = SecretDetector::new();
        let body = "SSN: 123-45-6789 is the social security number.";
        let result = detector.check(body);
        assert!(result.findings.iter().any(|f| f.category == "SSN"));
    }

    #[test]
    fn detects_email() {
        let detector = SecretDetector::new();
        let body = "Contact us at john.doe@example.com for help.";
        let result = detector.check(body);
        assert!(result.findings.iter().any(|f| f.category == "email address"));
    }

    #[test]
    fn detects_generic_api_key_with_high_entropy() {
        let detector = SecretDetector::new();
        let body = r#"api_key = "sk_test_FAKEFAKEFAKE1234567890abcdefABCD""#;
        let result = detector.check(body);
        assert!(result.findings.iter().any(|f| f.category == "API key/token"));
    }

    #[test]
    fn no_false_positive_on_normal_text() {
        let detector = SecretDetector::new();
        let body = "The weather today is sunny with a high of 75°F. \
                    Remember to drink water and stay hydrated.";
        let result = detector.check(body);
        assert!(result.findings.is_empty());
    }

    #[test]
    fn produces_edits_for_findings() {
        let detector = SecretDetector::new();
        let body = "Key: AKIAIOSFODNN7EXAMPLE, SSN: 123-45-6789";
        let result = detector.check(body);
        let (verdict, edits) = detector.to_verdict_and_edits(&result);
        assert!(verdict.is_some());
        assert_eq!(verdict.unwrap().outcome, Outcome::Edit);
        assert!(edits.len() >= 2);
        assert!(edits.iter().any(|e| e.replacement.contains("REDACTED")));
    }

    #[test]
    fn shannon_entropy_values() {
        assert!(shannon_entropy("aaaa") < 1.0);
        assert!(shannon_entropy("abcdefgh") > 2.5);
        assert!(shannon_entropy("xJ7kQ2mP9nR4tY1wZ3vB5cD8fG0hA6iE") > 4.0);
    }

    #[test]
    fn luhn_valid_cards() {
        assert!(luhn_check("4532015112830366"));
        assert!(luhn_check("5425233430109903"));
        assert!(luhn_check("4111111111111111"));
    }

    #[test]
    fn luhn_invalid_cards() {
        assert!(!luhn_check("1234567890123456"));
        assert!(!luhn_check("1111111111111112"));
    }
}
