-- Multi-turn conversation context tracking (Round 2, Task R2.1)
-- Adds session_id to link related turns in the same conversation.

ALTER TABLE intercepted_calls ADD COLUMN IF NOT EXISTS session_id UUID;

CREATE INDEX IF NOT EXISTS idx_calls_session ON intercepted_calls(session_id, created_at ASC)
  WHERE session_id IS NOT NULL;
