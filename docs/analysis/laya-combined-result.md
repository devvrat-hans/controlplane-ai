# How the combined result is calculated for each verdict with the Laya judge ON

> **Scope:** `DECISION_JUDGE=laya|jev` and `checks.decision_judge_enabled` is not `false`
> and `LAYA_URL` is set — i.e. the judge is actually running on the shadow path.
>
> **Grounded in code:** `services/shadow-analysis/src/laya_client.rs`,
> `services/shadow-analysis/src/calibration.rs`,
> `services/decision/src/aggregator.rs`, `services/decision/src/router.rs`,
> `infra/migrations/023_create_detector_calibration.sql`. Verified 2026-09-25.
> Companion docs: `checks-inventory.md` (the full check list) and
> `laya-integration-plan.md` (the design rationale).

---

## 0. The one thing to understand first: there are TWO combination stages

"Turning on the judge" does **not** by itself change how verdicts are combined. There are
two independent calibration stages, and **both are gated on the same condition**: a row in
`detector_calibration` with **`calibrated = TRUE`**. The table ships with only
`calibrated = FALSE` rows, so out of the box neither stage runs.

```
   LAYA ANSWERS                         verdicts (one per detector)
        │                                        │
        │  ┌──────────────────────────────────┐  │
        └─▶│ STAGE 1 — TEMPERATURE SCALING    │──┘   reads detector_calibration.temperature
           │ (shadow-analysis, per detector)  │      where calibrated = TRUE
           │ p = sigmoid(logit(p_raw) / T)    │
           └──────────────────────────────────┘
                            │
                            ▼   controlplane.verdict.shadow
           ┌──────────────────────────────────┐
           │ STAGE 2 — WEIGHTED NOISY-OR      │       reads detector_calibration.weight
           │ (decision, per axis)             │       where calibrated = TRUE
           │ p_axis = 1 − Π(1 − w_d·p_d)      │       + JUDGE_* env thresholds
           └──────────────────────────────────┘
                            │
                            ▼
                    final outcome + reason
```

### The three states you can actually be in

| State | `detector_calibration` | Stage 1 (temperature) | Stage 2 (fusion) | What decides the outcome |
|---|---|---|---|---|
| **A. Judge off** | — | not called | not called | Fast path only (+ no `laya-*` verdicts exist) |
| **B. Judge ON, no fit** | only `calibrated = FALSE` | **identity** (`T = 1.0`) → `p = p_raw` | `weights` empty ⇒ `fuse_evidence()` returns `None` | **Legacy aggregator**: worst-of + compound-risk (§5.1) |
| **C. Judge ON + a fit** | ≥ 1 row `calibrated = TRUE` | **temperature applied** to every judge detector | **weighted noisy-OR fusion** runs | Fusion (§5.2), which can also escalate on disagreement |

> **Say this in Q&A:** *"With the judge on but no calibration fit, the judge still votes —
> its calibrated probability is its raw score, and the deterministic worst-of aggregator
> combines it exactly like any other detector. The weighted fusion is opt-in: it engages
> only once `scripts/eval_accuracy.sh --fit` has written `calibrated = TRUE` rows."*

The rest of this document is state **C** — the full path. State **B** is summarised in §5.1.

---

## 1. Stage 0 — the raw risk for each judge answer

Laya is a non-autoregressive **System One** model: one `state` + typed `questions` in, typed
`answers` out, **one forward pass** (one HTTP call per response window, max 8 windows
max-pooled). There is no free text, so there is nothing to parse.

Each question maps to exactly one detector via `QUESTION_SCALES`:

| Question key | Detector (`check_name`) | Axis | Risk kind | Calibration key (`primitive`, `option_count`) | Levels | Polarity | Needs context? | Emits evidence? |
|---|---|---|---|---|---|---|---|---|
| `hallucination` | `laya-hallucination` | Performance | YesNo | `choice`, 2 | 2 | higher = worse | **yes** | yes |
| `groundedness` | `laya-groundedness` | Performance | Ordinal | `score`, 3 | 3 | **lower = worse (inverted)** | **yes** | yes |
| `injection_attempt` | `laya-prompt-injection` | Responsibility | YesNo | `choice`, 2 | 2 | higher = worse | no | yes |
| `tool_call_risk` | `laya-tool-use` | Performance | Category | `choice`, 5 | 5 | higher = worse | no | **no** |
| `bias_present` | `laya-bias` | Responsibility | YesNo | `choice`, 2 | 2 | higher = worse | no | yes |
| `toxicity_severity` | `laya-toxicity` | Responsibility | Ordinal | `score`, 3 | 3 | higher = worse | no | yes |
| `is_reidentifiable` | `laya-semantic-pii` | Responsibility | YesNo | `choice`, 2 | 2 | higher = worse | no | yes |
| `filler_ratio` | `laya-verbosity` | Cost | Ordinal | `score`, 3 | 3 | higher = worse | no | **no** |

Four further questions are **qualifiers, not detectors** — they add explanation to a
finding but never produce a verdict of their own: `injection_family`, `bias_category`,
`reid_type`, `hallucination_severity`.

### 1.1 `p_raw` by risk kind

```text
YesNo    : p_raw = P(option "B")              # POSITIVE_OPTION = "B"
Ordinal  : p_raw = score / (levels − 1)       # then inverted if polarity says so
Category : p_raw = rank(choice)               # low 0.25 · medium 0.50 · high 0.75 · destructive 1.00 · other 0.00
```

**Reading a yes/no probability (`probability_of`).** Preferred source is the full
distribution; if the backend returns only the chosen option, the code recovers:

```rust
if probabilities[label] present      -> clamp(probabilities[label], 0, 1)
else if choice == label              -> clamp(confidence, 0, 1)
else if some other choice was made   -> clamp(1 − confidence, 0, 1)   // two-option approximation
else                                 -> None   // answer unusable, detector contributes nothing
```

**Polarity.** `groundedness` is the only inverted scale: a *high* rubric score means *well
supported*, which is **low** risk, so `risk = 1 − normalized`.

**Category scales ignore the distribution for the outcome.** `laya-tool-use` maps the chosen
label directly (`destructive`/`high` → Escalate, `medium` → Edit, `low`/`other` → nothing);
calibration only affects the confidence attached to that finding.

**Severity upgrade.** A material hallucination escalates regardless of the binary answer:
if `hallucination_severity` normalizes to `≥ 0.66`, `laya-hallucination` is forced to
`Escalate`.

---

## 2. Stage 1 — temperature scaling (shadow path, per detector)

```text
p = sigmoid( logit(p_raw) / T )
```

| Term | Where it comes from | Default |
|---|---|---|
| `T` | `detector_calibration.temperature` for `(detector, primitive, option_bucket(option_count))` where `calibrated = TRUE` | **1.0 (identity)** |

Implementation details that matter:

- `logit` clamps `p` to `[1e-6, 1−1e-6]`, so a reported `0.0` or `1.0` cannot produce an
  infinite logit.
- A non-finite or non-positive `T`, or `T == 1.0`, returns `p` **unchanged** — a malformed
  fitted parameter can never silently distort a decision.
- Option counts are **bucketed**, not fitted one-by-one: `0–2 → "2"`, `3–5 → "3-5"`,
  `6–10 → "6-10"`, `11–20 → "11-20"`. (e.g. `laya-tool-use` with 5 options fits the
  `"3-5"` bucket.)
- `T > 1` **softens** over-confident scores toward the middle; `T < 1` **sharpens** them.
  It is monotonic, so the threshold bands stay correctly ordered.
- The result is written into `Verdict.confidence`. **Downstream, `confidence` *is* the
  calibrated probability** — nothing re-interprets it.

**Only judge detectors are temperature-calibrated.** The heuristics (`prompt_injection`,
`bias_classification`, `groundedness`, `semantic_pii`) and the sidecar models
(`presidio-pii`, `llm-guard-toxicity`, `input-bias`, `deepeval-hallucination`) publish their
raw score directly as `confidence`. A fitted weight can still include them in Stage 2 — but
their probability is never tempered.

---

## 3. Stage 1b — the judge's own outcome per detector

Applied to the **calibrated** probability, in `laya_client.rs::classify`:

```text
p ≥ 0.90  →  Escalate
p ≥ 0.70  →  Edit
0.45 ≤ p < 0.70 → no action; published as a Pass verdict named "<detector>-evidence"
p < 0.45  →  discarded entirely
```

Per-detector exceptions:

| Detector | Outcome rule |
|---|---|
| `laya-hallucination` | severity ≥ 0.66 ⇒ **Escalate** regardless of the band |
| `laya-groundedness` | normal bands on the *inverted* risk |
| `laya-prompt-injection` | normal bands; the attack family is folded into the reason |
| `laya-tool-use` | label-driven, **not** band-driven: `destructive`/`high` → Escalate, `medium` → Edit, else nothing. Never emits evidence |
| `laya-bias` | normal bands; the category is folded into the reason |
| `laya-toxicity` | normal bands, but in practice only the **top tier** fires — Toxic-BERT owns the middle |
| `laya-semantic-pii` | normal bands |
| `laya-verbosity` | normal bands; never emits evidence (low-stakes, Cost axis) |

The `-evidence` verdict is deliberately `Outcome::Pass`: it **can never on its own** edit,
escalate or block anything. It exists so Stage 2 can fuse it and a reviewer can see where
the judge and the heuristics disagree.

---

## 4. Stage 2 — the weighted noisy-OR fusion (decision engine)

Entry point: `VerdictAggregator::fuse_evidence(verdicts, config)`.

**Returns `None` — i.e. does nothing — when `config.weights` is empty.** That is the entire
reason an un-fitted deployment behaves exactly as it did before.

### 4.1 The config, and where each value comes from

| Field | Source | Default |
|---|---|---|
| `weights: HashMap<detector, f64>` | `SELECT DISTINCT ON (detector, primitive, option_count) detector, weight FROM detector_calibration WHERE calibrated = TRUE ORDER BY … version DESC` | empty ⇒ fusion off |
| `escalate_threshold` | `JUDGE_ESCALATE_THRESHOLD` env | **0.90** |
| `edit_threshold` | `JUDGE_EDIT_THRESHOLD` env | **0.70** |
| `evidence_threshold` | `JUDGE_EVIDENCE_THRESHOLD` env | **0.45** |
| `corroboration_floor` | code constant | **0.50** |
| `calibrated_judge_confidence` | code constant | **0.90** |
| `disagreement_delta` | `JUDGE_DISAGREEMENT_DELTA` env | **0.40** |
| `calibration_version` | max `version` over the fitted rows | `None` |

A database error while loading this config leaves the fusion **off** — the decision service
never fails because its calibration could not be read.

### 4.2 The algorithm, per axis

```text
for each axis (deterministically ordered):
  1. collapse to ONE reading per detector — keep the strongest p
     (the "-evidence" suffix is stripped, so an evidence reading and its
      actionable twin are the same detector and cannot double-count)

  2. for each detector reading:
       weight = fitted_weight(base(check_name))        # None if outside the fit
       p      = clamp(verdict.confidence, 0, 1)
       calibrated = base(check_name).starts_with("laya-")

       if weight is Some:
           if p < 0.45: skip entirely                  # below the evidence floor
           p_axis *= (1 − weight · p)                  # noisy-OR term
       else:
           unfitted_outcome = worst(unfitted_outcome, reading.outcome)   # pass-through

  3. p_axis = clamp(1 − p_axis, 0, 1)

  4. base_outcome:
       p_axis ≥ 0.90 → Escalate
       p_axis ≥ 0.70 → Edit
       else          → Pass

  5. corroboration:
       corroborated = #{fitted readings with (outcome ≠ Pass or calibrated) and p ≥ 0.50} ≥ 2
                      OR  any calibrated (judge) reading with p ≥ 0.90
       if base_outcome is Escalate/Block and NOT corroborated → downgrade to Edit

  6. outcome = worst(fused_outcome, unfitted_outcome)

  7. disagreement:
       heuristic_p = max p over non-calibrated readings
       judge_p     = max p over calibrated readings
       if both > 0 and |heuristic_p − judge_p| ≥ 0.40
           outcome = escalate_at_least(outcome)        # never weakens a Block
           record a disagreement reason
```

The axis score — the actual "combined result" — is therefore:

```text
p_axis = 1 − Π ( 1 − w_d · p_d )      over fitted detectors d with p_d ≥ 0.45
```

### 4.3 Properties the tests pin (why it is built this way)

| Rule | Why | Test |
|---|---|---|
| One reading per detector | Two readings from one detector would double-count the same evidence and inflate confidence | `one_detector_never_contributes_twice` |
| Sub-floor readings dropped | Below 0.45 a reading is noise; fusing it only adds false positives | `readings_below_the_evidence_floor_are_discarded` |
| Lone heuristic capped at Edit | One over-eager rule must not escalate alone — the ensemble improves precision, it does not amplify | `a_single_heuristic_detector_is_capped_at_edit` |
| A confident judge may act alone | The judge is the calibrated voter; two-independent-witness corroboration is not required of it | `a_confident_calibrated_judge_acts_without_corroboration` |
| Unfitted detectors keep full authority | A partial fit must never silently weaken the checks it does not cover | `a_partial_fit_never_weakens_the_detectors_it_does_not_cover` |
| Evidence uses its detector's weight | `laya-bias-evidence` is still `laya-bias` | `an_evidence_reading_uses_its_detectors_weight` |
| Disagreement routes to a human | A judge/heuristic gap is exactly where a reviewer adds value — and it generates the next labelled sample | `judge_and_heuristic_disagreement_is_routed_to_a_human` |
| A block is never weakened | `escalate_at_least(Block) == Block` | `disagreement_never_weakens_an_existing_block` |
| Deterministic | Pure function of scores + thresholds (`AGENTS.md` rule 4) | `fusion_is_deterministic` |

---

## 5. From per-axis scores to the final outcome

### 5.1 State B — no fit (the default with the judge on)

`aggregate_with_reasoning` runs instead:

1. `final = worst(all verdicts)` (`Block > Edit > Escalate > Pass`).
2. **Compound risk:** count distinct non-pass axes; if **≥ 2** and the worst is `Pass` or
   `Edit`, raise to `Escalate`.
3. `primary_reason` = reason of the highest-confidence verdict at the final severity level.

All judge verdicts participate here exactly like every other detector; `-evidence` readings
are `Pass`, so they cannot change the outcome.

### 5.2 State C — a fit exists

```text
1. final_outcome = worst over axes' outcomes
2. triggered_axes = axes whose outcome ≠ Pass
3. compound risk: if |triggered_axes| ≥ 2 and final_outcome ∈ {Pass, Edit} → Escalate
4. primary_axis  = the axis that produced final_outcome with the highest p
                   (falling back to the highest-p axis overall)
5. primary_reason:
       disagreement present → "<disagreement reason> Fused p_<axis>=<p> across N detector(s)."
       otherwise            → the reason of the loudest (max-p) detector's verdict,
                              or "Fused p_<axis>=<p>; no detector cleared the evidence floor"
   then append " [calibrated fusion v<version>]" when a version is recorded
6. contributing_ids = every verdict whose outcome ≠ Pass (evidence readings excluded)
7. confidence       = primary_axis.p   (a fused probability, not any single verdict's)
```

---

## 6. Worked examples (numbers taken from the passing test suite)

Assume a fit giving: `prompt_injection = 0.6`, `semantic_pii = 0.6`,
`bias_classification = 1.0`, `input-bias = 0.6`, `laya-bias = 1.0`,
`laya-prompt-injection = 0.5`, and `T = 1.0` everywhere (so `p = p_raw`).

**① Two moderate judges, one axis**
`prompt_injection` p=0.90 and `semantic_pii` p=0.90, both w=0.6:

```text
p_axis = 1 − (1 − 0.6·0.90)² = 1 − 0.46² = 0.7884
0.70 ≤ 0.7884 < 0.90  →  base = Edit
corroboration: two fitted readings ≥ 0.50  →  corroborated
final = Edit
```

**② One loud heuristic alone cannot escalate**
`bias_classification` p=0.95, w=1.0:

```text
p_axis = 1 − (1 − 0.95) = 0.95   →  base = Escalate
corroboration: only ONE reading at ≥ 0.50, and it is not the judge  →  NOT corroborated
Escalate downgraded  →  final = Edit
```

**③ A heuristic plus a moderate judge corroborate and may escalate**
`bias_classification` p=0.95 w=1.0 **+** `input-bias` p=0.60 w=0.6:

```text
p_axis = 1 − (1 − 0.95)·(1 − 0.36) = 0.968  →  base = Escalate
corroboration: two readings ≥ 0.50  →  corroborated  →  final = Escalate
```

**④ A confident calibrated judge acts alone**
`laya-bias` p=0.95 w=1.0:

```text
p_axis = 0.95  →  base = Escalate
corroboration: calibrated reading ≥ 0.90  →  corroborated  →  final = Escalate
```

**⑤ Judge and heuristic disagree → a human decides**
`prompt_injection` p=0.95 (heuristic) **+** `laya-prompt-injection-evidence` p=0.45 (judge),
weights 1.0 / 0.5:

```text
p_axis = 1 − (1 − 0.95)·(1 − 0.225) = 0.9613  →  base = Escalate
heuristic_p = 0.95, judge_p = 0.45  →  delta = 0.50 ≥ 0.40  →  disagreement
outcome = escalate_at_least(Escalate) = Escalate
primary_reason = "Judge/heuristic disagreement on the responsibility axis:
                  heuristic p=0.95 vs judge p=0.45 (delta 0.50) — routed to human review.
                  Fused p_responsibility=0.96 across 2 detector(s)."
```

**⑥ A sub-floor reading is discarded**
`bias_classification` p=0.30:

```text
0.30 < 0.45 evidence floor  →  skipped; no readings remain
p_axis = 0.0  →  Pass;  contributing_ids = []
```

**⑦ An evidence reading uses its detector's weight**
`laya-bias-evidence` p=0.90, fitted `laya-bias = 0.8`:

```text
base_detector("laya-bias-evidence") = "laya-bias"  →  w = 0.8
p_axis = 1 − (1 − 0.8·0.90) = 0.72     (without stripping the suffix it would be 0.27)
```

**⑧ Compound risk across axes**
`laya-bias` p=0.75 (Responsibility) + `laya-groundedness` p=0.75 (Performance), w=1.0 each:

```text
each axis → Edit;  two axes triggered;  final ∈ {Pass, Edit}  →  Escalate
compound_risk = true,  triggered_axes = ["performance", "responsibility"]
```

---

## 7. Per-detector reference: everything that can fuse

`w` is the **seed** weight shipped in migration 023 (all rows `calibrated = FALSE`, i.e.
inert until a fit). Judge detectors are marked **[J]** — they are the only ones that can
act alone at ≥ 0.90 and the only ones temperature-scaled in Stage 1.

| Detector | Axis | Source | Primitive / options | Seed `w` | Stage 1 (temperature) | Emits `-evidence` |
|---|---|---|---|---|---|---|
| `laya-hallucination` **[J]** | Performance | judge | choice / 2 | 0.50 | yes | yes |
| `laya-groundedness` **[J]** | Performance | judge | score / 3 | 0.50 | yes | yes |
| `laya-prompt-injection` **[J]** | Responsibility | judge | choice / 2 | 0.50 | yes | yes |
| `laya-tool-use` **[J]** | Performance | judge | choice / 5 | 0.50 | yes | no |
| `laya-bias` **[J]** | Responsibility | judge | choice / 2 | 0.50 | yes | yes |
| `laya-toxicity` **[J]** | Responsibility | judge | score / 3 | 0.30 | yes | yes |
| `laya-semantic-pii` **[J]** | Responsibility | judge | choice / 2 | 0.50 | yes | yes |
| `laya-verbosity` **[J]** | Cost | judge | score / 3 | 0.20 | yes | no |
| `prompt_injection` | Responsibility | native heuristic | score / 1 | 0.40 | no | no |
| `bias_classification` | Responsibility | native heuristic | score / 1 | 0.20 | no | no |
| `groundedness` | Performance | native heuristic | score / 1 | 0.20 | no | no |
| `semantic_pii` | Responsibility | native heuristic | score / 1 | 0.20 | no | no |
| `presidio-pii` | Responsibility | sidecar (Presidio) | score / 1 | **0.80** | no | no |
| `llm-guard-toxicity` | Responsibility | sidecar (toxic-roberta) | score / 1 | **0.80** | no | no |
| `input-bias` | Responsibility | sidecar (distilroberta) | score / 1 | 0.70 | no | no |
| `deepeval-hallucination` | Performance | sidecar (DeepEval) | score / 1 | 0.30 | no | no |

Notes:

- **A detector absent from the fit is not fused at all** — its verdict keeps exactly the
  authority it has today. Fitting is how an operator says *"I know this detector's error
  profile — apply the calibrated rule to it."*
- `semantic_pii` is additionally **dropped** by the shadow worker when Presidio *or* the
  judge reported PII on the same response (`demote_semantic_pii`), so it normally only
  exists as the fail-open fallback and will not double-count against `presidio-pii`.
- Fast-path verdicts (`unsafe_content`, `secret_detection`, `cost_cap`, …) are Path=Fast and
  are not calibrated — but they **do** reach the aggregator, so a fast-path `Block` still
  dominates the final outcome.

---

## 8. Constants and configuration ledger

| Constant / setting | Value | Defined in |
|---|---|---|
| Judge escalate band | `≥ 0.90` | `laya_client.rs::ESCALATE_THRESHOLD` |
| Judge edit band | `≥ 0.70` | `EDIT_THRESHOLD` |
| Judge evidence floor | `0.45` | `EVIDENCE_THRESHOLD` |
| Evidence suffix | `-evidence` | `EVIDENCE_SUFFIX` (pinned in the decision crate too) |
| Judge detector prefix | `laya-` | `JUDGE_DETECTOR_PREFIX` |
| Fusion escalate / edit | 0.90 / 0.70 | `JUDGE_ESCALATE_THRESHOLD` / `JUDGE_EDIT_THRESHOLD` |
| Fusion evidence floor | 0.45 | `JUDGE_EVIDENCE_THRESHOLD` |
| Corroboration floor | 0.50 | `FusionConfig::default()` |
| Judge acts alone at | 0.90 | `calibrated_judge_confidence` |
| Disagreement delta | 0.40 | `JUDGE_DISAGREEMENT_DELTA` |
| Option buckets | `2` / `3-5` / `6-10` / `11-20` | `calibration.rs::option_bucket` |
| Max response windows | 8 | `governance_questions.rs` |
| Judge HTTP timeout | `LAYA_TIMEOUT_MS` (default 5000 ms) | `ShadowConfig` |

---

## 9. Fail-open: what happens when a piece is missing

| Failure | Result |
|---|---|
| Judge unreachable / 5xx / timeout / malformed JSON | **zero** `laya-*` verdicts (absence = pass) |
| One window of many fails | the windows that answered still contribute (max-pool) |
| `detector_calibration` unreadable | fusion silently **off** → legacy aggregator |
| Weight missing for a detector | that detector is passed through with its existing authority |
| Malformed/non-positive temperature | `p` returned unchanged (identity) |
| p below 0.45 | reading discarded, contributes nothing |
| Judge on but `LAYA_URL` unset | judge skipped, warning logged, shadow path otherwise unchanged |

---

## 10. 60-second answer for the jury

> When the judge is on, every verdict carries a **calibrated probability**, not a
> confidence from a generative model. The judge's raw reading is first temperature-scaled
> against a fit stored in `detector_calibration` — that fit is what turns an over-confident
> score into a statistically meaningful one. Then the decision engine combines all
> detectors on each axis with a **weighted noisy-OR**, `p_axis = 1 − Π(1 − w_d·p_d)`, using
> a fitted reliability weight per detector. That fused probability is bucketed
> deterministically (≥0.90 human review, ≥0.70 edit, else pass), with three safeguards: a
> **lone heuristic is capped at edit** so one eager rule cannot escalate on its own, an
> **unfitted detector keeps exactly the authority it has today** so a partial fit can never
> weaken the checks it doesn't cover, and a **judge-vs-heuristic disagreement of 0.40 or
> more is routed to a human** — which is also how the system collects its next round of
> labels. Two or more axes firing compound into an escalation. Nothing here runs a model:
> the final outcome is a pure function of the scores and the stored thresholds.
