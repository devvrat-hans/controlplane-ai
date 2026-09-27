# Documentation

This folder is split by **when the document was authored**, so the original submission
artifacts stay cleanly separated from the later review/analysis work.

```text
docs/
├── original/    # Authored during the build — the submission artifacts
└── analysis/    # Authored later — review, planning & operational notes
```

---

## `original/` — submission artifacts

These are the hackathon deliverable documents. They describe the project as built.

| Document | Purpose |
|---|---|
| [`architecture.md`](original/architecture.md) | Target vs current architecture; every component labelled `IMPLEMENTED` / `SCAFFOLD` / `TARGET` |
| [`business-proposal.md`](original/business-proposal.md) | Problem, market, differentiation, risks, metrics |
| [`presentation-script.md`](original/presentation-script.md) | Slide content + speaker notes (4:30) |
| [`video-script.md`](original/video-script.md) | Narration for the pre-recorded demo (4:30) |
| [`demo-checklist.md`](original/demo-checklist.md) | Step-by-step live-demo checklist |
| [`demo-credentials.md`](original/demo-credentials.md) | Seeded accounts and auth flow |

## `analysis/` — review & planning documents

These were authored later, for the executive review and the grand finale. They are
grounded in the code, not the pitch.

| Document | Purpose |
|---|---|
| [`executive-briefing.md`](analysis/executive-briefing.md) | Master context + meeting prep: what each feature does, demo script, Q&A answers, honesty notes |
| [`checks-inventory.md`](analysis/checks-inventory.md) | Every governance check: how it works, which OSS library, and **whether it actually runs** |
| [`repo-audit.md`](analysis/repo-audit.md) | Full repository audit with a prioritised improvement roadmap |
| [`jev-laya-integration.md`](analysis/jev-laya-integration.md) | Early design proposal for adding a System One decision-model judge (Jev / Laya) to the shadow path — superseded by the plan below |
| [`laya-integration-plan.md`](analysis/laya-integration-plan.md) | **Current plan:** hybrid per-activity model routing for the shadow path — which engine (or combination) is best for each check, and how fusion improves accuracy |
| [`laya-benchmark-report.md`](analysis/laya-benchmark-report.md) | **Measured** Laya/hybrid benchmark: the pre-Laya baseline on the real corpus, and an explicit account of what could not be measured and why (judge never deployed, 0 judge verdicts) |
| [`laya-combined-result.md`](analysis/laya-combined-result.md) | How each verdict is combined when the judge is ON: the two calibration stages, the noisy-OR fusion, corroboration and disagreement routing |
| [`bias-when-judge-on.md`](analysis/bias-when-judge-on.md) | Whether `laya-bias` replaces or combines with the existing bias detectors |
| [`architecture-diagram.md`](analysis/architecture-diagram.md) | Slide-by-slide Mermaid architecture diagrams for the deck (simple, one per slide, with how to explain each) plus the current-vs-Laya views |
| [`why-rust.md`](analysis/why-rust.md) | Why the backend is Rust: performance, safety and concurrency, with the real code patterns — and why not Python or JavaScript |
| [`open-source-stack.md`](analysis/open-source-stack.md) | Every open-source component we use, the key benefit of each, the alternative we rejected, and what we deliberately did not adopt |
| [`docker-build-troubleshooting.md`](analysis/docker-build-troubleshooting.md) | Runbook for `input/output error` / disk-full failures with Docker + Colima |

---

## Reading order

- **New to the project?** `original/architecture.md` → `analysis/executive-briefing.md`
- **Presenting it?** `analysis/executive-briefing.md` (demo script + Q&A) →
  `original/presentation-script.md`
- **Reviewing the code?** `analysis/checks-inventory.md` → `analysis/repo-audit.md`
- **Planning the finale?** `analysis/repo-audit.md` §6–7
- **`docker compose up --build` failing?** `analysis/docker-build-troubleshooting.md`

> **Note on claims:** where the original docs and the code disagreed, the analysis
> documents follow the **code** and say so. Treat `analysis/` as the current source of
> truth for what is actually running.
