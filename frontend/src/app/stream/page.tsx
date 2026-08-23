"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { DashboardShell } from "@/components/layout/dashboard-shell";
import { Card, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";

const API_BASE = process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080";

/** Shape returned by GET /api/v1/verdicts/recent */
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

/** Shape received from SSE events */
interface SseVerdict {
  type: string;
  correlation_id: string;
  app_id: string;
  timestamp: string;
  verdict: {
    id: string;
    call_id: string;
    axis: string;
    path: string;
    outcome: string;
    confidence: number;
    reason: string;
    check_name: string;
  };
}

/** Unified shape used by the UI */
interface StreamVerdict {
  id: string;
  call_id: string;
  correlation_id: string;
  app_id: string;
  timestamp: string;
  axis: string;
  path: string;
  outcome: string;
  confidence: number;
  reason: string;
  check_name: string;
}

function dbVerdictToStream(d: DbVerdict): StreamVerdict {
  return {
    id: d.id,
    call_id: d.call_id,
    correlation_id: d.call_id, // DB records don't have correlation_id
    app_id: d.app_id ?? "",
    timestamp: d.created_at,
    axis: d.axis,
    path: d.path,
    outcome: d.outcome,
    confidence: d.confidence,
    reason: d.reason,
    check_name: d.check_name,
  };
}

function sseVerdictToStream(s: SseVerdict): StreamVerdict {
  return {
    id: s.verdict.id,
    call_id: s.verdict.call_id,
    correlation_id: s.correlation_id,
    app_id: s.app_id,
    timestamp: s.timestamp,
    axis: s.verdict.axis,
    path: s.verdict.path,
    outcome: s.verdict.outcome,
    confidence: s.verdict.confidence,
    reason: s.verdict.reason,
    check_name: s.verdict.check_name,
  };
}

type OutcomeFilter = "all" | "pass" | "edit" | "block" | "escalate";
type AxisFilter = "all" | "performance" | "cost" | "responsibility";

export default function LiveStreamPage() {
  const [verdicts, setVerdicts] = useState<StreamVerdict[]>([]);
  const [connected, setConnected] = useState(false);
  const [loading, setLoading] = useState(true);
  const [paused, setPaused] = useState(false);
  const [outcomeFilter, setOutcomeFilter] = useState<OutcomeFilter>("all");
  const [axisFilter, setAxisFilter] = useState<AxisFilter>("all");
  const [expanded, setExpanded] = useState<string | null>(null);
  const eventSourceRef = useRef<EventSource | null>(null);
  const bufferRef = useRef<StreamVerdict[]>([]);
  const seenIdsRef = useRef<Set<string>>(new Set());

  // ─── 1. Load historical verdicts from DB on mount ───────────
  useEffect(() => {
    let cancelled = false;

    async function fetchRecent() {
      try {
        const res = await fetch(`${API_BASE}/api/v1/verdicts/recent?limit=100`);
        if (!res.ok) return;
        const data = await res.json();
        if (cancelled) return;

        const rows: DbVerdict[] = data.verdicts ?? [];
        const mapped = rows.map(dbVerdictToStream);

        // Track IDs to avoid duplicates when SSE events arrive
        mapped.forEach((v) => seenIdsRef.current.add(v.id));

        setVerdicts(mapped);
        bufferRef.current = mapped;
      } catch {
        // API might not be running — that's fine, SSE will provide data
      } finally {
        if (!cancelled) setLoading(false);
      }
    }

    fetchRecent();
    return () => { cancelled = true; };
  }, []);

  // ─── 2. Connect SSE for real-time updates ───────────────────
  const connect = useCallback(() => {
    if (eventSourceRef.current) {
      eventSourceRef.current.close();
    }

    const es = new EventSource(`${API_BASE}/api/v1/verdicts/stream`);
    eventSourceRef.current = es;

    es.onopen = () => setConnected(true);
    es.onerror = () => {
      setConnected(false);
      setTimeout(connect, 3000);
    };

    es.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data) as SseVerdict;
        if (data.type === "verdict") {
          const mapped = sseVerdictToStream(data);

          // Deduplicate — skip if already loaded from DB
          if (seenIdsRef.current.has(mapped.id)) return;
          seenIdsRef.current.add(mapped.id);

          bufferRef.current = [mapped, ...bufferRef.current].slice(0, 200);
          if (!paused) {
            setVerdicts((prev) => [mapped, ...prev].slice(0, 200));
          }
        }
      } catch {
        // ignore malformed events
      }
    };
  }, [paused]);

  useEffect(() => {
    connect();
    return () => {
      eventSourceRef.current?.close();
    };
  }, [connect]);

  const togglePause = () => {
    if (paused) {
      setVerdicts(bufferRef.current);
    }
    setPaused(!paused);
  };

  const filteredVerdicts = verdicts.filter((v) => {
    if (outcomeFilter !== "all" && v.outcome !== outcomeFilter) return false;
    if (axisFilter !== "all" && v.axis !== axisFilter) return false;
    return true;
  });

  return (
    <DashboardShell>
      <div className="space-y-4">
        {/* Header with controls */}
        <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-3">
          <div className="space-y-1">
            <h1 className="text-[24px] font-semibold tracking-tighter-brand">Live Stream.</h1>
            <p className="text-[14px] text-muted-foreground tracking-tight-brand">
              Real-time verdict feed from the proxy.
            </p>
          </div>
          <div className="flex items-center gap-3">
            <div className="flex items-center gap-2">
              <div
                className={`h-2 w-2 rounded-full ${connected ? "bg-green-500 animate-pulse" : "bg-red-500"}`}
              />
              <span className="text-xs text-muted-foreground">
                {connected ? "Connected" : "Disconnected"}
              </span>
            </div>
            <button
              onClick={togglePause}
              className={`rounded-md px-3 py-1.5 text-xs font-medium border transition-colors ${
                paused
                  ? "border-green-500/50 text-green-500 hover:bg-green-500/10"
                  : "border-yellow-500/50 text-yellow-500 hover:bg-yellow-500/10"
              }`}
            >
              {paused ? "Resume" : "Pause"}
            </button>
          </div>
        </div>

        {/* Filters */}
        <div className="space-y-2">
          <div className="flex flex-wrap items-center gap-2">
            <FilterGroup
              label="Outcome"
              value={outcomeFilter}
              onChange={(v) => setOutcomeFilter(v as OutcomeFilter)}
              options={[
                { value: "all", label: "All" },
                { value: "pass", label: "Pass" },
                { value: "edit", label: "Edit" },
                { value: "block", label: "Block" },
                { value: "escalate", label: "Escalate" },
              ]}
            />
            <div className="ml-auto text-xs text-muted-foreground">
              {filteredVerdicts.length} verdicts
              {paused && " (paused)"}
            </div>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <FilterGroup
              label="Axis"
              value={axisFilter}
              onChange={(v) => setAxisFilter(v as AxisFilter)}
              options={[
                { value: "all", label: "All" },
                { value: "performance", label: "Performance" },
                { value: "cost", label: "Cost" },
                { value: "responsibility", label: "Responsibility" },
              ]}
            />
          </div>
        </div>

        {/* Stream */}
        <Card>
          <CardContent className="p-0">
            {loading ? (
              <div className="flex flex-col items-center justify-center py-16 text-center">
                <div className="h-12 w-12 rounded-full bg-muted flex items-center justify-center mb-3 animate-pulse">
                  <StreamIcon className="h-6 w-6 text-muted-foreground" />
                </div>
                <p className="text-sm text-muted-foreground">Loading verdicts...</p>
              </div>
            ) : filteredVerdicts.length === 0 ? (
              <div className="flex flex-col items-center justify-center py-16 text-center">
                <div className="h-12 w-12 rounded-full bg-muted flex items-center justify-center mb-3">
                  <StreamIcon className="h-6 w-6 text-muted-foreground" />
                </div>
                <p className="text-sm text-muted-foreground">
                  {connected
                    ? "No verdicts match the current filters."
                    : "Connecting to SSE endpoint..."}
                </p>
                <p className="text-xs text-muted-foreground/60 mt-1">
                  Send requests through the proxy or seed the database to see verdicts here.
                </p>
              </div>
            ) : (
              <div className="divide-y divide-border max-h-[calc(100vh-300px)] overflow-y-auto">
                {filteredVerdicts.map((v) => (
                  <VerdictRow
                    key={v.id}
                    data={v}
                    expanded={expanded === v.id}
                    onToggle={() =>
                      setExpanded(expanded === v.id ? null : v.id)
                    }
                  />
                ))}
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </DashboardShell>
  );
}

function VerdictRow({
  data,
  expanded,
  onToggle,
}: {
  data: StreamVerdict;
  expanded: boolean;
  onToggle: () => void;
}) {
  const time = new Date(data.timestamp).toLocaleTimeString();

  return (
    <div
      className="px-4 py-3 hover:bg-accent/30 cursor-pointer transition-colors"
      onClick={onToggle}
    >
      <div className="flex items-center gap-3">
        <OutcomeDot outcome={data.outcome} />
        <span className="text-xs font-mono text-muted-foreground w-16 shrink-0">
          {time}
        </span>
        <span className="text-sm font-medium truncate flex-1">
          {data.check_name}
        </span>
        <Badge variant="outline" className="text-[10px] capitalize">
          {data.axis}
        </Badge>
        <Badge
          variant="outline"
          className={`text-[10px] ${
            data.path === "fast"
              ? "border-blue-500/50 text-blue-500"
              : "border-purple-500/50 text-purple-500"
          }`}
        >
          {data.path}
        </Badge>
        <OutcomeBadge outcome={data.outcome} />
        <span className="text-[10px] text-muted-foreground w-10 text-right">
          {(data.confidence * 100).toFixed(0)}%
        </span>
      </div>

      {/* Expandable detail */}
      {expanded && (
        <div className="mt-3 ml-9 space-y-2 rounded-md border border-border bg-muted/30 p-3">
          <div className="grid grid-cols-2 gap-2 text-xs">
            <div>
              <span className="text-muted-foreground">Correlation ID:</span>
              <code className="ml-1 text-[10px]">
                {data.correlation_id.slice(0, 8)}...
              </code>
            </div>
            <div>
              <span className="text-muted-foreground">Call ID:</span>
              <code className="ml-1 text-[10px]">
                {data.call_id.slice(0, 8)}...
              </code>
            </div>
            <div>
              <span className="text-muted-foreground">App ID:</span>
              <code className="ml-1 text-[10px]">
                {data.app_id.slice(0, 8)}...
              </code>
            </div>
            <div>
              <span className="text-muted-foreground">Confidence:</span>
              <span className="ml-1">{(data.confidence * 100).toFixed(1)}%</span>
            </div>
          </div>
          <div className="text-xs">
            <span className="text-muted-foreground">Reason:</span>
            <p className="mt-1 text-foreground">{data.reason}</p>
          </div>
          <a
            href={`/requests/${data.call_id}`}
            className="inline-flex items-center gap-1 text-xs text-primary hover:underline mt-2"
            onClick={(e) => e.stopPropagation()}
          >
            View full request details →
          </a>
        </div>
      )}
    </div>
  );
}

function OutcomeDot({ outcome }: { outcome: string }) {
  const colors: Record<string, string> = {
    pass: "bg-[#0070f3]",
    edit: "bg-[#f5a623]",
    block: "bg-[#ee0000]",
    escalate: "bg-[#7928ca]",
  };
  return <div className={`h-2.5 w-2.5 rounded-full shrink-0 ${colors[outcome] ?? "bg-[#888888]"}`} />;
}

function OutcomeBadge({ outcome }: { outcome: string }) {
  const styles: Record<string, string> = {
    pass: "bg-[#0070f3]/10 text-[#0070f3] border-[#0070f3]/20",
    edit: "bg-[#f5a623]/10 text-[#ab570a] border-[#f5a623]/20",
    block: "bg-[#ee0000]/10 text-[#ee0000] border-[#ee0000]/20",
    escalate: "bg-[#7928ca]/10 text-[#7928ca] border-[#7928ca]/20",
  };
  return (
    <Badge className={`text-[10px] capitalize ${styles[outcome] ?? ""}`}>
      {outcome}
    </Badge>
  );
}

function FilterGroup({
  label,
  value,
  onChange,
  options,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  options: { value: string; label: string }[];
}) {
  return (
    <div className="flex items-center gap-1">
      <span className="text-[11px] text-muted-foreground font-mono uppercase tracking-wider mr-1">
        {label}
      </span>
      {options.map((opt) => (
        <button
          key={opt.value}
          onClick={() => onChange(opt.value)}
          className={`rounded-full px-3 py-1 text-[12px] font-medium transition-colors ${
            value === opt.value
              ? "bg-primary text-primary-foreground"
              : "text-muted-foreground hover:bg-accent"
          }`}
        >
          {opt.label}
        </button>
      ))}
    </div>
  );
}

function StreamIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M22 12h-2.48a2 2 0 0 0-1.93 1.46l-2.35 8.36a.25.25 0 0 1-.48 0L9.24 2.18a.25.25 0 0 0-.48 0l-2.35 8.36A2 2 0 0 1 4.49 12H2" />
    </svg>
  );
}
