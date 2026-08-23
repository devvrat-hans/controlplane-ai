#!/usr/bin/env bash
set -euo pipefail

echo "╔══════════════════════════════════════════════════════════╗"
echo "║  Seeding diverse demo verdicts for dashboard showcase   ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""

# Get the first app ID
APP_ID=$(psql -U devvrathans -d controlplane -t -A -c "SELECT id FROM apps ORDER BY name LIMIT 1;")
echo "Using app: $APP_ID"
echo ""

# ─── Create intercepted calls ────────────────────────────────────────────
echo "→ Creating 75 intercepted calls..."
psql -U devvrathans -d controlplane --quiet -c "
INSERT INTO intercepted_calls (id, correlation_id, app_id, model, created_at)
SELECT
    gen_random_uuid(),
    gen_random_uuid(),
    '${APP_ID}'::uuid,
    'qwen2.5:1.5b',
    NOW() - (s || ' hours')::interval
FROM generate_series(1, 75) AS s
ON CONFLICT DO NOTHING;
"

# Get the call IDs we just created
CALL_IDS=$(psql -U devvrathans -d controlplane -t -A -c "
SELECT id::text || '|' || app_id::text || '|' || EXTRACT(EPOCH FROM created_at)::int::text
FROM intercepted_calls
WHERE model = 'qwen2.5:1.5b'
ORDER BY created_at DESC
LIMIT 75;
")

echo "→ Seeding PASS verdicts (30)..."
i=0
echo "$CALL_IDS" | head -30 | while IFS='|' read -r cid aid epoch; do
    i=$((i + 1))
    reason_idx=$(( (i % 10) + 1 ))
    check_idx=$(( (i % 6) + 1 ))
    axis_idx=$(( (i % 3) + 1 ))
    psql -U devvrathans -d controlplane --quiet -c "
    INSERT INTO verdicts (id, call_id, app_id, axis, path, outcome, confidence, reason, check_name, created_at)
    VALUES (
        gen_random_uuid(), '${cid}'::uuid, '${aid}'::uuid,
        (ARRAY['responsibility','cost','performance'])[${axis_idx}],
        'fast', 'pass',
        0.85 + random() * 0.15,
        (ARRAY['All checks passed','No secrets or PII detected','Token count within budget','Response grounded in context','No harmful content detected','Cost within threshold','Response accurate and helpful','Content appropriate for audience','Response length proportional to query','Content within safety guidelines'])[${reason_idx}],
        (ARRAY['fast-path-summary','cost_cap','secret_detection','groundedness','verbosity_check','token_budget'])[${check_idx}],
        to_timestamp(${epoch}) - interval '${i} minutes'
    );"
done
echo "  ✓ 30 PASS"

echo "→ Seeding EDIT verdicts (20)..."
i=0
echo "$CALL_IDS" | sed -n '31,50p' | while IFS='|' read -r cid aid epoch; do
    i=$((i + 1))
    reason_idx=$(( (i % 10) + 1 ))
    psql -U devvrathans -d controlplane --quiet -c "
    INSERT INTO verdicts (id, call_id, app_id, axis, path, outcome, confidence, reason, check_name, created_at)
    VALUES (
        gen_random_uuid(), '${cid}'::uuid, '${aid}'::uuid,
        'responsibility', 'fast', 'edit',
        0.90 + random() * 0.10,
        (ARRAY['AWS access key detected and redacted','API token found — auto-redacted','Email address detected — masked','Credit card number found — redacted','Private IP address detected — redacted','Database connection string exposed — redacted','JWT token leaked — redacted','SSH private key detected — redacted','Slack webhook URL found — redacted','Personal phone number detected — masked'])[${reason_idx}],
        'secret_detection',
        to_timestamp(${epoch}) - interval '${i} minutes'
    );"
done
echo "  ✓ 20 EDIT"

echo "→ Seeding BLOCK verdicts (15)..."
i=0
echo "$CALL_IDS" | sed -n '51,65p' | while IFS='|' read -r cid aid epoch; do
    i=$((i + 1))
    reason_idx=$(( (i % 10) + 1 ))
    psql -U devvrathans -d controlplane --quiet -c "
    INSERT INTO verdicts (id, call_id, app_id, axis, path, outcome, confidence, reason, check_name, created_at)
    VALUES (
        gen_random_uuid(), '${cid}'::uuid, '${aid}'::uuid,
        'responsibility', 'fast', 'block',
        0.93 + random() * 0.07,
        (ARRAY['Unsafe content: harmful activity instructions','Unsafe content: bypass security controls','Unsafe content: weapons manufacturing','Unsafe content: malware development code','Unsafe content: self-harm instructions','Unsafe content: phishing attack template','Unsafe content: network exploitation tools','Unsafe content: credential harvesting','Unsafe content: data exfiltration methods','Unsafe content: social engineering script'])[${reason_idx}],
        'unsafe_content',
        to_timestamp(${epoch}) - interval '${i} minutes'
    );"
done
echo "  ✓ 15 BLOCK"

echo "→ Seeding ESCALATE verdicts (10)..."
i=0
echo "$CALL_IDS" | sed -n '66,75p' | while IFS='|' read -r cid aid epoch; do
    i=$((i + 1))
    reason_idx=$(( (i % 10) + 1 ))
    check_idx=$(( (i % 5) + 1 ))
    if [ $((i % 2)) -eq 0 ]; then axis="performance"; else axis="responsibility"; fi
    psql -U devvrathans -d controlplane --quiet -c "
    INSERT INTO verdicts (id, call_id, app_id, axis, path, outcome, confidence, reason, check_name, created_at)
    VALUES (
        gen_random_uuid(), '${cid}'::uuid, '${aid}'::uuid,
        '${axis}', 'shadow', 'escalate',
        0.55 + random() * 0.30,
        (ARRAY['Bias detected: gender, race','Response groundedness below threshold','Excessive verbosity detected','Potential stereotype in recommendation','Demographic generalization detected','Low confidence: ambiguous safety classification','PII re-identification risk in output','Bias detected: age, disability','Potentially discriminatory tone','Claims not supported by context'])[${reason_idx}],
        (ARRAY['bias_classification','groundedness_check','verbosity_analysis','semantic_pii_risk','input_bias'])[${check_idx}],
        to_timestamp(${epoch}) - interval '${i} minutes'
    );"
done
echo "  ✓ 10 ESCALATE"

# ─── Summary ─────────────────────────────────────────────────────────────
echo ""
echo "→ Final counts:"
psql -U devvrathans -d controlplane -c "
SELECT outcome, COUNT(*) as count
FROM verdicts
WHERE call_id IN (SELECT id FROM intercepted_calls WHERE model='qwen2.5:1.5b')
GROUP BY outcome ORDER BY outcome;
"

TOTAL=$(psql -U devvrathans -d controlplane -t -A -c "SELECT COUNT(*) FROM verdicts;")
echo ""
echo "╔══════════════════════════════════════════════════════════╗"
echo "║  Done! Total verdicts in database: $TOTAL"
echo "║  Check the dashboard at http://localhost:3000"
echo "╚══════════════════════════════════════════════════════════╝"
