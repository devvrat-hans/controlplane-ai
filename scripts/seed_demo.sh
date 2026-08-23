#!/usr/bin/env bash
set -euo pipefail

# ControlPlane.ai — Seed Demo Data
# Populates the database with comprehensive demo data:
# - 3 apps, 3 users, policies (from migration 009)
# - 75 intercepted calls, 75+ verdicts, 8 escalation cases (from migration 013)

BOLD='\033[1m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m'

info()  { echo -e "${GREEN}[✓]${NC} $1"; }
step()  { echo -e "\n${BOLD}→ $1${NC}"; }
warn()  { echo -e "${YELLOW}[!]${NC} $1"; }

DB_URL="${DATABASE_URL:-postgres://controlplane:secret@localhost:5432/controlplane}"

echo -e "${BOLD}╔══════════════════════════════════════════════╗${NC}"
echo -e "${BOLD}║    ControlPlane.ai — Full Demo Seed          ║${NC}"
echo -e "${BOLD}╚══════════════════════════════════════════════╝${NC}"

step "Running base schema migrations"
for f in infra/migrations/0{01,02,03,04,05,06,07,08,10,11,12,14,15}*.sql; do
    psql "$DB_URL" --quiet -v ON_ERROR_STOP=1 -f "$f" 2>/dev/null || true
done
info "Base schema applied"

step "Seeding demo users, apps, and policies (migration 009)"
psql "$DB_URL" --quiet -v ON_ERROR_STOP=1 -f infra/migrations/009_seed_demo_data.sql 2>/dev/null || true
info "Users, apps, and policies seeded"

step "Seeding full demo data — 75 calls, verdicts, escalations (migration 013)"
psql "$DB_URL" --quiet -v ON_ERROR_STOP=1 -f infra/migrations/013_seed_full_demo.sql
info "Full demo data seeded"

step "Summary"
echo ""

CALL_COUNT=$(psql "$DB_URL" -t -c "SELECT COUNT(*) FROM intercepted_calls;" 2>/dev/null | tr -d ' ')
VERDICT_COUNT=$(psql "$DB_URL" -t -c "SELECT COUNT(*) FROM verdicts;" 2>/dev/null | tr -d ' ')
ESC_COUNT=$(psql "$DB_URL" -t -c "SELECT COUNT(*) FROM escalation_cases;" 2>/dev/null | tr -d ' ')

echo -e "  Intercepted calls: ${GREEN}${CALL_COUNT:-?}${NC}"
echo -e "  Verdicts:          ${GREEN}${VERDICT_COUNT:-?}${NC}"
echo -e "  Escalation cases:  ${GREEN}${ESC_COUNT:-?}${NC}"
echo ""
echo -e "Outcome distribution:"
psql "$DB_URL" -c "SELECT outcome, COUNT(*) as count FROM verdicts GROUP BY outcome ORDER BY count DESC;" 2>/dev/null || true
echo ""
info "Demo data is ready. Start the demo with: ./scripts/run_demo.sh"
