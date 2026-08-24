use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tracing::{error, info};
use uuid::Uuid;

use crate::chain::{compute_record_hash, GENESIS_HASH};

/// Append-only audit record repository.
/// Rule: no UPDATE or DELETE on audit_records table, ever.
pub struct AuditRepository {
    pool: PgPool,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AuditRow {
    pub id: Uuid,
    pub call_id: Uuid,
    pub verdict_id: Uuid,
    pub action_taken: String,
    pub prev_hash: String,
    pub record_hash: String,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

impl AuditRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Append a new audit record to the chain.
    /// Automatically fetches the previous hash and computes the new hash.
    pub async fn append(
        &self,
        call_id: Uuid,
        verdict_id: Uuid,
        app_id: Uuid,
        action_taken: &str,
        metadata: Option<serde_json::Value>,
    ) -> Result<AuditRow, sqlx::Error> {
        let prev_hash = self.get_latest_hash().await?;
        let now = Utc::now();
        let id = Uuid::now_v7();
        let record_hash = compute_record_hash(&prev_hash, call_id, verdict_id, action_taken, now);

        sqlx::query(
            "INSERT INTO audit_records (id, call_id, verdict_id, app_id, action_taken, prev_hash, record_hash, metadata, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"
        )
        .bind(id)
        .bind(call_id)
        .bind(verdict_id)
        .bind(app_id)
        .bind(action_taken)
        .bind(&prev_hash)
        .bind(&record_hash)
        .bind(&metadata)
        .bind(now)
        .execute(&self.pool)
        .await?;

        info!(
            audit_id = %id,
            call_id = %call_id,
            action = action_taken,
            "Audit record appended"
        );

        Ok(AuditRow {
            id,
            call_id,
            verdict_id,
            action_taken: action_taken.to_string(),
            prev_hash,
            record_hash,
            metadata,
            created_at: now,
        })
    }

    /// Get the hash of the most recent audit record (for chaining).
    async fn get_latest_hash(&self) -> Result<String, sqlx::Error> {
        let result: Option<String> = sqlx::query_scalar(
            "SELECT record_hash FROM audit_records ORDER BY created_at DESC LIMIT 1"
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.unwrap_or_else(|| GENESIS_HASH.to_string()))
    }

    /// Query audit records with filters and cursor-based pagination.
    pub async fn query(
        &self,
        filters: &AuditQueryFilters,
    ) -> Result<Vec<AuditRow>, sqlx::Error> {
        let mut query = String::from(
            "SELECT id, call_id, verdict_id, action_taken, prev_hash, record_hash, metadata, created_at \
             FROM audit_records WHERE 1=1"
        );
        let mut params: Vec<String> = Vec::new();
        let mut bind_idx = 1;

        if filters.app_id.is_some() {
            query.push_str(&format!(
                " AND call_id IN (SELECT id FROM intercepted_calls WHERE app_id = ${bind_idx})"
            ));
            bind_idx += 1;
        }

        if filters.action.is_some() {
            query.push_str(&format!(" AND action_taken = ${bind_idx}"));
            bind_idx += 1;
        }

        if filters.axis.is_some() {
            query.push_str(&format!(" AND verdict_id IN (SELECT id FROM verdicts WHERE axis = ${bind_idx})"));
            bind_idx += 1;
        }

        if filters.from.is_some() {
            query.push_str(&format!(" AND created_at >= ${bind_idx}"));
            bind_idx += 1;
        }

        if filters.to.is_some() {
            query.push_str(&format!(" AND created_at <= ${bind_idx}"));
            bind_idx += 1;
        }

        if let Some(cursor) = &filters.cursor {
            query.push_str(&format!(" AND created_at < ${bind_idx}"));
            bind_idx += 1;
            params.push(cursor.to_rfc3339());
        }

        query.push_str(" ORDER BY created_at DESC");
        query.push_str(&format!(" LIMIT ${bind_idx}"));
        let _ = bind_idx;

        let limit = filters.limit.unwrap_or(50).min(200);

        // Build dynamic query
        let mut db_query = sqlx::query_as::<_, AuditRow>(&query);

        if let Some(app_id) = filters.app_id {
            db_query = db_query.bind(app_id);
        }
        if let Some(action) = &filters.action {
            db_query = db_query.bind(action);
        }
        if let Some(axis) = &filters.axis {
            db_query = db_query.bind(axis);
        }
        if let Some(from) = filters.from {
            db_query = db_query.bind(from);
        }
        if let Some(to) = filters.to {
            db_query = db_query.bind(to);
        }
        if let Some(cursor) = filters.cursor {
            db_query = db_query.bind(cursor);
        }
        db_query = db_query.bind(limit as i64);

        db_query.fetch_all(&self.pool).await
    }

    /// Verify the hash chain integrity between two timestamps.
    pub async fn verify_chain(
        &self,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
    ) -> Result<VerificationResult, sqlx::Error> {
        let mut query = String::from(
            "SELECT id, call_id, verdict_id, action_taken, prev_hash, record_hash, metadata, created_at \
             FROM audit_records"
        );

        let mut conditions = Vec::new();
        if from.is_some() {
            conditions.push("created_at >= $1".to_string());
        }
        if to.is_some() {
            let idx = if from.is_some() { 2 } else { 1 };
            conditions.push(format!("created_at <= ${idx}"));
        }

        if !conditions.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&conditions.join(" AND "));
        }

        query.push_str(" ORDER BY created_at ASC");

        let mut db_query = sqlx::query_as::<_, AuditRow>(&query);
        if let Some(from) = from {
            db_query = db_query.bind(from);
        }
        if let Some(to) = to {
            db_query = db_query.bind(to);
        }

        let records = db_query.fetch_all(&self.pool).await?;

        if records.is_empty() {
            return Ok(VerificationResult {
                valid: true,
                records_checked: 0,
                first_broken_at: None,
            });
        }

        let mut checked = 0u64;

        for record in &records {
            let expected_hash = compute_record_hash(
                &record.prev_hash,
                record.call_id,
                record.verdict_id,
                &record.action_taken,
                record.created_at,
            );

            if expected_hash != record.record_hash {
                error!(
                    audit_id = %record.id,
                    expected = %expected_hash,
                    actual = %record.record_hash,
                    "Audit chain integrity violation detected!"
                );

                return Ok(VerificationResult {
                    valid: false,
                    records_checked: checked,
                    first_broken_at: Some(record.id),
                });
            }

            checked += 1;
        }

        // Verify chain linkage: each record's prev_hash should match prior record's record_hash
        for window in records.windows(2) {
            let prev = &window[0];
            let curr = &window[1];

            if curr.prev_hash != prev.record_hash {
                error!(
                    broken_at = %curr.id,
                    expected_prev = %prev.record_hash,
                    actual_prev = %curr.prev_hash,
                    "Audit chain linkage broken!"
                );

                return Ok(VerificationResult {
                    valid: false,
                    records_checked: checked,
                    first_broken_at: Some(curr.id),
                });
            }
        }

        Ok(VerificationResult {
            valid: true,
            records_checked: checked,
            first_broken_at: None,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct AuditQueryFilters {
    pub app_id: Option<Uuid>,
    pub action: Option<String>,
    pub axis: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub cursor: Option<DateTime<Utc>>,
    pub limit: Option<i32>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VerificationResult {
    pub valid: bool,
    pub records_checked: u64,
    pub first_broken_at: Option<Uuid>,
}
