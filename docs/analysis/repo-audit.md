# ControlPlane.ai — Full Repository Audit & Grand Finale Plan

> **Audit date:** 2026-09-22 · **Scope:** whole workspace (Rust crates, Next.js
> frontend, Python guardrails sidecar, infra, docs)
> **Method:** static review of source + migrations + compose + docs, cross-checked
> against `AGENTS.md` contracts. Findings are labelled with the file that proves them.
>
> **Status of this document:** analysis/recommendation. Nothing here is implemented
> unless marked done.

---

## 5-minute verdict

| Area | Grade | One-line |
|---|---|---|
| Architecture & crate boundaries | **A−** | Clean, contract-driven, fail-open baked in |
| Fast-path performance | **A** | Microsecond overhead, meets the 10ms budget with room |
| Detection depth | **B−** | Real models in 3 places; several documented checks are heuristics |
| Security & auth | **D** | Demo-only auth, default secrets, no rate limiting |
| Data integrity | **C** | Good design; audit chain has a concurrency race and no DB enforcement |
| Observability | **C−** | Structured logs + `/ready`, but no metrics/CI |
| Test suite | **B** | Large suite claimed (~390), but e2e is scaffold and CI is absent |
| Docs | **B+** | Extensive; several overclaims (now corrected in the analysis docs) |
| **Repo readiness to push** | **B−** | No `LICENSE` file, no CI, demo secrets in compose |

**Headline:** the *engineering core is genuinely strong*; the risk is concentrated in
**auth, audit atomicity, and production hygiene**. Those four P0 items below are the
difference between "impressive hackathon demo" and "credible product".

---

## 0. Severity legend

| Level | Meaning |
|---|---|
| **P0 / Critical** | Can be exploited or breaks a stated guarantee; fix before finale |
| **P1 / High** | Real correctness/operational risk; should fix before finale |
| **P2 / Medium** | Quality, consistency, maintainability; fix if time allows |
| **P3 / Low** | Polish, docs, DX |

---

## 1. P0 — Critical

### 1.1 Authentication is bypassed by design 🔴
**Files:** `services/dashboard-api/src/auth.rs`, `services/platform/src/config.rs`,
`docker-compose.yml`

`auth_middleware` *allows every unauthenticated request*:

```rust
// Demo mode: allow unauthenticated access
Ok(next.run(request).await)
```

An invalid token gets `401`, but **no token at all is accepted**. This covers the whole
dashboard API — including policy writes, escalation resolution, and audit export.

The JWT signing key also defaults to a known literal in three places:
`"dev-secret-change-me"` (`auth.rs`, `config.rs`) and
`JWT_SECRET: change-me-in-production` (`docker-compose.yml`). If pushed as-is, anyone can
mint an `admin` token.

**Fix (pick one, do it before the finale):**
1. Add `AUTH_MODE=demo|enforce`. In `enforce`, reject anonymous requests and require a
   valid token; set a `--fail-closed` default for any non-local bind.
2. Refuse to start when `JWT_SECRET` is unset/default and `AUTH_MODE=enforce`.
3. Keep the demo path for the live demo, but make it an explicit flag — never the default
   for a pushed repo.

**Effort:** ~half a day.

### 1.2 The audit hash chain can fork under concurrency 🔴
**File:** `services/audit/src/repository.rs` (`append`, `get_latest_hash`)

```rust
let prev_hash = self.get_latest_hash().await?;   // read
...
INSERT INTO audit_records (..., prev_hash, record_hash, ...)  // write
```

Two decisions processed concurrently can both read the same `prev_hash` and append
records with the same parent — a **forked chain**. Verification then reports tampering
even though nobody tampered. The read-then-write is not transactional and not serialized.
`get_latest_hash` also uses `ORDER BY created_at DESC`, which is ambiguous when two rows
share a timestamp (should order by the UUIDv7 `id`).

**Fix:**
- Take a transaction + advisory lock (`pg_advisory_xact_lock`) around read+insert, **or**
  make the audit writer single-threaded (one consumer task).
- Order by `created_at DESC, id DESC`.
- Consider `record_hash` uniqueness constraint to make forks detectable at write time.

### 1.3 "Append-only" audit is convention-only 🔴
**Files:** `infra/migrations/006_create_audit_records.sql`, `AGENTS.md`

`AGENTS.md` says "no service may DELETE or UPDATE audit records", but **nothing in the
database enforces it**. A bug or a rogue SQL statement can rewrite history.

**Fix:** add a migration with
```sql
CREATE OR REPLACE FUNCTION audit_records_immutable() RETURNS trigger AS $$
BEGIN RAISE EXCEPTION 'audit_records is append-only'; END; $$ LANGUAGE plpgsql;
CREATE TRIGGER audit_records_no_update_delete
  BEFORE UPDATE OR DELETE ON audit_records
  FOR EACH ROW EXECUTE FUNCTION audit_records_immutable();
```
and `REVOKE UPDATE, DELETE ON audit_records FROM app_user;`.

### 1.4 No timeout on upstream LLM calls 🔴
**File:** `services/gateway/src/main.rs` — `http_client: reqwest::Client::new()`

A default `reqwest::Client` has **no request timeout**. A hung Ollama/provider connection
will hang the proxy request indefinitely, holding connections and eventually the whole
proxy. (The guardrails client *does* set a 5s timeout — this one doesn't.)

**Fix:** `.timeout(Duration::from_secs(60))` + `connect_timeout`, configurable via env.

---

## 2. P1 — High

### 2.1 The proxy has no rate limiting (dead code) 🟠
**Files:** `services/proxy/src/rate_limiter.rs` (defined), `services/proxy/src/router.rs`

A fully implemented, tested `RateLimiter` exists and is re-exported from `proxy::lib`,
but **it is never constructed or called** — `ProxyState` has no rate-limiter field and
`proxy_router` only mounts `proxy_handle`. There is no DoS protection on the ingress path.

**Fix:** add `rate_limiter: Arc<RateLimiter>` to `ProxyState`, call `check(app_id_or_ip)`
at the top of `proxy_handler`, return `429`, and spawn a periodic `cleanup()`.

### 2.2 The hallucination check does not run as documented 🟠
**Files:** `services/guardrails/main.py`, `services/guardrails/requirements.txt`

DeepEval's `HallucinationMetric` needs an LLM judge; **no judge key/model is configured**,
so it raises and silently falls back to a word-overlap heuristic — and it only fires when
RAG context is present. Details in `docs/analysis/checks-inventory.md §2.3`.

**Fix:** either configure a judge (local model or API key) to make it real, **or** relabel
it honestly as a heuristic in the UI/docs. Also expose a `/health` field showing whether
the judge is live so the dashboard can't imply more than is running.

### 2.3 API keys are decorative — never enforced 🟠
**Files:** `services/dashboard-api/src/router.rs` (CRUD), `services/proxy/src/handler.rs`

There is an API-key table, SHA-256 hashing, revoke, and per-key analytics — but the
**proxy never validates a client API key**. Requests are attributed by `app_id` only, so
`api_key_analytics` will show near-empty data and the keys provide no access control.

**Fix:** hash the incoming `Authorization`/`X-Api-Key`, look up `api_keys` (status active),
bind `api_key_id` onto `intercepted_calls`, and reject unknown keys when `AUTH_MODE=enforce`.

### 2.4 Duplicated cost/pricing logic (drift risk) 🟠
**Files:** `services/dashboard-api/src/router.rs` (`model_price_per_million_tokens`) vs
`services/cost-accounting/src/pricing.rs`

Two independent price tables now exist. They will drift, and the dashboard's math sums
input+output tokens against a single per-model rate (ignoring separate input/output
pricing). Cost numbers on `/cost` can disagree with the ledger service.

**Fix:** one pricing source (move to `common` or expose via the cost service), used by both.

### 2.5 Dashboard stats use a statistical fudge 🟠
**File:** `services/dashboard-api/src/router.rs` (`stats_overview`)

To avoid double-counting verdicts that exist in **both** in-memory store and DB, the code
scales in-memory counts by a proportional `overlap_ratio`. That's an estimate applied to
an exact quantity — counts can be wrong, which is bad for a compliance dashboard.

**Fix:** make the DB the single source of truth for persisted verdicts; use the in-memory
store only for sub-second live tails. Or key the in-memory store by verdict id and diff
exactly (ids are already available).

### 2.6 CORS falls back to `Any` origin 🟠
**File:** `services/dashboard-api/src/router.rs`

`DASHBOARD_CORS_ORIGINS` restricts origins when set, but the **default is `Any`** — so an
unconfigured deployment is world-readable/writable from any browser origin.

**Fix:** default to `http://localhost:3000` and require explicit `*` to opt out.

### 2.7 Fast-path budget exceeds the stated contract 🟠
**File:** `services/fast-path/src/engine.rs` (`FAST_PATH_BUDGET_MS = 50`),
`services/proxy/src/handler.rs` (50ms `tokio::time::timeout`)

`AGENTS.md` and the README promise **<10ms p50 / <25ms p99 (hard)**. Both the internal
budget and the outer timeout are **50ms**, and a test asserts `<50ms`. The safety margin is
larger than the contract, which blurs the guarantee.

**Fix:** set the internal budget to ~10ms and the outer timeout to ~25ms, and update the
test to assert the documented bound.

### 2.8 No CI, and no `LICENSE` file 🟠
**Files:** repo root

- **No `.github/workflows`** — `cargo test`, `npx vitest`, `clippy`, and `fmt` are never
  run automatically. Nothing prevents a broken push.
- **No `LICENSE`** despite the README badge claiming **Apache-2.0** and `Cargo.toml`
  declaring `license = "Apache-2.0"`. A pushed repo without a license file is
  legally ambiguous.

**Fix:** add `LICENSE` (Apache-2.0 full text) and a CI workflow that runs
`cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --workspace`, and
`cd frontend && npx vitest run`.

---

## 3. P2 — Medium

| # | Finding | File | Suggested fix |
|---|---|---|---|
| 3.1 | Every email (0.80) and separated phone number (0.75) is flagged and redacted — high EDIT noise | `fast-path/checks/secret_detection.rs` | Raise thresholds or make categories configurable per app |
| 3.2 | `dead code`: `proxy::Interceptor` unused | `proxy/src/interceptor.rs`, `proxy/src/lib.rs` | Delete or wire it |
| 3.3 | `get_system_config` returns the raw PostgreSQL `version()` string and env internals | `dashboard-api/src/router.rs` | Return a coarse version or gate behind admin |
| 3.4 | Webhook uses a plaintext shared-secret header, no HMAC signature, no retry/backoff | `notification/src/webhook.rs` | Sign the body (HMAC-SHA256) + exponential backoff |
| 3.5 | Request body limited to 10MB; no configurable max; large `limit` (up to 10 000) responses | `proxy/src/handler.rs`, `dashboard-api/src/router.rs` | Make limits configurable; page large queries |
| 3.6 | Two aggregation paths (`aggregate` vs `aggregate_with_reasoning`) can diverge | `decision/src/aggregator.rs`, `decision/src/router.rs` | Make `aggregate` delegate to the reasoning version |
| 3.7 | In-memory fallback mode mixes with DB silently ("zero mock data" claim) | `dashboard-api`, `gateway` | Surface "degraded / no DB" state in the UI |
| 3.8 | Python deps unpinned (`laya`-style ranges) → non-reproducible builds | `services/guardrails/requirements.txt` | Pin versions / add `pip-compile` lock |
| 3.9 | `.env` in compose commits `POSTGRES_PASSWORD: secret` and a JWT secret default | `docker-compose.yml` | Mark clearly as demo-only; require overrides for deploy |
| 3.10 | 287 `unwrap()/expect()` in Rust source (concentrated in tests, but some in handlers) | `services/**` | Audit handler paths; prefer `?` + typed errors |
| 3.11 | No Prometheus metrics despite the architecture doc implying observability | `platform`, `dashboard-api` | Add counters/histograms (`/metrics`) |

---

## 4. P3 — Low / DX / polish

| # | Finding | Fix |
|---|---|---|
| 4.1 | Docs were flat and had stale claims | **Done:** split into `docs/original/` + `docs/analysis/`, corrected claims |
| 4.2 | `scripts/demo_showcase.sh` / `preflight.sh` referenced but absent | Now referenced honestly in docs; consider adding a real seeder |
| 4.3 | `frontend/e2e/` is SCAFFOLD (plan only) | Implement a Playwright smoke flow (login → stream → resolve) |
| 4.4 | No `rustfmt.toml` / `clippy.toml` | Add for consistent style |
| 4.5 | Touch target: `README.md` is 1 300+ lines | Split quickstart vs reference, or move to `docs/original/` |
| 4.6 | Test counts (390) unverified in this audit | Run the suites and record exact numbers |

---

## 5. What is already strong (protect these)

- **Crate boundaries match `AGENTS.md`** — one concern per crate, event-driven, no shared
  mutable state. This is genuinely well-disciplined for a hackathon.
- **Fail-open is real** — panic/timeout catch in `run_fast_path_safe`; shadow absence = pass.
- **Fast path is genuinely fast** — microsecond measured overhead vs a 10ms budget.
- **SQL is parameterized** — a full pass found **no string-interpolated user input** in
  queries (only bind-index construction), so no SQL injection surface.
- **Structured logging** with `correlation_id` threaded through every service and event.
- **Graceful shutdown** via a `watch` channel across all workers.
- **Real models where it counts** — Microsoft Presidio + spaCy for PII, HuggingFace
  classifiers for toxicity/bias.
- **Clean git hygiene** — `.env` is gitignored, no secrets committed.

---

## 6. Grand finale roadmap

### Phase 0 — "Pushable" (before you hand the repo over)
1. Add `LICENSE` (Apache-2.0) — matches `Cargo.toml` and README badge.
2. Add CI: `fmt --check` · `clippy -D warnings` · `cargo test --workspace` · `vitest run`.
3. Move demo secrets behind explicit flags; add `.env.example` entries for `AUTH_MODE`.

### Phase 1 — "Defensible" (security & integrity)
4. `AUTH_MODE=enforce` path + refuse-to-start on default `JWT_SECRET` (1.1).
5. Serialize audit appends + immutable trigger (1.2, 1.3).
6. Upstream HTTP timeout (1.4).
7. Wire proxy rate limiting (2.1).
8. CORS default to localhost (2.6).

### Phase 2 — "Credible" (accuracy & consistency)
9. Make the hallucination judge real, or relabel the check (2.2).
10. Enforce API keys on the proxy path (2.3).
11. Unify pricing into one source (2.4).
12. Replace the stats overlap fudge with exact DB counts (2.5).
13. Tighten fast-path budget to the documented 10/25ms (2.7).

### Phase 3 — "Impressive" (elevate the demo)
14. Add a `/metrics` endpoint + dashboard health badges (judge live? DB connected?).
15. Implement the Playwright e2e smoke flow (4.3).
16. Build a real seed script so the demo never depends on missing scripts (4.2).
17. If time allows, prototype the Jev/Laya shadow judge
    (`docs/analysis/jev-laya-integration.md`) — it directly upgrades 2.2.

---

## 7. The 10 highest-leverage moves

| # | Move | Why it matters most | Effort |
|---|---|---|---|
| 1 | Enforce auth behind a flag | Closes the one D-grade | ½ day |
| 2 | Serialize the audit chain | Protects the core guarantee | ½ day |
| 3 | Add immutability trigger | Makes "append-only" true | 1 hr |
| 4 | Upstream timeout | Prevents a full proxy hang | 15 min |
| 5 | Wire rate limiting | Uses code you already wrote | 2 hr |
| 6 | Add `LICENSE` + CI | Table stakes for a public repo | 1 hr |
| 7 | Make/relabel hallucination | Removes the biggest demo overclaim | 2 hr |
| 8 | Enforce API keys | Makes a whole feature real | ½ day |
| 9 | Unify pricing | One source of truth for cost | 2 hr |
| 10 | Exact dashboard counts | Trustworthy compliance numbers | ½ day |

---

## 8. Verification commands

```bash
# Rust
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Frontend
cd frontend && npx vitest run && npx tsc --noEmit

# Guardrails sidecar
cd services/guardrails && python -m compileall . && curl -s localhost:8200/health

# End-to-end smoke (manual today)
curl -s localhost:8080/ready | jq
curl -s -X POST localhost:8900/v1/messages -H 'content-type: application/json' \
  -d '{"model":"qwen2.5:1.5b","messages":[{"role":"user","content":"What is 2+2?"}]}'
```

---

## 9. Metric snapshot (at audit time)

| Metric | Value |
|---|---|
| Rust workspace crates | 12 (+1 Python sidecar) |
| REST endpoints | 33 |
| Governance checks (product / emitted / toggles) | 14 / 15 / 10 |
| Rust `unwrap()/expect()` occurrences | 287 |
| `TODO`/`FIXME` in source | 0 |
| CI workflows | 0 |
| `LICENSE` file | **absent** |
| Tracked docs before reorg | 3 of 9 (6 were uncommitted) |
