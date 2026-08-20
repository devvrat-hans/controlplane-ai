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

interface CostAnomaly {
  app_id: string;
  metric: string;
  current_value: number;
  baseline_value: number;
  deviation_pct: number;
}

// Demo data for charts when API is not available
const DEMO_USAGE_DATA = Array.from({ length: 24 }, (_, i) => ({
  hour: `${i}:00`,
  chatbot: Math.floor(Math.random() * 5000 + 2000),
  copilot: Math.floor(Math.random() * 3000 + 1000),
  support: Math.floor(Math.random() * 2000 + 500),
}));

const DEMO_COST_DATA = [
  { day: "Mon", chatbot: 4.2, copilot: 2.8, support: 1.5 },
  { day: "Tue", chatbot: 3.8, copilot: 3.2, support: 1.8 },
  { day: "Wed", chatbot: 5.1, copilot: 2.5, support: 1.2 },
  { day: "Thu", chatbot: 4.5, copilot: 2.9, support: 2.1 },
  { day: "Fri", chatbot: 6.2, copilot: 3.1, support: 1.6 },
  { day: "Sat", chatbot: 2.1, copilot: 1.2, support: 0.8 },
  { day: "Sun", chatbot: 1.8, copilot: 0.9, support: 0.5 },
];

export default function CostPage() {
  const { data: summary } = useQuery<CostSummary>({
    queryKey: ["cost-summary"],
    queryFn: () => fetchApi<CostSummary>("/api/v1/cost/summary"),
    refetchInterval: 10000,
  });

  const { data: anomalies } = useQuery<CostAnomaly[]>({
    queryKey: ["cost-anomalies"],
    queryFn: () => fetchApi<CostAnomaly[]>("/api/v1/cost/anomalies"),
    refetchInterval: 30000,
  });

  const totalSpend = summary?.total_cost_usd ?? 12.8;
  const projectedMonthly = totalSpend * 30;
  const baseline = projectedMonthly * 0.85;

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
          />
          <SummaryCard
            title="Projected Monthly"
            value={`$${projectedMonthly.toFixed(0)}`}
          />
          <SummaryCard
            title="vs. Baseline"
            value={`${projectedMonthly > baseline ? "+" : ""}${(((projectedMonthly - baseline) / baseline) * 100).toFixed(1)}%`}
            variant={projectedMonthly > baseline * 1.2 ? "warning" : "success"}
          />
          <SummaryCard
            title="Tokens Today"
            value={
              summary
                ? `${(summary.total_tokens / 1000).toFixed(0)}K`
                : "124K"
            }
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
                      <span className="text-muted-foreground ml-2">
                        {a.metric}
                      </span>
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
                Token Usage (24h by App)
              </CardTitle>
            </CardHeader>
            <CardContent>
              <ResponsiveContainer width="100%" height={250}>
                <LineChart data={DEMO_USAGE_DATA}>
                  <CartesianGrid strokeDasharray="3 3" opacity={0.1} />
                  <XAxis dataKey="hour" fontSize={10} tickCount={6} />
                  <YAxis fontSize={10} />
                  <Tooltip />
                  <Legend />
                  <Line
                    type="monotone"
                    dataKey="chatbot"
                    stroke="#3b82f6"
                    strokeWidth={2}
                    dot={false}
                    name="chatbot-prod"
                  />
                  <Line
                    type="monotone"
                    dataKey="copilot"
                    stroke="#8b5cf6"
                    strokeWidth={2}
                    dot={false}
                    name="copilot-internal"
                  />
                  <Line
                    type="monotone"
                    dataKey="support"
                    stroke="#10b981"
                    strokeWidth={2}
                    dot={false}
                    name="support-agent"
                  />
                </LineChart>
              </ResponsiveContainer>
            </CardContent>
          </Card>

          {/* Cost per app bar chart */}
          <Card>
            <CardHeader>
              <CardTitle className="text-sm font-medium">
                Daily Cost by App ($)
              </CardTitle>
            </CardHeader>
            <CardContent>
              <ResponsiveContainer width="100%" height={250}>
                <BarChart data={DEMO_COST_DATA}>
                  <CartesianGrid strokeDasharray="3 3" opacity={0.1} />
                  <XAxis dataKey="day" fontSize={10} />
                  <YAxis fontSize={10} />
                  <Tooltip />
                  <Legend />
                  <Bar
                    dataKey="chatbot"
                    fill="#3b82f6"
                    radius={[2, 2, 0, 0]}
                    name="chatbot-prod"
                  />
                  <Bar
                    dataKey="copilot"
                    fill="#8b5cf6"
                    radius={[2, 2, 0, 0]}
                    name="copilot-internal"
                  />
                  <Bar
                    dataKey="support"
                    fill="#10b981"
                    radius={[2, 2, 0, 0]}
                    name="support-agent"
                  />
                  <ReferenceLine
                    y={5.0}
                    stroke="#ef4444"
                    strokeDasharray="3 3"
                    label={{ value: "Budget", position: "right", fontSize: 10 }}
                  />
                </BarChart>
              </ResponsiveContainer>
            </CardContent>
          </Card>
        </div>
      </div>
    </DashboardShell>
  );
}

function SummaryCard({
  title,
  value,
  variant,
}: {
  title: string;
  value: string;
  variant?: "warning" | "success";
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
        <p className={`text-2xl font-bold ${color}`}>{value}</p>
      </CardContent>
    </Card>
  );
}
