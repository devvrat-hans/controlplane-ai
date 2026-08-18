-- Demo seed data: users, apps, and default policies.
-- Only run in development/demo environments.

-- Demo users (passwords are bcrypt hashes of "Demo#Admin2026")
INSERT INTO users (id, email, password_hash, role, name) VALUES
    ('00000000-0000-0000-0000-000000000001', 'admin@controlplane.test',
     '$argon2id$v=19$m=19456,t=2,p=1$placeholder_hash_admin', 'admin', 'Admin User'),
    ('00000000-0000-0000-0000-000000000002', 'reviewer@controlplane.test',
     '$argon2id$v=19$m=19456,t=2,p=1$placeholder_hash_reviewer', 'reviewer', 'Reviewer User'),
    ('00000000-0000-0000-0000-000000000003', 'viewer@controlplane.test',
     '$argon2id$v=19$m=19456,t=2,p=1$placeholder_hash_viewer', 'viewer', 'Viewer User')
ON CONFLICT (email) DO NOTHING;

-- Demo apps
INSERT INTO apps (id, name, team_id, api_key_hash, description) VALUES
    ('10000000-0000-0000-0000-000000000001', 'ChatBot-Prod', NULL,
     'demo_key_hash_chatbot', 'Production customer-facing chatbot'),
    ('10000000-0000-0000-0000-000000000002', 'Agent-Internal', NULL,
     'demo_key_hash_agent', 'Internal agentic workflow assistant'),
    ('10000000-0000-0000-0000-000000000003', 'RAG-Customer-Support', NULL,
     'demo_key_hash_rag', 'RAG-powered customer support system')
ON CONFLICT DO NOTHING;

-- Default policies for ChatBot-Prod
INSERT INTO policies (app_id, axis, threshold_config, version) VALUES
    ('10000000-0000-0000-0000-000000000001', 'performance',
     '{"groundedness_threshold": 0.6, "action": "escalate"}', 1),
    ('10000000-0000-0000-0000-000000000001', 'cost',
     '{"max_tokens_per_request": 4096, "daily_budget_cents": 10000, "retry_max": 5, "retry_window_seconds": 60, "action": "block"}', 1),
    ('10000000-0000-0000-0000-000000000001', 'responsibility',
     '{"pii_action": "edit", "bias_threshold": 0.7, "bias_action": "escalate", "unsafe_action": "block"}', 1)
ON CONFLICT DO NOTHING;

-- Default policies for Agent-Internal
INSERT INTO policies (app_id, axis, threshold_config, version) VALUES
    ('10000000-0000-0000-0000-000000000002', 'performance',
     '{"groundedness_threshold": 0.5, "action": "escalate"}', 1),
    ('10000000-0000-0000-0000-000000000002', 'cost',
     '{"max_tokens_per_request": 8192, "daily_budget_cents": 50000, "retry_max": 10, "retry_window_seconds": 120, "action": "escalate"}', 1),
    ('10000000-0000-0000-0000-000000000002', 'responsibility',
     '{"pii_action": "edit", "bias_threshold": 0.8, "bias_action": "escalate", "unsafe_action": "block"}', 1)
ON CONFLICT DO NOTHING;

-- Default policies for RAG-Customer-Support
INSERT INTO policies (app_id, axis, threshold_config, version) VALUES
    ('10000000-0000-0000-0000-000000000003', 'performance',
     '{"groundedness_threshold": 0.7, "action": "escalate"}', 1),
    ('10000000-0000-0000-0000-000000000003', 'cost',
     '{"max_tokens_per_request": 2048, "daily_budget_cents": 5000, "retry_max": 3, "retry_window_seconds": 30, "action": "block"}', 1),
    ('10000000-0000-0000-0000-000000000003', 'responsibility',
     '{"pii_action": "edit", "bias_threshold": 0.6, "bias_action": "escalate", "unsafe_action": "block"}', 1)
ON CONFLICT DO NOTHING;
