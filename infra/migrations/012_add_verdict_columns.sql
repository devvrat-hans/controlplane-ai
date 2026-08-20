-- Add app_id and latency_ms columns to verdicts for dashboard queries.
-- app_id is denormalized from intercepted_calls for efficient filtering.
ALTER TABLE verdicts ADD COLUMN IF NOT EXISTS app_id UUID REFERENCES apps(id);
ALTER TABLE verdicts ADD COLUMN IF NOT EXISTS latency_ms INT;

CREATE INDEX IF NOT EXISTS idx_verdicts_app ON verdicts(app_id, created_at DESC);

-- Backfill app_id from intercepted_calls for existing rows
UPDATE verdicts v
SET app_id = ic.app_id
FROM intercepted_calls ic
WHERE v.call_id = ic.id AND v.app_id IS NULL;
