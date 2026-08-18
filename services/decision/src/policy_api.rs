use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use controlplane_common::events::{subjects, EventEnvelope};
use controlplane_platform::messaging::EventPublisher;

pub struct PolicyApiState {
    pub pool: PgPool,
    pub publisher: Arc<dyn EventPublisher>,
}

/// Router for policy CRUD endpoints.
pub fn policy_crud_router(state: Arc<PolicyApiState>) -> Router {
    Router::new()
        .route("/api/v1/policies/{app_id}", get(get_policy).put(update_policy))
        .route("/api/v1/policies/{app_id}/history", get(get_policy_history))
        .with_state(state)
}

// === Request/Response Types ===

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PolicyConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub performance: Option<PerformancePolicyConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<CostPolicyConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responsibility: Option<ResponsibilityPolicyConfig>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PerformancePolicyConfig {
    pub groundedness_threshold: Option<f64>,
    pub action: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CostPolicyConfig {
    pub max_tokens_per_request: Option<i64>,
    pub daily_budget_cents: Option<i64>,
    pub retry_max: Option<i64>,
    pub retry_window_seconds: Option<i64>,
    pub action: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ResponsibilityPolicyConfig {
    pub bias_threshold: Option<f64>,
    pub pii_action: Option<String>,
    pub unsafe_action: Option<String>,
    pub unsafe_keywords: Option<Vec<String>>,
}

#[derive(Serialize)]
struct PolicyResponse {
    app_id: Uuid,
    policies: Vec<PolicyAxisResponse>,
}

#[derive(Serialize)]
struct PolicyAxisResponse {
    axis: String,
    threshold_config: serde_json::Value,
    version: i32,
    is_active: bool,
    updated_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct PolicyHistoryResponse {
    app_id: Uuid,
    history: Vec<PolicyHistoryEntry>,
}

#[derive(Serialize)]
struct PolicyHistoryEntry {
    id: Uuid,
    axis: String,
    threshold_config: serde_json::Value,
    version: i32,
    is_active: bool,
    updated_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct UpdatePolicyRequest {
    #[serde(default)]
    updated_by: Option<Uuid>,
    policy: PolicyConfig,
}

#[derive(Serialize)]
struct UpdatePolicyResponse {
    app_id: Uuid,
    updated_axes: Vec<String>,
    new_version: i32,
}

// === DB Row Types ===

#[derive(sqlx::FromRow)]
struct PolicyRow {
    id: Uuid,
    axis: String,
    threshold_config: serde_json::Value,
    version: i32,
    is_active: bool,
    updated_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
}

// === Handlers ===

async fn get_policy(
    State(state): State<Arc<PolicyApiState>>,
    Path(app_id): Path<Uuid>,
) -> Result<Json<PolicyResponse>, (StatusCode, String)> {
    let rows = sqlx::query_as::<_, PolicyRow>(
        "SELECT id, axis, threshold_config, version, is_active, updated_at, created_at \
         FROM policies WHERE app_id = $1 AND is_active = TRUE ORDER BY axis"
    )
    .bind(app_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let policies: Vec<PolicyAxisResponse> = rows.iter().map(|r| PolicyAxisResponse {
        axis: r.axis.clone(),
        threshold_config: r.threshold_config.clone(),
        version: r.version,
        is_active: r.is_active,
        updated_at: r.updated_at,
    }).collect();

    Ok(Json(PolicyResponse { app_id, policies }))
}

async fn get_policy_history(
    State(state): State<Arc<PolicyApiState>>,
    Path(app_id): Path<Uuid>,
) -> Result<Json<PolicyHistoryResponse>, (StatusCode, String)> {
    let rows = sqlx::query_as::<_, PolicyRow>(
        "SELECT id, axis, threshold_config, version, is_active, updated_at, created_at \
         FROM policies WHERE app_id = $1 ORDER BY version DESC, axis"
    )
    .bind(app_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let history: Vec<PolicyHistoryEntry> = rows.iter().map(|r| PolicyHistoryEntry {
        id: r.id,
        axis: r.axis.clone(),
        threshold_config: r.threshold_config.clone(),
        version: r.version,
        is_active: r.is_active,
        updated_at: r.updated_at,
        created_at: r.created_at,
    }).collect();

    Ok(Json(PolicyHistoryResponse { app_id, history }))
}

async fn update_policy(
    State(state): State<Arc<PolicyApiState>>,
    Path(app_id): Path<Uuid>,
    Json(request): Json<UpdatePolicyRequest>,
) -> Result<Json<UpdatePolicyResponse>, (StatusCode, String)> {
    // Get current max version for this app
    let current_max: Option<i32> = sqlx::query_scalar(
        "SELECT MAX(version) FROM policies WHERE app_id = $1"
    )
    .bind(app_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let new_version = current_max.unwrap_or(0) + 1;
    let updated_by = request.updated_by;
    let mut updated_axes: Vec<String> = Vec::new();

    // Deactivate old active policies for axes being updated
    let mut tx = state.pool.begin().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("TX error: {e}")))?;

    if let Some(perf) = &request.policy.performance {
        let config = serde_json::json!({
            "groundedness_threshold": perf.groundedness_threshold.unwrap_or(0.6),
            "action": perf.action.as_deref().unwrap_or("escalate"),
        });
        upsert_policy_axis(&mut tx, app_id, "performance", &config, new_version, updated_by).await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;
        updated_axes.push("performance".to_string());
    }

    if let Some(cost) = &request.policy.cost {
        let mut config = serde_json::Map::new();
        if let Some(v) = cost.max_tokens_per_request {
            config.insert("max_tokens_per_request".to_string(), serde_json::Value::from(v));
        }
        if let Some(v) = cost.daily_budget_cents {
            config.insert("daily_budget_cents".to_string(), serde_json::Value::from(v));
        }
        if let Some(v) = cost.retry_max {
            config.insert("retry_max".to_string(), serde_json::Value::from(v));
        }
        if let Some(v) = cost.retry_window_seconds {
            config.insert("retry_window_seconds".to_string(), serde_json::Value::from(v));
        }
        config.insert("action".to_string(), serde_json::Value::from(cost.action.as_deref().unwrap_or("block")));

        upsert_policy_axis(&mut tx, app_id, "cost", &serde_json::Value::Object(config), new_version, updated_by).await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;
        updated_axes.push("cost".to_string());
    }

    if let Some(resp) = &request.policy.responsibility {
        let mut config = serde_json::Map::new();
        if let Some(v) = resp.bias_threshold {
            config.insert("bias_threshold".to_string(), serde_json::Value::from(v));
        }
        if let Some(v) = &resp.pii_action {
            config.insert("pii_action".to_string(), serde_json::Value::from(v.as_str()));
        }
        if let Some(v) = &resp.unsafe_action {
            config.insert("unsafe_action".to_string(), serde_json::Value::from(v.as_str()));
        }
        if let Some(v) = &resp.unsafe_keywords {
            config.insert("unsafe_keywords".to_string(), serde_json::json!(v));
        }

        upsert_policy_axis(&mut tx, app_id, "responsibility", &serde_json::Value::Object(config), new_version, updated_by).await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;
        updated_axes.push("responsibility".to_string());
    }

    tx.commit().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("TX commit error: {e}")))?;

    // Publish policy updated event
    let event_payload = serde_json::json!({
        "app_id": app_id,
        "new_version": new_version,
        "updated_axes": updated_axes,
    });
    let envelope = EventEnvelope::new(
        subjects::POLICY_UPDATED,
        Uuid::now_v7(),
        app_id,
        event_payload,
    );
    if let Ok(bytes) = envelope.to_bytes() {
        let _ = state.publisher.publish(subjects::POLICY_UPDATED, &bytes).await;
    }

    // Signal policy cache reload
    let _ = state.publisher.publish("controlplane.policy.reload", b"policy_updated").await;

    info!(
        app_id = %app_id,
        new_version,
        axes = ?updated_axes,
        "Policy updated"
    );

    Ok(Json(UpdatePolicyResponse {
        app_id,
        updated_axes,
        new_version,
    }))
}

async fn upsert_policy_axis(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    app_id: Uuid,
    axis: &str,
    config: &serde_json::Value,
    new_version: i32,
    updated_by: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    // Deactivate the current active policy for this axis
    sqlx::query(
        "UPDATE policies SET is_active = FALSE WHERE app_id = $1 AND axis = $2 AND is_active = TRUE"
    )
    .bind(app_id)
    .bind(axis)
    .execute(&mut **tx)
    .await?;

    // Insert new version
    sqlx::query(
        "INSERT INTO policies (app_id, axis, threshold_config, version, is_active, updated_by) \
         VALUES ($1, $2, $3, $4, TRUE, $5)"
    )
    .bind(app_id)
    .bind(axis)
    .bind(config)
    .bind(new_version)
    .bind(updated_by)
    .execute(&mut **tx)
    .await?;

    Ok(())
}
