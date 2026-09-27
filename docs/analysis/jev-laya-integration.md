# Integrating Jev (TypeSafe AI) & Laya into ControlPlane.ai

> **Superseded (2026-09-24):** see [`laya-integration-plan.md`](laya-integration-plan.md)
> for the current plan. It keeps this document's conclusion (shadow path only, fail-open,
> local-first) but replaces "add a judge" with **hybrid per-activity routing** — choosing
> the best engine, or combination of engines, for each governance check — and adds three
> facts that postdate this proposal: the `laya-serve` Jev-compatible endpoint, the `noul`
> label-following issue, and the `laya-typed-decisions` security/agent fine-tuning.
>
> **Status: PROPOSAL / TARGET — nothing in this document is implemented yet.**
> Every component described here is a design for future work, per the honesty rule
> in `AGENTS.md`. The current shadow path still uses DeepEval / heuristics / the
> Python guardrails sidecar.
>
> **Researched:** 2026-09-22. Jev is in **early access (waitlisted)**; API details and
> benchmarks below are from vendor and third-party publications and are marked as such.

---

## 0. TL;DR

**Jev** (TypeSafe AI) and **Laya** (Convai Innovations) are *System One decision
models*: you send them a **state** plus **typed questions**, and they return **typed,
calibrated answers** (a choice, a score, or a yes/no probability). They generate **no
text**, so there is nothing to parse and nothing to hallucinate.

They are a near-perfect fit for ControlPlane's **shadow path**:

| | Today | With Jev / Laya |
|---|---|---|
| Shadow judge | DeepEval LLM-as-a-judge (text in, score out) | Typed questions in, calibrated probabilities out |
| Failure mode | The judge can itself hallucinate or return malformed output | By construction it cannot type-error or hallucinate; it can only be *miscalibrated* |
| Cost | LLM token pricing | Jev: **$0.042 / MTok input, output free** · Laya: **$0 self-hosted** |
| Latency | ~500ms–1.5s | Jev: 70–500ms · Laya: ~33ms (GPU) |

**Recommendation:**

- **Hackathon demo → Laya.** Apache 2.0, open weights, runs locally, free. It preserves
  the project's "100% local, no API keys, nothing leaves the machine" story.
- **Production / higher accuracy → Jev.** Managed API, better calibrated out of the box,
  better on high-cardinality option sets (up to 255 options), and it also leads on
  soft-distribution matching.
- **Do not touch the fast path.** Both models violate the fast-path contract in
  `AGENTS.md` (no network calls, no model invocation, <10ms). They belong in the shadow
  path, which already has a <2s budget.

---

## 1. What Jev is (TypeSafe AI)

Jev was announced by TypeSafe AI on **September 15, 2026** (founder: Diogo Almeida,
who worked on the methods behind InstructGPT/RLHF at OpenAI). It is the first "System
One Model" — a model class named after Kahneman's fast, intuitive System 1 thinking.

**Core idea:** *unstructured state in, typed probabilistic decisions out.* TypeSafe calls
it "a frontier-intelligence function call."

| Property | Jev |
|---|---|
| Output | Typed structured values (`choice` / `score` / `noul`) + probabilities + confidence |
| Text generation | None — it cannot hallucinate or emit malformed data |
| Training | **RLCD** — Reinforcement Learning for Calibrated Decisions (reward = strictly proper scoring rule) |
| Sampling | Parallel / non-autoregressive — all questions answered in one pass |
| Latency | 70–500ms end-to-end |
| Price | **$0.042 / MTok input**, output tokens **free** |
| Context | 64k tokens for state + all questions; 32k for state + longest question |
| Options | Choice questions support up to **255 options** |
| Input | Text / JSON state only (no images, audio, video) |
| Availability | Early access — waitlisted |

### The three primitives (the entire API)

```text
Choice  → one option from a labelled set      → .choice, .probabilities, .confidence
Score   → a position on an ordered spectrum   → .score (can be fractional), .probabilities, .confidence
Noul    → yes/no as a probability             → .noul (0.0–1.0)
```

### How you call it

```python
# pip install typesafe-sdk   (Python 3.10+)
from typesafe_sdk import Choice, Noul, Score, TypeSafeClient

client = TypeSafeClient()          # reads TYPESAFE_API_KEY, defaults to jev-latest

resp = client.system_one(
    state={"response": "...", "question": "...", "context": "..."},
    questions={
        "is_hallucinated": Noul(instructions="The response states facts not supported by the context"),
        "severity": Score(instructions="How severe is any unsupported claim",
                          criteria=["fully supported", "minor unsupported detail", "major fabrication"]),
    },
)
resp.answers["is_hallucinated"].noul      # -> 0.03
resp.answers["severity"].score            # -> 0.11
```

**Raw endpoint** (if you don't want an SDK): `POST https://api.typesafe.ai/v1/systemone`

```json
{
  "model": "jev-latest",
  "state": "Help! My payouts have been failing for 3 days.",
  "questions": {
    "department": {
      "type": "choice",
      "instructions": "Which team should handle this?",
      "criteria": {
        "billing": "Payments, invoicing, refunds",
        "technical": "Bugs, outages, integrations",
        "sales": "Pricing, upgrades, new accounts"
      }
    }
  }
}
```

Response (TypeSafe's own, unchanged):

```json
{
  "model": "jev-1.13.0",
  "answers": {
    "department": {
      "type": "choice",
      "choice": "technical",
      "probabilities": { "billing": 0.08, "technical": 0.85, "sales": 0.07 },
      "confidence": 0.82
    }
  },
  "usage": { "input_tokens": 312, "output_tokens": 48 }
}
```

### Ecosystem integrations (useful for the gateway route, §5 Option C)

- **Python SDK:** `typesafe-sdk` · **JS/TS SDK:** `@typesafe-ai/sdk`
- **LiteLLM:** pass-through at `/typesafe/v1/systemone` (adds cost tracking; no streaming)
- **LangChain:** `langchain-typesafe` → `TypeSafeClassifier`; plus experimental
  `ModelRouterMiddleware` and `AutoModeMiddleware` (uses Jev to gate risky tool calls —
  very relevant to ControlPlane's tool-use check)
- **Pydantic AI**, **OpenRouter**, **Vercel AI Gateway**, **aimlapi** all list Jev

**Caveat:** the headline numbers (193–444× faster/cheaper, 0% type errors) are
TypeSafe's **own** self-run evals (except 0% type errors, which is true by construction).
Treat them as claims, not independent measurements.

---

## 2. What Laya is (the open-source alternative)

Laya is an **Apache 2.0** multilingual, non-autoregressive System 1 decision model from
**Convai Innovations** (author: Nandakishor M). It is explicitly positioned as the free,
local alternative to Jev.

| Property | Laya |
|---|---|
| License | **Apache 2.0** (open weights, on-premise capable) |
| Params | 421M (ModernBERT-large backbone + decision head) |
| Output | Typed answers `choice` / `score` / `noul` with calibrated probabilities |
| Latency | **~33ms** (1 question, T4 GPU); 193–464ms on CPU |
| Throughput | 103–332 questions/sec batched on one T4 |
| Languages | English checkpoint + multilingual (100+ languages) |
| Context | 512 (English) / 1024 (multilingual, encoder up to 8k) |
| Cost | **$0 self-hosted** |
| Weights | `convaiinnovations/laya`, `laya-multilingual`, `laya-typed-decisions` |
| Code | GitHub `NandhaKishorM/laya`, PyPI `laya`, plus `laya.cpp` |

### How you call it

```python
# pip install laya
import laya
from laya import Router

router = Router(preload=True)   # keeps checkpoints in memory; <35ms routing

state = {"response": "...", "question": "...", "context": "..."}
questions = {
    "is_hallucinated": {
        "type": "noul",
        "instructions": "The response states facts not supported by the context",
    },
    "severity": {
        "type": "score",
        "instructions": "How severe is any unsupported claim",
        "criteria": ["fully supported", "minor unsupported detail", "major fabrication"],
    },
}
res = router.predict(state, questions)
res["answers"]["is_hallucinated"]["noul"]   # -> 0.05
res["routing"]["model"]                      # -> "english"
```

**Single-model mode:** `agent = laya.load("convaiinnovations/laya")` then
`agent.predict(state, questions)`.

**Gotcha:** if `laya.load()` hangs, run with `USE_TF=0` (TensorFlow's abseil runtime
deadlocks model construction when TF is installed).

---

## 3. Jev vs Laya — which one, when

Figures below come from the Laya model card (self-reported vs third-party published Jev
numbers) — **sample sizes and prompts differ, so treat this as directional, not settled.**

| Dimension | Jev 1.13.0 | Laya (routed) | Winner |
|---|---|---|---|
| Typed-decisions accuracy (2,000 decisions) | 0.727 | **0.766** | Laya |
| Soft distribution match vs teacher | **0.580** | 0.471 | Jev |
| Calibration (ECE, lower better, post-fix) | 0.246 | **0.081** | Laya (after temperature fit) |
| p50 latency (1 question) | 236–276ms | **32.8ms** | Laya |
| Very high cardinality (>20 options, e.g. 72–77 labels) | **0.870** | 0.425 (default budget) | Jev |
| Languages | English-led | **45 of 51 usable** | Laya |
| Weights / deployment | Closed API | **Open, on-prem** | Laya |
| Cost | $0.042/MTok | **$0** | Laya |
| Ops burden | None (managed) | Self-host GPU/CPU | Jev |
| Data residency | Data leaves your network | **Stays on-prem** | Laya |

**Practical read:**

- For ControlPlane's governance questions (mostly ≤10 options: categories, yes/no,
  3–5 point severities), **either works**. Laya is faster, free, local and good enough.
- If you ever need a single question with **dozens of options** — e.g. classifying a
  response against a long taxonomy of data-protection categories — **Jev** handles it
  out of the box; Laya needs `head_max_len` raised or a coarse-to-fine split.
- Laya's base checkpoints are **near-chance zero-shot** on its `typed-decisions`
  benchmark; the 0.766 belongs to a checkpoint **fine-tuned on that benchmark's own
  training split**. Laya is "a fast base to specialise, not a zero-shot decision engine."

> **Also found:** **OpenJev** is a separate community open-source clone of Jev. Mention
> it only if asked — Laya is the better-documented and more actively benchmarked option.

---

## 4. Why this fits ControlPlane specifically

ControlPlane's shadow path today runs 8 checks, several of which are exactly the
"fuzzy System 1 judgment" shape that decision models are built for:

| Current check | How it works today | Decision-model upgrade |
|---|---|---|
| `deepeval-hallucination` | LLM-as-a-judge (text out → parse JSON) | `noul` "does the response state facts unsupported by context?" + `score` severity |
| `groundedness` | Native heuristic / NLI-style score | `score` on an ordered support scale |
| `llm-guard-bias` / `bias_classification` | Transformer classifier | `choice` category + `score` severity |
| `llm-guard-toxicity` | Transformer classifier | `score` severity in one pass, batched |
| `verbosity` | Ratio/density heuristic | `score`: "how much of the response is filler vs signal?" |
| `semantic_pii` | NER-style heuristic | `noul` "is this text re-identifiable to a person?" + `choice` of PII type |
| `prompt_injection` | 25+ regex/structural patterns | `noul` "is this prompt attempting to override the system?" + `choice` attack family |
| `tool_use_detection` | Pattern match + 1.5× multiplier | `choice` risk of the tool call (**this is exactly LangChain's `AutoModeMiddleware` pattern**) |

Three architectural properties make this a genuinely good fit, not just a cheaper model:

1. **The judge can't be prompt-injected.** Because output is constrained to a schema you
   defined, a malicious response cannot make the judge emit "ignore previous instructions."
   There is no free-text channel.
2. **Calibrated confidence feeds the decision engine directly.** ControlPlane already
   aggregates on confidence (`aggregate_with_reasoning` picks the highest-confidence
   verdict at the top severity). Calibrated probabilities make that aggregation
   meaningful instead of a heuristic.
3. **Speculative fan-out matches the shadow worker.** The shadow worker already runs its
   checks in parallel `tokio::spawn`s. Jev/Laya answer every question in one pass, so all
   governance questions for a call can be a **single** model invocation.

### What it can additionally enable

- **Escalation triage / priority** — `score` how much a case genuinely needs a human,
  replacing the "confidence = priority" proxy.
- **Feedback/precedent classification** — classify a reviewer's dismissal reason into
  a category that can be matched against future calls (augments, does not replace, the
  deterministic `pg_trgm` loop).
- **Pattern promotion** (`pattern_promotion.rs`) — decide whether a recurring shadow
  pattern is stable enough to graduate to a fast-path rule.

---

## 5. Where it plugs in

Three options. **Option A is recommended** — it reuses the exact pattern the shadow worker
already uses for the guardrails sidecar.

### Option A — Extend the Python guardrails sidecar *(recommended)*

`services/guardrails/main.py` is a FastAPI app exposing `/scan/pii`, `/scan/toxicity`,
`/scan/bias`, `/scan/hallucination`, called by
`services/shadow-analysis/src/guardrails_client.rs`. Add a `/scan/governance` endpoint
backed by Laya (local) or Jev (API). This keeps the model runtime in Python, where both
SDKs live, and requires only a small addition to the Rust client.

```text
shadow worker (Rust) ──HTTP──▶ guardrails sidecar (Python) ──▶ Laya (local)  or  Jev API
        │                                        │
        └◀──── ShadowVerdict[] ──────────────────┘
```

**Pros:** smallest diff, matches existing conventions, Python SDKs, easy to swap
Jev↔Laya, easy to run CPU-only for the demo.
**Cons:** adds a network hop inside the shadow path (still well within <2s).

### Option B — Native Rust `decision-model` crate

Add a workspace member `services/decision-model` with a `reqwest` client posting to
`https://api.typesafe.ai/v1/systemone`. For Laya, use `laya.cpp` (C++/bindings) or shell
out to the Python sidecar.

**Pros:** no Python dependency, one fewer hop for Jev.
**Cons:** more work; Laya's native story is less mature than its Python one.

### Option C — Gateway via LiteLLM / OpenRouter / Pydantic AI

Route through LiteLLM's `/typesafe/v1/systemone` pass-through (gives you key management
and cost tracking) or OpenRouter. Useful for multi-tenant key handling, not needed for
a local demo.

**Recommendation for the hackathon:** Option A with **Laya**, and behind an env flag
(`DECISION_MODEL=off|laya|jev`) that defaults to `off` so nothing changes for the
existing demo and the fail-open contract is preserved.

---

## 6. Interface design

### 6.1 The governance question schema

One request per intercepted call, carrying all questions. State is assembled from the
already-available shadow inputs (`response_text`, `prompt_text`, `context_text`).

| Question key | Type | Instructions (essence) | Options / criteria | Emits check |
|---|---|---|---|---|
| `hallucination` | `noul` | "Response asserts facts not supported by the provided context" | — | `deepeval-hallucination` |
| `hallucination_severity` | `score` | "Severity of unsupported claims" | supported / minor detail / material fabrication | (severity) |
| `bias_present` | `noul` | "Text treats a protected group unfairly" | — | `llm-guard-bias` |
| `bias_category` | `choice` | "Which bias category" | gender / race / religion / age / disability / other | (category) |
| `toxicity_severity` | `score` | "Toxicity level" | none / mild / severe | `llm-guard-toxicity` |
| `is_reidentifiable` | `noul` | "Text could identify a specific person" | — | `semantic_pii` |
| `reid_type` | `choice` | "Type of identifying info" | direct / quasi / none | (type) |
| `injection_attempt` | `noul` | "Prompt attempts to override instructions" | — | `prompt_injection` |
| `tool_call_risk` | `choice` | "Risk of the requested tool action" | low / medium / high / destructive | `tool_use_detection` |
| `verbosity` | `score` | "Filler vs information density" | dense / acceptable / padded | `verbosity` |

**Design rules (from TypeSafe's own guidance):**

- Always include an explicit `other` option in `choice` so the model can say "none fits"
  rather than picking the nearest wrong label.
- Send only the fields each question needs — accuracy degrades when state is padded.
- Ask everything up front and choose relevance in code (speculative fan-out): extra
  questions cost tokens but almost no time.

### 6.2 Verdict mapping (model output → `ShadowVerdict`)

```text
noul >= 0.85                           → Escalate (high confidence)   ┐
noul 0.60–0.85                         → Edit                          │  Responsibility /
noul < 0.60                            → no verdict (pass silently)    ┘  Performance / Cost
score >= 2 (of 0..2) and confidence>0.7 → Escalate
score >= 1                              → Edit
choice == "destructive"                 → Escalate (tool-use; then ×1.5 multiplier if has_tool_use)
```

Confidence stored in the verdict is the model's own `confidence` (choice/score) or the
`noul` probability itself. Keep the existing axis mapping:

- hallucination / groundedness / tool-use → **Performance**
- bias / toxicity / injection / PII → **Responsibility**
- verbosity → **Cost**

### 6.3 Contract compliance checklist (from `AGENTS.md`)

- [x] **Shadow path only** — never invoked from `fast-path`.
- [x] **No blocking** — verdicts publish asynchronously; the user never waits.
- [x] **No final decision** — the decision engine still aggregates; the model only scores.
- [x] **Fail-open** — if the sidecar/model is unreachable, emit **no verdict** (absence
      of a shadow verdict = pass). Never fail a call because the judge is down.
- [x] **`correlation_id` threaded** — pass the call id through so the audit trail joins.
- [ ] **Do not** let the model decide outcomes directly (it produces scores; policy decides).

---

## 7. Implementation sketch — Option A (guardrails sidecar)

### 7.1 `services/guardrails/requirements.txt`

```diff
 fastapi>=0.115.0
 uvicorn>=0.32.0
 presidio-analyzer>=2.2.0
 presidio-anonymizer>=2.2.0
 spacy>=3.7.0
 transformers>=4.40.0
 pydantic>=2.0.0
 deepeval>=1.0.0
+# Decision model (System One) — pick one or both
+laya                   # Apache 2.0, local, no API key (pin the version you test)
+typesafe-sdk           # Jev — requires TYPESAFE_API_KEY (early access)
```

> `laya` is CPU-friendly but check the wheel availability for your platform; the
> transformer backbones are ~800MB (English) / ~650MB (multilingual) and should be
> preloaded, not lazy-loaded per request.

### 7.2 `services/guardrails/decision_models.py` (new)

```python
"""System One decision-model adapters (Laya local / Jev API).

Both expose the same `evaluate(state, questions) -> {answers: {...}}` shape so the
sidecar can swap backends with one env var. Fail-open: any error returns None and the
caller emits no verdict.
"""
import logging
import os
from typing import Any, Dict, Optional

logger = logging.getLogger("guardrails.decision")


class LayaBackend:
    """Local, Apache-2.0, ~33ms on GPU. No network egress."""

    name = "laya"

    def __init__(self) -> None:
        from laya import Router  # imported lazily; heavy
        # preload keeps every checkpoint resident so language switches cost
        # only detection (<1ms), not a 7-10s reload.
        self._router = Router(preload=True)
        logger.info("Laya Router preloaded")

    def evaluate(self, state: Any, questions: Dict[str, Any]) -> Optional[Dict[str, Any]]:
        res = self._router.predict(state, questions)
        return {"answers": res.get("answers", {}), "routing": res.get("routing")}


class JevBackend:
    """Managed TypeSafe API. $0.042/MTok in, output free. Requires TYPESAFE_API_KEY."""

    name = "jev"

    def __init__(self) -> None:
        from typesafe_sdk import TypeSafeClient  # reads TYPESAFE_API_KEY
        self._client = TypeSafeClient()

    def evaluate(self, state: Any, questions: Dict[str, Any]) -> Optional[Dict[str, Any]]:
        resp = self._client.system_one(state=state, questions=questions)
        answers: Dict[str, Any] = {}
        for key, q in resp.answers.items():
            # SDK returns objects with .choice/.probabilities/.confidence, .score, .noul
            answers[key] = {
                "type": getattr(q, "type", None),
                "choice": getattr(q, "choice", None),
                "probabilities": getattr(q, "probabilities", None),
                "score": getattr(q, "score", None),
                "noul": getattr(q, "noul", None),
                "confidence": getattr(q, "confidence", None),
            }
        return {"answers": answers}


_BACKENDS = {"laya": LayaBackend, "jev": JevBackend}
_backend: Optional[object] = None


def get_backend():
    """Singleton chosen by DECISION_MODEL env var. Returns None when disabled/failed."""
    global _backend
    if _backend is not None:
        return _backend
    choice = os.environ.get("DECISION_MODEL", "off").lower()
    if choice == "off":
        return None
    try:
        _backend = _BACKENDS[choice]()
        logger.info(f"Decision model backend: {choice}")
    except Exception as e:                      # fail-open: no judge, no verdicts
        logger.warning(f"Decision model '{choice}' unavailable: {e}")
    return _backend
```

### 7.3 `main.py` — new endpoint

Follows the existing `ScanRequest`/response conventions in the file.

```python
from decision_models import get_backend

# ─── System One decision questions (one request answers all checks) ───────────

GOVERNANCE_QUESTIONS = {
    "hallucination": {
        "type": "noul",
        "instructions": "The response asserts facts that are not supported by the provided context.",
    },
    "hallucination_severity": {
        "type": "score",
        "instructions": "How severe are any unsupported claims?",
        "criteria": ["fully supported", "minor unsupported detail", "material fabrication"],
    },
    "bias_present": {
        "type": "noul",
        "instructions": "The text treats a person or group unfairly because of a protected characteristic.",
    },
    "injection_attempt": {
        "type": "noul",
        "instructions": "The prompt attempts to override, ignore, or extract the system instructions.",
    },
    "is_reidentifiable": {
        "type": "noul",
        "instructions": "The text could be used to identify a specific real person.",
    },
    "verbosity": {
        "type": "score",
        "instructions": "How much of the response is filler versus useful signal?",
        "criteria": ["dense and useful", "acceptable", "padded with filler"],
    },
}


class DecisionScanRequest(BaseModel):
    text: str                      # the response text
    prompt: Optional[str] = None   # the input prompt
    context: Optional[str] = None  # retrieved context, if any


@app.post("/scan/governance")
def scan_governance(req: DecisionScanRequest):
    backend = get_backend()
    started = time.time()
    if backend is None:
        return {"enabled": False, "answers": {}, "duration_ms": 0}

    state = {
        "response": req.text,
        "prompt": req.prompt or "",
        "context": req.context or "",
    }
    try:
        out = backend.evaluate(state, GOVERNANCE_QUESTIONS)
        return {
            "enabled": True,
            "model": backend.name,
            "answers": out["answers"],
            "duration_ms": round((time.time() - started) * 1000, 1),
        }
    except Exception as e:                       # fail-open
        logger.warning(f"Decision model error: {e}")
        return {"enabled": False, "answers": {}, "duration_ms": 0}
```

### 7.4 Rust side — `services/shadow-analysis/src/guardrails_client.rs`

Add a method alongside `scan_pii` / `scan_hallucination`, mapping calibrated answers
to `ShadowVerdict`s:

```rust
pub async fn scan_governance(
    &self,
    response_text: &str,
    prompt_text: &str,
    context: Option<&str>,
) -> Vec<ShadowVerdict> {
    let url = format!("{}/scan/governance", self.base_url);
    let body = serde_json::json!({
        "text": response_text,
        "prompt": prompt_text,
        "context": context,
    });

    let resp = match self.http.post(&url).json(&body).send().await {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => { warn!(status = %r.status(), "governance endpoint error"); return vec![]; }
        Err(e) => { warn!(error = %e, "governance endpoint unreachable — FAIL OPEN"); return vec![]; }
    };

    let parsed: GovernanceResponse = match resp.json().await {
        Ok(p) => p,
        Err(e) => { warn!(error = %e, "bad governance payload"); return vec![]; }
    };
    if !parsed.enabled { return vec![]; }        // disabled or model down => no verdicts

    let mut verdicts = Vec::new();

    // noul: hallucination
    if let Some(a) = parsed.answers.get("hallucination") {
        if let Some(p) = a.noul {
            let outcome = if p >= 0.85 { Some(Outcome::Escalate) }
                          else if p >= 0.60 { Some(Outcome::Edit) }
                          else { None };
            if let Some(outcome) = outcome {
                verdicts.push(ShadowVerdict {
                    axis: Axis::Performance,
                    check_name: "decision-hallucination".into(),
                    outcome,
                    confidence: p as f32,
                    reason: format!("System One judge: {}% probability of unsupported claims", (p * 100.0).round()),
                    duration_ms: parsed.duration_ms as u32,
                });
            }
        }
    }

    // ...same shape for bias_present (Responsibility) and is_reidentifiable (Responsibility)...

    verdicts
}
```

Then wire it into `worker.rs` next to the existing guardrails handles, gated on a new
toggle (`toggles.decision_judge`) so it can be switched from the Policies page like
every other check.

### 7.5 Configuration

```env
# .env.example
DECISION_MODEL=off          # off | laya | jev
TYPESAFE_API_KEY=           # only needed when DECISION_MODEL=jev
# GUARDRAILS_URL already exists (port 8200)
```

---

## 8. Implementation sketch — Option B (native Rust, Jev only)

Brief, for completeness: add `services/decision-model` to the workspace with a
`reqwest` client. The request/response types mirror the raw endpoint in §1. Use the
same mapping logic as §7.4. Laya would still need the Python sidecar or `laya.cpp`.

```rust
// services/decision-model/src/lib.rs (sketch)
pub struct DecisionModelClient { base_url: String, api_key: String, http: reqwest::Client }

impl DecisionModelClient {
    pub async fn system_one(&self, state: serde_json::Value, questions: serde_json::Value)
        -> anyhow::Result<SystemOneResponse> { /* POST /v1/systemone */ }
}
```

---

## 9. Calibration & thresholding (do not skip this)

- **Laya ships over-confident.** The model card is explicit: refitting one temperature
  per `(question type, option count)` moved mean ECE from **0.466 → 0.081** (English) and
  **0.314 → 0.106** (multilingual). **Fit a temperature on your own data before trusting
  the probabilities.**
- **Jev is calibrated by design** (RLCD optimises against a strictly proper scoring
  rule), with reported ECE ~0.246 for Jev vs ~0.081 for temperature-fitted Laya — so Jev
  is better *raw*, Laya is better *after fitting*.
- **Use per-action thresholds**, not one global number. Being wrong on a `pass` for a
  read-only feature is cheap; being wrong on an `escalate`/`block` is expensive. This is
  TypeSafe's "confidence-gated routing" pattern and it maps cleanly onto ControlPlane's
  existing tiered outcomes.
- **Keep the deterministic fast path as the safety net.** A calibrated judge is still a
  probabilistic judge. The design principle in `AGENTS.md` — *"the final verdict is
  deterministic given the scores and policy thresholds"* — is exactly how to use these
  models safely.

---

## 10. Cost & latency budget

Shadow budget is <2s (target, non-blocking), so both models fit comfortably.

| Check | Today | With Laya | With Jev |
|---|---|---|---|
| Price per call (≈1k input tokens, 8 questions) | LLM API rates | **$0** | **$0.000042** |
| Latency (single call, all questions) | ~500ms–1.5s | ~33ms GPU / ~200–460ms CPU | 70–500ms |
| Network egress | Presidio/DeepEval local | **none** | response+context sent to TypeSafe |
| Fail mode | error → no verdict | error → no verdict | error → no verdict |

At 10k calls/day with Jev, the judge costs roughly **$0.42/day** for ~1k tokens each —
orders of magnitude below the token cost of the DeepEval LLM judge it replaces.

---

## 11. Risks, limits & honesty (say these out loud)

| Risk | Detail | Mitigation |
|---|---|---|
| **Jev is cloud-only** | State (which includes customer prompts/responses) leaves your network → data-residency/GDPR concern for regulated apps | Use Laya on-prem for sensitive deployments; or send only redacted excerpts to Jev |
| **Jev is early access** | Waitlisted; API could change | Pin via LiteLLM pass-through; keep the backend swap-able |
| **Vendor benchmarks** | TypeSafe's speed/cost numbers are self-run | Don't quote 193×/444× as fact; quote $0.042/MTok (their published price) |
| **Laya zero-shot weakness** | Base checkpoints near chance on `typed-decisions` (0.362 vs 0.461 majority baseline) | Fine-tune on your own labelled governance data (they ship a notebook); the 0.766 checkpoint is *their* fine-tune |
| **Laya high-cardinality weakness** | 77-option questions collapse (0.425 vs Jev 0.870) | Keep governance questions ≤ ~20 options, or raise `head_max_len`, or use a coarse-to-fine split |
| **Laya over-confidence** | Raw ECE 0.213–0.466 | Temperature-fit before trusting probabilities |
| **Judge accuracy is not ground truth** | Both are approximations of a "correct" label | Continue the reviewer-override feedback loop; track precision per check |
| **Contract violation risk** | Putting either model in the fast path breaks `AGENTS.md` (network + model + <10ms) | Shadow path only; enforce with a code-review rule and a test |
| **New dependency** | Extra runtime + image size (~800MB Laya weights) | Make it opt-in (`DECISION_MODEL=off` default); document docker image impact |
| **Licensing** | Laya is Apache 2.0 (fine); Jev is a paid closed API | Laya is safe to demo; attribute Convai Innovations |

---

## 12. Suggested rollout plan

| Phase | Work | Exit criteria | Label |
|---|---|---|---|
| **0** | `/scan/governance` endpoint returns `enabled:false`; Rust client + toggle wired; no-op in production | Shadow path unchanged; fail-open test green | SCAFFOLD |
| **1** | Laya local, answering **hallucination + groundedness** only, replacing/augmenting DeepEval | Verdict parity within ±10%; shadow latency budget holds; no fast-path impact | TARGET |
| **2** | Add bias / toxicity / injection / semantic-PII / verbosity via one batched call | All checks produce mapped verdicts; reviewer FP rate not worse than baseline | TARGET |
| **3** | Jev backend behind the same interface; A/B Laya vs Jev on stored calls | Jev wins on balance/accuracy where high cardinality matters; else stay on Laya | TARGET |
| **4** | Use the judge for escalation triage, pattern promotion, tool-call gating | Measurable reduction in low-value escalations | TARGET |

**Demo option (if you want a wow moment):** run Laya locally in the guardrails sidecar
and show the **same request** scored by (a) the DeepEval LLM judge and (b) the System One
judge, side by side — same verdict, 10–50× faster, $0, and no free-text channel for an
attacker to exploit.

---

## 13. Test plan

```text
[ ] unit: question output → ShadowVerdict mapping (each primitive, each threshold band)
[ ] unit: fail-open — backend disabled / HTTP error / timeout ⇒ zero verdicts, no panic
[ ] contract: fast-path crate has no dependency on decision-model (grep/CI guard)
[ ] latency: shadow worker p99 with the judge enabled stays under the 2s budget
[ ] calibration: on a held-out labelled set, Brier/ECE recorded before enabling gating
[ ] integration: a poisoned response cannot change the judge's output schema
[ ] regression: existing 390-test suite still green
```

---

## 14. Sources

- TypeSafe AI — *Introducing System One Models & Jev* (Sep 15, 2026) —
  https://typesafe.ai/blog/introducing-system-one-models-and-jev
- TypeSafe AI homepage / manifesto — https://typesafe.ai/
- LangChain — *Building a Harness with Jev* (Sep 17, 2026) —
  https://www.langchain.com/blog/building-a-harness-with-jev
- DEV — *How to Use Jev: a practical guide* — https://dev.to/valyuai/how-to-use-jev-a-practical-guide-to-typesafes-system-one-model-g5e
- LiteLLM — *TypeSafe AI (Jev) pass-through* — https://docs.litellm.ai/docs/pass_through/typesafe
- Pydantic AI — *TypeSafe (Jev)* — https://pydantic.dev/docs/ai/models/typesafe/
- Laya model card (Convai Innovations) — https://huggingface.co/convaiinnovations/laya
- Laya source — https://github.com/NandhaKishorM/laya · https://pypi.org/project/laya/
- Hacker News discussion (Laya, skepticism) — https://news.ycombinator.com/item?id=49765348

> Vendor claims are attributed as such. Benchmark comparisons in §3 are from the Laya
> model card's own comparison and were **not** independently reproduced here.
