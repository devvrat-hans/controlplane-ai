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
  ReferenceLine,
} from "recharts";
import { DashboardShell } from "@/components/layout/dashboard-shell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { fetchApi } from "@/lib/api";

interface ModelCost {
  model: string;
  tokens: number;
  cost_usd: number;
}

interface CostSummary {
  total_tokens: number;
  total_cost_usd: number;
  request_count: number;
  avg_tokens_per_request: number;
  by_model: ModelCost[];
}

interface CostTimeseries {
  hour: string;
  tokens: number;
  requests: number;
}

interface CostAnomaly {
  app_id: string;
  metric: string;
  current_value: number;
  baseline_value: number;
  deviation_pct: number;
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

  const totalSpend = summary?.total_cost_usd ?? 0;
  const projectedMonthly = totalSpend * 30;
  const baseline = projectedMonthly * 0.85;

  // Build chart data from real timeseries, converting UTC to local time
  const chartData = (timeseries ?? []).map((t) => {
    const date = new Date(t.hour);
    const localHour = date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });
    const localDay = date.toLocaleDateString([], { day: "numeric", month: "short" });
    return {
      hour: `${localDay} ${localHour}`,
      tokens: t.tokens,
      requests: t.requests,
    };
  });

  return (
    <DashboardShell>
      <div className="space-y-6">
        <div>
          <h2 className="text-2xl font-bold tracking-tight">Cost Analytics</h2>
          <p className="text-muted-foreground">
            Token usage and spend tracking across applications.
          </p>
        </div>

        {/* Summary cards */}
        <div className="grid gap-4 md:grid-cols-4">
          <SummaryCard
            title="Total Spend Today"
            value={`$${totalSpend.toFixed(2)}`}
            subtitle="estimated"
            loading={summaryLoading}
          />
          <SummaryCard
            title="Projected Monthly"
            value={`$${projectedMonthly.toFixed(0)}`}
            subtitle="estimated"
            loading={summaryLoading}
          />
          <SummaryCard
            title="vs. Baseline"
            value={`${projectedMonthly > baseline ? "+" : ""}${baseline > 0 ? (((projectedMonthly - baseline) / baseline) * 100).toFixed(1) : "0"}%`}
            variant={projectedMonthly > baseline * 1.2 ? "warning" : "success"}
            loading={summaryLoading}
          />
          <SummaryCard
            title="Tokens Today"
            value={summary ? `${(summary.total_tokens / 1000).toFixed(0)}K` : "0"}
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
                        Current: {(a.current_value / 1000).toFixed(1)}K tokens/hr | Baseline: {(a.baseline_value / 1000).toFixed(1)}K tokens/hr
                      </p>
                    </div>
                    <Badge className="text-xs bg-orange-500/10 text-orange-500 border-orange-500/20">
                      +{a.deviation_pct.toFixed(0)}% above baseline
                    </Badge>
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

          {/* Requests bar chart */}
          <Card>
            <CardHeader>
              <CardTitle className="text-sm font-medium">
                Requests per Hour
              </CardTitle>
            </CardHeader>
            <CardContent>
              {chartData.length > 0 ? (
                <ResponsiveContainer width="100%" height={250}>
                  <BarChart data={chartData}>
                    <CartesianGrid strokeDasharray="3 3" opacity={0.1} />
                    <XAxis dataKey="hour" fontSize={10} />
                    <YAxis fontSize={10} />
                    <Tooltip />
                    <Legend />
                    <Bar
                      dataKey="requests"
                      fill="#3b82f6"
                      radius={[2, 2, 0, 0]}
                      name="Requests"
                    />
                  </BarChart>
                </ResponsiveContainer>
              ) : (
                <div className="flex h-[250px] items-center justify-center text-sm text-muted-foreground">
                  No request data yet. Send requests through the proxy to populate.
                </div>
              )}
            </CardContent>
          </Card>
        </div>

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
                <p className="text-muted-foreground">Total Requests</p>
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
                <p className="text-muted-foreground">Est. Cost/Request</p>
                <p className="text-lg font-bold">
                  ${summary && summary.request_count > 0
                    ? (summary.total_cost_usd / summary.request_count).toFixed(6)
                    : "0.000000"}
                </p>
              </div>
            </div>
          </CardContent>
        </Card>

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
                            {(m.tokens / 1000).toFixed(1)}K tokens
                          </Badge>
                        </div>
                        <div className="text-right">
                          <span className="font-mono text-sm font-bold">${m.cost_usd.toFixed(4)}</span>
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
                    <Tooltip formatter={(value, name) => [name === "tokens" ? `${(Number(value) / 1000).toFixed(1)}K` : `$${Number(value).toFixed(4)}`, name === "tokens" ? "Tokens" : "Cost"]} />
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
