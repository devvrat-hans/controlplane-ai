-- Regulatory/Geographic Policy Profiles (Round 2, Task R2.2)
-- Named configurations combining geography + industry + risk appetite.

CREATE TABLE IF NOT EXISTS policy_profiles (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    geography TEXT NOT NULL,
    industry TEXT NOT NULL,
    risk_appetite TEXT NOT NULL CHECK (risk_appetite IN ('conservative', 'moderate', 'permissive')),
    default_thresholds JSONB NOT NULL,
    regulations TEXT[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Add profile reference to policies table
ALTER TABLE policies ADD COLUMN IF NOT EXISTS profile TEXT REFERENCES policy_profiles(id);

-- Seed regulatory profiles
INSERT INTO policy_profiles (id, name, description, geography, industry, risk_appetite, default_thresholds, regulations) VALUES
('eu-financial', 'EU Financial Services', 'Strict compliance for EU-regulated financial institutions. Aligned with AI Act high-risk classification and GDPR data protection requirements.', 'EU', 'Financial Services', 'conservative',
 '{"performance": {"groundedness_threshold": 0.8, "hallucination_action": "block"}, "cost": {"max_tokens_per_request": 2000, "daily_budget_action": "block"}, "responsibility": {"bias_threshold": 0.5, "pii_action": "block", "unsafe_action": "block"}}',
 ARRAY['EU AI Act', 'GDPR', 'MiFID II', 'DORA']),

('us-healthcare', 'US Healthcare', 'HIPAA-compliant configuration for healthcare AI applications. Strict PII controls and conservative hallucination thresholds for patient safety.', 'US', 'Healthcare', 'conservative',
 '{"performance": {"groundedness_threshold": 0.85, "hallucination_action": "block"}, "cost": {"max_tokens_per_request": 3000, "daily_budget_action": "escalate"}, "responsibility": {"bias_threshold": 0.4, "pii_action": "block", "unsafe_action": "block"}}',
 ARRAY['HIPAA', 'FDA AI/ML Guidance', '21 CFR Part 11']),

('india-general', 'India General', 'Moderate configuration for general enterprise AI use in India. Aligned with DPDP Act and emerging AI governance framework.', 'India', 'General Enterprise', 'moderate',
 '{"performance": {"groundedness_threshold": 0.6, "hallucination_action": "escalate"}, "cost": {"max_tokens_per_request": 4000, "daily_budget_action": "escalate"}, "responsibility": {"bias_threshold": 0.65, "pii_action": "edit", "unsafe_action": "block"}}',
 ARRAY['DPDP Act 2023', 'IT Act 2000', 'SEBI AI Guidelines']),

('us-financial', 'US Financial Services', 'SEC and FINRA compliant configuration for US financial AI applications. Focus on accuracy and fair lending.', 'US', 'Financial Services', 'conservative',
 '{"performance": {"groundedness_threshold": 0.8, "hallucination_action": "block"}, "cost": {"max_tokens_per_request": 3000, "daily_budget_action": "block"}, "responsibility": {"bias_threshold": 0.45, "pii_action": "block", "unsafe_action": "block"}}',
 ARRAY['SEC AI Guidance', 'FINRA', 'ECOA', 'FCRA']),

('global-internal', 'Global Internal Tools', 'Permissive configuration for internal-only AI tools with lower regulatory exposure. Still maintains safety baselines.', 'Global', 'Internal', 'permissive',
 '{"performance": {"groundedness_threshold": 0.4, "hallucination_action": "escalate"}, "cost": {"max_tokens_per_request": 8000, "daily_budget_action": "escalate"}, "responsibility": {"bias_threshold": 0.75, "pii_action": "edit", "unsafe_action": "escalate"}}',
 ARRAY['Internal Policy']),

('eu-general', 'EU General Enterprise', 'Standard EU configuration aligned with the AI Act for general-purpose AI systems (not high-risk).', 'EU', 'General Enterprise', 'moderate',
 '{"performance": {"groundedness_threshold": 0.6, "hallucination_action": "escalate"}, "cost": {"max_tokens_per_request": 4000, "daily_budget_action": "escalate"}, "responsibility": {"bias_threshold": 0.6, "pii_action": "edit", "unsafe_action": "block"}}',
 ARRAY['EU AI Act', 'GDPR'])

ON CONFLICT (id) DO NOTHING;
