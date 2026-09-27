# Bias when the judge is ON — combined, or `laya-bias` alone?

**Short answer: both run. `laya-bias` does not replace the existing bias detectors — and
when a calibration fit exists, they are combined numerically on the same axis.**

Grounded in `services/shadow-analysis/src/worker.rs`, `.../laya_client.rs`,
`services/decision/src/aggregator.rs`. Verified 2026-09-25.

---

## 1. There are three bias detectors, not two

| `check_name` | Where it comes from | Input scanned | Gated by |
|---|---|---|---|
| `bias_classification` | **native Rust heuristic** (`bias.rs`) — keyword/stereotype scoring | **response** | policy `bias_detection` |
| `input-bias` | **sidecar model** `valurank/distilroberta-bias` via guardrails | **prompt** | `bias_enabled` + `bias_detection` |
| `laya-bias` | **judge** (`laya_client.rs`) | **response** | `DECISION_JUDGE=laya\|jev` + `decision_judge_enabled` |

> Note: the sidecar's `llm-guard-bias` on the **response** is deliberately **disabled**
> (`guardrails_bias_handle = None` — *"opinionated ≠ biased"*). Response-side bias is
> covered by `bias_classification` and `laya-bias` only.

The judge toggle and the bias toggle are **independent**, so turning the judge on does not
turn anything else off.

---

## 2. What "combined" means depends on the calibration state

| State | Do both `bias_classification` and `laya-bias` run? | How the result is produced |
|---|---|---|
| **Judge ON, no fit** (default) | **Yes — both** | **Combined at the aggregation level only.** Each is a separate `Responsibility` verdict; `aggregate_with_reasoning` takes the **worst outcome** (`Block > Edit > Escalate > Pass`). No weights, no averaging. |
| **Judge ON + a fit** (`calibrated = TRUE` rows) | **Yes — both** | **Combined numerically.** Both are fused into the same axis score: `p_responsibility = 1 − Π(1 − w_d·p_d)`, then bucketed (≥0.90 Escalate / ≥0.70 Edit / else Pass). |
| **Judge OFF** | Only the non-judge detectors | legacy aggregation, same as state 1 |

So: **you never get "solely `laya-bias`" by default** — you get a combined result. The
judge adds a third opinion; it does not supersede the other two.

---

## 3. When you *can* get `laya-bias` alone

Only if you explicitly switch the other bias checks off in the app's policy:

```jsonc
"checks": {
  "bias_detection": false,          // kills BOTH bias_classification and input-bias
  "decision_judge_enabled": true    // judge stays on  →  laya-bias is the only bias voice
}
```

`bias_detection = false` is the only knob that silences the native + sidecar bias detectors.
`decision_judge_enabled` can only opt an app *out* of the judge, never the other way round.

---

## 4. Two things worth knowing

1. **There is no demotion rule for bias.** `semantic_pii` gets dropped when Presidio or the
   judge reported PII (`demote_semantic_pii`), because three correlated PII signals add
   noise. Bias has **no such rule** — `bias_classification` and `laya-bias` genuinely stack
   in the noisy-OR. The fusion's "one reading per detector" collapse does **not** dedupe
   them, because they are two different `check_name`s.
2. **Fusion is per *axis*, not per check.** `laya-bias` is not combined only with the other
   bias detectors — it is folded into the whole `responsibility` axis alongside toxicity,
   semantic PII, prompt injection and unsafe content. That is what makes the
   **compound-risk** rule work (two axes firing ⇒ escalate).

---

## 5. One-line summary

> With the judge on, bias is **combined, not replaced**: `bias_classification` (response),
> `input-bias` (prompt) and `laya-bias` (response) all report; with no calibration fit the
> combination is *worst-outcome-wins*, and with a fit it is a *weighted noisy-OR on the
> responsibility axis*. `laya-bias` acts alone only if `bias_detection` is set to `false`.
