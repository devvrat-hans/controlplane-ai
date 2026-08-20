const API_BASE = process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080";

export async function fetchApi<T>(path: string): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    headers: { "Content-Type": "application/json" },
  });
  if (!res.ok) {
    throw new Error(`API error: ${res.status} ${res.statusText}`);
  }
  return res.json() as Promise<T>;
}

export interface StatsOverview {
  total_calls_24h: number;
  total_verdicts_24h: number;
  blocks_24h: number;
  escalations_24h: number;
  passes_24h: number;
  open_escalations: number;
  avg_fast_path_latency_ms: number;
  top_blocked_axes: { axis: string; count: number }[];
}

export interface VerdictRow {
  id: string;
  call_id: string;
  app_id: string | null;
  axis: string;
  path: string;
  outcome: string;
  confidence: number;
  reason: string;
  check_name: string;
  latency_ms: number | null;
  created_at: string;
}

export interface RecentVerdictsResponse {
  verdicts: VerdictRow[];
  total: number;
}

export function getStatsOverview() {
  return fetchApi<StatsOverview>("/api/v1/stats/overview");
}

export function getRecentVerdicts(limit = 10) {
  return fetchApi<RecentVerdictsResponse>(
    `/api/v1/verdicts/recent?limit=${limit}`
  );
}
