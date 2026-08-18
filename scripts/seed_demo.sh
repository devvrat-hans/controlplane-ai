#!/usr/bin/env bash
set -euo pipefail

# ControlPlane.ai — Seed Demo Data
# Populates the database with sample intercepted calls, verdicts, and escalations.

BOLD='\033[1m'
GREEN='\033[0;32m'
NC='\033[0m'

info()  { echo -e "${GREEN}[✓]${NC} $1"; }
step()  { echo -e "\n${BOLD}→ $1${NC}"; }

DB_URL="${DATABASE_URL:-postgres://controlplane:secret@localhost:5432/controlplane}"

echo -e "${BOLD}╔══════════════════════════════════════╗${NC}"
echo -e "${BOLD}║    ControlPlane.ai — Seed Demo       ║${NC}"
echo -e "${BOLD}╚══════════════════════════════════════╝${NC}"

step "Inserting sample intercepted calls"

psql "$DB_URL" --quiet -v ON_ERROR_STOP=1 <<'SQL'
-- Sample intercepted calls for demonstration
INSERT INTO intercepted_calls (id, correlation_id, app_id, model, token_count_input, token_count_output, upstream_latency_ms, created_at)
VALUES
    ('20000000-0000-0000-0000-000000000001', 'c0000000-0000-0000-0000-000000000001',
     '10000000-0000-0000-0000-000000000001', 'claude-sonnet-4-20250514', 150, 420, 1200, NOW() - INTERVAL '5 minutes'),
    ('20000000-0000-0000-0000-000000000002', 'c0000000-0000-0000-0000-000000000002',
     '10000000-0000-0000-0000-000000000001', 'claude-sonnet-4-20250514', 200, 1800, 2400, NOW() - INTERVAL '4 minutes'),
    ('20000000-0000-0000-0000-000000000003', 'c0000000-0000-0000-0000-000000000003',
     '10000000-0000-0000-0000-000000000002', 'claude-sonnet-4-20250514', 80, 300, 800, NOW() - INTERVAL '3 minutes'),
    ('20000000-0000-0000-0000-000000000004', 'c0000000-0000-0000-0000-000000000004',
     '10000000-0000-0000-0000-000000000003', 'claude-sonnet-4-20250514', 500, 2100, 3100, NOW() - INTERVAL '2 minutes'),
    ('20000000-0000-0000-0000-000000000005', 'c0000000-0000-0000-0000-000000000005',
     '10000000-0000-0000-0000-000000000001', 'claude-sonnet-4-20250514', 120, 350, 900, NOW() - INTERVAL '1 minute')
ON CONFLICT DO NOTHING;

-- Sample verdicts
INSERT INTO verdicts (call_id, axis, path, outcome, confidence, reason, check_name, duration_ms) VALUES
    ('20000000-0000-0000-0000-000000000001', 'responsibility', 'fast', 'edit', 0.95,
     'Detected AWS access key (AKIA...) in response — redacted', 'secret_detection', 2),
    ('20000000-0000-0000-0000-000000000002', 'cost', 'fast', 'block', 0.99,
     'Response token count (1800) exceeds per-request cap (1500)', 'cost_cap', 1),
    ('20000000-0000-0000-0000-000000000003', 'performance', 'shadow', 'escalate', 0.42,
     'Groundedness score 0.42 — response makes ungrounded claims', 'groundedness', 480),
    ('20000000-0000-0000-0000-000000000004', 'responsibility', 'shadow', 'escalate', 0.68,
     'Potential gender bias detected in hiring recommendation', 'bias_classifier', 320),
    ('20000000-0000-0000-0000-000000000005', 'cost', 'fast', 'pass', 0.10,
     'Token count within budget, no anomalies', 'cost_cap', 1)
ON CONFLICT DO NOTHING;
SQL

info "Sample calls and verdicts inserted"

step "Done"
echo ""
echo "Demo data is ready. Start the demo with: ./scripts/run_demo.sh"
