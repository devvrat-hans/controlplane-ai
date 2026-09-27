# Architecture

> Two diagrams, never merged: what the **target** production architecture is,
> and what the **current prototype** actually runs. Every component is labelled
> `IMPLEMENTED`, `SCAFFOLD`, or `TARGET`.

## Current prototype (what actually runs)

A **modular monolith with an event-driven pipeline**: one gateway process owns
the proxy, API, and (when `EVENT_BUS=inproc`) runs the shadow-path worker
in-process. All service boundaries exist as crates in a single Rust workspace —
the proxy is a crate, the fast-path is a crate, the shadow analyzer is a crate —
but the events, envelopes, and contracts are the same ones that would flow over
NATS in production.

```text
 Next.js frontend (http://localhost:3000)
        │  /api/v1/* (REST + SSE, bearer JWT)
        ▼
 Rust gateway (axum + hyper) ─── :8080 (dashboard API) + :8900 (proxy)
   │         │
   │         └─ Proxy listener
   │              │
   │              ├─ Captures request + response
   │              ├─ Generates correlation_id
   │              ├─ Resolves app_id (body → header → default)
   │              ├─ Resolves session_id (body → header → derived)
   │              ├─ Runs fast-path checks (in-process, <10ms)
   │              │    ├─ Secret/PII regex + entropy           [IMPLEMENTED]
   │              │    ├─ Cost cap enforcement (per-app)       [IMPLEMENTED]
   │              │    ├─ Retry/loop detection (session-aware) [IMPLEMENTED]
   │              │    ├─ Unsafe content keywords              [IMPLEMENTED]
   │              │    ├─ Tool-use risk detection              [IMPLEMENTED]
   │              │    └─ Session risk accumulation            [IMPLEMENTED]
   │              │
   │              └─ Publishes to shadow path (in-process or NATS)
   │
   ├─ Shadow-path worker (embedded or separate process)
   │    ├─ Prompt injection (3-layer detection)         [IMPLEMENTED]
   │    ├─ Groundedness scoring (NLI model)             [IMPLEMENTED]
   │    ├─ Hallucination detection (DeepEval)           [IMPLEMENTED]
   │    ├─ Bias classifier (native + guardrails)        [IMPLEMENTED]
   │    ├─ Verbosity heuristic                          [IMPLEMENTED]
   │    ├─ Semantic PII (NER)                           [IMPLEMENTED]
   │    └─ Guardrails sidecar (Presidio, LLM Guard)     [IMPLEMENTED]
   │
   ├─ Decision engine                                   [IMPLEMENTED]
   │    ├─ Verdict aggregation (fast + shadow)
   │    ├─ Compound/intersection risk escalation
   │    ├─ Feedback loop suppression (RAG precedents)
   │    └─ Policy threshold enforcement
   │
   ├─ Audit service (hash-chained, append-only)         [IMPLEMENTED]
   ├─ Cost accounting (per-app token tracking)          [IMPLEMENTED]
   ├─ Escalation queue (priority-scored, deduped)       [IMPLEMENTED]
   └─ Notification (Slack/webhook)                      [IMPLEMENTED]

 Persistence: PostgreSQL via sqlx                       [IMPLEMENTED]
 Messaging: in-process broker or NATS (same envelopes) [IMPLEMENTED]
 Policy cache: arc-swap (lock-free hot-reload)          [IMPLEMENTED]
 Feedback store: reviewer_overrides + pg_trgm           [IMPLEMENTED]
```

### Runtime topology switch (`.env`)

| Concern | Local demo (default) | Infrastructure mode |
|---|---|---|
| Persistence | `postgres` (DATABASE_URL) | `postgres` (DATABASE_URL) |
| Event bus | `inproc` | `nats` (NATS_URL) |
| Shadow worker | embedded in gateway | separate process (planned) |
| Upstream LLM | Ollama (qwen2.5:1.5b) | Cloud API (OpenAI, Anthropic) |

Both topologies run the **same** pipeline code; only the adapters differ.

## Request routing

Every inbound request resolves two identifiers before processing:

| Field | Resolution order | Purpose |
|---|---|---|
| `app_id` | Request body → `X-App-Id` header → default app | Selects which app's policies apply |
| `session_id` | Request body → `X-Session-Id` header → derived from message hash | Links multi-turn conversations |

This allows a single proxy instance to enforce different governance policies for
multiple downstream applications simultaneously.

## Multi-turn session tracking

Sessions enable conversation-level governance:

1. **Risk accumulation**: Each turn in a session contributes to a cumulative risk
   score. If multiple borderline verdicts accumulate in the same session, the
   system escalates even though no single turn crossed a threshold individually.

2. **Retry detection**: The system detects repeated identical messages within a
   session (retry loops), which can indicate adversarial probing or stuck agents.

3. **Escalation context**: When a session is escalated, the reviewer sees the full
   conversation thread — not just the triggering turn — enabling informed decisions.

## Feedback loop (reviewer-override RAG)

The system learns from human reviewers:

```text
Escalation created → Reviewer resolves (confirm / dismiss / override)
    │
    ▼
Resolution stored in reviewer_overrides table
    │
    ▼
Future similar requests checked via pg_trgm similarity
    │
    ├── >=60% similar request previously dismissed → suppress escalation
    └── No match → proceed normally
```

This operates at two levels:
- **Decision service**: Downgrades escalate/edit verdicts to pass if a similar
  case was previously dismissed (>=60% similarity on response content)
- **Escalation service**: Skips case creation if the request text matches a
  previously dismissed precedent (>=60% trigram similarity)

## Regulatory profiles

Named configurations that combine geography + industry + risk appetite:

| Profile | Geography | Industry | Risk appetite | Groundedness min | Bias threshold | Max tokens |
|---|---|---|---|---|---|---|
| `us-financial` | US | Financial Services | conservative | 0.80 | 0.45 | 3000 |
| `eu-financial` | EU | Financial Services | conservative | 0.80 | 0.50 | 2000 |
| `us-healthcare` | US | Healthcare | conservative | 0.85 | 0.40 | 3000 |
| `india-general` | India | General Enterprise | moderate | 0.60 | 0.65 | 4000 |
| `eu-general` | EU | General Enterprise | moderate | 0.60 | 0.60 | 4000 |
| `global-internal` | Global | Internal | permissive | 0.40 | 0.75 | 8000 |

Profiles are independently editable and can be applied to any app with a single
API call. Applying a profile copies its thresholds to the app's runtime policies.

## Target architecture (production)

```text
 Next.js UI (Vercel/CDN)
    │
    ▼
 API Gateway / BFF (Rust, axum)                    [SCAFFOLD → today's gateway]
    │
    ▼
 Ingress Proxy (Rust, Pingora)                     [TARGET — hyper today]
    │
    ├── Fast-path policy engine (in-process)
    │     ├── PII/Secret detection
    │     ├── Cost cap enforcement
    │     ├── Tool-use risk detection
    │     ├── Session risk accumulation
    │     └── Retry/loop detection
    │
    └── NATS ──→ Shadow Analysis Workers (scalable)
                    ├── Prompt injection (3-layer)
                    ├── Groundedness (judge model)
                    ├── Bias (ONNX, dedicated GPU)
                    ├── Hallucination detection
                    ├── Semantic PII
                    └── Verbosity scoring
    │
    ▼
 Decision Service ──→ Policy Store (PostgreSQL)
    │                  + Feedback Store (reviewer_overrides)
    │
    ├── Audit Service (PostgreSQL, append-only, hash-chained)
    ├── Cost Service (PostgreSQL + ClickHouse for analytics)
    ├── Escalation Service (priority queue, deduplication)
    └── Notification Service (Slack, webhooks, email)

 Infrastructure:
    PostgreSQL (system of record: policies, audit, verdicts, feedback)
    ClickHouse (analytics: time-series verdict/cost data)
    NATS (event bus)
    Prometheus + Grafana (observability)
    OIDC IdP (enterprise auth)
```

Nothing in the target diagram that is not implemented is claimed to be running.

## Data flow

```text
Client request
    │
    ├── app_id + session_id resolved
    │
    ▼
Proxy captures (correlation_id assigned)
    │
    ├── Fast path (sync, in-process, <10ms)
    │     ├── Verdict: pass / edit / block
    │     ├── Session risk checked (accumulation)
    │     └── Tool-use detection (risk multiplier 1.5x)
    │
    └── Shadow path (async, NATS/in-process)
          ├── Prompt injection detection
          ├── Groundedness + Hallucination scoring
          ├── Bias classification
          └── Verdict: pass / escalate
    │
    ▼
Decision engine aggregates all verdicts
    │
    ├── Feedback loop check (suppress if similar case dismissed)
    ├── Compound risk check (multiple axes → intersection escalation)
    │
    ├── Final outcome → Audit record (hash-chained)
    ├── If escalate → Escalation case created (priority-scored)
    ├── If block/edit → Notification sent
    └── All → Dashboard SSE stream + cost ledger update
```

## Key design decisions

| Decision | Rationale |
|---|---|
| Fast-path in-process (no network hop) | The 10ms latency budget is non-negotiable; any network call adds 1-5ms minimum |
| arc-swap for policy cache | Lock-free reads on every request; writers (policy reload) don't block the hot path |
| Shadow path via NATS | Fan-out to multiple analyzers, backpressure, replay on failure, decoupled scaling |
| PostgreSQL for audit (not append-only log service) | Hash chain provides tamper evidence; PostgreSQL provides queryability. Both are needed. |
| Fail-open on infrastructure failure | A governance layer that causes outages is worse than ungoverned traffic |
| Single Rust workspace | Same binary, one deployment unit, crate boundaries enforce contracts without deployment complexity |
| pg_trgm for feedback matching | Fuzzy similarity without an external vector DB; good enough for text-based RAG at this scale |
| Per-app policy isolation | Different apps have different risk profiles; a chatbot and an internal agent shouldn't share thresholds |
| Session-level risk accumulation | Individual turns may be borderline; the conversation as a whole may be adversarial |
| Verdict-level deduplication (not time-window) | Each unique verdict creates exactly one escalation case; avoids both suppression and spam |

## Latency budget

| Stage | Target | Measured |
|---|---|---|
| Proxy overhead (forwarding only) | < 3ms | ~1ms |
| Fast-path policy checks | < 10ms p50 / < 25ms p99 | ~3-5ms p50 |
| Shadow-path completion | < 2s | ~500ms-1.5s |
| Dashboard live update | < 500ms from verdict to UI | ~200ms (SSE) |

## Governance checks (14 total)

### Fast-path (synchronous, <10ms)

| Check | Axis | Description |
|---|---|---|
| Secret detection | Responsibility | Regex + entropy scoring for API keys, tokens |
| PII detection | Responsibility | Regex/pattern matching for SSN, email, phone, etc. |
| Unsafe content | Responsibility | Keyword-based harmful content detection |
| Cost cap | Cost | Per-app max token limit enforcement |
| Retry detection | Cost | Session-aware loop/retry detection |
| Tool-use risk | Performance | Detects function_call/tool_use patterns, applies 1.5x multiplier |
| Session risk | All | Cumulative risk accumulation across conversation turns |

### Shadow-path (asynchronous, <2s)

| Check | Axis | Description |
|---|---|---|
| Prompt injection | Responsibility | 3-layer: patterns, structural heuristics, encoding |
| Groundedness | Performance | NLI model scoring of claims vs context |
| Hallucination | Performance | DeepEval LLM-as-a-judge cross-reference |
| Bias classifier | Responsibility | Native classifier + guardrails sidecar |
| Verbosity | Cost | Response-to-prompt ratio and info density |
| Toxicity | Responsibility | LLM Guard toxicity classification |
| Semantic PII | Responsibility | NER-based re-identification on responses |
| PII (Presidio) | Responsibility | Microsoft Presidio scan on responses |

## Observability

- `/health` — gateway liveness
- `/ready` — dependency checks (PostgreSQL, NATS connectivity)
- Structured logs with `correlation_id` threaded through every event
- Counters: requests_total, verdicts_total (by axis, path, outcome)
- Histograms: proxy_latency_ms, fast_path_latency_ms, shadow_path_latency_ms
- Dashboard metrics: detection quality, FP/FN rates, trust score, feedback effectiveness

## Scaling strategy

| Component | Scaling approach |
|---|---|
| Proxy | Horizontal (stateless, round-robin LB) |
| Fast-path | In-process with proxy (scales with proxy) |
| Shadow workers | Horizontal (NATS consumer groups) |
| PostgreSQL | Read replicas for dashboard queries |
| Dashboard API | Horizontal (stateless, any replica) |
| Event bus | NATS clustering (3-node minimum) |

Load tested at 100 concurrent requests (3 apps, 3 axes) with <10ms governance
overhead on top of LLM inference time.
