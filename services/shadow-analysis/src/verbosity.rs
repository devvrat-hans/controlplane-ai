use std::collections::HashSet;

use controlplane_common::types::{Axis, Outcome};

use crate::types::ShadowVerdict;

/// Verbosity/redundancy heuristic — flags responses whose length
/// is disproportionate to their information content.
pub struct VerbosityChecker {
    /// Maximum acceptable verbosity ratio (response_tokens / prompt_tokens).
    max_ratio: f32,
    /// Minimum information density (unique_tokens / total_tokens).
    min_density: f32,
}

pub struct VerbosityResult {
    pub info_density: f32,
    pub length_ratio: f32,
    pub verbosity_score: f32,
    pub verdict: Option<ShadowVerdict>,
    pub duration_ms: f64,
}

impl VerbosityChecker {
    pub fn new(max_ratio: f32, min_density: f32) -> Self {
        Self { max_ratio, min_density }
    }

    /// Check response verbosity relative to the prompt.
    pub fn check(&self, response: &str, prompt: &str) -> VerbosityResult {
        let start = std::time::Instant::now();

        let response_tokens = tokenize(response);
        let prompt_tokens = tokenize(prompt);

        let response_len = response_tokens.len() as f32;
        let prompt_len = prompt_tokens.len().max(1) as f32;

        // Information density: unique tokens / total tokens
        let unique_tokens: HashSet<&str> = response_tokens.iter().copied().collect();
        let info_density = if response_len > 0.0 {
            unique_tokens.len() as f32 / response_len
        } else {
            1.0
        };

        // Length ratio: how much longer is the response vs the prompt
        let length_ratio = response_len / prompt_len;

        // Composite verbosity score (0.0 = concise, 1.0 = extremely verbose)
        let density_penalty = (1.0 - info_density).max(0.0);
        let ratio_penalty = if length_ratio > self.max_ratio {
            ((length_ratio - self.max_ratio) / self.max_ratio).min(1.0)
        } else {
            0.0
        };

        let verbosity_score = (density_penalty * 0.4 + ratio_penalty * 0.6).min(1.0);

        let duration_ms = start.elapsed().as_secs_f64() * 1000.0;

        let verdict = if verbosity_score > 0.6 || (info_density < self.min_density && length_ratio > self.max_ratio) {
            Some(ShadowVerdict {
                axis: Axis::Performance,
                check_name: "verbosity".to_string(),
                outcome: Outcome::Escalate,
                confidence: verbosity_score,
                reason: format!(
                    "Response is excessively verbose (density: {:.2}, ratio: {:.1}x, score: {:.2})",
                    info_density, length_ratio, verbosity_score
                ),
                duration_ms,
            })
        } else {
            None
        };

        VerbosityResult {
            info_density,
            length_ratio,
            verbosity_score,
            verdict,
            duration_ms,
        }
    }
}

impl Default for VerbosityChecker {
    fn default() -> Self {
        Self::new(10.0, 0.3)
    }
}

/// Simple whitespace tokenizer.
fn tokenize(text: &str) -> Vec<&str> {
    text.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|w| !w.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concise_response_passes() {
        let checker = VerbosityChecker::default();
        let prompt = "What is the capital of France?";
        let response = "The capital of France is Paris.";
        let result = checker.check(response, prompt);
        assert!(result.verdict.is_none());
        assert!(result.verbosity_score < 0.5, "Score: {}", result.verbosity_score);
    }

    #[test]
    fn excessively_repetitive_response_flagged() {
        let checker = VerbosityChecker::new(5.0, 0.3);
        let prompt = "Say hello.";
        // Very repetitive (low density)
        let response = "hello hello hello hello hello hello hello hello \
                        hello hello hello hello hello hello hello hello \
                        hello hello hello hello hello hello hello hello \
                        hello hello hello hello hello hello hello hello \
                        hello hello hello hello hello hello hello hello \
                        hello hello hello hello hello hello hello hello \
                        hello hello hello hello hello hello hello hello.";
        let result = checker.check(response, prompt);
        assert!(result.info_density < 0.1, "Density: {}", result.info_density);
        assert!(result.verdict.is_some());
    }

    #[test]
    fn proportional_long_response_passes() {
        let checker = VerbosityChecker::default();
        let prompt = "Explain how photosynthesis works in detail, \
                      including the light reactions, the Calvin cycle, \
                      and the role of chlorophyll in capturing light energy.";
        let response = "Photosynthesis is the process by which plants convert \
                        sunlight into chemical energy. Light reactions occur in \
                        thylakoid membranes where chlorophyll absorbs photons. \
                        Water molecules are split, releasing oxygen. The Calvin \
                        cycle occurs in the stroma, fixing carbon dioxide into \
                        glucose using ATP and NADPH from light reactions.";
        let result = checker.check(response, prompt);
        assert!(result.verdict.is_none(), "Score: {}", result.verbosity_score);
    }

    #[test]
    fn disproportionately_long_response_flagged() {
        let checker = VerbosityChecker::new(5.0, 0.3);
        let prompt = "What is 2+2?";
        let filler = "The answer involves many complex mathematical principles. \
                      In the field of arithmetic, numbers represent quantities. \
                      When we combine two quantities together through addition, \
                      we get a sum that represents the total. In this particular case, \
                      considering all relevant mathematical frameworks and axioms, \
                      and looking at Peano arithmetic and set theory foundations, \
                      we can definitively say that two plus two equals four. \
                      This has been verified by mathematicians across centuries. \
                      From ancient Babylonian to modern computational methods, \
                      the result has been confirmed through rigorous proof. \
                      Additionally, various mathematical proofs exist including \
                      constructive proofs and proofs by contradiction.";
        let result = checker.check(filler, prompt);
        assert!(result.length_ratio > 5.0);
    }

    #[test]
    fn info_density_calculation() {
        let checker = VerbosityChecker::default();
        let prompt = "test";

        // All unique words
        let high_density = "alpha beta gamma delta epsilon zeta eta theta";
        let result = checker.check(high_density, prompt);
        assert!(result.info_density > 0.9, "Density: {}", result.info_density);

        // All same word
        let low_density = "word word word word word word word word";
        let result = checker.check(low_density, prompt);
        assert!(result.info_density < 0.2, "Density: {}", result.info_density);
    }
}
