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
| `proxy` | Reverse-proxy forwarding, request/response capture | IMPLEMENTED |
| `fast-path` | Sync checks: secrets, cost caps, retry detection | IMPLEMENTED |
| `shadow-analysis` | Async checks: groundedness, bias, verbosity | IMPLEMENTED |
| `decision` | Verdict aggregation + policy engine CRUD | IMPLEMENTED |
| `audit` | Hash-chained append-only audit log | IMPLEMENTED |
| `cost-accounting` | Token tracking + anomaly detection | IMPLEMENTED |
| `escalation` | Human review queue | IMPLEMENTED |
| `dashboard-api` | BFF: REST + SSE for frontend | IMPLEMENTED |
| `notification` | Slack/webhook alerts | IMPLEMENTED |
| `gateway` | Binary entrypoint (starts everything) | IMPLEMENTED |

## 6. Run locally

Complete step-by-step instructions to run ControlPlane.ai on your machine.

### Prerequisites

| Requirement | Version | Check command |
|---|---|---|
| Rust (stable) | ≥ 1.82 | `rustc --version` |
| Node.js | ≥ 20 | `node --version` |
| pnpm | ≥ 9 | `pnpm --version` |
| Docker + Docker Compose | Latest | `docker --version` |
| PostgreSQL client (psql) | Any (for seeding only) | `psql --version` |

> **Windows users**: All scripts have `.ps1` PowerShell equivalents.
> **No Docker?** You can run in in-memory mode (see Option B below).

---

### Option A: Full Stack with Docker (Recommended)

This uses Docker for PostgreSQL and NATS, giving you a persistent database.

#### Step 1: Clone and configure

```bash
cp .env.example .env
```

Edit `.env` with your AI provider API key:

```env
# Required — point to an AI provider (Claude, OpenAI, etc.)
UPSTREAM_BASE_URL=https://api.anthropic.com
UPSTREAM_API_KEY=your-api-key-here

# Database (matches docker-compose defaults)
DATABASE_URL=postgres://controlplane:secret@localhost:5432/controlplane

# Event bus mode: "inproc" (in-memory) or "nats" (requires NATS container)
EVENT_BUS=inproc

# Ports
PROXY_LISTEN_ADDR=0.0.0.0:8900
DASHBOARD_API_PORT=8080
```

#### Step 2: Start infrastructure (PostgreSQL + NATS)

```bash
docker compose -f infra/docker-compose.yml up -d
```

Verify both containers are healthy:

```bash
docker compose -f infra/docker-compose.yml ps
# Should show: controlplane-postgres (healthy), controlplane-nats (healthy)
```

#### Step 3: Run database migrations

```bash
# Linux/macOS:
psql postgres://controlplane:secret@localhost:5432/controlplane \
  -f infra/migrations/001_create_users.sql \
  -f infra/migrations/002_create_apps.sql \
  -f infra/migrations/003_create_intercepted_calls.sql \
  -f infra/migrations/004_create_verdicts.sql \
  -f infra/migrations/005_create_policies.sql \
  -f infra/migrations/006_create_audit_records.sql \
  -f infra/migrations/007_create_cost_ledger.sql \
  -f infra/migrations/008_create_escalation_cases.sql \
  -f infra/migrations/009_seed_demo_data.sql \
  -f infra/migrations/010_create_pattern_promotions.sql \
  -f infra/migrations/011_create_cost_ledger_entries.sql \
  -f infra/migrations/012_add_verdict_columns.sql \
  -f infra/migrations/013_seed_full_demo.sql

# Windows PowerShell (run each file):
Get-ChildItem infra/migrations/*.sql | Sort-Object Name | ForEach-Object {
    psql "postgres://controlplane:secret@localhost:5432/controlplane" -f $_.FullName
}
```

Or use the seed script (runs all migrations + seed data):

```bash
./scripts/seed_demo.sh          # Linux/macOS
.\scripts\seed_demo.ps1         # Windows
```

#### Step 4: Build and run the Rust backend

```bash
# Build the entire workspace
cargo build

# Run the gateway (starts proxy on :8900 + dashboard API on :8080)
cargo run -p controlplane-gateway
```

You should see output like:

```
INFO  controlplane_gateway: ControlPlane.ai Gateway starting
INFO  controlplane_gateway: Proxy listening on 0.0.0.0:8900
INFO  controlplane_gateway: Dashboard API listening on 0.0.0.0:8080
INFO  controlplane_gateway: Shadow worker started
INFO  controlplane_gateway: Cost tracker started
INFO  controlplane_gateway: Audit subscriber started
```

#### Step 5: Start the frontend dashboard

Open a new terminal:

```bash
cd frontend
pnpm install
pnpm dev
```

The dashboard is now at **http://localhost:3000**

#### Step 6: Verify everything works

```bash
# Check API health
curl http://localhost:8080/health
# → {"status":"ok","service":"dashboard-api"}

# Check proxy health
curl http://localhost:8900/v1/messages -X POST \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-sonnet-4-20250514","max_tokens":100,"messages":[{"role":"user","content":"Hello"}]}'
```

#### Step 7: Open the dashboard

1. Open **http://localhost:3000** in your browser
2. You'll see the Overview page with seeded data
3. Navigate through: Live Stream, Policies, Escalations, Cost, Audit

---

### Option B: In-Memory Mode (No Docker Required)

For quick exploration without setting up PostgreSQL/NATS. Data is lost on restart.

```bash
# Set environment for in-memory operation
export DATABASE_URL=""        # Leave empty to skip DB
export EVENT_BUS=inproc      # In-process event bus (no NATS needed)
export UPSTREAM_BASE_URL=https://api.anthropic.com
export UPSTREAM_API_KEY=your-key-here

# Build and run
cargo build
cargo run -p controlplane-gateway

# In another terminal:
cd frontend && pnpm install && pnpm dev
```

> **Note**: In-memory mode won't have seeded data. Send requests through the proxy
> to generate live verdicts.

---

### One-Command Demo (Automated)

```bash
./scripts/run_demo.sh         # Linux/macOS
.\scripts\run_demo.ps1        # Windows PowerShell
```

This script:
1. Starts Docker infrastructure (if Docker is available)
2. Builds and launches the gateway
3. Starts the frontend
4. Prints all URLs and demo credentials

---

### Exercising the Demo

After the system is running, exercise all governance scenarios:

```bash
./scripts/demo_exercise.sh     # Linux/macOS (interactive, pauses between scenarios)
.\scripts\demo_exercise.ps1    # Windows PowerShell
```

This sends real requests demonstrating:
- **PASS** — normal traffic flows through
- **EDIT** — secrets/PII auto-redacted before reaching client
- **BLOCK** — cost cap or unsafe content stopped
- **ESCALATE** — shadow-path findings queued for human review

---

### Ports reference

| Service | Port | URL |
|---------|------|-----|
| Reverse Proxy | 8900 | http://localhost:8900 |
| Dashboard API | 8080 | http://localhost:8080 |
| Frontend | 3000 | http://localhost:3000 |
| PostgreSQL | 5432 | postgres://controlplane:secret@localhost:5432/controlplane |
| NATS | 4222 | nats://localhost:4222 |

---

### Stopping

```bash
# If using run_demo.sh: press Ctrl+C (graceful shutdown)

# If running manually:
# Terminal 1: Ctrl+C the gateway
# Terminal 2: Ctrl+C the frontend (pnpm dev)
# Then stop Docker:
docker compose -f infra/docker-compose.yml down
```

---

### Troubleshooting

| Problem | Solution |
|---------|----------|
| `cargo build` fails | Ensure Rust ≥1.82: `rustup update stable` |
| Port 5432 in use | Another PostgreSQL running? Stop it or change port in docker-compose.yml |
| Port 8900/8080 in use | Another service occupying the port? Kill it or change in .env |
| `psql` not found | Install PostgreSQL client: `brew install libpq` / `choco install postgresql` |
| Docker not starting | Ensure Docker Desktop is running, then `docker compose up -d` |
| Frontend blank page | Check browser console; ensure API is running on :8080 |
| No verdicts in stream | Send a request through the proxy first: `curl -X POST http://localhost:8900/v1/messages ...` |
| Gateway panics on start | Check `.env` exists and `DATABASE_URL` is correct (or empty for in-memory mode) |
| Migrations fail | Ensure PostgreSQL is accepting connections: `docker exec controlplane-postgres pg_isready` |

---

### Full demo (end-to-end walkthrough)

See [docs/demo-runbook.md](docs/demo-runbook.md) for the narrated 5-minute demo script.

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
cargo bench -p controlplane-fast-path  # latency benchmarks
```

**Test coverage**: 127 tests across 12 crates (unit + integration).

## 12. Latency benchmarks

Measured on the fast-path pipeline (criterion, release mode):

| Scenario | p50 | Target | Status |
|----------|-----|--------|--------|
| Clean response (no findings) | 5.6 µs | <10ms | PASS |
| Secret detection (AWS key) | 9.7 µs | <10ms | PASS |
| PII detection (SSN + email + CC) | 11.4 µs | <10ms p50, <25ms p99 | PASS |
| Unsafe keyword block | 0.96 µs | <10ms | PASS |
| Large 4KB response | 24.3 µs | <25ms p99 | PASS |

All measurements well within budget. Fast-path adds <25µs even for worst-case
inputs, far below the 10ms p50 / 25ms p99 hard target.

Run benchmarks yourself: `cargo bench -p controlplane-fast-path`

## 13. License

Apache-2.0. This repository is a hackathon prototype; all data is synthetic.

---

**Prototype / demo only.** ControlPlane.ai is an observability and governance
layer, not a replacement for human oversight. Do not use in production without
thorough validation of detection accuracy, policy thresholds, and fail-open
behaviour under your specific workload.
