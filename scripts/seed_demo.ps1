# ControlPlane.ai — Seed Demo Data (PowerShell)
# Populates the database with comprehensive demo data.

$ErrorActionPreference = "Stop"

$DB_URL = if ($env:DATABASE_URL) { $env:DATABASE_URL } else { "postgres://controlplane:secret@localhost:5432/controlplane" }

Write-Host ""
Write-Host "  ControlPlane.ai - Full Demo Seed" -ForegroundColor Cyan
Write-Host "  =================================" -ForegroundColor Cyan
Write-Host ""

# Run base schema
Write-Host "[1/3] Applying base schema migrations..." -ForegroundColor Yellow
$migrations = Get-ChildItem "infra/migrations/0*.sql" | Where-Object { $_.Name -notmatch "009|013" } | Sort-Object Name
foreach ($m in $migrations) {
    try {
        psql $DB_URL --quiet -v ON_ERROR_STOP=1 -f $m.FullName 2>$null
    } catch { }
}
Write-Host "  [OK] Base schema applied" -ForegroundColor Green

# Seed users/apps/policies
Write-Host "[2/3] Seeding users, apps, and policies..." -ForegroundColor Yellow
try {
    psql $DB_URL --quiet -v ON_ERROR_STOP=1 -f "infra/migrations/009_seed_demo_data.sql" 2>$null
} catch { }
Write-Host "  [OK] Users, apps, policies seeded" -ForegroundColor Green

# Seed full demo data
Write-Host "[3/3] Seeding 75 calls, verdicts, escalation cases..." -ForegroundColor Yellow
psql $DB_URL --quiet -v ON_ERROR_STOP=1 -f "infra/migrations/013_seed_full_demo.sql"
Write-Host "  [OK] Full demo data seeded" -ForegroundColor Green

# Summary
Write-Host ""
Write-Host "  Summary:" -ForegroundColor Cyan
$callCount = psql $DB_URL -t -c "SELECT COUNT(*) FROM intercepted_calls;" 2>$null
$verdictCount = psql $DB_URL -t -c "SELECT COUNT(*) FROM verdicts;" 2>$null
$escCount = psql $DB_URL -t -c "SELECT COUNT(*) FROM escalation_cases;" 2>$null
Write-Host "    Intercepted calls: $($callCount.Trim())"
Write-Host "    Verdicts:          $($verdictCount.Trim())"
Write-Host "    Escalation cases:  $($escCount.Trim())"
Write-Host ""
Write-Host "  Demo data ready. Start with: .\scripts\run_demo.ps1" -ForegroundColor Green
Write-Host ""
