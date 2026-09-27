#!/usr/bin/env bash
# ControlPlane.ai — Laya / hybrid-judge benchmark driver.
#
# Reproduces every measurement quoted in docs/analysis/laya-benchmark-report.md.
# It does not change production behaviour: it reads the governance corpus, runs
# the existing test suite, and runs the existing fast-path criterion bench.
#
# Usage:
#   ./scripts/bench_laya.sh                     # full run, timestamped results dir
#   ./scripts/bench_laya.sh --skip-tests        # skip the workspace test suite
#   ./scripts/bench_laya.sh --skip-bench        # skip the criterion bench
#   ./scripts/bench_laya.sh --days 3650         # widen the label window
#
# Results are written to benchmark-results/run-<UTC timestamp>/ and a checksum
# manifest is printed at the end so a report can name its exact inputs.
#
# Honesty rules (mirrors scripts/eval_accuracy.sh):
#   * A quality claim is only printed when the harness can actually compute it.
#   * If the judge has produced no verdicts, that is reported as "not measurable",
#     never as a score.
#   * No row is invented for a case that has no data.

set -euo pipefail

cd "$(dirname "$0")/.."

DB_URL="${DATABASE_URL:-postgres://controlplane:secret@localhost:5432/controlplane}"
DAYS=3650
SKIP_TESTS=0
SKIP_BENCH=0

while [ $# -gt 0 ]; do
  case "$1" in
    --db)         DB_URL="$2"; shift 2 ;;
    --days)       DAYS="$2"; shift 2 ;;
    --skip-tests) SKIP_TESTS=1; shift ;;
    --skip-bench) SKIP_BENCH=1; shift ;;
    -h|--help)    sed -n '2,24p' "$0"; exit 0 ;;
    *) echo "Unknown option: $1" >&2; exit 2 ;;
  esac
done

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="benchmark-results/run-${STAMP}"
mkdir -p "$OUT"
echo "Results dir: $OUT"

API_URL="${CONTROLPLANE_API_URL:-http://localhost:8080}"

# Reports whether a judge-enabled gateway is actually reachable. Enabling the judge
# takes THREE things, and the sidecar container being up is only the first:
#   1. the `judge` compose profile started (the Laya container itself)
#   2. the GATEWAY (re)started with DECISION_JUDGE=laya|jev and LAYA_URL set --
#      both are read from the process environment at startup, so an already-running
#      gateway keeps reporting "off" until it is recreated
#   3. a policy that does not set checks.decision_judge_enabled = false (defaults ON)
# This block never fails the run; it just tells the truth before any number is quoted.
echo "════ 0/6  Judge readiness preflight ════"
{
  echo "# Judge readiness preflight — $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "gateway: $API_URL"
  CFG="$(curl -sS -m 5 "$API_URL/api/v1/system/config" 2>/dev/null || true)"
  if [ -z "$CFG" ]; then
    echo "gateway config: UNREACHABLE — cannot confirm judge state"
  else
    echo "decision_judge:            $(printf '%s' "$CFG" | sed -n 's/.*"decision_judge":"\([^"]*\)".*/\1/p')"
    echo "decision_judge_configured: $(printf '%s' "$CFG" | sed -n 's/.*"decision_judge_configured":\([a-z]*\).*/\1/p')"
    echo "fusion_enabled:            $(printf '%s' "$CFG" | sed -n 's/.*"fusion_enabled":\([a-z]*\).*/\1/p')"
    echo "calibrated_detectors:      $(printf '%s' "$CFG" | sed -n 's/.*"calibrated_detectors":\([0-9]*\).*/\1/p')"
  fi
  if command -v psql >/dev/null 2>&1; then
    JUDGE_ROWS="$(psql "$DB_URL" -t -A -c "SELECT count(*) FROM verdicts WHERE check_name LIKE 'laya-%'" 2>/dev/null || echo '?')"
    echo "laya-* verdicts in corpus:  $JUDGE_ROWS"
    if [ "$JUDGE_ROWS" = "0" ]; then
      echo
      echo "  !! NO JUDGE VERDICTS EXIST. Any 'post-Laya' comparison below is IMPOSSIBLE,"
      echo "     not merely missing: there is no judge prediction to score. Report the"
      echo "     baseline only, and say so explicitly."
    elif [ "$JUDGE_ROWS" != "?" ] && [ "$JUDGE_ROWS" -gt 0 ] 2>/dev/null; then
      echo
      echo "  Judge has produced verdicts — the ablation below now contains a real"
      echo "  judge-only row and the judge latency / disagreement sections are meaningful."
    fi
  fi
} | tee "$OUT/00b-judge-preflight.txt"

echo
echo "════ 1/6  Environment manifest ════"
{
  echo "# Environment manifest — captured $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo
  echo "## Host"
  echo "hardware: $(sysctl -n machdep.cpu.brand_string 2>/dev/null || echo unknown)"
  echo "arch: $(uname -m)"
  echo "cpus: $(sysctl -n hw.ncpu 2>/dev/null || echo unknown)"
  echo "memory_gb: $(( $(sysctl -n hw.memsize 2>/dev/null || echo 0) / 1073741824 ))"
  echo "os: $(sw_vers -productName 2>/dev/null || uname -s) $(sw_vers -productVersion 2>/dev/null || uname -r)"
  echo
  echo "## Toolchain"
  echo "rustc: $(rustc --version)"
  echo "cargo: $(cargo --version)"
  echo "psql: $(psql --version 2>/dev/null || echo not-installed)"
  echo
  echo "## Source revision"
  echo "git_head: $(git rev-parse HEAD 2>/dev/null || echo unknown)"
  echo "git_branch: $(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo unknown)"
  echo "workspace_version: $(grep -m1 '^version' Cargo.toml | sed 's/.*= *//')"
  echo
  echo "## Judge configuration in the observed corpus"
  echo "DECISION_JUDGE (local DB corpus): see judge_verdicts below"
} | tee "$OUT/00-manifest.txt"

echo
echo "════ 2/6  Corpus census ════"
{
  echo "# Corpus census"
  psql "$DB_URL" -c "
    SELECT 'verdicts' t, count(*) FROM verdicts
    UNION ALL SELECT 'intercepted_calls', count(*) FROM intercepted_calls
    UNION ALL SELECT 'reviewer_overrides (labels)', count(*) FROM reviewer_overrides
    UNION ALL SELECT 'detector_calibration', count(*) FROM detector_calibration
    UNION ALL SELECT 'calibrated=TRUE (fusion on)', count(*) FROM detector_calibration WHERE calibrated
    UNION ALL SELECT 'judge_verdicts (laya-*)', count(*) FROM verdicts WHERE check_name LIKE 'laya-%';"
  echo
  echo "## Label composition (ground truth)"
  psql "$DB_URL" -c "SELECT reviewer_action, count(*) FROM reviewer_overrides GROUP BY 1 ORDER BY 1;"
  psql "$DB_URL" -c "SELECT axis, count(*) FROM reviewer_overrides GROUP BY 1 ORDER BY 1;"
  echo
  echo "## Detector census (what actually fires)"
  psql "$DB_URL" -c "SELECT axis, check_name, outcome, count(*) FROM verdicts GROUP BY 1,2,3 ORDER BY 4 DESC LIMIT 40;"
} | tee "$OUT/01-corpus-census.txt"

echo
echo "════ 3/6  Baseline accuracy ablation (heuristic-only / judge-only / fused) ════"
if command -v psql >/dev/null 2>&1; then
  bash scripts/eval_accuracy.sh --db "$DB_URL" --days "$DAYS" 2>&1 | tee "$OUT/02-ablation.txt"
  bash scripts/eval_accuracy.sh --db "$DB_URL" --days "$DAYS" --json 2>&1 | tail -12 | tee "$OUT/03-ablation-json.txt"
  bash scripts/eval_accuracy.sh --db "$DB_URL" --days "$DAYS" --fit 2>&1 | tail -20 | tee "$OUT/04-fit-dryrun.txt"
else
  echo "psql not installed — ablation skipped" | tee "$OUT/02-ablation.txt"
fi

echo
echo "════ 4/6  Confusion matrices with Wilson 95% intervals ════"
if command -v psql >/dev/null 2>&1; then
  # Same scoring as scripts/eval_accuracy.sh, but we emit the raw (axis, config,
  # label, score) rows so the confusion matrix and Wilson intervals can be derived
  # here rather than hand-copied into the report.
  cat > "$OUT/ablation_rows.sql" <<SQL
WITH det AS (
    SELECT v.call_id, v.axis, v.check_name, v.outcome, v.confidence,
           regexp_replace(v.check_name, '-evidence$', '') AS base_name,
           CASE WHEN v.check_name LIKE 'laya-%' THEN 'judge' ELSE 'heuristic' END AS family
    FROM verdicts v
    WHERE v.created_at > NOW() - (${DAYS}::int * INTERVAL '1 day')
),
weights AS (
    SELECT detector, MAX(weight) AS weight FROM detector_calibration WHERE calibrated = TRUE GROUP BY detector
),
temperatures AS (
    SELECT DISTINCT ON (detector) detector, temperature FROM detector_calibration
    WHERE calibrated = TRUE ORDER BY detector, version DESC
),
scored AS (
    SELECT d.call_id, d.axis, d.family, d.outcome, d.confidence,
           LEAST(0.9999, GREATEST(0.0, COALESCE(w.weight, 1.0) * d.confidence)) AS weighted
    FROM det d
    LEFT JOIN weights w ON w.detector = d.base_name
    LEFT JOIN temperatures t ON t.detector = d.base_name
),
labels AS (
    SELECT DISTINCT ON (ro.call_id, ro.axis) ro.call_id, ro.axis,
           CASE WHEN ro.reviewer_action = 'confirm' THEN 1 ELSE 0 END AS y
    FROM reviewer_overrides ro
    WHERE ro.created_at > NOW() - (${DAYS}::int * INTERVAL '1 day')
    ORDER BY ro.call_id, ro.axis, ro.created_at DESC
),
predictions AS (
    SELECT call_id, axis, 'heuristic-only' AS config, MAX(confidence) AS p
    FROM scored WHERE family = 'heuristic' AND outcome <> 'pass' GROUP BY call_id, axis
    UNION ALL
    SELECT call_id, axis, 'judge-only', MAX(confidence)
    FROM scored WHERE family = 'judge' AND outcome <> 'pass' GROUP BY call_id, axis
    UNION ALL
    SELECT call_id, axis, 'fused', 1.0 - EXP(SUM(LN(GREATEST(1e-9, 1.0 - weighted))))
    FROM scored WHERE confidence >= 0.45 GROUP BY call_id, axis
)
SELECT l.axis, c.config, l.y, COALESCE(pr.p, 0)
FROM labels l
CROSS JOIN (SELECT unnest(ARRAY['heuristic-only','judge-only','fused']) AS config) c
LEFT JOIN predictions pr ON pr.call_id = l.call_id AND pr.axis = l.axis AND pr.config = c.config
ORDER BY l.axis, c.config;
SQL
  psql "$DB_URL" -t -A -F'|' -f "$OUT/ablation_rows.sql" > "$OUT/05-ablation-rows.psv"
  awk -F'|' '
  # Wilson score interval for a binomial proportion, 95%, two-sided.
  #   denom  = n + z^2
  #   center = (k + z^2/2) / denom
  #   half   = z * sqrt( k(n-k)/n + z^2/4 ) / denom
  # The k(n-k)/n term (NOT divided by n) is the one that matters at small n --
  # dropping it makes the interval look far more confident than the data allows.
  function wilson(k,n,   z,z2,den,mid,half,lo,hi,i) {
    if (n==0) return "n/a";
    z=1.96; z2=z*z; den=n+z2;
    mid=(k+z2/2)/den;
    half=(z*sqrt(k*(n-k)/n + z2/4))/den;
    lo=mid-half; if(lo<0)lo=0; hi=mid+half; if(hi>1)hi=1;
    return sprintf("[%.3f,%.3f]", lo, hi);
  }
  BEGIN {
    printf "%-14s %-15s %4s %4s %4s %4s %8s %8s %8s %-16s %-16s\n", \
      "AXIS","CONFIG","N","TP","FP","FN","RECALL","FPR","PREC","RECALL_95CI","FPR_95CI";
  }
  {
    axis=$1; config=$2; y=$3+0; p=$4+0; pred=(p>=0.70)?1:0;
    if (y==1 && pred==1) tp[axis,config]++; else if (y==0 && pred==1) fp[axis,config]++;
    else if (y==1 && pred==0) fn[axis,config]++; else tn[axis,config]++;
    if (!((axis,config) in seen)) { seen[axis,config]=1; order[++k]=axis "|" config; }
    if (y==1 && pred==1) ptp[config]++; else if (y==0 && pred==1) pfp[config]++;
    else if (y==1 && pred==0) pfn[config]++; else ptn[config]++;
  }
  END {
    for(i=1;i<=k;i++){
      split(order[i],a,"|"); axis=a[1]; config=a[2];
      TP=tp[axis,config]+0; FP=fp[axis,config]+0; FN=fn[axis,config]+0; TN=tn[axis,config]+0;
      N=TP+FP+FN+TN;
      rec=(TP+FN)>0?TP/(TP+FN):0; fpr=(FP+TN)>0?FP/(FP+TN):0; prec=(TP+FP)>0?TP/(TP+FP):0;
      printf "%-14s %-15s %4d %4d %4d %4d %8.3f %8.3f %8.3f %-16s %-16s\n", axis, config, N, TP, FP, FN, rec, fpr, prec, wilson(TP,TP+FN), wilson(FP,FP+TN);
    }
    print "----";
    ncfg=split("heuristic-only|judge-only|fused",cfgs,"|");
    for(j=1;j<=ncfg;j++){
      config=cfgs[j];
      TP=ptp[config]+0; FP=pfp[config]+0; FN=pfn[config]+0; TN=ptn[config]+0; N=TP+FP+FN+TN;
      rec=(TP+FN)>0?TP/(TP+FN):0; fpr=(FP+TN)>0?FP/(FP+TN):0; prec=(TP+FP)>0?TP/(TP+FP):0;
      printf "%-14s %-15s %4d %4d %4d %4d %8.3f %8.3f %8.3f %-16s %-16s\n", "POOLED", config, N, TP, FP, FN, rec, fpr, prec, wilson(TP,TP+FN), wilson(FP,FP+TN);
    }
  }' "$OUT/05-ablation-rows.psv" | tee "$OUT/06-confusion-matrix.txt"
else
  echo "psql not installed — confusion matrix skipped" | tee "$OUT/06-confusion-matrix.txt"
fi

echo
echo "════ 5/6  Regression: workspace test suite ════"
# DATABASE_URL is deliberately unset for this step. With it set, the DB-backed
# integration tests (db_integration_test, api_integration_test) run against whatever
# database it names: against the populated demo corpus they fail on FK/constraint
# fixtures that assume a pristine migrated schema. That is an environment artefact,
# not a judge regression, so the reported regression baseline is the DATABASE_URL-unset
# run. Set BENCH_TEST_WITH_DB=1 to capture the DB-backed variant as well.
if [ "$SKIP_TESTS" -eq 0 ]; then
  set +e
  env -u DATABASE_URL cargo test --workspace --no-fail-fast > "$OUT/07-workspace-tests.txt" 2>&1
  TEST_RC=$?
  set -e
  grep -E "^test result" "$OUT/07-workspace-tests.txt" \
    | awk '{p+=$4; f+=$6; i+=$8} END {printf "WORKSPACE TESTS (DATABASE_URL unset): passed=%d failed=%d ignored=%d\n", p, f, i}' \
    | tee "$OUT/08-test-totals.txt"
  echo "cargo test exit code: $TEST_RC" | tee -a "$OUT/08-test-totals.txt"

  if [ "${BENCH_TEST_WITH_DB:-0}" = "1" ]; then
    set +e
    DATABASE_URL="$DB_URL" cargo test --workspace --no-fail-fast > "$OUT/07b-workspace-tests-db.txt" 2>&1
    TEST_RC_DB=$?
    set -e
    grep -E "^test result" "$OUT/07b-workspace-tests-db.txt" \
      | awk '{p+=$4; f+=$6; i+=$8} END {printf "WORKSPACE TESTS (DATABASE_URL set): passed=%d failed=%d ignored=%d\n", p, f, i}' \
      | tee "$OUT/08b-test-totals-db.txt"
    echo "cargo test exit code: $TEST_RC_DB" | tee -a "$OUT/08b-test-totals-db.txt"
  fi
else
  echo "(skipped)" | tee "$OUT/08-test-totals.txt"
fi

echo
echo "════ 6/6  Synchronous-path latency: criterion bench ════"
if [ "$SKIP_BENCH" -eq 0 ]; then
  # NOTE: the workspace release profile sets `strip = "symbols"`, which on recent
  # macOS produces a corrupt host proc-macro dylib ("mis-aligned LINKEDIT string
  # pool") and the bench cannot link. Override strip for the bench profile only.
  set +e
  env -u DATABASE_URL CARGO_PROFILE_BENCH_STRIP=none \
    cargo bench -p controlplane-fast-path --bench fast_path_bench -- \
      --warm-up-time 1 --measurement-time 5 > "$OUT/09-fastpath-bench.txt" 2>&1
  BENCH_RC=$?
  set -e
  grep -E "time:|^fast_path" "$OUT/09-fastpath-bench.txt" | grep -v Collecting | tee "$OUT/10-fastpath-summary.txt"
  echo "cargo bench exit code: $BENCH_RC" | tee -a "$OUT/10-fastpath-summary.txt"
else
  echo "(skipped)" | tee "$OUT/10-fastpath-summary.txt"
fi

echo
echo "════ Checksums (SHA-256) ════"
(cd "$OUT" && shasum -a 256 ./* 2>/dev/null || sha256sum ./*) | tee "$OUT/11-checksums.txt"

echo
echo "Done. Results in $OUT"
