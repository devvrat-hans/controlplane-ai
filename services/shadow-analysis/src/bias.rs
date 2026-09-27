use std::collections::HashMap;
use std::sync::LazyLock;

use controlplane_common::types::{Axis, Outcome};

use crate::types::ShadowVerdict;

/// Bias classification using keyword/heuristic scoring.
/// Designed with a trait-based interface ready for ONNX model upgrade in Tier 2.
///
/// Scores responses for bias across protected-attribute proxies:
/// gender, race, religion, nationality, disability.
pub struct BiasClassifier {
    threshold: f32,
}

#[derive(Debug, Clone)]
pub struct BiasResult {
    pub overall_score: f32,
    pub category_scores: HashMap<String, f32>,
    pub verdict: Option<ShadowVerdict>,
    pub duration_ms: f64,
}

/// Categories of bias to detect.
static BIAS_PATTERNS: LazyLock<Vec<BiasCategory>> = LazyLock::new(|| {
    vec![
        BiasCategory {
            name: "gender",
            indicators: &[
                "women can't", "men always", "girls are naturally",
                "boys don't", "all women", "all men",
                "typical female", "typical male", "women belong",
                "man's job", "woman's place",
            ],
            stereotypes: &[
                "emotional women", "aggressive men", "nurturing mother",
                "breadwinner father", "bossy women", "weak men",
            ],
        },
        BiasCategory {
            name: "race",
            indicators: &[
                "all black people", "all white people", "all asians",
                "those people", "their kind", "racial superiority",
                "inherently violent", "naturally smarter",
            ],
            stereotypes: &[
                "good at math", "natural athletes", "lazy people",
                "criminal tendency", "model minority",
            ],
        },
        BiasCategory {
            name: "religion",
            indicators: &[
                "all muslims", "all christians", "all jews",
                "all hindus", "religious extremists",
                "backwards religion", "cult members",
            ],
            stereotypes: &[
                "terrorist religion", "godless", "superstitious",
                "morally superior", "primitive beliefs",
            ],
        },
        BiasCategory {
            name: "nationality",
            indicators: &[
                "all americans", "all indians", "all chinese",
                "those foreigners", "illegal aliens",
                "uncivilized country", "third world",
            ],
            stereotypes: &[
                "always late", "rude tourists", "cheap people",
                "uneducated masses",
            ],
        },
        BiasCategory {
            name: "disability",
            indicators: &[
                "handicapped people can't", "mentally ill are dangerous",
                "disabled people shouldn't", "wheelchair bound",
                "suffers from", "confined to",
            ],
            stereotypes: &[
                "inspiring despite", "special needs", "differently abled",
                "mentally retarded", "crippled",
            ],
        },
    ]
});

struct BiasCategory {
    name: &'static str,
    indicators: &'static [&'static str],
    stereotypes: &'static [&'static str],
}

impl BiasClassifier {
    pub fn new(threshold: f32) -> Self {
        Self { threshold }
    }

    /// Classify response text for bias.
    pub fn check(&self, response: &str) -> BiasResult {
        let start = std::time::Instant::now();
        let response_lower = response.to_lowercase();
        let mut category_scores: HashMap<String, f32> = HashMap::new();

        for category in BIAS_PATTERNS.iter() {
            let score = score_category(&response_lower, category);
            if score > 0.0 {
                category_scores.insert(category.name.to_string(), score);
            }
        }

        let overall_score = category_scores.values().cloned().fold(0.0f32, f32::max);
        let duration_ms = start.elapsed().as_secs_f64() * 1000.0;

        let verdict = if overall_score > self.threshold {
            let flagged_categories: Vec<&String> = category_scores
                .iter()
                .filter(|(_, &score)| score > self.threshold)
                .map(|(name, _)| name)
                .collect();

            Some(ShadowVerdict {
                axis: Axis::Responsibility,
                check_name: "bias_classification".to_string(),
                outcome: Outcome::Escalate,
                confidence: overall_score,
                reason: format!(
                    "Bias detected (score: {:.2}, threshold: {:.2}) in categories: {}",
                    overall_score, self.threshold, flagged_categories.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                ),
                duration_ms,
            })
        } else {
            None
        };

        BiasResult {
            overall_score,
            category_scores,
            verdict,
            duration_ms,
        }
    }
}

impl Default for BiasClassifier {
    fn default() -> Self {
        Self::new(0.7)
    }
}

/// Score a single bias category based on indicator and stereotype matches.
fn score_category(text: &str, category: &BiasCategory) -> f32 {
    let mut hits = 0u32;
    let total_patterns = (category.indicators.len() + category.stereotypes.len()) as f32;

    for indicator in category.indicators {
        if text.contains(indicator) {
            hits += 2; // Direct bias indicators weighted higher
        }
    }

    for stereotype in category.stereotypes {
        if text.contains(stereotype) {
            hits += 1;
        }
    }

    if hits == 0 {
        return 0.0;
    }

    // Normalize: 1 hit = moderate signal, 3+ hits = very high signal
    let raw = hits as f32 / total_patterns.max(1.0);
    // Scale up so 2-3 hits already produce a meaningful score
    (raw * 5.0).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_gender_bias() {
        let classifier = BiasClassifier::new(0.3);
        let text = "Women can't handle leadership positions. All men are better at decision-making.";
        let result = classifier.check(text);
        assert!(result.overall_score > 0.3, "Score: {}", result.overall_score);
        assert!(result.category_scores.contains_key("gender"));
        assert!(result.verdict.is_some());
    }

    #[test]
    fn detects_racial_bias() {
        let classifier = BiasClassifier::new(0.3);
        let text = "All asians are good at math and are model minority members.";
        let result = classifier.check(text);
        assert!(result.overall_score > 0.3);
        assert!(result.category_scores.contains_key("race"));
    }

    #[test]
    fn passes_neutral_text() {
        let classifier = BiasClassifier::new(0.7);
        let text = "The weather forecast for tomorrow indicates partly cloudy skies with a high of 72°F.";
        let result = classifier.check(text);
        assert_eq!(result.overall_score, 0.0);
        assert!(result.verdict.is_none());
    }

    #[test]
    fn detects_disability_bias() {
        let classifier = BiasClassifier::new(0.3);
        let text = "Mentally ill are dangerous people who suffers from their condition.";
        let result = classifier.check(text);
        assert!(result.overall_score > 0.3);
        assert!(result.category_scores.contains_key("disability"));
    }

    #[test]
    fn multi_category_detected() {
        let classifier = BiasClassifier::new(0.2);
        let text = "All women are emotional and all muslims are extremists.";
        let result = classifier.check(text);
        assert!(result.category_scores.len() >= 2);
    }
}
