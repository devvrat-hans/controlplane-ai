import { describe, it, expect, vi, beforeEach } from "vitest";
import { fetchApi, getStatsOverview, getRecentVerdicts } from "@/lib/api";

const mockFetch = vi.fn();
global.fetch = mockFetch;

describe("API Client Integration", () => {
  beforeEach(() => {
    mockFetch.mockReset();
  });

  describe("fetchApi", () => {
    it("prepends API_BASE to path", async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ data: "test" }),
      });

      await fetchApi("/api/v1/health");
      expect(mockFetch).toHaveBeenCalledWith(
        "http://localhost:8080/api/v1/health",
        expect.objectContaining({
          headers: { "Content-Type": "application/json" },
        })
      );
    });

    it("throws on non-OK responses", async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 500,
        statusText: "Internal Server Error",
      });

      await expect(fetchApi("/api/v1/broken")).rejects.toThrow(
        "API error: 500 Internal Server Error"
      );
    });

    it("throws on network errors", async () => {
      mockFetch.mockRejectedValueOnce(new Error("Network unreachable"));

      await expect(fetchApi("/api/v1/test")).rejects.toThrow("Network unreachable");
    });

    it("parses JSON response correctly", async () => {
      const mockData = { verdicts: [], total: 0 };
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve(mockData),
      });

      const result = await fetchApi("/api/v1/verdicts/recent");
      expect(result).toEqual(mockData);
    });
  });

  describe("getStatsOverview", () => {
    it("calls correct endpoint and returns typed data", async () => {
      const mockStats = {
        total_calls_24h: 75,
        total_verdicts_24h: 52,
        blocks_24h: 9,
        escalations_24h: 8,
        passes_24h: 34,
        open_escalations: 3,
        avg_fast_path_latency_ms: 3.2,
        top_blocked_axes: [
          { axis: "cost", count: 5 },
          { axis: "responsibility", count: 4 },
        ],
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve(mockStats),
      });

      const result = await getStatsOverview();
      expect(result.total_calls_24h).toBe(75);
      expect(result.blocks_24h).toBe(9);
      expect(result.top_blocked_axes).toHaveLength(2);
      expect(mockFetch).toHaveBeenCalledWith(
        "http://localhost:8080/api/v1/stats/overview",
        expect.any(Object)
      );
    });
  });

  describe("getRecentVerdicts", () => {
    it("passes limit parameter", async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ verdicts: [], total: 0 }),
      });

      await getRecentVerdicts(25);
      expect(mockFetch).toHaveBeenCalledWith(
        "http://localhost:8080/api/v1/verdicts/recent?limit=25",
        expect.any(Object)
      );
    });

    it("defaults to limit=10", async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ verdicts: [], total: 0 }),
      });

      await getRecentVerdicts();
      expect(mockFetch).toHaveBeenCalledWith(
        "http://localhost:8080/api/v1/verdicts/recent?limit=10",
        expect.any(Object)
      );
    });

    it("returns verdict rows with correct shape", async () => {
      const mockVerdicts = {
        verdicts: [
          {
            id: "b0000000-0001-0000-0000-000000000001",
            call_id: "a0000000-0001-0000-0000-000000000001",
            app_id: "10000000-0000-0000-0000-000000000001",
            axis: "cost",
            path: "fast",
            outcome: "pass",
            confidence: 0.05,
            reason: "Token count within budget",
            check_name: "cost_cap",
            latency_ms: 3,
            created_at: "2026-08-21T10:00:00Z",
          },
        ],
        total: 1,
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve(mockVerdicts),
      });

      const result = await getRecentVerdicts(5);
      expect(result.verdicts).toHaveLength(1);
      expect(result.verdicts[0].outcome).toBe("pass");
      expect(result.verdicts[0].confidence).toBe(0.05);
      expect(result.verdicts[0].check_name).toBe("cost_cap");
    });
  });
});
