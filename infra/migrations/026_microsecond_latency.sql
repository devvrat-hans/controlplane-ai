-- Exact (microsecond) timings alongside the existing whole-millisecond columns.
-- Fast checks routinely finish in well under 1 ms, which the integer *_ms columns
-- store as 0; these columns keep the real value. Existing *_ms columns are left
-- untouched so older readers keep working. Historical rows stay NULL here.
ALTER TABLE verdicts ADD COLUMN IF NOT EXISTS duration_us BIGINT;
ALTER TABLE intercepted_calls ADD COLUMN IF NOT EXISTS upstream_latency_us BIGINT;
ALTER TABLE intercepted_calls ADD COLUMN IF NOT EXISTS fast_path_latency_us BIGINT;
-- {check_name: microseconds} for every fast-path check that ran, including passes.
ALTER TABLE intercepted_calls ADD COLUMN IF NOT EXISTS fast_path_check_timings_us JSONB;
