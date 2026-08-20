"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { DashboardShell } from "@/components/layout/dashboard-shell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";

const API_BASE = process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080";

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

type OutcomeFilter = "all" | "pass" | "edit" | "block" | "escalate";
type AxisFilter = "all" | "performance" | "cost" | "responsibility";

export default function LiveStreamPage() {
  const [verdicts, setVerdicts] = useState<SseVerdict[]>([]);
  const [connected, setConnected] = useState(false);
  const [paused, setPaused] = useState(false);
  const [outcomeFilter, setOutcomeFilter] = useState<OutcomeFilter>("all");
  const [axisFilter, setAxisFilter] = useState<AxisFilter>("all");
  const [expanded, setExpanded] = useState<string | null>(null);
  const eventSourceRef = useRef<EventSource | null>(null);
  const bufferRef = useRef<SseVerdict[]>([]);

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
          bufferRef.current = [data, ...bufferRef.current].slice(0, 200);
          if (!paused) {
            setVerdicts((prev) => [data, ...prev].slice(0, 200));
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
    if (outcomeFilter !== "all" && v.verdict.outcome !== outcomeFilter)
      return false;
    if (axisFilter !== "all" && v.verdict.axis !== axisFilter) return false;
    return true;
  });

  return (
    <DashboardShell>
      <div className="space-y-4">
        {/* Header with controls */}
        <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-3">
          <div>
            <h2 className="text-2xl font-bold tracking-tight">Live Stream</h2>
            <p className="text-muted-foreground">
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
        <div className="flex flex-wrap gap-2">
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
          <div className="ml-auto text-xs text-muted-foreground self-center">
            {filteredVerdicts.length} verdicts
            {paused && " (paused)"}
          </div>
        </div>

        {/* Stream */}
        <Card>
          <CardContent className="p-0">
            {filteredVerdicts.length === 0 ? (
              <div className="flex flex-col items-center justify-center py-16 text-center">
                <div className="h-12 w-12 rounded-full bg-muted flex items-center justify-center mb-3">
                  <StreamIcon className="h-6 w-6 text-muted-foreground" />
                </div>
                <p className="text-sm text-muted-foreground">
                  {connected
                    ? "Waiting for verdicts..."
                    : "Connecting to SSE endpoint..."}
                </p>
                <p className="text-xs text-muted-foreground/60 mt-1">
                  Send requests through the proxy to see live verdicts here.
                </p>
              </div>
            ) : (
              <div className="divide-y divide-border max-h-[calc(100vh-300px)] overflow-y-auto">
                {filteredVerdicts.map((v) => (
                  <VerdictRow
                    key={v.verdict.id}
                    data={v}
                    expanded={expanded === v.verdict.id}
                    onToggle={() =>
                      setExpanded(
                        expanded === v.verdict.id ? null : v.verdict.id
                      )
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
  data: SseVerdict;
  expanded: boolean;
  onToggle: () => void;
}) {
  const v = data.verdict;
  const time = new Date(data.timestamp).toLocaleTimeString();

  return (
    <div
      className="px-4 py-3 hover:bg-accent/30 cursor-pointer transition-colors"
      onClick={onToggle}
    >
      <div className="flex items-center gap-3">
        <OutcomeDot outcome={v.outcome} />
        <span className="text-xs font-mono text-muted-foreground w-16 shrink-0">
          {time}
        </span>
        <span className="text-sm font-medium truncate flex-1">
          {v.check_name}
        </span>
        <Badge variant="outline" className="text-[10px] capitalize">
          {v.axis}
        </Badge>
        <Badge
          variant="outline"
          className={`text-[10px] ${
            v.path === "fast"
              ? "border-blue-500/50 text-blue-500"
              : "border-purple-500/50 text-purple-500"
          }`}
        >
          {v.path}
        </Badge>
        <OutcomeBadge outcome={v.outcome} />
        <span className="text-[10px] text-muted-foreground w-10 text-right">
          {(v.confidence * 100).toFixed(0)}%
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
                {v.call_id.slice(0, 8)}...
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
              <span className="ml-1">{(v.confidence * 100).toFixed(1)}%</span>
            </div>
          </div>
          <div className="text-xs">
            <span className="text-muted-foreground">Reason:</span>
            <p className="mt-1 text-foreground">{v.reason}</p>
          </div>
        </div>
      )}
    </div>
  );
}

function OutcomeDot({ outcome }: { outcome: string }) {
  const colors: Record<string, string> = {
    pass: "bg-green-500",
    edit: "bg-yellow-500",
    block: "bg-red-500",
    escalate: "bg-orange-500",
  };
  return <div className={`h-2.5 w-2.5 rounded-full shrink-0 ${colors[outcome] ?? "bg-gray-500"}`} />;
}

function OutcomeBadge({ outcome }: { outcome: string }) {
  const styles: Record<string, string> = {
    pass: "bg-green-500/10 text-green-500 border-green-500/20",
    edit: "bg-yellow-500/10 text-yellow-500 border-yellow-500/20",
    block: "bg-red-500/10 text-red-500 border-red-500/20",
    escalate: "bg-orange-500/10 text-orange-500 border-orange-500/20",
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
      <span className="text-xs text-muted-foreground mr-1">{label}:</span>
      {options.map((opt) => (
        <button
          key={opt.value}
          onClick={() => onChange(opt.value)}
          className={`rounded-md px-2 py-1 text-xs transition-colors ${
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
