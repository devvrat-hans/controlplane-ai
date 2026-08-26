# ControlPlane.ai — Pitch Deck Outline

## Slide 1: Title
**ControlPlane.ai** — Real-Time AI Governance Proxy
*Problem Track 1: Governance & Safety for LLM Applications*

---

## Slide 2: The Problem
**Every LLM call is a governance blind spot**

- No real-time guardrails — content moderation happens after the fact
- No audit trail — can't prove due diligence to regulators
- No feedback loop — human corrections are lost
- No cost visibility — token spend is opaque

> "87% of enterprises cite governance as the #1 barrier to LLM adoption" — Gartner 2025

---

## Slide 3: The Solution
**An AI governance proxy that sits between your app and LLM providers**

- **Real-time fast-path** (<10ms): PII detection, unsafe content blocking, secret redaction
- **Shadow analysis** (async): Hallucination, bias, prompt injection detection
- **Compound risk**: Catches overlapping risks across axes
- **Reviewer learning loop**: Human corrections become precedents (RAG)
- **Audit trail**: Hash-chained, tamper-evident, one-click verification

---

## Slide 4: Architecture
**No LLM in the decision path**

```
Client → [ControlPlane Proxy] → LLM Provider
              ↓
         Fast-Path (<10ms)
         ├─ Secret detection
         ├─ Unsafe content
         └─ PII redaction
              ↓
         Shadow Path (async)
         ├─ Hallucination check
         ├─ Bias classification
         └─ Prompt injection
              ↓
         Decision Engine
         ├─ Verdict aggregation
         ├─ Precedent retrieval (RAG)
         └─ Policy thresholds
              ↓
         Audit Trail (hash chain)
```

**Key insight**: Fast-path is pure Rust/deterministic. No LLM in the decision path. Judge model only in shadow path.

---

## Slide 5: Tiered Responses
**Not binary — graduated governance**

| Outcome | What happens | When |
|---------|-------------|------|
| **Pass** | Response delivered unmodified | Clean content |
| **Edit** | Secrets/PII redacted transparently | PII or credentials detected |
| **Escalate** | Delivered + flagged for human review | Shadow-path finds issue |
| **Block** | Response withheld from client | Unsafe content / cost cap |

> Show live demo: Edit (redaction) happening in real-time

---

## Slide 6: The Learning Loop
**Human corrections make the system smarter**

1. Model flags content → escalated to reviewer
2. Reviewer overrides: "Stats were reliable in this context"
3. Override stored as precedent (pg_trgm similarity)
4. Next similar call → `[Learned] ⚠ 82%-similar past case was overridden`
5. System suggests (never silently flips) — human-in-the-loop preserved

> "The system learns from corrections instead of repeating mistakes"

---

## Slide 7: Policy Effectiveness
**Know which policies work and which generate false positives**

- Per-check breakdown: Blocked / Escalated / Edited / Passed
- False-positive rate per check (red highlight >30%)
- Regulatory profiles: EU-Financial, US-Healthcare (one-click apply)
- Version-controlled policies with instant hot-reload

---

## Slide 8: Live Demo
**30-second demo script**

1. **Normal request** → Pass (green verdict, <10ms)
2. **Secret in response** → Edit (AWS key redacted)
3. **Compound risk** → Escalate (bias + hallucination)
4. **Override** → Precedent captured
5. **Similar request** → `[Learned]` annotation visible
6. **Audit trail** → Hash chain verified ✓

---

## Slide 9: Competitive Landscape

| | ControlPlane.ai | Guardrails AI | Lakera |
|---|---|---|---|
| Real-time proxy | ✅ | ❌ (library) | ✅ |
| Shadow analysis | ✅ | ❌ | ❌ |
| Feedback learning | ✅ (RAG) | ❌ | ❌ |
| Audit trail | ✅ (hash chain) | ❌ | ❌ |
| Multi-provider | ✅ (4) | ✅ | ✅ |

---

## Slide 10: Market & Business Model

**Target**: AI Platform Teams, Compliance Officers, ML Engineers

| Tier | Price | Includes |
|------|-------|----------|
| Community | Free | Self-hosted, 3 apps |
| Pro | $499/mo | 10 apps, all checks |
| Enterprise | Custom | SSO, SLA, dedicated support |

**Year 1 target**: $600K ARR

---

## Slide 11: Technical Achievements
**13 Rust crates + Next.js frontend**

- 272+ passing tests (Rust) + 109 frontend tests
- Fast-path latency: <10ms p50, <25ms p99
- Hash-chained audit trail with SHA-256 integrity
- pg_trgm trigram similarity for precedent retrieval (zero external deps)
- Live policy hot-reload via NATS events
- 4 provider integrations (Anthropic, Gemini, OpenCode, Ollama)

---

## Slide 12: Roadmap
**Q3 2026**: Core platform ✅ (this demo)
**Q4 2026**: Helm chart, custom checks, cost alerting, SSO
**Q1 2027**: Model benchmarking, A/B testing, multi-tenant, SOC 2

---

## Slide 13: The Ask
**What we need**

- Feedback on governance approach and learning loop design
- Connections to enterprise teams evaluating LLM governance
- Guidance on regulatory positioning (EU AI Act compliance)

---

## Slide 14: Closing
**"Block what you can catch fast. Escalate what you can't. Learn from every correction."**

ControlPlane.ai — Real-Time AI Governance Proxy

---

*Deck should be 10–12 slides maximum. Each slide = one idea. Live demo replaces slides 5–6.*
