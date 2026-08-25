-- Data Source Governance Integration (Round 2, Task R2.9)
-- Allows apps to declare their data governance level, which affects
-- how strictly checks are applied (lower governance = stricter thresholds).

ALTER TABLE apps ADD COLUMN IF NOT EXISTS data_governance_level TEXT NOT NULL DEFAULT 'medium'
  CHECK (data_governance_level IN ('high', 'medium', 'low'));

-- Update seed apps with appropriate governance levels
UPDATE apps SET data_governance_level = 'high'
  WHERE name ILIKE '%customer%' OR name ILIKE '%chatbot%' OR name ILIKE '%prod%';

UPDATE apps SET data_governance_level = 'medium'
  WHERE name ILIKE '%agent%' OR name ILIKE '%internal%';

UPDATE apps SET data_governance_level = 'low'
  WHERE name ILIKE '%rag%' OR name ILIKE '%support%';
