"use client";

import { useQuery } from "@tanstack/react-query";
import {
  LineChart,
  Line,
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  Legend,
} from "recharts";
import { DashboardShell } from "@/components/layout/dashboard-shell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { fetchApi } from "@/lib/api";

interface ModelCost {
  model: string;
  tokens: number;
  requests: number;
  cost_usd: number;
}

interface AppCost {
  app_id: string;
  app_name: string;
  requests_24h: number;
  cost_24h_usd: number;
  requests_all_time: number;
  cost_all_time_usd: number;
}

interface CostSummary {
  cost_per_request_usd: number;
  total_tokens: number;
  total_cost_usd: number;
  request_count: number;
  avg_tokens_per_request: number;
  spend_last_hour_usd: number;
  spend_7d_usd: number;
  baseline_daily_spend_usd: number;
  spend_all_time_usd: number;
  requests_all_time: number;
  first_request_at: string | null;
  peak_hour: string | null;
  peak_hour_spend_usd: number;
  by_model: ModelCost[];
  by_app: AppCost[];
}

interface CostTimeseries {
  hour: string;
  tokens: number;
  requests: number;
  cost_usd: number;
}

interface CostDaily {
  day: string;
  tokens: number;
  requests: number;
  cost_usd: number;
}

interface CostAnomaly {
  app_id: string;
  metric: string;
  current_value: number;
  baseline_value: number;
  deviation_pct: number | null;
  unit: "usd" | "tokens";
  window: string;
  severity: "medium" | "high";
  message: string;
}

const usd = (v: number, digits = 2) =>
  `$${v.toLocaleString(undefined, { minimumFractionDigits: digits, maximumFractionDigits: digits })}`;

function formatAnomalyValue(a: CostAnomaly, v: number) {
  return a.unit === "usd" ? usd(v) : `${(v / 1000).toFixed(1)}K tokens`;
}

export default function CostPage() {
  const { data: summary, isLoading: summaryLoading } = useQuery<CostSummary>({
    queryKey: ["cost-summary"],
    queryFn: () => fetchApi<CostSummary>("/api/v1/cost/summary"),
    refetchInterval: 10000,
  });

  const { data: timeseries } = useQuery<CostTimeseries[]>({
    queryKey: ["cost-timeseries"],
    queryFn: () => fetchApi<CostTimeseries[]>("/api/v1/cost/timeseries"),
    refetchInterval: 30000,
  });

  const { data: anomalies } = useQuery<CostAnomaly[]>({
    queryKey: ["cost-anomalies"],
    queryFn: () => fetchApi<CostAnomaly[]>("/api/v1/cost/anomalies"),
    refetchInterval: 30000,
  });

  interface LatencyBucket { hour: string; avg_fast_path_ms: number; p99_fast_path_ms: number; sample_count: number; }
  const { data: latencyData } = useQuery<LatencyBucket[]>({
    queryKey: ["latency-timeseries"],
    queryFn: () => fetchApi<LatencyBucket[]>("/api/v1/metrics/latency-timeseries"),
    refetchInterval: 30000,
  });

  const { data: daily } = useQuery<CostDaily[]>({
    queryKey: ["cost-daily"],
    queryFn: () => fetchApi<CostDaily[]>("/api/v1/cost/daily"),
    refetchInterval: 60000,
  });

  const totalSpend = summary?.total_cost_usd ?? 0;
  const baselineDaily = summary?.baseline_daily_spend_usd ?? 0;
  // Project from the 7-day baseline when there is one; a single bursty day is a poor predictor.
  const projectedMonthly = (baselineDaily > 0 ? baselineDaily : totalSpend) * 30;
  const vsBaselinePct = baselineDaily > 0 ? ((totalSpend - baselineDaily) / baselineDaily) * 100 : null;

  const dailyData = (daily ?? []).map((d) => ({
    day: new Date(`${d.day}T00:00:00Z`).toLocaleDateString([], { day: "numeric", month: "short", timeZone: "UTC" }),
    cost: d.cost_usd,
    requests: d.requests,
  }));

  // Build chart data from real timeseries, converting UTC to local time
  const chartData = (timeseries ?? []).map((t) => {
    const date = new Date(t.hour);
    const localHour = date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });
    const localDay = date.toLocaleDateString([], { day: "numeric", month: "short" });
    return {
      hour: `${localDay} ${localHour}`,
      tokens: t.tokens,
      requests: t.requests,
      cost: t.cost_usd,
    };
  });

  return (
    <DashboardShell>
      <div className="space-y-6">
        <div>
          <h2 className="text-2xl font-bold tracking-tight">Cost Analytics</h2>
          <p className="text-muted-foreground">
            Request volume and spend tracking across applications — billed at a flat{" "}
            {usd(summary?.cost_per_request_usd ?? 1)} per request.
          </p>
        </div>

        {/* Summary cards */}
        <div className="grid gap-4 md:grid-cols-4">
          <SummaryCard
            title="Spend (24h)"
            value={usd(totalSpend)}
            subtitle={`${summary?.request_count ?? 0} requests · ${usd(summary?.spend_last_hour_usd ?? 0)} last hour`}
            loading={summaryLoading}
          />
          <SummaryCard
            title="Projected Monthly"
            value={usd(projectedMonthly, 0)}
            subtitle={baselineDaily > 0 ? "7-day average × 30" : "last 24h × 30"}
            loading={summaryLoading}
          />
          <SummaryCard
            title="vs. 7-Day Baseline"
            value={vsBaselinePct === null ? "—" : `${vsBaselinePct > 0 ? "+" : ""}${vsBaselinePct.toFixed(1)}%`}
            subtitle={baselineDaily > 0 ? `baseline ${usd(baselineDaily)}/day` : "no spend in prior 7 days"}
            variant={vsBaselinePct === null ? undefined : vsBaselinePct > 20 ? "warning" : "success"}
            loading={summaryLoading}
          />
          <SummaryCard
            title="All-Time Spend"
            value={usd(summary?.spend_all_time_usd ?? 0, 0)}
            subtitle={
              summary?.first_request_at
                ? `${summary.requests_all_time.toLocaleString()} requests since ${new Date(summary.first_request_at).toLocaleDateString([], { day: "numeric", month: "short" })}`
                : undefined
            }
            loading={summaryLoading}
          />
        </div>

        {/* Anomaly alerts */}
        {anomalies && anomalies.length > 0 && (
          <Card className="border-orange-500/30">
            <CardHeader>
              <CardTitle className="text-sm font-medium flex items-center gap-2">
                <span className="h-2 w-2 rounded-full bg-orange-500 animate-pulse" />
                Cost Anomalies Detected
              </CardTitle>
            </CardHeader>
            <CardContent>
              <div className="space-y-2">
                {anomalies.map((a, i) => (
                  <div
                    key={i}
                    className="flex items-center justify-between rounded-md border border-border p-3"
                  >
                    <div className="text-sm">
                      <span className="font-medium">{a.app_id}</span>
                      <span className="text-muted-foreground ml-2">— {a.metric}</span>
                      <p className="text-xs text-muted-foreground mt-0.5">
                        Current ({a.window}): {formatAnomalyValue(a, a.current_value)}
                        {a.baseline_value > 0 && (
                          <> | {a.metric.startsWith("Daily budget") ? "Budget" : "Baseline"}: {formatAnomalyValue(a, a.baseline_value)}</>
                        )}
                      </p>
                    </div>
                    <div className="flex items-center gap-2 shrink-0">
                      <Badge
                        variant="outline"
                        className={`text-xs ${a.severity === "high" ? "text-red-500 border-red-500/30" : "text-orange-500 border-orange-500/30"}`}
                      >
                        {a.severity === "high" ? "▲ High" : "● Medium"}
                      </Badge>
                      <Badge className="text-xs bg-orange-500/10 text-orange-500 border-orange-500/20">
                        {a.deviation_pct === null
                          ? "no baseline"
                          : a.metric.startsWith("Daily budget")
                            ? `${(a.deviation_pct + 100).toFixed(0)}% of budget`
                            : `+${a.deviation_pct.toFixed(0)}% above baseline`}
                      </Badge>
                    </div>
                  </div>
                ))}
              </div>
            </CardContent>
          </Card>
        )}

        <div className="grid gap-4 md:grid-cols-2">
          {/* Token usage line chart */}
          <Card>
            <CardHeader>
              <CardTitle className="text-sm font-medium">
                Token Usage (24h)
              </CardTitle>
            </CardHeader>
            <CardContent>
              {chartData.length > 0 ? (
                <ResponsiveContainer width="100%" height={250}>
                  <LineChart data={chartData}>
                    <CartesianGrid strokeDasharray="3 3" opacity={0.1} />
                    <XAxis dataKey="hour" fontSize={10} tickCount={6} />
                    <YAxis fontSize={10} />
                    <Tooltip />
                    <Legend />
                    <Line
                      type="monotone"
                      dataKey="tokens"
                      stroke="#3b82f6"
                      strokeWidth={2}
                      dot={false}
                      name="Tokens"
                    />
                  </LineChart>
                </ResponsiveContainer>
              ) : (
                <div className="flex h-[250px] items-center justify-center text-sm text-muted-foreground">
                  No token usage data yet. Send requests through the proxy to populate.
                </div>
              )}
            </CardContent>
          </Card>

          {/* Spend per hour bar chart */}
          <Card>
            <CardHeader>
              <CardTitle className="text-sm font-medium">
                Spend per Hour (24h)
              </CardTitle>
            </CardHeader>
            <CardContent>
              {chartData.length > 0 ? (
                <ResponsiveContainer width="100%" height={250}>
                  <BarChart data={chartData}>
                    <CartesianGrid strokeDasharray="3 3" opacity={0.1} />
                    <XAxis dataKey="hour" fontSize={10} />
                    <YAxis fontSize={10} tickFormatter={(v) => `$${v}`} />
                    <Tooltip
                      formatter={(value, _name, item) => [
                        `${usd(Number(value))} (${item.payload.requests} requests)`,
                        "Spend",
                      ]}
                    />
                    <Bar dataKey="cost" fill="#3b82f6" radius={[4, 4, 0, 0]} name="Spend" />
                  </BarChart>
                </ResponsiveContainer>
              ) : (
                <div className="flex h-[250px] items-center justify-center text-sm text-muted-foreground">
                  No spend in the last 24 hours. Send requests through the proxy to populate.
                </div>
              )}
            </CardContent>
          </Card>
        </div>

        {/* Historical daily spend */}
        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-medium">
              Daily Spend (30 days)
              {summary && (
                <span className="ml-2 font-normal text-muted-foreground">
                  · {usd(summary.spend_7d_usd, 0)} last 7 days
                </span>
              )}
            </CardTitle>
          </CardHeader>
          <CardContent>
            {dailyData.some((d) => d.requests > 0) ? (
              <ResponsiveContainer width="100%" height={220}>
                <BarChart data={dailyData}>
                  <CartesianGrid strokeDasharray="3 3" opacity={0.1} vertical={false} />
                  <XAxis dataKey="day" fontSize={10} interval="preserveStartEnd" minTickGap={16} />
                  <YAxis fontSize={10} tickFormatter={(v) => `$${v}`} />
                  <Tooltip
                    formatter={(value, _name, item) => [
                      `${usd(Number(value))} (${item.payload.requests} requests)`,
                      "Spend",
                    ]}
                  />
                  <Bar dataKey="cost" fill="#3b82f6" radius={[4, 4, 0, 0]} name="Spend" />
                </BarChart>
              </ResponsiveContainer>
            ) : (
              <div className="flex h-[220px] items-center justify-center text-sm text-muted-foreground">
                No spend in the last 30 days.
              </div>
            )}
          </CardContent>
        </Card>

        {/* Response Time Histogram */}
        {latencyData && latencyData.length > 0 && (
          <Card>
            <CardHeader>
              <CardTitle className="text-sm font-medium">Response Time Distribution (24h)</CardTitle>
            </CardHeader>
            <CardContent>
              <div className="flex gap-4 mb-3">
                <div className="text-sm">
                  <span className="text-muted-foreground">avg </span>
                  <span className="font-mono font-bold">{latencyData[latencyData.length - 1]?.avg_fast_path_ms.toFixed(1)}ms</span>
                </div>
                <div className="text-sm">
                  <span className="text-muted-foreground">p99 </span>
                  <span className="font-mono font-bold">{latencyData[latencyData.length - 1]?.p99_fast_path_ms.toFixed(1)}ms</span>
                </div>
              </div>
              <ResponsiveContainer width="100%" height={200}>
                <BarChart
                  data={latencyData.map(d => {
                    const date = new Date(d.hour);
                    return {
                      time: date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false }),
                      avg: d.avg_fast_path_ms,
                      p99: d.p99_fast_path_ms,
                    };
                  })}
                >
                  <CartesianGrid strokeDasharray="3 3" opacity={0.1} />
                  <XAxis dataKey="time" fontSize={10} tickCount={6} />
                  <YAxis fontSize={10} />
                  <Tooltip formatter={(value) => [`${Number(value).toFixed(1)}ms`, ""]} />
                  <Legend />
                  <Bar dataKey="avg" fill="#3b82f6" radius={[2, 2, 0, 0]} name="Avg" />
                  <Bar dataKey="p99" fill="#f5a623" radius={[2, 2, 0, 0]} name="P99" />
                </BarChart>
              </ResponsiveContainer>
            </CardContent>
          </Card>
        )}

        {/* Cost details */}
        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-medium">Cost Breakdown</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
              <div>
                <p className="text-muted-foreground">Requests (24h)</p>
                <p className="text-lg font-bold">{summary?.request_count ?? 0}</p>
              </div>
              <div>
                <p className="text-muted-foreground">Total Tokens</p>
                <p className="text-lg font-bold">{summary?.total_tokens?.toLocaleString() ?? 0}</p>
              </div>
              <div>
                <p className="text-muted-foreground">Avg Tokens/Request</p>
                <p className="text-lg font-bold">{summary?.avg_tokens_per_request?.toFixed(0) ?? 0}</p>
              </div>
              <div>
                <p className="text-muted-foreground">Cost/Request</p>
                <p className="text-lg font-bold">{usd(summary?.cost_per_request_usd ?? 1)}</p>
              </div>
              <div>
                <p className="text-muted-foreground">Spend Last Hour</p>
                <p className="text-lg font-bold">{usd(summary?.spend_last_hour_usd ?? 0)}</p>
              </div>
              <div>
                <p className="text-muted-foreground">Spend Last 7 Days</p>
                <p className="text-lg font-bold">{usd(summary?.spend_7d_usd ?? 0)}</p>
              </div>
              <div>
                <p className="text-muted-foreground">Avg Daily (prior 7d)</p>
                <p className="text-lg font-bold">{usd(baselineDaily)}</p>
              </div>
              <div>
                <p className="text-muted-foreground">Peak Hour (7d)</p>
                <p className="text-lg font-bold">
                  {summary?.peak_hour ? usd(summary.peak_hour_spend_usd, 0) : "—"}
                </p>
                {summary?.peak_hour && (
                  <p className="text-[10px] text-muted-foreground">
                    {new Date(summary.peak_hour).toLocaleString([], { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit", hour12: false })}
                  </p>
                )}
              </div>
            </div>
          </CardContent>
        </Card>

        {/* Per-app spend: last 24h and all-time */}
        {summary && summary.by_app && summary.by_app.length > 0 && (
          <Card>
            <CardHeader>
              <CardTitle className="text-sm font-medium">Spend by App</CardTitle>
            </CardHeader>
            <CardContent>
              <div className="space-y-3">
                {summary.by_app.map((a) => {
                  const share = summary.spend_all_time_usd > 0 ? a.cost_all_time_usd / summary.spend_all_time_usd : 0;
                  return (
                    <div key={a.app_id} className="space-y-1">
                      <div className="flex items-center justify-between text-sm">
                        <span className="font-medium truncate">{a.app_name}</span>
                        <span className="font-mono text-xs text-muted-foreground">
                          {usd(a.cost_24h_usd)} 24h ·{" "}
                          <span className="font-bold text-foreground">{usd(a.cost_all_time_usd, 0)}</span> all-time
                        </span>
                      </div>
                      <div className="h-1.5 w-full rounded-full bg-muted" title={`${(share * 100).toFixed(1)}% of all-time spend`}>
                        <div className="h-1.5 rounded-full bg-[#3b82f6]" style={{ width: `${share * 100}%` }} />
                      </div>
                    </div>
                  );
                })}
              </div>
            </CardContent>
          </Card>
        )}

        {/* Per-model cost breakdown with bar chart */}
        {summary && summary.by_model && summary.by_model.length > 0 && (
          <div className="grid gap-4 md:grid-cols-2">
            <Card>
              <CardHeader>
                <CardTitle className="text-sm font-medium">Cost by Model</CardTitle>
              </CardHeader>
              <CardContent>
                <div className="space-y-3">
                  {summary.by_model
                    .sort((a, b) => b.cost_usd - a.cost_usd)
                    .map((m) => (
                      <div key={m.model} className="flex items-center justify-between">
                        <div className="flex items-center gap-3">
                          <div className="font-mono text-sm font-medium truncate max-w-[150px]">{m.model}</div>
                          <Badge variant="outline" className="text-xs">
                            {m.requests} req · {(m.tokens / 1000).toFixed(1)}K tokens
                          </Badge>
                        </div>
                        <div className="text-right">
                          <span className="font-mono text-sm font-bold">{usd(m.cost_usd)}</span>
                        </div>
                      </div>
                    ))}
                </div>
              </CardContent>
            </Card>
            <Card>
              <CardHeader>
                <CardTitle className="text-sm font-medium">Token Distribution</CardTitle>
              </CardHeader>
              <CardContent>
                <ResponsiveContainer width="100%" height={200}>
                  <BarChart
                    data={summary.by_model
                      .sort((a, b) => b.tokens - a.tokens)
                      .map(m => ({ name: m.model.length > 15 ? m.model.slice(0, 15) + '...' : m.model, tokens: m.tokens, cost: m.cost_usd }))}
                    layout="vertical"
                  >
                    <CartesianGrid strokeDasharray="3 3" opacity={0.1} />
                    <XAxis type="number" fontSize={10} />
                    <YAxis type="category" dataKey="name" fontSize={10} width={120} />
                    <Tooltip formatter={(value, name) => [name === "tokens" ? `${(Number(value) / 1000).toFixed(1)}K` : usd(Number(value)), name === "tokens" ? "Tokens" : "Cost"]} />
                    <Bar dataKey="tokens" fill="#3b82f6" radius={[0, 4, 4, 0]} name="tokens" />
                  </BarChart>
                </ResponsiveContainer>
              </CardContent>
            </Card>
          </div>
        )}
      </div>
    </DashboardShell>
  );
}

function SummaryCard({
  title,
  value,
  subtitle,
  variant,
  loading,
}: {
  title: string;
  value: string;
  subtitle?: string;
  variant?: "warning" | "success";
  loading?: boolean;
}) {
  const color =
    variant === "warning"
      ? "text-orange-500"
      : variant === "success"
        ? "text-green-500"
        : "";

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-xs font-medium text-muted-foreground">
          {title}
        </CardTitle>
      </CardHeader>
      <CardContent>
        {loading ? (
          <div className="h-7 w-20 animate-pulse rounded bg-muted" />
        ) : (
          <div>
            <p className={`text-2xl font-bold ${color}`}>{value}</p>
            {subtitle && (
              <p className="text-[10px] text-muted-foreground mt-0.5 italic">{subtitle}</p>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
