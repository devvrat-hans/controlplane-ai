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

interface CostSummary {
  total_tokens: number;
  total_cost_usd: number;
  request_count: number;
  avg_tokens_per_request: number;
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

  const totalSpend = summary?.total_cost_usd ?? 0;
  const projectedMonthly = totalSpend * 30;
  const baseline = projectedMonthly * 0.85;

  // Build chart data from real timeseries
  const chartData = (timeseries ?? []).map((t) => ({
    hour: t.hour,
    tokens: t.tokens,
    requests: t.requests,
  }));

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
            loading={summaryLoading}
          />
          <SummaryCard
            title="Projected Monthly"
            value={`$${projectedMonthly.toFixed(0)}`}
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
                    className="flex items-center justify-between rounded-md border border-border p-2"
                  >
                    <div className="text-sm">
                      <span className="font-medium">{a.app_id.slice(0, 8)}</span>
                      <span className="text-muted-foreground ml-2">{a.metric}</span>
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
      </div>
    </DashboardShell>
  );
}

function SummaryCard({
  title,
  value,
  variant,
  loading,
}: {
  title: string;
  value: string;
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
          <p className={`text-2xl font-bold ${color}`}>{value}</p>
        )}
      </CardContent>
    </Card>
  );
}
