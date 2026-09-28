import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

const mockFetch = vi.fn();
global.fetch = mockFetch;

vi.mock("next/navigation", () => ({
  useRouter: () => ({
    push: vi.fn(),
    replace: vi.fn(),
    back: vi.fn(),
    prefetch: vi.fn(),
  }),
  usePathname: () => "/policies",
  useSearchParams: () => new URLSearchParams(),
}));

import PoliciesPage from "@/app/policies/page";

// ═══════════════════════════════════════════════════════════════════
// Test fixtures
// ═══════════════════════════════════════════════════════════════════

const MOCK_APPS = [
  { id: "10000000-0000-0000-0000-000000000001", name: "chatbot-prod" },
  { id: "10000000-0000-0000-0000-000000000002", name: "copilot-internal" },
  { id: "10000000-0000-0000-0000-000000000003", name: "support-agent" },
];

const ALL_ENABLED_POLICY = {
  policies: [
    {
      id: "policy-1",
      axis: "responsibility",
      config: {
        block_threshold: 0.9,
        escalate_threshold: 0.6,
        max_tokens_per_request: 4000,
        retry_max_count: 3,
        pii_detection: true,
        toxicity_detection: true,
        bias_detection: true,
        unsafe_content_enabled: true,
        secret_detection_enabled: true,
        prompt_injection_enabled: true,
        hallucination_detection_enabled: true,
        groundedness_enabled: true,
        verbosity_enabled: true,
        semantic_pii_enabled: true,
      },
      is_active: true,
    },
  ],
};

const ALL_DISABLED_POLICY = {
  policies: [
    {
      id: "policy-2",
      axis: "responsibility",
      config: {
        block_threshold: 0.5,
        escalate_threshold: 0.3,
        max_tokens_per_request: 2000,
        retry_max_count: 1,
        pii_detection: false,
        toxicity_detection: false,
        bias_detection: false,
        unsafe_content_enabled: false,
        secret_detection_enabled: false,
        prompt_injection_enabled: false,
        hallucination_detection_enabled: false,
        groundedness_enabled: false,
        verbosity_enabled: false,
        semantic_pii_enabled: false,
      },
      is_active: true,
    },
  ],
};

const MIXED_POLICY = {
  policies: [
    {
      id: "policy-3",
      axis: "cost",
      config: {
        block_threshold: 0.85,
        escalate_threshold: 0.55,
        pii_detection: true,
        toxicity_detection: false,
        bias_detection: true,
        unsafe_content_enabled: false,
        secret_detection_enabled: true,
        prompt_injection_enabled: false,
        hallucination_detection_enabled: true,
        groundedness_enabled: false,
        verbosity_enabled: true,
        semantic_pii_enabled: false,
      },
      is_active: true,
    },
  ],
};

const PARTIAL_CONFIG_POLICY = {
  policies: [
    {
      id: "policy-4",
      axis: "responsibility",
      config: {
        block_threshold: 0.8,
        // Missing all toggle fields — tests default fallback
      },
      is_active: true,
    },
  ],
};

const EMPTY_POLICY = { policies: [] };
const NO_CONFIG_POLICY = {
  policies: [{ id: "policy-5", axis: "responsibility", config: null, is_active: true }],
};

// ═══════════════════════════════════════════════════════════════════
// All toggle labels and their keys
// ═══════════════════════════════════════════════════════════════════

const ALL_TOGGLE_LABELS = [
  { label: "Unsafe Content Detection", ariaLabel: "Unsafe Content Detection toggle", key: "unsafe_content_enabled" },
  { label: "Secret Detection", ariaLabel: "Secret Detection toggle", key: "secret_detection_enabled" },
  { label: "Prompt Injection Detection", ariaLabel: "Prompt Injection Detection toggle", key: "prompt_injection_enabled" },
  { label: "Hallucination Detection", ariaLabel: "Hallucination Detection toggle", key: "hallucination_detection_enabled" },
  { label: "Groundedness Scoring", ariaLabel: "Groundedness Scoring toggle", key: "groundedness_enabled" },
  { label: "Verbosity Detection", ariaLabel: "Verbosity Detection toggle", key: "verbosity_enabled" },
  { label: "Semantic PII Detection", ariaLabel: "Semantic PII Detection toggle", key: "semantic_pii_enabled" },
  { label: "PII Detection (Presidio)", ariaLabel: "PII Detection (Presidio) toggle", key: "pii_detection" },
  { label: "Toxicity Detection", ariaLabel: "Toxicity Detection toggle", key: "toxicity_detection" },
  { label: "Bias Detection", ariaLabel: "Bias Detection toggle", key: "bias_detection" },
];

const FAST_PATH_LABELS = ALL_TOGGLE_LABELS.filter((t) =>
  ["unsafe_content_enabled", "secret_detection_enabled"].includes(t.key)
);
const SHADOW_PATH_LABELS = ALL_TOGGLE_LABELS.filter((t) =>
  ["prompt_injection_enabled", "hallucination_detection_enabled", "groundedness_enabled", "verbosity_enabled", "semantic_pii_enabled"].includes(t.key)
);
const GUARDRAILS_LABELS = ALL_TOGGLE_LABELS.filter((t) =>
  ["pii_detection", "toxicity_detection", "bias_detection"].includes(t.key)
);

// ═══════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════

function renderAsAdmin() {
  sessionStorage.setItem("cp-token", "mock-token");
  sessionStorage.setItem("cp-user", JSON.stringify({ email: "admin@controlplane.ai", role: "admin" }));
  return render(<PoliciesPage />);
}

function renderAsViewer() {
  sessionStorage.setItem("cp-token", "mock-token");
  sessionStorage.setItem("cp-user", JSON.stringify({ email: "viewer@controlplane.ai", role: "viewer" }));
  return render(<PoliciesPage />);
}

function getToggleSwitches(): HTMLElement[] {
  return screen.getAllByRole("switch");
}

function getToggleByLabel(label: string): HTMLElement {
  const labelEl = screen.getByText(label);
  const row = labelEl.closest('[class*="rounded-xl"]');
  if (!row) throw new Error(`Could not find toggle row for "${label}"`);
  const switchEl = row.querySelector('[role="switch"]') as HTMLElement;
  if (!switchEl) throw new Error(`Could not find switch for "${label}"`);
  return switchEl;
}

/** Wait until the enabled-count badge reflects the expected count. This
 *  ensures the async policy fetch has completed before we assert toggle states. */
async function waitForCount(expectedCount: number, total = 10) {
  await waitFor(() => {
    expect(screen.getByText(`${expectedCount}/${total} checks enabled`)).toBeInTheDocument();
  });
}

function mockAppsAndPolicy(policy: typeof ALL_ENABLED_POLICY) {
  mockFetch.mockImplementation((url: string) => {
    if (url.includes("/api/v1/apps"))
      return Promise.resolve({ ok: true, json: () => Promise.resolve(MOCK_APPS) });
    if (url.includes("/api/v1/policies/"))
      return Promise.resolve({ ok: true, json: () => Promise.resolve(policy) });
    return Promise.resolve({ ok: true, json: () => Promise.resolve({}) });
  });
}

function mockAppsAndPolicyForApp(appId: string, policy: typeof ALL_ENABLED_POLICY) {
  mockFetch.mockImplementation((url: string) => {
    if (url.includes("/api/v1/apps"))
      return Promise.resolve({ ok: true, json: () => Promise.resolve(MOCK_APPS) });
    if (url.includes(`/api/v1/policies/${appId}`))
      return Promise.resolve({ ok: true, json: () => Promise.resolve(policy) });
    return Promise.resolve({ ok: true, json: () => Promise.resolve({}) });
  });
}

// ═══════════════════════════════════════════════════════════════════
// Test suite
// ═══════════════════════════════════════════════════════════════════

describe("Policies Page — Toggle Switches", () => {
  beforeEach(() => {
    sessionStorage.clear();
    mockFetch.mockReset();
  });

  afterEach(() => {
    sessionStorage.clear();
  });

  // ───────────────────────────────────────────────────────────────
  // 1. Toggle rendering
  // ───────────────────────────────────────────────────────────────

  describe("Rendering", () => {
    beforeEach(() => {
      mockAppsAndPolicy(ALL_ENABLED_POLICY);
      renderAsAdmin();
    });

    it("renders exactly 10 toggle switches", async () => {
      await waitForCount(10);
      expect(getToggleSwitches()).toHaveLength(10);
    });

    it("renders all fast-path toggle labels", async () => {
      await waitForCount(10);
      expect(screen.getByText(/Fast-Path Checks/)).toBeInTheDocument();
      for (const { label } of FAST_PATH_LABELS) {
        expect(screen.getByText(label)).toBeInTheDocument();
      }
    });

    it("renders all shadow-path toggle labels", async () => {
      await waitForCount(10);
      expect(screen.getByText(/Shadow-Path Checks/)).toBeInTheDocument();
      for (const { label } of SHADOW_PATH_LABELS) {
        expect(screen.getByText(label)).toBeInTheDocument();
      }
    });

    it("renders all guardrails sidecar toggle labels", async () => {
      await waitForCount(10);
      expect(screen.getByText(/Guardrails Sidecar/)).toBeInTheDocument();
      for (const { label } of GUARDRAILS_LABELS) {
        expect(screen.getByText(label)).toBeInTheDocument();
      }
    });

    it("renders each toggle with a matching aria-label", async () => {
      await waitForCount(10);
      for (const { ariaLabel } of ALL_TOGGLE_LABELS) {
        expect(screen.getByRole("switch", { name: ariaLabel })).toBeInTheDocument();
      }
    });

    it("renders provider badges (Fast-Path Engine, Pure Rust, etc.)", async () => {
      await waitForCount(10);
      // Fast-Path Engine appears for Unsafe Content Detection + Secret Detection
      expect(screen.getAllByText("Fast-Path Engine").length).toBe(2);
      expect(screen.getByText("Pure Rust")).toBeInTheDocument();
      expect(screen.getByText("Laya")).toBeInTheDocument();
      expect(screen.getByText("NLI Model")).toBeInTheDocument();
      expect(screen.getByText("Shadow Analysis")).toBeInTheDocument();
      expect(screen.getByText("NER Model")).toBeInTheDocument();
      expect(screen.getByText("Microsoft Presidio")).toBeInTheDocument();
      // LLM Guard appears for Toxicity + Bias Detection
      expect(screen.getAllByText("LLM Guard").length).toBe(2);
    });
  });

  // ───────────────────────────────────────────────────────────────
  // 2. Toggle default state from API
  // ───────────────────────────────────────────────────────────────

  describe("Default state from API", () => {
    it("all toggles aria-checked=true when policy has all enabled", async () => {
      mockAppsAndPolicy(ALL_ENABLED_POLICY);
      renderAsAdmin();

      await waitForCount(10);
      for (const s of getToggleSwitches()) {
        expect(s).toHaveAttribute("aria-checked", "true");
      }
    });

    it("all toggles aria-checked=false when policy has all disabled", async () => {
      mockAppsAndPolicy(ALL_DISABLED_POLICY);
      renderAsAdmin();

      await waitForCount(0);
      for (const s of getToggleSwitches()) {
        expect(s).toHaveAttribute("aria-checked", "false");
      }
    });

    it("mixed policy correctly reflects individual toggle states", async () => {
      mockAppsAndPolicy(MIXED_POLICY);
      renderAsAdmin();

      // MIXED_POLICY has 5 enabled, 5 disabled
      await waitForCount(5);

      expect(getToggleByLabel("PII Detection (Presidio)")).toHaveAttribute("aria-checked", "true");
      expect(getToggleByLabel("Toxicity Detection")).toHaveAttribute("aria-checked", "false");
      expect(getToggleByLabel("Bias Detection")).toHaveAttribute("aria-checked", "true");
      expect(getToggleByLabel("Unsafe Content Detection")).toHaveAttribute("aria-checked", "false");
      expect(getToggleByLabel("Secret Detection")).toHaveAttribute("aria-checked", "true");
      expect(getToggleByLabel("Prompt Injection Detection")).toHaveAttribute("aria-checked", "false");
      expect(getToggleByLabel("Hallucination Detection")).toHaveAttribute("aria-checked", "true");
      expect(getToggleByLabel("Groundedness Scoring")).toHaveAttribute("aria-checked", "false");
      expect(getToggleByLabel("Verbosity Detection")).toHaveAttribute("aria-checked", "true");
      expect(getToggleByLabel("Semantic PII Detection")).toHaveAttribute("aria-checked", "false");
    });
  });

  // ───────────────────────────────────────────────────────────────
  // 3. Toggle click behavior
  // ───────────────────────────────────────────────────────────────

  describe("Click behavior", () => {
    it("clicking an enabled toggle disables it", async () => {
      mockAppsAndPolicy(ALL_ENABLED_POLICY);
      renderAsAdmin();

      await waitForCount(10);
      const toggle = getToggleByLabel("Unsafe Content Detection");
      expect(toggle).toHaveAttribute("aria-checked", "true");

      fireEvent.click(toggle);
      expect(toggle).toHaveAttribute("aria-checked", "false");
    });

    it("clicking a disabled toggle enables it", async () => {
      mockAppsAndPolicy(ALL_DISABLED_POLICY);
      renderAsAdmin();

      await waitForCount(0);
      const toggle = getToggleByLabel("Toxicity Detection");
      expect(toggle).toHaveAttribute("aria-checked", "false");

      fireEvent.click(toggle);
      expect(toggle).toHaveAttribute("aria-checked", "true");
    });

    it("double-click returns toggle to original state", async () => {
      mockAppsAndPolicy(ALL_ENABLED_POLICY);
      renderAsAdmin();

      await waitForCount(10);
      const toggle = getToggleByLabel("Prompt Injection Detection");

      fireEvent.click(toggle);
      expect(toggle).toHaveAttribute("aria-checked", "false");

      fireEvent.click(toggle);
      expect(toggle).toHaveAttribute("aria-checked", "true");
    });

    it("toggling one switch does not affect other switches", async () => {
      mockAppsAndPolicy(ALL_ENABLED_POLICY);
      renderAsAdmin();

      await waitForCount(10);
      fireEvent.click(getToggleByLabel("Secret Detection"));
      expect(getToggleByLabel("Secret Detection")).toHaveAttribute("aria-checked", "false");

      // All other 9 should still be true
      for (const { label } of ALL_TOGGLE_LABELS) {
        if (label === "Secret Detection") continue;
        expect(getToggleByLabel(label)).toHaveAttribute("aria-checked", "true");
      }
    });

    it("toggling all 10 switches off results in all aria-checked=false", async () => {
      mockAppsAndPolicy(ALL_ENABLED_POLICY);
      renderAsAdmin();

      await waitForCount(10);
      for (const { label } of ALL_TOGGLE_LABELS) {
        fireEvent.click(getToggleByLabel(label));
      }

      for (const s of getToggleSwitches()) {
        expect(s).toHaveAttribute("aria-checked", "false");
      }
    });

    it("toggling multiple switches in mixed state works correctly", async () => {
      mockAppsAndPolicy(ALL_ENABLED_POLICY);
      renderAsAdmin();

      await waitForCount(10);

      fireEvent.click(getToggleByLabel("Unsafe Content Detection"));
      fireEvent.click(getToggleByLabel("Toxicity Detection"));
      fireEvent.click(getToggleByLabel("Verbosity Detection"));

      expect(getToggleByLabel("Unsafe Content Detection")).toHaveAttribute("aria-checked", "false");
      expect(getToggleByLabel("Toxicity Detection")).toHaveAttribute("aria-checked", "false");
      expect(getToggleByLabel("Verbosity Detection")).toHaveAttribute("aria-checked", "false");

      // The other 8 should remain enabled
      const otherLabels = ALL_TOGGLE_LABELS.filter(
        (t) => !["unsafe_content_enabled", "toxicity_detection", "verbosity_enabled"].includes(t.key)
      );
      for (const { label } of otherLabels) {
        expect(getToggleByLabel(label)).toHaveAttribute("aria-checked", "true");
      }
    });
  });

  // ───────────────────────────────────────────────────────────────
  // 4. Enabled count badge
  // ───────────────────────────────────────────────────────────────

  describe("Enabled count badge", () => {
    it("shows 10/10 when all toggles are enabled", async () => {
      mockAppsAndPolicy(ALL_ENABLED_POLICY);
      renderAsAdmin();
      await waitForCount(10);
    });

    it("shows 0/10 when all toggles are disabled", async () => {
      mockAppsAndPolicy(ALL_DISABLED_POLICY);
      renderAsAdmin();
      await waitForCount(0);
    });

    it("updates count when a toggle is clicked", async () => {
      mockAppsAndPolicy(ALL_ENABLED_POLICY);
      renderAsAdmin();

      await waitForCount(10);
      fireEvent.click(getToggleByLabel("Bias Detection"));
      expect(screen.getByText("9/10 checks enabled")).toBeInTheDocument();
    });

    it("decrements count with each disable click", async () => {
      mockAppsAndPolicy(ALL_ENABLED_POLICY);
      renderAsAdmin();

      await waitForCount(10);

      fireEvent.click(getToggleByLabel("Bias Detection"));
      expect(screen.getByText("9/10 checks enabled")).toBeInTheDocument();

      fireEvent.click(getToggleByLabel("Toxicity Detection"));
      expect(screen.getByText("8/10 checks enabled")).toBeInTheDocument();

      fireEvent.click(getToggleByLabel("PII Detection (Presidio)"));
      expect(screen.getByText("7/10 checks enabled")).toBeInTheDocument();
    });

    it("increments count when a disabled toggle is re-enabled", async () => {
      mockAppsAndPolicy(MIXED_POLICY);
      renderAsAdmin();

      await waitForCount(5);

      fireEvent.click(getToggleByLabel("Toxicity Detection"));
      expect(screen.getByText("6/10 checks enabled")).toBeInTheDocument();
    });
  });

  // ───────────────────────────────────────────────────────────────
  // 5. Viewer restrictions
  // ───────────────────────────────────────────────────────────────

  describe("Viewer restrictions", () => {
    it("all toggle switches are disabled for viewer role", async () => {
      mockAppsAndPolicy(ALL_ENABLED_POLICY);
      renderAsViewer();

      await waitForCount(10);
      for (const s of getToggleSwitches()) {
        expect(s).toBeDisabled();
      }
    });

    it("clicking a toggle does not change state for viewer", async () => {
      mockAppsAndPolicy(ALL_ENABLED_POLICY);
      renderAsViewer();

      await waitForCount(10);
      const toggle = getToggleByLabel("Bias Detection");
      fireEvent.click(toggle);
      expect(toggle).toHaveAttribute("aria-checked", "true");
    });

    it("count does not change for viewer after click", async () => {
      mockAppsAndPolicy(ALL_ENABLED_POLICY);
      renderAsViewer();

      await waitForCount(10);
      fireEvent.click(getToggleByLabel("Unsafe Content Detection"));
      expect(screen.getByText("10/10 checks enabled")).toBeInTheDocument();
    });

    it("viewer sees read-only message instead of save button", async () => {
      mockAppsAndPolicy(ALL_ENABLED_POLICY);
      renderAsViewer();

      await waitForCount(10);
      expect(screen.getByText(/Read-only/i)).toBeInTheDocument();
      expect(screen.queryByRole("button", { name: /Save Policy/i })).toBeNull();
    });
  });

  // ───────────────────────────────────────────────────────────────
  // 6. Save payload includes toggle states
  // ───────────────────────────────────────────────────────────────

  describe("Save payload", () => {
    it("PUT request includes all toggle states after toggling", async () => {
      mockAppsAndPolicy(ALL_ENABLED_POLICY);
      renderAsAdmin();

      await waitForCount(10);

      fireEvent.click(getToggleByLabel("Toxicity Detection"));
      fireEvent.click(getToggleByLabel("Verbosity Detection"));

      fireEvent.click(screen.getByRole("button", { name: /Save Policy/i }));

      await waitFor(() => {
        const putCalls = mockFetch.mock.calls.filter(
          (call: [string, RequestInit]) => call[1]?.method === "PUT"
        );
        expect(putCalls.length).toBeGreaterThan(0);
        const body = JSON.parse(putCalls[0][1]?.body as string);

        // Disabled
        expect(body.toxicity_detection).toBe(false);
        expect(body.verbosity_enabled).toBe(false);

        // Still enabled
        expect(body.pii_detection).toBe(true);
        expect(body.bias_detection).toBe(true);
        expect(body.unsafe_content_enabled).toBe(true);
        expect(body.secret_detection_enabled).toBe(true);
        expect(body.prompt_injection_enabled).toBe(true);
        expect(body.hallucination_detection_enabled).toBe(true);
        expect(body.groundedness_enabled).toBe(true);
        expect(body.semantic_pii_enabled).toBe(true);
        // The decision judge was removed; its key is no longer sent.
        expect(body).not.toHaveProperty("decision_judge_enabled");
      });
    });

    it("PUT request targets the selected app endpoint", async () => {
      mockAppsAndPolicy(ALL_ENABLED_POLICY);
      renderAsAdmin();

      await waitForCount(10);
      fireEvent.click(screen.getByRole("button", { name: /Save Policy/i }));

      await waitFor(() => {
        const putCalls = mockFetch.mock.calls.filter(
          (call: [string, RequestInit]) => call[1]?.method === "PUT"
        );
        expect(putCalls[0][0]).toContain("/api/v1/policies/10000000-0000-0000-0000-000000000001");
      });
    });

    it("success message appears after save", async () => {
      mockAppsAndPolicy(ALL_ENABLED_POLICY);
      renderAsAdmin();

      await waitForCount(10);
      fireEvent.click(screen.getByRole("button", { name: /Save Policy/i }));

      await waitFor(() => {
        expect(screen.getByText(/Policy saved successfully/i)).toBeInTheDocument();
      });
    });
  });

  // ───────────────────────────────────────────────────────────────
  // 7. App switching loads correct toggle states
  // ───────────────────────────────────────────────────────────────

  describe("App switching", () => {
    it("switching apps loads different toggle states", async () => {
      mockFetch.mockImplementation((url: string) => {
        if (url.includes("/api/v1/apps"))
          return Promise.resolve({ ok: true, json: () => Promise.resolve(MOCK_APPS) });
        if (url.includes("/api/v1/policies/10000000-0000-0000-0000-000000000001"))
          return Promise.resolve({ ok: true, json: () => Promise.resolve(ALL_ENABLED_POLICY) });
        if (url.includes("/api/v1/policies/10000000-0000-0000-0000-000000000002"))
          return Promise.resolve({ ok: true, json: () => Promise.resolve(ALL_DISABLED_POLICY) });
        return Promise.resolve({ ok: true, json: () => Promise.resolve({}) });
      });
      renderAsAdmin();

      // Initially all enabled (app 1)
      await waitForCount(10);

      // Switch to app 2 (all disabled)
      const selects = screen.getAllByRole("combobox");
      const appSelect = selects.find((sel) => {
        const options = Array.from(sel.querySelectorAll("option"));
        return options.some((o) => o.value.includes("10000000"));
      });
      fireEvent.change(appSelect!, { target: { value: "10000000-0000-0000-0000-000000000002" } });

      // Now should show 0/10
      await waitForCount(0);
    });
  });

  // ───────────────────────────────────────────────────────────────
  // 8. API fallback / edge cases
  // ───────────────────────────────────────────────────────────────

  describe("API fallback", () => {
    it("falls back to defaults when API returns empty policies", async () => {
      mockAppsAndPolicy(EMPTY_POLICY);
      renderAsAdmin();
      await waitForCount(10); // defaults are all enabled
    });

    it("falls back to defaults when config is null", async () => {
      mockAppsAndPolicy(NO_CONFIG_POLICY);
      renderAsAdmin();
      await waitForCount(10);
    });

    it("uses defaults for missing fields in partial config", async () => {
      mockAppsAndPolicy(PARTIAL_CONFIG_POLICY);
      renderAsAdmin();

      // Wait for block_threshold to load from API (0.8) instead of default (0.9)
      await waitFor(() => {
        expect(screen.getByText("0.80")).toBeInTheDocument();
      });
      // All toggle fields missing -> defaults (all true)
      expect(screen.getByText("10/10 checks enabled")).toBeInTheDocument();
    });
    it("falls back to defaults when API fetch fails", async () => {
      mockFetch.mockImplementation((url: string) => {
        if (url.includes("/api/v1/apps"))
          return Promise.resolve({ ok: true, json: () => Promise.resolve(MOCK_APPS) });
        if (url.includes("/api/v1/policies/"))
          return Promise.reject(new Error("Network error"));
        return Promise.resolve({ ok: true, json: () => Promise.resolve({}) });
      });
      renderAsAdmin();
      await waitForCount(10);
    });

    it("falls back to defaults when policy response is not ok", async () => {
      mockFetch.mockImplementation((url: string) => {
        if (url.includes("/api/v1/apps"))
          return Promise.resolve({ ok: true, json: () => Promise.resolve(MOCK_APPS) });
        if (url.includes("/api/v1/policies/"))
          return Promise.resolve({ ok: false, status: 404, json: () => Promise.resolve({}) });
        return Promise.resolve({ ok: true, json: () => Promise.resolve({}) });
      });
      renderAsAdmin();
      await waitForCount(10);
    });
  });

  // ───────────────────────────────────────────────────────────────
  // 9. Toggle visual state (CSS classes)
  // ───────────────────────────────────────────────────────────────

  describe("Visual state", () => {
    it("enabled toggle switch has emerald background", async () => {
      mockAppsAndPolicy(ALL_ENABLED_POLICY);
      renderAsAdmin();

      await waitForCount(10);
      const toggle = getToggleByLabel("Bias Detection");
      expect(toggle.className).toContain("bg-emerald-500");
    });

    it("disabled toggle switch has neutral background", async () => {
      mockAppsAndPolicy(ALL_DISABLED_POLICY);
      renderAsAdmin();

      await waitForCount(0);
      const toggle = getToggleByLabel("Bias Detection");
      // Should NOT have emerald background
      expect(toggle.className).not.toContain("bg-emerald-500");
      // Should have the disabled/muted background
      expect(toggle.className).toContain("bg-border");
    });

    it("enabled toggle row has emerald border accent", async () => {
      mockAppsAndPolicy(ALL_ENABLED_POLICY);
      renderAsAdmin();

      await waitForCount(10);
      const labelEl = screen.getByText("Bias Detection");
      const row = labelEl.closest('[class*="rounded-xl"]') as HTMLElement;
      expect(row.className).toContain("border-emerald-500/25");
    });

    it("disabled toggle row has neutral border", async () => {
      mockAppsAndPolicy(ALL_DISABLED_POLICY);
      renderAsAdmin();

      await waitForCount(0);
      const labelEl = screen.getByText("Bias Detection");
      const row = labelEl.closest('[class*="rounded-xl"]') as HTMLElement;
      expect(row.className).toContain("border-border");
    });

    it("enabled toggle provider badge has emerald color", async () => {
      mockAppsAndPolicy(ALL_ENABLED_POLICY);
      renderAsAdmin();

      await waitForCount(10);
      const badges = screen.getAllByText("Fast-Path Engine");
      badges.forEach((badge) => {
        const badgeEl = badge.closest('[class*="rounded"]') as HTMLElement;
        expect(badgeEl.className).toContain("text-emerald-500/80");
      });
    });

    it("disabled toggle provider badge has muted color", async () => {
      mockAppsAndPolicy(ALL_DISABLED_POLICY);
      renderAsAdmin();

      await waitForCount(0);
      const badges = screen.getAllByText("Fast-Path Engine");
      badges.forEach((badge) => {
        const badgeEl = badge.closest('[class*="rounded"]') as HTMLElement;
        expect(badgeEl.className).toContain("text-muted-foreground/60");
      });
    });
  });
});
