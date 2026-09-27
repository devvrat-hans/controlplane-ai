# The Open-Source Stack — What We Use and Why

> **What this document is:** every open-source component in ControlPlane.ai, what it does for
> us, the key benefit, and the alternative we deliberately did not choose.
>
> **Why it matters:** the project is 100% open-source and runs locally with no API keys. This
> is the list to defend when asked *"why this library and not that one?"*
>
> **Verified against:** `Cargo.toml`, `services/guardrails/requirements.txt`,
> `frontend/package.json`, `docker-compose.yml`. Licences are all permissive
> (MIT / Apache-2.0 / BSD / PostgreSQL) with no copyleft — confirm exact terms before making
> a formal legal claim.

**Legend:** *Why this* = the benefit we actually rely on. *Not* = the alternative we passed on.

---

## 1. Rust — runtime and web layer

| Component | What it is | Why this | Not |
|---|---|---|---|
| **tokio** | Async runtime | The de-facto standard; multi-threaded work-stealing (real parallelism for our CPU-bound checks), and every other crate we use assumes it | `async-std` / `smol` — less ecosystem momentum, fewer battle-tested integrations |
| **axum** | HTTP framework | From the tokio team; typed extractors, tower middleware, minimal magic — routing reads as plain Rust | `actix-web` (own actor runtime, more machinery), `rocket` (macro-heavy), `warp` (filter combinators hurt readability) |
| **hyper** | HTTP/1 + HTTP/2 primitives | Battle-tested transport under axum; full control over the proxy path | Writing on raw sockets — unnecessary risk |
| **tower / tower-http** | Middleware | CORS, tracing, request-id, body limit and timeout as composable layers | Hand-rolling middleware per service |
| **reqwest** | HTTP client | Async, `rustls` TLS (no OpenSSL build pain), used for upstream LLM calls | `hyper` directly (more boilerplate), `ureq` (blocking) |
| **async-nats** | NATS client | Lets the same event contracts run in-process for the demo and over NATS in production | Hand-written bus abstraction per deployment mode |

---

## 2. Data and persistence

| Component | What it is | Why this | Not |
|---|---|---|---|
| **PostgreSQL 16** | System of record | ACID guarantees for the audit ledger, JSONB for flexible policy config, and `pg_trgm` — one engine covers relational, document and fuzzy-match needs | MySQL (weaker JSON/extension story), MongoDB (no hash-chain-grade transactional guarantees we want) |
| **sqlx** | Async SQL toolkit | Async, **no ORM, no DSL** — the SQL in the code is the SQL that runs, which matters for an auditable system. Parameterised binds throughout (183 call sites) | `diesel` (sync + heavy DSL), `sea-orm` (an abstraction layer over the queries we want to read literally) |
| **pg_trgm** | Trigram similarity extension | Powers the reviewer-override precedent matching (60% similarity) **without a vector database** — zero extra infrastructure | Pinecone / Weaviate / FAISS — real cost and a second service for a problem Postgres already solves at our scale |
| **JSONB policies** | Policy-as-code storage | Per-app, versioned policy configs that can evolve without migrations | A rigid `policies_v2` schema churn |

---

## 3. Correctness and domain primitives

| Component | What it is | Why this | Not |
|---|---|---|---|
| **serde / serde_json** | Serialization | One `EventEnvelope<T>` contract serialises identically in every crate — the whole event bus depends on it | Hand-rolled JSON (drift between services) |
| **uuid v7** | Identifiers | **Time-ordered** — sortable and index-friendly, unlike v4, which is why the audit chain and keyset pagination work well | v4 (random, poor index locality), auto-increment integers (leaks volume, painful to shard) |
| **chrono** | Date/time | RFC3339 timestamps feed directly into the audit hash | `std::time` alone (no timezone/format ergonomics) |
| **thiserror / anyhow** | Error types | Typed error enums with `Display` (thiserror) in libraries; ergonomic propagation (anyhow) at the binary edge | Unstructured string errors, or panicking on recoverable conditions |
| **regex** | Pattern matching | Fast, linear-time regex engine with no catastrophic backtracking — critical when matching untrusted model output | Naive substring scans (the current fallback), or PCRE-style engines with backtracking blowups |
| **sha2 + hex** | Hashing | SHA-256 for the audit hash chain, with deterministic unit tests proving chain integrity | A non-standard hash, or a database default that hides the algorithm |
| **jsonwebtoken** | JWT (HS256) | Standard, well-audited token handling for the dashboard API | A hand-rolled token format |
| **arc-swap** | Lock-free atomic swap | **Readers never block** — policy hot-reload during peak traffic costs the request path nothing | `RwLock` (writer contention and priority inversion on the hot path) |

---

## 4. Observability

| Component | What it is | Why this | Not |
|---|---|---|---|
| **tracing / tracing-subscriber** | Structured logging | Spans carry `correlation_id` through every service, so one intercepted call can be followed end-to-end; JSON output available for log pipelines | `log` + `env_logger` (no span context), or unstructured `println!` |
| **criterion** | Benchmark harness | Turns the 10 ms budget into a **regression gate** in CI rather than a promise in a README | Ad-hoc timing loops (noisy, no statistics) |

---
## 5. AI and detection — the Python guardrails sidecar

These run **asynchronously in the shadow path**, off the request path.

| Component | What it is | Why this | Not |
|---|---|---|---|
| **FastAPI + uvicorn** | Service framework | Async, tiny, and **pydantic-validated** request/response models — the sidecar is a handful of typed endpoints | Flask (sync), Django (far too much for four endpoints) |
| **pydantic** | Schema validation | Every scan request is validated before it reaches a model; bad input is a 422, not a crash | Hand-parsed JSON |
| **Microsoft Presidio** (analyzer + anonymizer) | PII detection and redaction | Purpose-built for this: recognises SSNs, cards, IBANs, emails, phones across many locales, returns **entity type + character offsets + confidence**, and ships an anonymizer. Local, no data egress | Cloud DLP APIs (AWS Comprehend / Google DLP — the data leaves the network), hand-written regex (no NER), spaCy alone (no PII rule packs), an LLM (cannot return reliable spans and can hallucinate) |
| **spaCy** (`en_core_web_lg`) | NLP / NER backbone | The model Presidio's NER recognizers run on; mature and fast | Training our own NER — months of work for a worse model |
| **HuggingFace transformers** | Model runtime | One interface to run downloaded classifier weights locally, cached at image build time | Cloud moderation APIs (data egress, rate limits, vendor lock-in) |
| **`unitary/unbiased-toxic-roberta`** | Toxicity classifier | **Multi-label** (toxic, severe_toxic, obscene, threat, insult, identity_hate) and trained on Jigsaw — a genuinely well-validated model, which is why we keep it as the primary toxicity engine | Perspective API (cloud, ToS limits), OpenAI moderation (cloud, no local story), a general LLM (cost, latency, uncalibrated confidence) |
| **`valurank/distilroberta-bias`** | Bias classifier | Small, fast, purpose-trained for the bias label. We deliberately run it on **inputs only** — response-side bias produced too many false positives | A general LLM judge for bias (expensive, and uncalibrated) |
| **DeepEval** | Hallucination metric (LLM-as-a-judge) | Open-source, brings a standard hallucination metric with RAG context | Writing our own judge harness. *Honest caveat:* it needs a configured judge model; without one it falls back to a word-overlap heuristic — which is exactly why the Laya plan exists |
| **PyTorch (CPU-only wheel)** | Model runtime | Installed from the CPU index first to avoid a 2 GB+ CUDA download — keeps the sidecar image practical | Default CUDA wheel (huge image for a demo that runs on CPU) |

---

## 6. LLM runtime

| Component | What it is | Why this | Not |
|---|---|---|---|
| **Ollama** | Local model server | OpenAI-compatible API, **no API key, no cost, nothing leaves the machine** — which is the whole local-first story | Hosted APIs (keys, cost, data residency) |
| **qwen2.5:1.5b** | The model | Small enough to run on a laptop CPU inside Docker while still producing realistic responses for the demo | A larger model (slow cold start on CPU), or a tiny one (unrealistic output) |
| **Provider abstraction** | `common/src/provider*.rs` | Ollama, Anthropic, Gemini and OpenCode behind one trait — the proxy is provider-agnostic | Hard-coding one vendor |

---
## 7. Frontend — the dashboard

| Component | What it is | Why this | Not |
|---|---|---|---|
| **Next.js 16 + React 19 + TypeScript** | Dashboard framework | Server components, file-based routing, and types across the API boundary; ships the SSE live-stream page cleanly | Plain React + Vite (no routing/server story), a templating dashboard (no interactivity) |
| **Tailwind CSS v4** | Styling | Utility-first keeps 15 pages visually consistent without a bespoke CSS codebase | Hand-written CSS, or a heavy component framework |
| **shadcn/ui + Base UI** | Component primitives | Copy-in components we own and can restyle, on accessible Base UI primitives — no runtime dependency lock-in | MUI / Ant Design (opinionated, heavy theming fights) |
| **CVA + clsx + tailwind-merge** | Variant + class utilities | Typed component variants without class-name collisions | String-concatenated class names |
| **TanStack Query** | Server-state management | Caching, refetch and loading states for every REST call, so pages have real data states (and no mock data) | `useEffect` + `fetch` everywhere (manual cache invalidation bugs) |
| **Recharts** | Charts | Composable React charts for verdict distribution, trends and cost timeseries | Hand-rolled SVG, or a heavyweight charting bundle |
| **lucide-react** | Icons | Consistent, tree-shakeable icon set | Mixed icon sources |
| **Vitest + Testing Library + jsdom** | Tests | Fast, Vite-native, React Testing Library ergonomics for behaviour tests | Jest (slower, more config) |
| **Biome + ESLint** | Lint / format | Biome is a single fast tool; ESLint remains for Next.js-specific rules | Prettier + ESLint + plugins (more moving parts) |
| **pnpm** | Package manager | Fast, disk-efficient, strict about phantom dependencies | npm (slower, looser resolution) |

---

## 8. Infrastructure and developer experience

| Component | What it is | Why this | Not |
|---|---|---|---|
| **Docker + Docker Compose** | Packaging and orchestration | One command brings up Postgres, Ollama, the gateway, guardrails and the frontend — the workspace is the unit of deployment | Kubernetes (far too much for a single-workspace build) |
| **Colima** | Docker runtime on macOS | A lightweight, scriptable VM backend | Docker Desktop (licensing and heavier footprint) |
| **dotenvy** | `.env` loading | Matches the twelve-factor config story; `.env` is gitignored | Hard-coded config |
| **GitHub Actions (planned)** | CI | `fmt`, `clippy -D warnings`, `cargo test`, `vitest` as the verification gate — currently **absent**, tracked in `repo-audit.md` | Manual "it works on my machine" |

---

## 9. What we deliberately did **not** use

Just as important as the choices above — these were considered and rejected.

| Rejected | Why |
|---|---|
| **LangChain / LlamaIndex** | Heavy abstraction over the exact control flow we need to be explicit and auditable. Our decision path must be deterministic and readable |
| **A vector database** (Pinecone, Weaviate, FAISS) | `pg_trgm` solves precedent matching at our scale with zero extra services. A vector DB belongs in the *target* state if semantic recall becomes the bottleneck |
| **Kafka / RabbitMQ** | NATS is lighter and sufficient; the in-process bus runs the same contracts with no broker for the demo |
| **Redis** | Postgres plus in-memory counters (retry window, session risk) cover the needs. One less service to run |
| **An ORM** | Explicit SQL is auditable; the SQL in the code is the SQL that runs |
| **gRPC between services** | JSON over the event bus keeps contracts readable, debuggable and language-agnostic (Rust ↔ Python) |
| **Cloud moderation / DLP APIs** | The product's differentiator is local-first: no API keys, no data leaving the network |
| **Managed auth (Auth0/Clerk)** | Demo-mode JWT is enough; production needs SSO — tracked honestly in `repo-audit.md` |

---

## 10. Why open source matters here

1. **Runs entirely on-premise.** A governance layer sees every prompt and response. Sending that to a third-party moderation API would defeat the purpose — and break the data-residency story for regulated apps.
2. **No API keys, no per-call cost.** The whole stack launches locally with one command.
3. **Auditable behaviour.** Presidio's recognizers, toxic-roberta's labels and our own thresholds are all inspectable — you can explain *why* a verdict was produced, which the hash-chained audit trail then records.
4. **Permissive licensing.** Everything is MIT / Apache-2.0 / BSD / PostgreSQL — no copyleft, safe for enterprise distribution. *(Verify exact terms before any formal legal claim.)*
5. **Swappable, not locked in.** Every engine sits behind a small interface: providers behind one trait, PII behind a sidecar endpoint, the shadow panel behind `ShadowVerdict`. That is what makes the Laya addition a plan and not a rewrite.

---

## 11. Sources

- Rust dependency list and features: `Cargo.toml`
- Python sidecar: `services/guardrails/requirements.txt`, `services/guardrails/Dockerfile`
- Frontend dependencies: `frontend/package.json`
- Service topology: `docker-compose.yml`, `services/gateway/src/main.rs`
- Honest status of each detection engine: `docs/analysis/checks-inventory.md`
- The Laya addition to this stack: `docs/analysis/laya-integration-plan.md`
