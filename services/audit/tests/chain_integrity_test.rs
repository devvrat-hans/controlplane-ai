//! Integration test: create 100 audit records, verify the chain,
//! tamper with one record, verify that integrity check detects the break.

use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

use controlplane_audit::chain::{compute_record_hash, verify_record, GENESIS_HASH};

struct AuditRecord {
    call_id: Uuid,
    verdict_id: Uuid,
    action: String,
    timestamp: DateTime<Utc>,
    prev_hash: String,
    record_hash: String,
}

/// Build a chain of N audit records.
fn build_chain(n: usize) -> Vec<AuditRecord> {
    let base_time = Utc::now();
    let actions = ["pass", "edit", "block", "escalate"];
    let mut chain = Vec::with_capacity(n);
    let mut prev_hash = GENESIS_HASH.to_string();

    for i in 0..n {
        let call_id = Uuid::new_v4();
        let verdict_id = Uuid::new_v4();
        let action = actions[i % actions.len()].to_string();
        let timestamp = base_time + Duration::seconds(i as i64);

        let record_hash = compute_record_hash(&prev_hash, call_id, verdict_id, &action, timestamp);

        chain.push(AuditRecord {
            call_id,
            verdict_id,
            action,
            timestamp,
            prev_hash: prev_hash.clone(),
            record_hash: record_hash.clone(),
        });

        prev_hash = record_hash;
    }

    chain
}

/// Verify all records in a chain. Returns the index of the first broken record, or None.
fn verify_chain(chain: &[AuditRecord]) -> Option<usize> {
    for (i, record) in chain.iter().enumerate() {
        let valid = verify_record(
            &record.record_hash,
            &record.prev_hash,
            record.call_id,
            record.verdict_id,
            &record.action,
            record.timestamp,
        );
        if !valid {
            return Some(i);
        }

        // Also verify prev_hash linkage (except first record)
        if i > 0 && record.prev_hash != chain[i - 1].record_hash {
            return Some(i);
        }
    }
    None
}

#[test]
fn chain_of_100_records_verifies_successfully() {
    let chain = build_chain(100);
    assert_eq!(chain.len(), 100);
    assert_eq!(verify_chain(&chain), None, "Clean chain should verify");
}

#[test]
fn tampered_action_is_detected() {
    let mut chain = build_chain(100);

    // Tamper with record at index 50: change its action
    chain[50].action = "TAMPERED".to_string();

    let broken_at = verify_chain(&chain);
    assert_eq!(broken_at, Some(50), "Should detect tamper at index 50");
}

#[test]
fn tampered_hash_breaks_subsequent_record() {
    let mut chain = build_chain(100);

    // Tamper with the record hash of record 30 (breaks linkage at 31)
    chain[30].record_hash = "deadbeef".repeat(8);

    let broken_at = verify_chain(&chain);
    // The tampered record itself will still verify if we check it alone,
    // but the next record's prev_hash won't match
    assert!(
        broken_at == Some(30) || broken_at == Some(31),
        "Should detect break at 30 or 31, got {:?}",
        broken_at
    );
}

#[test]
fn tampered_prev_hash_is_detected() {
    let mut chain = build_chain(100);

    // Change prev_hash of record 75 (record itself won't verify)
    chain[75].prev_hash = "0".repeat(64);

    let broken_at = verify_chain(&chain);
    assert_eq!(broken_at, Some(75), "Should detect tamper at index 75");
}

#[test]
fn single_record_chain_verifies() {
    let chain = build_chain(1);
    assert_eq!(verify_chain(&chain), None);
    assert_eq!(chain[0].prev_hash, GENESIS_HASH);
}

#[test]
fn empty_chain_verifies() {
    let chain: Vec<AuditRecord> = Vec::new();
    assert_eq!(verify_chain(&chain), None);
}

#[test]
fn chain_hashes_are_unique() {
    let chain = build_chain(100);
    let mut hashes: Vec<&str> = chain.iter().map(|r| r.record_hash.as_str()).collect();
    hashes.sort();
    hashes.dedup();
    assert_eq!(hashes.len(), 100, "All 100 hashes should be unique");
}
