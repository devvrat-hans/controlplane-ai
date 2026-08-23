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
   │              ├─ Runs fast-path checks (in-process, <10ms)
   │              │    ├─ Secret/PII regex + entropy   [IMPLEMENTED]
   │              │    ├─ Cost cap enforcement         [IMPLEMENTED]
   │              │    ├─ Retry/loop detection         [IMPLEMENTED]
   │              │    └─ Unsafe content keywords      [IMPLEMENTED]
   │              │
   │              └─ Publishes to shadow path (in-process or NATS)
   │
   ├─ Shadow-path worker (embedded or separate process)
   │    ├─ Prompt injection detection (3-layer)       [IMPLEMENTED]
   │    ├─ Groundedness scoring                       [IMPLEMENTED]
   │    ├─ Bias classifier (guardrails sidecar)       [IMPLEMENTED]
   │    └─ Verbosity heuristic                        [IMPLEMENTED]
   │
   ├─ Decision engine (verdict aggregation + policy)  [IMPLEMENTED]
   ├─ Audit service (hash-chained, append-only)       [IMPLEMENTED]
   ├─ Cost accounting (token tracking)                [IMPLEMENTED]
   ├─ Escalation queue                                [IMPLEMENTED]
   └─ Notification (Slack/webhook)                    [IMPLEMENTED]

 Persistence: PostgreSQL via sqlx (or in-memory for zero-dep demo) [IMPLEMENTED]
 Messaging: in-process broker or NATS (same envelopes)             [IMPLEMENTED]
 Policy cache: arc-swap (lock-free hot-reload)                     [IMPLEMENTED]
```

### Runtime topology switch (`.env`)

| Concern | Local demo (default) | Infrastructure mode |
|---|---|---|
| Persistence | `memory` (planned) | `postgres` (DATABASE_URL) |
| Event bus | `inproc` | `nats` (NATS_URL) |
| Shadow worker | embedded in gateway | separate process (planned) |

Both topologies run the **same** pipeline code; only the adapters differ.

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
    │
    └── NATS ──→ Shadow Analysis Workers (scalable)
                    ├── Groundedness (judge model)
                    ├── Bias (ONNX, dedicated GPU)
                    └── Verbosity
    │
    ▼
 Decision Service ──→ Policy Store (PostgreSQL)
    │
    ├── Audit Service (PostgreSQL, append-only, hash-chained)
    ├── Cost Service (PostgreSQL + ClickHouse for analytics)
    ├── Escalation Service
    └── Notification Service (Slack, webhooks, email)

 Infrastructure:
    PostgreSQL (system of record: policies, audit, verdicts)
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
    ▼
Proxy captures (correlation_id assigned)
    │
    ├── Fast path (sync, in-process)
    │     └── Verdict: pass / edit / block
    │
    └── Shadow path (async, NATS)
          └── Verdict: pass / escalate
    │
    ▼
Decision engine aggregates all verdicts
    │
    ├── Final outcome → Audit record (hash-chained)
    ├── If escalate → Escalation case created
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

## Latency budget

| Stage | Target | Notes |
|---|---|---|
| Proxy overhead (forwarding only) | < 3ms | hyper/Pingora-level |
| Fast-path policy checks | < 10ms p50 / < 25ms p99 | Hard budget |
| Shadow-path completion | < 2s | Doesn't block the user |
| Dashboard live update | < 500ms from verdict to UI | Via SSE |

## Observability

- `/health` — gateway liveness
- `/ready` — dependency checks (PostgreSQL, NATS connectivity)
- Structured logs with `correlation_id` threaded through every event
- Counters: requests_total, verdicts_total (by axis, path, outcome)
- Histograms: proxy_latency_ms, fast_path_latency_ms, shadow_path_latency_ms
