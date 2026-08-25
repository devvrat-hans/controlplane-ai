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
import { getStatsOverview, getRecentVerdicts, fetchApi } from "@/lib/api";
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

        {/* Detection Quality & Feedback Metrics */}
        <DetectionQualitySection />

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
      <div className="flex h-[300px] items-center justify-center">
        <div className="h-40 w-40 animate-pulse rounded-full bg-muted" />
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
      <div className="flex h-[300px] items-center justify-center text-sm text-muted-foreground">
        No verdict data in the last 24h
      </div>
    );
  }

  return (
    <ResponsiveContainer width="100%" height={300}>
      <PieChart>
        <Pie
          data={data}
          cx="50%"
          cy="50%"
          innerRadius={60}
          outerRadius={100}
          paddingAngle={3}
          dataKey="value"
          label={(props: PieLabelRenderProps) =>
            `${props.name ?? ""} ${(((props.percent as number | undefined) ?? 0) * 100).toFixed(0)}%`
          }
          labelLine={true}
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
      <div className="flex h-[300px] items-center justify-center">
        <div className="h-full w-full animate-pulse rounded bg-muted" />
      </div>
    );
  }

  if (data.length === 0) {
    return (
      <div className="flex h-[300px] items-center justify-center text-sm text-muted-foreground">
        No blocks recorded in the last 24h
      </div>
    );
  }

  return (
    <ResponsiveContainer width="100%" height={300}>
      <BarChart data={data} layout="vertical" margin={{ left: 20, top: 10, bottom: 10 }}>
        <CartesianGrid strokeDasharray="3 3" opacity={0.1} />
        <XAxis type="number" />
        <YAxis dataKey="axis" type="category" width={110} fontSize={12} />
        <Tooltip />
        <Bar dataKey="count" fill="#ef4444" radius={[0, 4, 4, 0]} barSize={24} />
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

// === Detection Quality & Feedback Loop Metrics ===

interface DetectionQuality {
  overall_trust_score: number;
  total_escalations_resolved: number;
  true_positives: number;
  false_positives: number;
  precision: number;
  checks: Array<{
    axis: string;
    total_flagged: number;
    confirmed: number;
    overridden: number;
    dismissed: number;
    precision: number;
  }>;
}

interface FeedbackEffectivenessData {
  patterns_promoted: number;
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

function DetectionQualitySection() {
  const { data: quality } = useQuery<DetectionQuality>({
    queryKey: ["detection-quality"],
    queryFn: () => fetchApi<DetectionQuality>("/api/v1/metrics/detection-quality"),
    refetchInterval: 10000,
  });

  const { data: feedback } = useQuery<FeedbackEffectivenessData>({
    queryKey: ["feedback-effectiveness"],
    queryFn: () => fetchApi<FeedbackEffectivenessData>("/api/v1/metrics/feedback-effectiveness"),
    refetchInterval: 10000,
  });

  if (!quality && !feedback) return null;

  const trustColor =
    (quality?.overall_trust_score ?? 0) >= 0.8
      ? "text-[#50e3c2]"
      : (quality?.overall_trust_score ?? 0) >= 0.5
        ? "text-[#f5a623]"
        : "text-[#ee0000]";

  const trendIcon =
    feedback?.improvement_indicators.escalation_rate_trend === "improving"
      ? "↓"
      : feedback?.improvement_indicators.escalation_rate_trend === "worsening"
        ? "↑"
        : "→";

  const trendColor =
    feedback?.improvement_indicators.escalation_rate_trend === "improving"
      ? "text-[#50e3c2]"
      : feedback?.improvement_indicators.escalation_rate_trend === "worsening"
        ? "text-[#ee0000]"
        : "text-muted-foreground";

  return (
    <div className="grid gap-4 md:grid-cols-2">
      {/* Detection Quality */}
      <Card>
        <CardHeader>
          <CardTitle className="text-sm font-medium">
            Detection Quality
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-baseline gap-3">
            <span className={`text-3xl font-bold ${trustColor}`}>
              {quality ? `${(quality.overall_trust_score * 100).toFixed(0)}%` : "—"}
            </span>
            <span className="text-xs text-muted-foreground">Trust Score</span>
          </div>

          <div className="grid grid-cols-3 gap-3 text-center">
            <div>
              <p className="text-lg font-semibold text-[#50e3c2]">
                {quality?.true_positives ?? 0}
              </p>
              <p className="text-[10px] text-muted-foreground">Confirmed</p>
            </div>
            <div>
              <p className="text-lg font-semibold text-[#f5a623]">
                {quality?.false_positives ?? 0}
              </p>
              <p className="text-[10px] text-muted-foreground">False Positives</p>
            </div>
            <div>
              <p className="text-lg font-semibold">
                {quality?.total_escalations_resolved ?? 0}
              </p>
              <p className="text-[10px] text-muted-foreground">Total Resolved</p>
            </div>
          </div>

          {quality?.checks && quality.checks.length > 0 && (
            <div className="space-y-2 pt-2 border-t border-border">
              <p className="text-[10px] text-muted-foreground uppercase tracking-wide">
                Precision by Axis
              </p>
              {quality.checks.map((c) => (
                <div key={c.axis} className="flex items-center gap-2">
                  <span className="text-xs capitalize w-24">{c.axis}</span>
                  <div className="flex-1 h-2 rounded-full bg-muted overflow-hidden">
                    <div
                      className="h-full rounded-full bg-[#50e3c2]"
                      style={{ width: `${c.precision * 100}%` }}
                    />
                  </div>
                  <span className="text-[10px] text-muted-foreground w-10 text-right">
                    {(c.precision * 100).toFixed(0)}%
                  </span>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      {/* Feedback Loop Effectiveness */}
      <Card>
        <CardHeader>
          <CardTitle className="text-sm font-medium">
            Feedback Loop
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid grid-cols-2 gap-4">
            <div>
              <p className="text-lg font-semibold">
                {feedback?.patterns_promoted ?? 0}
              </p>
              <p className="text-[10px] text-muted-foreground">
                Patterns Promoted
              </p>
            </div>
            <div>
              <p className="text-lg font-semibold">
                {feedback?.threshold_adjustments ?? 0}
              </p>
              <p className="text-[10px] text-muted-foreground">
                Threshold Adjustments
              </p>
            </div>
            <div>
              <p className="text-lg font-semibold">
                {feedback?.avg_resolution_time_hours ?? 0}h
              </p>
              <p className="text-[10px] text-muted-foreground">
                Avg Resolution Time
              </p>
            </div>
            <div>
              <p className={`text-lg font-semibold ${trendColor}`}>
                {trendIcon} {feedback?.improvement_indicators.escalation_rate_trend ?? "—"}
              </p>
              <p className="text-[10px] text-muted-foreground">
                Escalation Trend
              </p>
            </div>
          </div>

          {feedback?.resolution_distribution && (
            <div className="pt-2 border-t border-border space-y-2">
              <p className="text-[10px] text-muted-foreground uppercase tracking-wide">
                Resolution Distribution
              </p>
              <div className="flex h-3 rounded-full overflow-hidden">
                {feedback.resolution_distribution.confirm_pct > 0 && (
                  <div
                    className="bg-[#50e3c2]"
                    style={{ width: `${feedback.resolution_distribution.confirm_pct}%` }}
                  />
                )}
                {feedback.resolution_distribution.override_pct > 0 && (
                  <div
                    className="bg-[#f5a623]"
                    style={{ width: `${feedback.resolution_distribution.override_pct}%` }}
                  />
                )}
                {feedback.resolution_distribution.dismiss_pct > 0 && (
                  <div
                    className="bg-muted-foreground/30"
                    style={{ width: `${feedback.resolution_distribution.dismiss_pct}%` }}
                  />
                )}
              </div>
              <div className="flex justify-between text-[10px] text-muted-foreground">
                <span>Confirmed {feedback.resolution_distribution.confirm_pct}%</span>
                <span>Overridden {feedback.resolution_distribution.override_pct}%</span>
                <span>Dismissed {feedback.resolution_distribution.dismiss_pct}%</span>
              </div>
            </div>
          )}

          {feedback?.improvement_indicators && (
            <div className="pt-2 border-t border-border">
              <p className="text-[10px] text-muted-foreground uppercase tracking-wide mb-2">
                System Learning
              </p>
              <p className="text-xs">
                Reviewer agreement:{" "}
                <span className="font-medium">
                  {(feedback.improvement_indicators.reviewer_agreement_rate * 100).toFixed(0)}%
                </span>
              </p>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
