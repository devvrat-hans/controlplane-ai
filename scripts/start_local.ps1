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

# ─── Kill old gateway (must happen BEFORE build to release EXE lock) ─────
Write-Step "Stopping any previous gateway"
# Kill from saved PID file
$pidFile = Join-Path $PWD ".gateway.pid"
if (Test-Path $pidFile) {
    $oldPid = Get-Content $pidFile -ErrorAction SilentlyContinue
    if ($oldPid) {
        Write-Host "  Killing saved PID $oldPid" -ForegroundColor DarkGray
        Stop-Process -Id ([int]$oldPid) -Force -ErrorAction SilentlyContinue
    }
    Remove-Item $pidFile -Force -ErrorAction SilentlyContinue
}
# Stop all old background jobs from previous script runs
Get-Job | Stop-Job -PassThru -ErrorAction SilentlyContinue | Remove-Job -Force -ErrorAction SilentlyContinue
# Kill gateway process by name (catches any instance)
Get-Process -Name "controlplane-gateway" -ErrorAction SilentlyContinue |
    Stop-Process -Force -ErrorAction SilentlyContinue
# Kill anything holding our ports
foreach ($port in @(8900, 8080)) {
    Get-NetTCPConnection -LocalPort $port -ErrorAction SilentlyContinue | ForEach-Object {
        Write-Host "  Killing PID $($_.OwningProcess) on port $port" -ForegroundColor DarkGray
        Stop-Process -Id $_.OwningProcess -Force -ErrorAction SilentlyContinue
    }
}
# Wait for ports to fully release
Start-Sleep -Seconds 3
# Verify ports are free
$still_busy = Get-NetTCPConnection -LocalPort 8900,8080 -State Listen -ErrorAction SilentlyContinue
if ($still_busy) {
    Write-Warn "Ports still in use after cleanup — waiting longer..."
    Start-Sleep -Seconds 5
    # Try once more
    Get-NetTCPConnection -LocalPort 8900,8080 -State Listen -ErrorAction SilentlyContinue | ForEach-Object {
        Stop-Process -Id $_.OwningProcess -Force -ErrorAction SilentlyContinue
    }
    Start-Sleep -Seconds 2
}

# ─── Build Rust ──────────────────────────────────────────────────────────
Write-Step "Building Rust workspace"
cargo build -p controlplane-gateway
if ($LASTEXITCODE -ne 0) {
    Write-Err "Build failed (exit code $LASTEXITCODE). Is another gateway process still running?"
}
Write-Info "Build complete"

# ─── Start gateway ───────────────────────────────────────────────────────
Write-Step "Starting ControlPlane gateway (provider: ollama, model: $OLLAMA_MODEL)"

$env:DATABASE_URL = $DB_URL
$env:EVENT_BUS = "inproc"
$env:UPSTREAM_PROVIDER = "ollama"
$env:UPSTREAM_MODEL = $OLLAMA_MODEL
$env:UPSTREAM_BASE_URL = $OLLAMA_BASE_URL
$env:DASHBOARD_API_PORT = "8080"
$env:RUST_LOG = "info,controlplane=debug"

$gatewayBin = Join-Path $PWD "target\debug\controlplane-gateway.exe"
$gatewayLog = Join-Path $PWD "gateway.log"

# Use Start-Process for a direct process handle (more reliable than Start-Job)
$gatewayProc = Start-Process -FilePath $gatewayBin -NoNewWindow -PassThru `
    -RedirectStandardError $gatewayLog -RedirectStandardOutput "$gatewayLog.stdout"

# Save PID for cleanup
$gatewayProc.Id | Set-Content (Join-Path $PWD ".gateway.pid")
Write-Host "  Gateway PID: $($gatewayProc.Id)" -ForegroundColor DarkGray

Write-Step "Waiting for gateway..."
$ready = $false
for ($i = 1; $i -le 45; $i++) {
    # Check if process died
    if ($gatewayProc.HasExited) {
        Write-Warn "Gateway process exited with code $($gatewayProc.ExitCode)"
        if (Test-Path $gatewayLog) { Get-Content $gatewayLog | Select-Object -Last 15 | Write-Host }
        Write-Err "Gateway crashed on startup"
    }
    try {
        Invoke-RestMethod "http://127.0.0.1:8080/health" -TimeoutSec 2 | Out-Null
        Write-Info "Gateway is healthy (took ${i}s)"
        $ready = $true
        break
    } catch {
        if ($i % 10 -eq 0) { Write-Host "  ... still waiting (${i}s)" -ForegroundColor DarkGray }
        Start-Sleep -Seconds 1
    }
}
if (-not $ready) {
    Write-Warn "Gateway health check timed out after 45s. Last log lines:"
    if (Test-Path $gatewayLog) { Get-Content $gatewayLog | Select-Object -Last 15 | Write-Host }
    Write-Err "Gateway failed to start after 45s"
}

# ─── Start frontend ──────────────────────────────────────────────────────
Write-Step "Starting frontend dashboard"

$env:NEXT_PUBLIC_API_URL = "http://localhost:8080"
$frontendJob = Start-Job -ScriptBlock {
    Set-Location "$using:PWD\frontend"
    $env:NEXT_PUBLIC_API_URL = "http://localhost:8080"
    pnpm dev 2>&1
}

Write-Step "Waiting for frontend..."
for ($i = 1; $i -le 20; $i++) {
    try {
        Invoke-WebRequest "http://127.0.0.1:3000" -TimeoutSec 2 | Out-Null
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
        if ($gatewayProc.HasExited) { Write-Warn "Gateway process died (exit code: $($gatewayProc.ExitCode))"; break }
    }
} finally {
    Write-Step "Shutting down..."
    # Stop gateway process
    if (-not $gatewayProc.HasExited) {
        Stop-Process -Id $gatewayProc.Id -Force -ErrorAction SilentlyContinue
    }
    # Stop frontend job
    Stop-Job $frontendJob -ErrorAction SilentlyContinue
    Remove-Job $frontendJob -Force -ErrorAction SilentlyContinue
    # Cleanup PID file
    Remove-Item (Join-Path $PWD ".gateway.pid") -Force -ErrorAction SilentlyContinue
    Remove-Item (Join-Path $PWD "gateway.log") -Force -ErrorAction SilentlyContinue
    Remove-Item (Join-Path $PWD "gateway.log.stdout") -Force -ErrorAction SilentlyContinue
    Write-Info "All services stopped."
}
