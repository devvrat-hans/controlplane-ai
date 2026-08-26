# ControlPlane.ai — Business Proposal

## Problem

Enterprise teams deploying LLM-powered applications face a governance vacuum:

1. **No real-time guardrails**: Content moderation happens after the fact, not in the request path. By the time a policy violation is caught, the harmful response has already reached the user.

2. **No audit trail**: Regulatory requirements (SOC 2, GDPR, EU AI Act) demand explainable AI decisions. Most teams have no record of *why* a particular output was allowed or blocked.

3. **No feedback loop**: When a human reviewer overrides a model decision, that knowledge is lost. The same mistake repeats because there's no mechanism to learn from corrections.

4. **No cost visibility**: LLM token spend is opaque. Teams can't attribute costs to specific applications or detect anomalous usage patterns.

## Who We Serve

| Segment | Pain Point | Value Prop |
|---------|-----------|------------|
| **AI Platform Teams** | Need governance layer across multiple LLM apps | Single pane of glass for all AI governance |
| **Compliance Officers** | Must demonstrate AI decision auditability | Hash-chained audit trail with every decision |
| **ML Engineers** | Want to iterate on prompts without governance friction | Policy-as-code with per-app granularity |
| **Product Managers** | Need to balance safety with user experience | Tiered responses (allow/edit/flag/block) |

## Solution

ControlPlane.ai is an **AI governance proxy** that sits between your application and LLM providers, providing:

### Core Capabilities

1. **Reverse Proxy with Governance**: All LLM API calls flow through ControlPlane. The fast-path engine (<10ms) applies deterministic checks: PII detection, unsafe content blocking, secret redaction.

2. **Parallel Shadow Analysis**: Expensive checks (hallucination detection, bias classification, prompt injection) run asynchronously after the response is delivered. No impact on user latency.

3. **Compound Risk Detection**: When multiple axes fire simultaneously (e.g., hallucination + privacy), the system escalates even if individual confidences are below threshold — catching overlapping risks that single-axis systems miss.

4. **Reviewer Override Learning (RAG)**: When a human reviewer overrides a model decision, the full context is stored. Similar future cases retrieve these "precedents" and surface them during decision-making — the system learns from corrections.

5. **Policy-as-Code**: Per-application, per-axis policies with versioning. Regulatory profiles (EU-Financial, US-Healthcare) apply pre-configured thresholds.

6. **Hash-Chained Audit Trail**: Every decision is cryptographically linked to the previous one. Tampering is detectable. Verification is one API call.

### Architecture (per AGENTS.md)

- **No LLM in the decision path**: Fast-path is pure Rust/deterministic. Judge model only in shadow path.
- **Fail-open**: Governance never blocks the user on its own errors.
- **Single responsibility**: Each service owns one concern. Events flow through NATS.

## Business Model

| Tier | Price | Includes |
|------|-------|----------|
| **Community** | Free | Self-hosted, 3 apps, basic checks |
| **Pro** | $499/mo | 10 apps, all checks, priority support |
| **Enterprise** | Custom | SSO, SLA, custom policies, dedicated support |

### Revenue Projections (Year 1)

- Target: 50 Pro customers × $499/mo = $24,950/mo = ~$300K ARR
- Enterprise: 5 deals × $5K/mo = $25K/mo = ~$300K ARR
- **Target: $600K ARR by end of Year 1**

## Competitive Landscape

| Feature | ControlPlane.ai | Guardrails AI | Lakera | Rebuff |
|---------|----------------|---------------|--------|--------|
| Real-time proxy | ✅ | ❌ (library) | ✅ | ❌ |
| Shadow analysis | ✅ | ❌ | ❌ | ❌ |
| Feedback learning | ✅ (RAG) | ❌ | ❌ | ❌ |
| Audit trail | ✅ (hash chain) | ❌ | ❌ | ❌ |
| Policy-as-code | ✅ | ✅ | ✅ | ❌ |
| Multi-provider | ✅ (4 providers) | ✅ | ✅ | ✅ |

## Roadmap

### Q3 2026 (Current)
- ✅ Fast-path engine with PII, unsafe content, secret detection
- ✅ Shadow analysis (hallucination, bias, prompt injection, verbosity)
- ✅ Reviewer override RAG learning loop
- ✅ Policy-wise effectiveness stats
- ✅ Regulatory profiles (EU-Financial, US-Healthcare)

### Q4 2026
- [ ] Self-hosted Helm chart for Kubernetes deployment
- [ ] Custom check framework (user-defined regex/pattern rules)
- [ ] Cost anomaly alerting with Slack/webhook integration
- [ ] SSO integration (SAML, OIDC)

### Q1 2027
- [ ] Model performance benchmarking (response quality metrics)
- [ ] A/B testing framework for prompt variations
- [ ] Multi-tenant team management
- [ ] SOC 2 Type II compliance

## Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| LLM providers add native guardrails | Reduced differentiation | Focus on cross-provider governance + audit trail + feedback loop — things providers won't build |
| Enterprise security concerns (proxy in request path) | Slow adoption | Open-source core, self-hosted option, SOC 2 certification |
| Performance overhead too high | User experience degradation | Fast-path <10ms budget, shadow path non-blocking, benchmarks prove <1% overhead |
| Regulatory landscape changes | Feature misalignment | Modular policy engine, regulatory profile marketplace |

## Assumptions

- **Call volume**: Tens of thousands of API calls per week per customer (10K–100K/week). This is realistic for enterprise chatbots, code assistants, and customer support bots.
- **Model consumption**: Primarily API-consumed models (OpenAI, Anthropic, Google, local via Ollama for dev). Self-hosted models are out of scope for v1.
- **Existing infrastructure**: Customers have existing LLM-powered applications and need governance layered on top — not building from scratch.
- **Demo environment**: Uses Ollama (qwen2.5:1.5b) for local testing. Production targets cloud LLM APIs (Claude, GPT-4, Gemini).
- **Team size**: Solo developer hackathon build. Production would need 3–5 engineers for hardening.
- **Latency budget**: Fast-path <10ms p50, <25ms p99. Shadow-path <2s (non-blocking). This assumes the proxy runs close to the application (same datacenter/region).
- **Storage**: PostgreSQL for persistence, NATS for event bus. Both are commodity and self-hostable.
- **Auth**: JWT-based. Demo mode allows unauthenticated access for ease of demonstration. Production would enforce authentication.

---

*Prepared for Round 2 judging. Codebase: 13 Rust crates + Next.js frontend, 272+ passing tests.*
