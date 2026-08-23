$ErrorActionPreference = "Stop"

function Write-Info { param($msg) Write-Host "  [OK] $msg" -ForegroundColor Green }
function Write-Warn { param($msg) Write-Host "  [!] $msg" -ForegroundColor Yellow }
function Write-Err  { param($msg) Write-Host "  [X] $msg" -ForegroundColor Red; exit 1 }
function Write-Step { param($msg) Write-Host "`n-> $msg" -ForegroundColor White }

# ─── Check prerequisites ────────────────────────────────────────────────
Write-Step "Checking prerequisites"

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { Write-Err "Rust/Cargo not found. Install from https://rustup.rs" }
Write-Info "Rust $(rustc --version)"

if (-not (Get-Command node -ErrorAction SilentlyContinue)) { Write-Err "Node.js not found. Install from https://nodejs.org" }
Write-Info "Node.js $(node --version)"

if (-not (Get-Command psql -ErrorAction SilentlyContinue)) { Write-Err "psql not found - need PostgreSQL client" }
Write-Info "psql available"

try { pg_isready | Out-Null } catch { Write-Err "PostgreSQL is not running. Start it from Services or pgAdmin." }
Write-Info "PostgreSQL is running"

# ─── Check Ollama ────────────────────────────────────────────────────────
Write-Step "Checking Ollama"

$OLLAMA_MODEL = if ($env:OLLAMA_MODEL) { $env:OLLAMA_MODEL } else { "qwen2.5:1.5b" }
$OLLAMA_BASE_URL = "http://127.0.0.1:11434"

if (-not (Get-Command ollama -ErrorAction SilentlyContinue)) {
    Write-Err "Ollama not found. Install from https://ollama.com/download"
}
Write-Info "Ollama installed"

try {
    Invoke-RestMethod "$OLLAMA_BASE_URL/api/tags" -TimeoutSec 3 | Out-Null
    Write-Info "Ollama server is running"
} catch {
    Write-Warn "Ollama server not running - starting it..."
    Start-Process ollama -ArgumentList "serve" -WindowStyle Hidden
    Start-Sleep -Seconds 3
    try {
        Invoke-RestMethod "$OLLAMA_BASE_URL/api/tags" -TimeoutSec 3 | Out-Null
        Write-Info "Ollama server started"
    } catch {
        Write-Err "Failed to start Ollama server"
    }
}

# Check if model is pulled
$tags = Invoke-RestMethod "$OLLAMA_BASE_URL/api/tags"
$modelExists = $tags.models | Where-Object { $_.name -like "*$OLLAMA_MODEL*" }

if (-not $modelExists) {
    Write-Warn "Model '$OLLAMA_MODEL' not found - pulling (may take a few minutes)..."
    ollama pull $OLLAMA_MODEL
    Write-Info "Model '$OLLAMA_MODEL' pulled successfully"
} else {
    Write-Info "Model '$OLLAMA_MODEL' is available"
}

# Quick test
Write-Info "Testing Ollama with a quick request..."
try {
    $testBody = '{"model":"' + $OLLAMA_MODEL + '","messages":[{"role":"user","content":"Say hi in 3 words"}],"max_tokens":20}'
    Invoke-RestMethod "$OLLAMA_BASE_URL/v1/chat/completions" -Method Post -ContentType "application/json" -Body $testBody -TimeoutSec 30 | Out-Null
    Write-Info "Ollama is responding correctly"
} catch {
    Write-Warn "Ollama test request may have failed - check logs"
}

# ─── Database setup ──────────────────────────────────────────────────────
Write-Step "Setting up database"

$DB_URL = "postgres://controlplane:secret@localhost:5432/controlplane"

# Try connecting with the controlplane user first (skip postgres superuser if DB already exists)
$dbExists = $false
try {
    $check = psql $DB_URL -t -A -c "SELECT 1;" 2>$null
    if ($check -eq "1") { $dbExists = $true }
} catch {}

if (-not $dbExists) {
    Write-Warn "Database not found - creating with postgres superuser (requires PGPASSWORD or trust auth)"
    $env:PGPASSWORD = if ($env:PGPASSWORD) { $env:PGPASSWORD } else { "postgres" }
    psql -U postgres -w -c "CREATE USER controlplane WITH PASSWORD 'secret' SUPERUSER;" 2>$null
    psql -U postgres -w -c "CREATE DATABASE controlplane OWNER controlplane;" 2>$null
    $env:PGPASSWORD = $null
}

$tableCheck = psql $DB_URL -t -A -c "SELECT 1 FROM users LIMIT 1" 2>$null
if (-not $tableCheck) {
    Write-Step "Running migrations"
    Get-ChildItem "infra\migrations\*.sql" | Sort-Object Name | ForEach-Object {
        Write-Host "  Applying: $($_.Name)"
        psql $DB_URL -f $_.FullName --quiet 2>$null
    }
    Write-Info "Migrations applied"
} else {
    Write-Info "Database 'controlplane' already has tables"
}

$verdictCount = psql $DB_URL -t -A -c "SELECT COUNT(*) FROM verdicts;" 2>$null
Write-Info "Verdicts in database: $verdictCount"

# ─── Build Rust ──────────────────────────────────────────────────────────
Write-Step "Building Rust workspace"
cargo build
Write-Info "Build complete"

# ─── Start gateway ───────────────────────────────────────────────────────
Write-Step "Starting ControlPlane gateway (provider: ollama, model: $OLLAMA_MODEL)"

# Kill old processes on ports
Get-NetTCPConnection -LocalPort 8900,8080 -ErrorAction SilentlyContinue |
    ForEach-Object { Stop-Process -Id $_.OwningProcess -Force -ErrorAction SilentlyContinue }
Start-Sleep -Seconds 1

$env:DATABASE_URL = $DB_URL
$env:EVENT_BUS = "inproc"
$env:UPSTREAM_PROVIDER = "ollama"
$env:UPSTREAM_MODEL = $OLLAMA_MODEL
$env:UPSTREAM_BASE_URL = $OLLAMA_BASE_URL
$env:DASHBOARD_API_PORT = "8080"
$env:RUST_LOG = "info,controlplane=debug"

$gatewayJob = Start-Job -ScriptBlock {
    Set-Location $using:PWD
    $env:DATABASE_URL = $using:DB_URL
    $env:EVENT_BUS = "inproc"
    $env:UPSTREAM_PROVIDER = "ollama"
    $env:UPSTREAM_MODEL = $using:OLLAMA_MODEL
    $env:UPSTREAM_BASE_URL = $using:OLLAMA_BASE_URL
    $env:DASHBOARD_API_PORT = "8080"
    $env:RUST_LOG = "info,controlplane=debug"
    cargo run -p controlplane-gateway 2>&1
}

Write-Step "Waiting for gateway..."
$ready = $false
for ($i = 1; $i -le 30; $i++) {
    try {
        Invoke-RestMethod "http://localhost:8080/health" -TimeoutSec 2 | Out-Null
        Write-Info "Gateway is healthy"
        $ready = $true
        break
    } catch {
        Start-Sleep -Seconds 1
    }
}
if (-not $ready) { Write-Err "Gateway failed to start. Check 'Receive-Job $($gatewayJob.Id)'" }

# ─── Start frontend ──────────────────────────────────────────────────────
Write-Step "Starting frontend dashboard"

$frontendJob = Start-Job -ScriptBlock {
    Set-Location "$using:PWD\frontend"
    $env:NEXT_PUBLIC_API_URL = "http://localhost:8080"
    pnpm dev 2>&1
}

Write-Step "Waiting for frontend..."
for ($i = 1; $i -le 20; $i++) {
    try {
        Invoke-WebRequest "http://localhost:3000" -TimeoutSec 2 | Out-Null
        Write-Info "Frontend is ready"
        break
    } catch {
        if ($i -eq 20) { Write-Warn "Frontend may still be starting" }
        Start-Sleep -Seconds 1
    }
}

# ─── Summary ─────────────────────────────────────────────────────────────
Write-Host ""
Write-Host "  ControlPlane.ai is running!" -ForegroundColor Cyan
Write-Host "  ========================================" -ForegroundColor Cyan
Write-Host "  Provider:   ollama (local)" -ForegroundColor DarkGray
Write-Host "  Model:      $OLLAMA_MODEL" -ForegroundColor DarkGray
Write-Host "  Database:   PostgreSQL (controlplane@localhost:5432)" -ForegroundColor DarkGray
Write-Host "  Event Bus:  In-process" -ForegroundColor DarkGray
Write-Host ""
Write-Host "  Proxy:      http://localhost:8900" -ForegroundColor Green
Write-Host "  Dashboard:  http://localhost:8080" -ForegroundColor Green
Write-Host "  Frontend:   http://localhost:3000" -ForegroundColor Green
Write-Host "  Ollama:     http://localhost:11434" -ForegroundColor Green
Write-Host ""
Write-Host "  Demo accounts:" -ForegroundColor DarkGray
Write-Host "    admin@controlplane.ai / admin123" -ForegroundColor DarkGray
Write-Host "    reviewer@controlplane.ai / reviewer123" -ForegroundColor DarkGray
Write-Host "    viewer@controlplane.ai / viewer123" -ForegroundColor DarkGray
Write-Host ""
Write-Host "  Quick test:" -ForegroundColor Yellow
Write-Host "    Invoke-RestMethod http://localhost:8900/v1/messages -Method Post ``" -ForegroundColor Yellow
Write-Host "      -ContentType 'application/json' ``" -ForegroundColor Yellow
Write-Host "      -Body '{`"model`":`"$OLLAMA_MODEL`",`"messages`":[{`"role`":`"user`",`"content`":`"Hi`"}],`"max_tokens`":256}'" -ForegroundColor Yellow
Write-Host ""
Write-Host "  Press Ctrl+C to stop all services." -ForegroundColor DarkGray
Write-Host ""

try {
    while ($true) {
        Start-Sleep -Seconds 5
        if ($gatewayJob.State -eq "Failed") { Write-Warn "Gateway job failed"; break }
    }
} finally {
    Write-Step "Shutting down..."
    Stop-Job $gatewayJob -ErrorAction SilentlyContinue
    Stop-Job $frontendJob -ErrorAction SilentlyContinue
    Remove-Job $gatewayJob -Force -ErrorAction SilentlyContinue
    Remove-Job $frontendJob -Force -ErrorAction SilentlyContinue
    Get-NetTCPConnection -LocalPort 8900,8080 -ErrorAction SilentlyContinue |
        ForEach-Object { Stop-Process -Id $_.OwningProcess -Force -ErrorAction SilentlyContinue }
    Write-Info "All services stopped."
}
