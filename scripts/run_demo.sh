#!/usr/bin/env bash
set -euo pipefail

# ControlPlane.ai — Run Demo
# Starts all services needed for a live demonstration.

BOLD='\033[1m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { echo -e "${GREEN}[✓]${NC} $1"; }
warn()  { echo -e "${YELLOW}[!]${NC} $1"; }
step()  { echo -e "\n${BOLD}→ $1${NC}"; }

echo -e "${BOLD}╔══════════════════════════════════════╗${NC}"
echo -e "${BOLD}║    ControlPlane.ai — Demo Runner     ║${NC}"
echo -e "${BOLD}╚══════════════════════════════════════╝${NC}"

# --- Bootstrap if needed -----------------------------------------------------

if [ ! -f target/debug/controlplane-gateway ] && [ ! -f target/release/controlplane-gateway ]; then
    step "First run detected — running bootstrap"
    bash scripts/bootstrap.sh
fi

# --- Ensure infrastructure is up ---------------------------------------------

if command -v docker >/dev/null 2>&1; then
    step "Ensuring infrastructure is running"
    docker compose -f infra/docker-compose.yml up -d
    sleep 2
    info "Infrastructure ready"
fi

# --- Start gateway in background ---------------------------------------------

step "Starting ControlPlane gateway"
cargo run -p controlplane-gateway &
GATEWAY_PID=$!
info "Gateway starting (PID: $GATEWAY_PID)"

sleep 3

# --- Start frontend ----------------------------------------------------------

step "Starting frontend dashboard"
cd frontend
pnpm dev &
FRONTEND_PID=$!
cd ..
info "Frontend starting (PID: $FRONTEND_PID)"

sleep 3

# --- Summary -----------------------------------------------------------------

echo ""
echo -e "${BOLD}╔══════════════════════════════════════════════════╗${NC}"
echo -e "${BOLD}║    Demo is running!                              ║${NC}"
echo -e "${BOLD}╠══════════════════════════════════════════════════╣${NC}"
echo -e "${BOLD}║  Proxy:      http://localhost:8900               ║${NC}"
echo -e "${BOLD}║  API:        http://localhost:8080               ║${NC}"
echo -e "${BOLD}║  Dashboard:  http://localhost:3000               ║${NC}"
echo -e "${BOLD}╠══════════════════════════════════════════════════╣${NC}"
echo -e "${BOLD}║  Demo accounts:                                  ║${NC}"
echo -e "${BOLD}║    admin@controlplane.test / Demo#Admin2026      ║${NC}"
echo -e "${BOLD}║    reviewer@controlplane.test / Demo#Reviewer2026║${NC}"
echo -e "${BOLD}║    viewer@controlplane.test / Demo#Viewer2026    ║${NC}"
echo -e "${BOLD}╚══════════════════════════════════════════════════╝${NC}"
echo ""
echo "Press Ctrl+C to stop all services."

# --- Cleanup on exit ---------------------------------------------------------

cleanup() {
    echo ""
    step "Shutting down..."
    kill $GATEWAY_PID 2>/dev/null || true
    kill $FRONTEND_PID 2>/dev/null || true
    info "All services stopped."
}

trap cleanup EXIT INT TERM
wait
