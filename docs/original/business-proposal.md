# ControlPlane.ai — Business Proposal

## Problem

Enterprise teams deploying LLM-powered applications face a governance vacuum:

1. **No real-time guardrails**: Content moderation happens after the fact, not in the request path. By the time a policy violation is caught, the harmful response has already reached the user.

2. **No audit trail**: Regulatory requirements (SOC 2, GDPR, EU AI Act) demand explainable AI decisions. Most teams have no record of *why* a particular output was allowed or blocked.

3. **No feedback loop**: When a human reviewer overrides a model decision, that knowledge is lost. The same mistake repeats because there's no mechanism to learn from corrections.

4. **No cost visibility**: LLM token spend is opaque. Teams can't attribute costs to specific applications or detect anomalous usage patterns.

5. **No conversation-level governance**: Individual turns may seem benign, but multi-turn conversations and AI agents that chain actions introduce compounding risk that single-request checks miss entirely.

6. **No regulatory adaptability**: GDPR, EU AI Act, CCPA, HIPAA — each demands different thresholds. Teams can't easily switch governance postures without rebuilding their policy stack.

## Who We Serve

| Segment | Pain Point | Value Prop |
|---------|-----------|------------|
| **AI Platform Teams** | Need governance layer across multiple LLM apps | Single pane of glass for all AI governance, per-app policies |
| **Compliance Officers** | Must demonstrate AI decision auditability | Hash-chained audit trail with every decision, regulatory profiles |
| **ML Engineers** | Want to iterate on prompts without governance friction | Policy-as-code with per-app granularity, feedback-informed thresholds |
| **Product Managers** | Need to balance safety with user experience | Tiered responses (allow/edit/flag/block), trust score trending |
| **Security Teams** | Need to detect adversarial probing and tool-use risks | Session-level risk tracking, prompt injection detection, tool-use monitoring |

## Solution

ControlPlane.ai is an **AI governance proxy** that sits between your application and LLM providers, providing:

### Core Capabilities

1. **Reverse Proxy with Governance**: All LLM API calls flow through ControlPlane. The fast-path engine (<10ms) applies deterministic checks: PII detection, unsafe content blocking, secret redaction, cost cap enforcement, retry-loop detection, tool-use risk, and session-risk accumulation.

2. **Parallel Shadow Analysis**: Expensive checks (hallucination detection, bias classification, groundedness scoring, verbosity analysis) run asynchronously after the response is delivered. No impact on user latency.

3. **Multi-Turn Session Governance**: Conversations are tracked across turns via `session_id`. A conversation risk accumulator detects adversarial patterns that emerge over multiple exchanges — catching probing attacks that stay just below per-turn thresholds.

4. **Agent/Tool-Use Risk Detection**: When model responses contain action directives (`function_call`, `tool_use`), stricter thresholds apply automatically. A 1.5x confidence multiplier ensures actions receive proportionally more scrutiny than pure text outputs.

5. **Compound Risk Detection**: When multiple axes fire simultaneously (e.g., hallucination + privacy), the system escalates even if individual confidences are below threshold — catching overlapping risks that single-axis systems miss.

6. **Reviewer Override Learning (RAG Feedback Loop)**: When a human reviewer overrides a model decision, the full context is stored. Future similar cases are matched via trigram similarity (pg_trgm). If a previously dismissed pattern recurs, it's automatically suppressed — reducing alert fatigue and demonstrating measurable system improvement over time.

7. **Regulatory Profiles**: Pre-configured governance postures (US-Financial, EU-Financial, US-Healthcare, India-General, EU-General, Global-Internal) with per-axis thresholds. Profiles are independently editable and applied to apps with a single API call. Switching regulatory posture takes seconds, not sprints.

8. **Policy-as-Code**: Per-application, per-axis policies with versioning. 14 governance checks across 3 axes (Responsibility, Performance, Cost) — each configurable independently.

9. **Hash-Chained Audit Trail**: Every decision is cryptographically linked to the previous one (SHA-256). Tampering is detectable. Verification is one API call. Supports export to CSV/JSON for compliance reporting.

10. **Detection Quality Metrics**: Trust score tracking, false positive/negative rate monitoring, precision by axis, and 7-day trending — demonstrating that the system improves over time as reviewers provide feedback.

### Architecture (per AGENTS.md)

- **No LLM in the decision path**: Fast-path is pure Rust/deterministic. Judge model only in shadow path.
- **Fail-open**: Governance never blocks the user on its own errors.
- **Single responsibility**: Each service owns one concern. Events flow through NATS/in-process bus.
- **Per-app isolation**: Each application gets its own policy configuration and risk profile.

## Solutioning Areas — Mapping to Round 2 Brief

The problem statement asks teams to explore solutioning areas selectively.
Below is how ControlPlane.ai addresses each area.

### 1. Detection Techniques

| Technique | Where Used | Latency |
|---|---|---|
| Regex + entropy scoring | Secret/API key detection (fast-path) | <1ms |
| Pattern matching (25+ patterns) | Prompt injection — 3-layer: patterns, structural heuristics, encoding detection | <2s (shadow) |
| Keyword matching | Unsafe content detection (25+ keyword patterns) | <1ms |
| NLI model | Groundedness scoring — compares response claims against prompt context | <2s (shadow) |
| LLM-as-judge (DeepEval) | Hallucination detection — cross-references factual claims | <2s (shadow) |
| NER-based PII | Semantic PII detection on responses (NER model) | <2s (shadow) |
| Microsoft Presidio | PII detection on inputs (guardrails sidecar) | <2s (shadow) |
| LLM Guard | Toxicity + bias classification (guardrails sidecar) | <2s (shadow) |
| Sliding window | Retry/loop detection — in-memory, session-aware | <1ms |
| Confidence scoring | Every check returns 0.0–1.0 confidence, not binary | — |

### 2. Decision Logic

**Tiered responses** — not binary pass/fail:

| Token Usage vs Cap | Outcome | Description |
|---|---|---|
| >100% | **Block** | Exceeds allowed token budget |
| 90–100% | **Escalate** | Near cap, flagged for human review |
| 75–90% | **Edit** | Approaching limit, response may be trimmed |
| <75% | **Pass** | Within budget, no action taken |

**Confidence-based escalation**: Each check returns a confidence score. The decision
engine aggregates fast-path and shadow-path verdicts, applying per-app thresholds.
Low-confidence verdicts are escalated for human judgment rather than auto-decided.

**Compound risk**: When 2+ axes fire simultaneously (even if individual confidence
is below threshold), the system upgrades the outcome to ESCALATE.

### 3. Architecture

**Where the checker sits**: Inline middleware (reverse proxy). All LLM API calls
flow through ControlPlane before reaching the model provider.

**Parallel checks**: Fast-path runs synchronously (<10ms, in-process). Shadow-path
runs asynchronously (<2s, via NATS/in-process bus). No impact on user latency.

**Fail-open**: If ControlPlane errors, traffic passes through unmodified.
A governance layer that causes outages is worse than ungoverned traffic.

**No LLM in the decision path**: Fast-path is pure deterministic Rust. Judge model
only runs in the shadow path for non-blocking analysis.

### 4. Governance

**Policy-as-code**: Per-application, per-axis policies with versioning. 14 governance
checks across 3 axes — each configurable independently from the `/policies` page.

**Per-app isolation**: A customer-facing chatbot gets strict controls. An internal
tool gets lighter ones. Each app has its own policy configuration.

**Regulatory profiles**: 6 pre-built governance postures (EU-Financial, US-Healthcare,
India-General, EU-General, Global-Internal). Profiles are independently
editable and applied to apps with a single API call.

**Audit trail**: SHA-256 hash-chained append-only ledger. Every decision is
cryptographically linked. Tampering is detectable. Verification is one API call.

### 5. Feedback Loops

**Reviewer Override RAG Learning Loop**:

1. Reviewer resolves escalation (confirm / override / dismiss) with a text reason
2. Full context stored: question, answer, escalation reason, resolution reason
3. Future similar requests checked via `pg_trgm` trigram similarity
4. ≥60% similar to a previously dismissed case → auto-suppressed
5. Verdict annotated: `[Learned] ⚠ 82%-similar past case was dismissed`

**Two-layer suppression**:
- **Escalation layer**: Skips case creation if request matches dismissed precedent
- **Decision layer**: Downgrades escalate/edit → pass if response matches dismissed precedent

**Measurable improvement**: FP rate tracked over 7/30 days. Visible decrease as
more cases are resolved. Metrics on Overview page.

### 6. Metrics & Monitoring

**Detection quality** (`GET /api/v1/metrics/detection-quality`):
- Trust score (0.0–1.0)
- Total flags, confirmed, overridden, dismissed
- Precision by axis (responsibility, performance, cost)
- False positive rate

**Feedback effectiveness** (`GET /api/v1/metrics/feedback-effectiveness`):
- Pattern promotions (shadow → fast-path)
- Resolution distribution (confirm/override/dismiss)
- FP rate trend over time

**Cost tracking** (`GET /api/v1/cost/summary`):
- Per-model token costs
- Hourly timeseries
- Anomaly detection

**Latency sparkline**: Hourly avg + p99 for fast-path overhead, visible on Overview.

---

## Differentiation

### What makes ControlPlane.ai unique:

1. **Conversation-level governance** — Not just per-request. Multi-turn risk accumulation catches adversarial patterns that single-request systems miss.

2. **Active feedback loop** — The system demonstrably gets better over time. Alert fatigue decreases as reviewer decisions inform future suppressions.

3. **14 governance checks, 2 paths** — 6 synchronous fast-path checks (<10ms) and 8 asynchronous shadow checks. No other solution offers this depth without latency impact.

4. **Per-app, per-axis policy granularity** — A customer support bot and an internal code agent shouldn't have the same governance thresholds. ControlPlane.ai handles this natively.

5. **Regulatory profile switching** — Change from GDPR-strict to startup-relaxed with one API call. Profiles are editable independently of apps.

## Competitive Landscape

| Feature | ControlPlane.ai | Guardrails AI | Lakera | Rebuff | Arthur AI |
|---------|----------------|---------------|--------|--------|-----------|
| Real-time proxy | ✅ | ❌ (library) | ✅ | ❌ | ✅ |
| Shadow analysis | ✅ | ❌ | ❌ | ❌ | ❌ |
| Multi-turn tracking | ✅ | ❌ | ❌ | ❌ | ❌ |
| Feedback learning (RAG) | ✅ | ❌ | ❌ | ❌ | ❌ |
| Agent/tool-use detection | ✅ | ❌ | ❌ | ❌ | ❌ |
| Hash-chained audit trail | ✅ | ❌ | ❌ | ❌ | ❌ |
| Regulatory profiles | ✅ | ✅ | ✅ | ❌ | ✅ |
| Policy-as-code | ✅ | ✅ | ✅ | ❌ | ✅ |
| Per-app policy isolation | ✅ | ❌ | ✅ | ❌ | ✅ |
| Detection quality metrics | ✅ | ❌ | ❌ | ❌ | ✅ |
| Compound risk detection | ✅ | ❌ | ❌ | ❌ | ❌ |
| Multi-provider | ✅ (4+ providers) | ✅ | ✅ | ✅ | ✅ |
| <10ms fast-path | ✅ | N/A | ❌ (~50ms) | N/A | ❌ (~100ms) |

## Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| LLM providers add native guardrails | Reduced differentiation | Focus on cross-provider governance + audit trail + feedback loop + multi-turn tracking — things providers won't build |
| Enterprise security concerns (proxy in request path) | Slow adoption | Open-source core, self-hosted option, SOC 2 certification, fail-open guarantee |
| Performance overhead too high | User experience degradation | Fast-path <10ms budget (proven), shadow path non-blocking, load test benchmarks prove <1% overhead |
| Regulatory landscape changes | Feature misalignment | Modular policy engine, regulatory profile system, profiles editable without code changes |
| Alert fatigue causes reviewer disengagement | Governance quality drops | Active feedback loop suppresses recurring false positives; priority scoring surfaces high-value cases |
| Multi-turn adversarial attacks | Undetected pattern exploitation | Session risk accumulator, conversation-level governance, tool-use detection |

## Assumptions

- **Call volume**: Tens of thousands of API calls per week per customer (10K–100K/week). This is realistic for enterprise chatbots, code assistants, and customer support bots.
- **Model consumption**: Primarily API-consumed models (OpenAI, Anthropic, Google, local via Ollama for dev). Self-hosted models are out of scope for v1.
- **Existing infrastructure**: Customers have existing LLM-powered applications and need governance layered on top — not building from scratch.
- **Demo environment**: Uses Ollama (qwen2.5:1.5b) for local testing. Production targets cloud LLM APIs (Claude, GPT-4, Gemini).
- **Team size**: Solo developer hackathon build. Production would need 3–5 engineers for hardening.
- **Latency budget**: Fast-path <10ms p50, <25ms p99. Shadow-path <2s (non-blocking). This assumes the proxy runs close to the application (same datacenter/region).
- **Storage**: PostgreSQL for persistence, NATS for event bus. Both are commodity and self-hostable.
- **Auth**: JWT-based. Demo mode allows unauthenticated access for ease of demonstration. Production would enforce authentication.
- **Feedback quality**: Assumes reviewers make correct decisions most of the time. The 60% trigram similarity threshold (applied to both request and response excerpts) prevents overfitting to individual cases.

## Metrics that matter

| Metric | Current | Target (6mo) |
|--------|---------|--------------|
| Fast-path p50 latency | ~3ms | <5ms |
| Fast-path p99 latency | ~8ms | <25ms |
| False positive rate | Measurable (per-check) | <10% across all axes |
| Escalation suppression (feedback loop) | Active | 30% reduction in repeat cases |
| Detection coverage | 14 checks, 3 axes | 20+ checks, 4 axes |
| Regulatory profiles | 6 | 12+ (community-contributed) |

---

*Prepared for Round 2 judging. Codebase: 12 Rust crates + a Python guardrails sidecar + a Next.js frontend. Demonstrates multi-turn governance, active feedback loops, regulatory adaptability, agent risk tracking, and scalable architecture.*
