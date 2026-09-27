# AGENTS.md — Service Contracts & Responsibilities

This document defines the contract for each service (crate) in the ControlPlane.ai
system. Each service is a specialist with bounded inputs, outputs, allowed actions,
and forbidden actions.

---

## General principles

1. **Single responsibility**: each crate owns one concern. If a change touches
   two crates, it flows through an event or an API call, never a shared mutable
   reference.
2. **Fail-open**: any service that sits in the synchronous request path (proxy,
   fast-path) must fail open on its own errors. Shadow-path services may fail
   closed (they don't block the user).
3. **Correlation ID threading**: every operation carries a `correlation_id` from
   the original intercepted call. This is the join key across all services and
   the audit trail.
4. **No LLM in the decision path**: the fast-path never calls an LLM. The
   shadow-path may use a small judge model, but the final verdict is
   deterministic given the scores and policy thresholds.
5. **Append-only audit**: no service may DELETE or UPDATE audit records. The
   hash chain is the integrity guarantee.

---

## Service contracts

### `common` — Domain Layer

- **Owns**: Type definitions, enums, event envelopes, error types
- **Inputs**: None (library crate)
- **Outputs**: Types used by all other crates
- **Allowed**: Define shared types, implement Display/Serialize/Deserialize
- **Forbidden**: Any I/O, any side effects, any business logic

### `platform` — Infrastructure Adapters

- **Owns**: Database pool, NATS connection, config loading, health checks
- **Inputs**: Environment variables, .env file
- **Outputs**: Connected adapters (PgPool, NatsClient, AppConfig)
- **Allowed**: Open connections, expose health endpoints, provide metrics
- **Forbidden**: Business logic, domain decisions, direct request handling

### `proxy` — Ingress Reverse Proxy

- **Owns**: Accepting inbound requests, forwarding to upstream, capturing responses
- **Inputs**: HTTP requests from client applications
- **Outputs**: HTTP responses (possibly modified by fast-path), events to shadow-path
- **Allowed**: Forward requests, generate correlation_id, invoke fast-path, publish
  captured calls to event bus
- **Forbidden**: Making governance decisions (delegates to fast-path), blocking
  without a fast-path verdict, modifying requests before forwarding

### `fast-path` — Synchronous Policy Engine

- **Owns**: All checks that must complete before the response reaches the client
- **Inputs**: Response body, correlation_id, app_id, policy cache
- **Outputs**: FastPathResult (outcome, edits, verdicts)
- **Allowed**: Regex matching, entropy scoring, counter checks, keyword matching,
  reading from the lock-free policy cache
- **Forbidden**: Network calls, database queries, LLM invocations, anything that
  cannot complete in <10ms. If a check might exceed budget, it belongs in shadow-path.
- **Latency budget**: <10ms p50, <25ms p99 (hard)
- **Failure mode**: FAIL OPEN. On panic/timeout, return Outcome::Pass.

### `shadow-analysis` — Asynchronous Deep Analysis

- **Owns**: Expensive checks that cannot fit in the fast-path latency budget
- **Inputs**: Captured request+response via NATS event
- **Outputs**: Verdicts published to NATS (axis, outcome, confidence, reason)
- **Allowed**: Call judge models, run ONNX inference, perform NLP analysis,
  take up to 2 seconds per check
- **Forbidden**: Blocking the client response, modifying delivered responses,
  making final governance decisions (only produces scores; decision engine decides)
- **Latency budget**: <2s (target, not blocking)
- **Failure mode**: Log error, publish no verdict (absence of shadow verdict = pass)

### `decision` — Verdict Aggregation & Policy

- **Owns**: Combining fast-path and shadow-path verdicts into a final outcome,
  policy CRUD, threshold management
- **Inputs**: Verdicts from both paths, policy configuration
- **Outputs**: Final decision record, policy API responses
- **Allowed**: Aggregate verdicts, apply policy thresholds, version policies,
  publish final decisions
- **Forbidden**: Running detection checks itself, modifying the response,
  making decisions without a verdict

### `audit` — Hash-Chained Ledger

- **Owns**: The tamper-evident audit trail
- **Inputs**: Final decision records (call_id, verdict_id, action_taken)
- **Outputs**: Audit records with hash chain, verification results
- **Allowed**: INSERT audit records, compute hash chains, verify integrity
- **Forbidden**: UPDATE or DELETE any audit record, ever. Break the chain.
  Expose raw payloads without access control.
- **Integrity guarantee**: SHA-256(prev_hash + call_id + verdict_id + action + timestamp)

### `cost-accounting` — Token & Spend Tracking

- **Owns**: Per-app, per-window token counts and cost aggregation
- **Inputs**: Intercepted call token counts, pricing configuration
- **Outputs**: Cost summaries, anomaly alerts, time-series data
- **Allowed**: Aggregate tokens, compute costs, detect anomalies against baselines
- **Forbidden**: Blocking requests (cost checks that block are in fast-path),
  modifying responses

### `escalation` — Human Review Queue

- **Owns**: The lifecycle of cases requiring human judgment
- **Inputs**: Verdicts with outcome=Escalate
- **Outputs**: Escalation cases, resolution records
- **Allowed**: Create cases, assign to reviewers, record resolutions,
  feed overrides back to policy engine
- **Forbidden**: Auto-resolving cases (the point is human judgment),
  showing low-confidence cases that don't genuinely need review

### `dashboard-api` — Backend-for-Frontend

- **Owns**: Serving the Next.js dashboard with REST + SSE
- **Inputs**: Requests from the frontend, NATS verdict stream
- **Outputs**: JSON API responses, SSE event stream
- **Allowed**: Proxy to other services, aggregate stats, push live updates,
  enforce auth (JWT)
- **Forbidden**: Business logic (delegates to decision/audit/cost/escalation
  services), direct database writes

### `mcp-server` — Agent Protocol Gateway

- **Owns**: The Model Context Protocol surface — tool/resource catalogue, JSON-RPC
  framing, and the stdio + Streamable HTTP transports that expose governance data
  and reviewer actions to external agents
- **Inputs**: MCP JSON-RPC messages over stdio or HTTP, bearer tokens
- **Outputs**: Tool results and read-only resources sourced from the dashboard API
  and the proxy, sanitized MCP errors, health/readiness responses
- **Allowed**: Authenticate and authorize callers (capability + optional app scope),
  enforce rate limits, timeouts and payload bounds, redact sensitive data, call the
  dashboard API and proxy, propagate correlation IDs
- **Forbidden**: Duplicating governance logic, touching the database directly, exposing
  internal-only services (guardrails sidecar, Laya/Jev judge) by default, sitting on the
  synchronous request path, returning unsanitized upstream errors or payloads
- **Failure mode**: Fail closed on authentication and capacity; a failure here must never
  affect proxy or dashboard traffic

---

### `notification` — Alert Delivery

- **Owns**: Sending alerts on actionable events (block, escalate)
- **Inputs**: NATS events for block/escalate decisions
- **Outputs**: Slack messages, webhook calls
- **Allowed**: Send notifications, rate-limit to avoid spam, format messages
- **Forbidden**: Making governance decisions, blocking requests, accessing
  raw request/response payloads

### `gateway` — Binary Entrypoint

- **Owns**: Process lifecycle, startup, graceful shutdown
- **Inputs**: Configuration (.env)
- **Outputs**: Running services
- **Allowed**: Initialize all services, wire them together, handle signals
- **Forbidden**: Business logic (pure orchestration)

---

## Event subjects (NATS / in-process)

| Subject | Publisher | Subscribers |
|---|---|---|
| `controlplane.intercept.captured` | proxy | cost-accounting |
| `controlplane.intercept.shadow` | proxy | shadow-analysis |
| `controlplane.verdict.fast` | proxy (fast-path) | decision, dashboard-api |
| `controlplane.verdict.shadow` | shadow-analysis | decision, dashboard-api |
| `controlplane.decision.final` | decision | audit, escalation, notification, dashboard-api |
| `controlplane.escalation.created` | escalation | notification, dashboard-api |
| `controlplane.policy.updated` | decision | fast-path (cache reload trigger) |

---

## Build rules

1. **One developer, one workspace**: this is a solo-developer hackathon build.
   The workspace is the unit of deployment, crates are the unit of contract.
2. **No premature extraction**: a crate becomes a separate service when it needs
   independent scaling or a different security boundary, not before.
3. **Honesty over ambition**: if something is scaffolded but not implemented,
   label it `SCAFFOLD`. If it's target-state, label it `TARGET`. Never present
   scaffolded code as working.
4. **Tests prove contracts**: each crate's tests verify its contract from this
   document, not implementation details.
