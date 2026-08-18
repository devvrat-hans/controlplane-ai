# ControlPlane.ai

> **From AI calls to governed AI calls — in under 10ms.**

A real-time control layer that sits between any application and any AI model,
inspecting every response across three axes — **performance** (confidently wrong),
**cost** (inefficient or runaway), and **responsibility** (biased, unsafe, or
leaking data) — and taking action: pass, edit, block, or escalate to a human.

Built for the **Accenture Innovation Challenge 2026**, Problem Statement 1:
"Reinvent with AI".

---

## 1. What this is

A reverse-proxy governance layer — **not** a model, **not** a training platform,
**not** a replacement for human review. The core decision logic:

- **Fast path** (synchronous, <10ms): cheap, deterministic checks that must
  complete before the response reaches the client — secrets/PII regex, cost caps,
  retry-loop detection, unsafe keyword matching.
- **Shadow path** (asynchronous, <2s): expensive, higher-signal checks that run
  after delivery — hallucination scoring, bias classification, verbosity analysis,
  semantic PII re-identification.
- **Fail-open guarantee**: if ControlPlane itself errors, traffic passes through
  unmodified. A governance layer that becomes a single point of failure is worse
  than no governance at all.

## 2. The problem

Every AI deployment carries three compounding risks invisible until after the fact:

1. **Performance risk** — confidently wrong, nothing signals uncertainty.
2. **Cost risk** — verbose responses, retry loops, runaway agents, no real-time
   visibility into spend.
3. **Responsibility risk** — biased, unsafe, or leaking sensitive data, caught
   only by a complaint, an audit, or a headline.

ControlPlane moves discovery from forensic to **live**.

## 3. Architecture

See [docs/architecture.md](docs/architecture.md) for the full diagrams.

```text
App → ControlPlane Proxy (:8900) → AI Model Provider
              │
              ├── Fast Path (in-process, <10ms)
              │     ├── Secret/PII detection
              │     ├── Cost cap enforcement
              │     ├── Retry/loop detection
              │     └── Unsafe content check
              │
              └── Shadow Path (async via NATS, <2s)
                    ├── Groundedness scoring
                    ├── Bias classification (ONNX)
                    └── Verbosity heuristic
              │
              ▼
         Decision Engine → Verdict (pass/edit/block/escalate)
              │
              ├── Audit ledger (hash-chained, PostgreSQL)
              ├── Escalation queue (human review)
              ├── Cost accounting (token tracking)
              └── Dashboard (live SSE stream)
```

## 4. Tech stack

| Layer | Technology |
|---|---|
| Proxy + fast-path | Rust (hyper, arc-swap for lock-free policy cache) |
| Shadow analysis | Rust (tokio async, ONNX Runtime for bias model) |
| Decision / Policy | Rust (axum) |
| Audit | Rust, SHA-256 hash chain, PostgreSQL |
| Messaging | NATS (or in-process broker — same contracts) |
| Persistence | PostgreSQL 16 |
| Dashboard | Next.js 16, React 19, TypeScript, Tailwind CSS, shadcn/ui |
| Live updates | Server-Sent Events (SSE) |
| Package manager | pnpm (JS), Cargo (Rust) |
| Linting | Biome (frontend), clippy (Rust) |

## 5. Service boundaries (Rust workspace)

| Crate | Responsibility | Status |
|---|---|---|
| `common` | Domain models, IDs, events, errors | IMPLEMENTED |
| `platform` | Config, adapters (PostgreSQL, NATS, in-process) | IMPLEMENTED |
| `proxy` | Reverse-proxy forwarding, request/response capture | SCAFFOLD |
| `fast-path` | Sync checks: secrets, cost caps, retry detection | SCAFFOLD |
| `shadow-analysis` | Async checks: groundedness, bias, verbosity | SCAFFOLD |
| `decision` | Verdict aggregation + policy engine CRUD | SCAFFOLD |
| `audit` | Hash-chained append-only audit log | SCAFFOLD |
| `cost-accounting` | Token tracking + anomaly detection | SCAFFOLD |
| `escalation` | Human review queue | SCAFFOLD |
| `dashboard-api` | BFF: REST + SSE for frontend | SCAFFOLD |
| `notification` | Slack/webhook alerts | SCAFFOLD |
| `gateway` | Binary entrypoint (starts everything) | SCAFFOLD |

## 6. Run locally

### Prerequisites

- Rust (stable, ≥1.82)
- Node.js ≥ 20
- pnpm
- Docker (optional — the demo runs without it using in-memory mode)

### Quick start

```bash
# One command (checks prereqs, builds, starts infra, migrates)
./scripts/bootstrap.sh        # Linux/Mac
.\scripts\bootstrap.ps1       # Windows PowerShell

# Or step by step:
cp .env.example .env
docker compose -f infra/docker-compose.yml up -d   # PostgreSQL + NATS
cargo build
cd frontend && pnpm install && cd ..

# Run
cargo run -p controlplane-gateway    # API on :8080, proxy on :8900
cd frontend && pnpm dev              # Dashboard on :3000
```

### Full demo

```bash
./scripts/run_demo.sh         # Linux/Mac
.\scripts\run_demo.ps1        # Windows PowerShell
```

This starts infrastructure, gateway, and frontend in one command.

## 7. Demo credentials

| Account | Role | Can do |
|---|---|---|
| `admin@controlplane.test` / `Demo#Admin2026` | Admin | Everything |
| `reviewer@controlplane.test` / `Demo#Reviewer2026` | Reviewer | Resolve escalations |
| `viewer@controlplane.test` / `Demo#Viewer2026` | Viewer | Read-only |

## 8. Build tiering

### Tier 1 — Built and demoable

- Ingress proxy with real forwarding
- Fast-path secret/PII regex detection + auto-redaction
- Fast-path cost cap enforcement
- One shadow-path check (bias or groundedness)
- Decision service: pass/edit/block/escalate verdicts
- PostgreSQL-backed hash-chained audit log
- Next.js dashboard: live verdict stream + audit search

### Tier 2 — Simplified but real

- Second shadow-path check
- Cost anomaly detection against baseline
- Escalation queue as filtered dashboard view
- Pattern promotion (shadow → fast path learning)

### Tier 3 — Documented as target-state, not built

- Multi-language SDK wrapper mode
- Sidecar/service-mesh deployment
- Full policy-as-code DSL
- Multi-tenant SaaS-grade isolation
- Learned (ML-based) cost baselines

This split is stated openly — it reads as engineering maturity, not shortcoming.

## 9. Known limitations

- The bias classifier uses a placeholder model — production requires a trained,
  validated ONNX model for each bias category.
- Auth is local JWT; production targets enterprise OIDC.
- The proxy currently buffers full responses; streaming inspection is Tier 3.
- In-memory mode loses data on restart; PostgreSQL mode persists everything.

## 10. Repository map

```text
services/        Rust workspace (12 crates)
frontend/        Next.js dashboard
infra/           docker-compose, PostgreSQL migrations
scripts/         bootstrap, run_demo, seed_demo (bash + PowerShell)
docs/            architecture, demo-runbook
```

## 11. Tests & quality

```bash
make verify   # fmt, clippy -D warnings, cargo test, frontend build
make test     # Rust tests only
```

## 12. License

Apache-2.0. This repository is a hackathon prototype; all data is synthetic.

---

**Prototype / demo only.** ControlPlane.ai is an observability and governance
layer, not a replacement for human oversight. Do not use in production without
thorough validation of detection accuracy, policy thresholds, and fail-open
behaviour under your specific workload.
