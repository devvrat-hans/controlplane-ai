> **Update (2026-09-29):** DeepEval and the general Laya decision judge were removed.
> Hallucination is now checked by **Laya** with a dedicated 3-question call
> (`laya-hallucination` / `laya-groundedness`), only when the request carries grounding
> context. The guardrails sidecar now runs PII, toxicity and bias only. Entries below
> that describe `deepeval-hallucination` or the batched judge are historical.

# ControlPlane.ai — Governance Check Inventory & Decision Flow

> **What this document is:** for every governance check, *how* it is implemented, *which*
> open-source library powers it, **and — new in this revision — the exact path a verdict
> takes from a score to an action** (`pass` / `edit` / `escalate` / `block`).
>
> **Grounded in code, not the pitch.** Every threshold and `check_name` below was read
> from `services/` on **2026-09-25**. Where an older doc overstates a check, this doc says
> so. Where the running system diverges from the intended architecture, it is called out
> in **§8 "As-built reality"** so nobody is caught out in Q&A.

---

## 0. Read this first — how a decision is actually taken (60 seconds)

There are **four stages**, and only the first can change what the user receives.

```text
   ┌───────────────┐   ┌───────────────┐   ┌───────────────┐   ┌───────────────┐
   │ A. FAST PATH  │   │ B. SHADOW     │   │ C. DECISION   │   │ D. CONSUMERS  │
   │ sync, <50 ms  │   │ async, <2 s   │   │ aggregation   │   │ side effects  │
   └───────────────┘   └───────────────┘   └───────────────┘   └───────────────┘
   6 deterministic     5 native heur.       worst-outcome +     escalation case
   checks. CAN         + 3 sidecar         compound-risk +     audit record
   BLOCK, REDACT       models + optional   (opt-in) fusion +   cost entry
   or PASS.            judge. ADVISORY     precedent loop      notification
                       ONLY — never        → final outcome     dashboard / SSE
                       alters delivery     pass/edit/block/
                                           escalate
```

| Stage | Where | Can it change the delivered response? | Failure mode |
|---|---|---|---|
| **A. Fast-path** | in-process, `proxy/handler.rs` → `fast_path/engine.rs` | **YES** — can `block` (403) or `edit` (redact) | **fail open** (panic/timeout ⇒ `pass`) |
| **B. Shadow** | `shadow-analysis` worker, NATS | **NO** — advisory verdicts only | **fail open** (no verdict ⇒ pass) |
| **C. Decision** | `decision` crate (`VerdictAggregator`) | determines the *recorded* final outcome | **fail open** (no fit ⇒ legacy aggregator) |
| **D. Consumers** | escalation / audit / cost / notification / dashboard | records & alerts | each fails independently |

**The one-sentence version for the jury:** *the fast path is the only thing that can act
before the user sees the answer; everything else produces evidence, and a deterministic
aggregator turns that evidence into a recorded outcome — no model is ever in the decision.*

### Outcome severity order (memorise this — it drives `worst()`)

```rust
Pass = 0  <  Escalate = 1  <  Edit = 2  <  Block = 3
```

`Outcome::worst(a, b)` returns the **numerically higher** value. Anything that folds
verdicts together uses it. So:

- `Block` beats everything.
- `Edit` beats `Escalate`.
- `Escalate` beats `Pass`.

> ⚠️ A common trap: **`Escalate` is *not* "worse than Edit"**. It is *human review*, which
> is a **different** action from *automatic redaction*, not a stronger one. The code does
> things like `escalate_at_least()` when it deliberately wants to *add* a human without
> removing an automatic edit.

---

## 1. Count reconciliation (corrected)

| Count | What it is | Where |
|---|---|---|
| **14** | **Product checks** — the number to quote (6 fast + 8 shadow) | this doc, §3 |
| **6** | Fast-path `check_name`s: `unsafe_content`, `secret_detection`, `cost_cap`, `retry_detection`, `tool_use_detection`, `session_risk_accumulator` | `fast-path/src/checks/*` |
| **5** | Native shadow checks: `prompt_injection`, `groundedness`, `verbosity`, `semantic_pii`, `bias_classification` | `shadow-analysis/src/*` |
| **2** | Active guardrails sidecar checks: `presidio-pii`, `llm-guard-toxicity` (hallucination moved to Laya) | `guardrails/main.py` |
| **+2** | Input-side re-scans (`input-toxicity`, `input-bias`) — the *same* engines as toxicity/bias, renamed | `shadow-analysis/src/worker.rs` |
| **+1** | `fast-path-summary` — a **synthetic `Pass`** emitted 3× (one per axis) when no fast-path check fires, so the dashboard's axis breakdown is even | `proxy/src/handler.rs` |
| **8** | **Opt-in** judge detectors `laya-*` (+ their `-evidence` variants) | `shadow-analysis/src/laya_client.rs` |

**Two corrections to the previous revision:**

1. The judge defines **8** detectors, not 7 — `laya-verbosity` (Cost axis) was missed.
   So "distinct check_names including the judge" is **23 detectors**, and **31** distinct
   strings once you count the 8 `<name>-evidence` variants.
2. **`llm-guard-bias` is no longer emitted at all.** Response-side bias scanning is
   disabled in the worker (`guardrails_bias_handle = None`, with the comment *"Bias on
   response text produces too many false positives"*). Bias now only runs **on the input
   prompt**, and the verdict is renamed **`input-bias`**. `bias_classification` (the native
   heuristic) still scans the response.

> ⚠️ **"13 or 14?"** — **14**: 6 fast-path + 8 shadow-path. The old "13" omitted
> tool-use detection.

---

## 2. Stage A — the fast path (the only stage that can act)

**Entry point:** `services/proxy/src/handler.rs` → `run_fast_path_safe()` →
`services/fast-path/src/engine.rs::evaluate_with_app_context()`.

Every request is buffered, forwarded upstream, and the **response body** is evaluated
synchronously **before delivery**.

### 2.1 Execution order (short-circuits on the first `Block`)

| Order | Check | Axis | Cost | Why this order |
|---|---|---|---|---|
| 1 | `unsafe_content` | Responsibility | cheapest | Most critical; a hit returns immediately |
| 2 | `secret_detection` | Responsibility | regex | Produces the edits that must be applied |
| 3 | `cost_cap` | Cost | integer | Can block before we bother with the rest |
| 4 | `retry_detection` | Cost | map lookup | Session-scoped |
| 5 | `tool_use_detection` | Responsibility | substring | Sets the risk multiplier |
| 6 | `session_risk_accumulator` | Responsibility | counter | Needs to know if anything else fired |

Between each step the engine checks `start.elapsed() > FAST_PATH_BUDGET_MS` (**50 ms**)
and **returns early** with the outcome accumulated so far.

### 2.2 The three fast-path actions — exact mechanics

**① `Block` → the user never sees the model's answer.**

```jsonc
HTTP/1.1 403 Forbidden
{
  "error": {
    "code": "blocked_by_policy",
    "message": "<reason from the winning verdict>",
    "correlation_id": "0193…"
  }
}
```
On block the handler **returns immediately**: `intercept.captured` is emitted (cost), but
the shadow-path publish is **never reached**.

**② `Edit` → the user sees a modified answer.**

Edits come only from `secret_detection`. Each finding produces a
`ResponseEdit { original, replacement }` where `replacement = [REDACTED:<CATEGORY>]`
(e.g. `[REDACTED:AWS_ACCESS_KEY]`). The handler applies them with a **literal string
replace over the whole response body**:

```rust
for edit in &fast_path_result.edits {
    body_str = body_str.replace(&edit.original, &edit.replacement);
}
```

**③ `Escalate` → the user still gets the answer; a human gets a queue item.**
The delivered body is untouched. The verdict flows to the escalation listener (§5).

**④ `Pass` → delivered verbatim**, plus three synthetic `fast-path-summary` verdicts so
the dashboard shows a balanced 3-axis breakdown.

### 2.3 Response headers on every call

| Header | Value |
|---|---|
| `X-ControlPlane-Correlation-Id` | the call's UUID (the join key across every service) |
| `X-ControlPlane-Latency-Ms` | **fast-path** latency (not total) |

### 2.4 Fail-open guarantee

`run_fast_path_safe` wraps the engine in a **50 ms `tokio::timeout` + `spawn_blocking`**.
Any panic, or exceeding 50 ms, returns `FastPathSafeResult::pass()` — the response is
delivered as-is. **A broken governor must never break the product.**

---

## 3. Every check, and exactly which outcome it produces

**Legend** — ✅ REAL (dedicated engine) · ⚠️ HEURISTIC (keyword/overlap) ·
🔶 DEGRADED (falls back) · 🟣 OPT-IN (judge off by default)

### 3.1 Fast path (6)

| # | `check_name` | Trigger | Outcome as a function of the signal | Confidence |
|---|---|---|---|---|
| 1 | `unsafe_content` | Lowercase substring vs **9 built-in phrases** + policy `unsafe_keywords` | **always `Block`** (0.95 built-in / 0.90 policy) | 0.95 / 0.90 |
| 2 | `secret_detection` | 8 regexes + Shannon entropy + Luhn | **always `Edit`** when ≥1 finding; redacts every hit | max finding (0.75–0.98) |
| 3 | `cost_cap` | output tokens vs per-app cap | `> cap` → **Block**; `> 90%` → **Escalate**; `> 75%` → **Edit**; else nothing | 0.99 / 0.75 / 0.60 |
| 4 | `retry_detection` | count in sliding window > `retry_max_count` | **`Escalate`** | 0.85 |
| 5 | `tool_use_detection` | tool-call JSON markers, or dangerous directives (`rm -rf`, `DROP TABLE`, `DELETE FROM`, `sudo `, …) | dangerous directive → **`Escalate`**; structured tool use alone → no verdict but sets `has_tool_use` | 0.80 |
| 6 | `session_risk_accumulator` | ≥ **3** non-pass events for the session in **3600 s** | **`Escalate`** | 0.75 |

**Bonus rule — the tool-use risk multiplier.** If `tool_use_detection` found *any* tool
use, every non-pass fast-path verdict's confidence is multiplied by **1.5×** (capped at
1.0) because actions have higher downstream impact than text.

**`secret_detection` pattern detail** (the strongest fast-path check):

| Category | Validation | Conf. |
|---|---|---|
| AWS access key | `AKIA[0-9A-Z]{16}` | 0.98 |
| AWS secret key | `secret_key = <40 chars>` | 0.95 |
| Generic API key/token | `api_key/token/secret/password = <20–64 chars>` **+ entropy > 3.5** | entropy/5 ≤ 0.95 |
| Credit card | 16 digits **+ Luhn** | 0.97 |
| US SSN | `\d{3}-\d{2}-\d{4}` | 0.90 |
| Aadhaar (IN) | 12 digits, not starting 0/1 | 0.85 |
| Email | email regex | 0.80 |
| Phone | ≥10 digits **and** contains a separator | 0.75 |

> Honest note: every email and separated phone number is redacted at 0.75–0.80. Aggressive
> by design; expect noise on real traffic.

### 3.2 Shadow path — native heuristics (5)

| # | `check_name` | Axis | Trigger → outcome | Confidence |
|---|---|---|---|---|
| 7 | `prompt_injection` | Responsibility | 21 weighted patterns + structural bonuses; score ≥ **0.90** → **Block**, ≥ threshold (**0.5**) → **Escalate** | the score |
| 8 | `groundedness` | Performance | sentence word-overlap vs context; score < **0.6** → **Escalate**; **no context ⇒ no verdict** | `1 − score` |
| 9 | `verbosity` | Performance | `0.4·density_penalty + 0.6·ratio_penalty`; score > **0.6**, or (density < 0.3 **and** ratio > 10) → **Escalate** | the score |
| 10 | `semantic_pii` | Responsibility | quasi-identifier categories; `categories ≥ 3` **and** `risk ≥ 0.5` → **Escalate** (`risk = categories/4`) | risk |
| 11 | `bias_classification` | Responsibility | weighted keyword hits across protected categories; score > **0.7** → **Escalate** | the score |

All five are **input-independent of the fast path**: they run in parallel `tokio::spawn`
tasks and each contributes only if its toggle is on.

### 3.3 Shadow path — guardrails sidecar (3 active)

Python FastAPI service (`services/guardrails/main.py`, `:8200`), called over HTTP with a
**5 s timeout**.

| # | `check_name` | Engine | Outcome band | Confidence |
|---|---|---|---|---|
| 12 | `presidio-pii` | **Microsoft Presidio** + spaCy NER, 12 sensitive entity types @ score ≥ 0.7 | entities ≥ 3 → **Escalate**, else **Edit** | max entity score |
| 13 | `llm-guard-toxicity` | **HuggingFace** `unitary/unbiased-toxic-roberta` | score > 0.9 → **Block**, > 0.7 → **Escalate**, else **Edit** | score |
| 14 | ~~`deepeval-hallucination`~~ | **Removed** — replaced by `laya-hallucination` (Laya, 3-question call; **no context ⇒ skip**) | see the Laya table | — |

> The `llm-guard-*` names come from a log line, but **no `llm-guard` dependency exists** —
> these are raw `transformers` pipelines. The models are real; the label is wrong.

### 3.4 Input-side re-scans (aliases, not new engines)

| `check_name` | Same engine as | Applied to |
|---|---|---|
| `input-toxicity` | `llm-guard-toxicity` | the **user prompt** |
| `input-bias` | `llm-guard-bias` (which no longer scans responses) | the **user prompt** |

### 3.5 PII de-duplication (important for precision)

`worker.rs::demote_semantic_pii` — when **Presidio *or* the judge** reported PII on the
same response, the keyword `semantic_pii` verdict is **dropped**. The heuristic survives
only when neither stronger detector reported (i.e. as the fail-open fallback). This is a
deliberate anti-double-counting rule.

---

## 4. Stage B → C: how the verdicts become one decision

### 4.1 Shadow verdicts are advisory

The shadow path **never** touches the delivered response. It publishes
`controlplane.verdict.shadow` for each finding. Absence of a verdict means pass.

### 4.2 Default aggregation (`aggregate_with_reasoning`)

1. `final_outcome = max(verdict.outcome)` over all verdicts (`worst`) — see the severity
   order in §0.
2. **Compound-risk escalation:** count distinct axes that produced a non-pass verdict.
   If **≥ 2 axes fired** and the worst was `Pass` or `Edit`, the outcome is raised to
   **`Escalate`** — the "hallucination + privacy = compound risk" rule.
3. **Primary reason** = the reason of the highest-confidence verdict at the final
   severity level (falling back to the highest-confidence contributor when compound risk
   raised the outcome).

### 4.3 Calibrated fusion (`fuse_evidence`) — opt-in and inert by default

Engages **only** when `detector_calibration` has rows with `calibrated = TRUE` (it ships
with none). Per axis, deterministic:

1. **Collapse to one reading per detector** (keeps the strongest) — prevents
   double-counting the same evidence.
2. **Discard readings below the evidence floor** (`JUDGE_EVIDENCE_THRESHOLD`, default 0.45).
3. **Weighted noisy-OR:** `p_axis = 1 − Π(1 − wᵢ·pᵢ)`.
4. **Threshold map:** `p ≥ 0.90` → Escalate; `p ≥ 0.70` → Edit; else Pass.
5. **Corroboration rule:** a *lone heuristic* above threshold is capped at **Edit**
   (one over-eager rule may not escalate alone). A calibrated judge ≥ 0.90 may act alone.
6. **Disagreement routing:** if judge and heuristics differ by ≥ **0.40** on an axis, the
   case is raised to at least `Escalate` **with an explicit reason** — and that conflict
   becomes training data for the next fit.
7. **Unfitted detectors pass through with exactly their existing authority** — a partial
   fit can never silently weaken an un-fitted check.
8. Compound-risk escalation is re-applied on top.

> The whole fusion is a **pure function of scores + stored thresholds**. The decision crate
> *cannot* call an HTTP client — `contract_no_judge_in_fast_path.rs` asserts it.

### 4.4 The feedback loop (reviewer precedents)

`decision/src/router.rs` + `feedback.rs`, using `pg_trgm` trigram similarity (no
embeddings, no LLM):

- **Annotate:** similar past reviewer decisions are appended to the reason as
  `[Learned] …` and their IDs returned as `consulted_precedents`.
- **Auto-suppress:** if a **≥ 60 %-similar** precedent was `dismiss`/`override` **and**
  the current outcome is `Edit`/`Escalate`, it is downgraded to **`Pass`** with an
  explanatory reason.

Every escalation resolution (confirm / override / dismiss) is written to
`reviewer_overrides` and announced on `controlplane.feedback.recorded`.

---

## 5. Stage D: what happens to the final outcome

| Consumer | Subscribes to | Action |
|---|---|---|
| **Escalation** | `controlplane.verdict.*` | `outcome == Escalate` → create an `escalation_cases` row (`status = open`), dedup by `verdict_id`, plus a similar-precedent suppression check. Overrides publish `policy.reload`. |
| **Audit** | `controlplane.verdict.*` | Append a hash-chained record: `SHA-256(prev_hash + call_id + verdict_id + action + timestamp)`. **Append-only** — no UPDATE/DELETE, ever. |
| **Cost accounting** | `controlplane.intercept.captured` | Per-request token cost into `cost_entries`. |
| **Notification** | `controlplane.decision.final` | Slack / webhook for `block` / `escalate`, rate-limited (5 per app per 60 s). |
| **Dashboard API** | `controlplane.verdict.*` | SSE push to the browser + persist verdicts for queries. |

### Two worked examples

**Example 1 — a leaked AWS key in a clean answer.**
`unsafe_content` pass → `secret_detection` finds `AKIA…` → verdict `{Edit, 0.98}` + edit
`[…](AKIAIOSFODNN7EXAMPLE) → [REDACTED:AWS_ACCESS_KEY]`. Fast path returns `Edit`; the
edited body is delivered with 200 OK. Shadow path still runs. If no second axis fires,
the final outcome is **Edit**; no escalation (escalation only listens for `Escalate`).

**Example 2 — a prompt-injection attempt the model complied with.**
Fast path passes (nothing unsafe in the *output*). Shadow: `prompt_injection`
(Responsibility) scores 0.94 → **Block**; `groundedness` (Performance) drops to 0.5 →
**Escalate**. **Two distinct axes fired ⇒ compound risk**, which only matters if the worst
is `Pass`/`Edit` — here the worst is already `Block`, so the final outcome is **`Block`**.
An audit record is written per verdict; a notification would fire from the final-decision
event (see §8 for its runtime status).

**Example 3 — near a token cap on a RAG answer.**
`cost_cap` (Cost) = 92 % of cap → `Escalate`; `groundedness` (Performance) = 0.5 →
`Escalate`. Two axes ⇒ compound risk, but the worst is already `Escalate`, so the final
outcome stays **`Escalate`**. The user still receives the answer; a reviewer gets a case.

> These examples describe the aggregator's own logic, which is deterministic and fully
> unit-tested. **Whether that step is actually reached at runtime is a separate matter —
> see §8, gap 1.** For the jury, the safe framing is: *fast-path actions are live today;
> shadow-derived final outcomes and their downstream alerts are implemented and tested,
> and §8 lists exactly which wiring is still incomplete.*

---

## 6. The optional decision-model judge (`off` by default)

| Property | Value |
|---|---|
| Where it runs | `services/laya` container (`laya-serve`, `:8300`), **shadow path only** |
| Master switch | `DECISION_JUDGE=laya\|jev` — **unset/`off` ⇒ the judge is never called** |
| Per-app switch | `checks.decision_judge_enabled` (can only opt **out**) |
| Calls | **one** `POST /v1/systemone` per response window (max 8 windows, **max-pooled**) |
| Questions | 11–12 typed questions per call; no `noul` primitive (label-following bug) |
| Calibration | temperature scaling from `detector_calibration`; **inert until a fit exists** |
| Authority | **none** — emits scores; the decision engine applies thresholds |
| Fail-open | any error/timeout/garbage ⇒ **zero verdicts** |

**Judge thresholds** (`laya_client.rs`): `≥ 0.90` → **Escalate**; `≥ 0.70` → **Edit**;
`0.45–0.70` → **`Pass` evidence** verdict named `<detector>-evidence` (never actionable);
`< 0.45` → discarded.

**Per-question outcome rules:**

| Judge detector | Axis | Rule |
|---|---|---|
| `laya-hallucination` | Performance | severity ≥ 0.66 ⇒ Escalate regardless of the binary answer |
| `laya-groundedness` | Performance | inverted polarity (strong support = low risk) |
| `laya-prompt-injection` | Responsibility | carries the attack family into the reason |
| `laya-tool-use` | Performance | `destructive`/`high` ⇒ Escalate, `medium` ⇒ Edit, `low` ⇒ nothing |
| `laya-bias` | Responsibility | carries the category |
| `laya-toxicity` | Responsibility | **top tier only** — Toxic-BERT owns the middle |
| `laya-semantic-pii` | Responsibility | contextual re-identification Presidio cannot see |
| `laya-verbosity` | Cost | low-stakes; can raise an Edit, never Escalate |

---

## 7. Configuration reference

**Fast path** (`FastPathRuleSet`; per-app overrides come from `policies` rows)

| Setting | Default | Notes |
|---|---|---|
| `max_tokens_per_request` | `None` | Merged per-app via request context (not globally) |
| `retry_max_count` / `retry_window_seconds` | 5 / 60 | Lowered per app in seed data |
| `unsafe_keywords` | `[]` | Union of all apps' `responsibility.unsafe_keywords` |
| `secret_detection_enabled` / `cost_cap_enabled` | `true` | `pii_action = "off"` disables |
| **fast-path budget** | **50 ms** | `FAST_PATH_BUDGET_MS` + proxy timeout |

**Shadow path** (`ShadowConfig`)

| Setting | Default |
|---|---|
| `groundedness_threshold` | 0.6 |
| `bias_threshold` | 0.7 |
| `verbosity_max_ratio` / `verbosity_min_density` | 10.0 / 0.3 |
| `semantic_pii_min_identifiers` / `_risk_threshold` | 3 / 0.5 |
| `prompt_injection_threshold` | 0.5 |
| `pii_enabled` / `toxicity_enabled` / `bias_enabled` | `true` |

**Guardrails sidecar** — `TOXICITY_THRESHOLD=0.5`, `BIAS_THRESHOLD=0.66`, `PORT=8200`.

**Hard-coded** — session risk = 3 events / 3600 s; tool-use multiplier = 1.5×;
judge HTTP timeout = 5000 ms; guardrails HTTP timeout = 5 s.

---

## 8. As-built reality — gaps to know before Q&A

The honesty items. These are the answers to the questions a sharp jury will ask.

| # | Finding | Detail |
|---|---|---|
| **1** | **No final decision is published at runtime** | `controlplane.decision.final` is emitted **only** by `aggregate_verdicts`, an HTTP handler on `decision_router` — and the gateway **never mounts that router**. The background verdict collector only *persists* verdicts. **Consequence: the notification worker subscribes to an event that never arrives, so Slack/webhook alerts never fire.** Audit and escalation still work because they subscribe to `controlplane.verdict.*` directly. |
| **2** | **Cost accounting only sees blocked calls** | `publish_call_async` (the only publisher of `intercept.captured`) is called **only in the `Block` branch**. Passing/edited requests are never costed. |
| **3** | **Model is hardcoded `"unknown"`** | Both the shadow request and the cost event carry `model: "unknown"`, so pricing always falls back to the default $3/$15 per M rate. |
| **4** | **Fast-path instant reload is wired to the wrong subject** | The dashboard publishes `controlplane.policy.updated` (which the *toggle* reloader listens to), but the fast-path reloader listens on `controlplane.policy.reload`. So saving a policy hot-reloads shadow toggles but **not** the fast-path cache (30 s poll catches it). |
| **5** | **Shadow analysis never runs on blocked responses** | The `Block` branch returns before the shadow publish. |
| **6** | **`dashboard-api` bypasses the service contracts** | `router.rs` (3,099 lines) performs **direct SQL writes** to `policies`, `apps`, `users`, `api_keys`, `escalation_cases` and inserts `reviewer_overrides` — duplicating the escalation and decision services, which `AGENTS.md` forbids for this crate. The `audit_router`, `cost_router`, `escalation_router`, `decision_router` and `policy_crud_router` are compiled but **never served**. |
| **7** | **Auth is demo-mode** | The middleware lets unauthenticated requests through (it only rejects a *present-but-invalid* token). **No server-side role checks exist** — `canEditPolicies` etc. are frontend-only. JWT secret defaults to `change-me-in-production`; CORS defaults to `AllowOrigin::Any`. |
| **8** | **Budget mismatch** | `AGENTS.md` states a **25 ms** hard fast-path budget; the code uses **50 ms** in both the engine and the proxy timeout. |
| **9** | **Scaffold not wired** | `proxy/src/rate_limiter.rs` is implemented + tested but **never called** (so `rate_limit.enabled = true` is not enforced); `interceptor.rs` is a no-op; `dashboard-api/websocket.rs` is a design doc; `PatternPromoter` is implemented + tested but never constructed. |
| **10** | **Transport is not trustworthy** | `app_id`, `profile_id` and `session_id` are read from the request body/header with no auth, so a client can select a lax regulatory profile to raise its own caps. |

**What is genuinely strong** (say these too): fail-open is honoured at every layer; the
decision path is deterministic and structurally denied an HTTP client; calibration is
fail-safe and inert until fitted; the audit chain is append-only and verified; and every
check above has tests (**~424 tests, all passing**, `cargo check` clean).

---

## 9. Where each check runs (annotated)

```text
Client ──▶ Proxy (:8900)
              │
              ├── FAST PATH (sync, <50 ms, CAN ACT — fail open)
              │     1 unsafe_content            → Block
              │     2 secret_detection          → Edit (+ redaction applied)
              │     3 cost_cap                  → Block / Escalate / Edit
              │     4 retry_detection           → Escalate
              │     5 tool_use_detection        → Escalate (+1.5× confidence)
              │     6 session_risk_accumulator  → Escalate
              │
              └── SHADOW PATH (async, <2 s, ADVISORY — cannot alter delivery)
                    ├── native Rust heuristics
                    │    7  prompt_injection       → Block / Escalate
                    │    8  groundedness           → Escalate (needs context)
                    │    9  verbosity              → Escalate
                    │    10 semantic_pii           → Escalate (dropped if Presidio/judge reported)
                    │    11 bias_classification    → Escalate
                    │
                    └── guardrails sidecar (Python, :8200)
                         12 presidio-pii            [Presidio + spaCy] → Escalate / Edit
                         13 llm-guard-toxicity      [toxic-roberta]     → Block / Escalate / Edit

     input-side re-scans:  input-toxicity, input-bias

HALLUCINATION — Laya (:8300), only when the request has grounding context:
                    └── laya (3 questions per call)
                         15 laya-hallucination   16 laya-groundedness
REMOVED — the batched decision judge that also produced:
                         17 laya-prompt-injection 18 laya-tool-use
                         19 laya-bias            20 laya-toxicity
                         21 laya-semantic-pii    22 laya-verbosity
                         (+ `<name>-evidence` Pass verdicts in the 0.45–0.70 band)

                        │
                        ▼  controlplane.verdict.fast / .shadow
     ┌──────────────────────────────────────────────────────────────┐
     │ DECISION: worst-outcome → compound-risk → (opt-in) fusion     │
     │           → precedent annotation / auto-suppress              │
     └──────────────────────────────────────────────────────────────┘
                        │
     ┌──────────────────┼───────────────────┬──────────────┬───────────────┐
     ▼                  ▼                   ▼              ▼               ▼
  escalation        audit (hash chain)   cost          notification    dashboard SSE
  (Escalate only)   (append-only)        (⚠ blocks     (⚠ inert —    (live verdicts)
                                          only)         see §8, gap 1)
```

---

## 10. One-paragraph summary for a reviewer

> ControlPlane runs **14 governance checks** across two paths. The **fast path** holds six
> deterministic checks — secret/PII regex with entropy and Luhn, unsafe keywords, cost
> caps, retry loops, tool-use and session risk — and is the **only** stage that can change
> what the user receives: it may return `403`, redact the answer, or pass it through, and
> it **fails open** in under 50 ms. The **shadow path** holds eight more: five native
> heuristics (prompt injection, groundedness, verbosity, semantic PII, bias) plus a Python
> guardrails sidecar using **real models** — Microsoft Presidio with spaCy for PII,
> HuggingFace toxic-roberta for toxicity; hallucination is scored by Laya. Shadow verdicts
> can **never** alter delivery; they are evidence. A **deterministic aggregator** folds the
> evidence into one outcome (worst-of, compound-risk escalation, optional calibrated
> fusion with a corroboration rule and judge-vs-heuristic disagreement routing), and that
> outcome drives human escalation, the append-only audit chain, cost, and alerts. **Three
> of the shadow checks are heuristics standing in for the model the architecture targets**
> (groundedness → NLI, semantic PII → NER, bias → ONNX), and hallucination is answered by
> the Laya decision model (uncalibrated until a fit exists) — documented upgrade paths,
> not surprises.
>
> **One caveat to state up front:** the fast-path actions (block / redact / pass) are live
> end-to-end today. The **shadow-derived final outcome, its alerts, and per-call costing
> are implemented and fully tested but not yet wired end-to-end** — §8 lists each one,
> with the exact file and cause. Volunteering that list is stronger than being asked for it.
