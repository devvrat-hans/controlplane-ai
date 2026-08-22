//! Integration tests for the Dashboard API.
//! Spins up the actual Axum server with a real PostgreSQL pool and makes HTTP requests.
//!
//! Run with: cargo test -p controlplane-dashboard-api --test api_integration_test
//!
//! Requires DATABASE_URL env var pointing to a seeded controlplane database.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt;

use controlplane_dashboard_api::router::{dashboard_router, DashboardState};
use controlplane_dashboard_api::sse::SseBroadcaster;

async fn get_pool() -> Option<PgPool> {
    let url = std::env::var("DATABASE_URL").ok()?;
    if url.is_empty() {
        return None;
    }
    PgPool::connect(&url).await.ok()
}

fn make_app(pool: PgPool) -> axum::Router {
    let state = DashboardState {
        pool,
        broadcaster: SseBroadcaster::new(128),
    };
    dashboard_router(state)
}

macro_rules! skip_without_db {
    ($pool:expr) => {
        match $pool {
            Some(p) => p,
            None => {
                eprintln!("SKIPPED: DATABASE_URL not set or unreachable");
                return;
            }
        }
    };
}

#[tokio::test]
async fn health_endpoint_returns_ok() {
    let pool = skip_without_db!(get_pool().await);
    let app = make_app(pool);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "ok");
    assert_eq!(json["service"], "dashboard-api");
}

#[tokio::test]
async fn ready_endpoint_verifies_db() {
    let pool = skip_without_db!(get_pool().await);
    let app = make_app(pool);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn stats_overview_returns_valid_json() {
    let pool = skip_without_db!(get_pool().await);
    let app = make_app(pool);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/stats/overview")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["total_calls_24h"].is_number(), "total_calls_24h should be numeric");
    assert!(json["total_verdicts_24h"].is_number(), "total_verdicts_24h should be numeric");
    assert!(json["blocks_24h"].is_number(), "blocks_24h should be numeric");
    assert!(json["avg_fast_path_latency_ms"].is_number(), "avg_fast_path_latency_ms should be numeric");
}

#[tokio::test]
async fn recent_verdicts_returns_array() {
    let pool = skip_without_db!(get_pool().await);
    let app = make_app(pool);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/verdicts/recent?limit=5")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["verdicts"].is_array(), "Should return verdicts array");
    let verdicts = json["verdicts"].as_array().unwrap();
    assert!(verdicts.len() <= 5, "Should respect limit parameter");

    if !verdicts.is_empty() {
        let v = &verdicts[0];
        assert!(v["id"].is_string(), "verdict should have id");
        assert!(v["outcome"].is_string(), "verdict should have outcome");
        assert!(v["check_name"].is_string(), "verdict should have check_name");
        assert!(v["confidence"].is_number(), "verdict should have confidence");
    }
}

#[tokio::test]
async fn recent_verdicts_filters_by_outcome() {
    let pool = skip_without_db!(get_pool().await);
    let app = make_app(pool);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/verdicts/recent?outcome=block&limit=50")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let verdicts = json["verdicts"].as_array().unwrap();

    for v in verdicts {
        assert_eq!(
            v["outcome"].as_str().unwrap(),
            "block",
            "All verdicts should be 'block' when filtered"
        );
    }
}

#[tokio::test]
async fn list_apps_returns_seeded_apps() {
    let pool = skip_without_db!(get_pool().await);
    let app = make_app(pool);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/apps")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // list_apps returns a bare array, not wrapped in { "apps": [...] }
    assert!(json.is_array(), "Response should be an array of apps");
    let apps = json.as_array().unwrap();
    assert!(apps.len() >= 3, "Should have at least 3 seeded apps, got {}", apps.len());
}

#[tokio::test]
async fn get_policy_for_app() {
    let pool = skip_without_db!(get_pool().await);
    let app = make_app(pool);

    let app_id = "10000000-0000-0000-0000-000000000001";
    let response = app
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/policies/{}", app_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["policies"].is_array());
    let policies = json["policies"].as_array().unwrap();
    assert!(!policies.is_empty(), "ChatBot-Prod should have policies");

    for p in policies {
        assert!(p["axis"].is_string());
        // The field is called "config" in the API response (not "threshold_config")
        assert!(p["config"].is_object(), "Policy config should be an object, got: {:?}", p);
        assert_eq!(p["is_active"], true);
    }
}

#[tokio::test]
async fn update_policy_saves_and_returns() {
    let pool = skip_without_db!(get_pool().await);
    let app = make_app(pool);

    let app_id = "10000000-0000-0000-0000-000000000001";
    let policy_body = serde_json::json!({
        "policies": [
            {
                "axis": "cost",
                "threshold_config": {
                    "max_tokens_per_request": 5000,
                    "daily_budget_cents": 15000,
                    "action": "block"
                }
            }
        ]
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(&format!("/api/v1/policies/{}", app_id))
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_string(&policy_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "PUT policy should succeed"
    );
}

#[tokio::test]
async fn list_escalations_returns_cases() {
    let pool = skip_without_db!(get_pool().await);
    let app = make_app(pool);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/escalations")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["cases"].is_array());
    let cases = json["cases"].as_array().unwrap();

    if !cases.is_empty() {
        let c = &cases[0];
        assert!(c["id"].is_string());
        assert!(c["status"].is_string());
        assert!(c["reason"].is_string());
        assert!(c["app_id"].is_string());
    }
}

#[tokio::test]
async fn invalid_auth_token_rejected() {
    let pool = skip_without_db!(get_pool().await);
    let app = make_app(pool);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/stats/overview")
                .header("Authorization", "Bearer invalid.token.here")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Invalid JWT should be rejected"
    );
}
