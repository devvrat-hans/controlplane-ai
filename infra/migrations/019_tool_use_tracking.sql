-- Agent/Tool-Use Risk Tracking (Round 2, Task R2.8)
-- Tracks whether a model response contains tool/function calls.

ALTER TABLE intercepted_calls ADD COLUMN IF NOT EXISTS has_tool_use BOOLEAN DEFAULT FALSE;

CREATE INDEX IF NOT EXISTS idx_calls_tool_use ON intercepted_calls(has_tool_use, created_at DESC)
  WHERE has_tool_use = TRUE;
