use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use tracing::{error, info, warn};
use uuid::Uuid;

use controlplane_common::events::{subjects, EventEnvelope, InterceptCapturedPayload};
use controlplane_common::types::AppId;
use controlplane_platform::messaging::EventSubscriber;

use crate::pricing::{default_pricing_table, get_pricing, ModelPricing};

/// Cost ledger: tracks per-request costs and maintains running windows.
pub struct CostLedger {
    pool: PgPool,
    pricing_table: HashMap<String, ModelPricing>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CostWindowSummary {
    pub app_id: Uuid,
    pub window_start: DateTime<Utc>,
    pub window_end: DateTime<Utc>,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_cost_usd: f64,
    pub request_count: i64,
}

#[derive(Debug, Clone)]
pub struct CostAnomaly {
    pub app_id: Uuid,
    pub detected_at: DateTime<Utc>,
    pub metric: String,
    pub current_value: f64,
    pub baseline_value: f64,
    pub deviation_factor: f64,
    pub reason: String,
}

impl CostLedger {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            pricing_table: default_pricing_table(),
        }
    }

    pub fn with_pricing(mut self, table: HashMap<String, ModelPricing>) -> Self {
        self.pricing_table = table;
        self
    }

    /// Record a single request's token usage and cost.
    pub async fn record_usage(
        &self,
        app_id: AppId,
        model: &str,
        input_tokens: Option<i32>,
        output_tokens: Option<i32>,
    ) -> Result<f64, sqlx::Error> {
        let input = input_tokens.unwrap_or(0);
        let output = output_tokens.unwrap_or(0);
        let pricing = get_pricing(&self.pricing_table, model);
        let cost_usd = pricing.compute_cost(input, output);

        sqlx::query(
            "INSERT INTO cost_entries (id, app_id, model, input_tokens, output_tokens, cost_usd, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)"
        )
        .bind(Uuid::now_v7())
        .bind(app_id)
        .bind(model)
        .bind(input)
        .bind(output)
        .bind(cost_usd)
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;

        Ok(cost_usd)
    }

    /// Get cost summary for an app over a time window.
    pub async fn get_summary(
        &self,
        app_id: AppId,
        window: &str,
    ) -> Result<CostWindowSummary, sqlx::Error> {
        let duration = parse_window_duration(window);
        let window_start = Utc::now() - duration;
        let window_end = Utc::now();

        let row = sqlx::query_as::<_, SummaryRow>(
            "SELECT \
                COALESCE(SUM(input_tokens), 0) as total_input_tokens, \
                COALESCE(SUM(output_tokens), 0) as total_output_tokens, \
                COALESCE(SUM(cost_usd), 0.0) as total_cost_usd, \
                COUNT(*) as request_count \
             FROM cost_entries \
             WHERE app_id = $1 AND created_at >= $2"
        )
        .bind(app_id)
        .bind(window_start)
        .fetch_one(&self.pool)
        .await?;

        Ok(CostWindowSummary {
            app_id,
            window_start,
            window_end,
            total_input_tokens: row.total_input_tokens,
            total_output_tokens: row.total_output_tokens,
            total_cost_usd: row.total_cost_usd,
            request_count: row.request_count,
        })
    }

    /// Detect cost anomalies for an app by comparing recent usage to baseline.
    pub async fn detect_anomalies(
        &self,
        app_id: AppId,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
    ) -> Result<Vec<CostAnomaly>, sqlx::Error> {
        let to = to.unwrap_or_else(Utc::now);
        let from = from.unwrap_or_else(|| to - Duration::hours(1));

        // Get recent window stats
        let recent = sqlx::query_as::<_, SummaryRow>(
            "SELECT \
                COALESCE(SUM(input_tokens), 0) as total_input_tokens, \
                COALESCE(SUM(output_tokens), 0) as total_output_tokens, \
                COALESCE(SUM(cost_usd), 0.0) as total_cost_usd, \
                COUNT(*) as request_count \
             FROM cost_entries \
             WHERE app_id = $1 AND created_at >= $2 AND created_at <= $3"
        )
        .bind(app_id)
        .bind(from)
        .bind(to)
        .fetch_one(&self.pool)
        .await?;

        // Get 7-day baseline (same window duration, average per period)
        let baseline_start = from - Duration::days(7);
        let baseline = sqlx::query_as::<_, SummaryRow>(
            "SELECT \
                COALESCE(SUM(input_tokens), 0) as total_input_tokens, \
                COALESCE(SUM(output_tokens), 0) as total_output_tokens, \
                COALESCE(SUM(cost_usd), 0.0) as total_cost_usd, \
                COUNT(*) as request_count \
             FROM cost_entries \
             WHERE app_id = $1 AND created_at >= $2 AND created_at < $3"
        )
        .bind(app_id)
        .bind(baseline_start)
        .bind(from)
        .fetch_one(&self.pool)
        .await?;

        let mut anomalies = Vec::new();
        let window_hours = (to - from).num_hours().max(1) as f64;
        let baseline_hours = 7.0 * 24.0;

        // Normalize to per-hour rates
        let recent_cost_rate = recent.total_cost_usd / window_hours;
        let baseline_cost_rate = baseline.total_cost_usd / baseline_hours;

        let recent_token_rate = (recent.total_output_tokens as f64) / window_hours;
        let baseline_token_rate = (baseline.total_output_tokens as f64) / baseline_hours;

        // Check cost anomaly (2x deviation = anomaly)
        if baseline_cost_rate > 0.0 {
            let deviation = recent_cost_rate / baseline_cost_rate;
            if deviation > 2.0 {
                anomalies.push(CostAnomaly {
                    app_id,
                    detected_at: Utc::now(),
                    metric: "cost_per_hour".to_string(),
                    current_value: recent_cost_rate,
                    baseline_value: baseline_cost_rate,
                    deviation_factor: deviation,
                    reason: format!(
                        "{:.1}x normal cost detected (${:.4}/hr vs baseline ${:.4}/hr)",
                        deviation, recent_cost_rate, baseline_cost_rate
                    ),
                });
            }
        }

        // Check token anomaly
        if baseline_token_rate > 0.0 {
            let deviation = recent_token_rate / baseline_token_rate;
            if deviation > 2.0 {
                anomalies.push(CostAnomaly {
                    app_id,
                    detected_at: Utc::now(),
                    metric: "tokens_per_hour".to_string(),
                    current_value: recent_token_rate,
                    baseline_value: baseline_token_rate,
                    deviation_factor: deviation,
                    reason: format!(
                        "{:.1}x normal token usage detected ({:.0} tokens/hr vs baseline {:.0} tokens/hr)",
                        deviation, recent_token_rate, baseline_token_rate
                    ),
                });
            }
        }

        Ok(anomalies)
    }
}

#[derive(sqlx::FromRow)]
struct SummaryRow {
    total_input_tokens: i64,
    total_output_tokens: i64,
    total_cost_usd: f64,
    request_count: i64,
}

fn parse_window_duration(window: &str) -> Duration {
    match window {
        "1m" => Duration::minutes(1),
        "5m" => Duration::minutes(5),
        "15m" => Duration::minutes(15),
        "1h" => Duration::hours(1),
        "6h" => Duration::hours(6),
        "1d" | "24h" => Duration::days(1),
        "7d" => Duration::days(7),
        "30d" => Duration::days(30),
        _ => Duration::hours(1), // default
    }
}

/// Background subscriber that records token usage from intercept events.
pub fn spawn_cost_tracker(
    pool: PgPool,
    subscriber: Arc<dyn EventSubscriber>,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    let ledger = CostLedger::new(pool);

    tokio::spawn(async move {
        let mut receiver = match subscriber.subscribe(subjects::INTERCEPT_CAPTURED).await {
            Ok(rx) => rx,
            Err(e) => {
                error!(error = %e, "Cost tracker: failed to subscribe");
                return;
            }
        };

        info!("Cost tracker: listening on '{}'", subjects::INTERCEPT_CAPTURED);

        loop {
            tokio::select! {
                msg = receiver.recv() => {
                    match msg {
                        Some(payload) => {
                            match serde_json::from_slice::<EventEnvelope<InterceptCapturedPayload>>(&payload) {
                                Ok(envelope) => {
                                    let data = &envelope.payload;
                                    if let Err(e) = ledger.record_usage(
                                        envelope.app_id,
                                        &data.model,
                                        data.token_count_input,
                                        data.token_count_output,
                                    ).await {
                                        warn!(error = %e, "Failed to record cost");
                                    }
                                }
                                Err(e) => {
                                    warn!(error = %e, "Failed to deserialize intercept payload");
                                }
                            }
                        }
                        None => {
                            info!("Cost tracker subscription closed");
                            break;
                        }
                    }
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("Cost tracker shutting down");
                        break;
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_window_duration_values() {
        assert_eq!(parse_window_duration("1m").num_minutes(), 1);
        assert_eq!(parse_window_duration("1h").num_hours(), 1);
        assert_eq!(parse_window_duration("1d").num_days(), 1);
        assert_eq!(parse_window_duration("7d").num_days(), 7);
        assert_eq!(parse_window_duration("30d").num_days(), 30);
        assert_eq!(parse_window_duration("invalid").num_hours(), 1);
    }
}
