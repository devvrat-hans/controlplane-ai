# ControlPlane.ai — Run Demo (Windows PowerShell)
# Starts all services needed for a live demonstration.

$ErrorActionPreference = "Stop"

function Write-Step($msg) { Write-Host "`n→ $msg" -ForegroundColor Cyan }
function Write-Info($msg) { Write-Host "[✓] $msg" -ForegroundColor Green }

Write-Host "╔══════════════════════════════════════╗" -ForegroundColor White
Write-Host "║    ControlPlane.ai — Demo Runner     ║" -ForegroundColor White
Write-Host "╚══════════════════════════════════════╝" -ForegroundColor White

# --- Bootstrap if needed ---

if (-not (Test-Path target/debug/controlplane-gateway.exe)) {
    Write-Step "First run detected — running bootstrap"
    & "$PSScriptRoot\bootstrap.ps1"
}

# --- Infrastructure ---

$hasDocker = Get-Command docker -ErrorAction SilentlyContinue
if ($hasDocker) {
    Write-Step "Ensuring infrastructure is running"
    docker compose -f infra/docker-compose.yml up -d
    Start-Sleep -Seconds 2
    Write-Info "Infrastructure ready"
}

# --- Start gateway ---

Write-Step "Starting ControlPlane gateway"
$gatewayJob = Start-Job -ScriptBlock {
    Set-Location $using:PWD
    cargo run -p controlplane-gateway
}
Write-Info "Gateway starting (Job: $($gatewayJob.Id))"

Start-Sleep -Seconds 3

# --- Start frontend ---

Write-Step "Starting frontend dashboard"
$frontendJob = Start-Job -ScriptBlock {
    Set-Location "$using:PWD\frontend"
    pnpm dev
}
Write-Info "Frontend starting (Job: $($frontendJob.Id))"

Start-Sleep -Seconds 3

# --- Summary ---

Write-Host ""
Write-Host "╔══════════════════════════════════════════════════╗" -ForegroundColor White
Write-Host "║    Demo is running!                              ║" -ForegroundColor White
Write-Host "╠══════════════════════════════════════════════════╣" -ForegroundColor White
Write-Host "║  Proxy:      http://localhost:8900               ║" -ForegroundColor White
Write-Host "║  API:        http://localhost:8080               ║" -ForegroundColor White
Write-Host "║  Dashboard:  http://localhost:3000               ║" -ForegroundColor White
Write-Host "╠══════════════════════════════════════════════════╣" -ForegroundColor White
Write-Host "║  Demo accounts:                                  ║" -ForegroundColor White
Write-Host "║    admin@controlplane.test / Demo#Admin2026      ║" -ForegroundColor White
Write-Host "║    reviewer@controlplane.test / Demo#Reviewer2026║" -ForegroundColor White
Write-Host "║    viewer@controlplane.test / Demo#Viewer2026    ║" -ForegroundColor White
Write-Host "╚══════════════════════════════════════════════════╝" -ForegroundColor White
Write-Host ""
Write-Host "Press Ctrl+C to stop, or run: Stop-Job $($gatewayJob.Id), $($frontendJob.Id)"

try {
    Wait-Job $gatewayJob, $frontendJob
} finally {
    Write-Step "Shutting down..."
    Stop-Job $gatewayJob -ErrorAction SilentlyContinue
    Stop-Job $frontendJob -ErrorAction SilentlyContinue
    Remove-Job $gatewayJob, $frontendJob -Force -ErrorAction SilentlyContinue
    Write-Info "All services stopped."
}
