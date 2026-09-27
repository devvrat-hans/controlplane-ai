-- Per-check shadow-path run records for each call: every shadow check with its
-- status (ran / skipped / error), wall time in microseconds, and a skip/error
-- reason. Verdicts only exist for findings, so without this the dashboard cannot
-- tell a shadow check that passed from one that never ran.
-- Shape: [{"check_name": "groundedness", "status": "ran", "duration_us": 412}, ...]
ALTER TABLE intercepted_calls ADD COLUMN IF NOT EXISTS shadow_check_runs JSONB;
