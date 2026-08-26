# Demo Runbook

Step-by-step guide for demonstrating ControlPlane.ai live.

---

## Pre-demo checklist

- [ ] Run `./scripts/bootstrap.sh` (or `.ps1` on Windows) — builds everything
- [ ] Verify `.env` has `UPSTREAM_BASE_URL` pointing to a working AI API
- [ ] Have a fallback recorded video ready (in case of API/network issues)
- [ ] Open the dashboard in a browser tab before starting the live demo

## Starting the demo

```bash
# One command:
./scripts/run_demo.sh

# Or manually:
docker compose -f infra/docker-compose.yml up -d
cargo run -p controlplane-gateway &
cd frontend && pnpm dev &
```

Verify:
- Proxy on http://localhost:8900
- Dashboard on http://localhost:3000
- API on http://localhost:8080

## Automated demo exercise

For a guided, scripted walk-through of all scenarios:

```bash
./scripts/demo_exercise.sh     # Linux/macOS
.\scripts\demo_exercise.ps1    # Windows
```

This sends real requests and pauses between scenarios for narration.

## Demo flow (5 minutes)

### Act 1: Show the dashboard (30s)

1. Open http://localhost:3000
2. Point out: overview stats, verdict distribution, live stream connected
3. "This is the control plane — it sees every AI call in real time."

### Act 2: Normal request — pass through (30s)

1. Send a normal, benign request through the proxy:
   ```bash
   curl -X POST http://localhost:8900/v1/messages \
     -H "Content-Type: application/json" \
     -H "x-api-key: YOUR_API_KEY" \
     -H "anthropic-version: 2023-06-01" \
     -d '{"model":"qwen2.5:1.5b","max_tokens":256,"messages":[{"role":"user","content":"What is 2+2?"}]}'
   ```
2. Show the dashboard — a green "pass" verdict appears in the live stream.
3. "Normal traffic passes through with under 10ms added latency."

### Act 3: Secret detection — auto-edit (60s)

1. Send a request designed to trigger a planted secret in the response.
   (Use a prompt that makes the model output something resembling an API key.)
2. Show: the response arrives at the client with the secret **redacted**.
3. Switch to dashboard: a yellow "edit" verdict is visible.
4. Click it: "Detected potential AWS access key — redacted before delivery."
5. "Fast-path catch. Under 10ms. The client never saw the secret."

### Act 4: Cost cap — block (60s)

1. Send a request that would produce a very long response (or one that
   exceeds the configured per-request token cap).
2. Show: the response is **blocked** — the client receives an error.
3. Dashboard: a red "block" verdict with reason "Token budget exceeded."
4. "The cost axis caught a runaway response before it reached the user."

### Act 5: Shadow-path — escalation (60s)

1. Send a request where the model's response contains subtle bias
   (e.g., gendered hiring recommendation).
2. Show: the response **is delivered** (shadow path doesn't block).
3. Within 2 seconds, dashboard shows an orange "escalate" verdict.
4. Navigate to Escalations tab: the case appears with confidence score.
5. "The shadow path caught something the fast path couldn't — but it
   escalated rather than blocking, because the confidence wasn't high enough.
   Only block what you can detect fast and with high confidence."

### Act 6: Audit trail (30s)

1. Navigate to Audit tab.
2. Show the hash-chained records from the actions above.
3. Click "Verify Chain Integrity" — shows the chain is intact.
4. "Every decision is recorded in a tamper-evident audit log.
   If anyone modifies a record after the fact, the chain breaks."

### Act 7: Policy-wise stats (30s)

1. Navigate to Policies tab.
2. Scroll to "Policy Effectiveness" section — shows per-check blocked/escalated/edited/passed counts with FP rate.
3. "This tells you exactly which policies are working and which are generating false positives."

### Act 8: Reviewer override → RAG learning (60s)

1. Navigate to Escalations tab.
2. Open an escalated case — show the "Similar Past Cases" section with precedents.
3. Click "Override" with a reason (e.g., "Stats were reliable in this context").
4. Show the confirmation toast: "Case resolved — your decision has been recorded as a precedent."
5. Send a **similar** request through the proxy.
6. Show the verdict reason now includes: `[Learned] ⚠ 82%-similar past case was overridden by a reviewer`.
7. "The system learned from your correction. Next time, it considers your judgment."

### Act 9: Regulatory profile (30s)

1. Navigate to Policies tab.
2. Click "EU-Financial" regulatory profile.
3. Show that thresholds updated to stricter values.
4. "One click applies a regulatory profile — no manual threshold tuning."

### Act 10: Audit trail (30s)

1. Navigate to Audit tab.
2. Show the hash-chained records from all the actions above.
3. Click "Verify Chain Integrity" — shows the chain is intact.
4. "Every decision is recorded in a tamper-evident audit log.
   If anyone modifies a record after the fact, the chain breaks."

### Act 11: Policy configuration (30s)

1. Navigate to Policies tab.
2. Change a threshold (e.g., lower the bias escalation threshold).
3. "Policy changes take effect within 30 seconds — no code deploy needed.
   The compliance team can own these thresholds directly."

## Closing statement (30s)

"ControlPlane.ai gives you four things:
1. Real-time governance — not forensic log review.
2. The honest engineering trade-off — block what you can catch fast,
   escalate what you can't, and get better over time.
3. A learning loop — human corrections become precedents that
   improve future decisions.
4. An audit trail that proves due diligence — not a dashboard that
   explains what already went wrong."

## Troubleshooting

| Issue | Fix |
|---|---|
| Proxy returns 502 | Check `UPSTREAM_BASE_URL` in .env, verify API key |
| Dashboard blank | Verify frontend is running on :3000, check browser console |
| No verdicts appearing | Check gateway logs for errors, verify event bus mode |
| Docker not starting | Run `docker compose -f infra/docker-compose.yml logs` |
| Migrations fail | Ensure PostgreSQL container is healthy: `docker exec controlplane-postgres pg_isready` |

## Fallback: recorded demo

If live APIs are unavailable, use the pre-recorded demo video.
The recording follows the exact same flow above and was made against
a working deployment.
