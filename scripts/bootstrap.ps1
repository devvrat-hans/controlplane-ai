# ControlPlane.ai — Bootstrap Script (Windows PowerShell)
# Checks prerequisites, installs dependencies, and builds everything.

$ErrorActionPreference = "Stop"

function Write-Step($msg) { Write-Host "`n→ $msg" -ForegroundColor Cyan }
function Write-Info($msg) { Write-Host "[✓] $msg" -ForegroundColor Green }
function Write-Warn($msg) { Write-Host "[!] $msg" -ForegroundColor Yellow }
function Write-Err($msg)  { Write-Host "[✗] $msg" -ForegroundColor Red; exit 1 }

Write-Host "╔══════════════════════════════════════╗" -ForegroundColor White
Write-Host "║    ControlPlane.ai — Bootstrap       ║" -ForegroundColor White
Write-Host "╚══════════════════════════════════════╝" -ForegroundColor White

# --- Prerequisites ---

Write-Step "Checking prerequisites"

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Err "Rust/Cargo not found. Install from https://rustup.rs"
}
Write-Info "Rust $(rustc --version)"

if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
    Write-Err "Node.js not found. Install from https://nodejs.org"
}
Write-Info "Node.js $(node --version)"

if (-not (Get-Command pnpm -ErrorAction SilentlyContinue)) {
    Write-Warn "pnpm not found. Installing..."
    npm install -g pnpm
}
Write-Info "pnpm $(pnpm --version)"

$hasDocker = Get-Command docker -ErrorAction SilentlyContinue
if ($hasDocker) {
    Write-Info "Docker found"
} else {
    Write-Warn "Docker not found. Running in memory mode (no PostgreSQL/NATS)."
}

# --- Environment ---

Write-Step "Setting up environment"

if (-not (Test-Path .env)) {
    Copy-Item .env.example .env
    Write-Info "Created .env from .env.example"
} else {
    Write-Info ".env already exists"
}

# --- Rust build ---

Write-Step "Building Rust workspace"
cargo build
if ($LASTEXITCODE -ne 0) { Write-Err "Rust build failed" }
Write-Info "Rust workspace built successfully"

# --- Frontend ---

Write-Step "Installing frontend dependencies"
Push-Location frontend
pnpm install
if ($LASTEXITCODE -ne 0) { Write-Err "Frontend install failed" }
Write-Info "Frontend dependencies installed"

Write-Step "Building frontend"
pnpm build
if ($LASTEXITCODE -ne 0) { Write-Err "Frontend build failed" }
Write-Info "Frontend built successfully"
Pop-Location

# --- Infrastructure ---

if ($hasDocker) {
    Write-Step "Starting infrastructure (PostgreSQL + NATS)"
    docker compose -f infra/docker-compose.yml up -d
    Write-Info "Infrastructure containers started"

    Write-Step "Waiting for PostgreSQL..."
    Start-Sleep -Seconds 5

    Write-Step "Running database migrations"
    Get-ChildItem infra/migrations/*.sql | Sort-Object Name | ForEach-Object {
        Write-Host "  Applying: $($_.Name)"
        Get-Content $_.FullName | docker exec -i controlplane-postgres psql -U controlplane -d controlplane
    }
    Write-Info "Migrations applied"
} else {
    Write-Warn "Skipping infrastructure. Using in-memory mode."
}

# --- Done ---

Write-Host ""
Write-Host "╔══════════════════════════════════════╗" -ForegroundColor White
Write-Host "║    Bootstrap complete!                ║" -ForegroundColor White
Write-Host "╚══════════════════════════════════════╝" -ForegroundColor White
Write-Host ""
Write-Host "Next steps:"
Write-Host "  1. Start the gateway:   cargo run -p controlplane-gateway"
Write-Host "  2. Start the frontend:  cd frontend; pnpm dev"
Write-Host "  3. Open dashboard:      http://localhost:3000"
