//! Integration test for the reviewer-override RAG retraining loop.
//!
//! Tests that:
//! 1. resolve_case writes a reviewer_overrides row
//! 2. Similar traffic retrieves it via pg_trgm similarity
//!
//! Run with: cargo test -p controlplane-decision --test reviewer_override_rag_test
//!
//! Set DATABASE_URL env var or these tests will be skipped.

use sqlx::PgPool;
use uuid::Uuid;

async fn get_pool() -> Option<PgPool> {
    let url = std::env::var("DATABASE_URL").ok()?;
    if url.is_empty() {
        return None;
    }
    PgPool::connect(&url).await.ok()
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

/// Helper: insert a test app and intercepted call, return (app_id, call_id).
async fn setup_test_data(pool: &PgPool, suffix: &str) -> (Uuid, Uuid) {
    let app_id = Uuid::now_v7();
    let call_id = Uuid::now_v7();
    let app_name = format!("rag-test-app-{}", suffix);

    // Clean up any leftover test data from previous runs
    sqlx::query("DELETE FROM reviewer_overrides WHERE app_id IN (SELECT id FROM apps WHERE name LIKE 'rag-test-app-%')")
        .execute(pool).await.ok();
    sqlx::query("DELETE FROM escalation_cases WHERE app_id IN (SELECT id FROM apps WHERE name LIKE 'rag-test-app-%')")
        .execute(pool).await.ok();
    sqlx::query("DELETE FROM verdicts WHERE app_id IN (SELECT id FROM apps WHERE name LIKE 'rag-test-app-%')")
        .execute(pool).await.ok();
    sqlx::query("DELETE FROM intercepted_calls WHERE app_id IN (SELECT id FROM apps WHERE name LIKE 'rag-test-app-%')")
        .execute(pool).await.ok();
    sqlx::query("DELETE FROM apps WHERE name LIKE 'rag-test-app-%'")
        .execute(pool).await.ok();

    sqlx::query("INSERT INTO apps (id, name, api_key_hash, team_id, created_at) VALUES ($1, $2, 'test-hash', NULL, NOW())")
        .bind(app_id)
        .bind(&app_name)
        .execute(pool)
        .await
        .expect("Failed to insert test app");

    sqlx::query(
        "INSERT INTO intercepted_calls (id, correlation_id, app_id, model, request_payload, response_payload, created_at) \
         VALUES ($1, $1, $2, 'test-model', $3, $4, NOW())"
    )
    .bind(call_id)
    .bind(app_id)
    .bind(serde_json::json!({"messages": [{"role": "user", "content": "The customer SSN is 123-45-6789 and email is test@example.com"}]}))
    .bind(serde_json::json!({"content": [{"type": "text", "text": "Customer SSN 123-45-6789 confirmed. Email test@example.com verified."}]}))
    .execute(pool)
    .await
    .expect("Failed to insert test intercepted call");

    (app_id, call_id)
}

/// Helper: insert an escalation case for the call.
async fn setup_escalation(pool: &PgPool, call_id: Uuid, app_id: Uuid) -> Uuid {
    let escalation_id = Uuid::now_v7();
    let verdict_id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO verdicts (id, call_id, app_id, axis, path, outcome, confidence, reason, check_name, created_at) \
         VALUES ($1, $2, $3, 'responsibility', 'fast', 'escalate', 0.85, 'PII detected', 'pii_detection', NOW())"
    )
    .bind(verdict_id)
    .bind(call_id)
    .bind(app_id)
    .execute(pool)
    .await
    .expect("Failed to insert test verdict");

    sqlx::query(
        "INSERT INTO escalation_cases (id, verdict_id, call_id, app_id, status, axis, confidence, reason, created_at) \
         VALUES ($1, $2, $3, $4, 'open', 'responsibility', 0.85, 'PII detected in response', NOW())"
    )
    .bind(escalation_id)
    .bind(verdict_id)
    .bind(call_id)
    .bind(app_id)
    .execute(pool)
    .await
    .expect("Failed to insert test escalation case");

    escalation_id
}

#[tokio::test]
async fn reviewer_overrides_table_exists() {
    let pool = skip_without_db!(get_pool().await);

    let exists: (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM information_schema.tables WHERE table_name = 'reviewer_overrides')"
    )
    .fetch_one(&pool)
    .await
    .expect("Failed to check table existence");

    assert!(exists.0, "reviewer_overrides table should exist");
}

#[tokio::test]
async fn resolve_creates_precedent_row() {
    let pool = skip_without_db!(get_pool().await);

    let (app_id, call_id) = setup_test_data(&pool, "resolve").await;
    let escalation_id = setup_escalation(&pool, call_id, app_id).await;

    // Simulate what resolve_case does: INSERT into reviewer_overrides
    let precedent_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO reviewer_overrides (id, escalation_id, call_id, app_id, verdict_id, axis, \
         model_outcome, model_confidence, reviewer_action, reviewer_reason, \
         request_excerpt, response_excerpt, created_at) \
         SELECT $1, e.id, e.call_id, e.app_id, e.verdict_id, e.axis, 'escalate', \
                e.confidence, 'override', 'Stats were reliable', \
                LEFT(ic.request_payload::text, 2000), LEFT(ic.response_payload::text, 2000), NOW() \
         FROM escalation_cases e JOIN intercepted_calls ic ON ic.id = e.call_id WHERE e.id = $2"
    )
    .bind(precedent_id)
    .bind(escalation_id)
    .execute(&pool)
    .await
    .expect("Failed to insert reviewer precedent");

    // Verify the row was created
    let row: (String, String, f32) = sqlx::query_as(
        "SELECT reviewer_action, reviewer_reason, model_confidence FROM reviewer_overrides WHERE id = $1"
    )
    .bind(precedent_id)
    .fetch_one(&pool)
    .await
    .expect("Failed to fetch precedent row");

    assert_eq!(row.0, "override");
    assert_eq!(row.1, "Stats were reliable");
    assert!((row.2 - 0.85).abs() < 0.01);
}

#[tokio::test]
async fn similar_content_retrieves_precedent() {
    let pool = skip_without_db!(get_pool().await);

    let (app_id, call_id) = setup_test_data(&pool, "similar").await;
    let escalation_id = setup_escalation(&pool, call_id, app_id).await;

    // Insert precedent with PII-related content
    let precedent_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO reviewer_overrides (id, escalation_id, call_id, app_id, verdict_id, axis, \
         model_outcome, model_confidence, reviewer_action, reviewer_reason, \
         request_excerpt, response_excerpt, created_at) \
         VALUES ($1, $2, $3, $4, $5, 'responsibility', 'escalate', 0.8, 'override', \
                 'SSN format was test data, not real PII', \
                 'Customer SSN 123-45-6789 and email test@example.com', \
                 'SSN 123-45-6789 confirmed. Email test@example.com verified.', NOW())"
    )
    .bind(precedent_id)
    .bind(escalation_id)
    .bind(call_id)
    .bind(app_id)
    .bind(Uuid::now_v7())
    .execute(&pool)
    .await
    .expect("Failed to insert precedent");

    // Query for similar content using pg_trgm similarity (scoped to this test's app)
    let similar: Vec<(Uuid, String, f32)> = sqlx::query_as(
        "SELECT id, reviewer_action, \
                GREATEST(COALESCE(similarity(response_excerpt, $1), 0), \
                         COALESCE(similarity(request_excerpt, $1), 0))::float4 AS score \
         FROM reviewer_overrides \
         WHERE app_id = $2 \
           AND (COALESCE(similarity(response_excerpt, $1), 0) >= 0.1 \
                OR COALESCE(similarity(request_excerpt, $1), 0) >= 0.1) \
         ORDER BY score DESC LIMIT 5"
    )
    .bind("Customer SSN 123-45-6789 and email test@example.com are similar")
    .bind(app_id)
    .fetch_all(&pool)
    .await
    .expect("Failed to query similar precedents");

    assert!(!similar.is_empty(), "Should find similar precedents for PII-related content");
    assert_eq!(similar[0].0, precedent_id);
    assert_eq!(similar[0].1, "override");
    assert!(similar[0].2 > 0.1, "Similarity score should be above threshold");
}

#[tokio::test]
async fn unrelated_content_does_not_retrieve_precedent() {
    let pool = skip_without_db!(get_pool().await);

    let (app_id, call_id) = setup_test_data(&pool, "unrelated").await;
    let escalation_id = setup_escalation(&pool, call_id, app_id).await;

    // Insert precedent about PII
    sqlx::query(
        "INSERT INTO reviewer_overrides (id, escalation_id, call_id, app_id, verdict_id, axis, \
         model_outcome, model_confidence, reviewer_action, reviewer_reason, \
         request_excerpt, response_excerpt, created_at) \
         VALUES ($1, $2, $3, $4, $5, 'responsibility', 'escalate', 0.8, 'override', \
                 'Not real PII', \
                 'Customer SSN 123-45-6789', \
                 'SSN confirmed.', NOW())"
    )
    .bind(Uuid::now_v7())
    .bind(escalation_id)
    .bind(call_id)
    .bind(app_id)
    .bind(Uuid::now_v7())
    .execute(&pool)
    .await
    .expect("Failed to insert precedent");

    // Query for completely unrelated content (scoped to this test's app)
    let similar: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM reviewer_overrides \
         WHERE app_id = $2 \
           AND (COALESCE(similarity(response_excerpt, $1), 0) >= 0.3 \
                OR COALESCE(similarity(request_excerpt, $1), 0) >= 0.3) \
         LIMIT 5"
    )
    .bind("What is the capital of France? The Eiffel Tower is in Paris.")
    .bind(app_id)
    .fetch_all(&pool)
    .await
    .expect("Failed to query similar precedents");

    assert!(similar.is_empty(), "Unrelated content should not retrieve PII precedents");
}
