import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { fetchApi, getStatsOverview, getRecentVerdicts } from "@/lib/api";

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
      "http://localhost:8081/api/v1/test",
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
