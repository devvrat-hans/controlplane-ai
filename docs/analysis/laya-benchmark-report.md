# Laya / hybrid-judge benchmark report

> **Status: PARTIAL — baseline AND post-Laya latency/reliability measured;
> post-Laya decision quality still NOT measurable.**
>
> Measured 2026-09-26/27 on `feature/mcp-server` @ `6a105d4`. Every number was
> produced by a command in §9; where a measurement could not be taken, the cell
> says **not measurable**. Nothing here is estimated or carried over from
> documentation. A judge-enabled gateway was stood up locally for this benchmark
> (see the Appendix), so post-Laya latency and reliability are real measurements
> — the earlier "post-Laya is not measurable" position applied only while no
> judge was reachable.
>
> Raw results: `benchmark-results/`.

---

## 1. Executive summary and conclusion

**Conclusion: the judge now runs, and what it costs is measurable — what it buys
is not.** Laya measurably raises client-visible latency by ~50 ms mean / +16 % p50
on a shared 12-core laptop, its own latency p50 (2 359 ms) is already above the
documented 2 s shadow-path budget, and 26 of 40 calls received **no judge verdict
at all**. Whether it improves decisions remains unmeasured, because there are still
zero human labels on any judge-on call.

What was established, before and after standing the judge up:

| Question | Answer | Evidence |
|---|---|---|
| Does a Laya-enabled deployment exist? | **Yes — as of this run** | local gateway reported `decision_judge: "laya"`, `decision_judge_configured: true` |
| Has the judge produced verdicts? | **Yes — 59**, across 6 detectors and 44 calls | `SELECT count(*) ... LIKE 'laya-%'` |
| Is the hybrid fusion active? | **Still no** | `detector_calibration`: 16 rows, 0 with `calibrated = TRUE` |
| Can the pre-Laya baseline be measured? | **Yes** | 1138 verdicts / 1114 calls / 25 reviewer labels |
| Does Laya touch the synchronous path? | **Not architecturally — but it slows the client anyway** | fast-path 2.3–19 µs; end-to-end +50 ms mean (§4.4) |
| Did anything regress? | **No** | 514 tests pass, 0 fail (§9.2) |

What the baseline measurement shows is uncomfortable and worth stating: on the 25
labelled cases that exist, the **heuristic-only baseline has a pooled false-positive
rate of 0.684** (13 false positives against 19 clean pairs) at the default 0.70
threshold, with recall 0.500. That is the number Laya was meant to improve. It
remains uncomparable, but for a narrower reason than before: the judge now runs,
yet **none of the judge-on calls have been human-reviewed**, so there is no ground
truth to score either engine against. **Nothing here is evidence for or against
Laya's accuracy.** The 95 % interval on baseline recall is `[0.188, 0.812]` — wider
than any difference the integration was built to deliver.

---

## 2. Exact application and dependency changes attributable to Laya

No third-party crate was added for Laya. The judge is reached over plain HTTP by
the existing `reqwest` dependency, and the Python sidecar is a separate image.

| File | Laya-attributable content |
|---|---|
| `services/shadow-analysis/src/laya_client.rs` | Jev-compatible client; 8 detector scales; batching, max-8 response windows, max-pooling; fail-open; band classification |
| `services/shadow-analysis/src/governance_questions.rs` | Question schema + `QUESTION_SCALES`; emits `laya-*` verdict names |
| `services/shadow-analysis/src/calibration.rs` | Temperature scaling, option bucketing, weight clamping |
| `services/shadow-analysis/src/worker.rs` | Judge invocation on the shadow path; `demote_semantic_pii` interaction |
| `services/shadow-analysis/src/toggles.rs`, `types.rs`, `lib.rs` | `DECISION_JUDGE` / `LAYA_*` config, exported types |
| `services/decision/src/aggregator.rs` | Weighted noisy-OR fusion, corroboration rule, disagreement routing |
| `services/gateway/src/main.rs` | Calibration reloader wiring, judge startup logging |
| `services/dashboard-api/src/router.rs` | Judge-agreement + calibration surface |
| `infra/migrations/023_create_detector_calibration.sql` | `detector_calibration` table (16 seed rows, all inert) |
| `infra/migrations/024_add_decision_judge_toggle.sql` | Per-policy `decision_judge_enabled` |
| `services/laya/Dockerfile` | The sidecar image (`laya[serve]`), `judge` compose profile, not started by default |
| `docker-compose.yml`, `docker-compose.gpu.yml` | `laya` service behind the `judge` profile |
| `services/fast-path/tests/contract_no_judge_in_fast_path.rs` | Pins the no-LLM-in-fast-path invariant |
| `services/gateway/tests/{laya_judge_contract_test,hybrid_pipeline_test}.rs`, `common/mod.rs` | Real client over a real socket against a scripted `FakeLaya` server |
| `scripts/eval_accuracy.sh` | The ablation harness used in §4 |
| `scripts/bench_laya.sh` | **Added by this benchmark run** — reproducible driver |

**Environment variables that switch the judge on** (all read at startup):
`DECISION_JUDGE=laya|jev`, `LAYA_URL`, `LAYA_TIMEOUT_MS` (default 5000), `LAYA_MODEL`
(default `auto`), `LAYA_API_KEY` (optional), `ENABLE_INTERNAL_SCANS` unrelated.
Per-app gating: policy `checks.decision_judge_enabled`.

**Defaults, as observed in the running system:** `decision_judge = off`,
`decision_judge_configured = false`, `decision_judge_url = null`,
`calibration_version = null`, `fusion_enabled = false`.

---

## 3. Architecture and request flow

### 3.1 Where Laya sits (shadow only)

```mermaid
flowchart LR
  C[Client] --> P[proxy :8900]
  P -->|fail-open, <10ms| FP[fast-path]
  FP -->|Outcome| P
  P -->|response| C
  P -.->|intercept.captured| CA[cost-accounting]
  P -.->|intercept.shadow| SH[shadow-analysis]

  subgraph SHADOW["shadow path (async, <2s, never blocks)"]
    SH --> H1[native heuristics]
    SH --> H2[groundedness / verbosity / pii]
    SH --> GC[guardrails sidecar :8200]
    SH --> J{{LayaClient}}
    J -->|POST /v1/systemone| LY[Laya sidecar :8300]
    J -.->|unreachable / timeout / bad JSON| FO[ZERO laya-* verdicts = pass]
  end

  H1 --> AGG[decision :8080 aggregator]
  H2 --> AGG
  GC --> AGG
  J -->|laya-* verdicts| AGG
  AGG --> AUD[audit] & ESC[escalation] & NOT[notification] & DASH[dashboard]
```

### 3.2 The two combination stages, and their single gate

```mermaid
flowchart TD
  A["raw judge scores (per detector)"] --> S1{"Stage 1: temperature scaling"}
  W1["detector_calibration.temperature WHERE calibrated = TRUE"] --> S1
  S1 -->|no fitted row: identity, T = 1.0| V["verdicts (confidence = p)"]
  S1 -->|fitted: p = sigmoid(logit(p)/T)| V
  V --> S2{"Stage 2: weighted noisy-OR (per axis)"}
  W2["detector_calibration.weight WHERE calibrated = TRUE"] --> S2
  S2 -->|weights empty| LEG["legacy aggregator: worst-of + compound risk"]
  S2 -->|weights present| FUS["p_axis = 1 - PROD(1 - w_d * p_d)"]
  FUS --> BANDS["p >= 0.90 Escalate / p >= 0.70 Edit / else Pass"]
  BANDS --> CORR["corroboration: lone heuristic capped at Edit; judge may act alone at >= 0.90"]
  CORR --> DIS["|heuristic p - judge p| >= 0.40  =>  route to human"]
  DIS --> FIN["final outcome"]
  LEG --> FIN
```

**Observed state (measured, not assumed):** the gate is closed. `calibrated = TRUE`
counts 0 rows, so Stage 1 is the identity and Stage 2 returns `None` — the fusion
never runs and the system behaves exactly as pre-Laya. The pre-Laya control path is
therefore not a historical build: it is the **current, live configuration**.

### 3.3 Which component decides what

| Detector family | Invoked by | In the decision path? | Calibrated? | Fired in the corpus? |
|---|---|---|---|---|
| `secret_detection`, `unsafe_content`, `cost_cap` | fast-path | yes, synchronous | no | yes (266 / 207 / 102) |
| `groundedness`, `bias_classification`, `prompt_injection`, `semantic_pii` | native heuristics | async only | no | yes |
| `presidio-*`, `llm-guard-*`, `input_bias`, `deepeval-hallucination` | guardrails sidecar | async only | no | yes |
| `laya-*` (8 detectors) | LayaClient | async only | stage 1 only | **never** |

---

## 4. Baseline versus post-Laya

### 4.1 Baseline — measured

Corpus: local PostgreSQL 16.15, 30/3650-day window, labels = latest
`reviewer_overrides` row per `(call_id, axis)`, `confirm` = positive.
Threshold 0.70 (the project default `JUDGE_EDIT_THRESHOLD`).

| Axis | Config | N | Prec | Rec | F1 | FP-rate | Brier | ECE | TP/FP |
|---|---|---|---|---|---|---|---|---|---|
| performance | heuristic-only | 10 | 0.333 | 0.500 | 0.400 | 0.250 | 0.350 | 0.447 | 1/2 |
| performance | fused | 10 | 0.250 | 0.500 | 0.333 | 0.375 | 0.452 | 0.537 | 1/3 |
| performance | judge-only | 10 | 0.000 | 0.000 | 0.000 | 0.000 | 0.200 | 0.200 | 0/0 |
| responsibility | heuristic-only | 15 | 0.154 | 0.500 | 0.235 | 1.000 | 0.622 | 0.699 | 2/11 |
| responsibility | fused | 15 | 0.154 | 0.500 | 0.235 | 1.000 | 0.644 | 0.711 | 2/11 |
| responsibility | judge-only | 15 | 0.000 | 0.000 | 0.000 | 0.000 | 0.267 | 0.267 | 0/0 |

`fused+calibrated` is numerically identical to `fused` in this run and is omitted
from the table above; that is expected, not a coincidence — with zero calibrated
rows every fitted parameter falls back to its identity value.

**`judge-only` rows are empty by construction, not by performance.** All 0/0 cells
mean the judge contributed no prediction to score, because it emitted no verdicts.
They are not evidence of conservative behaviour.

### 4.2 Post-Laya — latency and reliability measured, quality not

Judge enabled (`DECISION_JUDGE=laya`, `LAYA_URL=http://127.0.0.1:8000`), CPU
inference, English checkpoint, shadow path only.

| Metric | Measured value | n |
|---|---|---|
| Judge verdicts produced | 59 (6 detectors, 44 distinct calls) | 59 |
| Judge latency p50 | **2 359 ms** | 14 |
| Judge latency p95 / max | 4 928 ms / **4 953 ms** | 14 |
| Judge readings over the 2 s shadow budget | **8 of 14 (57 %)** | 14 |
| Calls receiving **no** judge verdict | **26 of 40 (65 %)** | 40 |
| Logged fail-open windows | 26 (8 + 18 across two arms) | 80 requests |
| Judge verdict mix | `laya-tool-use` 26, `laya-prompt-injection` 8, `laya-semantic-pii` 3, plus `-evidence` and `laya-verbosity` | 59 |
| Judge/heuristic comparable pairs | 1 | 1 |
| Disagreement rate (Δ ≥ 0.40) | 0.0 % | 1 |
| Precision / recall / F1 | **NOT measurable** | 0 labelled judge-on calls |
| Cost or tokens per judge call | **not measurable** | — |
| Change in block / escalate / edit rate vs baseline | **not measurable** | — |

Two caveats that matter more than the numbers:

1. **Judge latency here is CPU-only on a laptop.** The repo's own notes put a CPU
   forward pass at 200–460 ms versus ~33 ms on GPU, and the compose file ships a
   `judge` profile plus a GPU override for exactly this reason. A GPU deployment
   would move these figures substantially; they are not a general result.
2. **The 2 s breach count is small-sample.** 14 readings carried `duration_ms`; 8
   exceeded 2 s. `duration_ms` is populated for only a minority of judge verdicts
   (§7.2), so the true breach rate is unmeasured, not 57 % of all judge calls.

**Why quality is still unmeasurable.** The ablation scores configurations against
`reviewer_overrides`. All 25 labels predate this run (2026-08-20 → 08-26) and none
covers a judge-on call (2026-09-27). The `judge-only` row is therefore still empty —
now because there is no ground truth for the judge's calls, not because the judge
was silent. Producing a post-Laya accuracy figure requires human review of judge-on
traffic, which is a labelling task, not a benchmark run.

### 4.3 The one comparison that *is* available

The remote deployment (reachable through an existing SSH tunnel) runs the same
software with the judge off. Its judge-agreement surface, 30-day window:

| Metric | Value |
|---|---|
| comparable (call, axis) pairs | 0 |
| judge-flagged | 0 |
| heuristic-flagged | 534 |
| disagreements | 0 |
| `agreement_rate` reported | 1.0 |
| `calibration_version` | `null` |
| `raw_probabilities` | `true` |

**`agreement_rate = 1.0` here is an artefact of the denominator being zero**, not
agreement. The remote surface is the same shape as the local one: heuristics vote,
the judge is silent.

### 4.4 End-to-end client latency: judge ON vs OFF (paired, same inputs)

Three arms, identical 40-request pool in identical order, run back to back on the
same host. A and C are the judge on; B is the judge off. Client latency is measured
at the proxy boundary.

| Arm | Judge | n | min | p50 | p95 | p99 | max | HTTP 200 | errors |
|---|---|---|---|---|---|---|---|---|---|
| A | **ON** (run 1) | 40 | 143 ms | 352 ms | 575 ms | 2 676 ms | 2 676 ms | 40 | 0 |
| B | OFF (control) | 40 | 99 ms | 211 ms | 478 ms | 522 ms | 522 ms | 40 | 0 |
| C | **ON** (run 2) | 40 | 168 ms | 244 ms | 525 ms | 526 ms | 526 ms | 40 | 0 |

Paired by prompt index, **C vs B: the judge-on arm was slower on 34 of 40 requests**
(6 faster, 0 tied), **mean delta +50 ms**.

Honest reading of the three arms:

- **The reproducible effect is B → C: +33 ms on p50 (+16 %), +47 ms on p95 (+10 %),
  mean +50 ms paired.** That is the number to quote.
- **Do not quote A's 2 676 ms tail as a judge effect.** A ran first, when the model
  and OS caches were cold; C reproduces neither the p50 352 ms nor the tail. One
  outlier is not a distribution, so the "judge adds a 5× p99 tail" claim is not
  supported by this data.
- **The mechanism is contention, not the request path.** The judge is architecturally
  off-path (it runs after the response is delivered and its absence is defined as
  pass), yet it still costs the client ~50 ms because the Rust proxy, Ollama and the
  CPU-bound judge share 12 cores. This is a real cost that the contract's
  "shadow-path, non-blocking" framing does not capture — non-blocking is not free.

---

## 5. Results by workflow, domain, payload size, concurrency, warm/cold state

| Dimension | Status | Measured value |
|---|---|---|
| Policy evaluation | **partial** | fast-path bench §7; live `avg_fast_path_latency_ms` = 0.0332 ms (remote, 24 h window) |
| PII detection / redaction | **partial** | fast-path regex PII sample: 3.04 µs (§7). Sidecar/`laya-semantic-pii` quality: not measurable |
| Hallucination checks | **not measurable** | `deepeval-hallucination` fired 16× in the corpus but has no label-level ground truth |
| Citation checks | **does not exist** | no citation detector is implemented anywhere in the repo |
| Shadow-path analysis | **partial** | detectors fire (§9.1 census); shadow latency measured for 8 of 375 shadow verdicts: 350–460 ms, avg 408 ms (§7.2) |
| Hybrid routing / fallback | **partial** | judge-on and judge-off arms measured (§4.4); 26 fail-open windows logged (§8). Fusion gate still closed |
| Domain variation (healthcare / financial / custom) | **not measurable** | the label table carries no domain dimension; profiles exist but are not represented in the 25 labels |
| Payload size | **measured (fast-path only)** | 4 KB response = 19.04 µs vs short clean = 2.34 µs (§7) |
| Concurrency / throughput / queueing | **not measured** | three *serial* 40-request arms only; no concurrency sweep (§10.6) |
| Cold start vs warm cache | **partially observed** | arm A (cold) p50 352 ms → arm C (warm) p50 244 ms on identical inputs (§4.4); not a designed cold-start test |

---

## 6. Quality analysis: confusion matrices and error categories

Threshold 0.70, positive = `confirm`. Intervals are 95% Wilson score intervals.

| Axis | Config | N | TP | FP | FN | TN | Recall [95% CI] | FP-rate [95% CI] | Precision |
|---|---|---|---|---|---|---|---|---|---|
| performance | heuristic-only | 10 | 1 | 2 | 1 | 6 | 0.500 [0.095, 0.905] | 0.250 [0.071, 0.591] | 0.333 |
| performance | fused | 10 | 1 | 3 | 1 | 5 | 0.500 [0.095, 0.905] | 0.375 [0.137, 0.694] | 0.250 |
| performance | judge-only | 10 | 0 | 0 | 2 | 8 | 0.000 [0.000, 0.658] | 0.000 [0.000, 0.324] | 0.000 |
| responsibility | heuristic-only | 15 | 2 | 11 | 2 | 0 | 0.500 [0.150, 0.850] | 1.000 [0.741, 1.000] | 0.154 |
| responsibility | fused | 15 | 2 | 11 | 2 | 0 | 0.500 [0.150, 0.850] | 1.000 [0.741, 1.000] | 0.154 |
| responsibility | judge-only | 15 | 0 | 0 | 4 | 11 | 0.000 [0.000, 0.490] | 0.000 [0.000, 0.259] | 0.000 |
| **pooled** | **heuristic-only** | 25 | 3 | 13 | 3 | 6 | **0.500 [0.188, 0.812]** | **0.684 [0.460, 0.846]** | **0.188** |
| pooled | fused | 25 | 3 | 14 | 3 | 5 | 0.500 [0.188, 0.812] | 0.737 [0.512, 0.882] | 0.176 |
| pooled | judge-only | 25 | 0 | 0 | 6 | 19 | 0.000 [0.000, 0.390] | 0.000 [0.000, 0.168] | 0.000 |

### Error categories (derived from the rows, not from inspection of payloads)

- **False positives dominate the `responsibility` axis**: 11 of 11 clean pairs are
  flagged (FP-rate 1.000). Every one of those is a clean call pushed into an action —
  the visible pain the fusion was designed to reduce.
- **Precision collapse (0.154)** follows directly: the axis flags almost everything,
  so a true positive is indistinguishable from noise at this threshold.
- **All three missed detections (FN)** are labelled `confirm` but scored below 0.70.
- Note `fused` is *worse* than `heuristic-only` on `performance` (FP 3 vs 2) in this
  run. That is a property of the *unfitted* noisy-OR admitting sub-threshold
  `-evidence` readings, not a Laya result — no judge reading exists in these rows.
- No raw prompts, responses, credentials, or payloads were read, logged, or included
  at any point. The confusion matrix is computed from `(label, score)` pairs alone.

---

## 7. Latency breakdown

### 7.1 Synchronous path (measured, criterion, M4 Pro, 100 samples, 1 s warm-up, 5 s measurement)

| Scenario | Lower bound | **Estimate** | Upper bound | Budget |
|---|---|---|---|---|
| `fast_path_clean_response` | 2.3317 µs | **2.3389 µs** | 2.3465 µs | <10 000 µs ✅ |
| `fast_path_secret_detection` | 2.1745 µs | **2.1805 µs** | 2.1871 µs | ✅ |
| `fast_path_pii_detection` | 3.0196 µs | **3.0356 µs** | 3.0587 µs | ✅ |
| `fast_path_unsafe_block` | 158.59 ns | **158.97 ns** | 159.38 ns | ✅ |
| `fast_path_4kb_response` | 18.927 µs | **19.041 µs** | 19.234 µs | ✅ |

Outlier counts: 5/100, 5/100, 2/100, 3/100, 1/100 per scenario (5% / 5% / 2% / 3% / 1%).
Dominant contributor: response-size-proportional scanning — the 4 KB case is ~8× the
short clean case, and both are three orders of magnitude inside the budget.

Cross-check against production traffic (remote deployment, 24 h): reported
`avg_fast_path_latency_ms` = **0.033175 ms**. Consistent with the microscope numbers.

### 7.2 Shadow path and Laya

| Segment | n | min | avg | max | over 2 s budget |
|---|---|---|---|---|---|
| fast-path detectors | 76 | 0 ms | 2 ms | 12 ms | 0 |
| shadow-path detectors | 8 | 350 ms | 408 ms | 460 ms | 0 |
| judge (`laya-*`), pre-benchmark corpus | **0** | — | — | — | — |
| judge (`laya-*`), this benchmark | **14** | 702 ms | 2 552 ms | 4 953 ms | **8** |

Per detector, among the rows that carry `duration_ms`:

| Family | Detector | n | min | avg | max |
|---|---|---|---|---|---|
| shadow | `groundedness` | 4 | 420 ms | 443 ms | 460 ms |
| shadow | `bias_classifier` | 4 | 350 ms | 373 ms | 390 ms |
| fast | `pii_detection` | 5 | 3 ms | 3 ms | 4 ms |
| fast | `fast-path-summary` | 9 | 0 ms | 2 ms | 12 ms |
| fast | `secret_detection` | 28 | 2 ms | 2 ms | 4 ms |
| fast | `unsafe_content` | 2 | 2 ms | 2 ms | 2 ms |
| fast | `cost_cap` | 31 | 1 ms | 1 ms | 1 ms |
| fast | `retry_detection` | 1 | 1 ms | 1 ms | 1 ms |

**Coverage is the caveat:** in the pre-benchmark corpus only 84 of 1138 verdicts
carry `duration_ms` (76/763 fast, 8/375 shadow). After the judge-on runs, 14 of the
59 `laya-*` verdicts carry it. Treat every figure here as indicative of magnitude,
not as a distribution.

| Segment | Measured |
|---|---|
| Fast-path share of end-to-end | 0.0332 ms (measured remotely) |
| Laya model forward pass (CPU) | 702 ms – 4 953 ms per judged call; **p50 2 359 ms**, p95 4 928 ms |
| Judge verdicts over the 2 s budget | **8 of 14 readings (57 %)** |
| Fusion compute (`fuse_evidence`) | **not measurable** — returns `None` with no fitted weights |
| Client-visible cost of enabling the judge | **+50 ms mean, +33 ms p50 (+16 %)** (§4.4) |

### 7.3 Does Laya add latency? Yes — measured, and not where the contract implies

The judge does not sit on the request path: it runs after the response is delivered
and an absent verdict is defined as pass. That remains true and is contract-pinned.

But the *client* still waits ~50 ms longer on average with the judge on (§4.4),
because the CPU-bound judge contends with the proxy and Ollama on the same host.
The two statements are consistent: **off the request path is not the same as free of
cost.** Two dominated contributors, in order:

1. **Model forward pass, CPU-bound: 702 ms – 4 953 ms per judged call**, p50
   2 359 ms — already past the documented 2 s shadow budget.
2. **CPU contention with the synchronous path: ~+50 ms mean** client-visible.

Not measured: the judge's own contribution on GPU (the repo's notes claim ~33 ms),
timeout rate (the client aborts at `LAYA_TIMEOUT_MS` = 5 000 ms, and the observed
max 4 953 ms sits just under it), and retry behaviour (none is implemented).

**Does Laya add latency?** Unknown from measurement. Structurally it cannot affect
the client-visible response: it runs after the response is delivered, and its
absence is defined as pass. That is a contract property (pinned by
`the_fast_path_has_no_route_to_the_judge` and the fast-path tests), not a
measurement, and this report does not present it as one.

---

## 8. Resource, cost, risk, failure behaviour

**Resource utilisation (CPU / memory / network / provider usage): not measured.**
No container was started, so there is no cgroup data to report.

**Cost / token usage:** the corpus records token counts (`cost_cap`,
`token_budget`, `cost_entries` all fire), but the judge does not report token usage
in its verdicts, so there is no per-judge-call token or currency figure to compute.
Projecting one would be fabrication. The measurable cost of the judge is **CPU time
and latency**, not tokens (§7.2, §8 items 1–3).

### Operationally significant risks found while running this benchmark

1. **The judge silently drops most of its work under load.** Across two 40-request
   arms the gateway logged **26 fail-open windows** (8 + 18; `Laya judge unreachable
   — FAIL OPEN for this window`), and in the second arm **26 of 40 calls received no
   judge verdict at all**. Because "absence of a shadow verdict = pass" is the
   documented contract, a judge that is overloaded degrades into one that quietly
   agrees with everything. This is the single most important operational finding here.

   > The two 26s are different quantities that happen to coincide — windows that
   > failed open, versus calls with no judge reading. Do not read one as the other;
   > the second is the union of fail-open and legitimate sub-floor discards.
   *Caveat:* silence has two causes that the verdict table cannot distinguish —
   logged fail-open, and legitimate sub-0.45 evidence-floor discards. The 26 logged
   warnings prove the former occurred; the split between them is not measured.
2. **The judge already breaches the documented shadow-path budget on CPU.** 8 of 14
   instrumented readings exceeded 2 s, p50 2 359 ms, max 4 953 ms — inside a 5 s
   client timeout, so the breach surfaces as latency rather than an error. The repo's
   own notes claim ~200–460 ms per CPU forward pass, so either this host is slower
   than assumed, or a judged call aggregates several windows (up to 8 are allowed).
3. **Enabling the judge slows the synchronous path by ~50 ms mean (+16 % p50)** even
   though it is architecturally off-path (§4.4). Non-blocking is not free when the
   judge and the request path share a CPU.
4. **The judge is unexercised against human ground truth.** 59 verdicts now exist,
   but not one has been reviewed, so nothing here validates the judge's *decisions*,
   only its runtime behaviour.
2. **The labelled corpus is far too small to gate a rollout.** 25 labels, 6 positives
   on the pooled set; the CI on recall spans 0.19–0.81. `eval_accuracy.sh` refuses to
   fit without labelled judge verdicts — correct, but it means the fusion cannot be
   activated from this data at all.
3. **The default configuration is inert in a way that is easy to misread.** `fused`
   and `heuristic-only` differ on `performance` (FP 3 vs 2) even with the judge
   completely silent, because unfitted `-evidence` readings enter the noisy-OR. An
   operator seeing a `fused` number may believe the judge participated. It did not.
4. **DB-backed integration tests are not isolated from real data.** With
   `DATABASE_URL` set, `db_integration_test` / `api_integration_test` run against
   whatever database it names and fail on FK/constraint fixtures when that database
   holds real rows. Observed failure counts varied between runs (6, then 1) on the
   same code — order- and state-dependent, not deterministic. §9.2.
5. **`strip = "symbols"` breaks the release/bench profile on macOS 27.** The host
   proc-macro dylib comes out corrupt (`mis-aligned LINKEDIT string pool`) and no
   benchmark can link. Reproduced from a clean target dir, so it is deterministic,
   not a cache fluke. Worked around with `CARGO_PROFILE_BENCH_STRIP=none`.

**Failure behaviour — now observed against a real Laya process, not just asserted
by tests.** The fail-open path fired 26 times in production-shaped traffic and
behaved exactly as designed: the affected windows produced **no** `laya-*` verdicts,
the request still returned HTTP 200, and nothing on the synchronous path was
affected (0 errors in 80 judge-on requests). That is correct engineering and it is
also the risk in item 1: the system is *reliable* by silently declining to judge.

Also pinned by the suite: judge unreachable, HTTP 5xx, malformed JSON and timeout
each yield zero `laya-*` verdicts; a partial window failure keeps the windows that
answered; an unreadable calibration table silently disables fusion; a malformed
temperature returns `p` unchanged; a judge verdict can never `Block` on its own.
These are covered by the 23-test `laya_judge_contract_test` and 10-test
`hybrid_pipeline_test`, all passing.

**The shipped checkpoint does not ship a valid calibration.** On every load, Laya
itself warns:

```
RuntimeWarning: laya: this checkpoint ships invalid temperatures or values outside
[0.5, 5]; using choice:11+=0.10058280825614929 -> 0.5. Treat confidence from the
affected entries as uncalibrated.
```

The model author is telling us that at least one calibration entry is out of range
and has been silently clamped, so confidence from affected entries should not be
treated as calibrated. That lands directly on the two-stage calibration story in
`laya-combined-result.md` and is a second, independent reason not to trust judge
probabilities without our own fit.

---

## 9. Reproduction

### 9.1 Commands executed

| # | Command | Result |
|---|---|---|
| 1 | `bash scripts/eval_accuracy.sh` (default 30-day window) | exit 0 — no labels in window, "an accuracy claim here would be fabricated" |
| 2 | `psql -c "select min/max(created_at) from reviewer_overrides"` | labels dated 2026-08-20 → 2026-08-26 (outside the 30-day default) |
| 3 | `bash scripts/eval_accuracy.sh --days 3650` | exit 0 — baseline table §4.1 |
| 4 | `bash scripts/eval_accuracy.sh --days 3650 --json` | exit 0 — `comparable_pairs=0`, `judge_latency_p50_ms=0` |
| 5 | `bash scripts/eval_accuracy.sh --days 3650 --fit` (dry run) | exit 0 — "no labelled judge verdicts … nothing to calibrate" |
| 6 | `cargo test --workspace --no-fail-fast` (DATABASE_URL unset) | **pass** 514 / fail 0 |
| 7 | `cargo test --workspace --no-fail-fast` (DATABASE_URL set) | **fail** 513 / fail 1 (`update_policy_saves_and_returns`, HTTP 500); earlier identical run: 6 failures |
| 8 | `cargo bench -p controlplane-fast-path --bench fast_path_bench` | initially **fail** (corrupt dylib); **pass** after `CARGO_PROFILE_BENCH_STRIP=none` |
| 9 | `psql -c "select check_name, count(*), min/avg/max(duration_ms) from verdicts ..."` | exit 0 — latency breakdown §7.2 |
| 10 | `bash scripts/bench_laya.sh` | exit 0 — full run, all artefacts below |
| 11 | `python3.11 -m venv services/laya/.venv` + `uv pip install "laya[serve]>=0.3.11"` | **pass** — `laya==0.3.20`, `torch==2.14.0` (no Docker; see Appendix) |
| 12 | `hf download convaiinnovations/laya` (English set only) | **pass** — 842.6 MB `model.safetensors`, SHA-256 verified against the repo blob id |
| 13 | `JUDGE_MODE=laya N_REQUESTS=40 ./scripts/run_judge_on.sh` | exit 0 — arm A (§4.4), 41 judge verdicts, 8 fail-open windows |
| 14 | `JUDGE_MODE=off N_REQUESTS=40 ./scripts/run_judge_on.sh` | exit 0 — control arm B, 0 judge verdicts, 0 fail-open windows |
| 15 | `JUDGE_MODE=laya N_REQUESTS=40 ./scripts/run_judge_on.sh` (repeat) | exit 0 — arm C (§4.4), 18 fail-open windows, 3/3 readings over 2 s |
| 16 | `paste <(awk …) <(awk …) \| awk '{d=$2-$1…}'` | exit 0 — paired latency delta, §4.4 |

Also exercised read-only against the tunneled deployment: `GET /api/v1/system/config`,
`/api/v1/metrics/judge-agreement`, `/api/v1/stats/overview`,
`/api/v1/verdicts/recent`. No write requests were made to it.

### 9.2 The `DATABASE_URL` sensitivity (important for interpreting #6 vs #7)

| Environment | Tests run | Passed | Failed |
|---|---|---|---|
| `DATABASE_URL` unset | 514 | 514 | 0 |
| `DATABASE_URL` set to the populated demo DB | 514 | 513 | 1 |
| same, earlier run on same code | 514 | 508 | 6 |

The failures are FK/constraint fixtures assuming a pristine schema, plus one
env-restore assertion. They are **not** judge regressions and they are **not
deterministic**; the driver reports the unset run as the regression baseline and
captures the set run separately (`BENCH_TEST_WITH_DB=1`).

### 9.3 Environment

| | |
|---|---|
| Host | Apple M4 Pro, arm64, 12 CPUs, 24 GB RAM |
| OS | macOS 27.0 (26A428), Darwin 27.0.0 |
| Toolchain | rustc 1.96.0, cargo 1.96.0 (Homebrew); Python 3.14.7; psql 14.20 |
| Docker | client 29.4.0; **daemon not running** |
| Revision | `feature/mcp-server` @ `6a105d4d52fbd415f1d8cd5f3677314a9a9e001b`, workspace 0.7.0 |
| Database (measured) | PostgreSQL 16.15 (local); PostgreSQL 16.15 aarch64-unknown-linux-musl (remote) |
| Upstream | ollama / `qwen2.5:1.5b` (`http://ollama:11434`) |
| Judge | `off` in both environments; Laya port 8300 not reachable |
| Region | none — single workstation |
| Seeds | N/A: every measurement here is deterministic given the data; the criterion bench is not seeded, hence the reported intervals and outlier counts |

### 9.4 Raw artefacts and checksums

**Pre-Laya baseline artefacts** — `benchmark-results/run-20260926T173430Z/`:

| File | SHA-256 |
|---|---|
| `00-manifest.txt` | `38c2a07fee6999b9f5cf987612b1d1e9000a5c50e61e2e733403d606365288c0` |
| `01-corpus-census.txt` | `3d36179c2f6cc960233b4c52a3236733663e15e3423885a21773ca1f1122ccde` |
| `02-ablation.txt` | `7c7390f2f2366df5c9eeb49caaa25d2a4dd1bff9ad9d70a004e8a78b464e7659` |
| `03-ablation-json.txt` | `173705a7be8825e2cb9eaabced55643cd983601511769c6bfd21eda913fed026` |
| `04-fit-dryrun.txt` | `64401b1b2ebe84f482e9ddf978a77383d8c728868ecd500ece604300f90a4968` |
| `05-ablation-rows.psv` | `77713b4435e39ec0fadd2daaa2f2a74be66a0100df457c5f14b659d399da8837` |
| `06-confusion-matrix.txt` | `4812bbeb86de061a88b3dd194c334df47a85ed2346efa4605ac58bdcbaac20d8` |
| `07-workspace-tests.txt` | `81c966bc431d704e44ec338af30e2ea9a5ce5874960093fbc09d116eaad813f7` |
| `07b-workspace-tests-db.txt` | `16a13e483eaf0808f73cb905a5e212accf1249ef824c31de4c76a35af2b215ed` |
| `09-fastpath-bench.txt` | `bebc8235bb5487d2daaaf280f24f66899e3e13848da15156cffb73aaf49b319f` |
| `10-fastpath-summary.txt` | `61d9303b4a1bdd6000abf41f34d57ed60770ab6ac19cf5389212b2630c9e7bb7` |
| `ablation_rows.sql` | `91856ea9e4f1f6cb12831a9d83988d0b218a7c8d7f0c94806f067ad127c8e13e` |

**Judge-on artefacts** — `benchmark-results/judge-on-pass/`, `judge-off-pass/`,
`judge-laya-pass/` (each holds `traffic.txt` with per-request `http_code latency_ms`,
`gateway.log`, `laya-serve.log`, `judge-fail-signals.txt`, `laya-latency-pooled.txt`,
`laya-detector-breakdown.txt`, `run-window-start.txt`) and `laya-sidecar/laya.log`.

Reproduce the whole thing with:

```bash
# 1. baseline + regression (no judge needed)
export DATABASE_URL="postgres://controlplane:secret@localhost:5432/controlplane"
BENCH_TEST_WITH_DB=1 ./scripts/bench_laya.sh

# 2. judge-on measurement, then the paired control (same 40 requests, same order)
JUDGE_MODE=laya N_REQUESTS=40 DATABASE_URL="$DATABASE_URL" ./scripts/run_judge_on.sh
JUDGE_MODE=off  N_REQUESTS=40 DATABASE_URL="$DATABASE_URL" ./scripts/run_judge_on.sh

# 3. re-run the ablation now that judge verdicts exist
./scripts/eval_accuracy.sh --days 3650
```

### 9.5 Dataset manifest

| Table | Rows | Note |
|---|---|---|
| `verdicts` | 1138 | window 3650 days |
| `intercepted_calls` | 1114 | |
| `reviewer_overrides` | 25 | 6 confirm / 6 dismiss / 13 override |
| — by axis | | performance 10, responsibility 15 |
| `detector_calibration` | 16 | 0 with `calibrated = TRUE` |
| `verdicts` where `check_name LIKE 'laya-%'` | **59** | produced by this benchmark's judge-on arms (was 0 before) |
| `verdicts` judge-on, carrying `duration_ms` | 14 | coverage gap — see §7.2 |

No checksum is given for the dataset because it is mutable live data, not a frozen
fixture. The exact rows used are pinned by `ablation_rows.sql` + the query in §5 of
`scripts/bench_laya.sh`.

### 9.6 Files changed by this benchmark

| Path | Status |
|---|---|
| `scripts/bench_laya.sh` | **new** — reproducible baseline/regression driver |
| `scripts/run_judge_on.sh` | **new** — self-contained judge-on/off measurement pass |
| `docs/analysis/laya-benchmark-report.md` | **new** — this report |
| `benchmark-results/**` | **new** — raw outputs, 692 KB (§9.4) |
| `services/laya/.venv/` | **created, gitignored** — the native Laya interpreter (717 MB) |
| `target/bench/`, `.uv-cache/` | **created, gitignored / removed** — build + package caches |

**No production code, configuration, migration, or test was modified.** Specifically:

- No `.rs`, `Cargo.toml`, compose, `.env`, or migration file was changed.
- No write was issued against `detector_calibration`: the `--fit` run was the
  dry-run default, so `calibrated_detectors` is still **0** and the fusion is
  still inert. The judge was run with no calibration fit on purpose — that is the
  configuration the fusion gate leaves you in by default.
- Environment interventions were limited to: deleting a corrupt build artifact,
  overriding `CARGO_PROFILE_BENCH_STRIP=none` for the bench invocation, and using
  ports 8090/8081/8901/8000 so nothing collides with the tunnelled deployment.
- The judge-on runs wrote real traffic into the local corpus. Those 59 judge
  verdicts and their calls are new rows in the local PostgreSQL database; the 25
  pre-existing labels were not touched.

---

## 10. Limitations, unsupported scenarios, recommendations

### What this run resolved, and what still blocks a quality claim

**Resolved by running the judge locally (no Docker required — see the Appendix):**

1. Judge latency, budget compliance, fail-open behaviour and the client-visible cost
   of enabling the judge are now **measured** (§4.2, §4.4, §7.2, §8).

**Still blocking:**

2. **No human labels exist on any judge-on call**, so accuracy, precision, recall,
   F1 and calibration of the hybrid path remain **unmeasurable**. The judge has
   produced 59 verdicts; none has been reviewed. This is a labelling task, not a
   benchmark run — there is no code change that can shortcut it.
3. **The fusion still cannot activate.** `--fit` needs labelled *judge* verdicts;
   there are none, so it correctly refuses and `fused` remains identical to
   `heuristic-only` apart from unfitted `-evidence` readings.
4. **The 25 labels predate the judge entirely** (2026-08-20 → 08-26 vs judge-on
   2026-09-27), so they cannot be reused as ground truth for the judge's calls.
5. **The 25 labels are not a dataset for quality claims.** Pooled CI on recall
   `[0.188, 0.812]`; per-axis `N` is 10 and 15.
6. **Load coverage is shallow.** Three 40-request arms at *average serial* rate is
   not a concurrency sweep: throughput, queueing time, saturation behaviour and the
   timeout/retry rates under load remain unmeasured, as does cold-start versus warm.
   The live deployment behind the SSH tunnel was deliberately not load-tested — it is
   not this benchmark's to hammer — so all load numbers here are from the local
   judge-on stack.
7. **Domain variation is unsupported by the data.** The label table has no domain
   or tenant column, so a healthcare/financial/custom split is not derivable.

### Prioritized recommendations

1. **Fix judge capacity before anything else.** 26 of 40 calls got no judge verdict
   and 26 fail-open windows were logged under a merely sequential 40-request load.
   Since absence = pass, an overloaded judge silently becomes a no-op. Scale the
   sidecar (GPU per `docker-compose.gpu.yml`, or more uvicorn workers), and cap
   judge concurrency so an overload surfaces as a metric rather than as silence.
2. **Emit an explicit "judge declined" signal.** Today a fail-open window and a
   legitimate sub-0.45 discard are indistinguishable in the verdict table (§8.1).
   Persist the distinction, or the operational health of the judge cannot be
   monitored at all.
3. **Do not claim a Laya accuracy result.** Nothing here measures the judge's
   decisions. The number that should inform rollout is still the baseline's pooled
   FP-rate of 0.684 on the `responsibility` axis — plus the measured fact that
   enabling the judge costs **+50 ms mean** client latency and, on CPU, brings its
   own path past its 2 s budget.
4. **Generate labelled judge-on traffic, then fit.** Target ≈ 200 labelled
   `(call, axis)` pairs per axis; the Wilson interval only narrows to roughly ±0.14
   recall at that size. Until then `--fit` must stay a dry run.
5. **Close the latency-instrumentation gap.** `duration_ms` is populated for only
   84 of 1138 baseline verdicts and 14 of 59 judge verdicts, so every latency figure
   here is a magnitude, not a distribution. Emit it on every verdict before any
   rollout — you cannot monitor a 2 s budget you do not record.
6. **Make the inert state impossible to misread.** `fused` differing from
   `heuristic-only` while the judge is silent (§8.3) is a reporting trap. Have
   `eval_accuracy.sh` print "judge contributed 0 readings" inline next to any fused
   figure, and have the decision engine tag an outcome as fusion-derived explicitly.
7. **Isolate the DB-backed tests.** Point `db_integration_test` /
   `api_integration_test` at a dedicated schema or test database; the current
   behaviour (fail, and fail *non-deterministically*, against live data) makes the
   regression signal untrustworthy.
8. **Fix the refresh path before refresh-day.** Make `--reset` part of the documented
   rollout so `fused+calibrated` is never double-applied.
9. **Document the `strip = "symbols"` / macOS defect** in the contributor guide so
   nobody else spends an afternoon on a corrupt proc-macro dylib.
10. **Keep the judge off the synchronous path** (fast-path 2.3–19 µs, budget
    10 000 µs). That property is both measured and contract-pinned; the ~50 ms
    contention cost is the reason to also budget CPU, not a reason to move the judge.

### Appendix: how to actually turn the judge on (verified, and used for this report)

Enabling the judge takes **four** independent conditions. The sidecar being up is
only the first, and it is the one people assume is sufficient — it is not.

| # | Condition | Where it is read | State before this run |
|---|---|---|---|
| 1 | a Laya sidecar is reachable | its own process | not started |
| 2 | the **gateway** is started with `DECISION_JUDGE=laya` | process env, at startup | **unset ⇒ `off`** |
| 3 | `LAYA_URL` points at it | process env, at startup | unset ⇒ `decision_judge_configured: false` |
| 4 | policy does not set `checks.decision_judge_enabled = false` | `policies.threshold_config` | defaults ON; not a blocker |

Conditions 2 and 3 are read from the environment **once, at startup**
(`judge_configuration()` in `services/dashboard-api/src/router.rs`). A gateway that
is already running keeps reporting `off` no matter what you do to the sidecar — it
**must be recreated**. This is the step that was missed earlier in this session:
a Laya container being up while the gateway reports `decision_judge: "off"` is a
perfectly consistent state, and it is the state that produces zero judge verdicts.

**The sanctioned path (Docker):**

```bash
# the whole stack plus the judge profile, with the gateway's process env set
DECISION_JUDGE=laya docker compose --profile judge up --build -d

# the Laya image pip-installs laya[serve] and loads ~808 MB of checkpoints on first
# request; its healthcheck carries start_period: 180s, so expect a slow first start
docker compose ps laya

# verify the gateway actually picked it up -- this is the authoritative check
curl -s localhost:8080/api/v1/system/config \
  | grep -o '"decision_judge":"[^"]*"\|"decision_judge_configured":[a-z]*'
# expect: "decision_judge":"laya"  "decision_judge_configured":true
```

**The path this report actually used (no Docker daemon available).** The sidecar
runs fine natively; `services/laya/Dockerfile` is only a packaging of it. Reproduce
with `scripts/run_judge_on.sh`, whose key steps and gotchas are:

```bash
# 1. interpreter: the image pins python:3.11-slim, so match it (a 3.14 host default
#    is not what you want); uv is much faster than pip here
python3.11 -m venv services/laya/.venv
uv pip install --python services/laya/.venv/bin/python "laya[serve]>=0.3.11"
# -> laya==0.3.20, torch==2.14.0
```

```bash
# 2. get the English checkpoint (~808 MB). Fetch ONLY the root variant:
#    multilingual/ and typed-decisions/ are separate subfolders (~1.5 GB extra)
#    that the default English model never loads.
#
#    Do NOT use `hf download` for the big file on this network: it sustained
#    ~430 KB/s and, worse, started a FRESH partial on every invocation, so nothing
#    accumulated. Plain curl fetched the same 842.6 MB in ~90 s and resumes:
curl -sSL -C - -o /tmp/laya-model.safetensors \
  https://huggingface.co/convaiinnovations/laya/resolve/main/model.safetensors
#    then verify and place it in the cache as a blob, with a snapshot symlink
#    under the revision sha. scripts/run_judge_on.sh assumes this is done.
```

```bash
# 3. run the sidecar. NOTE: `laya-serve` ignores `--port` and binds 0.0.0.0:8000.
#    The compose file maps container 8000 -> host 8300, which is why the docs say 8300.
# (device=CPU and preload are set inside scripts/run_judge_on.sh)
# `laya-serve` ignores --port and binds 0.0.0.0:8000 -- NOT 8300
services/laya/.venv/bin/laya-serve
#    expect: RuntimeWarning: checkpoint ships invalid temperatures ... (see §8)

# 4. start the gateway with the judge wired in (this is the step that is missed)
DECISION_JUDGE=laya LAYA_URL=http://127.0.0.1:8000 \
  target/debug/controlplane-gateway

# 5. verify — this is the authoritative check, not `docker compose ps`
curl -s localhost:8080/api/v1/system/config \
  | grep -o '"decision_judge":"[^"]*"\|"decision_judge_configured":[a-z]*'
# expect: "decision_judge":"laya"  "decision_judge_configured":true
```

Then drive traffic and measure:

```bash
JUDGE_MODE=laya N_REQUESTS=40 ./scripts/run_judge_on.sh   # judge on
JUDGE_MODE=off  N_REQUESTS=40 ./scripts/run_judge_on.sh   # paired control
./scripts/bench_laya.sh                                   # preflight reports judge state
psql "$DATABASE_URL" -c \
  "SELECT count(*) FROM verdicts WHERE check_name LIKE 'laya-%'"
```

`scripts/bench_laya.sh` step 0 prints exactly which condition is unmet and refuses
to let a missing judge verdict be read as a result:

```
decision_judge:            off
decision_judge_configured: false
laya-* verdicts in corpus:  0

  !! NO JUDGE VERDICTS EXIST. Any 'post-Laya' comparison below is IMPOSSIBLE,
     not merely missing: there is no judge prediction to score.
```

Two practical constraints worth knowing before repeating this:

- **A judge-on measurement must run inside one invocation.** The sidecar and the
  gateway must both be alive while traffic is driven, and background processes here
  do not survive across tool calls. `scripts/run_judge_on.sh` is self-contained for
  that reason, and writes its results to PostgreSQL, which does persist.
- **Scope every query to the run window.** `run-window-start.txt` exists because
  `laya-*` counts otherwise accumulate across passes and quietly overstate the
  sample size — the same mistake the report warns about for the fusion.

### Unsupported / non-existent (stated so they are not assumed)

- **Citation checking does not exist** in the platform. There is no detector, no
  endpoint, and no documentation claiming otherwise.
- **Direct judge scoring is not exposed** through MCP or any public API.
- **SSE streaming, metrics counters** on the MCP surface are `TARGET`, not implemented.
- **No multi-tenant model beyond app scoping** exists, so per-tenant variation is
  not measurable.
