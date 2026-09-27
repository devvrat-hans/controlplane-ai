use std::collections::HashSet;

use controlplane_common::types::{Axis, Outcome};

use crate::types::ShadowVerdict;

/// Groundedness/hallucination scoring — compares response claims
/// against the provided context documents.
///
/// Simple approach for demo: sentence-level overlap scoring.
/// Score: 0.0 (completely ungrounded) to 1.0 (fully grounded).
pub struct GroundednessChecker {
    threshold: f32,
}

impl GroundednessChecker {
    pub fn new(threshold: f32) -> Self {
        Self { threshold }
    }

    /// Score how grounded the response is relative to the context.
    /// Returns (score, verdict_if_any).
    pub fn check(&self, response: &str, context: Option<&str>) -> GroundednessResult {
        let start = std::time::Instant::now();

        let context_text = match context {
            Some(c) if !c.is_empty() => c,
            _ => {
                // No context provided — can't assess groundedness, pass
                return GroundednessResult {
                    score: 1.0,
                    verdict: None,
                    duration_ms: start.elapsed().as_secs_f64() * 1000.0,
                };
            }
        };

        let score = sentence_overlap_score(response, context_text);
        let duration_ms = start.elapsed().as_secs_f64() * 1000.0;

        let verdict = if score < self.threshold {
            Some(ShadowVerdict {
                axis: Axis::Performance,
                check_name: "groundedness".to_string(),
                outcome: Outcome::Escalate,
                confidence: 1.0 - score,
                reason: format!(
                    "Low groundedness score ({:.2}) below threshold ({:.2}): response may contain hallucinations",
                    score, self.threshold
                ),
                duration_ms,
            })
        } else {
            None
        };

        GroundednessResult { score, verdict, duration_ms }
    }
}

impl Default for GroundednessChecker {
    fn default() -> Self {
        Self::new(0.6)
    }
}

pub struct GroundednessResult {
    pub score: f32,
    pub verdict: Option<ShadowVerdict>,
    pub duration_ms: f64,
}

/// Sentence-level overlap scoring:
/// For each sentence in the response, check what fraction of its significant
/// words appear in the context. Average across all sentences.
fn sentence_overlap_score(response: &str, context: &str) -> f32 {
    let context_words = extract_significant_words(context);
    if context_words.is_empty() {
        return 1.0;
    }

    let sentences = split_sentences(response);
    if sentences.is_empty() {
        return 1.0;
    }

    let mut total_score = 0.0;
    let mut scored_count = 0;

    for sentence in &sentences {
        let words = extract_significant_words(sentence);
        if words.is_empty() {
            continue;
        }

        let matching = words.iter().filter(|w| context_words.contains(*w)).count();
        let ratio = matching as f32 / words.len() as f32;
        total_score += ratio;
        scored_count += 1;
    }

    if scored_count == 0 {
        return 1.0;
    }

    total_score / scored_count as f32
}

/// Split text into sentences (simple heuristic: split on `.`, `!`, `?`).
fn split_sentences(text: &str) -> Vec<&str> {
    text.split(['.', '!', '?'])
        .map(|s| s.trim())
        .filter(|s| s.len() > 5)
        .collect()
}

/// Extract significant words (lowercased, >3 chars, excluding stop words).
fn extract_significant_words(text: &str) -> HashSet<String> {
    const STOP_WORDS: &[&str] = &[
        "the", "and", "for", "are", "but", "not", "you", "all", "any",
        "can", "had", "her", "was", "one", "our", "out", "has", "have",
        "this", "that", "with", "they", "from", "will", "been", "each",
        "make", "like", "them", "than", "then", "what", "when", "were",
        "into", "also", "some", "more", "very", "just", "about", "which",
        "would", "there", "their", "could", "other", "should",
    ];

    text.split_whitespace()
        .map(|w| w.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| w.len() > 3 && !STOP_WORDS.contains(&w.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_groundedness_when_response_matches_context() {
        let checker = GroundednessChecker::new(0.6);
        let context = "The capital of France is Paris. Paris has a population of 2.1 million.";
        let response = "Paris is the capital of France with 2.1 million people.";
        let result = checker.check(response, Some(context));
        assert!(result.score > 0.6, "Score was {}", result.score);
        assert!(result.verdict.is_none());
    }

    #[test]
    fn low_groundedness_when_response_unrelated_to_context() {
        let checker = GroundednessChecker::new(0.6);
        let context = "The weather in London is typically rainy with cool temperatures.";
        let response = "Quantum computing uses qubits to perform calculations exponentially faster than classical computers.";
        let result = checker.check(response, Some(context));
        assert!(result.score < 0.4, "Score was {}", result.score);
        assert!(result.verdict.is_some());
        assert_eq!(result.verdict.unwrap().outcome, Outcome::Escalate);
    }

    #[test]
    fn passes_when_no_context() {
        let checker = GroundednessChecker::new(0.6);
        let result = checker.check("Some random response.", None);
        assert_eq!(result.score, 1.0);
        assert!(result.verdict.is_none());
    }

    #[test]
    fn passes_when_empty_context() {
        let checker = GroundednessChecker::new(0.6);
        let result = checker.check("Some response.", Some(""));
        assert_eq!(result.score, 1.0);
        assert!(result.verdict.is_none());
    }

    #[test]
    fn sentence_splitting_works() {
        let sentences = split_sentences("Hello world. This is a test! How are you?");
        assert_eq!(sentences.len(), 3);
    }

    #[test]
    fn significant_words_filters_stop_words() {
        let words = extract_significant_words("The quick brown fox jumps over the lazy dog");
        assert!(words.contains("quick"));
        assert!(words.contains("brown"));
        assert!(words.contains("jumps"));
        assert!(words.contains("lazy"));
        assert!(!words.contains("the")); // stop word
        assert!(!words.contains("fox")); // too short (3 chars)
    }
}
