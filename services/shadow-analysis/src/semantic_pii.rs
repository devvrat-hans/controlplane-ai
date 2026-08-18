use std::collections::HashSet;

use controlplane_common::types::{Axis, Outcome};

use crate::types::ShadowVerdict;

/// Semantic PII / re-identification risk detection.
/// Detects combinations of quasi-identifiers that together could
/// re-identify an individual (k-anonymity violation).
///
/// More compute-intensive than simple regex (fast-path) — analyzes
/// semantic patterns and co-occurrence of identifying attributes.
pub struct SemanticPiiDetector {
    /// Minimum number of quasi-identifiers to trigger a risk flag.
    min_quasi_identifiers: usize,
    risk_threshold: f32,
}

pub struct SemanticPiiResult {
    pub quasi_identifiers_found: Vec<QuasiIdentifier>,
    pub risk_score: f32,
    pub verdict: Option<ShadowVerdict>,
    pub duration_ms: u32,
}

#[derive(Debug, Clone)]
pub struct QuasiIdentifier {
    pub category: &'static str,
    pub snippet: String,
}

/// Categories of quasi-identifiers that together can re-identify individuals.
const QUASI_ID_PATTERNS: &[(&str, &[&str])] = &[
    ("age/dob", &[
        "born in", "years old", "age of", "birthday", "date of birth",
        "turned 18", "turned 21", "turned 30", "turned 40", "turned 50",
    ]),
    ("location", &[
        "lives in", "residing at", "located in", "from the city of",
        "neighborhood of", "apartment", "street address",
        "zip code", "postal code", "block number",
    ]),
    ("occupation", &[
        "works at", "employed by", "job title", "profession is",
        "career as", "works as a", "position of",
    ]),
    ("education", &[
        "graduated from", "attended university", "studied at",
        "class of", "alumni of", "degree from",
    ]),
    ("medical", &[
        "diagnosed with", "medical condition", "prescription for",
        "treatment for", "hospital visit", "doctor appointment",
        "blood type", "allergic to",
    ]),
    ("financial", &[
        "salary of", "earns", "bank account", "credit score",
        "annual income", "net worth", "tax bracket",
    ]),
    ("family", &[
        "married to", "spouse named", "children named",
        "son is", "daughter is", "parent of",
        "family of", "brother named", "sister named",
    ]),
    ("biometric", &[
        "fingerprint", "facial recognition", "retina scan",
        "voice pattern", "dna profile", "genetic marker",
    ]),
];

impl SemanticPiiDetector {
    pub fn new(min_quasi_identifiers: usize, risk_threshold: f32) -> Self {
        Self { min_quasi_identifiers, risk_threshold }
    }

    pub fn check(&self, response: &str) -> SemanticPiiResult {
        let start = std::time::Instant::now();
        let response_lower = response.to_lowercase();

        let mut found: Vec<QuasiIdentifier> = Vec::new();
        let mut categories_hit: HashSet<&str> = HashSet::new();

        for (category, patterns) in QUASI_ID_PATTERNS {
            for pattern in *patterns {
                if let Some(pos) = response_lower.find(pattern) {
                    // Extract a snippet of context around the match
                    let snippet_start = pos.saturating_sub(10);
                    let snippet_end = (pos + pattern.len() + 30).min(response.len());
                    let snippet = response[snippet_start..snippet_end].to_string();

                    if !categories_hit.contains(category) {
                        found.push(QuasiIdentifier {
                            category,
                            snippet,
                        });
                        categories_hit.insert(category);
                    }
                    break; // One hit per category is enough
                }
            }
        }

        // Risk score based on number of distinct quasi-identifier categories
        // 1 category = low risk, 3+ = high re-identification risk
        let risk_score = if found.is_empty() {
            0.0
        } else {
            let category_count = categories_hit.len() as f32;
            (category_count / 4.0).min(1.0) // 4+ categories = max risk
        };

        let duration_ms = start.elapsed().as_millis() as u32;

        let verdict = if categories_hit.len() >= self.min_quasi_identifiers && risk_score >= self.risk_threshold {
            let cats: Vec<&str> = categories_hit.into_iter().collect();
            Some(ShadowVerdict {
                axis: Axis::Responsibility,
                check_name: "semantic_pii".to_string(),
                outcome: Outcome::Escalate,
                confidence: risk_score,
                reason: format!(
                    "Re-identification risk: {} quasi-identifier categories detected ({}). \
                     Combined data may uniquely identify an individual.",
                    found.len(),
                    cats.join(", ")
                ),
                duration_ms,
            })
        } else {
            None
        };

        SemanticPiiResult {
            quasi_identifiers_found: found,
            risk_score,
            verdict,
            duration_ms,
        }
    }
}

impl Default for SemanticPiiDetector {
    fn default() -> Self {
        Self::new(3, 0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_multiple_quasi_identifiers() {
        let detector = SemanticPiiDetector::new(3, 0.5);
        let response = "John lives in apartment 4B on Oak Street, \
                        works at Acme Corp as a senior engineer, \
                        and graduated from MIT class of 2015.";
        let result = detector.check(response);
        assert!(result.quasi_identifiers_found.len() >= 3);
        assert!(result.risk_score >= 0.5);
        assert!(result.verdict.is_some());
    }

    #[test]
    fn passes_when_few_identifiers() {
        let detector = SemanticPiiDetector::new(3, 0.5);
        let response = "The person lives in a city and enjoys reading books.";
        let result = detector.check(response);
        assert!(result.quasi_identifiers_found.len() < 3);
        assert!(result.verdict.is_none());
    }

    #[test]
    fn detects_medical_and_location() {
        let detector = SemanticPiiDetector::new(2, 0.3);
        let response = "The patient was diagnosed with diabetes and \
                        resides at 123 Main Street, zip code 90210.";
        let result = detector.check(response);
        assert!(result.quasi_identifiers_found.len() >= 2);
    }

    #[test]
    fn no_false_positives_on_generic_text() {
        let detector = SemanticPiiDetector::default();
        let response = "Machine learning algorithms use training data \
                        to make predictions. Neural networks consist of \
                        layers of interconnected nodes.";
        let result = detector.check(response);
        assert!(result.quasi_identifiers_found.is_empty());
        assert_eq!(result.risk_score, 0.0);
    }

    #[test]
    fn high_risk_with_many_categories() {
        let detector = SemanticPiiDetector::new(2, 0.3);
        let response = "Jane is 35 years old, lives in Chicago, \
                        works at Google, graduated from Stanford, \
                        and was diagnosed with asthma.";
        let result = detector.check(response);
        assert!(result.risk_score > 0.7);
        assert!(result.verdict.is_some());
    }
}
