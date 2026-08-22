#!/usr/bin/env bash
set -euo pipefail

# ControlPlane.ai — All-in-One Startup
# Starts infrastructure, builds, configures Gemini, and launches everything.

BOLD='\033[1m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
CYAN='\033[0;36m'
NC='\033[0m'

info()  { echo -e "${GREEN}[✓]${NC} $1"; }
warn()  { echo -e "${YELLOW}[!]${NC} $1"; }
error() { echo -e "${RED}[✗]${NC} $1"; exit 1; }
step()  { echo -e "\n${BOLD}→ $1${NC}"; }

# ─── Docker compose detection ────────────────────────────────────────────
if docker compose version >/dev/null 2>&1; then
    DC="docker compose"
else
    DC="docker-compose"
fi

# ─── Load existing .env if present ───────────────────────────────────────
if [ -f .env ]; then
    set -a
    # shellcheck disable=SC1091
    source .env
    set +a
    info ".env loaded"
fi

# ─── Config ──────────────────────────────────────────────────────────────
PROVIDER="${UPSTREAM_PROVIDER:-gemini}"
API_KEY="${UPSTREAM_API_KEY:-}"
MODEL="${UPSTREAM_MODEL:-gemini-2.0-flash}"
PROXY_PORT=8900
API_PORT=8080
FRONTEND_PORT=3000
DB_URL="postgres://controlplane:secret@localhost:5432/controlplane"

echo ""
echo -e "${BOLD}╔══════════════════════════════════════════════════╗${NC}"
echo -e "${BOLD}║   ControlPlane.ai — Full Stack Startup           ║${NC}"
echo -e "${BOLD}║   Provider: ${CYAN}${PROVIDER}${NC}${BOLD}  Model: ${CYAN}${MODEL}${NC}${BOLD}              ║${NC}"
echo -e "${BOLD}╚══════════════════════════════════════════════════╝${NC}"

# ─── Step 1: Prerequisites ──────────────────────────────────────────────
step "Checking prerequisites"

command -v cargo >/dev/null 2>&1 || error "Rust/Cargo not found. Install from https://rustup.rs"
info "Rust $(rustc --version | awk '{print $2}')"

command -v node >/dev/null 2>&1 || error "Node.js not found. Install from https://nodejs.org"
info "Node.js $(node --version)"

command -v pnpm >/dev/null 2>&1 || {
    warn "pnpm not found. Installing..."
    npm install -g pnpm
}
info "pnpm $(pnpm --version)"

if ! command -v docker >/dev/null 2>&1; then
    warn "Docker not found. Will run in in-memory mode (no PostgreSQL/NATS)."
fi

if ! command -v psql >/dev/null 2>&1; then
    warn "psql not found. Migrations may need to be run manually."
fi

# ─── Step 2: API Key check ──────────────────────────────────────────────
step "Checking API key"

if [ -z "$API_KEY" ]; then
    echo ""
    echo -e "${YELLOW}No UPSTREAM_API_KEY set.${NC}"
    echo ""
    echo "  For Gemini:  get a key at https://aistudio.google.com/apikey"
    echo "  For Anthropic: get a key at https://console.anthropic.com/"
    echo ""
    read -rp "  Paste your API key (or press Enter to skip): " API_KEY
    if [ -z "$API_KEY" ]; then
        warn "No API key provided. Gateway will start but upstream calls will fail."
        API_KEY="no-key-provided"
    fi
fi
info "API key configured (${#API_KEY} chars)"

# ─── Step 3: Create/update .env ──────────────────────────────────────────
step "Configuring .env"

if [ ! -f .env ]; then
    cp .env.example .env
    info "Created .env from .env.example"
fi

# Update or add Gemini config values
update_env() {
    local key="$1" value="$2"
    if grep -q "^${key}=" .env 2>/dev/null; then
        # macOS sed needs '', Linux sed doesn't — handle both
        if [[ "$OSTYPE" == "darwin"* ]]; then
            sed -i '' "s|^${key}=.*|${key}=${value}|" .env
        else
            sed -i "s|^${key}=.*|${key}=${value}|" .env
        fi
    else
        echo "${key}=${value}" >> .env
    fi
}

update_env "UPSTREAM_PROVIDER" "$PROVIDER"
update_env "UPSTREAM_API_KEY" "$API_KEY"
update_env "UPSTREAM_MODEL" "$MODEL"
update_env "DATABASE_URL" "$DB_URL"
update_env "EVENT_BUS" "inproc"
update_env "SEED_DEMO_USERS" "true"

info ".env configured for ${PROVIDER} / ${MODEL}"

# ─── Step 4: Start Docker infrastructure ─────────────────────────────────
if command -v docker >/dev/null 2>&1; then
    step "Starting PostgreSQL + NATS"
    $DC -f infra/docker-compose.yml up -d

    step "Waiting for PostgreSQL to be ready"
    for i in $(seq 1 30); do
        if docker exec controlplane-postgres pg_isready -U controlplane >/dev/null 2>&1; then
            info "PostgreSQL is ready"
            break
        fi
        if [ "$i" -eq 30 ]; then
            error "PostgreSQL did not become ready in 30s"
        fi
        sleep 1
    done

    step "Running database migrations"
    if command -v psql >/dev/null 2>&1; then
        for migration in infra/migrations/*.sql; do
            filename=$(basename "$migration")
            psql "$DB_URL" -f "$migration" -v ON_ERROR_STOP=1 --quiet 2>/dev/null || true
        done
        info "Migrations applied"
    else
        warn "psql not found — skipping migrations. Data may not persist."
    fi
else
    warn "Docker not available — running in in-memory mode"
fi

# ─── Step 5: Build Rust backend ──────────────────────────────────────────
step "Building Rust workspace"
cargo build 2>&1 | tail -n 5
info "Rust workspace built"

# ─── Step 6: Install frontend deps ───────────────────────────────────────
step "Installing frontend dependencies"
cd frontend
pnpm install --frozen-lockfile 2>/dev/null || pnpm install
cd ..
info "Frontend dependencies installed"

# ─── Step 7: Start gateway ───────────────────────────────────────────────
step "Starting ControlPlane gateway"
cargo run -p controlplane-gateway > /tmp/controlplane-gateway.log 2>&1 &
GATEWAY_PID=$!
info "Gateway starting (PID: $GATEWAY_PID)"

# Wait for gateway to be healthy
step "Waiting for gateway to be ready"
for i in $(seq 1 15); do
    if curl -sf "http://localhost:${API_PORT}/health" >/dev/null 2>&1; then
        info "Gateway is healthy"
        break
    fi
    if [ "$i" -eq 15 ]; then
        warn "Gateway health check timed out — it may still be starting"
        echo "    Check logs: tail -f /tmp/controlplane-gateway.log"
    fi
    sleep 1
done

# ─── Step 8: Start frontend ─────────────────────────────────────────────
step "Starting frontend dashboard"
cd frontend
pnpm dev > /tmp/controlplane-frontend.log 2>&1 &
FRONTEND_PID=$!
cd ..
info "Frontend starting (PID: $FRONTEND_PID)"

# Wait for frontend
step "Waiting for frontend to be ready"
for i in $(seq 1 15); do
    if curl -sf "http://localhost:${FRONTEND_PORT}" >/dev/null 2>&1; then
        info "Frontend is ready"
        break
    fi
    if [ "$i" -eq 15 ]; then
        warn "Frontend may still be starting"
        echo "    Check logs: tail -f /tmp/controlplane-frontend.log"
    fi
    sleep 1
done

# ─── Step 9: Quick smoke test ────────────────────────────────────────────
step "Running smoke test"
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:${API_PORT}/health" 2>/dev/null || echo "000")
if [ "$HTTP_CODE" = "200" ]; then
    info "Dashboard API is responding"
else
    warn "Dashboard API returned HTTP $HTTP_CODE"
fi

# ─── Step 10: Start demo traffic (optional) ─────────────────────────────
if [ "${DEMO_TRAFFIC:-false}" = "true" ] || [ "${DEMO_TRAFFIC:-0}" = "1" ]; then
    step "Starting demo traffic generator"
    bash scripts/demo_traffic.sh 30 600 > /tmp/controlplane-traffic.log 2>&1 &
    TRAFFIC_PID=$!
    info "Demo traffic running (PID: $TRAFFIC_PID, 30 req/min for 10 min)"
else
    info "Skipping demo traffic (set DEMO_TRAFFIC=true to enable)"
fi

# ─── Summary ─────────────────────────────────────────────────────────────
echo ""
echo -e "${BOLD}╔══════════════════════════════════════════════════════════╗${NC}"
echo -e "${BOLD}║   ControlPlane.ai is running!                           ║${NC}"
echo -e "${BOLD}╠══════════════════════════════════════════════════════════╣${NC}"
echo -e "${BOLD}║  Provider:  ${CYAN}${PROVIDER}${NC}"
echo -e "${BOLD}║  Model:     ${CYAN}${MODEL}${NC}"
echo -e "${BOLD}║                                                         ║${NC}"
echo -e "${BOLD}║  Proxy:       ${CYAN}http://localhost:${PROXY_PORT}${NC}"
echo -e "${BOLD}║  Dashboard:   ${CYAN}http://localhost:${API_PORT}${NC}"
echo -e "${BOLD}║  Frontend:    ${CYAN}http://localhost:${FRONTEND_PORT}${NC}"
echo -e "${BOLD}╠══════════════════════════════════════════════════════════╣${NC}"
echo -e "${BOLD}║  Demo accounts:                                          ║${NC}"
echo -e "${BOLD}║    admin@controlplane.test / Demo#Admin2026              ║${NC}"
echo -e "${BOLD}║    reviewer@controlplane.test / Demo#Reviewer2026        ║${NC}"
echo -e "${BOLD}║    viewer@controlplane.test / Demo#Viewer2026            ║${NC}"
echo -e "${BOLD}╠══════════════════════════════════════════════════════════╣${NC}"
echo -e "${BOLD}║  Quick test (Gemini via proxy):                          ║${NC}"
echo -e "${BOLD}║  ${CYAN}curl -s -X POST http://localhost:${PROXY_PORT}/v1/messages \\${NC}"
echo -e "${BOLD}║  ${CYAN}  -H 'Content-Type: application/json' \\${NC}"
echo -e "${BOLD}║  ${CYAN}  -d '{\"contents\":[{\"role\":\"user\",\"parts\":[{\"text\":\"Hello\"}]}]}'${NC}"
echo -e "${BOLD}║                                                         ║${NC}"
echo -e "${BOLD}║  Logs:                                                  ║${NC}"
echo -e "${BOLD}║    Gateway:    ${CYAN}tail -f /tmp/controlplane-gateway.log${NC}"
echo -e "${BOLD}║    Frontend:   ${CYAN}tail -f /tmp/controlplane-frontend.log${NC}"
echo -e "${BOLD}╚══════════════════════════════════════════════════════════╝${NC}"
echo ""
echo "Press Ctrl+C to stop all services."

# ─── Cleanup on exit ─────────────────────────────────────────────────────
cleanup() {
    echo ""
    step "Shutting down..."
    kill $GATEWAY_PID 2>/dev/null || true
    kill $FRONTEND_PID 2>/dev/null || true
    kill $TRAFFIC_PID 2>/dev/null || true
    info "All services stopped."
}

trap cleanup EXIT INT TERM
wait
