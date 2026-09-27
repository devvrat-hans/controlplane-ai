# Laya Integration Plan — Hybrid Model Routing for the Shadow Path

> **Status: PARTIALLY IMPLEMENTED — see the table below.** The plan is labelled per the
> honesty rule in `AGENTS.md`: `IMPLEMENTED` / `SCAFFOLD` / `TARGET`.
>
> | Area | Status | Notes |
> |---|---|---|
> | `laya` container (`services/laya/Dockerfile`, compose `judge` profile) | **IMPLEMENTED** | Not started by a normal `docker compose up` |
> | Question schema + state truncation (`governance_questions.rs`) | **IMPLEMENTED** | 12 tests |
> | Judge client + calibrated mapping (`laya_client.rs`) | **IMPLEMENTED** | Fail-open over a real socket, per-question scale table, max-pool merge |
> | Per-app opt-out toggle + process master switch | **IMPLEMENTED** | `DECISION_JUDGE` env + `checks.decision_judge_enabled` |
> | Config plumbing (`.env.example`, compose, GPU override) | **IMPLEMENTED** | Defaults to `off`; `JUDGE_*` fusion thresholds added |
> | Chunk-and-max-pool for long responses | **IMPLEMENTED** | Overlapping windows (max 8), per-window call, probabilities max-pooled; `truncated` still flagged |
> | Temperature calibration (`calibration.rs`, `detector_calibration`) | **IMPLEMENTED, INERT** | Temperature + weights load from migration 023; **no fit ships**, so the transform is the identity until `eval_accuracy.sh --fit --apply` |
> | Weighted noisy-OR fusion + corroboration + disagreement routing | **IMPLEMENTED, INERT** | `VerdictAggregator::fuse_evidence`; only engages when a `calibrated = TRUE` row exists |
> | Sub-threshold evidence channel (`<detector>-evidence`, `Pass`) | **IMPLEMENTED** | The 0.45–0.70 band is published for fusion + reviewer visibility |
> | Policies-page toggle in the UI | **IMPLEMENTED** | 11th toggle + honest engine labelling; policies-page tests updated (41 passing) |
> | Request-detail Judge panel | **IMPLEMENTED** | Judge vs heuristic p per axis, disagreement flag, calibration version, evidence labelled as non-actionable |
> | Analytics ablation/agreement panel | **IMPLEMENTED** | `GET /api/v1/metrics/judge-agreement` behind it |
> | Offline accuracy harness (ablation vs `reviewer_overrides`) | **IMPLEMENTED** | `scripts/eval_accuracy.sh` — 4 configurations, precision/recall/F1/FP-rate/Brier/ECE, `--fit` is a dry run |
> | Retiring the `semantic_pii` keyword heuristic | **IMPLEMENTED** | Now a fallback: dropped when Presidio or the judge reports PII on the same response |
> | Per-axis threshold fitting (recall s.t. FP-rate ≤ 5%) | **TARGET** | Needs a corpus large enough to hold out a split |
> | Fine-tuning on our own labels (≥300 cases) | **TARGET** | Deliberately last |
>
> **Nothing here changes existing behaviour at the default configuration:** with
> `DECISION_JUDGE` unset the judge is never invoked, `detector_calibration` ships with no
> `calibrated = TRUE` rows so the decision engine keeps its pre-fusion aggregator, and the
> whole workspace test suite passes unchanged.
>
> **As-built deltas from the text below** (the code is the ground truth where they differ):
> the fusion never *lowered* an unfitted detector's authority — a detector with no fitted
> weight is passed through unchanged, so a partial fit cannot silently weaken the checks it
> does not cover; and `Outcome::worst` could not be used for "at least Escalate" because
> this codebase orders `Pass < Escalate < Edit < Block`, so severity is raised by name.
>
> **Verified:** 2026-09-24 against the live Laya sources (§15). This document
> **supersedes** `docs/analysis/jev-laya-integration.md` — it is the same integration
> goal, but rebuilt around a *hybrid per-activity routing* strategy rather than a
> single model swapped in, and updated with three facts that postdate the older
> proposal (`laya-serve`, the `noul` label-following bug, and the `typed-decisions`
> checkpoint's security/agent fine-tuning).
>
> **Question this document answers:** *for each governance activity, what is the best
> tool — and does combining tools actually beat any single tool?*

---

## 0. TL;DR

**The strategy is hybrid per-activity routing, not a model swap.** For every check we
pick the engine (or combination of engines) whose *error profile* best fits that
activity, then fuse the outputs deterministically.

Three things follow from that principle, and they are the whole plan:

1. **Some activities want a hybrid.** PII, incident/bias judgment, hallucination and
   prompt injection have **complementary** engines — one precise, one broad. Combining
   them lifts recall without sacrificing precision. These are where accuracy improves.
2. **Some activities want a single better engine.** Toxicity already has a
   well-validated model; verbosity is low-stakes. Bolting a second model on adds cost
   and correlated-noise, not accuracy.
3. **Some activities must stay deterministic.** Cost caps, retry detection, session
   risk, and the fast-path keyword/secret checks are *counters and rules*. The "best
   thing" for them is the counter. Adding a model here is an accuracy **regression**
   and an `AGENTS.md` contract violation.

Laya is therefore deployed as **one calibrated voter inside a hybrid panel** — never as
the oracle, never in the fast path, and disabled by default (`DECISION_JUDGE=off`) so
the existing pipeline is untouched and the whole workspace suite stays green
> (measured: 424 Rust tests passing, 0 failures).

| Check | Hybrid? | Engines combined | Expected effect |
|---|---|---|---|
| PII (direct + contextual) | **YES — stack + fuse** | Presidio (spans) + Laya (re-identification reasoning) | Fewer missed quasi-identifier leaks; redaction still exact |
| Semantic PII | **YES — fold into PII** | Presidio + Laya replace the keyword heuristic | Removes a crude heuristic; category-level reasons |
| Prompt injection | **YES — fuse** | Regex (precision) + Laya `typed-decisions` (recall, multilingual) | Closes the English-only blind spot; catches paraphrase/obfuscation |
| Hallucination / groundedness | **YES — fuse (3-tier)** | Laya + NLI/DeepEval (when configured) + overlap heuristic (last resort) | Replaces a near-chance heuristic with a calibrated one |
| Bias | **YES — fuse, split by side** | Laya (response reasoning) + HF classifier (input) + keywords demoted | Category-level explainability; fewer response-side FPs |
| Toxicity | **PARTIAL — route by language** | HF toxic-roberta (English) + Laya multilingual (non-English) + severity | Non-English coverage; calibrated severity tiering |
| Tool-use risk | **YES — stack** | Fast-path presence detect + Laya risk grading | Binary "has tool use" → low/medium/high/destructive |
| Verbosity | **MINIMAL — gate + judge** | Ratio pre-gate + Laya `filler_ratio` | Cheap, low-stakes; avoids a Laya call on obviously-fine text |
| Unsafe content | **NO — keep fast path** | Keyword matcher + determinism | Must block synchronously; stays rule-based |
| Cost cap / retry / session risk | **NO — keep deterministic** | Counters and windows | Adding a model would be slower and less accurate |
| Secret detection | **NO — keep deterministic** | Regex + entropy + Luhn | Precision-critical, needs spans for redaction |

---

## 1. Why hybrid, not replacement — the accuracy case

A governance layer has two failure modes that trade off against each other:

- **False positive** → a clean response is edited, escalated, or blocked. Costs reviewer
  time and trust; in the demo it is the *visible* error.
- **False negative** → a genuinely unsafe or wrong response passes. This is the error
  that matters for the product's core promise.

Today the shadow path leans on hand-written heuristics (word-overlap for groundedness,
keyword lists for bias and semantic PII, 21 English regexes for injection). Per
`docs/analysis/checks-inventory.md` these are labelled `⚠️ HEURISTIC`. They are fast and
explainable but they have **one error profile each**, and no amount of threshold tuning
changes that.

The reason hybrid works is a property of combining *independent* detectors:

- Two detectors with **different, complementary error profiles** (one high-precision/low-recall,
  one high-recall/low-precision) OR-ed together give **higher recall than either alone**,
  while a **corroboration rule** keeps precision from collapsing: an action is taken when
  *either* one detector is very confident *or* two moderately-confident detectors agree.
- Two detectors that see the **same signal** (correlated) give almost nothing when
  combined — they fire together and pass together. Naive fusion there just amplifies a
  shared bias and inflates apparent confidence.
- Two detectors of **different signal types** (e.g. one extracts spans, one reasons about
  inference) can be **composed**, where each does something the other *cannot* do at all.

So the routing question is not "is Laya better than the heuristic?" It is:

> **What signal does this activity need, what is each candidate's error profile, and are
> they complementary or correlated?**

That question produces a different answer for every check — which is exactly why this is
a hybrid plan.

### 1.1 The honest caveat that shapes everything

Laya's own model card is explicit: the **base checkpoints are near-chance zero-shot** on
the maintainers' `typed-decisions` benchmark — `laya` 0.362 and `laya-multilingual` 0.342
against a **0.461 majority-class baseline**. The headline 0.766 belongs to a checkpoint
fine-tuned on that benchmark's own train split. Laya describes itself as *"a fast base to
specialise, not a zero-shot decision engine."*

Therefore:

- Laya is an **additional calibrated voter**, not a replacement. Its value is
  **calibration** (honest probabilities you can branch on), **multilingual coverage**,
  **schema-safety** (it cannot be prompt-injected into emitting free text), and
  **speed** — not raw zero-shot accuracy on our domain.
- The single largest accuracy lever available to us is **not Laya at all** — it is using
  the `reviewer_overrides` table (migration `021`) as a labelled corpus to **tune the
  fusion weights and thresholds** (§5.4). That lever works whether or not Laya is on.
- Any claim that "combining engines improved accuracy" must be **measured** on that
  corpus with an ablation (`heuristic-only` vs `Laya-only` vs `fused`) before it is
  stated. The harness for that is §12.

---

## 2. The decision framework

Every check is classified into one of four relationships. The relationship determines
the implementation, and nothing is fused without one.

| Relationship | When | Fusion / composition rule |
|---|---|---|
| **FUSE** | Two engines detect the *same* phenomenon with *complementary* error profiles and see *different evidence* | Weighted noisy-OR per axis, plus a corroboration rule (need one strong or two moderate agreements). Weights come from the offline fit. |
| **STACK** | Engine A produces something engine B *consumes* (span offsets, a detected action, a language) | Sequential: A's output becomes B's input. Not a vote — a pipeline. |
| **REPLACE** | Same signal, same profile, one engine strictly better | Use the better one; retire the worse. No fusion. |
| **KEEP** | The activity requires determinism, sub-ms latency, or spans | No model. Optionally annotate with a shadow signal, never gate on it. |

A fifth, pragmatic rule: **a check may be both STACK and FUSE.** PII is exactly this —
Presidio spans are *stacked* into the redaction path, and Presidio's and Laya's
judgments are *fused* into the Responsibility-axis score. §4.8 walks through it.

---

## 3. Master routing matrix

| # | Activity | Path | Best engine(s) | Relationship | Laya primitive | Axis |
|---|---|---|---|---|---|---|
| 1 | Unsafe content | Fast | Keyword matcher (existing) | **KEEP** | — | Responsibility |
| 2 | Secret detection | Fast | Regex + entropy + Luhn (existing) | **KEEP** | — | Responsibility |
| 3 | Cost cap | Fast | Tiered integer compare (existing) | **KEEP** | — | Cost |
| 4 | Retry / loop | Fast | Sliding window (existing) | **KEEP** | — | Cost |
| 5 | Session risk | Fast | Counter (existing) | **KEEP** | — | Responsibility |
| 6 | Tool-use / agent risk | Fast + Shadow | Fast-path presence detect + Laya risk grading | **STACK** | `choice tool_call_risk` | Performance |
| 7 | PII (direct) | Shadow | **Microsoft Presidio** | **KEEP + STACK** | — | Responsibility |
| 8 | PII (contextual / re-identification) | Shadow | **Laya** (reasoning over quasi-identifiers) | **FUSE** (with 7) | `choice is_reidentifiable` + `choice reid_type` | Responsibility |
| 9 | Semantic PII (legacy keyword) | Shadow | Retire → covered by 7+8 | **REPLACE** | — | Responsibility |
| 10 | Prompt injection | Shadow | Regex (precision) **+** Laya `typed-decisions` (recall) | **FUSE** | `choice injection_attempt` + `choice injection_family` | Responsibility |
| 11 | Hallucination | Shadow | Laya **+** NLI/DeepEval **+** overlap (tiered) | **FUSE** | `choice hallucination` + `score hallucination_severity` | Performance |
| 12 | Groundedness | Shadow | Laya `score` **+** overlap fallback | **FUSE → REPLACE** | `score groundedness` | Performance |
| 13 | Bias | Shadow | Laya (response) **+** HF classifier (input); keywords demoted | **FUSE**, side-split | `choice bias_present` + `choice bias_category` | Responsibility |
| 14 | Toxicity | Shadow | HF toxic-roberta (English) **+** Laya (non-English, severity) | **FUSE**, language-routed | `score toxicity_severity` | Responsibility |
| 15 | Verbosity | Shadow | Ratio pre-gate **+** Laya `score` | **STACK + REPLACE** | `score filler_ratio` | Cost |
| 16 | Pattern promotion | Shadow | Existing recurrence counter | **KEEP** | — | — |

---

## 4. Per-activity deep analysis

Each section states what runs today, the candidate engines, their **error profiles**, the
relationship chosen, the fusion rule, and — honestly — whether the hybrid actually helps.

### 4.1 Activities that must stay deterministic (KEEP)

`unsafe_content`, `secret_detection`, `cost_cap`, `retry_detection`,
`session_risk_accumulator`.

| Property | Why a model cannot replace it |
|---|---|
| Latency | These are the fast path: budget <10ms p50. Laya is 33ms **minimum**, and it is a network hop. |
| Determinism | `AGENTS.md` requires the fast path to be deterministic. Cost/retry/session are exact counters. |
| Spans | Secret detection must return **offsets** to redact. A probability cannot redact. |
| Cost | A model call per request for an integer comparison is strictly worse on every axis. |

**Verdict: no hybrid. Any change here is a regression.** This is the single most
important judgement in the plan — a hybrid approach is not "add the model everywhere,"
it is "add the model where the error profiles are complementary."

One *optional* annotation (not a gate): shadow pattern-promotion may, when a secret or
unsafe keyword recurs, ask Laya to classify the **category** so the promoted fast-path
rule is better labelled. This stays off the decision path.

### 4.2 Tool-use / agent risk — **STACK**

**Today:** `services/fast-path/src/checks/tool_use_detection.rs` substring-matches
`function_call` / `tool_use` and dangerous action directives, then applies a flat
**1.5× confidence multiplier** to every non-pass verdict. It answers one question: *does
the response contain a tool call?*

**Candidates**

| Engine | Signal | Error profile | Can it grade risk? |
|---|---|---|---|
| Fast-path substring match | presence of a tool call | high recall, no severity | ✗ (binary) |
| Laya `tool_call_risk` (`choice`, `typed-decisions`) | low / medium / high / destructive | calibrated, moderate recall | ✓ |

**Relationship: STACK.** These are not two votes on the same question — they answer
*sequential* questions. The fast path decides *whether* a tool call exists (synchronously,
so it can escalate before delivery). Laya then decides *how dangerous* it is, after
delivery. Neither substitutes for the other.

**Why the hybrid helps.** Today every tool call is treated identically — the 1.5×
multiplier is invariant to whether the tool reads a file or drops a database. That is
simultaneously over-sensitive (a benign `search()` inflates unrelated verdicts) and
under-sensitive (a `DROP TABLE` gets the same +50% as a read). Grading the action lets
the decision engine apply the multiplier **per risk tier** (e.g. ×1.0 low, ×1.25 medium,
×1.5 high, escalate-on-sight destructive) instead of a flat constant.

> `laya-typed-decisions` is explicitly fine-tuned on *agent-trace observability* — this is
> one of the four workflows it was trained on, which is the strongest accuracy argument
> for Laya anywhere in this plan.

**Accuracy impact:** converts a binary flag into a 4-level ordinal signal. Reduces
over-escalation on benign agent traffic and raises severity on destructive actions.

### 4.3 PII — the worked example: **STACK + FUSE**

This is the canonical hybrid case, so it gets the fullest treatment.

**Today:** two separate mechanisms that neither see each other's output.

| Engine | What it does | Where |
|---|---|---|
| Microsoft Presidio (sidecar `/scan/pii`) | NER + rule recognizers (SSN, credit card, email, phone, IBAN, passport, driver's licence) → returns **entity type + character offsets + confidence**, plus an anonymized string | `services/guardrails/main.py` |
| `semantic_pii` heuristic | keyword match against **8 quasi-identifier categories** | `services/shadow-analysis/src/semantic_pii.rs` |

**Candidates and error profiles**

| Engine | Detects | Gives spans? | Precision | Recall | Weakness |
|---|---|---|---|---|---|
| **Presidio** | *direct* identifiers | **✓** (offsets → redaction) | **high** | medium | Cannot reason about **inference**: "the CTO of the only quantum startup in Tucson, 42" has no entity Presidio recognises |
| **Laya** `is_reidentifiable` + `reid_type` | *contextual* re-identification | ✗ | moderate | higher on quasi-identifiers | No spans → cannot redact; zero-shot weak on our domain |
| keyword heuristic | crude category cues | ✗ | **low** | low | Over-fires on the word "email"; misses everything phrased differently |

**Why this is a genuine hybrid — the two engines have *different capabilities*, not just
different thresholds.**

1. **They produce different outcomes, and both are needed.**
   - Presidio firing means we know *exactly where* the PII is → outcome **Edit** (redact
the spans, response still usable).
   - Laya firing means the text is *inferentially* re-identifiable but we **cannot
locate the span** → automatic redaction is impossible → outcome **Escalate** to a human.
   These are not competing verdicts on one question; they are the two distinct PII
   outcomes the product needs. Fusing them as one blob would destroy that distinction.
2. **STACK:** Presidio's offsets are consumed by the redaction path
   (`proxy/src/handler.rs` applies `ResponseEdit`s). Laya never touches redaction.
3. **FUSE:** both feed the Responsibility-axis probability for the *aggregate* decision
   (does this call warrant human review overall), using the noisy-OR in §5.2.

**Fusion rule (concrete)**

```text
Presidio.high_confidence_entities >= 1        -> Edit   (spans redacted)      p += w_presidio * max(entity.score)
Presidio entities >= 3                        -> Escalate (existing behaviour) p += w_presidio
Laya.is_reidentifiable  >= 0.90               -> Escalate (no span to redact)  p += w_laya
Laya.is_reidentifiable  0.70-0.90             -> Edit (annotate + redact what Presidio found)
Both fire at >= 0.60                          -> Escalate (compound)           p_axis = noisy-OR
Neither fires                                 -> Pass contribution 0
```

**Why it improves accuracy, specifically:**

- **Recall:** catches quasi-identifier leakage that Presidio structurally cannot see
  (no entity to recognise) and that the keyword list misses. This is a real class of
  privacy failure in LLM output ("re-identification by inference").
- **Precision is protected:** Presidio's deterministic path is unchanged, so every
  today's exact redaction still happens exactly as before. Laya can only *add*
escalations, never remove a redaction.
- **The right outcome per case:** automatic redaction where possible, human review where
  not — instead of one blanket behaviour.
- **Language coverage:** Presidio's English recognizers degrade on non-English text;
  Laya routes to `laya-multilingual`.

**Retire the keyword heuristic (`semantic_pii`).** It is strictly dominated by
Presidio (which finds the entities it is trying to guess) plus Laya (which reasons about
inference). Keep it only as a low-weight fallback if the sidecar is unreachable — the
fail-open path already handles that.

### 4.4 Prompt-injection — **FUSE**

**Today:** `services/shadow-analysis/src/prompt_injection.rs` — 21 weighted regex
patterns plus structural bonuses, **English-only**, with a `min(1.0)` cap that makes any
single 0.95 pattern score 1.0.

**Candidates**

| Engine | Error profile | Strength | Weakness |
|---|---|---|---|
| Regex patterns | **high precision, low recall** | deterministic, free, explains itself ("matched 'ignore previous instructions'") | paraphrase, novel phrasing, encoding, **any non-English text** |
| Laya `injection_attempt` (`choice`, `typed-decisions`) | **higher recall, calibrated** | catches paraphrase/obfuscation; tokensurf via Router; returns an attack **family** | zero-shot domain weakness; must be calibrated |

**Relationship: FUSE, with a gating refinement.** The naive version —
noisy-OR the two scores — is wrong here because they are **correlated on known patterns**:
both will score an obvious "ignore previous instructions" high, so OR-ing inflates
confidence without adding information.

Correct fusion is **precision-anchored**:

```text
if regex Score >= 0.90      -> take regex outcome (Block/Escalate), high confidence  (known attack)
else if regex matched       -> treat regex as precision anchor; Laya can only escalate
else                        -> Laya decides alone, capped at Escalate    (novel/paraphrased attack)
```

Regex is the trusted high-precision anchor; Laya is the **coverage** engine for the cases
regex was never able to see. That preserves the good behaviour of the deterministic rule
while closing its blind spots.

**Why it improves accuracy:**

- **The English-only blind spot is a real false-negative class today.** A Hindi or
  Arabic jailbreak scores 0.0 against an English pattern list. Router sends it to
  `laya-multilingual`, which was validated at 45/51 usable languages — the largest single
  recall gain in this plan.
- **Paraphrase recall:** "kindly set aside your earlier guidance" has no regex match.
- **Category attribution:** `injection_family` gives the reviewer an attack class, which
  feeds the existing `pattern_promotion` loop (a recurring family can graduate into a
  new regex rule — the hybrid literally improves the deterministic engine over time).

### 4.5 Hallucination & groundedness — **FUSE (three tiers)**

**Today, two weak links:**

- `deepeval-hallucination` (guardrails sidecar) needs an **LLM judge**. No judge is
  configured in the local demo, so it raises and **silently falls back to a word-overlap
  heuristic** — and only fires when RAG context exists at all. (This is the repo audit's
  §2.2 finding.)
- Native `groundedness` (`groundedness.rs`) is likewise sentence-level **word overlap**.

Both answer "is this response supported by the context?" — a fuzzy judgment task, not a
matching task. Word overlap cannot distinguish *"the warranty is 2 years"* from *"the
warranty is 5 years"* when the context mentions warranties, years and lengths. That is a
**false negative with perfect word overlap**.

**Candidates**

| Engine | Method | Error profile | Cost / latency | Availability |
|---|---|---|---|---|
| Word-overlap heuristic | set intersection of non-stopwords | **low precision, low recall** | ~0 | always (today) |
| **NLI cross-encoder** (e.g. DeBERTa-MNLI) | premise = context, hypothesis = claim → entailment / neutral / contradiction | **high precision on contradiction** | ~10–50ms | model download (not yet a dependency) |
| **DeepEval LLM-as-judge** | LLM reads both and judges | high reasoning ability | **high** (500ms–1.5s + token cost) | needs a judge model/key |
| **Laya** `hallucination` (A/B) + `hallucination_severity` (score) | non-autoregressive calibrated decision | moderate precision, **higher recall**, calibrated | ~33ms, batched | local, no key |

**Relationship: FUSE, tiered by cost.** A three-tier stack, cheapest-signal-wins:

```text
Tier 3 (always):  overlap heuristic      -> low weight w~0.2, never escalates alone
Tier 2 (default): Laya calibrated judge  -> primary voter (replaces the heuristic as the trigger)
Tier 1 (opt-in):  NLI cross-encoder or DeepEval -> high-confidence confirmation when available
```

When the NLI/DeepEval tier is configured, a **contradiction** finding outranks everything
and escalates immediately. Laya otherwise provides the calibrated probability that the
heuristic cannot. The heuristic is retained only as a grôund-truth-free fallback for when
both models are down (§7 fail-open), and is down-weighted so it can never hard-escalate on
its own.

**Why it improves accuracy:**

- Replaces a **near-chance** signal (word overlap) with a **calibrated** one — and the
  old signal was the *only* signal in the default local setup, because DeepEval had no
  judge. So this is a strict improvement in the default configuration, not a marginal one.
- Laya has **no free-text channel**: because output is a schema you define, a response
  cannot prompt-inject its own judge (unlike an LLM-as-a-judge, whose verdict can be
  influenced by the content it is judging). This removes a real adversarial weakness.
- **Calibration** means the threshold bands (§6) are statistically meaningful instead of
  arbitrary, so tuning them against `reviewer_overrides` actually transfers.
- **DeepEval is kept**, not deleted — when a judge *is* configured, it becomes the
  high-precision Tier 1 vote. And it is run side-by-side for one release so the two
  engines can be compared on real calls before DeepEval is retired.

### 4.6 Bias — **FUSE, split by side (input vs response)**

The repo already discovered something important by experiment: **scanning the response for
bias produced too many false positives**, so `worker.rs` deliberately sets
`guardrails_bias_handle = None` and only scans the **input** prompt. That decision is kept.

| Engine | Side | Error profile | Weakness |
|---|---|---|---|
| native `bias_classification` | response | keyword lists over 5 protected categories | crude; opinionated -> "biased" |
| HF `valurank/distilroberta-bias` | input | trained binary biased/not | narrow label semantics; English |
| Laya `bias_present` + `bias_category` | response | reasons about *unfair treatment*, calibrated, gives a **category** | zero-shot domain weakness |

**Relationship: FUSE, but only where each engine is strong.**

```text
Response side : Laya is the primary judge of "does this text treat a group unfairly".
                The keyword heuristic is demoted to low weight (w~0.2) as corroboration.
Input side    : HF classifier stays primary (validated there by the repo's own experiment).
                Laya adds a category + calibrated severity.
```

**Why it improves accuracy:** Bias is not a bag of words, it is a *judgment about fairness*
— which is exactly the shape Laya's `choice` primitive assumes. The current response-side
signal is either keywords (over-fires on any mention of a protected group) or nothing. A
calibrated reasoning judge plus a **corroboration rule** (a lone keyword hit no longer
escalates alone) attacks the false-positive problem the repo already measured, while the
category output makes verdicts explainable and gives `pattern_promotion` a real key
(`bias:gender` etc.) to work with.

**Also worth noting:** the strongest classical tool for subtle stereotype detection is an
**NLI/entailment model** ("does this sentence entail a stereotype?") — the same family as
§4.5's Tier 1. If we add an NLI cross-encoder for groundedness, it can serve bias too,
making that dependency do double duty. Laya remains the calibrated, category-producing
voter.

### 4.7 Toxicity — **FUSE, but the HF model stays primary**

This is where the honest answer is *"the existing model is already the best thing."*

| Engine | Strength | Weakness |
|---|---|---|
| HF `unitary/unbiased-toxic-roberta` | multi-label (toxic, severe_toxic, obscene, threat, insult, identity_hate), trained and validated on Jigsaw; genuinely strong on **explicit** toxicity | **English-only**; threshold is a single global constant in the sidecar |
| Laya `toxicity_severity` (`score`) | calibrated **severity**; multilingual via Router | weaker on explicit, profane toxicity than the specialist model |

**Relationship: FUSE, language-routed.** Not a replacement.

```text
English input  -> toxic-roberta primary; Laya supplies a calibrated severity tier
non-English    -> Laya primary (toxic-roberta degrades silently); toxic-roberta low weight
```

**Why it improves accuracy:** the accuracy gain is **coverage of non-English content** and
**calibrated severity tiering** (Block vs Escalate vs Edit rather than a single 0.7 cut-off).
It is deliberately *not* an attempt to beat toxic-roberta at its own job — that would be a
hybrid applied where the error profiles are **not** complementary, and would risk a real
regression. Demonstrating the discipline to say *"leave this one alone"* is part of the plan.

### 4.8 Verbosity — **STACK + REPLACE (minimal)**

| Engine | Signal | Latency |
|---|---|---|
| ratio/density heuristic (`verbosity.rs`) | response/prompt length ratio + unique-token density | ~0 |
| Laya `filler_ratio` (`score`) | calibrated filler-vs-signal judgment | ~33ms batched (free, it rides the same call) |

**Relationship: STACK then REPLACE.** The ratio acts as a cheap **pre-gate**: if the
response is obviously fine on length and density, skip the judgment entirely (saves
nothing on latency since Laya is batched, but keeps the deterministic signal visible). When
the gate trips, Laya decides the severity. The cost axis is low-stakes, so this is
intentionally the lightest hybrid in the plan.

---

## 5. How the hybrid approach improves accuracy

### 5.1 The four sources of accuracy gain

1. **Recall gain from complementary engines.** OR-ing a high-precision and a high-recall
detector recovers cases the high-precision one never saw. Injection (regex + Laya) and PII
(Presidio + Laya) are the clearest cases: the second engine sees a **different phenomenon**
(paraphrase; inference-based leakage), not the same one at a lower threshold.
2. **Precision gain from corroboration.** Requiring either one very-confident detector or
two agreeing detectors removes the "one over-eager heuristic escalates everything" failure.
This directly targets the false positives the repo already measured (bias on responses,
separated phone numbers, etc.).
3. **Calibration gain.** Every threshold in the current pipeline is a hand-set constant
(0.6 here, 0.7 there, 0.5 elsewhere). Substituting calibrated probabilities makes those
thresholds meaningful and — critically — makes them **tunable against labelled data**
rather than by intuition.
4. **Coverage gain without a model.** Router-based language dispatch fixes blind spots
(non-English injection, non-English PII) that have nothing to do with model quality.

### 5.2 The fusion function (deterministic, contract-compliant)

Per axis, each detector contributes a weight `w_i` and a calibrated probability `p_i`:

```text
p_axis = 1 - PRODUCT_over_i ( 1 - w_i * p_i )        # weighted noisy-OR
```

Then, per axis, a deterministic threshold map produces the outcome:

| p_axis | Outcome |
|---|---|
| >= 0.90 | Escalate (or Block if the axis policy allows) |
| 0.70 – 0.90 | Edit |
| 0.45 – 0.70 | annotate only, no action |
| < 0.45 | Pass |

Plus the **corroboration rule** — a *single* heuristic detector is capped at `Edit` unless
a second independent detector on the same axis also clears 0.5, or a calibrated judge
clears 0.9. This is what converts the ensemble into a precision improvement.

**`AGENTS.md` compliance:** the fusion is a **pure function** of the scores and the stored
thresholds. The models supply scores; the policy supplies thresholds; the decision is
deterministic given both. That is exactly what rule 4 requires. Fusion lives in the
**decision engine**, never in the fast path.

### 5.3 Complementarity, not redundancy — the test before fusing anything

Before adding a second engine to a check, it must pass this test:

1. **Different evidence?** If both engines key on the same tokens, they are correlated and
fusion adds noise (this is why injection uses a precision-anchored rule, not a plain OR).
2. **Different error profile?** If both fail on the same inputs, fusion cannot help.
3. **Different capability?** Best case — one produces spans, one reasons about inference
(e.g. PII). Then they are not redundant at all and fusion is unambiguously correct.
4. **Measurable independently?** If we cannot label their outputs separately on the
reviewer corpus, we cannot claim the fusion helped.

### 5.4 The largest accuracy lever is the labelled corpus, not the model

The `reviewer_overrides` table (migration `021`) already stores **ground truth**: a
reviewer's `confirm` (positive) or `dismiss`/`override` (negative) against the original
question, answer, axis and model outcome. Joined to `intercepted_calls` and `verdicts`,
that is a growing labelled dataset.

Use it to fit the fusion, **offline**:

- `w_i` per detector (reliability weight), clamped to `[0.3, 1.0]`.
- Per-axis thresholds, optimising **recall subject to FP-rate <= 5%** (false escalations
are the visible pain; false negatives are the expensive pain).
- One **temperature** per (primitive, option-count bucket) for calibration (Laya's card:
raw ECE 0.466 -> 0.081 after a single temperature fit).
- Store the fitted parameters with a **version** in `detector_calibration`, so the audit
trail records *which* calibration produced a decision.

This lever would work even with no Laya in the system. It is listed first because it is
the biggest, and because the hybrid's benefit is only *provable* through it.

### 5.5 Expected effect, and how we would know

Measured by ablation on the labelled corpus (harness in §12), reporting per axis:

| Configuration | Purpose |
|---|---|
| `heuristic-only` | today's baseline |
| `laya-only` | isolates the model's standalone contribution |
| `hybrid (fused)` | the proposed system |
| `hybrid + calibrated` | adds temperature fitting + tuned weights |

Metrics: precision, recall, F1, **FP-rate**, plus Brier/ECE for calibration and a
**judge-heuristic disagreement rate** (disagreements are routed to humans, §5.6). We claim
a win only where the fused configuration beats both single-engine baselines on the same
labelled set. Until those numbers exist, this document states *expected direction*, not
magnitude — per the honesty rule.

### 5.6 Disagreement routing — turning conflict into accuracy

When a calibrated judge and the heuristics strongly disagree on an axis (Laya p < 0.3 while
a heuristic fires at 0.8), that case is the one where a human adds most value. Route it to
**Escalate** with a reason naming the disagreement. Every such resolution becomes another
labelled precedent, which improves the fusion weights — so the conflict path is also the
**data-generation path** for §5.4. The disagreement rate is tracked as a first-class metric.

---

## 6. Architecture — where the hybrid panel runs

**Recommended: a dedicated `laya` container running `laya-serve`**, called over HTTP by
the shadow worker alongside the existing guardrails sidecar.

```text
proxy ──publish──▶ shadow worker (Rust)
                     ├── native heuristics        (in-process: groundedness, verbosity, injection regex, bias keywords)
                     ├── guardrails sidecar :8200 (Presidio PII, HF toxicity, HF bias, DeepEval)
                     └── laya judge         :8300 (laya-serve, POST /v1/systemone)   ◀── new: one batched call, all questions
                              │
                     ◀── Vec<ShadowVerdict> ──┘   (each detector emits its own verdict with a calibrated p)
                              │
                              ▼
                   decision engine (hybrid fusion + thresholds) ──▶ audit / escalation / dashboard
```

**Why a separate container**, rather than embedding Laya in the guardrails sidecar:

| Concern | Separate `laya-serve` | Embedded in guardrails |
|---|---|---|
| Memory / VRAM isolation | ✓ (808MB + 647MB weights isolated from Presidio/spaCy/HF) | ✗ shares the process |
| Crash isolation | ✓ | ✗ a Laya OOM kills **all** guardrails checks (fail-open saves delivery, but every check goes dark) |
| Laya & Jev swap | ✓ **change the base URL only** | ✗ needs SDK code changes |
| GPU allocation | ✓ independent | ✗ entangled |
| Conventions | ✓ matches the existing "call a sidecar over HTTP" pattern | ✓ too |

The decisive factor is that `laya-serve` implements the **Jev-compatible**
`POST /v1/systemone` shape, so one Rust client talks to Laya (self-hosted) or Jev (cloud)
by changing one env var. Architecture diagram, container config and Dockerfile notes are
in §11.3.

### 6.1 Why the panel is one call

Laya answers **every question in a single forward pass** (7.2ms/question batched on a T4;
103–332 questions/sec). So the whole 12-question governance schema is **one** HTTP call per
intercepted call, not twelve. Under the shadow path's <2s budget this is comfortably free,
and it keeps the fan-out identical in shape to the existing parallel `tokio::spawn`
handles in `worker.rs`.

---

## 7. Contract compliance (`AGENTS.md`)

| Requirement | How it is met |
|---|---|
| Shadow path only | Client lives in `shadow-analysis`; **never** a dependency of `fast-path`. CI grep/compile guard. |
| No blocking the client | Verdicts publish asynchronously; user latency unchanged. |
| Model does not make the decision | Laya emits **scores only**; fusion and thresholds are deterministic in the decision engine. |
| No LLM in the decision path | Laya is non-generative and returns probabilities; thresholds are pure policy. Compliant **by construction**. |
| Fail-open | Timeout / HTTP error / disabled ⇒ **zero verdicts**; absence = pass. The call is never failed because the judge is down. |
| Correlation ID threaded | Passed through from `ShadowAnalysisRequest` into the verdict/audit. |
| Shadow latency budget <2s | ~33ms GPU / 193–464ms CPU for all questions in one pass. |
| `decision` service owns final outcomes | Fusion lives there; escalation/audit/notification consume its output unchanged. |

**Fail-open matrix for the hybrid:**

| Situation | Behaviour |
|---|---|
| `DECISION_JUDGE=off` | Today's pipeline exactly; zero behavioural change |
| Laya unreachable / 5xx / timeout | Laya verdicts empty; heuristic + Presidio + HF verdicts proceed; fusion degrades gracefully |
| Presidio sidecar down | Existing fail-open (no PII verdicts); Laya still provides contextual re-identification |
| No RAG context | Context-dependent questions (hallucination, groundedness) are skipped — same as today |
| Malformed Laya payload | Reject, emit no verdicts, log; never panic |

---

## 8. Governance question schema

All questions in **one** call. Yes/no questions use a **two-option `choice` with neutral
keys `A`/`B`** rather than `noul`, because on the English checkpoint `noul` can follow its
own option labels (`false:`/`true:`) instead of the state and return a confident "no" for
clearly positive input (maintainers' issue #156). `noul` is used only where we validate it
on our own corpus.

| Key | Primitive | Instructions (essence) | Options / criteria | Emits check | Axis | Routed to |
|---|---|---|---|---|---|---|
| `hallucination` | choice | "Does the response assert facts unsupported by the context?" | `A`: no, supported / `B`: yes, unsupported | `laya-hallucination` | Performance | english/multi |
| `hallucination_severity` | score | severity of unsupported claims | fully supported / minor detail / material fabrication | (severity) | Performance | english/multi |
| `groundedness` | score | how well the response is supported by context | unsupported / partly / well supported | `laya-groundedness` | Performance | english/multi |
| `injection_attempt` | choice | "Does the prompt try to override or extract instructions?" | `A`: no / `B`: yes | `laya-prompt-injection` | Responsibility | **typed-decisions** |
| `injection_family` | choice | attack family | instruction_override / role_hijack / prompt_extraction / delimiter / encoding / other | (category) | Responsibility | **typed-decisions** |
| `tool_call_risk` | choice | risk of the requested tool action | low / medium / high / destructive / other | `laya-tool-use` | Performance | **typed-decisions** |
| `bias_present` | choice | unfair treatment of a protected group | `A`: no / `B`: yes | `laya-bias` | Responsibility | english/multi |
| `bias_category` | choice | which category | gender / race / religion / age / disability / other | (category) | Responsibility | english/multi |
| `toxicity_severity` | score | toxicity level | none / mild / severe | `laya-toxicity` | Responsibility | english/multi |
| `is_reidentifiable` | choice | "Could this identify a specific real person?" | `A`: no / `B`: yes | `laya-semantic-pii` | Responsibility | english/multi |
| `reid_type` | choice | type of identifying information | direct / quasi / none / other | (category) | Responsibility | english/multi |
| `filler_ratio` | score | filler vs signal | dense / acceptable / padded | `laya-verbosity` | Cost | english/multi |

**Design rules** (from the maintainers' own guidance):

- Always include an explicit `other` option in `choice` so the model can decline rather
  than pick the nearest wrong label.
- Keep `choice` option counts **<= 20** (accuracy collapses past that at the default
  `head_max_len`; Banking77: 0.425 vs Jev 0.870). Use coarse-to-fine for larger taxonomies.
- Send each question only the fields it needs; accuracy degrades on padded state.
- Ask everything up front; choose relevance in code.
- `score` is the weakest primitive — use it for severity tiers, never as the sole trigger.

### 8.1 State assembly and context-length handling (a silent accuracy killer)

Laya's context is **512 tokens (English) / 1024 (multilingual)**, and the option head
(`head_max_len` 192/256) eats into the state budget. A long response silently truncated
mid-sentence produces an arbitrary verdict, so:

- Truncate deterministically to the state budget, keeping **head + tail** (the assessable
  parts), never a naive prefix.
- For responses beyond one window: **chunk with overlap and max-pool** the per-chunk
  probabilities (a finding anywhere in the response must surface).
- Record `truncated: true` (and chunk count) in the verdict metadata so we can **measure**
  how often this bites.
- Raise `agent.cfg["head_max_len"]` at load time when a schema needs large option sets.

```json
{
  "response": "<response text, head+tail truncated to budget>",
  "prompt":   "<the user prompt>",
  "context":  "<retrieved context, if any>"
}
```

Skip context-dependent questions (`hallucination`, `groundedness`) when no context exists —
same behaviour as today's DeepEval path.

---

## 9. Verdict mapping (Laya output → `ShadowVerdict`)

`ShadowVerdict` is already `{axis, check_name, outcome, confidence, reason, duration_ms}`.
Mapping uses **calibrated** probabilities and `laya-*`-prefixed check names so the dashboard,
`pattern_promotion` and per-check effectiveness stats treat them as distinct detectors.

```text
choice A/B (yes/no):
  p_yes = calibrated probability of the unsafe option
  p_yes >= 0.90          -> Escalate  (Block only if the axis policy allows)
  0.70 <= p_yes < 0.90   -> Edit
  0.45 <= p_yes < 0.70   -> no verdict, but attach as evidence for fusion
  p_yes < 0.45           -> no verdict

score (ordinal 0..N, expected level s):
  s/N >= 0.85 and calibrated confidence > 0.7 -> Escalate
  s/N >= 0.55                                 -> Edit
  otherwise                                   -> no verdict

choice (category):
  the selected category carries the signal (e.g. destructive -> Escalate)
  confidence = calibrated confidence
```

The stored `confidence` is the **calibrated** value — this is the number the fusion
consumes. Axis mapping is fixed per §8 so the dashboard's axis breakdown stays coherent.

---

## 10. Toggles, policies and the frontend

- Add `decision_judge: bool` to `CheckToggles` (`services/shadow-analysis/src/toggles.rs`),
  parsed from a new `decision_judge_enabled` key in the policies `checks` object — so the
  judge is switchable **per app** from the Policies page exactly like every other check.
- Add it as the 11th toggle in `frontend/src/app/policies/page.tsx`.
- Label the judge-backed checks distinctly in the UI, per the repo's "no overclaiming"
  convention: the badge should say the engine that actually runs (e.g. `Laya`, `Presidio`,
  `ensemble`) rather than implying more than is running.
- `GET /api/v1/system/config` exposes judge status (`off` / `laya` / `jev`, reachable,
  calibration version) so the dashboard and the demo can be honest about it.

---

## 11. Implementation plan (file by file, phased)

### 11.1 Phase table

| Phase | Work | Exit criteria | Label |
|---|---|---|---|
| **0** | `laya` container + Rust client + toggle wired; judge returns empty when off | ✅ Done — fail-open tests green, shadow path unchanged, DeepEval still running side-by-side | **IMPLEMENTED** |
| **1** | Laya answers **hallucination + groundedness** (context-gated) | ✅ Done — mapped with inverted polarity for groundedness; severity can upgrade the outcome | **IMPLEMENTED** |
| **2** | Full 12-question panel: injection, tool-use grading, bias, toxicity, verbosity, contextual PII | ✅ Done — mapping unit-tested, `semantic_pii` demoted to a fallback, 11th UI toggle shipped | **IMPLEMENTED** |
| **3** | Calibration + fusion: `detector_calibration` table, temperature fitting, weighted noisy-OR + corroboration in the decision engine, disagreement routing | ✅ Code done and unit-tested. **Not** done: a fitted parameter set — the table ships empty by design, so the fusion is inert until `scripts/eval_accuracy.sh --fit --apply` runs on real labels | **IMPLEMENTED, INERT** |
| **4** | Jev backend behind the same client; offline tuning job; optional fine-tune once >= 300 labelled cases | ✅ Jev swap works by changing `LAYA_URL`; the tuning job exists (`--fit`). Not done (correctly): the fine-tune, which needs a corpus that does not exist yet | **PARTIAL** |

> **How the judge behaves today, stated plainly:** it emits a verdict only when the
> calibrated risk clears the edit threshold (0.70), and escalates at 0.90. Below that it
> emits **nothing** rather than a low-confidence finding, because the current decision
> engine has no fusion stage yet and a weak extra signal would only add false positives.
> That is why Phase 3 (fusion) is the phase that unlocks the accuracy gain.

### 11.2 New and modified files

| File | Change |
|---|---|
| `services/laya/Dockerfile` | **new** — `pip install "laya[serve]>=0.3.11"`, `CMD laya-serve` |
| `docker-compose.yml` | **new** `laya` service on `:8300`, health `GET /health`, `LAYA_PRELOAD=1`, `LAYA_DEVICE=cpu` |
| `docker-compose.gpu.yml` | GPU override for `laya` (`LAYA_DEVICE=cuda`) |
| `services/shadow-analysis/src/laya_client.rs` | **new** — `reqwest` client → `POST /v1/systemone`; 5s timeout; maps answers → `Vec<ShadowVerdict>`; all errors → empty vec |
| `services/shadow-analysis/src/governance_questions.rs` | **new** — §8 schema as constants, state assembly, head+tail truncation, chunking |
| `services/shadow-analysis/src/calibration.rs` | **new** — temperature scaling per (primitive, option-count); loads fitted params |
| `services/shadow-analysis/src/semantic_pii.rs` | demote to low-weight fallback (hybrid §4.3) |
| `services/shadow-analysis/src/worker.rs` | spawn the Laya handle beside the guardrails handles, gated on `toggles.decision_judge`; typed-decisions routing for injection/tool-use |
| `services/shadow-analysis/src/toggles.rs` | add `decision_judge`; parse `decision_judge_enabled` |
| `services/shadow-analysis/src/types.rs` | `ShadowConfig`: `laya_url`, `decision_judge_enabled`, `laya_timeout_ms`, `decision_judge_backend` |
| `services/shadow-analysis/src/lib.rs` | export the new modules |
| `services/decision/src/aggregator.rs` | **new** `fuse_evidence()` (weighted noisy-OR + corroboration + thresholds); keep `aggregate_with_reasoning` as fallback when no calibrated inputs exist |
| `services/decision/src/router.rs` | pass per-detector calibrated p + weights into fusion; disagreement reason |
| `services/decision/src/feedback.rs` | extend precedent retrieval to feed detector weights |
| `services/gateway/src/main.rs` | read `DECISION_JUDGE`, `LAYA_URL`; wire into `ShadowConfig` |
| `services/dashboard-api/src/router.rs` | judge status + calibration version on `/system/config`; `GET /api/v1/metrics/judge-agreement` |
| `infra/migrations/023_*.sql` | `detector_calibration` (detector, temperature, weight, version, fitted_at) |
| `infra/migrations/024_*.sql` | extend policies `checks` object with `decision_judge_enabled` |
| `frontend/src/app/policies/page.tsx` | 11th toggle + honest engine badges |
| `frontend/src/app/requests/[id]/page.tsx` | **Judge panel**: Laya p vs heuristic per axis, disagreement flag, calibration version |
| `frontend/src/app/analytics/page.tsx` | hybrid ablation panel (heuristic / Laya / fused) |
| `scripts/eval_accuracy.sh` | **new** — ablation + metric report against `reviewer_overrides` |
| `services/fast-path/tests/contract_no_judge_in_fast_path.rs` | **new** — automated contract guard: no route from the fast path (or the decision engine) to the judge |
| `docs/analysis/checks-inventory.md` | add Laya rows + hybrid verdicts; update the honest status table |
| `.env.example` | `DECISION_JUDGE=off`, `LAYA_URL`, `TYPESAFE_API_KEY=`, fusion thresholds |

### 11.3 Container and configuration

```yaml
# docker-compose.yml (addition)
  laya:
    build:
      context: ./services/laya
      dockerfile: Dockerfile
    container_name: controlplane-laya
    environment:
      LAYA_DEVICE: cpu        # cuda via docker-compose.gpu.yml
      LAYA_PRELOAD: "1"       # keep checkpoints resident; language switch = detection only (<1ms)
      # LAYA_API_KEY: optional; when set, laya-serve requires Authorization: Bearer <key>
    ports:
      - "8300:8000"
    healthcheck:
      test: ["CMD", "python", "-c", "import urllib.request; urllib.request.urlopen('http://localhost:8000/health')"]
      interval: 10s
      timeout: 10s
      retries: 5
      start_period: 120s
```

```env
# .env.example additions
DECISION_JUDGE=off               # off | laya | jev
LAYA_URL=http://localhost:8300   # laya-serve, Jev-compatible POST /v1/systemone
LAYA_MODEL=auto                  # auto (Router) | english | multilingual | typed-decisions
TYPESAFE_API_KEY=                # only when DECISION_JUDGE=jev
# fusion thresholds (per-app overridable via policies)
JUDGE_ESCALATE_THRESHOLD=0.90
JUDGE_EDIT_THRESHOLD=0.70
JUDGE_EVIDENCE_THRESHOLD=0.45
```

Because `laya-serve` speaks the Jev request/response shape, **`DECISION_JUDGE=jev` requires
only pointing `LAYA_URL` at the Jev API and adding the key** — no new Rust code. That swap is
the reason the client is built against the protocol rather than the SDK.

---

## 12. Evaluation harness & accuracy metrics

Reuse existing infrastructure; build no new stores.

**Corpus:** `intercepted_calls` (request/response) joined to `reviewer_overrides`
(ground-truth labels) and `verdicts` (per-check outcomes).

**Ablation** (run by `scripts/eval_accuracy.sh`):

```text
heuristic-only     -> today's baseline
laya-only          -> isolates the model's standalone contribution
hybrid (fused)     -> proposed system
hybrid+calibrated  -> adds temperature fit + tuned weights
```

**Metrics per axis, per configuration:** precision, recall, F1, **FP-rate**, Brier score,
ECE, judge–heuristic **disagreement rate**, and shadow p50/p99 latency.

**Objective for threshold/weight fitting:** maximize **recall subject to FP-rate <= 5%** per
axis — false escalations are the visible, costly error in the demo; false negatives are the
expensive error in production.

**Guardrails on the work itself:** the existing suite must stay green (measured after this
work: **424 Rust tests, 0 failures**), the
fast-path **benchmark** must not regress (it is untouched), and no accuracy claim is
published without the ablation numbers behind it.

---

## 13. Test plan

```text
[x] unit: each primitive -> ShadowVerdict for every threshold band
[x] unit: head+tail truncation and chunk max-pooling (truncated=true recorded, window count disclosed)
[x] unit: fail-open - disabled, HTTP 500, timeout, unreachable => zero verdicts, no panic
[x] unit: fusion - noisy-OR, corroboration rule, compound-risk escalation, unfitted pass-through
[ ] unit: precision-anchored injection rule (regex hit outranks Laya; regex miss allows Laya)
        -> NOT implemented as a special case. The fusion's corroboration + disagreement rules
           produce the same effect generically (a lone detector cannot escalate alone; a
           large judge/heuristic gap escalates with a reason). Kept as a TARGET: the
           precision-anchored form is strictly better once there are labels to fit it.
[x] unit: PII stack - the keyword heuristic is dropped when a stronger PII detector reported
[x] contract: fast-path has NO dependency on laya_client (CI grep / compile guard)
        -> enforced by services/fast-path/tests/contract_no_judge_in_fast_path.rs: it walks
           the dependency tables and the comment-stripped sources of fast-path and decision,
           and fails naming the offending file/token. `laya-` is allowed in `decision` only
           because that is how it names the verdicts it consumes; no HTTP client is.
[x] contract: fusion is pure; decision deterministic given scores+thresholds
[x] unit: sub-threshold readings are evidence-only and never actionable
[x] unit: a partial fit never weakens the detectors it does not cover
[x] integration: the REAL client against a REAL socket (fake `laya-serve`, `tests/common/mod.rs`)
        -> services/gateway/tests/laya_judge_contract_test.rs (23 tests): the Jev-compatible
           request shape, bearer auth, the `model` override, prompt/context presence, the
           typed question schema, one call per short response, chunk-and-max-pool across
           multiple HTTP round-trips (including a finding that exists ONLY in the region
           head+tail truncation discards), and fail-open on 5xx / malformed JSON / timeout /
           unreachable / partial-window failure.
[x] contract: the shadow -> decision wire format (`laya-` prefix, `-evidence` suffix)
        -> services/gateway/tests/hybrid_pipeline_test.rs (10 tests): verdicts produced by the
           real client are fed into the real `VerdictAggregator::fuse_evidence`, and
           `controlplane_decision::EVIDENCE_SUFFIX` is compared against the shadow crate's
           constant. Previously only a comment and a local literal pinned the pair.
[x] unit: the judge is OFF unless `DECISION_JUDGE=laya|jev`, and every judge setting is
        normalised (trim + case) so a padded value cannot silently disable it
        -> services/shadow-analysis/tests/shadow_config_env_test.rs (12 tests). This suite
           found a real defect and it is fixed: `LAYA_MODEL` was compared against the literal
           `"auto"` with no trim and no case-fold, so `LAYA_MODEL=AUTO` (or `" auto "`) was
           forwarded as a literal model name that `laya-serve` would reject — and because the
           judge fails open, that produced **no verdicts and no error**. `LAYA_URL`,
           `LAYA_API_KEY` and `LAYA_TIMEOUT_MS` were untrimmed in the same way.
[~] latency: shadow worker p99 < 2s with judge + guardrails + heuristics all enabled
        -> the harness reports judge p99 (scripts/eval_accuracy.sh); a full end-to-end
           shadow p99 with the judge enabled has not been measured on real GPU hardware.
[~] calibration: ECE/Brier recorded before enabling gating
        -> the harness computes Brier and ECE per configuration. The gate (calibrated=TRUE)
           is manual and documented, but no ECE threshold is enforced in code.
[~] integration: a poisoned response cannot change the judge's output schema
        -> guaranteed by construction (typed `choice`/`score` answers, no free-text channel).
           Client-side robustness on hostile input is now tested: an unsupported `noul`
           primitive yields NO verdict rather than a guessed polarity, a `choice` reply with
           no distribution falls back to its confidence, and malformed JSON yields nothing.
           Still missing: a test that points the client at a hostile *server* which changes
           the schema mid-conversation.
[ ] integration: non-English injection is caught (recall test vs the current regex baseline)
        -> needs the Laya container running; not covered by the default test suite.
[x] regression: cargo test --workspace + npx vitest run stay green
        -> measured: cargo test --workspace = 424 passed / 0 failed; vitest = 115 pass, plus one PRE-EXISTING unrelated failure
           in frontend/src/__tests__/components/header.test.tsx ("renders app selector with
           options" expects 4 options, gets 1) which fails on main too, with no header code
           touched by this work.
```

---

## 14. Risks & honesty notes

| Risk | Detail | Mitigation |
|---|---|---|
| **Laya zero-shot is weak on our domain** | 0.362 vs 0.461 majority baseline on its own benchmark | Ensemble, do not replace; `typed-decisions` for security/agent; fine-tune later |
| **Over-confidence** | Raw ECE 0.213–0.466 | Temperature-fit before gating (§5.4) |
| **`noul` label-following bug** | Confident "no" on positive input on the English checkpoint | Neutral `A`/`B` `choice` for all critical questions (§8) |
| **High-cardinality collapse** | > 20 options degrades sharply | <= 20 options, coarse-to-fine, or raise `head_max_len` |
| **Context truncation** | Long responses silently truncated | head+tail + chunk-and-pool; log `truncated`; measure |
| **Non-Latin scripts** | English checkpoint confidently wrong | `Router(preload=True)` mandatory |
| **Correlated detectors** | Fusing two engines that see the same tokens adds noise | Complementarity test (§5.3); precision-anchored injection rule |
| **Double-counting** | Two detectors for one phenomenon can inflate p | Weights `w_i` from the offline fit; clamp and validate via ablation |
| **Image size / RAM** | English ~808MB + multilingual ~647MB | Opt-in (`DECISION_JUDGE=off` default); pin `laya>=0.3.11` |
| **Vendor benchmarks** | Laya's vs-Jev numbers are self-reported | Quote only what we measure via the harness; do not repeat vendor deltas as fact |
| **Contract violation** | A judge in the fast path breaks `AGENTS.md` | Shadow-only; CI guard + code-review rule |
| **Unverified request-state shape** | The client sends `state` as an object `{response, prompt, context}` (§8.1), while the documented Jev raw example sends `state` as a plain string. Response parsing and the whole first-responder contract are covered by real-socket tests, but the request `state` shape has **not** been confirmed against a running `laya-serve`. | Before claiming end-to-end parity, start the container (`docker compose --profile judge up`) and confirm one live round-trip. |
| **`noul` is parsed but not consumed** | `LayaAnswer` deserialises `noul`, and no mapping reads it (critical yes/no questions are `choice` A/B by design, §8). If a backend answers a `choice` question with `noul`, the reading is silently dropped — and a silent no-op is invisible because the judge fails open. | Pinned by `a_noul_only_answer_is_not_consumed` so the behaviour is deliberate; if a live backend returns `noul`, add the polarity mapping rather than deleting the test. |
| **Accuracy is not ground truth** | Both heuristics and the judge approximate a "correct" label | Keep the reviewer feedback loop; track per-check precision; use the ablation |
| **Hybrid applied where it does not help** | e.g. toxicity already has a strong model | The matrix explicitly says KEEP / REPLACE for those checks |

**Framing for the demo (honest version):** *"We did not swap a heuristic for a model and
hope. For each check we picked the best engine, combined only where the error profiles are
complementary, fused the results deterministically, and tuned the weights against our own
reviewer labels — with an ablation that measures the gain per axis."*

---

## 15. Sources

- Laya homepage — https://laya.convaiinnovations.com/ (fetched 2026-09-24)
- Laya model card / hub — https://huggingface.co/convaiinnovations/laya (fetched 2026-09-24):
  checkpoints, `laya-serve`, `noul` issue #156, `act_probability` issue #185, honest limits,
  temperature-calibration numbers, benchmark table
- Laya source — https://github.com/NandhaKishorM/laya · https://pypi.org/project/laya/
- Repo ground truth — `docs/analysis/checks-inventory.md`, `docs/analysis/repo-audit.md`,
  `docs/analysis/executive-briefing.md`
- Prior proposal (superseded by this document) — `docs/analysis/jev-laya-integration.md`
- TypeSafe AI (Jev) — the Jev-compatible protocol `laya-serve` implements; relevant only as
  the swap target behind `DECISION_JUDGE=jev`
