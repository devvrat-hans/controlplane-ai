use chrono::{DateTime, Duration, DurationRound, Utc};
use sqlx::PgPool;
use tracing::{error, info};
use uuid::Uuid;

use crate::chain::{compute_record_hash, GENESIS_HASH};

/// Advisory-lock key serializing all audit chain appends ("AUDITCHN").
const AUDIT_CHAIN_LOCK_KEY: i64 = 0x4155_4449_5443_484E;

/// Timestamp for the next chain record.
///
/// Truncated to microseconds because that is what Postgres `timestamptz`
/// stores; hashing a nanosecond value would make the stored hash impossible to
/// recompute from the row. Forced strictly after the predecessor so chain order
/// and `created_at` order always agree, even when writers' clocks disagree.
fn next_chain_timestamp(now: DateTime<Utc>, prev: Option<DateTime<Utc>>) -> DateTime<Utc> {
    let now = now.duration_trunc(Duration::microseconds(1)).unwrap_or(now);
    match prev {
        Some(prev) if now <= prev => prev + Duration::microseconds(1),
        _ => now,
    }
}

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
    ///
    /// Read-latest + insert runs in one transaction under a Postgres advisory
    /// lock, so concurrent writers (even separate gateway processes sharing the
    /// database) cannot both chain onto the same predecessor and fork the chain.
    pub async fn append(
        &self,
        call_id: Uuid,
        verdict_id: Uuid,
        app_id: Uuid,
        action_taken: &str,
        metadata: Option<serde_json::Value>,
    ) -> Result<AuditRow, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(AUDIT_CHAIN_LOCK_KEY)
            .execute(&mut *tx)
            .await?;

        let latest: Option<(String, DateTime<Utc>)> = sqlx::query_as(
            "SELECT record_hash, created_at FROM audit_records ORDER BY created_at DESC LIMIT 1"
        )
        .fetch_optional(&mut *tx)
        .await?;

        let (prev_hash, prev_ts) = match latest {
            Some((hash, ts)) => (hash, Some(ts)),
            None => (GENESIS_HASH.to_string(), None),
        };
        let now = next_chain_timestamp(Utc::now(), prev_ts);
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
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Timelike};

    #[test]
    fn chain_timestamp_truncates_to_postgres_precision() {
        let now = Utc.with_ymd_and_hms(2026, 9, 24, 20, 52, 48).unwrap()
            + Duration::nanoseconds(817_906_123);
        let ts = next_chain_timestamp(now, None);
        assert_eq!(ts.nanosecond(), 817_906_000);
    }

    #[test]
    fn chain_timestamp_never_precedes_predecessor() {
        // A writer whose clock lags the predecessor's (the cause of the historical fork).
        let prev = Utc.with_ymd_and_hms(2026, 9, 24, 20, 52, 48).unwrap() + Duration::microseconds(817_909);
        let lagging = prev - Duration::microseconds(3);
        assert_eq!(next_chain_timestamp(lagging, Some(prev)), prev + Duration::microseconds(1));
        assert_eq!(next_chain_timestamp(prev, Some(prev)), prev + Duration::microseconds(1));
        let later = prev + Duration::milliseconds(5);
        assert_eq!(next_chain_timestamp(later, Some(prev)), later);
    }
}
