$ErrorActionPreference = "Stop"

$DB_URL = if ($env:DATABASE_URL) { $env:DATABASE_URL } else { "postgres://controlplane:secret@localhost:5432/controlplane" }

Write-Host ""
Write-Host "  ControlPlane.ai - Demo Showcase (DB Seed)" -ForegroundColor Cyan
Write-Host "  Seeds 100 diverse verdicts for dashboard showcase" -ForegroundColor DarkGray
Write-Host "  =============================================" -ForegroundColor Cyan
Write-Host ""

# Check psql
if (-not (Get-Command psql -ErrorAction SilentlyContinue)) {
    Write-Host "  [X] psql not found. Install PostgreSQL." -ForegroundColor Red
    exit 1
}

# Check DB connection
$testConn = psql $DB_URL -c "SELECT 1" 2>$null
if (-not $testConn) {
    Write-Host "  [X] Cannot connect to database. Is PostgreSQL running?" -ForegroundColor Red
    exit 1
}
Write-Host "  [OK] Connected to database" -ForegroundColor Green

# Get app ID
$APP_ID = (psql $DB_URL -t -A -c "SELECT id FROM apps ORDER BY name LIMIT 1;" 2>$null).Trim()
if (-not $APP_ID) {
    Write-Host "  [!] No apps found. Creating default..." -ForegroundColor Yellow
    psql $DB_URL --quiet -c "INSERT INTO apps (id, name, created_at) VALUES ('10000000-0000-0000-0000-000000000001', 'chatbot-prod', NOW()) ON CONFLICT DO NOTHING;" 2>$null
    $APP_ID = "10000000-0000-0000-0000-000000000001"
}
Write-Host "  [OK] Using app: $APP_ID" -ForegroundColor Green
Write-Host ""

# Step 1: Create intercepted calls
Write-Host "  --- Step 1: Creating intercepted calls ---" -ForegroundColor White
psql $DB_URL --quiet -c @"
INSERT INTO intercepted_calls (id, correlation_id, app_id, model, token_count_input, token_count_output, created_at)
SELECT
    gen_random_uuid(),
    gen_random_uuid(),
    '${APP_ID}'::uuid,
    'qwen2.5:1.5b',
    (random() * 500 + 50)::int,
    (random() * 1000 + 100)::int,
    NOW() - (s || ' hours')::interval
FROM generate_series(1, 100) AS s
ON CONFLICT DO NOTHING;
"@ 2>$null
Write-Host "  [OK] 100 intercepted calls created" -ForegroundColor Green

# Step 2: Seed PASS verdicts (40)
Write-Host "  --- Step 2: Seeding PASS verdicts (40) ---" -ForegroundColor White
$passSql = @"
DO `$`$
DECLARE
    i INT := 0;
    pass_reasons TEXT[] := ARRAY[
        'All checks passed','No secrets or PII detected','Token count within budget',
        'Response grounded in provided context','No harmful content detected',
        'Cost within threshold','Response accurate and helpful',
        'Content appropriate for target audience','Response length proportional to query',
        'Content within safety guidelines'
    ];
    pass_checks TEXT[] := ARRAY[
        'fast-path-summary','cost_cap','groundedness','verbosity_check',
        'token_budget','secret_detection'
    ];
    pass_axes TEXT[] := ARRAY['responsibility','cost','performance'];
    call_rec RECORD;
BEGIN
    FOR call_rec IN
        SELECT id, app_id FROM intercepted_calls
        WHERE model='qwen2.5:1.5b' ORDER BY created_at DESC LIMIT 40
    LOOP
        i := i + 1;
        INSERT INTO verdicts (id, call_id, app_id, axis, path, outcome, confidence, reason, check_name, created_at)
        VALUES (
            gen_random_uuid(), call_rec.id, call_rec.app_id,
            pass_axes[1+(i%3)], 'fast', 'pass',
            0.85+(random()*0.15),
            pass_reasons[1+(i%10)],
            pass_checks[1+(i%6)],
            NOW() - make_interval(hours => (40-i)*4)
        );
    END LOOP;
END `$`$;
"@
psql $DB_URL --quiet -c $passSql 2>$null
Write-Host "  [OK] 40 PASS verdicts seeded" -ForegroundColor Green

# Step 3: Seed EDIT verdicts (25)
Write-Host "  --- Step 3: Seeding EDIT verdicts (25) ---" -ForegroundColor White
$editSql = @"
DO `$`$
DECLARE
    i INT := 0;
    edit_reasons TEXT[] := ARRAY[
        'AWS access key detected and redacted',
        'API token found in response - auto-redacted',
        'Email address detected in output - masked',
        'Credit card number found - redacted',
        'Private IP address detected - redacted',
        'Database connection string exposed - redacted',
        'JWT token leaked in response - redacted',
        'SSH private key detected - redacted',
        'Slack webhook URL found - redacted',
        'Personal phone number detected - masked',
        'OAuth client secret found - redacted',
        'GCP service account key detected - redacted',
        'Azure storage key found - redacted',
        'Redis connection string exposed - redacted',
        'PostgreSQL credentials detected - redacted'
    ];
    call_rec RECORD;
BEGIN
    FOR call_rec IN
        SELECT id, app_id FROM intercepted_calls
        WHERE model='qwen2.5:1.5b' ORDER BY created_at DESC OFFSET 40 LIMIT 25
    LOOP
        i := i + 1;
        INSERT INTO verdicts (id, call_id, app_id, axis, path, outcome, confidence, reason, check_name, created_at)
        VALUES (
            gen_random_uuid(), call_rec.id, call_rec.app_id,
            'responsibility', 'fast', 'edit',
            0.90+(random()*0.10),
            edit_reasons[1+(i%15)],
            'secret_detection',
            NOW() - make_interval(hours => (25-i)*4)
        );
    END LOOP;
END `$`$;
"@
psql $DB_URL --quiet -c $editSql 2>$null
Write-Host "  [OK] 25 EDIT verdicts seeded" -ForegroundColor Yellow

# Step 4: Seed BLOCK verdicts (20)
Write-Host "  --- Step 4: Seeding BLOCK verdicts (20) ---" -ForegroundColor White
$blockSql = @"
DO `$`$
DECLARE
    i INT := 0;
    block_reasons TEXT[] := ARRAY[
        'Unsafe content: instructions for harmful activity',
        'Unsafe content: explicit instructions to bypass security controls',
        'Unsafe content: weapons manufacturing instructions',
        'Unsafe content: malware development code detected',
        'Unsafe content: self-harm instructions',
        'Unsafe content: phishing attack template detected',
        'Unsafe content: network exploitation tools',
        'Unsafe content: credential harvesting instructions',
        'Unsafe content: data exfiltration methods',
        'Unsafe content: social engineering attack script',
        'Unsafe content: ransomware development guide',
        'Unsafe content: SQL injection tutorial',
        'Unsafe content: DDoS attack methodology',
        'Unsafe content: keylogger development',
        'Unsafe content: botnet creation guide'
    ];
    call_rec RECORD;
BEGIN
    FOR call_rec IN
        SELECT id, app_id FROM intercepted_calls
        WHERE model='qwen2.5:1.5b' ORDER BY created_at DESC OFFSET 65 LIMIT 20
    LOOP
        i := i + 1;
        INSERT INTO verdicts (id, call_id, app_id, axis, path, outcome, confidence, reason, check_name, created_at)
        VALUES (
            gen_random_uuid(), call_rec.id, call_rec.app_id,
            'responsibility', 'fast', 'block',
            0.93+(random()*0.07),
            block_reasons[1+(i%15)],
            'unsafe_content',
            NOW() - make_interval(hours => (20-i)*4)
        );
    END LOOP;
END `$`$;
"@
psql $DB_URL --quiet -c $blockSql 2>$null
Write-Host "  [OK] 20 BLOCK verdicts seeded" -ForegroundColor Red

# Step 5: Seed ESCALATE verdicts (15)
Write-Host "  --- Step 5: Seeding ESCALATE verdicts (15) ---" -ForegroundColor White
$escSql = @"
DO `$`$
DECLARE
    i INT := 0;
    esc_reasons TEXT[] := ARRAY[
        'Bias detected in categories: gender, race',
        'Response groundedness score below threshold',
        'Excessive verbosity detected in response',
        'Potential stereotype in hiring recommendation',
        'Response contains demographic generalizations',
        'Low confidence: ambiguous safety classification',
        'PII re-identification risk in output',
        'Bias detected in categories: age, disability',
        'Response tone flagged as potentially discriminatory',
        'Groundedness: claims not supported by provided context',
        'Prompt injection attempt detected',
        'Potential jailbreak attempt in prompt',
        'Hallucination detected: unsupported factual claims',
        'Toxic language detected in response',
        'Bias detected in categories: religion, nationality'
    ];
    esc_checks TEXT[] := ARRAY[
        'bias_classification','groundedness_check','verbosity_analysis',
        'semantic_pii_risk','input_bias','prompt_injection',
        'llm-guard-toxicity','deepeval-hallucination'
    ];
    call_rec RECORD;
BEGIN
    FOR call_rec IN
        SELECT id, app_id FROM intercepted_calls
        WHERE model='qwen2.5:1.5b' ORDER BY created_at DESC OFFSET 85 LIMIT 15
    LOOP
        i := i + 1;
        INSERT INTO verdicts (id, call_id, app_id, axis, path, outcome, confidence, reason, check_name, created_at)
        VALUES (
            gen_random_uuid(), call_rec.id, call_rec.app_id,
            CASE WHEN i%2=0 THEN 'performance' ELSE 'responsibility' END,
            'shadow', 'escalate',
            0.55+(random()*0.30),
            esc_reasons[1+(i%15)],
            esc_checks[1+(i%8)],
            NOW() - make_interval(hours => (15-i)*4)
        );
    END LOOP;
END `$`$;
"@
psql $DB_URL --quiet -c $escSql 2>$null
Write-Host "  [OK] 15 ESCALATE verdicts seeded" -ForegroundColor Cyan

# Step 5b: Seed escalation cases
Write-Host "  --- Step 5b: Seeding escalation cases ---" -ForegroundColor White
$escCasesSql = @"
DO `$`$
DECLARE
    r RECORD;
    rnd FLOAT;
    res_status TEXT;
    res_resolution TEXT;
    res_resolved_at TIMESTAMPTZ;
    resolutions TEXT[] := ARRAY['confirm','override','dismiss'];
BEGIN
    FOR r IN
        SELECT v.id, v.call_id, v.app_id, v.axis, v.confidence, v.reason, v.created_at
        FROM verdicts v
        WHERE v.outcome = 'escalate'
          AND NOT EXISTS (SELECT 1 FROM escalation_cases ec WHERE ec.verdict_id = v.id)
    LOOP
        rnd := random();
        IF rnd < 0.15 THEN
            res_status := 'resolved';
            res_resolution := resolutions[1 + (floor(random()*3))::int];
            res_resolved_at := r.created_at + INTERVAL '1 hour';
        ELSIF rnd < 0.45 THEN
            res_status := 'in_review';
            res_resolution := NULL;
            res_resolved_at := NULL;
        ELSE
            res_status := 'open';
            res_resolution := NULL;
            res_resolved_at := NULL;
        END IF;
        INSERT INTO escalation_cases (id, verdict_id, call_id, app_id, status, axis, confidence, reason, resolution, resolved_at, created_at)
        VALUES (gen_random_uuid(), r.id, r.call_id, r.app_id, res_status, r.axis, r.confidence, r.reason, res_resolution, res_resolved_at, r.created_at);
    END LOOP;
END `$`$;
"@
psql $DB_URL --quiet -c $escCasesSql 2>$null
Write-Host "  [OK] Escalation cases seeded" -ForegroundColor Green

# Step 6: Seed audit records
Write-Host "  --- Step 6: Seeding audit records ---" -ForegroundColor White
$auditSql = @"
DO `$`$
DECLARE
    r RECORD;
    prev_hash_val TEXT := '0000000000000000000000000000000000000000000000000000000000000000';
    curr_hash TEXT;
BEGIN
    FOR r IN
        SELECT v.call_id, v.id AS verdict_id, v.outcome, v.axis, v.check_name, v.created_at
        FROM verdicts v
        WHERE v.call_id IN (SELECT id FROM intercepted_calls WHERE model = 'qwen2.5:1.5b')
        ORDER BY v.created_at ASC
    LOOP
        curr_hash := encode(sha256(
            (prev_hash_val || r.call_id::text || r.verdict_id::text || r.outcome || extract(epoch from r.created_at)::text)::bytea
        ), 'hex');
        INSERT INTO audit_records (id, call_id, verdict_id, action_taken, app_id, prev_hash, record_hash, metadata, created_at)
        VALUES (
            gen_random_uuid(), r.call_id, r.verdict_id, r.outcome,
            (SELECT id FROM apps LIMIT 1),
            prev_hash_val, curr_hash,
            jsonb_build_object('axis', r.axis, 'check_name', r.check_name),
            r.created_at
        );
        prev_hash_val := curr_hash;
    END LOOP;
END `$`$;
"@
psql $DB_URL --quiet -c $auditSql 2>$null
Write-Host "  [OK] Audit records seeded" -ForegroundColor Green

# Summary
Write-Host ""
Write-Host "  --- Final Counts ---" -ForegroundColor White
psql $DB_URL -c "SELECT outcome, COUNT(*) as count FROM verdicts WHERE call_id IN (SELECT id FROM intercepted_calls WHERE model='qwen2.5:1.5b') GROUP BY outcome ORDER BY outcome;" 2>$null

$totalCalls = (psql $DB_URL -t -A -c "SELECT COUNT(*) FROM intercepted_calls WHERE model='qwen2.5:1.5b';" 2>$null).Trim()
$totalVerdicts = (psql $DB_URL -t -A -c "SELECT COUNT(*) FROM verdicts WHERE call_id IN (SELECT id FROM intercepted_calls WHERE model='qwen2.5:1.5b');" 2>$null).Trim()
$totalAudit = (psql $DB_URL -t -A -c "SELECT COUNT(*) FROM audit_records;" 2>$null).Trim()

Write-Host ""
Write-Host "  Demo seeded successfully!" -ForegroundColor Cyan
Write-Host "  ========================================" -ForegroundColor Cyan
Write-Host "  $totalCalls intercepted calls"
Write-Host "  $totalVerdicts verdicts (40 pass, 25 edit, 20 block, 15 escalate)"
Write-Host "  $totalAudit audit records"
Write-Host ""
Write-Host "  Open http://localhost:3000 to see the dashboard" -ForegroundColor Green
Write-Host ""
