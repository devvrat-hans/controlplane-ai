-- Lower cost caps to realistic values for Ollama qwen2.5:1.5b (typical output 10-100 tokens)
-- This ensures cost axis verdicts actually fire during load tests

UPDATE policies SET
  threshold_config = '{"max_tokens_per_request": 75, "retry_max": 3, "retry_window_seconds": 30, "daily_budget_action": "escalate"}'::jsonb,
  version = version + 1
WHERE app_id = '10000000-0000-0000-0000-000000000001' AND axis = 'cost' AND is_active = true;

UPDATE policies SET
  threshold_config = '{"max_tokens_per_request": 30, "retry_max": 2, "retry_window_seconds": 30}'::jsonb,
  version = version + 1
WHERE app_id = '10000000-0000-0000-0000-000000000002' AND axis = 'cost' AND is_active = true;

UPDATE policies SET
  threshold_config = '{"max_tokens_per_request": 120, "retry_max": 3, "retry_window_seconds": 30, "daily_budget_cents": 5000, "action": "block"}'::jsonb,
  version = version + 1
WHERE app_id = '10000000-0000-0000-0000-000000000003' AND axis = 'cost' AND is_active = true;
