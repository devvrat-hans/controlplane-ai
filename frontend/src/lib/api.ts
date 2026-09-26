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
  requests_per_minute: number;
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

export function getStatsOverview(appId?: string) {
  const params = appId && appId !== "all" ? `?app_id=${appId}` : "";
  return fetchApi<StatsOverview>(`/api/v1/stats/overview${params}`);
}

export function getRecentVerdicts(limit = 10, appId?: string) {
  const params = new URLSearchParams({ limit: String(limit) });
  if (appId && appId !== "all") params.set("app_id", appId);
  return fetchApi<RecentVerdictsResponse>(
    `/api/v1/verdicts/recent?${params.toString()}`
  );
}

// --- Policy-wise effectiveness stats ---

export interface PolicyCheckStat {
  check_name: string;
  axis: string;
  total: number;
  passes: number;
  edits: number;
  escalates: number;
  blocks: number;
  confirmed: number;
  overridden: number;
  dismissed: number;
  precision: number;
}

export interface PolicyStatsResponse {
  window_hours: number;
  checks: PolicyCheckStat[];
  policies: { axis: string; config: Record<string, unknown> }[];
}

export function getPolicyStats(appId: string, windowHours = 24) {
  return fetchApi<PolicyStatsResponse>(
    `/api/v1/stats/policy?app_id=${appId}&window_hours=${windowHours}`
  );
}

// --- Feedback effectiveness ---

export interface FeedbackEffectiveness {
  patterns_promoted: number;
  overrides_applied_count: number;
  threshold_adjustments: number;
  avg_resolution_time_hours: number;
  resolution_distribution: {
    confirm_pct: number;
    override_pct: number;
    dismiss_pct: number;
  };
  improvement_indicators: {
    escalation_rate_trend: string;
    repeat_flag_rate: number;
    reviewer_agreement_rate: number;
  };
}

export function getFeedbackEffectiveness() {
  return fetchApi<FeedbackEffectiveness>("/api/v1/metrics/feedback-effectiveness");
}

// --- Detection quality ---

export interface DetectionQuality {
  overall_trust_score: number;
  total_escalations_resolved: number;
  true_positives: number;
  false_positives: number;
  precision: number;
  checks: {
    axis: string;
    total_flagged: number;
    confirmed: number;
    overridden: number;
    dismissed: number;
    precision: number;
  }[];
}

export function getDetectionQuality() {
  return fetchApi<DetectionQuality>("/api/v1/metrics/detection-quality");
}

// --- Apps ---

export interface AppInfo {
  id: string;
  name: string;
  data_governance_level?: string;
}

export function getApps() {
  return fetchApi<AppInfo[]>("/api/v1/apps");
}

// --- Requests list ---

export interface RequestListItem {
  id: string;
  app_id: string;
  model: string;
  token_count_input: number | null;
  token_count_output: number | null;
  upstream_latency_ms: number | null;
  fast_path_latency_ms: number | null;
  outcome: string;
  created_at: string;
}

export interface RequestsListResponse {
  requests: RequestListItem[];
  total: number;
  limit: number;
  offset: number;
}

export function getRequests(opts: { limit?: number; offset?: number; search?: string; model?: string; outcome?: string; app_id?: string } = {}) {
  const params = new URLSearchParams();
  if (opts.limit) params.set("limit", String(opts.limit));
  if (opts.offset) params.set("offset", String(opts.offset));
  if (opts.search) params.set("search", opts.search);
  if (opts.model && opts.model !== "all") params.set("model", opts.model);
  if (opts.outcome && opts.outcome !== "all") params.set("outcome", opts.outcome);
  if (opts.app_id && opts.app_id !== "all") params.set("app_id", opts.app_id);
  return fetchApi<RequestsListResponse>(`/api/v1/requests?${params.toString()}`);
}
