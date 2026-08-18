#!/usr/bin/env bash
set -euo pipefail

# ControlPlane.ai — Bootstrap Script
# Checks prerequisites, installs dependencies, and builds everything.

BOLD='\033[1m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

info()  { echo -e "${GREEN}[✓]${NC} $1"; }
warn()  { echo -e "${YELLOW}[!]${NC} $1"; }
error() { echo -e "${RED}[✗]${NC} $1"; exit 1; }
step()  { echo -e "\n${BOLD}→ $1${NC}"; }

echo -e "${BOLD}╔══════════════════════════════════════╗${NC}"
echo -e "${BOLD}║    ControlPlane.ai — Bootstrap       ║${NC}"
echo -e "${BOLD}╚══════════════════════════════════════╝${NC}"

# --- Prerequisites -----------------------------------------------------------

step "Checking prerequisites"

command -v cargo >/dev/null 2>&1 || error "Rust/Cargo not found. Install from https://rustup.rs"
info "Rust $(rustc --version | awk '{print $2}')"

command -v node >/dev/null 2>&1 || error "Node.js not found. Install from https://nodejs.org"
NODE_VERSION=$(node --version)
info "Node.js $NODE_VERSION"

command -v pnpm >/dev/null 2>&1 || {
    warn "pnpm not found. Installing..."
    npm install -g pnpm
}
info "pnpm $(pnpm --version)"

if command -v docker >/dev/null 2>&1; then
    info "Docker $(docker --version | awk '{print $3}' | tr -d ',')"
else
    warn "Docker not found. You can still run in memory mode (no PostgreSQL/NATS required)."
fi

# --- Environment -------------------------------------------------------------

step "Setting up environment"

if [ ! -f .env ]; then
    cp .env.example .env
    info "Created .env from .env.example"
else
    info ".env already exists"
fi

# --- Rust build --------------------------------------------------------------

step "Building Rust workspace"

cargo build 2>&1 | tail -n 5
info "Rust workspace built successfully"

# --- Frontend ----------------------------------------------------------------

step "Installing frontend dependencies"

cd frontend
pnpm install --frozen-lockfile 2>/dev/null || pnpm install
info "Frontend dependencies installed"

step "Building frontend"

pnpm build 2>&1 | tail -n 10
info "Frontend built successfully"
cd ..

# --- Infrastructure (optional) -----------------------------------------------

if command -v docker >/dev/null 2>&1; then
    step "Starting infrastructure (PostgreSQL + NATS)"
    docker compose -f infra/docker-compose.yml up -d
    info "Infrastructure containers started"

    step "Waiting for PostgreSQL to be ready"
    for i in $(seq 1 30); do
        if docker exec controlplane-postgres pg_isready -U controlplane >/dev/null 2>&1; then
            info "PostgreSQL is ready"
            break
        fi
        sleep 1
    done

    step "Running database migrations"
    bash infra/migrate.sh
    info "Migrations applied"
else
    warn "Skipping infrastructure (Docker not available). Using in-memory mode."
fi

# --- Done --------------------------------------------------------------------

echo ""
echo -e "${BOLD}╔══════════════════════════════════════╗${NC}"
echo -e "${BOLD}║    Bootstrap complete!                ║${NC}"
echo -e "${BOLD}╚══════════════════════════════════════╝${NC}"
echo ""
echo "Next steps:"
echo "  1. Start the gateway:   cargo run -p controlplane-gateway"
echo "  2. Start the frontend:  cd frontend && pnpm dev"
echo "  3. Open dashboard:      http://localhost:3000"
echo ""
