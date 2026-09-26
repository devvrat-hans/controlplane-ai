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

1. **Reverse Proxy with Governance**: All LLM API calls flow through ControlPlane. The fast-path engine (<10ms) applies deterministic checks: PII detection, unsafe content blocking, secret redaction, cost cap enforcement, and prompt injection detection.

2. **Parallel Shadow Analysis**: Expensive checks (hallucination detection, bias classification, groundedness scoring, verbosity analysis) run asynchronously after the response is delivered. No impact on user latency.

3. **Multi-Turn Session Governance**: Conversations are tracked across turns via `session_id`. A conversation risk accumulator detects adversarial patterns that emerge over multiple exchanges — catching probing attacks that stay just below per-turn thresholds.

4. **Agent/Tool-Use Risk Detection**: When model responses contain action directives (`function_call`, `tool_use`), stricter thresholds apply automatically. A 1.5x confidence multiplier ensures actions receive proportionally more scrutiny than pure text outputs.

5. **Compound Risk Detection**: When multiple axes fire simultaneously (e.g., hallucination + privacy), the system escalates even if individual confidences are below threshold — catching overlapping risks that single-axis systems miss.

6. **Reviewer Override Learning (RAG Feedback Loop)**: When a human reviewer overrides a model decision, the full context is stored. Future similar cases are matched via trigram similarity (pg_trgm). If a previously dismissed pattern recurs, it's automatically suppressed — reducing alert fatigue and demonstrating measurable system improvement over time.

7. **Regulatory Profiles**: Pre-configured governance postures (EU-Financial, US-Healthcare, India-General, APAC-Fintech, UK-Insurance, Global-Startup) with per-axis thresholds. Profiles are independently editable and applied to apps with a single API call. Switching regulatory posture takes seconds, not sprints.

8. **Policy-as-Code**: Per-application, per-axis policies with versioning. 14 governance checks across 3 axes (Responsibility, Performance, Cost) — each configurable independently.

9. **Hash-Chained Audit Trail**: Every decision is cryptographically linked to the previous one (SHA-256). Tampering is detectable. Verification is one API call. Supports export to CSV/JSON for compliance reporting.

10. **Detection Quality Metrics**: Trust score tracking, false positive/negative rate monitoring, precision by axis, and 7-day trending — demonstrating that the system improves over time as reviewers provide feedback.

### Architecture (per AGENTS.md)

- **No LLM in the decision path**: Fast-path is pure Rust/deterministic. Judge model only in shadow path.
- **Fail-open**: Governance never blocks the user on its own errors.
- **Single responsibility**: Each service owns one concern. Events flow through NATS/in-process bus.
- **Per-app isolation**: Each application gets its own policy configuration and risk profile.

## Differentiation

### What makes ControlPlane.ai unique:

1. **Conversation-level governance** — Not just per-request. Multi-turn risk accumulation catches adversarial patterns that single-request systems miss.

2. **Active feedback loop** — The system demonstrably gets better over time. Alert fatigue decreases as reviewer decisions inform future suppressions.

3. **14 governance checks, 2 paths** — 8 synchronous fast-path checks (<10ms) and 6 asynchronous shadow checks. No other solution offers this depth without latency impact.

4. **Per-app, per-axis policy granularity** — A customer support bot and an internal code agent shouldn't have the same governance thresholds. ControlPlane.ai handles this natively.

5. **Regulatory profile switching** — Change from GDPR-strict to startup-relaxed with one API call. Profiles are editable independently of apps.

## Business Model

| Tier | Price | Includes |
|------|-------|----------|
| **Community** | Free | Self-hosted, 3 apps, basic fast-path checks |
| **Pro** | $499/mo | 10 apps, all 14 checks, shadow analysis, priority support |
| **Enterprise** | Custom | SSO, SLA, custom policies, unlimited apps, dedicated support |

### Revenue Projections (Year 1)

- Target: 50 Pro customers × $499/mo = $24,950/mo = ~$300K ARR
- Enterprise: 5 deals × $5K/mo = $25K/mo = ~$300K ARR
- **Target: $600K ARR by end of Year 1**

### Unit Economics

- Infrastructure cost per customer: ~$50/mo (PostgreSQL + compute)
- Gross margin: ~90% (Pro), ~85% (Enterprise with dedicated support)
- CAC target: < $2,000 (dev-tool GTM: content marketing + community)

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

## Roadmap

### Q3 2026 (Current — Implemented)

- ✅ Fast-path engine: PII, unsafe content, secret detection, prompt injection, cost caps
- ✅ Shadow analysis: hallucination, bias, groundedness, verbosity
- ✅ Multi-turn conversation tracking with session risk accumulation
- ✅ Agent/tool-use risk detection with confidence multiplier
- ✅ Reviewer override RAG learning loop (active suppression)
- ✅ Policy-wise effectiveness stats with FP/FN tracking
- ✅ 6 regulatory profiles (EU-Financial, US-Healthcare, India-General, APAC-Fintech, UK-Insurance, Global-Startup)
- ✅ Compound/intersection risk escalation
- ✅ Alert fatigue mitigation (deduplication + feedback suppression)
- ✅ Detection quality metrics (trust score, precision by axis)
- ✅ Load testing framework (100 requests, 3 apps, 3 axes)
- ✅ Full API documentation with interactive "Try it" testing
- ✅ Per-app configurable policies with independent profile editing

### Q4 2026

- [ ] Self-hosted Helm chart for Kubernetes deployment
- [ ] Custom check framework (user-defined regex/pattern rules)
- [ ] Cost anomaly alerting with Slack/webhook integration
- [ ] SSO integration (SAML, OIDC)
- [ ] Vector DB integration for higher-fidelity feedback matching

### Q1 2027

- [ ] Model performance benchmarking (response quality metrics)
- [ ] A/B testing framework for prompt variations
- [ ] Multi-tenant team management
- [ ] SOC 2 Type II compliance
- [ ] Marketplace for community-contributed policy profiles

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
- **Feedback quality**: Assumes reviewers make correct decisions most of the time. The similarity threshold (40% request, 60% response) prevents overfitting to individual cases.

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

*Prepared for Round 2 judging. Codebase: 13 Rust crates + Next.js frontend. Demonstrates multi-turn governance, active feedback loops, regulatory adaptability, agent risk tracking, and scalable architecture.*
