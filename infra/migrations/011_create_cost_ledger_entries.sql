-- Per-request cost tracking entries (complements the windowed cost_ledger table).
-- This gives full granularity for anomaly detection and detailed analytics.
ALTER TABLE cost_ledger ADD COLUMN IF NOT EXISTS model TEXT;
ALTER TABLE cost_ledger ADD COLUMN IF NOT EXISTS input_tokens INT DEFAULT 0;
ALTER TABLE cost_ledger ADD COLUMN IF NOT EXISTS output_tokens INT DEFAULT 0;
ALTER TABLE cost_ledger ADD COLUMN IF NOT EXISTS cost_usd DOUBLE PRECISION DEFAULT 0.0;

-- Drop the UNIQUE constraint to allow per-request inserts
ALTER TABLE cost_ledger DROP CONSTRAINT IF EXISTS cost_ledger_app_id_window_start_key;
