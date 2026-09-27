# Architecture Diagrams for the Deck

> **How to use this file:** one **simple** diagram per presentation slide — nothing here
> needs more than ~10 boxes, because you have to explain every box out loud. Render the
> one you need at **3× and export SVG**, drop it into that slide, and read the
> *"How to explain it"* line underneath it.
>
> **Slide order matches `docs/original/presentation-script.md`** (8 slides, 4:30).
>
> - `§1` → **Slide 3** — the main architecture slide (the one diagram you must nail)
> - `§2` → **Slide 3 (variant)** — the same picture **with Laya**, for the "what's next" beat
> - `§3` → **Slide 4** — the demo, as a simple request flow
> - `§4` → **Slide 5** — the feedback loop
> - `§5` → **Slide 6** — governance, policies and audit
> - `§6` → **Backup slide** — the full detailed architecture (appendix only; do **not**
>   put this on a slide you have to narrate)
> - `§7` → rendering tips + a consistency checklist so the deck never contradicts itself

---
## 0. Slide 2 — the problem, in three boxes

The whole point of this slide is the **empty space in the middle**. Draw it red and leave
it bare — the absence is the product opportunity.

```mermaid
flowchart LR
  A["Application"] -->|"every call"| M["AI Model"]
  M -->|"raw response"| U["Customer"]
  M -.->|"nothing in between"| X["No real-time guardrails<br/>No audit trail<br/>No feedback loop<br/>No cost visibility<br/>No conversation governance"]

  classDef base fill:#e5e7eb,stroke:#6b7280,stroke-width:2px,color:#111827
  classDef gap fill:#fee2e2,stroke:#dc2626,stroke-width:2.5px,color:#7f1d1d
  class A,M,U base
  class X gap
```

**How to explain it (15 s):**

> "This is the entire pipeline today: the app calls the model, the model answers, the
> customer gets it. There is **nothing between the model and the user**. No guardrails in
> real time, no audit trail, no cost visibility, no memory of the conversation — and no way
> to learn from a mistake. That empty space is where ControlPlane lives."

---

## 1. Slide 3 — what we built (THE diagram)

Seven boxes. Four connections that matter. This is the slide you stand on for 45 seconds.

```mermaid
flowchart LR
  APP["Client App"] -->|"1 · request"| PX["<b>ControlPlane Proxy</b> :8900<br/>intercepts every call<br/>correlation_id · policy routing"]
  PX -->|"2 · forward unmodified"| MODEL["AI Model<br/>Ollama · Anthropic · Gemini"]
  PX -->|"6 · governed response"| APP

  PX -->|"3 · sync"| FAST["<b>FAST PATH</b> — synchronous<br/>deterministic Rust · under 10 ms<br/><br/>secrets / PII · cost cap<br/>retry loops · session risk<br/>unsafe content"]
  PX -.->|"4 · async"| SHADOW["<b>SHADOW PATH</b> — asynchronous<br/>after delivery · under 2 s<br/><br/>prompt injection · hallucination<br/>bias · toxicity · groundedness<br/>semantic PII"]

  FAST --> DEC["<b>Decision Engine</b><br/>worst outcome wins<br/>two axes firing = escalate"]
  SHADOW -.-> DEC
  DEC --> OUT["PASS · EDIT · ESCALATE · BLOCK"]

  classDef app fill:#eef2ff,stroke:#6366f1,stroke-width:2px,color:#1e1b4b
  classDef edge fill:#e0f2fe,stroke:#0284c7,stroke-width:2px,color:#082f49
  classDef fast fill:#dcfce7,stroke:#16a34a,stroke-width:2.5px,color:#052e16
  classDef shadow fill:#fae8ff,stroke:#a855f7,stroke-width:2.5px,color:#3b0764
  classDef decision fill:#fef3c7,stroke:#d97706,stroke-width:2px,color:#451a03
  classDef out fill:#ccfbf1,stroke:#0d9488,stroke-width:2px,color:#042f2e
  class APP,MODEL app
  class PX edge
  class FAST fast
  class SHADOW shadow
  class DEC decision
  class OUT out
```

**How to explain it (45 s):**

> "ControlPlane is a reverse proxy. The app points at us instead of the model. We forward
> the request **unmodified** — we are not a model and we do not edit requests.
>
> Then we inspect the response on **two paths, not one**. The **fast path** runs
> synchronously, before the response reaches the user — deterministic Rust, under ten
> milliseconds. The **shadow path** runs asynchronously, after delivery — the expensive
> checks, so the user never waits for them.
>
> Both feed the decision engine, which returns one of four outcomes: pass, edit, escalate,
> or block. And if ControlPlane itself errors, we **fail open** — traffic passes through
> untouched."

**Colour key for the audience:** green = synchronous, before the user sees anything.
Purple = asynchronous, after delivery. Amber = the decision. Teal = the outcome.

---

## 2. Slide 3 variant — the same picture **with Laya** (`TARGET`)

Only **three things change**, and they are drawn dashed. Use this as the "what's next"
beat, or in the appendix. Never present Laya as shipped — say it is planned.

```mermaid
flowchart LR
  APP["Client App"] -->|"request"| PX["<b>ControlPlane Proxy</b> :8900"]
  PX -->|"forward unmodified"| MODEL["AI Model"]
  PX -->|"governed response"| APP

  PX --> FAST["<b>FAST PATH</b> — synchronous · under 10 ms<br/>stays deterministic — <b>no model here</b>"]
  PX -.-> SHADOW["<b>SHADOW PATH</b> — asynchronous · under 2 s"]
  SHADOW --> GR["Guardrails :8200<br/>Presidio PII · toxic-roberta<br/>bias · DeepEval"]
  SHADOW -.-> LJ["<b>Laya judge</b> :8300 — new<br/>12 typed questions in ONE pass<br/>calibrated probabilities<br/>multilingual routing<br/>Promise: one extra independent voter"]

  DEC["<b>Decision Engine</b><br/>fusion — weighted noisy-OR<br/>+ corroboration rule<br/>then worst-outcome + compound risk"]
  OUT["PASS · EDIT · ESCALATE · BLOCK"]

  FAST --> DEC
  GR -.-> DEC
  LJ -.-> DEC
  DEC --> OUT

  classDef edge fill:#e0f2fe,stroke:#0284c7,stroke-width:2px,color:#082f49
  classDef fast fill:#dcfce7,stroke:#16a34a,stroke-width:2.5px,color:#052e16
  classDef shadow fill:#fae8ff,stroke:#a855f7,stroke-width:2.5px,color:#3b0764
  classDef decision fill:#fef3c7,stroke:#d97706,stroke-width:2px,color:#451a03
  classDef out fill:#ccfbf1,stroke:#0d9488,stroke-width:2px,color:#042f2e
  classDef target fill:#fffbe6,stroke:#eab308,stroke-width:2.5px,stroke-dasharray:6 4,color:#422006
  classDef base fill:#eef2ff,stroke:#6366f1,stroke-width:2px,color:#1e1b4b
  class APP,MODEL base
  class PX edge
  class FAST fast
  class SHADOW,GR shadow
  class DEC decision
  class OUT out
  class LJ target
```

**How to explain it (20 s):**

> "Three things are new, and I've drawn them dashed because they are **planned, not
> shipped**. Laya is a small non-autoregressive decision model — you give it typed
> questions and it answers with **calibrated probabilities** in one forward pass. It joins
> the shadow panel as **one more independent voter** for the fuzzy checks: re-identification,
> hallucination, bias, injection.
>
> Two things to notice. The **fast path does not change** — we will never put a 33 ms model
> in a 10 ms budget. And Laya never makes the decision — it produces scores; our thresholds
> still decide. That is what keeps it a governance layer and not a black box."

**Why it makes us better (if asked):** it is an *ensemble* — each check keeps the engine
that is best for it (Presidio for PII spans, toxic-roberta for toxicity, regex for known
injection patterns) and Laya adds calibrated judgment and multilingual coverage. We measure
the gain by ablation on our own reviewer-labelled data — see
`docs/analysis/laya-integration-plan.md`.

---

## 3. Slide 4 — the demo, as a simple request flow

The point of this slide is **the horizontal line**: everything above it the user waits for;
everything below it they do not. Put the three demo scenarios (`PASS`, `EDIT`, `ESCALATE`)
as three chips under the final box.

```mermaid
flowchart LR
  A["App"] --> B["Proxy<br/>:8900"]
  B --> C["AI Model"]
  C --> D["Fast Path<br/>sync · 5.6 µs measured<br/>under 10 ms budget"]
  D --> E["Response delivered<br/>to the user"]
  E --> F["Shadow Path<br/>async · under 2 s"]
  F --> G["Decision<br/>+ audit + escalation"]

  classDef up fill:#dcfce7,stroke:#16a34a,stroke-width:2.5px,color:#052e16
  classDef down fill:#fae8ff,stroke:#a855f7,stroke-width:2.5px,color:#3b0764
  classDef base fill:#e0f2fe,stroke:#0284c7,stroke-width:2px,color:#082f49
  class A,B,C,E base
  class D up
  class F,G down
```

**How to explain it (20 s):**

> "Watch where the user's clock stops. The fast path is **synchronous** — secrets, PII,
> cost caps, unsafe content, all in microseconds, before the response is delivered. The
> moment the response reaches the user, their wait is over. Everything after this line —
> injection, hallucination, bias, re-identification — runs **after** delivery. Expensive
> checks, zero latency cost."

**Then run the three scenarios** (from the demo script), and for each one point at the
outcome chip:

| Demo | What fires | Where | Outcome |
|---|---|---|---|
| "What is 2+2?" | nothing | — | **PASS** |
| Response contains an AWS key | regex + entropy | Fast path | **EDIT** (key redacted) |
| "Ignore all previous instructions..." | 3-layer injection | Shadow path | **ESCALATE** |

---

## 4. Slide 5 — the feedback loop (the differentiator)

One loop. Four boxes. This is the slide to slow down on.

```mermaid
flowchart LR
  A["A case is<br/>escalated"] --> B["Reviewer decides<br/>confirm · override · dismiss<br/>+ writes a reason"]
  B --> C["Precedent stored<br/>question + answer<br/>+ axis + reason"]
  C --> D{"A new call<br/>arrives"}
  D -->|"not similar"| A
  D -->|"60%+ similar to a dismissed case"| E["Auto-suppressed<br/>no case created<br/>verdict annotated"]
  E --> F["False positives fall<br/>trust score rises"]
  F -.-> D

  classDef base fill:#e0f2fe,stroke:#0284c7,stroke-width:2px,color:#082f49
  classDef good fill:#dcfce7,stroke:#16a34a,stroke-width:2.5px,color:#052e16
  classDef win fill:#fef3c7,stroke:#d97706,stroke-width:2.5px,color:#451a03
  class A,B,C,D base
  class E good
  class F win
```

**How to explain it (40 s):**

> "Every other governance tool does the same thing: detect, flag, move on. We do something
> none of them do — **we learn from the reviewer**.
>
> When a reviewer dismisses a false positive, we store the whole context: the question, the
> answer, the axis, and their reason. Then, at **two layers**, a future call is checked
> against that precedent — we don't create the escalation case at all, and we downgrade the
> verdict. The verdict is annotated so the audit trail shows *why*: 'learned — a 82%-similar
> past case was dismissed'.
>
> No retraining. And it is measurable: false-positive rate trends down, trust score trends
> up. The system literally gets better every time a human makes a decision."

---

## 5. Slide 6 — governance, policies and audit

Two ideas only: **per-app policies** on the left, **tamper-evident audit** on the right.

```mermaid
flowchart LR
  subgraph APPS["Policy is per application"]
    direction TB
    A1["ChatBot-Prod<br/>STRICT"]
    A2["Agent-Internal<br/>MODERATE"]
    A3["RAG Support<br/>groundedness-focused"]
  end

  subgraph PROF["Regulatory profiles — one action"]
    direction TB
    P1["US Financial"]
    P2["EU Financial"]
    P3["US Healthcare"]
    P4["India · EU · Global"]
  end

  APPS --> POL["Policy engine<br/>toggles · cost caps · thresholds<br/>versioned · hot-reloaded"]
  PROF --> POL
  POL --> FAST["Fast path<br/>lock-free cache"]
  POL -.-> SHADOW["Shadow path<br/>check toggles"]

  POL --> DEC["Decision"]
  DEC --> AUD["Audit ledger<br/>SHA-256 hash chain<br/>append-only · tamper-evident"]
  AUD --> VER["One API call<br/>to verify the chain"]

  classDef base fill:#e0f2fe,stroke:#0284c7,stroke-width:2px,color:#082f49
  classDef pol fill:#fef3c7,stroke:#d97706,stroke-width:2px,color:#451a03
  classDef audit fill:#ccfbf1,stroke:#0d9488,stroke-width:2.5px,color:#042f2e
  classDef fast fill:#dcfce7,stroke:#16a34a,stroke-width:2px,color:#052e16
  classDef shadow fill:#fae8ff,stroke:#a855f7,stroke-width:2px,color:#3b0764
  class A1,A2,A3,P1,P2,P3,P4 base
  class POL,DEC pol
  class FAST fast
  class SHADOW shadow
  class AUD,VER audit
```

**How to explain it (25 s):**

> "Governance is **per application**. A customer-facing chatbot is strict; an internal
> copilot is moderate; a RAG tool is groundedness-focused. Change a toggle and it
> hot-reloads into both paths immediately.
>
> And six regulatory profiles — US Financial, EU, Healthcare, India, EU General, Global —
> so switching compliance posture takes **seconds, not sprints**.
>
> Finally, every single decision is written to a SHA-256 hash chain. Append-only: no record
> can be edited or deleted. One API call verifies the whole chain. That is what regulators
> ask for."

---

## 6. Backup slide — full detailed architecture (appendix only)

Keep this in the deck's **appendix**, for Q&A ("can we see the whole thing?"). Do not
narrate it — it has too many boxes to talk through in 4:30. It exists so a technical judge
can see the real service topology.

```mermaid
flowchart TB

  subgraph CLIENTS["Client Applications"]
    direction LR
    C1["ChatBot-Prod"]
    C2["Agent-Internal"]
    C3["RAG-Customer-Support"]
  end

  subgraph GW["controlplane-gateway — one Rust process"]
    direction TB
    PX["<b>Proxy Listener</b> :8900<br/>intercept · correlation_id<br/>app / session / profile routing"]
    subgraph FASTPATH["Fast Path - synchronous · deterministic · under 10 ms"]
      direction LR
      FP1["Unsafe content"]
      FP2["Secret detection"]
      FP3["Cost cap"]
      FP4["Retry / loop"]
      FP5["Session risk"]
      FP6["Tool-use"]
    end
    BFF["<b>Dashboard API (BFF)</b> :8080<br/>REST + SSE"]
  end

  OLL["Ollama · qwen2.5:1.5b :11434"]
  CLD["Anthropic · Gemini · OpenCode"]
  BUS["Event Bus<br/>in-process (demo) OR NATS<br/>same subjects and contracts"]

  subgraph SHADOWPATH["Shadow Path - asynchronous · under 2 s"]
    direction LR
    SHW["Shadow Worker"]
    GR["Guardrails sidecar :8200<br/>Presidio · toxic-roberta · bias · DeepEval"]
  end

  DEC["Decision Engine<br/>worst-outcome wins<br/>compound-risk escalation<br/>reviewer-feedback suppression"]

  subgraph DOWN["Downstream Services"]
    direction LR
    AUD["Audit<br/>SHA-256 chain"]
    ESC["Escalation Queue"]
    COST["Cost Accounting"]
    NOTIF["Notification"]
  end

  PG[("PostgreSQL 16<br/>system of record<br/>+ pg_trgm precedents")]
  FE["Next.js Dashboard :3000<br/>live stream · requests · policies<br/>escalations · cost · audit"]

  CLIENTS -->|"OpenAI-compatible request"| PX
  PX -->|"forward unmodified"| OLL
  PX -.->|"or"| CLD
  PX --> FASTPATH
  PX -->|"response"| CLIENTS
  PX -->|"shadow job + verdict.fast"| BUS
  BUS --> SHW
  SHW --> GR
  SHW -->|"verdict.shadow"| BUS
  BUS --> DEC
  DEC -->|"decision.final"| BUS
  DEC -->|"policy.updated"| FASTPATH
  DEC -.->|"policy.updated"| SHW
  BUS -->|"decision.final"| AUD
  BUS -->|"decision.final"| ESC
  BUS -->|"intercept.captured"| COST
  BUS -->|"decision.final"| NOTIF
  BUS -->|"live verdicts"| BFF
  ESC -->|"escalation.created"| BUS
  PX --> PG
  DEC --> PG
  AUD --> PG
  ESC --> PG
  COST --> PG
  BFF --> PG
  BFF -->|"REST + SSE"| FE

  classDef clients fill:#eef2ff,stroke:#6366f1,stroke-width:2px,color:#1e1b4b
  classDef edge fill:#e0f2fe,stroke:#0284c7,stroke-width:2px,color:#082f49
  classDef fast fill:#dcfce7,stroke:#16a34a,stroke-width:2px,color:#052e16
  classDef shadow fill:#fae8ff,stroke:#a855f7,stroke-width:2px,color:#3b0764
  classDef decision fill:#fef3c7,stroke:#d97706,stroke-width:2px,color:#451a03
  classDef svc fill:#ccfbf1,stroke:#0d9488,stroke-width:2px,color:#042f2e
  classDef data fill:#e2e8f0,stroke:#475569,stroke-width:2px,color:#0f172a
  classDef ui fill:#ffe4e6,stroke:#e11d48,stroke-width:2px,color:#4c0519
  classDef ext fill:#f8fafc,stroke:#94a3b8,stroke-width:2px,color:#0f172a
  class C1,C2,C3 clients
  class PX,BFF edge
  class FP1,FP2,FP3,FP4,FP5,FP6 fast
  class OLL,CLD,GR ext
  class BUS,SHW shadow
  class DEC decision
  class AUD,ESC,COST,NOTIF svc
  class PG data
  class FE ui
```

**One-line answer if a judge points at it:** *"Every box is a crate in one Rust workspace,
communicating over the event bus — the same subjects and contracts whether the bus is
in-process for the demo or NATS in production. The proxy never blocks on any of them."*

---

## 7. Rendering tips and consistency checklist

### 7.1 Rendering

1. Open **mermaid.live**, paste one diagram.
2. Use the **Actions → Download → SVG** (not PNG) — vector stays sharp on any projector.
3. In PowerPoint/Google Slides insert the SVG as a picture and stretch it.
4. Keep the diagram's colours and the slide's accent colour consistent.
5. If a renderer struggles, delete the `classDef`/`class` lines — the diagram still works,
   it just loses colour.

### 7.2 Consistency checklist (so the deck never contradicts itself)

| Fact | Say this | Not this |
|---|---|---|
| Crates | 12 Rust crates + 1 Python guardrails sidecar | "13 microservices" |
| Checks | 14 product checks — 6 fast-path + 8 shadow | "13" (the old count missed tool-use) |
| Toggles | 10 toggles on the Policies page | "all 14 are toggles" |
| Latency | "measured 5.6 µs, budget 10 ms" | "1,000× faster" as the headline |
| Injection | **shadow** path, asynchronous | never say fast-path |
| Suppression | **60%** similarity, both layers | "40%" |
| Bus | in-process is the demo default; NATS is supported | "we run NATS" |
| Auth | demo mode allows anonymous access by design | claiming it is production-hardened |
| Laya | **`TARGET` — dashed, planned** | presenting it as shipped |
| Ports | proxy 8900 · API 8080 · UI 3000 · Ollama 11434 · PG 5432 · guardrails 8200 · Laya 8300 (target) | mixing them up |
| Tests | 276 Rust + 114 frontend | "390 Rust tests" |

### 7.3 If a slide feels crowded while you rehearse

Cut in this order: (1) drop the colour legend onto the slide notes instead of the slide,
(2) delete `classDef` lines, (3) merge the 6 fast-path checks into one box that lists them
as text, (4) move the diagram to two slides and animate the second half in. The
**§1 diagram is already the minimum** — do not add boxes to it.

---

## 8. Sources

- Slide order and speaker notes: `docs/original/presentation-script.md`
- Check inventory and honest status: `docs/analysis/checks-inventory.md`
- Laya plan (the `TARGET` components in `§2`): `docs/analysis/laya-integration-plan.md`
- Topology, contracts and ports: `AGENTS.md`, `services/gateway/src/main.rs`, `README.md`
