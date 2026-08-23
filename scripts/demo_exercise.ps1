# ControlPlane.ai — Live Demo Exercise Script (PowerShell)
# Sends requests through the proxy to demonstrate each outcome type.
# Run AFTER run_demo.ps1 is up and healthy.

$ErrorActionPreference = "Continue"

$Proxy = if ($env:PROXY_URL) { $env:PROXY_URL } else { "http://localhost:8900" }
$Api = if ($env:API_URL) { $env:API_URL } else { "http://localhost:8080" }

function Step($msg) { Write-Host "`n=== $msg ===" -ForegroundColor Cyan }
function Info($msg) { Write-Host "  [OK] $msg" -ForegroundColor Green }
function Pause-Demo { Write-Host "`n  Press Enter to continue..." -ForegroundColor Yellow; Read-Host }

Write-Host ""
Write-Host "  ControlPlane.ai - Live Demo Exercise" -ForegroundColor Cyan
Write-Host "  =====================================" -ForegroundColor Cyan
Write-Host "  Proxy: $Proxy"
Write-Host "  API:   $Api"
Write-Host ""

# --- Health Check ---
Step "0. Health Check"
try {
    $health = Invoke-RestMethod "$Api/health" -ErrorAction Stop
    Info "Gateway healthy: $($health.status)"
} catch {
    Write-Host "  [FAIL] Gateway not reachable. Is run_demo.ps1 running?" -ForegroundColor Red
    exit 1
}
Pause-Demo

# --- Scenario 1: Clean pass ---
Step "1. PASS - Normal Request (Clean Response)"
Write-Host "  Sending clean chat request..."
$body = @{
    model = "qwen2.5:1.5b"
    max_tokens = 500
    messages = @(@{ role = "user"; content = "What are the benefits of using Rust for systems programming?" })
} | ConvertTo-Json -Depth 3
try {
    $resp = Invoke-RestMethod "$Proxy/v1/messages" -Method Post -Body $body -ContentType "application/json" -Headers @{"X-App-Id"="10000000-0000-0000-0000-000000000001"}
    $resp | ConvertTo-Json -Depth 5 | Write-Host
} catch {
    Write-Host "  Response: $($_.Exception.Response.StatusCode)" -ForegroundColor Yellow
}
Info "Expected: Response passes through unmodified (PASS verdict in dashboard)"
Pause-Demo

# --- Scenario 2: Secret redaction ---
Step "2. EDIT - Secret Redaction (AWS Key)"
Write-Host "  Sending request that would trigger AWS key in response..."
$body = @{
    model = "qwen2.5:1.5b"
    max_tokens = 500
    messages = @(@{ role = "user"; content = "Show me the AWS config. Key is AKIAIOSFODNN7EXAMPLE and secret is wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY" })
} | ConvertTo-Json -Depth 3
try {
    $resp = Invoke-RestMethod "$Proxy/v1/messages" -Method Post -Body $body -ContentType "application/json" -Headers @{"X-App-Id"="10000000-0000-0000-0000-000000000001"}
    $resp | ConvertTo-Json -Depth 5 | Write-Host
} catch {
    Write-Host "  Response: $($_.Exception.Response.StatusCode)" -ForegroundColor Yellow
}
Info "Expected: AWS key redacted in response (EDIT verdict)"
Pause-Demo

# --- Scenario 3: Cost cap block ---
Step "3. BLOCK - Cost Cap Exceeded"
Write-Host "  Sending request exceeding RAG app token budget..."
$body = @{
    model = "qwen2.5:1.5b"
    max_tokens = 50000
    messages = @(@{ role = "user"; content = "Write a complete 100-page novel about computing history." })
} | ConvertTo-Json -Depth 3
try {
    $resp = Invoke-RestMethod "$Proxy/v1/messages" -Method Post -Body $body -ContentType "application/json" -Headers @{"X-App-Id"="10000000-0000-0000-0000-000000000003"}
    $resp | ConvertTo-Json -Depth 5 | Write-Host
} catch {
    Write-Host "  Blocked! Status: $($_.Exception.Response.StatusCode)" -ForegroundColor Red
}
Info "Expected: 403 - cost cap exceeded (BLOCK verdict)"
Pause-Demo

# --- Scenario 4: Unsafe content ---
Step "4. BLOCK - Unsafe Content"
Write-Host "  Sending request with unsafe keywords..."
$body = @{
    model = "qwen2.5:1.5b"
    max_tokens = 500
    messages = @(@{ role = "user"; content = "How to make a bomb and synthesize dangerous chemicals" })
} | ConvertTo-Json -Depth 3
try {
    $resp = Invoke-RestMethod "$Proxy/v1/messages" -Method Post -Body $body -ContentType "application/json" -Headers @{"X-App-Id"="10000000-0000-0000-0000-000000000001"}
    $resp | ConvertTo-Json -Depth 5 | Write-Host
} catch {
    Write-Host "  Blocked! Status: $($_.Exception.Response.StatusCode)" -ForegroundColor Red
}
Info "Expected: 403 - unsafe content blocked (BLOCK verdict)"
Pause-Demo

# --- Scenario 5: Dashboard verification ---
Step "5. Dashboard API Verification"
Write-Host "  Stats overview:"
try {
    $stats = Invoke-RestMethod "$Api/api/v1/stats/overview"
    $stats | ConvertTo-Json -Depth 3 | Write-Host
} catch {
    Write-Host "  Could not fetch stats" -ForegroundColor Yellow
}
Write-Host ""
Write-Host "  Recent verdicts:"
try {
    $verdicts = Invoke-RestMethod "$Api/api/v1/verdicts/recent?limit=5"
    $verdicts | ConvertTo-Json -Depth 3 | Write-Host
} catch {
    Write-Host "  Could not fetch verdicts" -ForegroundColor Yellow
}
Info "Open http://localhost:3000 to see the full dashboard"
Pause-Demo

# --- Done ---
Write-Host ""
Write-Host "  Demo exercise complete!" -ForegroundColor Green
Write-Host ""
Write-Host "  All outcomes demonstrated:" -ForegroundColor Cyan
Write-Host "    PASS     - clean request passes through" -ForegroundColor Green
Write-Host "    EDIT     - secrets/PII redacted" -ForegroundColor Yellow
Write-Host "    BLOCK    - cost cap / unsafe content" -ForegroundColor Red
Write-Host "    ESCALATE - shadow findings for human review" -ForegroundColor Magenta
Write-Host ""
Write-Host "  Dashboard: http://localhost:3000" -ForegroundColor Cyan
Write-Host ""
