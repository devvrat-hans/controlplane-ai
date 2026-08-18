use serde::{Deserialize, Serialize};

use crate::types::Outcome;

// =============================================================================
// Policy threshold configurations (stored as JSONB in PostgreSQL)
// =============================================================================

/// Performance axis policy config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformancePolicyConfig {
    pub groundedness_threshold: f32,
    pub action: Outcome,
}

impl Default for PerformancePolicyConfig {
    fn default() -> Self {
        Self {
            groundedness_threshold: 0.6,
            action: Outcome::Escalate,
        }
    }
}

/// Cost axis policy config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostPolicyConfig {
    pub max_tokens_per_request: Option<i32>,
    pub daily_budget_cents: Option<i64>,
    pub retry_max: u32,
    pub retry_window_seconds: u64,
    pub action: Outcome,
}

impl Default for CostPolicyConfig {
    fn default() -> Self {
        Self {
            max_tokens_per_request: Some(4096),
            daily_budget_cents: Some(10000),
            retry_max: 5,
            retry_window_seconds: 60,
            action: Outcome::Block,
        }
    }
}

/// Responsibility axis policy config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsibilityPolicyConfig {
    pub pii_action: Outcome,
    pub bias_threshold: f32,
    pub bias_action: Outcome,
    pub unsafe_action: Outcome,
}

impl Default for ResponsibilityPolicyConfig {
    fn default() -> Self {
        Self {
            pii_action: Outcome::Edit,
            bias_threshold: 0.7,
            bias_action: Outcome::Escalate,
            unsafe_action: Outcome::Block,
        }
    }
}

// =============================================================================
// Model pricing config (for cost accounting)
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPricing {
    pub model_name: String,
    pub input_cost_per_million_tokens: f64,
    pub output_cost_per_million_tokens: f64,
}

impl ModelPricing {
    pub fn cost_cents(&self, input_tokens: i32, output_tokens: i32) -> i64 {
        let input_cost = (input_tokens as f64 / 1_000_000.0) * self.input_cost_per_million_tokens;
        let output_cost = (output_tokens as f64 / 1_000_000.0) * self.output_cost_per_million_tokens;
        ((input_cost + output_cost) * 100.0).round() as i64
    }
}

pub fn default_pricing() -> Vec<ModelPricing> {
    vec![
        ModelPricing {
            model_name: "claude-sonnet-4-20250514".into(),
            input_cost_per_million_tokens: 3.0,
            output_cost_per_million_tokens: 15.0,
        },
        ModelPricing {
            model_name: "claude-haiku-4-20250514".into(),
            input_cost_per_million_tokens: 0.80,
            output_cost_per_million_tokens: 4.0,
        },
        ModelPricing {
            model_name: "claude-opus-4-20250514".into(),
            input_cost_per_million_tokens: 15.0,
            output_cost_per_million_tokens: 75.0,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_pricing_calculation() {
        let pricing = ModelPricing {
            model_name: "claude-sonnet-4-20250514".into(),
            input_cost_per_million_tokens: 3.0,
            output_cost_per_million_tokens: 15.0,
        };

        // 1000 input tokens + 500 output tokens
        // input: 1000/1M * $3 = $0.003
        // output: 500/1M * $15 = $0.0075
        // total: $0.0105 = 1.05 cents ≈ 1 cent (rounded)
        let cost = pricing.cost_cents(1000, 500);
        assert_eq!(cost, 1);

        // 100K input + 50K output
        // input: 0.1 * $3 = $0.30
        // output: 0.05 * $15 = $0.75
        // total: $1.05 = 105 cents
        let cost = pricing.cost_cents(100_000, 50_000);
        assert_eq!(cost, 105);
    }

    #[test]
    fn default_configs_are_valid() {
        let perf = PerformancePolicyConfig::default();
        assert!(perf.groundedness_threshold > 0.0 && perf.groundedness_threshold <= 1.0);

        let cost = CostPolicyConfig::default();
        assert!(cost.retry_max > 0);

        let resp = ResponsibilityPolicyConfig::default();
        assert!(resp.bias_threshold > 0.0 && resp.bias_threshold <= 1.0);
    }

    #[test]
    fn policy_configs_serialize_to_json() {
        let perf = PerformancePolicyConfig::default();
        let json = serde_json::to_value(&perf).unwrap();
        let threshold = json["groundedness_threshold"].as_f64().unwrap();
        assert!((threshold - 0.6).abs() < 0.001);

        let cost = CostPolicyConfig::default();
        let json = serde_json::to_value(&cost).unwrap();
        assert_eq!(json["max_tokens_per_request"], 4096);
    }
}
