"use client";

import { useState, useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  BarChart,
  Bar,
  PieChart,
  Pie,
  Cell,
  AreaChart,
  Area,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  Legend,
  type PieLabelRenderProps,
} from "recharts";
import { DashboardShell } from "@/components/layout/dashboard-shell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { fetchApi } from "@/lib/api";
import { useApp } from "@/components/providers/app-provider";


// ─── Colors ────────────────────────────────────────────────────────────────
const OUTCOME_COLORS: Record<string, string> = {
  pass: "#0070f3",
  edit: "#f5a623",
  block: "#ee0000",
  escalate: "#7928ca",
};

const CHECK_COLORS = [
  "#0070f3",
  "#7928ca",
  "#50e3c2",
  "#ff0080",
  "#f5a623",
  "#ee0000",
  "#0761d1",
  "#4c2889",
  "#29bc9b",
  "#eb367f",
];

// ─── Types ─────────────────────────────────────────────────────────────────
interface DbVerdict {
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

interface PolicyCheckStat {
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

interface DetectionQuality {
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

interface FeedbackData {
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

interface RequestListItem {
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

type TimeRange = "1h" | "6h" | "24h" | "7d" | "30d";

// ─── Helpers ───────────────────────────────────────────────────────────────
function hoursFromRange(range: TimeRange): number {
  switch (range) {
    case "1h": return 1;
    case "6h": return 6;
    case "24h": return 24;
    case "7d": return 168;
    case "30d": return 720;
  }
}

function formatHour(date: Date): string {
  return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });
}

function formatDay(date: Date): string {
  return date.toLocaleDateString([], { day: "numeric", month: "short" });
}

function bucketByHour(records: DbVerdict[], hours: number) {
  const now = new Date();
  const cutoff = new Date(now.getTime() - hours * 3600 * 1000);
  const buckets: Record<string, { pass: number; edit: number; block: number; escalate: number; total: number }> = {};

  // Initialize all buckets
  for (let i = hours; i >= 0; i--) {
    const d = new Date(now.getTime() - i * 3600 * 1000);
    const key = hours <= 24 ? formatHour(d) : `${formatDay(d)} ${formatHour(d)}`;
    buckets[key] = { pass: 0, edit: 0, block: 0, escalate: 0, total: 0 };
  }

  records.forEach((r) => {
    const d = new Date(r.created_at);
    if (d < cutoff) return;
    const key = hours <= 24 ? formatHour(d) : `${formatDay(d)} ${formatHour(d)}`;
    if (!buckets[key]) buckets[key] = { pass: 0, edit: 0, block: 0, escalate: 0, total: 0 };
    const outcome = r.outcome.toLowerCase();
    if (outcome in buckets[key]) {
      (buckets[key] as Record<string, number>)[outcome]++;
    }
    buckets[key].total++;
  });

  return Object.entries(buckets).map(([hour, data]) => ({ hour, ...data }));
}

function filterByTimeRange(records: DbVerdict[], hours: number): DbVerdict[] {
  const cutoff = new Date(Date.now() - hours * 3600 * 1000);
  return records.filter((r) => new Date(r.created_at) >= cutoff);
}

// ─── Main Page ─────────────────────────────────────────────────────────────
export default function AnalyticsPage() {
  const { selectedAppId } = useApp();
  const [timeRange, setTimeRange] = useState<TimeRange>("24h");
  const [searchQuery, setSearchQuery] = useState("");
  const [selectedAxis, setSelectedAxis] = useState<string>("all");
  const [selectedOutcome, setSelectedOutcome] = useState<string>("all");
  const [selectedPolicy, setSelectedPolicy] = useState<string | null>(null);

  const hours = hoursFromRange(timeRange);
  const appQ = selectedAppId !== "all" ? `&app_id=${selectedAppId}` : "";

  // Fetch all data
  const { data: verdictsData, isLoading: verdictsLoading } = useQuery<{ verdicts: DbVerdict[] }>({
    queryKey: ["analytics-verdicts", selectedAppId],
    queryFn: () => fetchApi<{ verdicts: DbVerdict[] }>(`/api/v1/verdicts/recent?limit=500${appQ}`),
    refetchInterval: 10000,
  });

  const { data: policyStats } = useQuery<{ checks: PolicyCheckStat[] }>({
    queryKey: ["analytics-policy", selectedAppId, hours],
    queryFn: () => fetchApi<{ checks: PolicyCheckStat[] }>(`/api/v1/stats/policy?window_hours=${hours}${appQ}`),
    refetchInterval: 15000,
  });

  const { data: detectionQuality } = useQuery<DetectionQuality>({
    queryKey: ["analytics-quality"],
    queryFn: () => fetchApi<DetectionQuality>(`/api/v1/metrics/detection-quality`),
    refetchInterval: 15000,
  });

  const { data: feedbackData } = useQuery<FeedbackData>({
    queryKey: ["analytics-feedback"],
    queryFn: () => fetchApi<FeedbackData>(`/api/v1/metrics/feedback-effectiveness`),
    refetchInterval: 15000,
  });

  const { data: requestsData } = useQuery<{ requests: RequestListItem[] }>({
    queryKey: ["analytics-requests", selectedAppId],
    queryFn: () => fetchApi<{ requests: RequestListItem[] }>(`/api/v1/requests?limit=500${appQ}`),
    refetchInterval: 10000,
  });

  const allVerdicts = verdictsData?.verdicts ?? [];
  const allRequests = requestsData?.requests ?? [];

  // Filter verdicts by time range
  const verdicts = useMemo(() => filterByTimeRange(allVerdicts, hours), [allVerdicts, hours]);
  const requests = useMemo(() => filterByTimeRange(allRequests as unknown as DbVerdict[], hours).map((r) => r as unknown as RequestListItem), [allRequests, hours]);

  // Apply search + axis + outcome filters
  const filteredVerdicts = useMemo(() => {
    let result = verdicts;
    if (selectedAxis !== "all") result = result.filter((v) => v.axis === selectedAxis);
    if (selectedOutcome !== "all") result = result.filter((v) => v.outcome === selectedOutcome);
    if (searchQuery) {
      const q = searchQuery.toLowerCase();
      result = result.filter(
        (v) =>
          v.check_name.toLowerCase().includes(q) ||
          v.reason.toLowerCase().includes(q) ||
          v.app_id?.toLowerCase().includes(q) ||
          v.call_id.toLowerCase().includes(q)
      );
    }
    return result;
  }, [verdicts, selectedAxis, selectedOutcome, searchQuery]);

  // ─── Computed Stats ────────────────────────────────────────────────────
  const totalVerdicts = filteredVerdicts.length;
  const passCount = filteredVerdicts.filter((v) => v.outcome === "pass").length;
  const editCount = filteredVerdicts.filter((v) => v.outcome === "edit").length;
  const blockCount = filteredVerdicts.filter((v) => v.outcome === "block").length;
  const escalateCount = filteredVerdicts.filter((v) => v.outcome === "escalate").length;
  const blockRate = totalVerdicts > 0 ? ((blockCount / totalVerdicts) * 100).toFixed(1) : "0";
  const escalationRate = totalVerdicts > 0 ? ((escalateCount / totalVerdicts) * 100).toFixed(1) : "0";
  const avgConfidence = totalVerdicts > 0 ? (filteredVerdicts.reduce((s, v) => s + v.confidence, 0) / totalVerdicts * 100).toFixed(1) : "0";
  const fastPathCount = filteredVerdicts.filter((v) => v.path === "fast").length;
  const shadowPathCount = filteredVerdicts.filter((v) => v.path === "shadow").length;

  // Chart data
  const volumeData = useMemo(() => bucketByHour(filteredVerdicts, hours), [filteredVerdicts, hours]);

  const outcomePieData = [
    { name: "Pass", value: passCount },
    { name: "Edit", value: editCount },
    { name: "Block", value: blockCount },
    { name: "Escalate", value: escalateCount },
  ].filter((d) => d.value > 0);

  const axisOutcomeData = useMemo(() => {
    const map: Record<string, { pass: number; edit: number; block: number; escalate: number }> = {};
    filteredVerdicts.forEach((v) => {
      if (!map[v.axis]) map[v.axis] = { pass: 0, edit: 0, block: 0, escalate: 0 };
      const outcome = v.outcome.toLowerCase();
      if (outcome in map[v.axis]) {
        (map[v.axis] as Record<string, number>)[outcome]++;
      }
    });
    return Object.entries(map).map(([axis, data]) => ({ axis, ...data }));
  }, [filteredVerdicts]);

  const modelData = useMemo(() => {
    const map: Record<string, number> = {};
    requests.forEach((r) => {
      if (r.model) map[r.model] = (map[r.model] || 0) + 1;
    });
    return Object.entries(map)
      .map(([model, count]) => ({ model: model.length > 20 ? model.slice(0, 20) + "…" : model, count }))
      .sort((a, b) => b.count - a.count)
      .slice(0, 10);
  }, [requests]);

  const appData = useMemo(() => {
    const map: Record<string, number> = {};
    filteredVerdicts.forEach((v) => {
      const app = v.app_id || "unknown";
      map[app] = (map[app] || 0) + 1;
    });
    return Object.entries(map)
      .map(([app, count]) => ({ app: app.length > 16 ? app.slice(0, 16) + "…" : app, count }))
      .sort((a, b) => b.count - a.count)
      .slice(0, 8);
  }, [filteredVerdicts]);

  const confidenceDistribution = useMemo(() => {
    const buckets = [
      { range: "0-20%", count: 0 },
      { range: "20-40%", count: 0 },
      { range: "40-60%", count: 0 },
      { range: "60-80%", count: 0 },
      { range: "80-100%", count: 0 },
    ];
    filteredVerdicts.forEach((v) => {
      const pct = v.confidence * 100;
      if (pct < 20) buckets[0].count++;
      else if (pct < 40) buckets[1].count++;
      else if (pct < 60) buckets[2].count++;
      else if (pct < 80) buckets[3].count++;
      else buckets[4].count++;
    });
    return buckets;
  }, [filteredVerdicts]);

  const checks = policyStats?.checks ?? [];

  return (
    <DashboardShell>
      <div className="space-y-6">
        {/* Header */}
        <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4">
          <div>
            <h1 className="text-[24px] font-semibold tracking-tighter-brand">Analytics.</h1>
            <p className="text-[14px] text-muted-foreground tracking-tight-brand">
              Deep-dive into your AI governance performance.
            </p>
          </div>
          {/* Time range selector */}
          <div className="flex items-center gap-1 bg-muted/50 rounded-lg p-1">
            {(["1h", "6h", "24h", "7d", "30d"] as TimeRange[]).map((r) => (
              <button
                key={r}
                onClick={() => setTimeRange(r)}
                className={`px-3 py-1.5 text-xs font-medium rounded-md transition-colors ${
                  timeRange === r
                    ? "bg-background text-foreground shadow-sm"
                    : "text-muted-foreground hover:text-foreground"
                }`}
              >
                {r}
              </button>
            ))}
          </div>
        </div>

        {/* Filters */}
        <Card>
          <CardContent className="py-3">
            <div className="flex flex-wrap items-center gap-3">
              <input
                type="text"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder="Search checks, reasons, apps..."
                className="h-8 w-64 rounded-md border border-input bg-background px-3 text-xs focus:outline-none focus:ring-2 focus:ring-ring/20"
              />
              <select
                value={selectedAxis}
                onChange={(e) => setSelectedAxis(e.target.value)}
                className="h-8 rounded-md border border-input bg-background px-2 text-xs focus:outline-none focus:ring-2 focus:ring-ring/20"
              >
                <option value="all">All Axes</option>
                <option value="performance">Performance</option>
                <option value="cost">Cost</option>
                <option value="responsibility">Responsibility</option>
              </select>
              <select
                value={selectedOutcome}
                onChange={(e) => setSelectedOutcome(e.target.value)}
                className="h-8 rounded-md border border-input bg-background px-2 text-xs focus:outline-none focus:ring-2 focus:ring-ring/20"
              >
                <option value="all">All Outcomes</option>
                <option value="pass">Pass</option>
                <option value="edit">Edit</option>
                <option value="block">Block</option>
                <option value="escalate">Escalate</option>
              </select>
              <div className="ml-auto text-xs text-muted-foreground">
                {filteredVerdicts.length} of {totalVerdicts} verdicts in {timeRange}
              </div>
            </div>
          </CardContent>
        </Card>

        {/* Summary stat cards */}
        <div className="grid gap-4 grid-cols-2 md:grid-cols-3 lg:grid-cols-6">
          <MiniStatCard title="Total Verdicts" value={totalVerdicts} loading={verdictsLoading} />
          <MiniStatCard title="Block Rate" value={`${blockRate}%`} color={Number(blockRate) > 10 ? "text-[#ee0000]" : "text-[#0070f3]"} loading={verdictsLoading} />
          <MiniStatCard title="Escalation Rate" value={`${escalationRate}%`} color={Number(escalationRate) > 15 ? "text-[#f5a623]" : "text-[#50e3c2]"} loading={verdictsLoading} />
          <MiniStatCard title="Avg Confidence" value={`${avgConfidence}%`} loading={verdictsLoading} />
          <MiniStatCard title="Fast-Path" value={fastPathCount} color="text-[#0070f3]" loading={verdictsLoading} />
          <MiniStatCard title="Shadow-Path" value={shadowPathCount} color="text-[#7928ca]" loading={verdictsLoading} />
        </div>

        {/* Row 1: Volume + Outcome */}
        <div className="grid gap-4 md:grid-cols-3">
          {/* Request volume over time */}
          <Card className="md:col-span-2">
            <CardHeader>
              <CardTitle className="text-sm font-medium">Verdict Volume Over Time</CardTitle>
            </CardHeader>
            <CardContent>
              {volumeData.length > 0 ? (
                <ResponsiveContainer width="100%" height={280}>
                  <AreaChart data={volumeData}>
                    <CartesianGrid strokeDasharray="3 3" opacity={0.1} />
                    <XAxis dataKey="hour" fontSize={9} tickCount={8} />
                    <YAxis fontSize={9} width={30} />
                    <Tooltip />
                    <Legend />
                    <Area type="monotone" dataKey="pass" stackId="1" stroke="#0070f3" fill="#0070f3" fillOpacity={0.3} name="Pass" />
                    <Area type="monotone" dataKey="edit" stackId="1" stroke="#f5a623" fill="#f5a623" fillOpacity={0.3} name="Edit" />
                    <Area type="monotone" dataKey="block" stackId="1" stroke="#ee0000" fill="#ee0000" fillOpacity={0.3} name="Block" />
                    <Area type="monotone" dataKey="escalate" stackId="1" stroke="#7928ca" fill="#7928ca" fillOpacity={0.3} name="Escalate" />
                  </AreaChart>
                </ResponsiveContainer>
              ) : (
                <div className="flex h-[280px] items-center justify-center text-sm text-muted-foreground">
                  No data for this time range.
                </div>
              )}
            </CardContent>
          </Card>

          {/* Outcome distribution */}
          <Card>
            <CardHeader>
              <CardTitle className="text-sm font-medium">Outcome Distribution</CardTitle>
            </CardHeader>
            <CardContent>
              {outcomePieData.length > 0 ? (
                <ResponsiveContainer width="100%" height={280}>
                  <PieChart>
                    <Pie
                      data={outcomePieData}
                      cx="50%"
                      cy="50%"
                      innerRadius={55}
                      outerRadius={90}
                      paddingAngle={3}
                      dataKey="value"
                      label={(props: PieLabelRenderProps) =>
                        `${props.name ?? ""} ${(((props.percent as number | undefined) ?? 0) * 100).toFixed(0)}%`
                      }
                      labelLine={true}
                    >
                      {outcomePieData.map((entry) => (
                        <Cell key={entry.name} fill={OUTCOME_COLORS[entry.name.toLowerCase()] || "#6b7280"} />
                      ))}
                    </Pie>
                    <Tooltip />
                  </PieChart>
                </ResponsiveContainer>
              ) : (
                <div className="flex h-[280px] items-center justify-center text-sm text-muted-foreground">
                  No outcome data.
                </div>
              )}
            </CardContent>
          </Card>
        </div>

        {/* Row 2: Axis breakdown + Confidence distribution */}
        <div className="grid gap-4 md:grid-cols-2">
          {/* Axis stacked bar */}
          <Card>
            <CardHeader>
              <CardTitle className="text-sm font-medium">Outcomes by Axis</CardTitle>
            </CardHeader>
            <CardContent>
              {axisOutcomeData.length > 0 ? (
                <ResponsiveContainer width="100%" height={250}>
                  <BarChart data={axisOutcomeData}>
                    <CartesianGrid strokeDasharray="3 3" opacity={0.1} />
                    <XAxis dataKey="axis" fontSize={11} />
                    <YAxis fontSize={10} width={30} />
                    <Tooltip />
                    <Legend />
                    <Bar dataKey="pass" stackId="a" fill="#0070f3" name="Pass" />
                    <Bar dataKey="edit" stackId="a" fill="#f5a623" name="Edit" />
                    <Bar dataKey="block" stackId="a" fill="#ee0000" name="Block" />
                    <Bar dataKey="escalate" stackId="a" fill="#7928ca" name="Escalate" />
                  </BarChart>
                </ResponsiveContainer>
              ) : (
                <div className="flex h-[250px] items-center justify-center text-sm text-muted-foreground">
                  No axis data.
                </div>
              )}
            </CardContent>
          </Card>

          {/* Confidence distribution */}
          <Card>
            <CardHeader>
              <CardTitle className="text-sm font-medium">Confidence Distribution</CardTitle>
            </CardHeader>
            <CardContent>
              <ResponsiveContainer width="100%" height={250}>
                <BarChart data={confidenceDistribution}>
                  <CartesianGrid strokeDasharray="3 3" opacity={0.1} />
                  <XAxis dataKey="range" fontSize={10} />
                  <YAxis fontSize={10} width={30} />
                  <Tooltip />
                  <Bar dataKey="count" fill="#50e3c2" radius={[4, 4, 0, 0]} name="Verdicts" />
                </BarChart>
              </ResponsiveContainer>
            </CardContent>
          </Card>
        </div>

        {/* Row 3: Model usage + App breakdown */}
        <div className="grid gap-4 md:grid-cols-2">
          <Card>
            <CardHeader>
              <CardTitle className="text-sm font-medium">Top Models by Volume</CardTitle>
            </CardHeader>
            <CardContent>
              {modelData.length > 0 ? (
                <ResponsiveContainer width="100%" height={250}>
                  <BarChart data={modelData} layout="vertical" margin={{ left: 10 }}>
                    <CartesianGrid strokeDasharray="3 3" opacity={0.1} />
                    <XAxis type="number" fontSize={10} />
                    <YAxis type="category" dataKey="model" fontSize={10} width={130} />
                    <Tooltip />
                    <Bar dataKey="count" fill="#3b82f6" radius={[0, 4, 4, 0]} barSize={18} name="Requests" />
                  </BarChart>
                </ResponsiveContainer>
              ) : (
                <div className="flex h-[250px] items-center justify-center text-sm text-muted-foreground">
                  No model data.
                </div>
              )}
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className="text-sm font-medium">Top Apps by Verdict Volume</CardTitle>
            </CardHeader>
            <CardContent>
              {appData.length > 0 ? (
                <ResponsiveContainer width="100%" height={250}>
                  <BarChart data={appData} layout="vertical" margin={{ left: 10 }}>
                    <CartesianGrid strokeDasharray="3 3" opacity={0.1} />
                    <XAxis type="number" fontSize={10} />
                    <YAxis type="category" dataKey="app" fontSize={10} width={130} />
                    <Tooltip />
                    <Bar dataKey="count" fill="#7928ca" radius={[0, 4, 4, 0]} barSize={18} name="Verdicts" />
                  </BarChart>
                </ResponsiveContainer>
              ) : (
                <div className="flex h-[250px] items-center justify-center text-sm text-muted-foreground">
                  No app data.
                </div>
              )}
            </CardContent>
          </Card>
        </div>

        {/* Row 4: Detection Quality + Feedback Loop */}
        <div className="grid gap-4 md:grid-cols-2">
          <Card>
            <CardHeader>
              <CardTitle className="text-sm font-medium">Detection Quality by Axis</CardTitle>
            </CardHeader>
            <CardContent className="space-y-4">
              {detectionQuality ? (
                <>
                  <div className="flex items-baseline gap-3">
                    <span className={`text-3xl font-bold ${
                      detectionQuality.overall_trust_score >= 0.8 ? "text-[#50e3c2]"
                        : detectionQuality.overall_trust_score >= 0.5 ? "text-[#f5a623]"
                          : "text-[#ee0000]"
                    }`}>
                      {(detectionQuality.overall_trust_score * 100).toFixed(0)}%
                    </span>
                    <span className="text-xs text-muted-foreground">Overall Trust Score</span>
                  </div>
                  <div className="grid grid-cols-3 gap-3 text-center">
                    <div>
                      <p className="text-lg font-semibold text-[#50e3c2]">{detectionQuality.true_positives}</p>
                      <p className="text-[10px] text-muted-foreground">Confirmed</p>
                    </div>
                    <div>
                      <p className="text-lg font-semibold text-[#f5a623]">{detectionQuality.false_positives}</p>
                      <p className="text-[10px] text-muted-foreground">False Positives</p>
                    </div>
                    <div>
                      <p className="text-lg font-semibold">{detectionQuality.total_escalations_resolved}</p>
                      <p className="text-[10px] text-muted-foreground">Resolved</p>
                    </div>
                  </div>
                  {detectionQuality.checks.length > 0 && (
                    <div className="space-y-2 pt-2 border-t border-border">
                      <p className="text-[10px] text-muted-foreground uppercase tracking-wide">Precision by Axis</p>
                      {detectionQuality.checks.map((c) => (
                        <div key={c.axis} className="flex items-center gap-2">
                          <span className="text-xs capitalize w-28">{c.axis}</span>
                          <div className="flex-1 h-2 rounded-full bg-muted overflow-hidden">
                            <div className="h-full rounded-full bg-[#50e3c2]" style={{ width: `${c.precision * 100}%` }} />
                          </div>
                          <span className="text-[10px] text-muted-foreground w-10 text-right">
                            {(c.precision * 100).toFixed(0)}%
                          </span>
                        </div>
                      ))}
                    </div>
                  )}
                </>
              ) : (
                <div className="flex h-[150px] items-center justify-center text-sm text-muted-foreground">
                  No detection quality data.
                </div>
              )}
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className="text-sm font-medium">Feedback Loop Effectiveness</CardTitle>
            </CardHeader>
            <CardContent className="space-y-4">
              {feedbackData ? (
                <>
                  <div className="grid grid-cols-2 gap-4">
                    <div>
                      <p className="text-lg font-semibold">{feedbackData.patterns_promoted}</p>
                      <p className="text-[10px] text-muted-foreground">Patterns Promoted</p>
                    </div>
                    <div>
                      <p className="text-lg font-semibold">{feedbackData.threshold_adjustments}</p>
                      <p className="text-[10px] text-muted-foreground">Threshold Adjustments</p>
                    </div>
                    <div>
                      <p className="text-lg font-semibold">{feedbackData.avg_resolution_time_hours}h</p>
                      <p className="text-[10px] text-muted-foreground">Avg Resolution Time</p>
                    </div>
                    <div>
                      <p className={`text-lg font-semibold ${
                        feedbackData.improvement_indicators.escalation_rate_trend === "improving"
                          ? "text-[#50e3c2]"
                          : feedbackData.improvement_indicators.escalation_rate_trend === "worsening"
                            ? "text-[#ee0000]"
                            : "text-muted-foreground"
                      }`}>
                        {feedbackData.improvement_indicators.escalation_rate_trend === "improving" ? "↓" : "↑"}{" "}
                        {feedbackData.improvement_indicators.escalation_rate_trend}
                      </p>
                      <p className="text-[10px] text-muted-foreground">Escalation Trend</p>
                    </div>
                  </div>
                  {feedbackData.resolution_distribution && (
                    <div className="pt-2 border-t border-border space-y-2">
                      <p className="text-[10px] text-muted-foreground uppercase tracking-wide">Resolution Distribution</p>
                      <div className="flex h-3 rounded-full overflow-hidden">
                        {feedbackData.resolution_distribution.confirm_pct > 0 && (
                          <div className="bg-[#50e3c2]" style={{ width: `${feedbackData.resolution_distribution.confirm_pct}%` }} />
                        )}
                        {feedbackData.resolution_distribution.override_pct > 0 && (
                          <div className="bg-[#f5a623]" style={{ width: `${feedbackData.resolution_distribution.override_pct}%` }} />
                        )}
                        {feedbackData.resolution_distribution.dismiss_pct > 0 && (
                          <div className="bg-muted-foreground/30" style={{ width: `${feedbackData.resolution_distribution.dismiss_pct}%` }} />
                        )}
                      </div>
                      <div className="flex justify-between text-[10px] text-muted-foreground">
                        <span>Confirmed {feedbackData.resolution_distribution.confirm_pct}%</span>
                        <span>Overridden {feedbackData.resolution_distribution.override_pct}%</span>
                        <span>Dismissed {feedbackData.resolution_distribution.dismiss_pct}%</span>
                      </div>
                    </div>
                  )}
                  <div className="pt-2 border-t border-border">
                    <p className="text-xs">
                      Reviewer agreement:{" "}
                      <span className="font-medium">
                        {(feedbackData.improvement_indicators.reviewer_agreement_rate * 100).toFixed(0)}%
                      </span>
                    </p>
                  </div>
                </>
              ) : (
                <div className="flex h-[150px] items-center justify-center text-sm text-muted-foreground">
                  No feedback data.
                </div>
              )}
            </CardContent>
          </Card>
        </div>

        {/* Row 5: Per-Policy Effectiveness (all 10 policies) */}
        <Card>
          <CardHeader>
            <div className="flex items-center justify-between">
              <CardTitle className="text-sm font-medium">Policy Effectiveness Breakdown</CardTitle>
              <span className="text-xs text-muted-foreground font-mono">
                {checks.length} policies active
              </span>
            </div>
          </CardHeader>
          <CardContent>
            {checks.length > 0 ? (
              <div className="space-y-3">
                {/* Header row */}
                <div className="grid grid-cols-[1fr_60px_60px_60px_60px_60px_80px_80px_80px_80px] gap-2 text-[10px] text-muted-foreground uppercase tracking-wide font-mono px-3">
                  <span>Check</span>
                  <span className="text-center">Total</span>
                  <span className="text-center">Pass</span>
                  <span className="text-center">Edit</span>
                  <span className="text-center">Block</span>
                  <span className="text-center">Esc</span>
                  <span className="text-center">Confirmed</span>
                  <span className="text-center">Overridden</span>
                  <span className="text-center">Dismissed</span>
                  <span className="text-center">Precision</span>
                </div>
                {checks
                  .sort((a, b) => (b.blocks + b.escalates) - (a.blocks + a.escalates))
                  .map((c, idx) => {
                    const isSelected = selectedPolicy === c.check_name;
                    return (
                      <div
                        key={`${c.check_name}-${c.axis}`}
                        onClick={() => setSelectedPolicy(isSelected ? null : c.check_name)}
                        className={`grid grid-cols-[1fr_60px_60px_60px_60px_60px_80px_80px_80px_80px] gap-2 items-center rounded-md border px-3 py-2.5 text-xs cursor-pointer transition-colors ${
                          isSelected
                            ? "border-primary/40 bg-primary/5"
                            : "border-border hover:bg-accent/30"
                        }`}
                      >
                        <div className="flex items-center gap-2 min-w-0">
                          <div
                            className="h-2 w-2 shrink-0 rounded-full"
                            style={{ backgroundColor: CHECK_COLORS[idx % CHECK_COLORS.length] }}
                          />
                          <div className="min-w-0">
                            <span className="font-medium truncate block">{c.check_name}</span>
                            <span className="text-[9px] text-muted-foreground capitalize">{c.axis}</span>
                          </div>
                        </div>
                        <span className="text-center font-mono font-medium">{c.total}</span>
                        <span className="text-center font-mono text-[#0070f3]">{c.passes}</span>
                        <span className="text-center font-mono text-[#f5a623]">{c.edits}</span>
                        <span className="text-center font-mono text-[#ee0000]">{c.blocks}</span>
                        <span className="text-center font-mono text-[#7928ca]">{c.escalates}</span>
                        <span className="text-center font-mono text-[#50e3c2]">{c.confirmed}</span>
                        <span className="text-center font-mono text-[#f5a623]">{c.overridden}</span>
                        <span className="text-center font-mono text-muted-foreground">{c.dismissed}</span>
                        <div className="flex items-center justify-center gap-1">
                          <div className="w-12 h-1.5 rounded-full bg-muted overflow-hidden">
                            <div
                              className={`h-full rounded-full ${
                                c.precision >= 0.7 ? "bg-[#50e3c2]" : c.precision >= 0.4 ? "bg-[#f5a623]" : "bg-[#ee0000]"
                              }`}
                              style={{ width: `${c.precision * 100}%` }}
                            />
                          </div>
                          <span className="text-[10px] font-mono">{(c.precision * 100).toFixed(0)}%</span>
                        </div>
                      </div>
                    );
                  })}
              </div>
            ) : (
              <div className="py-8 text-center text-sm text-muted-foreground">
                No policy data available. Seed the database or send requests through the proxy.
              </div>
            )}
          </CardContent>
        </Card>

        {/* Row 6: Verdicts detail table (searchable) */}
        <Card>
          <CardHeader>
            <div className="flex items-center justify-between">
              <CardTitle className="text-sm font-medium">Recent Verdicts</CardTitle>
              <span className="text-xs text-muted-foreground font-mono">
                {filteredVerdicts.length} results
              </span>
            </div>
          </CardHeader>
          <CardContent className="p-0">
            {filteredVerdicts.length > 0 ? (
              <div className="divide-y divide-border">
                {/* Table header */}
                <div className="grid grid-cols-[100px_80px_1fr_100px_80px_80px_60px] gap-2 px-4 py-2 text-[10px] text-muted-foreground uppercase tracking-wide font-mono bg-muted/30">
                  <span>Time</span>
                  <span>Outcome</span>
                  <span>Check</span>
                  <span>Axis</span>
                  <span>Path</span>
                  <span>App</span>
                  <span className="text-right">Conf</span>
                </div>
                {filteredVerdicts.slice(0, 50).map((v) => (
                  <div
                    key={v.id}
                    className="grid grid-cols-[100px_80px_1fr_100px_80px_80px_60px] gap-2 px-4 py-2.5 text-xs hover:bg-accent/30 transition-colors"
                  >
                    <span className="text-muted-foreground font-mono">
                      {new Date(v.created_at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" })}
                    </span>
                    <Badge
                      className={`text-[10px] capitalize w-fit ${
                        v.outcome === "pass" ? "bg-[#0070f3]/10 text-[#0070f3] border-[#0070f3]/20"
                          : v.outcome === "edit" ? "bg-[#f5a623]/10 text-[#ab570a] border-[#f5a623]/20"
                            : v.outcome === "block" ? "bg-[#ee0000]/10 text-[#ee0000] border-[#ee0000]/20"
                              : "bg-[#7928ca]/10 text-[#7928ca] border-[#7928ca]/20"
                      }`}
                    >
                      {v.outcome}
                    </Badge>
                    <span className="truncate font-medium">{v.check_name}</span>
                    <span className="text-muted-foreground capitalize">{v.axis}</span>
                    <Badge
                      variant="outline"
                      className={`text-[10px] w-fit ${
                        v.path === "fast" ? "border-blue-500/50 text-blue-500" : "border-purple-500/50 text-purple-500"
                      }`}
                    >
                      {v.path}
                    </Badge>
                    <span className="text-muted-foreground truncate font-mono text-[10px]">
                      {v.app_id?.slice(0, 8) ?? "—"}
                    </span>
                    <span className="text-right font-mono text-muted-foreground">
                      {(v.confidence * 100).toFixed(0)}%
                    </span>
                  </div>
                ))}
                {filteredVerdicts.length > 50 && (
                  <div className="px-4 py-3 text-center text-xs text-muted-foreground border-t border-border">
                    Showing 50 of {filteredVerdicts.length} verdicts. Use filters to narrow results.
                  </div>
                )}
              </div>
            ) : (
              <div className="py-12 text-center text-sm text-muted-foreground">
                No verdicts match your current filters.
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </DashboardShell>
  );
}

// ─── Mini Stat Card ────────────────────────────────────────────────────────
function MiniStatCard({
  title,
  value,
  color,
  loading,
}: {
  title: string;
  value: string | number;
  color?: string;
  loading?: boolean;
}) {
  return (
    <Card className="shadow-vercel-sm">
      <CardContent className="py-3 px-4">
        <p className="text-[10px] text-muted-foreground uppercase tracking-wide font-mono">{title}</p>
        {loading ? (
          <div className="h-6 w-16 mt-1 animate-pulse rounded bg-muted" />
        ) : (
          <p className={`text-xl font-semibold tracking-tight-brand mt-0.5 ${color ?? "text-foreground"}`}>
            {value}
          </p>
        )}
      </CardContent>
    </Card>
  );
}
