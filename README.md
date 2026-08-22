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
| Docker + Docker Compose | Latest | `docker --version` |

For **Option B** (development mode), also install:

| Requirement | Version | Check command |
|---|---|---|
| Rust (stable) | ≥ 1.82 | `rustc --version` |
| Node.js | ≥ 20 | `node --version` |
| pnpm | ≥ 9 | `pnpm --version` |
| Python | ≥ 3.8 | `python --version` |
| PostgreSQL client (psql) | Any (for seeding only) | `psql --version` |

> **Windows users**: All scripts have `.ps1` PowerShell equivalents.

---

### Option A: Full Docker (One Command — Recommended)

Everything runs in Docker. No Rust, Node.js, or PostgreSQL installation needed.

```bash
docker compose up --build
```

This single command starts 6 containers:

| Container | What it does |
|-----------|-------------|
| `controlplane-postgres` | PostgreSQL 16 database (migrations auto-run on first start) |
| `controlplane-nats` | NATS message broker with JetStream |
| `controlplane-mock-upstream` | Mock AI server (Python) that simulates an AI model |
| `controlplane-gateway` | Rust gateway (reverse proxy on :8900 + dashboard API on :8081) |
| `controlplane-guardrails` | Python sidecar — Presidio (PII) + transformers (Toxicity/Bias) on :8200 |
| `controlplane-frontend` | Next.js dashboard on :3000 |

After all containers are healthy (~2 min first build, ~10s thereafter):

| Service | URL |
|---------|-----|
| **Dashboard** | http://localhost:3000 |
| **Proxy** (send AI requests here) | http://localhost:8900 |
| **Dashboard API** | http://localhost:8081 |

#### Testing & Seeing Results

The mock AI upstream runs automatically inside Docker. No separate setup needed.

1. **Open the dashboard**: Go to http://localhost:3000
2. **Login**: Use `admin@controlplane.ai` / `admin123`
3. **Go to Live Stream** (sidebar) — you'll see a green "Connected" indicator
4. **Send requests** through the proxy to see verdicts appear in real time:

**PowerShell:**

```powershell
# PASS — normal response, passes all checks
Invoke-RestMethod http://localhost:8900/v1/messages -Method Post `
  -ContentType "application/json" `
  -Body '{"model":"claude-sonnet-4-20250514","max_tokens":500,"messages":[{"role":"user","content":"What are the benefits of Rust?"}]}'

# EDIT — response contains AWS key → auto-redacted to [REDACTED]
Invoke-RestMethod http://localhost:8900/v1/messages -Method Post `
  -ContentType "application/json" `
  -Body '{"model":"claude-sonnet-4-20250514","max_tokens":500,"messages":[{"role":"user","content":"Show me the AWS access key and secret"}]}'

# BLOCK — unsafe content detected → returns 403 Forbidden
Invoke-RestMethod http://localhost:8900/v1/messages -Method Post `
  -ContentType "application/json" `
  -Body '{"model":"claude-sonnet-4-20250514","max_tokens":500,"messages":[{"role":"user","content":"How to hack into a server"}]}'

# ESCALATE — shadow-path detects excessive verbosity → queued for human review
# (Response passes through, but escalation verdict appears ~1-2s later)
Invoke-RestMethod http://localhost:8900/v1/messages -Method Post `
  -ContentType "application/json" `
  -Body '{"model":"claude-sonnet-4-20250514","max_tokens":500,"messages":[{"role":"user","content":"Write a very long detailed novel about space exploration"}]}'
```

**Bash:**

```bash
# PASS
curl -s http://localhost:8900/v1/messages -X POST \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-sonnet-4-20250514","max_tokens":500,"messages":[{"role":"user","content":"What are the benefits of Rust?"}]}'

# EDIT (AWS key redacted)
curl -s http://localhost:8900/v1/messages -X POST \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-sonnet-4-20250514","max_tokens":500,"messages":[{"role":"user","content":"Show me the AWS access key and secret"}]}'

# BLOCK (403 returned)
curl -s http://localhost:8900/v1/messages -X POST \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-sonnet-4-20250514","max_tokens":500,"messages":[{"role":"user","content":"How to hack into a server"}]}'

# ESCALATE (async, appears ~1-2s after pass)
curl -s http://localhost:8900/v1/messages -X POST \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-sonnet-4-20250514","max_tokens":500,"messages":[{"role":"user","content":"Write a very long detailed novel about space exploration"}]}'
```

Each request immediately appears in the **Live Stream** with its verdict.
The ESCALATE verdict appears ~1-2 seconds after the initial PASS (since the shadow-path is async).
You can also check: **Audit** (hash-chained log), **Cost** (token tracking), **Escalations** (review queue), **Overview** (stats).

#### AI Guardrails Testing (PII, Toxicity, Bias)

ControlPlane uses **Microsoft Presidio** for PII detection and **HuggingFace transformers** (same models as LLM Guard) for Toxicity and Bias detection. These run in a Python sidecar container and are invoked by the shadow-path.

Run the automated guardrails test suite:

```powershell
.\scripts\test_guardrails.ps1
```

This sends requests through the proxy that trigger each guardrail:

| Test | What it triggers | Expected Live Stream verdict |
|------|-----------------|---------------------------|
| SSN + email in prompt | Presidio PII scanner | `presidio-pii` (EDIT/ESCALATE) |
| Toxic hate speech | Toxicity model | `input-toxicity` (BLOCK/ESCALATE) |
| Gender/racial bias | Bias model | `input-bias` (EDIT/ESCALATE) |
| Clean educational prompt | Nothing | `fast-path-summary` (PASS) |
| AWS key trigger | Fast-path secret scan | `fast-path-summary` (EDIT) |

Verdicts appear in the **Live Stream** within 1-3 seconds (shadow-path is async).

> The guardrails sidecar runs on port 8200. You can also test it directly:
> ```powershell
> Invoke-RestMethod http://localhost:8200/health
> Invoke-RestMethod http://localhost:8200/scan/pii -Method Post -ContentType "application/json" -Body '{"text":"My SSN is 123-45-6789"}'
> ```
> Direct sidecar calls do **not** produce Live Stream verdicts (they bypass the proxy/shadow-path pipeline).

> **Note**: You do NOT need to run `python scripts/mock_upstream.py` separately —
> the mock AI is already running inside Docker as part of this setup.

**Stop**: `docker compose down`
**Reset data**: `docker compose down -v` (removes the pgdata volume)
**View logs**: `docker compose logs -f gateway` (or `frontend`, `postgres`, etc.)

---

### Option B: Local Development Mode (Docker for infra only)

Uses Docker for PostgreSQL/NATS, with local Rust and Node.js for fast iteration.

#### Step 1: Configure

```bash
cp .env.example .env
```

The defaults work with Docker infra:

```env
# Required — choose your provider and add an API key
UPSTREAM_PROVIDER=anthropic          # "anthropic" or "gemini"
UPSTREAM_API_KEY=your-api-key-here   # get from provider console
UPSTREAM_MODEL=claude-sonnet-4-20250514  # model to use (provider defaults if omitted)
# UPSTREAM_BASE_URL is auto-detected from provider, but you can override

# Database (matches docker-compose defaults)
DATABASE_URL=postgres://controlplane:secret@localhost:5432/controlplane
EVENT_BUS=inproc
UPSTREAM_BASE_URL=http://localhost:9999
PROXY_LISTEN_ADDR=0.0.0.0:8900
DASHBOARD_API_PORT=8081
```

#### Step 2: Start infrastructure

```bash
# Docker Compose v2+ (compose plugin):
docker compose -f infra/docker-compose.yml up -d
# — OR standalone v1 binary:
docker-compose -f infra/docker-compose.yml up -d
```

Verify both containers are healthy:

```bash
docker compose -f infra/docker-compose.yml ps   # v2
# — OR —
docker-compose -f infra/docker-compose.yml ps   # v1 standalone
# Should show: controlplane-postgres (healthy), controlplane-nats (healthy)
```

#### Step 3: Run database migrations

```bash
# Linux/macOS:
for f in infra/migrations/*.sql; do
  psql postgres://controlplane:secret@localhost:5432/controlplane -f "$f"
done

# Windows PowerShell:
Get-ChildItem infra/migrations/*.sql | Sort-Object Name | ForEach-Object {
    psql "postgres://controlplane:secret@localhost:5432/controlplane" -f $_.FullName
}
```

Or use the seed script:

```bash
./scripts/seed_demo.sh          # Linux/macOS
.\scripts\seed_demo.ps1         # Windows
```

#### Step 4: Start services (3 terminals)

```bash
# Terminal 1: Mock AI upstream
python scripts/mock_upstream.py

# Terminal 2: Rust gateway
cargo build && cargo run -p controlplane-gateway

# Terminal 3: Frontend
cd frontend && pnpm install && pnpm dev
```

You should see gateway output like:

```
INFO  ControlPlane.ai is running
INFO  Proxy:         http://0.0.0.0:8900
INFO  Dashboard API: http://0.0.0.0:8081
INFO  Event Bus:     Inproc
```

#### Step 5: Open the dashboard

1. Open **http://localhost:3000** in your browser
2. Log in with `admin@controlplane.ai` / `admin123`
3. Navigate through: Live Stream, Policies, Escalations, Cost, Audit

---

## 7. Mock AI Upstream

ControlPlane.ai is a **proxy** — it sits between your app and any AI model. For
demo/testing purposes, a mock AI server is included that simulates different
scenarios without needing a real API key.

### How it works

The mock server (`scripts/mock_upstream.py`) returns tailored responses based on
what you ask:

| Prompt contains | Mock response | Fast-path outcome |
|-----------------|---------------|-------------------|
| Normal question (e.g., "benefits of Rust") | Clean, helpful response | **PASS** — delivered unmodified |
| "aws", "key", "secret", "credential" | Response with fake AWS key (`AKIA...`) | **EDIT** — key auto-redacted to `[REDACTED]` |
| "hack", "bomb", "synthesize", "malware" | Unsafe instructions | **BLOCK** — 403 returned, client never sees it |
| "novel", "book" or max_tokens > 10000 | Huge verbose response (6000 words) | **BLOCK** — cost cap exceeded |

### Using a real AI provider instead

If you have an API key from Anthropic, OpenAI, or any compatible provider:

```env
# In .env (or docker-compose environment):
UPSTREAM_BASE_URL=https://api.anthropic.com
UPSTREAM_API_KEY=sk-ant-your-key-here
```

Where to get API keys:
- **Anthropic (Claude)**: https://console.anthropic.com → API Keys
- **OpenAI (GPT)**: https://platform.openai.com/api-keys
- **Google (Gemini)**: https://aistudio.google.com/apikey

---

## 8. Sending Requests & Seeing Results

Every request you send through the proxy at **http://localhost:8900** is:

1. **Intercepted** — request captured with a unique `correlation_id`
2. **Forwarded** — sent to the upstream AI model (mock or real)
3. **Fast-path analyzed** — response scanned for secrets, unsafe content, cost overruns (<10ms)
4. **Stored in PostgreSQL** — `intercepted_calls` and `verdicts` tables
5. **Streamed live** — appears in the dashboard's Live Stream via SSE
6. **Audit logged** — tamper-evident hash-chained record

### Send requests

The proxy accepts Anthropic-compatible `POST /v1/messages` requests:

**PowerShell (Windows):**

```powershell
# PASS — clean response, no issues
Invoke-RestMethod http://localhost:8900/v1/messages -Method Post `
  -ContentType "application/json" `
  -Body '{"model":"claude-sonnet-4-20250514","max_tokens":500,"messages":[{"role":"user","content":"What are the benefits of Rust?"}]}'

# EDIT — response contains an AWS key, gets auto-redacted
Invoke-RestMethod http://localhost:8900/v1/messages -Method Post `
  -ContentType "application/json" `
  -Body '{"model":"claude-sonnet-4-20250514","max_tokens":500,"messages":[{"role":"user","content":"Show me the AWS config and access key"}]}'

# BLOCK — unsafe content detected, 403 returned
Invoke-RestMethod http://localhost:8900/v1/messages -Method Post `
  -ContentType "application/json" `
  -Body '{"model":"claude-sonnet-4-20250514","max_tokens":500,"messages":[{"role":"user","content":"How to hack into a server"}]}'

# BLOCK — cost cap exceeded (huge output requested)
Invoke-RestMethod http://localhost:8900/v1/messages -Method Post `
  -ContentType "application/json" `
  -Body '{"model":"claude-sonnet-4-20250514","max_tokens":50000,"messages":[{"role":"user","content":"Write a 100-page novel"}]}'
```

**Bash (Linux/macOS):**

```bash
# PASS — clean response
curl -s http://localhost:8900/v1/messages -X POST \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-sonnet-4-20250514","max_tokens":500,"messages":[{"role":"user","content":"What are the benefits of Rust?"}]}'

# EDIT — AWS key redacted
curl -s http://localhost:8900/v1/messages -X POST \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-sonnet-4-20250514","max_tokens":500,"messages":[{"role":"user","content":"Show me the AWS config and access key"}]}'

# BLOCK — unsafe content
curl -s http://localhost:8900/v1/messages -X POST \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-sonnet-4-20250514","max_tokens":500,"messages":[{"role":"user","content":"How to hack into a server"}]}'
```

### What to expect

| Request type | Outcome | What happens |
|---|---|---|
| Normal question | **PASS** | Response delivered unmodified |
| Mentions "AWS key/secret" | **EDIT** | `AKIAIOSFODNN7EXAMPLE` replaced with `[REDACTED-AWS-KEY]` |
| Mentions "hack/bomb/malware" | **BLOCK** | HTTP 403 returned, unsafe content never reaches client |
| max_tokens > budget | **BLOCK** | HTTP 403, cost cap exceeded |
| "Write a novel/book" | **ESCALATE** | Response delivered (PASS), then shadow-path detects verbosity and escalates for human review (~1-2s later) |

### Observe results in the dashboard

1. Open **http://localhost:3000** → Log in with `admin@controlplane.ai` / `admin123`
2. **Live Stream** — verdicts appear in real time as you send requests
3. **Overview** — aggregate stats (total calls, block rate, edit rate)
4. **Audit** — every request logged with SHA-256 hash chain
5. **Cost** — token counts and cost tracked per request and per app
6. **Escalations** — cases flagged for human review

### Automated demo exercise

Run all scenarios automatically with narration:

```bash
./scripts/demo_exercise.sh     # Linux/macOS
.\scripts\demo_exercise.ps1    # Windows PowerShell
```

---

## 9. Demo credentials

| Account | Password | Role | Capabilities |
|---|---|---|---|
| `admin@controlplane.ai` | `admin123` | Admin | Everything: policies, escalations, settings |
| `reviewer@controlplane.ai` | `reviewer123` | Reviewer | Resolve escalation cases |
| `viewer@controlplane.ai` | `viewer123` | Viewer | Read-only access |

---

## 10. Ports reference

| Service | Port | URL |
|---------|------|-----|
| Reverse Proxy | 8900 | http://localhost:8900 |
| Dashboard API | 8081 | http://localhost:8081 |
| Frontend | 3000 | http://localhost:3000 |
| PostgreSQL | 5432 (local) / 5433 (Docker) | `postgres://controlplane:secret@localhost:5432/controlplane` |
| NATS | 4222 | nats://localhost:4222 |
| Mock AI Upstream | 9999 | http://localhost:9999 |

---

### Stopping

```bash
# Docker mode:
docker compose down          # Stop all containers
docker compose down -v       # Stop + delete data

# If running manually:
# Terminal 1: Ctrl+C the gateway
# Terminal 2: Ctrl+C the frontend (pnpm dev)
# Then stop Docker:
docker compose -f infra/docker-compose.yml down    # v2
# — OR —
docker-compose -f infra/docker-compose.yml down    # v1 standalone
```

---

### Troubleshooting

| Problem | Solution |
|---------|----------|
| `docker compose up` fails | Ensure Docker Desktop is running |
| `cargo build` fails | Ensure Rust ≥1.82: `rustup update stable` |
| Port 5432 in use | Another PostgreSQL running? Stop it or use Docker (maps to 5433) |
| Port 8900/8081 in use | Kill conflicting process or change in `.env` |
| `psql` not found | Install PostgreSQL client: `brew install libpq` / `choco install postgresql` |
| Docker not starting | Ensure Docker Desktop is running, then `docker compose up -d` |
| `docker compose` not found | Use `docker-compose` (hyphen) if you have the standalone v1 binary, not the v2 plugin |
| Frontend blank page | Check browser console; ensure API is running on :8080 |
| No verdicts in stream | Send a request through the proxy first: `curl -X POST http://localhost:8900/v1/messages ...` |
| Gateway panics on start | Check `.env` exists and `DATABASE_URL` is correct (or empty for in-memory mode) |
| Migrations fail | Ensure PostgreSQL is accepting connections: `docker exec controlplane-postgres pg_isready` |

---

### Full demo (end-to-end walkthrough)

See [docs/demo-runbook.md](docs/demo-runbook.md) for the narrated 5-minute demo script.

## 11. Build tiering

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

## 12. Known limitations

- The bias classifier uses a placeholder model — production requires a trained,
  validated ONNX model for each bias category.
- Auth is local JWT; production targets enterprise OIDC.
- The proxy currently buffers full responses; streaming inspection is Tier 3.
- In-memory mode loses data on restart; PostgreSQL mode persists everything.

## 13. Repository map

```text
services/              Rust workspace (12 crates)
frontend/              Next.js dashboard
infra/                 Infrastructure docker-compose + PostgreSQL migrations
scripts/               bootstrap, run_demo, seed_demo, mock_upstream (bash + PowerShell)
docs/                  architecture, demo-runbook, credentials
docker-compose.yml     Full-stack compose (all services)
Dockerfile             Rust gateway multi-stage build
frontend/Dockerfile    Next.js multi-stage build
```

## 14. Tests & quality

```bash
make verify   # fmt, clippy -D warnings, cargo test, frontend build
make test     # Rust tests only
cargo bench -p controlplane-fast-path  # latency benchmarks
```

**Test coverage**: 148 Rust tests across 12 crates (unit + integration + DB) + 77 frontend tests (Vitest).

## 15. Latency benchmarks

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

## 16. License

Apache-2.0. This repository is a hackathon prototype; all data is synthetic.

---

**Prototype / demo only.** ControlPlane.ai is an observability and governance
layer, not a replacement for human oversight. Do not use in production without
thorough validation of detection accuracy, policy thresholds, and fail-open
behaviour under your specific workload.
