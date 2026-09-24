#!/usr/bin/env bash
# ControlPlane.ai — Offline accuracy harness for the hybrid judge
# (docs/analysis/laya-integration-plan.md §5.4, §5.5, §12)
#
# Answers the only question that matters before claiming the hybrid helps:
#   heuristic-only  vs  judge-only  vs  fused  vs  fused+calibrated
# measured on OUR OWN labels, per axis — not on a vendor benchmark.
#
# Corpus (no new stores, per the plan):
#   reviewer_overrides (migration 021) -> ground truth (confirm = positive)
#   verdicts                           -> per-detector probabilities
#   intercepted_calls                  -> joined via call_id
#
# Metrics per axis per configuration:
#   precision, recall, F1, FP-rate, Brier score, ECE,
#   judge/heuristic disagreement rate, judge latency percentiles.
#
# Usage:
#   ./scripts/eval_accuracy.sh                    # report the ablation
#   ./scripts/eval_accuracy.sh --days 30          # widen the label window
#   ./scripts/eval_accuracy.sh --axis responsibility
#   ./scripts/eval_accuracy.sh --threshold 0.7
#   ./scripts/eval_accuracy.sh --fit              # propose a temperature/weight fit
#   ./scripts/eval_accuracy.sh --fit --apply      # write the fit (enables fusion)
#   ./scripts/eval_accuracy.sh --reset            # mark every fit uncalibrated (fusion off)
#   ./scripts/eval_accuracy.sh --json             # machine-readable summary
#
# Honesty rules this script enforces:
#   * No accuracy claim is printed without the counts behind it (plan §12).
#   * Every labelled (call, axis) pair counts, including the ones where no detector
#     fired — dropping those would silently delete true negatives and make the
#     false-positive rate look better than it is.
#   * `--fit` is a DRY RUN unless `--apply` is passed.
#   * The report states whether judge probabilities were already calibrated when the
#     verdicts were written, because that changes how to read `fused+calibrated`.

set -euo pipefail

DB_URL="${DATABASE_URL:-postgres://controlplane:secret@localhost:5432/controlplane}"
DAYS=30
AXIS=""
THRESHOLD="${JUDGE_EDIT_THRESHOLD:-0.70}"
EVIDENCE_FLOOR="${JUDGE_EVIDENCE_THRESHOLD:-0.45}"
DISAGREEMENT_DELTA="${JUDGE_DISAGREEMENT_DELTA:-0.40}"
DO_FIT=0
DO_APPLY=0
DO_RESET=0
JSON=0

usage() { sed -n '2,42p' "$0"; }

while [ $# -gt 0 ]; do
  case "$1" in
    --db)        DB_URL="$2"; shift 2 ;;
    --days)      DAYS="$2"; shift 2 ;;
    --axis)      AXIS="$2"; shift 2 ;;
    --threshold) THRESHOLD="$2"; shift 2 ;;
    --fit)       DO_FIT=1; shift ;;
    --apply)     DO_APPLY=1; DO_FIT=1; shift ;;
    --reset)     DO_RESET=1; shift ;;
    --json)      JSON=1; shift ;;
    -h|--help)   usage; exit 0 ;;
    *) echo "Unknown option: $1" >&2; exit 2 ;;
  esac
done

if ! command -v psql >/dev/null 2>&1; then
  echo "ERROR: psql not found. This harness reads the governance corpus directly." >&2
  echo "       Install the postgresql client, or run it inside the compose network." >&2
  exit 1
fi

if ! psql "$DB_URL" -c 'SELECT 1' >/dev/null 2>&1; then
  echo "ERROR: cannot connect to PostgreSQL at ${DB_URL%%@*}@..." >&2
  echo "       Set DATABASE_URL or pass --db <url>." >&2
  exit 1
fi

WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT

# Rows out, pipe-delimited, no header, no padding.
q()  { psql "$DB_URL" -t -A -F'|' -v ON_ERROR_STOP=1 -c "$1"; }
qf() { psql "$DB_URL" -t -A -F'|' -v ON_ERROR_STOP=1 -f "$1"; }

q_scalar() { psql "$DB_URL" -t -A -v ON_ERROR_STOP=1 -c "$1"; }

has_table() {
  [ "$(q_scalar "SELECT to_regclass('$1') IS NOT NULL")" = "t" ]
}

# ─── Schema guards ─────────────────────────────────────────────────────────────
MISSING=""
for table in reviewer_overrides verdicts intercepted_calls; do
  if ! has_table "$table"; then
    MISSING="$MISSING $table"
  fi
done

if [ -n "$MISSING" ]; then
  echo "ERROR: required table(s) missing:$MISSING" >&2
  echo "       Run ./infra/migrate.sh against this database first." >&2
  exit 1
fi

HAS_CALIBRATION=1
if ! has_table detector_calibration; then
  HAS_CALIBRATION=0
  echo "NOTE: detector_calibration is missing (pre-023 database), so the fusion is off." >&2
  echo "      The fused configurations below are reported, but neither can act today." >&2
fi

AXIS_FILTER=""
if [ -n "$AXIS" ]; then
  AXIS_FILTER="AND l.axis = '${AXIS}'"
fi

# ─── Reset ─────────────────────────────────────────────────────────────────────
if [ "$DO_RESET" -eq 1 ]; then
  if [ "$HAS_CALIBRATION" -eq 0 ]; then
    echo "ERROR: detector_calibration does not exist — nothing to reset." >&2
    exit 1
  fi
  q "UPDATE detector_calibration SET calibrated = FALSE" >/dev/null
  echo "All calibration rows marked uncalibrated. The decision engine is back on its"
  echo "pre-fusion aggregator (fail-safe), and judge probabilities are raw again."
  exit 0
fi

# ─── The ablation query ────────────────────────────────────────────────────────
# One row per (axis, configuration, label, score). The scoring lives in SQL so the
# numbers come from the same rows the decision engine read — nothing is re-derived
# from logs, and nothing is invented for a case that has no verdicts.
cat > "$WORKDIR/ablation.sql" <<SQL
WITH det AS (
    -- Evidence-only readings (outcome = 'pass', check_name ending in -evidence) ARE
    -- included: the decision engine fuses them, so the offline view must too. They are
    -- excluded from the single-engine configs below, where "did this detector fire?"
    -- is the question.
    SELECT v.call_id,
           v.axis,
           v.check_name,
           v.outcome,
           v.confidence,
           -- Mirror the engine's base_detector(): an evidence reading is attributed to
           -- its detector, so it uses that detector's fitted weight and temperature.
           regexp_replace(v.check_name, '-evidence$', '') AS base_name,
           CASE WHEN v.check_name LIKE 'laya-%' THEN 'judge' ELSE 'heuristic' END AS family
    FROM verdicts v
    WHERE v.created_at > NOW() - (${DAYS}::int * INTERVAL '1 day')
),
weights AS (
    SELECT detector, MAX(weight) AS weight
    FROM detector_calibration
    WHERE calibrated = TRUE
    GROUP BY detector
),
temperatures AS (
    SELECT DISTINCT ON (detector) detector, temperature
    FROM detector_calibration
    WHERE calibrated = TRUE
    ORDER BY detector, version DESC
),
scored AS (
    SELECT d.call_id,
           d.axis,
           d.family,
           d.outcome,
           d.confidence,
           LEAST(0.9999, GREATEST(0.0, COALESCE(w.weight, 1.0) * d.confidence)) AS weighted,
           1.0 / (1.0 + POWER(
               GREATEST(1e-6, 1.0 - d.confidence) / GREATEST(1e-6, d.confidence),
               1.0 / GREATEST(0.05, COALESCE(t.temperature, 1.0))
           )) AS calibrated_p,
           LEAST(0.9999, GREATEST(0.0, COALESCE(w.weight, 1.0) * (
               1.0 / (1.0 + POWER(
                   GREATEST(1e-6, 1.0 - d.confidence) / GREATEST(1e-6, d.confidence),
                   1.0 / GREATEST(0.05, COALESCE(t.temperature, 1.0))
               ))
           ))) AS weighted_calibrated
    FROM det d
    LEFT JOIN weights w ON w.detector = d.base_name
    LEFT JOIN temperatures t ON t.detector = d.base_name
),
labels AS (
    SELECT DISTINCT ON (ro.call_id, ro.axis)
           ro.call_id,
           ro.axis,
           CASE WHEN ro.reviewer_action = 'confirm' THEN 1 ELSE 0 END AS y
    FROM reviewer_overrides ro
    WHERE ro.created_at > NOW() - (${DAYS}::int * INTERVAL '1 day')
    ORDER BY ro.call_id, ro.axis, ro.created_at DESC
),
predictions AS (
    -- heuristic-only: today's baseline (actionable verdicts only)
    SELECT call_id, axis, 'heuristic-only' AS config, MAX(confidence) AS p
    FROM scored WHERE family = 'heuristic' AND outcome <> 'pass' GROUP BY call_id, axis
    UNION ALL
    -- judge-only: isolates the model's standalone contribution
    SELECT call_id, axis, 'judge-only', MAX(confidence)
    FROM scored WHERE family = 'judge' AND outcome <> 'pass' GROUP BY call_id, axis
    UNION ALL
    -- fused: the weighted noisy-OR the decision engine computes, per-detector floor applied
    SELECT call_id, axis, 'fused',
           1.0 - EXP(SUM(LN(GREATEST(1e-9, 1.0 - weighted))))
    FROM scored WHERE confidence >= ${EVIDENCE_FLOOR} GROUP BY call_id, axis
    UNION ALL
    -- fused+calibrated: the same fusion after re-applying the fitted temperatures
    SELECT call_id, axis, 'fused+calibrated',
           1.0 - EXP(SUM(LN(GREATEST(1e-9, 1.0 - weighted_calibrated))))
    FROM scored WHERE confidence >= ${EVIDENCE_FLOOR} GROUP BY call_id, axis
),
configs AS (
    SELECT unnest(ARRAY['heuristic-only','judge-only','fused','fused+calibrated']) AS config
)
SELECT l.axis, c.config, l.y, COALESCE(pr.p, 0)
FROM labels l
CROSS JOIN configs c
LEFT JOIN predictions pr
       ON pr.call_id = l.call_id AND pr.axis = l.axis AND pr.config = c.config
WHERE TRUE ${AXIS_FILTER}
ORDER BY l.axis, c.config;
SQL

# ─── Metrics ───────────────────────────────────────────────────────────────────
compute_metrics() {
  awk -v threshold="$THRESHOLD" '
    function pct(x) { return sprintf("%6.3f", x) }
    BEGIN {
      FS = "|"
      printf "%-14s %-18s %6s %6s %6s %6s %8s %8s %7s %9s\n", \
             "AXIS", "CONFIG", "N", "PREC", "REC", "F1", "FP_RATE", "BRIER", "ECE", "TP/FP"
    }
    {
      axis = $1; config = $2; y = $3 + 0; p = $4 + 0
      key = axis SUBSEP config
      if (!(key in seen)) { seen[key] = 1; order[++n] = key }

      pred = (p >= threshold) ? 1 : 0
      if (y == 1 && pred == 1) tp[key]++
      else if (y == 0 && pred == 1) fp[key]++
      else if (y == 1 && pred == 0) fn[key]++
      else tn[key]++

      total[key]++
      brier[key] += (p - y) * (p - y)

      bin = int(p * 10); if (bin > 9) bin = 9
      bk = key SUBSEP bin
      bin_n[bk]++
      bin_sum[bk] += p
      bin_pos[bk] += y
      if (!(bk in bin_seen)) { bin_seen[bk] = 1; bins[key] = bins[key] " " bin }
    }
    END {
      for (i = 1; i <= n; i++) {
        k = order[i]
        split(k, parts, SUBSEP)
        axis = parts[1]; config = parts[2]

        TP = tp[k] + 0; FP = fp[k] + 0; FN = fn[k] + 0; TN = tn[k] + 0; N = total[k] + 0
        prec = (TP + FP) > 0 ? TP / (TP + FP) : 0
        rec  = (TP + FN) > 0 ? TP / (TP + FN) : 0
        f1   = (prec + rec) > 0 ? 2 * prec * rec / (prec + rec) : 0
        # False-positive rate: a clean call pushed into an action. The visible pain.
        fpr  = (FP + TN) > 0 ? FP / (FP + TN) : 0
        brier_score = N > 0 ? brier[k] / N : 0

        ece = 0
        m = split(bins[k], bin_list, " ")
        for (j = 1; j <= m; j++) {
          if (bin_list[j] == "") continue
          bk = k SUBSEP bin_list[j]
          bn = bin_n[bk] + 0
          if (bn == 0) continue
          avg_p = bin_sum[bk] / bn
          acc = bin_pos[bk] / bn
          ece += (bn / N) * (avg_p > acc ? avg_p - acc : acc - avg_p)
        }

        printf "%-14s %-18s %6d %6s %6s %6s %8s %8s %7s %9s\n", \
               axis, config, N, pct(prec), pct(rec), pct(f1), pct(fpr), pct(brier_score), pct(ece), TP "/" FP
      }
    }
  '
}

# ─── Fit ───────────────────────────────────────────────────────────────────────
# Temperature: 1-D grid search minimising NLL on the labelled corpus, computed in SQL.
# Weight: the plan's reliability weight, clamped to [0.3, 1.0] — here derived from the
# detector's own measured precision. A per-axis threshold sweep is deliberately NOT
# automated: the plan's objective (maximise recall s.t. FP-rate <= 5%) needs more
# labels than a first corpus provides, and guessing it would be the opposite of honest.
run_fit() {
  if [ "$HAS_CALIBRATION" -eq 0 ]; then
    echo "ERROR: detector_calibration does not exist — run migration 023 first." >&2
    exit 1
  fi

  cat > "$WORKDIR/fit.sql" <<'SQL'
WITH labels AS (
    SELECT DISTINCT ON (ro.call_id, ro.axis)
           ro.call_id, ro.axis,
           CASE WHEN ro.reviewer_action = 'confirm' THEN 1 ELSE 0 END AS y
    FROM reviewer_overrides ro
    ORDER BY ro.call_id, ro.axis, ro.created_at DESC
),
scored AS (
    SELECT v.check_name, l.y, v.confidence AS p
    FROM verdicts v
    JOIN labels l ON l.call_id = v.call_id AND l.axis = v.axis
    WHERE v.check_name LIKE 'laya-%'
      AND v.outcome <> 'pass'
      AND v.check_name NOT LIKE '%-evidence'
),
grid AS (
    SELECT t AS temperature FROM (VALUES
        (0.25),(0.4),(0.5),(0.75),(1.0),(1.25),(1.5),(2.0),(2.5),(3.0),(4.0),(5.0)
    ) AS g(t)
),
candidates AS (
    SELECT s.check_name,
           g.temperature,
           -SUM(
               s.y * LN(GREATEST(1e-9, 1.0 / (1.0 + POWER(
                   GREATEST(1e-6, 1.0 - s.p) / GREATEST(1e-6, s.p), 1.0 / g.temperature))))
             + (1 - s.y) * LN(GREATEST(1e-9, 1.0 - 1.0 / (1.0 + POWER(
                   GREATEST(1e-6, 1.0 - s.p) / GREATEST(1e-6, s.p), 1.0 / g.temperature))))
           ) AS nll
    FROM scored s CROSS JOIN grid g
    GROUP BY s.check_name, g.temperature
),
best AS (
    SELECT DISTINCT ON (check_name) check_name, temperature
    FROM candidates
    ORDER BY check_name, nll ASC, temperature ASC
),
quality AS (
    SELECT check_name,
           COUNT(*) AS n,
           CASE WHEN COUNT(*) > 0
                THEN COUNT(*) FILTER (WHERE y = 1)::float8 / COUNT(*)::float8
                ELSE 0.0 END AS precision
    FROM scored
    GROUP BY check_name
)
SELECT b.check_name, b.temperature, COALESCE(q.precision, 0.0), COALESCE(q.n, 0)
FROM best b
LEFT JOIN quality q ON q.check_name = b.check_name
ORDER BY b.check_name;
SQL

  echo ""
  echo "=== Calibration fit (dry run) ==="
  echo "Objective: minimise NLL over temperatures in [0.25, 5.0]; weight from precision."
  echo ""

  if [ "$(q_scalar "SELECT COUNT(*) FROM reviewer_overrides")" = "0" ]; then
    echo "  (no reviewer labels at all — nothing to fit)"
    echo ""
    echo "Nothing was written. A fit needs called-and-reviewed traffic first:"
    echo "  resolve escalations (confirm / dismiss / override) in the dashboard, then re-run."
    return 0
  fi

  FIT_ROWS="$(qf "$WORKDIR/fit.sql")"

  if [ -z "$FIT_ROWS" ]; then
    echo "  (no labelled judge verdicts — the judge produced no actionable reading on any"
    echo "   reviewed call, so there is nothing to calibrate)"
    return 0
  fi

  printf '%-28s %12s %11s %9s %7s\n' "DETECTOR" "TEMPERATURE" "PRECISION" "WEIGHT" "N"

  VERSION="$(q_scalar "SELECT COALESCE(MAX(version), 0) + 1 FROM detector_calibration")"
  VERSION="${VERSION:-1}"
  COUNT=0
  : > "$WORKDIR/apply.sql"

  while IFS='|' read -r detector temperature precision n; do
    if [ -z "$detector" ]; then
      continue
    fi
    # Reliability weight: the detector's own precision, clamped to [0.3, 1.0] (plan §5.4).
    weight="$(awk -v p="$precision" 'BEGIN { w = p; if (w < 0.3) w = 0.3; if (w > 1.0) w = 1.0; printf "%.4f", w }')"
    printf '%-28s %12s %11s %9s %7s\n' "$detector" "$temperature" "$precision" "$weight" "$n"

    cat >> "$WORKDIR/apply.sql" <<SQL
INSERT INTO detector_calibration
    (detector, primitive, option_count, temperature, weight, calibrated, version)
VALUES ('${detector}', 'choice', 2, ${temperature}, ${weight}, TRUE, ${VERSION})
ON CONFLICT (detector, primitive, option_count, version)
DO UPDATE SET temperature = EXCLUDED.temperature,
              weight = EXCLUDED.weight,
              calibrated = TRUE,
              fitted_at = NOW();
SQL
    COUNT=$((COUNT + 1))
  done <<EOF
$FIT_ROWS
EOF

  echo ""
  echo "Proposed version: v${VERSION} across ${COUNT} detector(s)."
  echo ""

  if [ "$DO_APPLY" -eq 1 ]; then
    {
      echo "BEGIN;"
      cat "$WORKDIR/apply.sql"
      echo "COMMIT;"
    } > "$WORKDIR/apply_tx.sql"
    qf "$WORKDIR/apply_tx.sql" >/dev/null
    echo "Applied. The fusion is now ENABLED (detector_calibration.calibrated = TRUE)."
    echo "The gateway's calibration reloader picks this up within 60s."
    echo ""
    echo "Caveat before you trust these numbers: this fit was measured on the labels that"
    echo "exist today, and it was NOT validated on a held-out split. Re-run the ablation"
    echo "after the fit and treat the magnitude as provisional until the corpus supports"
    echo "a train/test split."
  else
    echo "Dry run — nothing written. Add --apply to enable the fusion, ideally against a"
    echo "copy of the database first."
  fi
}

# ─── Report ────────────────────────────────────────────────────────────────────
echo ""
echo "=== Hybrid accuracy ablation ==="
echo "Corpus:     reviewer_overrides (confirm = positive) joined to verdicts"
echo "Window:     last ${DAYS} day(s)"
echo "Axis:       ${AXIS:-all}"
echo "Threshold:  ${THRESHOLD} (a confidence at/above this is a positive prediction)"
echo "================================="

ROWS="$(qf "$WORKDIR/ablation.sql")"

if [ -z "$ROWS" ]; then
  echo ""
  echo "No labelled (call, axis) pairs in this window."
  echo "Nothing to report — an accuracy claim here would be fabricated, so there is none."
  echo ""
  echo "To populate the corpus:"
  echo "  1. run traffic through the proxy with DECISION_JUDGE=laya|jev and the sidecar up"
  echo "  2. resolve the resulting escalations (confirm / dismiss / override) in the dashboard"
  echo "  3. re-run this script"
  if [ "$DO_FIT" -eq 1 ]; then
    run_fit
  fi
  exit 0
fi

printf '%s\n' "$ROWS" | compute_metrics

# ─── Judge latency (shadow budget: <2s, plan §7) ───────────────────────────────
LATENCY="$(q "
  SELECT COALESCE(ROUND(PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY duration_ms)), 0),
         COALESCE(ROUND(PERCENTILE_CONT(0.99) WITHIN GROUP (ORDER BY duration_ms)), 0),
         COUNT(*)
  FROM verdicts
  WHERE check_name LIKE 'laya-%'
    AND duration_ms IS NOT NULL
    AND created_at > NOW() - (${DAYS}::int * INTERVAL '1 day')")"

IFS='|' read -r P50 P99 COUNT_LAYA <<EOF
$LATENCY
EOF
P50="${P50:-0}"; P99="${P99:-0}"; COUNT_LAYA="${COUNT_LAYA:-0}"

echo ""
echo "--- Judge latency (shadow-path budget is <2s) ---"
echo "  Verdicts: ${COUNT_LAYA}"
echo "  p50:      ${P50} ms"
echo "  p99:      ${P99} ms"

if awk -v p99="$P99" 'BEGIN { exit !(p99 >= 2000) }'; then
  echo "  [WARN] p99 >= 2s — the judge is exceeding the shadow-path budget."
fi

# ─── Disagreement rate (plan §5.6) ─────────────────────────────────────────────
D_SQL="
  WITH per_call AS (
    SELECT call_id, axis,
           MAX(confidence) FILTER (WHERE check_name LIKE 'laya-%') AS j,
           MAX(confidence) FILTER (WHERE check_name NOT LIKE 'laya-%' AND outcome <> 'pass') AS h
    FROM verdicts
    WHERE created_at > NOW() - (${DAYS}::int * INTERVAL '1 day')
    GROUP BY call_id, axis
  )
  SELECT COUNT(*) FILTER (WHERE j IS NOT NULL AND h IS NOT NULL),
         COUNT(*) FILTER (WHERE j IS NOT NULL AND h IS NOT NULL AND ABS(j - h) >= ${DISAGREEMENT_DELTA})
  FROM per_call
  WHERE j IS NOT NULL"
if [ -n "$AXIS" ]; then
  D_SQL="$D_SQL AND axis = '${AXIS}'"
fi

D_ROW="$(q "$D_SQL")"
IFS='|' read -r COMPARABLE DISAGREEMENTS <<EOF
$D_ROW
EOF
COMPARABLE="${COMPARABLE:-0}"
DISAGREEMENTS="${DISAGREEMENTS:-0}"

echo ""
echo "--- Judge / heuristic disagreements (routed to human review) ---"
echo "  Comparable (call, axis) pairs: ${COMPARABLE}"
echo "  Disagreements (gap >= ${DISAGREEMENT_DELTA}):     ${DISAGREEMENTS}"
if [ "${COMPARABLE:-0}" -gt 0 ]; then
  awk -v c="$COMPARABLE" -v d="$DISAGREEMENTS" \
    'BEGIN { printf "  Disagreement rate:             %.1f%%\n", 100.0 * d / c }'
fi

# ─── Calibration state (honesty) ───────────────────────────────────────────────
CAL_DETECTORS=0
CAL_VERSION="none"
if [ "$HAS_CALIBRATION" -eq 1 ]; then
  CAL="$(q "SELECT COUNT(*), COALESCE(MAX(version), 0) FROM detector_calibration WHERE calibrated = TRUE")"
  IFS='|' read -r CAL_DETECTORS CAL_VERSION <<EOF
$CAL
EOF
  CAL_DETECTORS="${CAL_DETECTORS:-0}"
  CAL_VERSION="${CAL_VERSION:-0}"

  echo ""
  echo "--- Calibration state ---"
  echo "  Fitted detectors: ${CAL_DETECTORS}"
  echo "  Fit version:      ${CAL_VERSION}"
  if [ "${CAL_DETECTORS:-0}" -eq 0 ]; then
    echo "  Fusion:           OFF — the decision engine used its pre-fusion aggregator,"
    echo "                    so the judge probabilities in this report are RAW."
  else
    echo "  Fusion:           ON"
  fi
fi

echo ""
echo "How to read this (plan §5.5):"
echo "  * A win is claimed only where 'fused' (or 'fused+calibrated') beats BOTH single-engine"
echo "    baselines on the SAME labelled set — that is what these columns show."
echo "  * FP-rate is the visible pain (a clean call pushed into an action); recall is the"
echo "    expensive pain in production. Neither is free."
echo "  * Every labelled pair counts, including calls where no detector fired (they are"
echo "    true negatives). Dropping them would flatter the FP-rate."
echo "  * 'fused+calibrated' re-applies the currently fitted temperature to the stored"
echo "    confidence. If the verdicts were written while a fit was already live, that column"
echo "    double-calibrates — run --reset and recollect before comparing it."
echo "  * Small N means noise. Treat a difference of a few points as inconclusive."
echo ""

if [ "$JSON" -eq 1 ]; then
  cat <<JSON
{
  "days": ${DAYS},
  "threshold": ${THRESHOLD},
  "disagreement_delta": ${DISAGREEMENT_DELTA},
  "comparable_pairs": ${COMPARABLE},
  "disagreements": ${DISAGREEMENTS},
  "fitted_detectors": ${CAL_DETECTORS},
  "judge_latency_p50_ms": ${P50},
  "judge_latency_p99_ms": ${P99}
}
JSON
fi

if [ "$DO_FIT" -eq 1 ]; then
  run_fit
fi
