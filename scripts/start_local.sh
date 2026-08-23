#!/usr/bin/env bash
set -euo pipefail

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

# ─── Cleanup on exit ─────────────────────────────────────────────────────
cleanup() {
    echo ""
    step "Shutting down..."
    kill $GATEWAY_PID 2>/dev/null || true
    kill $FRONTEND_PID 2>/dev/null || true
    wait $GATEWAY_PID 2>/dev/null || true
    wait $FRONTEND_PID 2>/dev/null || true
    info "All services stopped."
}
trap cleanup EXIT INT TERM

# ─── Check prerequisites ────────────────────────────────────────────────
step "Checking prerequisites"

command -v cargo >/dev/null 2>&1 || error "Rust/Cargo not found"
info "Rust $(rustc --version | awk '{print $2}')"

command -v node >/dev/null 2>&1 || error "Node.js not found"
info "Node.js $(node --version)"

command -v psql >/dev/null 2>&1 || error "psql not found — need PostgreSQL client"
info "psql available"

pg_isready >/dev/null 2>&1 || error "PostgreSQL is not running — start it with: brew services start postgresql@14"
info "PostgreSQL is running"

# ─── Check Ollama ────────────────────────────────────────────────────────
step "Checking Ollama"

OLLAMA_MODEL="${OLLAMA_MODEL:-qwen2.5:1.5b}"
OLLAMA_BASE_URL="http://127.0.0.1:11434"

if ! command -v ollama >/dev/null 2>&1; then
    error "Ollama not found — install with: brew install ollama"
fi
info "Ollama installed: $(ollama --version 2>/dev/null || echo 'unknown version')"

# Check if Ollama server is running
if ! curl -sf "$OLLAMA_BASE_URL/api/tags" >/dev/null 2>&1; then
    warn "Ollama server not running — starting it..."
    nohup ollama serve > /tmp/ollama.log 2>&1 &
    OLLAMA_PID=$!
    sleep 3
    if curl -sf "$OLLAMA_BASE_URL/api/tags" >/dev/null 2>&1; then
        info "Ollama server started (PID: $OLLAMA_PID)"
    else
        error "Failed to start Ollama server. Check: tail /tmp/ollama.log"
    fi
else
    info "Ollama server is running"
fi

# Check if model is pulled
MODEL_EXISTS=$(curl -s "$OLLAMA_BASE_URL/api/tags" | python3 -c "
import sys, json
data = json.load(sys.stdin)
models = [m['name'] for m in data.get('models', [])]
found = any('$OLLAMA_MODEL' in name for name in models)
print('yes' if found else 'no')
" 2>/dev/null || echo "no")

if [ "$MODEL_EXISTS" = "no" ]; then
    warn "Model '$OLLAMA_MODEL' not found — pulling (this may take a few minutes)..."
    curl -s -N -X POST "$OLLAMA_BASE_URL/api/pull" -d "{\"name\":\"$OLLAMA_MODEL\"}" | python3 -c "
import sys, json
for line in sys.stdin:
    line = line.strip()
    if line:
        try:
            d = json.loads(line)
            status = d.get('status', '')
            if 'total' in d:
                pct = d.get('completed', 0) * 100 // max(d.get('total', 1), 1)
                print(f'\r  {status}: {pct}%', end='', flush=True)
            else:
                print(f'  {status}')
        except:
            pass
print()
"
    info "Model '$OLLAMA_MODEL' pulled successfully"
else
    info "Model '$OLLAMA_MODEL' is available"
fi

# Quick test: send a request to Ollama directly
info "Testing Ollama with a quick request..."
TEST_RESPONSE=$(curl -s "$OLLAMA_BASE_URL/v1/chat/completions" \
    -H "Content-Type: application/json" \
    -d "{\"model\":\"$OLLAMA_MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"Say hi in 3 words\"}],\"max_tokens\":20}" 2>/dev/null || echo "")

if echo "$TEST_RESPONSE" | python3 -c "import sys,json; d=json.load(sys.stdin); assert d.get('choices')" 2>/dev/null; then
    info "Ollama is responding correctly ✓"
else
    warn "Ollama test request may have failed — check logs"
fi

# ─── Database setup ──────────────────────────────────────────────────────
step "Setting up database"

DB_URL="postgres://controlplane:secret@localhost:5432/controlplane"

# Create user and database if they don't exist
psql -U "$(whoami)" -d postgres -c "CREATE USER controlplane WITH PASSWORD 'secret' SUPERUSER;" 2>/dev/null || true
psql -U "$(whoami)" -d postgres -c "CREATE DATABASE controlplane OWNER controlplane;" 2>/dev/null || true

# Check if tables exist, if not run migrations
if ! psql "$DB_URL" -c "SELECT 1 FROM users LIMIT 1" >/dev/null 2>&1; then
    step "Running migrations"
    for f in infra/migrations/*.sql; do
        echo "  Applying: $(basename $f)"
        psql "$DB_URL" -f "$f" -v ON_ERROR_STOP=0 --quiet 2>/dev/null || true
    done
    info "Migrations applied"
else
    info "Database 'controlplane' already has tables"
fi

# Verify verdicts table has data
VERDICT_COUNT=$(psql "$DB_URL" -t -A -c "SELECT COUNT(*) FROM verdicts;" 2>/dev/null || echo "0")
info "Verdicts in database: $VERDICT_COUNT"

# ─── Build Rust ──────────────────────────────────────────────────────────
step "Building Rust workspace"
cargo build 2>&1 | tail -n 3
info "Build complete"

# ─── Start gateway ───────────────────────────────────────────────────────
step "Starting ControlPlane gateway (provider: ollama, model: $OLLAMA_MODEL)"

# Clean up old processes on ports
lsof -ti:8900 -ti:8080 2>/dev/null | xargs kill -9 2>/dev/null || true
sleep 1

DATABASE_URL="$DB_URL" \
EVENT_BUS=inproc \
UPSTREAM_PROVIDER=ollama \
UPSTREAM_MODEL="$OLLAMA_MODEL" \
UPSTREAM_BASE_URL="$OLLAMA_BASE_URL" \
RUST_LOG=info,controlplane=debug \
    cargo run -p controlplane-gateway > /tmp/controlplane-gateway.log 2>&1 &
GATEWAY_PID=$!

step "Waiting for gateway..."
for i in $(seq 1 30); do
    if curl -sf "http://localhost:8080/health" >/dev/null 2>&1; then
        info "Gateway is healthy (PID: $GATEWAY_PID)"
        break
    fi
    if [ "$i" -eq 30 ]; then
        error "Gateway failed to start. Check: tail /tmp/controlplane-gateway.log"
    fi
    sleep 1
done

# Verify gateway is connected to PostgreSQL
if grep -q "PostgreSQL connected" /tmp/controlplane-gateway.log 2>/dev/null; then
    info "Gateway connected to PostgreSQL ✓"
else
    warn "Gateway may not be connected to PostgreSQL — check logs"
fi

# ─── Start frontend ──────────────────────────────────────────────────────
step "Starting frontend dashboard"
cd frontend
NEXT_PUBLIC_API_URL=http://localhost:8080 pnpm dev > /tmp/controlplane-frontend.log 2>&1 &
FRONTEND_PID=$!
cd ..

step "Waiting for frontend..."
for i in $(seq 1 20); do
    if curl -sf "http://localhost:3000" >/dev/null 2>&1; then
        info "Frontend is ready (PID: $FRONTEND_PID)"
        break
    fi
    if [ "$i" -eq 20 ]; then
        warn "Frontend may still be starting"
    fi
    sleep 1
done

# ─── Summary ─────────────────────────────────────────────────────────────
echo ""
echo -e "${BOLD}╔══════════════════════════════════════════════════════════╗${NC}"
echo -e "${BOLD}║   ControlPlane.ai is running!                           ║${NC}"
echo -e "${BOLD}╠══════════════════════════════════════════════════════════╣${NC}"
echo -e "${BOLD}║  Provider:  ${CYAN}ollama (local)${NC}"
echo -e "${BOLD}║  Model:     ${CYAN}$OLLAMA_MODEL${NC}"
echo -e "${BOLD}║  Database:  ${CYAN}PostgreSQL (controlplane@localhost:5432)${NC}"
echo -e "${BOLD}║  Event Bus: ${CYAN}In-process${NC}"
echo -e "${BOLD}║                                                         ║${NC}"
echo -e "${BOLD}║  Proxy:       ${CYAN}http://localhost:8900${NC}"
echo -e "${BOLD}║  Dashboard:   ${CYAN}http://localhost:8080${NC}"
echo -e "${BOLD}║  Frontend:    ${CYAN}http://localhost:3000${NC}"
echo -e "${BOLD}║  Ollama:      ${CYAN}http://localhost:11434${NC}"
echo -e "${BOLD}╠══════════════════════════════════════════════════════════╣${NC}"
echo -e "${BOLD}║  Demo accounts:                                          ║${NC}"
echo -e "${BOLD}║    admin@controlplane.test / Demo#Admin2026              ║${NC}"
echo -e "${BOLD}║    reviewer@controlplane.test / Demo#Reviewer2026        ║${NC}"
echo -e "${BOLD}║    viewer@controlplane.test / Demo#Viewer2026            ║${NC}"
echo -e "${BOLD}╠══════════════════════════════════════════════════════════╣${NC}"
echo -e "${BOLD}║  Quick test (open another terminal):                     ║${NC}"
echo -e "${BOLD}║  ${CYAN}curl -s -X POST http://localhost:8900/v1/messages \\\\${NC}"
echo -e "${BOLD}║  ${CYAN}  -H 'Content-Type: application/json' \\\\${NC}"
echo -e "${BOLD}║  ${CYAN}  -d '{\"model\":\"$OLLAMA_MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"Hi\"}],\"max_tokens\":256}'${NC}"
echo -e "${BOLD}║                                                         ║${NC}"
echo -e "${BOLD}║  Logs:                                                  ║${NC}"
echo -e "${BOLD}║    Gateway:    ${CYAN}tail -f /tmp/controlplane-gateway.log${NC}"
echo -e "${BOLD}║    Frontend:   ${CYAN}tail -f /tmp/controlplane-frontend.log${NC}"
echo -e "${BOLD}║    Ollama:     ${CYAN}tail -f /tmp/ollama.log${NC}"
echo -e "${BOLD}╚══════════════════════════════════════════════════════════╝${NC}"
echo ""
echo "Press Ctrl+C to stop all services."

wait
