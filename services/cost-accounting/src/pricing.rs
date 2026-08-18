use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Per-model token pricing configuration.
/// Prices are in USD per 1 million tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPricing {
    pub input_price_per_million: f64,
    pub output_price_per_million: f64,
}

impl ModelPricing {
    pub fn compute_cost(&self, input_tokens: i32, output_tokens: i32) -> f64 {
        let input_cost = (input_tokens as f64 / 1_000_000.0) * self.input_price_per_million;
        let output_cost = (output_tokens as f64 / 1_000_000.0) * self.output_price_per_million;
        input_cost + output_cost
    }
}

/// Default pricing table for known models.
pub fn default_pricing_table() -> HashMap<String, ModelPricing> {
    let mut table = HashMap::new();

    table.insert("claude-sonnet".to_string(), ModelPricing {
        input_price_per_million: 3.0,
        output_price_per_million: 15.0,
    });

    table.insert("claude-haiku".to_string(), ModelPricing {
        input_price_per_million: 0.25,
        output_price_per_million: 1.25,
    });

    table.insert("claude-opus".to_string(), ModelPricing {
        input_price_per_million: 15.0,
        output_price_per_million: 75.0,
    });

    table.insert("gpt-4o".to_string(), ModelPricing {
        input_price_per_million: 2.5,
        output_price_per_million: 10.0,
    });

    table.insert("gpt-4o-mini".to_string(), ModelPricing {
        input_price_per_million: 0.15,
        output_price_per_million: 0.60,
    });

    // Fallback for unknown models
    table.insert("default".to_string(), ModelPricing {
        input_price_per_million: 3.0,
        output_price_per_million: 15.0,
    });

    table
}

/// Look up pricing for a model, falling back to default.
pub fn get_pricing<'a>(table: &'a HashMap<String, ModelPricing>, model: &str) -> &'a ModelPricing {
    let model_lower = model.to_lowercase();

    // Try exact match first
    if let Some(p) = table.get(&model_lower) {
        return p;
    }

    // Try prefix match (e.g., "claude-3-5-sonnet" → "claude-sonnet")
    for (key, pricing) in table {
        if model_lower.contains(key) {
            return pricing;
        }
    }

    table.get("default").unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_cost_claude_sonnet() {
        let pricing = ModelPricing {
            input_price_per_million: 3.0,
            output_price_per_million: 15.0,
        };

        // 1000 input + 500 output
        let cost = pricing.compute_cost(1000, 500);
        let expected = (1000.0 / 1_000_000.0) * 3.0 + (500.0 / 1_000_000.0) * 15.0;
        assert!((cost - expected).abs() < 1e-10);
    }

    #[test]
    fn lookup_known_model() {
        let table = default_pricing_table();
        let pricing = get_pricing(&table, "claude-sonnet");
        assert_eq!(pricing.input_price_per_million, 3.0);
    }

    #[test]
    fn lookup_prefix_match() {
        let table = default_pricing_table();
        let pricing = get_pricing(&table, "claude-3-5-sonnet-20240901");
        assert_eq!(pricing.input_price_per_million, 3.0);
    }

    #[test]
    fn lookup_unknown_falls_back() {
        let table = default_pricing_table();
        let pricing = get_pricing(&table, "some-unknown-model-v99");
        assert_eq!(pricing.input_price_per_million, 3.0); // default
    }
}
