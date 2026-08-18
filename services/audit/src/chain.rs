use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Compute the hash for an audit record.
/// record_hash = SHA-256(prev_hash + call_id + verdict_id + action + timestamp)
pub fn compute_record_hash(
    prev_hash: &str,
    call_id: Uuid,
    verdict_id: Uuid,
    action: &str,
    timestamp: DateTime<Utc>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prev_hash.as_bytes());
    hasher.update(call_id.as_bytes());
    hasher.update(verdict_id.as_bytes());
    hasher.update(action.as_bytes());
    hasher.update(timestamp.to_rfc3339().as_bytes());
    hex::encode(hasher.finalize())
}

/// The genesis hash for the first record in the chain.
pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Verify that a record's hash is consistent with its contents and predecessor.
pub fn verify_record(
    record_hash: &str,
    prev_hash: &str,
    call_id: Uuid,
    verdict_id: Uuid,
    action: &str,
    timestamp: DateTime<Utc>,
) -> bool {
    let expected = compute_record_hash(prev_hash, call_id, verdict_id, action, timestamp);
    expected == record_hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genesis_record_hash() {
        let call_id = Uuid::parse_str("12345678-1234-1234-1234-123456789012").unwrap();
        let verdict_id = Uuid::parse_str("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee").unwrap();
        let ts = DateTime::parse_from_rfc3339("2026-01-15T10:00:00Z").unwrap().into();

        let hash = compute_record_hash(GENESIS_HASH, call_id, verdict_id, "block", ts);
        assert_eq!(hash.len(), 64); // SHA-256 hex is 64 chars
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn hash_is_deterministic() {
        let call_id = Uuid::nil();
        let verdict_id = Uuid::nil();
        let ts = DateTime::parse_from_rfc3339("2026-08-01T00:00:00Z").unwrap().into();

        let h1 = compute_record_hash(GENESIS_HASH, call_id, verdict_id, "pass", ts);
        let h2 = compute_record_hash(GENESIS_HASH, call_id, verdict_id, "pass", ts);
        assert_eq!(h1, h2);
    }

    #[test]
    fn different_inputs_different_hash() {
        let call_id = Uuid::nil();
        let verdict_id = Uuid::nil();
        let ts = DateTime::parse_from_rfc3339("2026-08-01T00:00:00Z").unwrap().into();

        let h1 = compute_record_hash(GENESIS_HASH, call_id, verdict_id, "pass", ts);
        let h2 = compute_record_hash(GENESIS_HASH, call_id, verdict_id, "block", ts);
        assert_ne!(h1, h2);
    }

    #[test]
    fn verify_valid_record() {
        let call_id = Uuid::nil();
        let verdict_id = Uuid::nil();
        let ts = DateTime::parse_from_rfc3339("2026-08-01T00:00:00Z").unwrap().into();

        let hash = compute_record_hash(GENESIS_HASH, call_id, verdict_id, "edit", ts);
        assert!(verify_record(&hash, GENESIS_HASH, call_id, verdict_id, "edit", ts));
    }

    #[test]
    fn verify_tampered_record() {
        let call_id = Uuid::nil();
        let verdict_id = Uuid::nil();
        let ts = DateTime::parse_from_rfc3339("2026-08-01T00:00:00Z").unwrap().into();

        let hash = compute_record_hash(GENESIS_HASH, call_id, verdict_id, "pass", ts);
        // Tamper: verify with different action
        assert!(!verify_record(&hash, GENESIS_HASH, call_id, verdict_id, "block", ts));
    }

    #[test]
    fn chain_integrity() {
        let call1 = Uuid::new_v4();
        let verdict1 = Uuid::new_v4();
        let ts1 = Utc::now();

        let hash1 = compute_record_hash(GENESIS_HASH, call1, verdict1, "pass", ts1);

        let call2 = Uuid::new_v4();
        let verdict2 = Uuid::new_v4();
        let ts2 = Utc::now();

        let hash2 = compute_record_hash(&hash1, call2, verdict2, "escalate", ts2);

        // Verify chain
        assert!(verify_record(&hash1, GENESIS_HASH, call1, verdict1, "pass", ts1));
        assert!(verify_record(&hash2, &hash1, call2, verdict2, "escalate", ts2));

        // Break chain: use wrong prev_hash for record 2
        assert!(!verify_record(&hash2, GENESIS_HASH, call2, verdict2, "escalate", ts2));
    }
}
