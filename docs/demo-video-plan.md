# Fallback Demo Video — Recording Plan

> STATUS: DOCUMENT-ONLY (record before presentation day as backup)

## Purpose

Pre-recorded video demonstration in case the live API is unavailable during the
presentation (network issues, Docker problems, etc.).

## Duration Target

**Under 5 minutes** (aim for 4:00–4:30)

## Recording Script

### Intro (0:00–0:20)
- Show terminal: `./scripts/run_demo.sh` starting up
- Quick flash of docker-compose services becoming healthy
- Narrate: "ControlPlane.ai is a real-time governance layer for AI deployments"

### Architecture Overview (0:20–0:50)
- Show `docs/architecture.md` diagram briefly
- Narrate the fast-path / shadow-path split clearly:
  - "Fast-path: <10ms synchronous checks — secrets, PII, cost caps"
  - "Shadow-path: <2s async deep analysis — bias, groundedness"
  - "Both paths feed into the decision engine"

### Live Dashboard (0:50–1:30)
- Open browser to `http://localhost:3000`
- Show Overview page with live stats
- Navigate through sidebar: Stream, Policies, Escalations, Cost, Audit
- Highlight real-time updates

### Scenario 1: Secret Redaction (1:30–2:15)
- Run the demo_exercise.sh scenario 2
- Show terminal: curl request
- Show dashboard: EDIT verdict appears in real-time stream
- Narrate: "AWS key detected in response, redacted before reaching the client"
- Show the response with `[REDACTED:aws_key_***]` placeholder

### Scenario 2: Cost Cap Block (2:15–2:50)
- Run demo_exercise.sh scenario 3
- Show dashboard: BLOCK verdict appears
- Narrate: "RAG app exceeded its 2048-token budget, request blocked"
- Show the 403 response in terminal

### Scenario 3: Shadow-Path Escalation (2:50–3:30)
- Navigate to Escalations page
- Show an open escalation case
- Narrate: "Shadow-path detected potential bias in a hiring recommendation"
- Resolve the case: click Confirm, add reason
- Show case moving to Resolved tab

### Scenario 4: Audit Trail (3:30–4:10)
- Navigate to Audit page
- Click "Verify Chain Integrity"
- Show green checkmark: "Chain intact — 0 tampered records"
- Narrate: "Every decision is recorded in a tamper-evident hash chain"
- Filter by outcome=block, show the blocked requests

### Closing (4:10–4:30)
- Return to Overview
- Narrate: "All of this runs with <25 microsecond overhead on the fast-path"
- Show benchmark numbers briefly
- "Thank you — questions?"

## Recording Tools

- **Screen recording**: OBS Studio or built-in (Win+G on Windows)
- **Terminal**: Use a large font (14pt+) for readability
- **Browser**: Zoom to 125% for projector visibility
- **Resolution**: 1920x1080, 30fps

## Checklist Before Recording

- [ ] Docker services running (PostgreSQL, NATS)
- [ ] Database seeded (`./scripts/seed_demo.ps1`)
- [ ] Gateway compiled in release mode (`cargo build --release`)
- [ ] Frontend running (`pnpm dev`)
- [ ] Browser dark mode enabled (matches dashboard theme)
- [ ] Terminal with dark background, large font
- [ ] Notifications silenced
- [ ] Second monitor hidden (only record one screen)
