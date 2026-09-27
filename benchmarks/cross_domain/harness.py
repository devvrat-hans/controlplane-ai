#!/usr/bin/env python3
"""ControlPlane.ai — cross-domain benchmark harness.

Three steps, each independently re-runnable:
    traffic  — replay dataset.jsonl through the live proxy, record raw outcomes
    enrich   — join every request to its verdicts / escalation via PostgreSQL
    metrics  — confusion matrices, rates, Wilson intervals, latency percentile

Nothing here writes to the application: it is an HTTP client plus read-only SQL.
Raw prompts are NOT copied into the outputs; only ids, status codes, timings and
a boolean "was it redacted" survive.

Usage:
    python3 harness.py traffic --proxy http://127.0.0.1:8901 --out out/
    python3 harness.py enrich  --out out/ --db "$DATABASE_URL"
    python3 harness.py metrics --out out/
"""

from __future__ import annotations

import argparse
import json
import math
import pathlib
import subprocess
import time
import urllib.error
import urllib.request
import uuid

HERE = pathlib.Path(__file__).resolve().parent
REDACTION_MARKER = "[REDACTED:"
NS = uuid.UUID("6f1b6f4a-6f1b-4f1b-8f1b-000000000001")


# ─────────────────────────────── traffic ────────────────────────────────
def step_traffic(args: argparse.Namespace) -> None:
    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    rows = [json.loads(l) for l in (HERE / "dataset.jsonl").read_text().splitlines() if l.strip()]

    results = []
    for case in rows:
        session = str(uuid.uuid5(NS, case["session_key"]))
        for rep in range(case["repeats"]):
            body = json.dumps({
                "model": case["model"],
                "app_id": case["app_id"],
                "session_id": session,
                "messages": [{"role": "user", "content": case["prompt"]}],
                "max_tokens": case["max_tokens"],
            }).encode()
            req = urllib.request.Request(
                f"{args.proxy}/v1/messages", data=body,
                headers={"Content-Type": "application/json"}, method="POST")

            t0 = time.perf_counter()
            code, text, headers = 0, "", {}
            try:
                with urllib.request.urlopen(req, timeout=args.timeout) as resp:
                    code = resp.status
                    text = resp.read().decode("utf-8", "replace")
                    headers = dict(resp.headers)
            except urllib.error.HTTPError as e:
                code = e.code
                text = e.read().decode("utf-8", "replace")
                headers = dict(e.headers or {})
            except Exception as e:  # connection error, timeout, ...
                code = 0
                text = f"transport_error: {type(e).__name__}"
            dt_ms = int((time.perf_counter() - t0) * 1000)

            corr = (headers.get("X-ControlPlane-Correlation-Id")
                    or headers.get("x-controlplane-correlation-id") or "")
            blocked = code == 403 or '"blocked_by_policy"' in text
            # A 403 does NOT carry the correlation-id header; the id is only inside
            # the error body ({"error":{"code",...,"correlation_id"}}). Without this
            # fallback exactly the blocked calls -- the ones most worth tracing --
            # cannot be joined to their verdicts.
            if not corr:
                try:
                    parsed = json.loads(text)
                    err = parsed.get("error") if isinstance(parsed, dict) else None
                    if isinstance(err, dict):
                        corr = err.get("correlation_id") or ""
                    if not corr and isinstance(parsed, dict):
                        corr = parsed.get("correlation_id") or ""
                except Exception:
                    pass
            redacted = REDACTION_MARKER in text
            error = '"error"' in text and not blocked

            results.append({
                "id": case["id"], "domain": case["domain"], "category": case["category"],
                "generation_rule": case["generation_rule"], "gold_action": case["gold_action"],
                "gold_basis": case["gold_basis"], "app_id": case["app_id"],
                "session": session, "repeat": rep,
                "http_code": code, "latency_ms": dt_ms,
                "correlation_id": corr,
                "blocked": blocked, "redacted": redacted, "error": error,
                "body_bytes": len(text),
                # NOTE: the response body itself is deliberately not stored.
            })
            print(f"  {case['id']} r{rep}  http={code}  {dt_ms}ms"
                  f"{'  BLOCKED' if blocked else ''}{'  REDACTED' if redacted else ''}")

    (out / "results-raw.jsonl").write_text(
        "".join(json.dumps(r, sort_keys=True) + "\n" for r in results))
    print(f"traffic: {len(results)} requests -> {out/'results-raw.jsonl'}")


# ─────────────────────────────── enrich ─────────────────────────────────
PSQL_COLUMNS = (
    "COALESCE(ic.correlation_id::text,'')",
    "COALESCE(ic.id::text,'')",
    "COALESCE(ic.token_count_input,0)",
    "COALESCE(ic.token_count_output,0)",
    "COALESCE(ic.fast_path_latency_ms,0)",
    "COALESCE(ic.has_tool_use,false)",
    "COALESCE(v.agg,'[]')",
    "COALESCE(v.worst,'pass')",
    "COALESCE(v.n_verdicts,0)",
    "COALESCE(v.n_escalate,0)",
    "COALESCE(v.n_block,0)",
    "COALESCE(v.n_edit,0)",
    "COALESCE(v.checks,'[]')",
    "COALESCE(e.n_cases,0)",
    # --- component attribution ---
    # Every non-pass verdict with its outcome, so we can tell *which* component
    # produced the worst action rather than merely that something fired.
    "COALESCE(v.fired_detail,'[]')",
    # How many judge readings exist for the call at all (including passes) --
    # 0 means the judge abstained, which must not be silently read as "allowed".
    "COALESCE(v.judge_total,0)",
    "COALESCE(v.judge_nonpass,0)",
)


def step_enrich(args: argparse.Namespace) -> None:
    out = pathlib.Path(args.out)
    rows = [json.loads(l) for l in (out / "results-raw.jsonl").read_text().splitlines() if l.strip()]
    corrs = sorted({r["correlation_id"] for r in rows if r["correlation_id"]})
    if not corrs:
        print("enrich: no correlation ids captured; nothing to join")
        return

    sql = f"""
    WITH v AS (
      SELECT call_id,
             json_agg(json_build_object(
               'check_name', check_name, 'outcome', outcome,
               'confidence', confidence, 'axis', axis, 'path', path,
               'duration_ms', duration_ms) ORDER BY check_name)::text AS agg,
             -- worst outcome by the app's own severity order:
             -- Pass(-1) < Escalate(0) < Edit(1) < Block(2)   (checks-inventory.md §0)
             CASE max(CASE outcome
                        WHEN 'pass' THEN -1 WHEN 'escalate' THEN 0
                        WHEN 'edit' THEN 1  WHEN 'block' THEN 2 ELSE -1 END)
                  WHEN 2 THEN 'block' WHEN 1 THEN 'edit'
                  WHEN 0 THEN 'escalate' ELSE 'pass' END AS worst,
             count(*) AS n_verdicts,
             count(*) FILTER (WHERE outcome='escalate') AS n_escalate,
             count(*) FILTER (WHERE outcome='block')    AS n_block,
             count(*) FILTER (WHERE outcome='edit')     AS n_edit,
             (SELECT json_agg(DISTINCT check_name)::text FROM verdicts v2
               WHERE v2.call_id = v.call_id AND v2.outcome <> 'pass') AS checks,
             (SELECT json_agg(json_build_object(
                       'check_name', v3.check_name, 'outcome', v3.outcome,
                       'path', v3.path, 'confidence', v3.confidence))::text
                FROM verdicts v3
               WHERE v3.call_id = v.call_id AND v3.outcome <> 'pass') AS fired_detail,
             (SELECT count(*) FROM verdicts v4
               WHERE v4.call_id = v.call_id AND v4.check_name LIKE 'laya-%') AS judge_total,
             (SELECT count(*) FROM verdicts v5
               WHERE v5.call_id = v.call_id AND v5.check_name LIKE 'laya-%'
                 AND v5.outcome <> 'pass') AS judge_nonpass
      FROM verdicts v GROUP BY call_id
    ), e AS (
      SELECT call_id, count(*) AS n_cases FROM escalation_cases GROUP BY call_id
    )
    SELECT {', '.join(PSQL_COLUMNS)}
    FROM intercepted_calls ic
    LEFT JOIN v ON v.call_id = ic.id
    LEFT JOIN e ON e.call_id = ic.id
    WHERE ic.correlation_id::text = ANY(ARRAY[{','.join(f"'{c}'" for c in corrs)}]);
    """

    proc = subprocess.run(
        ["psql", args.db, "-t", "-A", "-F", "\x1f", "-c", sql],
        capture_output=True, text=True)
    if proc.returncode != 0:
        raise SystemExit(f"psql failed: {proc.stderr.strip()}")

    db = {}
    for line in proc.stdout.splitlines():
        f = line.split("\x1f")
        if len(f) != len(PSQL_COLUMNS):
            continue
        db[f[0]] = {
            "call_id": f[1], "token_in": int(f[2] or 0), "token_out": int(f[3] or 0),
            "fast_path_latency_ms": int(f[4] or 0), "has_tool_use": f[5] == "t",
            "verdicts": json.loads(f[6]), "worst_outcome": f[7], "n_verdicts": int(f[8] or 0),
            "n_escalate": int(f[9] or 0), "n_block": int(f[10] or 0), "n_edit": int(f[11] or 0),
            "fired_checks": json.loads(f[12]), "n_escalation_cases": int(f[13] or 0),
            "fired_detail": json.loads(f[14]), "judge_total": int(f[15] or 0),
            "judge_nonpass": int(f[16] or 0),
        }

    enriched = []
    for r in rows:
        e = db.get(r["correlation_id"], {})
        detected = bool(e.get("fired_checks"))
        enriched.append({**r,
                         "call_id": e.get("call_id"),
                         "token_out": e.get("token_out", 0),
                         "fast_path_latency_ms": e.get("fast_path_latency_ms", 0),
                         "has_tool_use": e.get("has_tool_use", False),
                         "worst_outcome": e.get("worst_outcome", "none"),
                         "n_verdicts": e.get("n_verdicts", 0),
                         "n_escalation_cases": e.get("n_escalation_cases", 0),
                         "fired_checks": e.get("fired_checks", []) or [],
                         "fired_detail": e.get("fired_detail", []) or [],
                         "judge_total": e.get("judge_total", 0),
                         "judge_nonpass": e.get("judge_nonpass", 0),
                         "detected": detected,
                         "matched_call": bool(e)})

    (out / "results-enriched.jsonl").write_text(
        "".join(json.dumps(r, sort_keys=True) + "\n" for r in enriched))
    joined = sum(1 for r in enriched if r["matched_call"])
    print(f"enrich: {joined}/{len(enriched)} requests joined to a call row "
          f"-> {out/'results-enriched.jsonl'}")


# ─────────────────────────────── metrics ────────────────────────────────
def wilson(k: int, n: int, z: float = 1.96) -> str:
    if n == 0:
        return "n/a"
    z2 = z * z
    den = n + z2
    mid = (k + z2 / 2) / den
    half = z * math.sqrt(k * (n - k) / n + z2 / 4) / den
    return f"[{max(0.0, mid - half):.3f},{min(1.0, mid + half):.3f}]"


def pct(values: list[int], p: float) -> int:
    if not values:
        return 0
    s = sorted(values)
    return s[min(len(s) - 1, int(len(s) * p))]


def confusion(rows: list[dict], predicted, expect=None) -> dict:
    """Confusion matrix for one prediction predicate.

    `expect` decides what counts as a positive. It defaults to DETECTION
    semantics ("any non-allow gold action should be detected"). Blocking must
    pass its own predicate: a detection is not a correct block -- only
    `gold_action == "block"` requires a block (plan requirement). Using the
    detection predicate for blocking silently turns every correctly-escorted
    or correctly-redacted case into a false negative.
    """
    if expect is None:
        expect = lambda r: r["gold_action"] != "allow"
    tp = fp = fn = tn = 0
    for r in rows:
        g = r["gold_action"]
        if g == "unknown":
            continue
        expect_val = bool(expect(r))
        pred = bool(predicted(r))
        if expect_val and pred:
            tp += 1
        elif not expect_val and pred:
            fp += 1
        elif expect_val and not pred:
            fn += 1
        else:
            tn += 1
    n = tp + fp + fn + tn
    prec = tp / (tp + fp) if (tp + fp) else 0.0
    rec = tp / (tp + fn) if (tp + fn) else 0.0
    f1 = 2 * prec * rec / (prec + rec) if (prec + rec) else 0.0
    spec = tn / (tn + fp) if (tn + fp) else 0.0
    return {
        "n": n, "tp": tp, "fp": fp, "fn": fn, "tn": tn,
        "precision": round(prec, 3), "recall": round(rec, 3), "f1": round(f1, 3),
        "specificity": round(spec, 3),
        "fpr": round(fp / (fp + tn), 3) if (fp + tn) else 0.0,
        "fnr": round(fn / (fn + tp), 3) if (fn + tp) else 0.0,
        "recall_ci": wilson(tp, tp + fn), "fpr_ci": wilson(fp, fp + tn),
    }


# ─────────────────────── component attribution ──────────────────────────
# Family for every check_name the application can actually emit. Sourced from
# the crate that constructs each verdict (grep, not guess):
#   fast-path/src/checks/*.rs ................................ fast_path
#   shadow-analysis/src/laya_client.rs (JUDGE_DETECTOR_PREFIX) judge_laya
#   shadow-analysis/src/{prompt_injection,verbosity,bias,groundedness,
#                        semantic_pii}.rs + worker input-* rewrites
#                                                              shadow_heuristic
#   shadow-analysis/src/guardrails_client.rs ................. guardrails_sidecar
# An unknown name is reported as "unclassified", never guessed into a family.
FAST_PATH_CHECKS = {
    "unsafe_content", "secret_detection", "cost_cap",
    "retry_detection", "tool_use_detection", "session_risk_accumulator",
}
SHADOW_HEURISTIC_CHECKS = {
    "prompt_injection", "semantic_pii", "verbosity",
    "bias_classification", "groundedness", "input-toxicity", "input-bias",
}
GUARDRAILS_SIDECAR_CHECKS = {
    "presidio-pii", "llm-guard-toxicity", "llm-guard-bias",
    "deepeval-hallucination",
}
JUDGE_PREFIX = "laya-"
EVIDENCE_SUFFIX = "-evidence"
COMPONENTS = ("judge_laya", "fast_path", "shadow_heuristic", "guardrails_sidecar")
# App severity order (checks-inventory.md §0): Pass < Escalate < Edit < Block.
SEVERITY = {"pass": -1, "escalate": 0, "edit": 1, "block": 2}


def component_of(check_name: str) -> str:
    base = (check_name[:-len(EVIDENCE_SUFFIX)]
            if check_name.endswith(EVIDENCE_SUFFIX) else check_name)
    if base.startswith(JUDGE_PREFIX):
        return "judge_laya"
    if base in FAST_PATH_CHECKS:
        return "fast_path"
    if base in GUARDRAILS_SIDECAR_CHECKS:
        return "guardrails_sidecar"
    if base in SHADOW_HEURISTIC_CHECKS:
        return "shadow_heuristic"
    return "unclassified"


def attribute(row: dict) -> dict:
    """Which component(s) contributed, and which set the worst action.

    `deciders` is the family set that produced the case's max-severity action.
    This is read off the recorded verdicts; it does NOT re-run the aggregator,
    so it describes *contributors*, not the fusion engine's internal weights.
    """
    fired = [d for d in (row.get("fired_detail") or [])
             if SEVERITY.get(d.get("outcome"), -1) >= 0]
    if not fired:
        return {"worst_severity": "pass", "deciders": [], "all_families": []}
    sev = max(SEVERITY.get(d["outcome"], -1) for d in fired)
    sev_name = {v: k for k, v in SEVERITY.items()}[sev]
    deciders = sorted({component_of(d["check_name"]) for d in fired
                       if SEVERITY.get(d["outcome"], -1) == sev})
    families = sorted({component_of(d["check_name"]) for d in fired})
    return {"worst_severity": sev_name, "deciders": deciders,
            "all_families": families}


def step_metrics(args: argparse.Namespace) -> None:
    out = pathlib.Path(args.out)
    rows = [json.loads(l) for l in (out / "results-enriched.jsonl").read_text().splitlines()
            if l.strip()]
    # one row per logical case: collapse retry repeats by worst observed action
    by_case: dict[str, dict] = {}
    for r in rows:
        cur = by_case.get(r["id"])
        if cur is None:
            by_case[r["id"]] = dict(r)
        else:
            cur["blocked"] = cur["blocked"] or r["blocked"]
            cur["redacted"] = cur["redacted"] or r["redacted"]
            cur["detected"] = cur["detected"] or r["detected"]
            cur["n_escalation_cases"] = max(cur["n_escalation_cases"], r["n_escalation_cases"])
            cur["worst_outcome"] = (r["worst_outcome"] if cur["worst_outcome"] == "pass"
                                    else cur["worst_outcome"])
            cur["judge_total"] = max(cur.get("judge_total", 0), r.get("judge_total", 0))
            cur["judge_nonpass"] = max(cur.get("judge_nonpass", 0), r.get("judge_nonpass", 0))
            merged = {(d["check_name"], d["outcome"]): d
                      for d in (cur.get("fired_detail") or [])}
            for d in (r.get("fired_detail") or []):
                merged[(d["check_name"], d["outcome"])] = d
            cur["fired_detail"] = list(merged.values())
            cur["fired_checks"] = sorted({d["check_name"] for d in cur["fired_detail"]})
    cases = list(by_case.values())
    for c in cases:
        c["attrib"] = attribute(c)

    def group(keyfn):
        g: dict[str, list[dict]] = {}
        for c in cases:
            g.setdefault(keyfn(c), []).append(c)
        return g

    report = {
        "requests": len(rows),
        "cases": len(cases),
        "outcomes": {
            "blocked": sum(1 for c in cases if c["blocked"]),
            "redacted": sum(1 for c in cases if c["redacted"]),
            "allowed": sum(1 for c in cases
                           if not c["blocked"] and not c["redacted"] and not c["error"]),
            "transport_or_error": sum(1 for c in cases if c["error"] or c["http_code"] == 0),
            "escalation_cases_created": sum(1 for c in cases if c["n_escalation_cases"] > 0),
            "detected_any": sum(1 for c in cases if c["detected"]),
            "unlabelled_unknown": sum(1 for c in cases if c["gold_action"] == "unknown"),
        },
        # headline matrices
        "detection": {
            "overall": confusion(cases, lambda r: r["detected"]),
            "by_domain": {k: confusion(v, lambda r: r["detected"]) for k, v in group(lambda r: r["domain"]).items()},
            "by_category": {k: confusion(v, lambda r: r["detected"])
                            for k, v in sorted(group(lambda r: r["category"]).items())},
        },
        # Blocking uses its OWN positive class: only gold_action == "block"
        # requires an HTTP 403. Detected-but-not-blocked is not a false negative
        # here if the gold action was escalate/redact/allow.
        "blocking": {
            "overall": confusion(cases, lambda r: r["blocked"],
                                 expect=lambda r: r["gold_action"] == "block"),
            "by_domain": {k: confusion(v, lambda r: r["blocked"],
                                        expect=lambda r: r["gold_action"] == "block")
                          for k, v in group(lambda r: r["domain"]).items()},
            "by_category": {k: confusion(v, lambda r: r["blocked"],
                                          expect=lambda r: r["gold_action"] == "block")
                            for k, v in sorted(group(lambda r: r["category"]).items())},
            "gold_block_cases": sorted(c["id"] for c in cases
                                       if c["gold_action"] == "block"),
            "blocked_cases": sorted(c["id"] for c in cases if c["blocked"]),
        },
        "redaction": {
            "overall": confusion([c for c in cases if c["gold_action"] in ("redact", "allow")],
                                 lambda r: r["redacted"]),
        },
        "latency_ms": {
            scope: {
                "n": len(v), "min": min(x["latency_ms"] for x in v),
                "p50": pct([x["latency_ms"] for x in v], 0.50),
                "p90": pct([x["latency_ms"] for x in v], 0.90),
                "p95": pct([x["latency_ms"] for x in v], 0.95),
                "p99": pct([x["latency_ms"] for x in v], 0.99),
                "max": max(x["latency_ms"] for x in v),
                "mean": round(sum(x["latency_ms"] for x in v) / len(v), 1),
            }
            for scope, v in ([("overall", cases)]
                             + list(group(lambda r: r["domain"]).items()))
        },
        # who decided: which component produced the case's max-severity action
        "attribution": {
            "decider_census": {
                fam: sum(1 for c in cases if c["attrib"]["deciders"] == [fam])
                for fam in COMPONENTS},
            "decider_census_incl_ties": {
                fam: sum(1 for c in cases if fam in c["attrib"]["deciders"])
                for fam in COMPONENTS},
            "worst_severity_census": {
                sev: sum(1 for c in cases if c["attrib"]["worst_severity"] == sev)
                for sev in ("pass", "escalate", "edit", "block")},
            # "what would this component alone have caught" -- detection matrix
            # restricted to one family's fired checks
            "by_component": {
                fam: confusion(cases, lambda r, f=fam: f in r["attrib"]["all_families"])
                for fam in COMPONENTS},
            "judge_abstention": {
                "no_judge_reading": sum(1 for c in cases if c["judge_total"] == 0),
                "judge_nonpass_0": sum(1 for c in cases if c["judge_nonpass"] == 0),
                "judge_read_pass": sum(1 for c in cases
                                       if c["judge_total"] > 0 and c["judge_nonpass"] == 0),
            },
            # same question at request granularity -- a case with repeats can be
            # judged on one attempt and not another, which case-collapsing hides
            "judge_abstention_requests": {
                "requests": len(rows),
                "no_judge_reading": sum(1 for r in rows if r.get("judge_total", 0) == 0),
                "judge_read_pass": sum(1 for r in rows if r.get("judge_total", 0) > 0
                                       and r.get("judge_nonpass", 0) == 0),
            },
            # judge fired but a non-judge component set the worst action, or the
            # judge is one of several tied deciders
            "judge_fired_but_not_decider": [
                {"id": c["id"], "gold_action": c["gold_action"],
                 "deciders": c["attrib"]["deciders"],
                 "judge_checks": [d["check_name"] for d in c["fired_detail"]
                                  if component_of(d["check_name"]) == "judge_laya"],
                 "decider_checks": [d["check_name"] for d in c["fired_detail"]
                                    if component_of(d["check_name"]) in c["attrib"]["deciders"]]}
                for c in cases
                if c["judge_nonpass"] > 0
                and "judge_laya" not in c["attrib"]["deciders"]],
            "multi_family_deciders": [
                {"id": c["id"], "gold_action": c["gold_action"],
                 "deciders": c["attrib"]["deciders"],
                 "worst_severity": c["attrib"]["worst_severity"]}
                for c in cases if len(c["attrib"]["deciders"]) > 1],
            "unclassified_checks": sorted({
                d["check_name"] for c in cases for d in c["fired_detail"]
                if component_of(d["check_name"]) == "unclassified"}),
        },
        "per_case": [{k: c[k] for k in (
            "id", "domain", "category", "gold_action", "http_code", "blocked",
            "redacted", "detected", "worst_outcome", "fired_checks",
            "n_escalation_cases", "matched_call", "token_out")}
            | {"deciders": c["attrib"]["deciders"],
               "fired_families": c["attrib"]["all_families"],
               "judge_total": c["judge_total"],
               "judge_nonpass": c["judge_nonpass"]}
            for c in cases],
    }
    (out / "metrics.json").write_text(json.dumps(report, indent=2, sort_keys=True))

    print(f"\n=== outcomes ({report['cases']} cases / {report['requests']} requests) ===")
    for k, v in report["outcomes"].items():
        print(f"  {k:26s} {v}")
    print("\n=== detection (any check fired) ===")
    for scope, m in list(report["detection"]["by_domain"].items()) + [("OVERALL", report["detection"]["overall"])]:
        print(f"  {scope:16s} n={m['n']:<3} tp={m['tp']:<3} fp={m['fp']:<3} "
              f"fn={m['fn']:<3} tn={m['tn']:<3} prec={m['precision']:.3f} "
              f"rec={m['recall']:.3f} fpr={m['fpr']:.3f} rec_ci={m['recall_ci']}")
    print("\n=== blocking (HTTP 403) ===")
    for scope, m in list(report["blocking"]["by_domain"].items()) + [("OVERALL", report["blocking"]["overall"])]:
        print(f"  {scope:16s} n={m['n']:<3} tp={m['tp']:<3} fp={m['fp']:<3} "
              f"fn={m['fn']:<3} tn={m['tn']:<3} prec={m['precision']:.3f} rec={m['recall']:.3f}")
    print("\n=== latency (ms, end-to-end) ===")
    for scope, m in report["latency_ms"].items():
        print(f"  {scope:16s} n={m['n']:<3} min={m['min']:<6} p50={m['p50']:<6} "
              f"p90={m['p90']:<6} p95={m['p95']:<6} p99={m['p99']:<6} max={m['max']:<6} mean={m['mean']}")

    at = report["attribution"]
    print("\n=== component attribution (who set the worst action) ===")
    for fam in COMPONENTS:
        m = at["by_component"][fam]
        print(f"  {fam:20s} decided={at['decider_census'][fam]:<3} "
              f"contributed_to={at['decider_census_incl_ties'][fam]:<3} "
              f"n={m['n']:<3} tp={m['tp']:<3} fp={m['fp']:<3} fn={m['fn']:<3} "
              f"prec={m['precision']:.3f} rec={m['recall']:.3f} fpr={m['fpr']:.3f}")
    ja, jr = at["judge_abstention"], at["judge_abstention_requests"]
    print(f"  judge abstention (cases): no_reading={ja['no_judge_reading']} "
          f"read_but_all_pass={ja['judge_read_pass']} nonpass_0={ja['judge_nonpass_0']}")
    print(f"  judge abstention (requests): no_reading={jr['no_judge_reading']}/{jr['requests']} "
          f"read_but_all_pass={jr['judge_read_pass']}")
    print(f"  worst-severity census: {at['worst_severity_census']}")
    if at["unclassified_checks"]:
        print(f"  !! unclassified check names: {at['unclassified_checks']}")
    print(f"\n-> {out/'metrics.json'}")


def step_compare(args: argparse.Namespace) -> None:
    """Baseline (judge OFF) vs post-Laya (judge ON) on the same cases."""
    off = json.loads((pathlib.Path(args.off) / "metrics.json").read_text())
    on = json.loads((pathlib.Path(args.on) / "metrics.json").read_text())

    def row(label, a, b):
        return (f"  {label:24s} {a:>10} {b:>10}  {b - a:+.3f}")

    lines = ["=== detection: judge OFF (baseline) vs judge ON (post-Laya) ===",
             f"  {'metric':24s} {'OFF':>10} {'ON':>10}  {'delta':>8}"]
    d_off, d_on = off["detection"]["overall"], on["detection"]["overall"]
    for k in ("n", "tp", "fp", "fn", "tn"):
        lines.append(f"  {k:24s} {d_off[k]:>10} {d_on[k]:>10}  {d_on[k] - d_off[k]:+8d}")
    for k in ("precision", "recall", "f1", "specificity", "fpr", "fnr"):
        lines.append(row(k, d_off[k], d_on[k]))
    lines.append(f"  {'recall_ci':24s} {d_off['recall_ci']:>10} {d_on['recall_ci']:>10}")
    lines.append(f"  {'fpr_ci':24s} {d_off['fpr_ci']:>10} {d_on['fpr_ci']:>10}")

    lines += ["", "=== blocking (HTTP 403; positive class = gold_action 'block') ===",
              f"  {'metric':24s} {'OFF':>10} {'ON':>10}  {'delta':>8}"]
    b_off, b_on = off["blocking"]["overall"], on["blocking"]["overall"]
    for k in ("n", "tp", "fp", "fn", "tn"):
        lines.append(f"  {k:24s} {b_off[k]:>10} {b_on[k]:>10}  {b_on[k] - b_off[k]:+8d}")
    for k in ("precision", "recall", "f1", "specificity", "fpr", "fnr"):
        lines.append(row(k, b_off[k], b_on[k]))
    lines.append(f"  {'recall_ci':24s} {b_off['recall_ci']:>10} {b_on['recall_ci']:>10}")
    lines.append(f"  gold_block_cases: {off['blocking']['gold_block_cases']}")
    lines.append(f"  blocked_cases:    OFF={off['blocking']['blocked_cases']} "
                 f"ON={on['blocking']['blocked_cases']}")

    lines += ["", "=== per-domain detection precision / fp-rate ==="]
    for dom in sorted(off["detection"]["by_domain"]):
        a, b = off["detection"]["by_domain"][dom], on["detection"]["by_domain"][dom]
        lines.append(f"  {dom:16s} prec {a['precision']:.3f} -> {b['precision']:.3f}"
                     f"   fpr {a['fpr']:.3f} -> {b['fpr']:.3f}   rec {a['recall']:.3f} -> {b['recall']:.3f}")

    lines += ["", "=== cases whose detected flag changed ==="]
    a_by = {c["id"]: c for c in off["per_case"]}
    changed = 0
    for c in on["per_case"]:
        a = a_by.get(c["id"])
        if a and a["detected"] != c["detected"]:
            changed += 1
            lines.append(f"  {c['id']:22s} gold={c['gold_action']:9s} "
                         f"detected {a['detected']} -> {c['detected']}  fired_on={c['fired_checks']}")
    if not changed:
        lines.append("  (none)")
    lines += ["", "=== component attribution: which family set the worst action ===",
              f"  {'family':20s} {'OFF decided':>12} {'ON decided':>12}"]
    ao, an = off["attribution"], on["attribution"]
    for fam in COMPONENTS:
        lines.append(f"  {fam:20s} {ao['decider_census'][fam]:>12} "
                     f"{an['decider_census'][fam]:>12}")
    lines += ["", "=== per-component detection (that family's fired checks only) ===",
              f"  {'family':20s} {'OFF prec':>9} {'ON prec':>9} {'OFF rec':>9} "
              f"{'ON rec':>9} {'OFF fpr':>9} {'ON fpr':>9}"]
    for fam in COMPONENTS:
        x, y = ao["by_component"][fam], an["by_component"][fam]
        lines.append(f"  {fam:20s} {x['precision']:>9.3f} {y['precision']:>9.3f} "
                     f"{x['recall']:>9.3f} {y['recall']:>9.3f} "
                     f"{x['fpr']:>9.3f} {y['fpr']:>9.3f}")
    lines += ["", "=== judge abstention (must never be read as 'allowed') ==="]
    for label, k in (("no judge reading at all", "no_judge_reading"),
                     ("judge read, all readings pass", "judge_read_pass")):
        lines.append(f"  {label:32s} OFF={ao['judge_abstention'][k]:<3} "
                     f"ON={an['judge_abstention'][k]:<3}")
    lines += ["", "=== disagreements (judge fired but did not set the worst action) ==="]
    dis = an["judge_fired_but_not_decider"]
    for d in dis:
        lines.append(f"  {d['id']:22s} gold={d['gold_action']:9s} "
                     f"deciders={d['deciders']} judge={d['judge_checks']}")
    if not dis:
        lines.append("  (none)")
    ties = an["multi_family_deciders"]
    lines += [f"  tied multi-family deciders: {len(ties)}" +
              (f" -> {[(t['id'], t['deciders']) for t in ties]}" if ties else "")]

    lines += ["", "=== end-to-end latency ==="]
    for scope in ("overall",):
        a, b = off["latency_ms"][scope], on["latency_ms"][scope]
        lines.append(f"  {scope:16s} p50 {a['p50']} -> {b['p50']} ms   "
                     f"p95 {a['p95']} -> {b['p95']} ms   max {a['max']} -> {b['max']} ms")

    text = "\n".join(lines)
    print(text)
    (pathlib.Path(args.out) / "comparison.txt").write_text(text + "\n")


def main() -> None:
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)

    t = sub.add_parser("traffic"); t.add_argument("--proxy", default="http://127.0.0.1:8901")
    t.add_argument("--out", required=True); t.add_argument("--timeout", type=int, default=90)
    t.set_defaults(fn=step_traffic)

    e = sub.add_parser("enrich"); e.add_argument("--out", required=True)
    e.add_argument("--db", default="postgres://controlplane:secret@localhost:5432/controlplane")
    e.set_defaults(fn=step_enrich)

    m = sub.add_parser("metrics"); m.add_argument("--out", required=True)
    m.set_defaults(fn=step_metrics)

    c = sub.add_parser("compare")
    c.add_argument("--off", required=True); c.add_argument("--on", required=True)
    c.add_argument("--out", required=True)
    c.set_defaults(fn=step_compare)

    a = ap.parse_args()
    a.fn(a)


if __name__ == "__main__":
    main()
