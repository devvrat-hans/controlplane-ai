# E2E Tests — Playwright (Tier 2)

> STATUS: SCAFFOLD — not yet implemented. This document outlines the plan.

## Overview

End-to-end tests using Playwright to verify full user flows against a running
instance of ControlPlane.ai (frontend + backend).

## Planned Test Flows

### 1. Login Flow
- Navigate to `/login`
- Fill demo credentials (`admin@controlplane.test` / `Demo#Admin2026`)
- Verify redirect to overview dashboard
- Verify JWT stored in sessionStorage

### 2. Live Verdict Stream
- Navigate to `/stream`
- Verify SSE connection is established (green "Connected" badge)
- Wait for at least one verdict to appear in the feed
- Verify verdict displays axis, outcome, confidence, and timestamp
- Test pause/resume toggle

### 3. Escalation Resolution
- Navigate to `/escalations`
- Select an open escalation case
- Fill resolution reason
- Click "Confirm" to resolve
- Verify case moves to "Resolved" tab

### 4. Policy Configuration
- Navigate to `/policies`
- Adjust a confidence threshold slider
- Click "Save Policy"
- Verify success toast or confirmation
- Reload page and verify persistence

### 5. Audit Trail Verification
- Navigate to `/audit`
- Trigger chain verification
- Verify "Chain intact" badge
- Filter by outcome and verify results change
- Test CSV export download

## Setup

```bash
pnpm add -D @playwright/test
npx playwright install
```

## Configuration (`playwright.config.ts`)

```typescript
import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  retries: 1,
  use: {
    baseURL: "http://localhost:3000",
    trace: "on-first-retry",
  },
  webServer: {
    command: "pnpm dev",
    port: 3000,
    reuseExistingServer: true,
  },
});
```

## Running

```bash
# Requires backend running (gateway binary or docker-compose)
pnpm exec playwright test

# With UI mode for debugging
pnpm exec playwright test --ui
```

## Dependencies

- Running PostgreSQL with seeded data
- Running gateway binary (proxy + dashboard-api)
- Frontend dev server on port 3000
