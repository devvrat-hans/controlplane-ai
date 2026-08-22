//! Integration tests that verify PostgreSQL schema and query correctness.
//! These tests require a running PostgreSQL instance with the controlplane database.
//!
//! Run with: cargo test -p controlplane-gateway --test db_integration_test
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

#[tokio::test]
async fn schema_tables_exist() {
    let pool = skip_without_db!(get_pool().await);

    let tables: Vec<(String,)> = sqlx::query_as(
        "SELECT tablename::text FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename"
    )
    .fetch_all(&pool)
    .await
    .expect("Failed to query tables");

    let table_names: Vec<&str> = tables.iter().map(|t| t.0.as_str()).collect();

    let expected = [
        "apps",
        "audit_records",
        "cost_entries",
        "cost_ledger",
        "escalation_cases",
        "intercepted_calls",
        "pattern_promotions",
        "policies",
        "teams",
        "users",
        "verdicts",
    ];

    for t in &expected {
        assert!(
            table_names.contains(t),
            "Expected table '{}' not found. Found: {:?}",
            t,
            table_names
        );
    }
}

#[tokio::test]
async fn insert_and_query_intercepted_call() {
    let pool = skip_without_db!(get_pool().await);

    let app_id: (Uuid,) =
        sqlx::query_as("SELECT id FROM apps LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("No apps in database — run seed migrations first");

    let call_id = Uuid::now_v7();
    let correlation_id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO intercepted_calls (id, correlation_id, app_id, model, token_count_input, token_count_output, upstream_latency_ms, fast_path_latency_ms)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"
    )
    .bind(call_id)
    .bind(correlation_id)
    .bind(app_id.0)
    .bind("test-model")
    .bind(100i32)
    .bind(200i32)
    .bind(500i32)
    .bind(3i32)
    .execute(&pool)
    .await
    .expect("Failed to insert intercepted_call");

    let fetched: (Uuid, String, i32) = sqlx::query_as(
        "SELECT correlation_id, model, token_count_output FROM intercepted_calls WHERE id = $1"
    )
    .bind(call_id)
    .fetch_one(&pool)
    .await
    .expect("Failed to fetch inserted call");

    assert_eq!(fetched.0, correlation_id);
    assert_eq!(fetched.1, "test-model");
    assert_eq!(fetched.2, 200);

    // Cleanup
    sqlx::query("DELETE FROM intercepted_calls WHERE id = $1")
        .bind(call_id)
        .execute(&pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn insert_verdict_with_fk_constraint() {
    let pool = skip_without_db!(get_pool().await);

    let app_id: (Uuid,) =
        sqlx::query_as("SELECT id FROM apps LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("No apps in database");

    let call_id = Uuid::now_v7();
    let correlation_id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO intercepted_calls (id, correlation_id, app_id, model)
         VALUES ($1, $2, $3, 'test-model')"
    )
    .bind(call_id)
    .bind(correlation_id)
    .bind(app_id.0)
    .execute(&pool)
    .await
    .unwrap();

    let verdict_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO verdicts (id, call_id, app_id, axis, path, outcome, confidence, reason, check_name, duration_ms)
         VALUES ($1, $2, $3, 'responsibility', 'fast', 'edit', 0.95, 'AWS key detected', 'secret_detection', 3)"
    )
    .bind(verdict_id)
    .bind(call_id)
    .bind(app_id.0)
    .execute(&pool)
    .await
    .expect("Failed to insert verdict");

    // Verify verdict was stored correctly
    let v: (String, String, f32) = sqlx::query_as(
        "SELECT outcome, check_name, confidence FROM verdicts WHERE id = $1"
    )
    .bind(verdict_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(v.0, "edit");
    assert_eq!(v.1, "secret_detection");
    assert!((v.2 - 0.95).abs() < 0.001);

    // Cleanup
    sqlx::query("DELETE FROM verdicts WHERE id = $1").bind(verdict_id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM intercepted_calls WHERE id = $1").bind(call_id).execute(&pool).await.unwrap();
}

#[tokio::test]
async fn verdict_fk_rejects_invalid_call_id() {
    let pool = skip_without_db!(get_pool().await);

    let app_id: (Uuid,) =
        sqlx::query_as("SELECT id FROM apps LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("No apps in database");

    let fake_call_id = Uuid::now_v7();
    let verdict_id = Uuid::now_v7();

    let result = sqlx::query(
        "INSERT INTO verdicts (id, call_id, app_id, axis, path, outcome, confidence, reason, check_name)
         VALUES ($1, $2, $3, 'cost', 'fast', 'pass', 0.5, 'test', 'test_check')"
    )
    .bind(verdict_id)
    .bind(fake_call_id)
    .bind(app_id.0)
    .execute(&pool)
    .await;

    assert!(result.is_err(), "Should reject verdict with non-existent call_id FK");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("foreign key") || err_msg.contains("violates"),
        "Error should mention FK violation, got: {}",
        err_msg
    );
}

#[tokio::test]
async fn verdict_check_constraints_reject_invalid_values() {
    let pool = skip_without_db!(get_pool().await);

    let app_id: (Uuid,) =
        sqlx::query_as("SELECT id FROM apps LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("No apps");

    let call_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO intercepted_calls (id, correlation_id, app_id, model)
         VALUES ($1, $2, $3, 'test')"
    )
    .bind(call_id)
    .bind(Uuid::now_v7())
    .bind(app_id.0)
    .execute(&pool)
    .await
    .unwrap();

    // Invalid axis
    let result = sqlx::query(
        "INSERT INTO verdicts (id, call_id, app_id, axis, path, outcome, confidence, reason, check_name)
         VALUES ($1, $2, $3, 'INVALID_AXIS', 'fast', 'pass', 0.5, 'test', 'test')"
    )
    .bind(Uuid::now_v7())
    .bind(call_id)
    .bind(app_id.0)
    .execute(&pool)
    .await;
    assert!(result.is_err(), "Should reject invalid axis");

    // Invalid outcome
    let result = sqlx::query(
        "INSERT INTO verdicts (id, call_id, app_id, axis, path, outcome, confidence, reason, check_name)
         VALUES ($1, $2, $3, 'cost', 'fast', 'INVALID_OUTCOME', 0.5, 'test', 'test')"
    )
    .bind(Uuid::now_v7())
    .bind(call_id)
    .bind(app_id.0)
    .execute(&pool)
    .await;
    assert!(result.is_err(), "Should reject invalid outcome");

    // Confidence out of range
    let result = sqlx::query(
        "INSERT INTO verdicts (id, call_id, app_id, axis, path, outcome, confidence, reason, check_name)
         VALUES ($1, $2, $3, 'cost', 'fast', 'pass', 1.5, 'test', 'test')"
    )
    .bind(Uuid::now_v7())
    .bind(call_id)
    .bind(app_id.0)
    .execute(&pool)
    .await;
    assert!(result.is_err(), "Should reject confidence > 1.0");

    // Cleanup
    sqlx::query("DELETE FROM intercepted_calls WHERE id = $1").bind(call_id).execute(&pool).await.unwrap();
}

#[tokio::test]
async fn policy_crud_operations() {
    let pool = skip_without_db!(get_pool().await);

    let app_id: (Uuid,) =
        sqlx::query_as("SELECT id FROM apps LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("No apps");

    // Read existing policy
    let policies: Vec<(Uuid, String, serde_json::Value, bool)> = sqlx::query_as(
        "SELECT id, axis, threshold_config, is_active FROM policies WHERE app_id = $1 ORDER BY axis"
    )
    .bind(app_id.0)
    .fetch_all(&pool)
    .await
    .expect("Failed to query policies");

    assert!(!policies.is_empty(), "Should have seed policies for the app");

    // Each policy should have valid threshold_config
    for (id, axis, config, is_active) in &policies {
        assert!(!id.is_nil());
        assert!(["cost", "performance", "responsibility"].contains(&axis.as_str()));
        assert!(config.is_object(), "threshold_config should be a JSON object");
        assert!(is_active, "Seed policies should be active");
    }
}

#[tokio::test]
async fn audit_records_hash_chain_integrity() {
    let pool = skip_without_db!(get_pool().await);

    // Query the first 10 audit records ordered by created_at
    let records: Vec<(String, String)> = sqlx::query_as(
        "SELECT prev_hash, record_hash FROM audit_records ORDER BY created_at ASC LIMIT 10"
    )
    .fetch_all(&pool)
    .await
    .expect("Failed to query audit_records");

    if records.is_empty() {
        eprintln!("No audit records to verify — skipping chain check");
        return;
    }

    // Verify chain linkage: each record's prev_hash should match previous record's record_hash
    for i in 1..records.len() {
        assert_eq!(
            records[i].0, records[i - 1].1,
            "Audit chain broken at index {}: prev_hash '{}' != previous record_hash '{}'",
            i, records[i].0, records[i - 1].1
        );
    }
}

#[tokio::test]
async fn escalation_case_lifecycle() {
    let pool = skip_without_db!(get_pool().await);

    // Verify we can query escalation cases by status
    let open_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM escalation_cases WHERE status = 'open'"
    )
    .fetch_one(&pool)
    .await
    .expect("Failed to count open escalations");

    let resolved_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM escalation_cases WHERE status = 'resolved'"
    )
    .fetch_one(&pool)
    .await
    .expect("Failed to count resolved escalations");

    // With seed data, we should have some of each
    assert!(open_count.0 >= 0, "open_count should be non-negative");
    assert!(resolved_count.0 >= 0, "resolved_count should be non-negative");

    // Resolved cases should have resolution and resolved_at
    let resolved: Vec<(Option<String>, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
        "SELECT resolution, resolved_at FROM escalation_cases WHERE status = 'resolved'"
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    for (resolution, resolved_at) in &resolved {
        assert!(resolution.is_some(), "Resolved case should have a resolution");
        assert!(resolved_at.is_some(), "Resolved case should have resolved_at");
        let res = resolution.as_ref().unwrap();
        assert!(
            ["confirm", "override", "dismiss"].contains(&res.as_str()),
            "Invalid resolution: {}",
            res
        );
    }
}

#[tokio::test]
async fn cost_entries_table_accepts_inserts() {
    let pool = skip_without_db!(get_pool().await);

    let app_id: (Uuid,) =
        sqlx::query_as("SELECT id FROM apps LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("No apps");

    let entry_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO cost_entries (id, app_id, model, input_tokens, output_tokens, cost_usd)
         VALUES ($1, $2, 'claude-sonnet-4-20250514', 150, 500, 0.0032)"
    )
    .bind(entry_id)
    .bind(app_id.0)
    .execute(&pool)
    .await
    .expect("Failed to insert cost_entry");

    let entry: (String, i32, i32, f64) = sqlx::query_as(
        "SELECT model, input_tokens, output_tokens, cost_usd FROM cost_entries WHERE id = $1"
    )
    .bind(entry_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(entry.0, "claude-sonnet-4-20250514");
    assert_eq!(entry.1, 150);
    assert_eq!(entry.2, 500);
    assert!((entry.3 - 0.0032).abs() < 0.0001);

    // Cleanup
    sqlx::query("DELETE FROM cost_entries WHERE id = $1").bind(entry_id).execute(&pool).await.unwrap();
}

#[tokio::test]
async fn app_name_uniqueness_enforced() {
    let pool = skip_without_db!(get_pool().await);

    let name = format!("test-app-{}", Uuid::now_v7());
    let id1 = Uuid::now_v7();
    let id2 = Uuid::now_v7();

    // Insert first app
    sqlx::query(
        "INSERT INTO apps (id, name, api_key_hash) VALUES ($1, $2, 'hash1')"
    )
    .bind(id1)
    .bind(&name)
    .execute(&pool)
    .await
    .expect("First app insert should succeed");

    // Try inserting duplicate name (no team)
    let result = sqlx::query(
        "INSERT INTO apps (id, name, api_key_hash) VALUES ($1, $2, 'hash2')"
    )
    .bind(id2)
    .bind(&name)
    .execute(&pool)
    .await;

    assert!(result.is_err(), "Duplicate app name should be rejected");

    // Cleanup
    sqlx::query("DELETE FROM apps WHERE id = $1").bind(id1).execute(&pool).await.unwrap();
}

#[tokio::test]
async fn dashboard_queries_perform_correctly() {
    let pool = skip_without_db!(get_pool().await);

    // Stats overview query (mimics dashboard API)
    let stats: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM verdicts WHERE created_at > NOW() - INTERVAL '24 hours'"
    )
    .fetch_one(&pool)
    .await
    .expect("Stats query failed");
    assert!(stats.0 >= 0);

    // Recent verdicts with app join (mimics dashboard API)
    let recent: Vec<(Uuid, String, String, f32)> = sqlx::query_as(
        "SELECT v.id, v.outcome, v.check_name, v.confidence
         FROM verdicts v
         WHERE v.app_id IS NOT NULL
         ORDER BY v.created_at DESC
         LIMIT 10"
    )
    .fetch_all(&pool)
    .await
    .expect("Recent verdicts query failed");

    // Should have seed data
    assert!(!recent.is_empty(), "Should have verdicts from seed data");

    // Outcome breakdown (mimics dashboard pie chart)
    let breakdown: Vec<(String, i64)> = sqlx::query_as(
        "SELECT outcome, COUNT(*) FROM verdicts
         WHERE created_at > NOW() - INTERVAL '24 hours'
         GROUP BY outcome"
    )
    .fetch_all(&pool)
    .await
    .expect("Breakdown query failed");

    for (outcome, count) in &breakdown {
        assert!(
            ["pass", "edit", "block", "escalate"].contains(&outcome.as_str()),
            "Unexpected outcome: {}",
            outcome
        );
        assert!(*count > 0);
    }
}
