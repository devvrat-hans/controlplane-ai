#!/usr/bin/env bash
# ControlPlane.ai — cross-domain benchmark orchestration.
#
# Brings up the gateway (optionally with the Laya judge), replays the synthetic
# cross-domain dataset through the proxy, then joins the results to the verdict
# store and prints the confusion matrices.
#
# Everything must happen inside one invocation: the stack has to stay alive while
# traffic is replayed, and background processes do not survive between calls here.
#
# Usage:
#   ./benchmarks/cross_domain/run.sh                    # judge off
#   JUDGE_MODE=laya ./benchmarks/cross_domain/run.sh    # judge on
#
# Prerequisites: local PostgreSQL with the corpus, Ollama + the guardrails sidecar
# reachable, target/debug/controlplane-gateway built, and (for judge runs) a
# running laya-serve on :8000 with the model cached. See the report's Appendix.

set -uo pipefail
cd "$(dirname "$0")/../.."

DB_URL="${DATABASE_URL:-postgres://controlplane:secret@localhost:5432/controlplane}"
PROXY_PORT="${PROXY_PORT:-8901}"
API_PORT="${API_PORT:-8081}"
LAYA_PORT="${LAYA_PORT:-8000}"
JUDGE_MODE="${JUDGE_MODE:-off}"
OUT="benchmark-results/cross-domain-${JUDGE_MODE}"
mkdir -p "$OUT"

echo "mode=${JUDGE_MODE}  results=${OUT}"

if [ "$JUDGE_MODE" = "laya" ]; then
  echo "════ launching Laya sidecar (:${LAYA_PORT}) ════"
  nohup env LAYA_DEVICE=cpu LAYA_PRELOAD=1 PYTHONUNBUFFERED=1 TRANSFORMERS_NO_TF=1 USE_TF=0 \
    services/laya/.venv/bin/laya-serve > "$OUT/laya-serve.log" 2>&1 < /dev/null &
  disown
  for i in $(seq 1 40); do nc -z 127.0.0.1 "$LAYA_PORT" 2>/dev/null && break; sleep 3; done
  nc -z 127.0.0.1 "$LAYA_PORT" 2>/dev/null \
    && echo "judge listening" \
    || { echo "!! judge never came up"; tail -20 "$OUT/laya-serve.log"; exit 1; }
fi

echo "════ launching gateway (DECISION_JUDGE=${JUDGE_MODE}) ════"
nohup env \
  DATABASE_URL="$DB_URL" EVENT_BUS=inproc \
  PROXY_LISTEN_ADDR="127.0.0.1:${PROXY_PORT}" DASHBOARD_API_PORT="$API_PORT" \
  UPSTREAM_PROVIDER=ollama UPSTREAM_BASE_URL="${UPSTREAM_BASE_URL:-http://localhost:11434}" \
  UPSTREAM_MODEL="${UPSTREAM_MODEL:-qwen2.5:1.5b}" \
  GUARDRAILS_URL="${GUARDRAILS_URL:-http://localhost:8200}" \
  JWT_SECRET=benchmark-local-only SEED_DEMO_USERS=false \
  DECISION_JUDGE="$JUDGE_MODE" LAYA_URL="http://127.0.0.1:${LAYA_PORT}" LAYA_TIMEOUT_MS=5000 \
  RUST_LOG=controlplane=info \
  target/debug/controlplane-gateway > "$OUT/gateway.log" 2>&1 < /dev/null &
disown

for i in $(seq 1 40); do
  curl -sf -m 2 "http://127.0.0.1:${API_PORT}/api/v1/system/config" >/dev/null 2>&1 && break
  sleep 2
done
curl -sS -m 5 "http://127.0.0.1:${API_PORT}/api/v1/system/config" \
  | grep -o '"decision_judge":"[^"]*"\|"decision_judge_configured":[a-z]*' \
  | tee "$OUT/gateway-judge-config.txt"

python3 - <<'PY' | tee "$OUT/run-window-start.txt"
import subprocess
print(subprocess.run(["psql", "postgres://controlplane:secret@localhost:5432/controlplane",
                      "-t", "-A", "-c", "SELECT now()"], capture_output=True, text=True).stdout.strip())
PY
RUN_TS="$(cat "$OUT/run-window-start.txt")"
echo "run window starts ${RUN_TS}"

echo
echo "════ replaying the cross-domain dataset ════"
python3 benchmarks/cross_domain/harness.py traffic \
  --proxy "http://127.0.0.1:${PROXY_PORT}" --out "$OUT"

echo
echo "waiting for shadow analysis ..."
sleep 30

python3 benchmarks/cross_domain/harness.py enrich --out "$OUT" --db "$DB_URL"
python3 benchmarks/cross_domain/harness.py metrics --out "$OUT"

echo
echo "════ judge health in this run ════"
printf "fail-open windows: %s\n" "$(grep -ciE 'FAIL OPEN' "$OUT/gateway.log")" | tee "$OUT/judge-fail-signals.txt"

echo
echo "════ verdict census for this run window ════"
psql "$DB_URL" -c "SELECT path, outcome, count(*) FROM verdicts
  WHERE created_at >= '${RUN_TS}' GROUP BY 1,2 ORDER BY 1,2;" | tee "$OUT/verdict-census.txt"
psql "$DB_URL" -c "SELECT check_name, count(*) FROM verdicts
  WHERE created_at >= '${RUN_TS}' AND outcome <> 'pass' GROUP BY 1 ORDER BY 2 DESC;" \
  | tee "$OUT/fired-checks.txt"

echo
echo "artifacts: $OUT"
