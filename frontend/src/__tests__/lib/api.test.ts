import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { fetchApi, getStatsOverview, getRecentVerdicts, getPolicyStats, getFeedbackEffectiveness, getDetectionQuality, getApps } from "@/lib/api";

const mockFetch = vi.fn();

beforeEach(() => {
  vi.stubGlobal("fetch", mockFetch);
});

afterEach(() => {
  vi.unstubAllGlobals();
  mockFetch.mockReset();
});

describe("fetchApi", () => {
  it("calls the correct URL with base from env", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () => Promise.resolve({ data: "test" }),
    });

    const result = await fetchApi<{ data: string }>("/api/v1/test");
    expect(mockFetch).toHaveBeenCalledWith(
      "http://localhost:8080/api/v1/test",
      expect.objectContaining({
        headers: { "Content-Type": "application/json" },
      })
    );
    expect(result).toEqual({ data: "test" });
  });

  it("throws on non-ok response", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: false,
      status: 500,
      statusText: "Internal Server Error",
    });

    await expect(fetchApi("/api/v1/broken")).rejects.toThrow(
      "API error: 500 Internal Server Error"
    );
  });

  it("throws on network failure", async () => {
    mockFetch.mockRejectedValueOnce(new TypeError("Network error"));

    await expect(fetchApi("/api/v1/offline")).rejects.toThrow("Network error");
  });
});

describe("getStatsOverview", () => {
  it("fetches /api/v1/stats/overview", async () => {
    const mockData = {
      total_calls_24h: 100,
      total_verdicts_24h: 95,
      blocks_24h: 5,
      escalations_24h: 2,
      passes_24h: 88,
      open_escalations: 1,
      avg_fast_path_latency_ms: 3.2,
      top_blocked_axes: [{ axis: "secret_leak", count: 3 }],
    };

    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () => Promise.resolve(mockData),
    });

    const result = await getStatsOverview();
    expect(result).toEqual(mockData);
    expect(mockFetch).toHaveBeenCalledWith(
      expect.stringContaining("/api/v1/stats/overview"),
      expect.any(Object)
    );
  });
});

describe("getRecentVerdicts", () => {
  it("fetches recent verdicts with default limit", async () => {
    const mockData = { verdicts: [], total: 0 };
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () => Promise.resolve(mockData),
    });

    await getRecentVerdicts();
    expect(mockFetch).toHaveBeenCalledWith(
      expect.stringContaining("/api/v1/verdicts/recent?limit=10"),
      expect.any(Object)
    );
  });

  it("supports custom limit parameter", async () => {
    const mockData = { verdicts: [], total: 0 };
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () => Promise.resolve(mockData),
    });

    await getRecentVerdicts(25);
    expect(mockFetch).toHaveBeenCalledWith(
      expect.stringContaining("/api/v1/verdicts/recent?limit=25"),
      expect.any(Object)
    );
  });
});

describe("getPolicyStats", () => {
  it("fetches policy stats with app_id and window_hours", async () => {
    const mockData = {
      window_hours: 24,
      checks: [
        { check_name: "secret_detection", axis: "responsibility", total: 10, passes: 8, edits: 2, escalates: 0, blocks: 0, confirmed: 1, overridden: 0, dismissed: 0, precision: 1.0 },
      ],
      policies: [],
    };

    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () => Promise.resolve(mockData),
    });

    const result = await getPolicyStats("app-123", 24);
    expect(result).toEqual(mockData);
    expect(mockFetch).toHaveBeenCalledWith(
      expect.stringContaining("/api/v1/stats/policy?app_id=app-123&window_hours=24"),
      expect.any(Object)
    );
  });

  it("defaults to 24h window", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () => Promise.resolve({ checks: [], policies: [] }),
    });

    await getPolicyStats("app-456");
    expect(mockFetch).toHaveBeenCalledWith(
      expect.stringContaining("window_hours=24"),
      expect.any(Object)
    );
  });
});

describe("getFeedbackEffectiveness", () => {
  it("fetches feedback effectiveness metrics", async () => {
    const mockData = {
      patterns_promoted: 5,
      overrides_applied_count: 12,
      threshold_adjustments: 3,
      avg_resolution_time_hours: 2.5,
      resolution_distribution: { confirm_pct: 60, override_pct: 25, dismiss_pct: 15 },
      improvement_indicators: { escalation_rate_trend: "improving", repeat_flag_rate: 0.1, reviewer_agreement_rate: 0.85 },
    };

    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () => Promise.resolve(mockData),
    });

    const result = await getFeedbackEffectiveness();
    expect(result).toEqual(mockData);
    expect(mockFetch).toHaveBeenCalledWith(
      expect.stringContaining("/api/v1/metrics/feedback-effectiveness"),
      expect.any(Object)
    );
  });
});

describe("getDetectionQuality", () => {
  it("fetches detection quality metrics", async () => {
    const mockData = {
      overall_trust_score: 0.85,
      total_escalations_resolved: 20,
      true_positives: 17,
      false_positives: 3,
      false_positive_rate: 0.15,
      precision: 0.85,
      checks: [],
    };

    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () => Promise.resolve(mockData),
    });

    const result = await getDetectionQuality();
    expect(result).toEqual(mockData);
    expect(result.false_positive_rate).toBe(0.15);
  });
});

describe("getApps", () => {
  it("fetches list of apps", async () => {
    const mockData = [
      { id: "app-1", name: "ChatBot", data_governance_level: "medium" },
      { id: "app-2", name: "CodeAssist", data_governance_level: "high" },
    ];

    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: () => Promise.resolve(mockData),
    });

    const result = await getApps();
    expect(result).toEqual(mockData);
    expect(result).toHaveLength(2);
  });
});
