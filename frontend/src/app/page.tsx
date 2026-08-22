"use client";

import { useQuery } from "@tanstack/react-query";
import {
  PieChart,
  Pie,
  Cell,
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  type PieLabelRenderProps,
} from "recharts";
import { DashboardShell } from "@/components/layout/dashboard-shell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { getStatsOverview, getRecentVerdicts } from "@/lib/api";
import type { StatsOverview, VerdictRow } from "@/lib/api";

const OUTCOME_COLORS: Record<string, string> = {
  pass: "#0070f3",
  edit: "#f5a623",
  block: "#ee0000",
  escalate: "#7928ca",
};

export default function OverviewPage() {
  const {
    data: stats,
    isLoading: statsLoading,
    error: statsError,
  } = useQuery<StatsOverview>({
    queryKey: ["stats-overview"],
    queryFn: getStatsOverview,
    refetchInterval: 5000,
  });

  const { data: recentData } = useQuery({
    queryKey: ["recent-verdicts"],
    queryFn: () => getRecentVerdicts(10),
    refetchInterval: 5000,
  });

  return (
    <DashboardShell>
      <div className="space-y-8">
        <div className="space-y-1">
          <h1 className="text-[24px] font-semibold tracking-tighter-brand">Overview.</h1>
          <p className="text-[14px] text-muted-foreground tracking-tight-brand">
            Real-time view of your AI governance layer.
          </p>
        </div>

        {/* Stats cards */}
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-5">
          <StatCard
            title="Requests (24h)"
            value={stats?.total_calls_24h ?? 0}
            loading={statsLoading}
          />
          <StatCard
            title="Block Rate"
            value={
              stats && stats.total_verdicts_24h > 0
                ? `${((stats.blocks_24h / stats.total_verdicts_24h) * 100).toFixed(1)}%`
                : "0%"
            }
            loading={statsLoading}
            variant="destructive"
          />
          <StatCard
            title="Open Escalations"
            value={stats?.open_escalations ?? 0}
            loading={statsLoading}
            variant="warning"
          />
          <StatCard
            title="Avg Latency Added"
            value={stats ? `${stats.avg_fast_path_latency_ms.toFixed(1)}ms` : "—"}
            loading={statsLoading}
          />
          <StatCard
            title="Cost Saved (est.)"
            value={
              stats
                ? `$${((stats.blocks_24h * 0.02) + (stats.escalations_24h * 0.01)).toFixed(2)}`
                : "—"
            }
            loading={statsLoading}
            variant="success"
          />
        </div>

        <div className="grid gap-4 md:grid-cols-2">
          {/* Verdict Distribution Chart */}
          <Card>
            <CardHeader>
              <CardTitle className="text-sm font-medium">
                Verdict Distribution (24h)
              </CardTitle>
            </CardHeader>
            <CardContent>
              <VerdictPieChart stats={stats} loading={statsLoading} />
            </CardContent>
          </Card>

          {/* Top Blocked Axes */}
          <Card>
            <CardHeader>
              <CardTitle className="text-sm font-medium">
                Top Blocked Axes
              </CardTitle>
            </CardHeader>
            <CardContent>
              <AxisBarChart
                data={stats?.top_blocked_axes ?? []}
                loading={statsLoading}
              />
            </CardContent>
          </Card>
        </div>

        {/* Recent Activity Feed */}
        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-medium">
              Recent Activity
            </CardTitle>
          </CardHeader>
          <CardContent>
            <ActivityFeed verdicts={recentData?.verdicts ?? []} />
          </CardContent>
        </Card>

        {statsError && (
          <div className="rounded-md border border-destructive/50 bg-destructive/5 p-4">
            <p className="text-sm text-destructive">
              Failed to load stats. Is the dashboard API running at{" "}
              <code className="text-xs">localhost:8080</code>?
            </p>
          </div>
        )}
      </div>
    </DashboardShell>
  );
}

function StatCard({
  title,
  value,
  loading,
  variant,
}: {
  title: string;
  value: string | number;
  loading?: boolean;
  variant?: "destructive" | "warning" | "success";
}) {
  const valueColor =
    variant === "destructive"
      ? "text-[#ee0000] dark:text-[#ff4444]"
      : variant === "warning"
        ? "text-[#f5a623]"
        : variant === "success"
          ? "text-[#50e3c2]"
          : "text-foreground";

  return (
    <Card className="shadow-vercel-sm">
      <CardHeader className="flex flex-row items-center justify-between pb-2">
        <CardTitle className="text-[12px] font-medium text-muted-foreground uppercase tracking-wide font-mono">
          {title}
        </CardTitle>
      </CardHeader>
      <CardContent>
        {loading ? (
          <div className="h-7 w-20 animate-pulse rounded bg-muted" />
        ) : (
          <div className={`text-[24px] font-semibold tracking-tight-brand ${valueColor}`}>
            {value}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function VerdictPieChart({
  stats,
  loading,
}: {
  stats?: StatsOverview;
  loading: boolean;
}) {
  if (loading || !stats) {
    return (
      <div className="flex h-[200px] items-center justify-center">
        <div className="h-32 w-32 animate-pulse rounded-full bg-muted" />
      </div>
    );
  }

  const edits = Math.max(
    0,
    stats.total_verdicts_24h -
      stats.passes_24h -
      stats.blocks_24h -
      stats.escalations_24h
  );

  const data = [
    { name: "Pass", value: stats.passes_24h },
    { name: "Edit", value: edits },
    { name: "Block", value: stats.blocks_24h },
    { name: "Escalate", value: stats.escalations_24h },
  ].filter((d) => d.value > 0);

  if (data.length === 0) {
    return (
      <div className="flex h-[200px] items-center justify-center text-sm text-muted-foreground">
        No verdict data in the last 24h
      </div>
    );
  }

  return (
    <ResponsiveContainer width="100%" height={200}>
      <PieChart>
        <Pie
          data={data}
          cx="50%"
          cy="50%"
          innerRadius={50}
          outerRadius={80}
          paddingAngle={2}
          dataKey="value"
          label={(props: PieLabelRenderProps) =>
            `${props.name ?? ""} ${(((props.percent as number | undefined) ?? 0) * 100).toFixed(0)}%`
          }
          labelLine={false}
        >
          {data.map((entry) => (
            <Cell
              key={entry.name}
              fill={OUTCOME_COLORS[entry.name.toLowerCase()] || "#6b7280"}
            />
          ))}
        </Pie>
        <Tooltip />
      </PieChart>
    </ResponsiveContainer>
  );
}

function AxisBarChart({
  data,
  loading,
}: {
  data: { axis: string; count: number }[];
  loading: boolean;
}) {
  if (loading) {
    return (
      <div className="flex h-[200px] items-center justify-center">
        <div className="h-full w-full animate-pulse rounded bg-muted" />
      </div>
    );
  }

  if (data.length === 0) {
    return (
      <div className="flex h-[200px] items-center justify-center text-sm text-muted-foreground">
        No blocks recorded in the last 24h
      </div>
    );
  }

  return (
    <ResponsiveContainer width="100%" height={200}>
      <BarChart data={data} layout="vertical" margin={{ left: 20 }}>
        <CartesianGrid strokeDasharray="3 3" opacity={0.1} />
        <XAxis type="number" />
        <YAxis dataKey="axis" type="category" width={100} fontSize={12} />
        <Tooltip />
        <Bar dataKey="count" fill="#ef4444" radius={[0, 4, 4, 0]} />
      </BarChart>
    </ResponsiveContainer>
  );
}

function ActivityFeed({ verdicts }: { verdicts: VerdictRow[] }) {
  if (verdicts.length === 0) {
    return (
      <p className="text-sm text-muted-foreground py-4 text-center">
        No recent activity. Connect to the live stream to see real-time verdicts.
      </p>
    );
  }

  return (
    <div className="space-y-2">
      {verdicts.map((v) => (
        <div
          key={v.id}
          className="flex items-center justify-between rounded-md border border-border p-3"
        >
          <div className="flex items-center gap-3">
            <OutcomeDot outcome={v.outcome} />
            <div>
              <p className="text-sm font-medium">{v.check_name}</p>
              <p className="text-xs text-muted-foreground truncate max-w-[300px]">
                {v.reason}
              </p>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <Badge variant="outline" className="text-[10px]">
              {v.axis}
            </Badge>
            <Badge
              variant="outline"
              className={`text-[10px] ${
                v.path === "fast" ? "border-blue-500/50 text-blue-500" : "border-purple-500/50 text-purple-500"
              }`}
            >
              {v.path}
            </Badge>
            <span className="text-xs text-muted-foreground whitespace-nowrap">
              {new Date(v.created_at).toLocaleTimeString()}
            </span>
          </div>
        </div>
      ))}
    </div>
  );
}

function OutcomeDot({ outcome }: { outcome: string }) {
  const color = OUTCOME_COLORS[outcome] || "#6b7280";
  return (
    <div
      className="h-3 w-3 rounded-full shrink-0"
      style={{ backgroundColor: color }}
    />
  );
}
