"use client";

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { DashboardShell } from "@/components/layout/dashboard-shell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { fetchApi } from "@/lib/api";

const API_BASE = process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080";

interface EscalationCase {
  id: string;
  verdict_id: string;
  call_id: string;
  app_id: string;
  status: string;
  assigned_to: string | null;
  resolution: string | null;
  resolution_reason: string | null;
  axis: string;
  confidence: number;
  reason: string;
  created_at: string;
  resolved_at: string | null;
}

interface EscalationListResponse {
  escalations: EscalationCase[];
  total: number;
}

export default function EscalationsPage() {
  const [tab, setTab] = useState<"open" | "resolved">("open");
  const [selected, setSelected] = useState<string | null>(null);

  const queryClient = useQueryClient();

  const { data, isLoading } = useQuery<EscalationListResponse>({
    queryKey: ["escalations", tab],
    queryFn: () =>
      fetchApi<EscalationListResponse>(
        `/api/v1/escalations?status=${tab === "open" ? "open" : "resolved"}&limit=50`
      ),
    refetchInterval: 5000,
  });

  const resolveMutation = useMutation({
    mutationFn: async ({
      id,
      action,
      reason,
    }: {
      id: string;
      action: string;
      reason: string;
    }) => {
      const res = await fetch(`${API_BASE}/api/v1/escalations/${id}/resolve`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action, reason }),
      });
      if (!res.ok) throw new Error("Failed to resolve");
      return res.json();
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["escalations"] });
      setSelected(null);
    },
  });

  const escalations = data?.escalations ?? [];
  const selectedCase = escalations.find((e) => e.id === selected);

  return (
    <DashboardShell>
      <div className="space-y-4">
        <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-3">
          <div>
            <h2 className="text-2xl font-bold tracking-tight">Escalations</h2>
            <p className="text-muted-foreground">
              Human review queue for ambiguous verdicts.
            </p>
          </div>
          <div className="flex items-center gap-1 rounded-md border border-border p-0.5">
            <button
              onClick={() => setTab("open")}
              className={`rounded px-3 py-1.5 text-xs font-medium transition-colors ${
                tab === "open"
                  ? "bg-primary text-primary-foreground"
                  : "text-muted-foreground hover:text-foreground"
              }`}
            >
              Open
            </button>
            <button
              onClick={() => setTab("resolved")}
              className={`rounded px-3 py-1.5 text-xs font-medium transition-colors ${
                tab === "resolved"
                  ? "bg-primary text-primary-foreground"
                  : "text-muted-foreground hover:text-foreground"
              }`}
            >
              Resolved
            </button>
          </div>
        </div>

        <div className="grid gap-4 lg:grid-cols-3">
          {/* Cases list */}
          <div className="lg:col-span-2">
            <Card>
              <CardContent className="p-0">
                {isLoading ? (
                  <div className="p-8 text-center">
                    <div className="mx-auto h-8 w-8 animate-spin rounded-full border-2 border-primary border-t-transparent" />
                  </div>
                ) : escalations.length === 0 ? (
                  <div className="p-8 text-center text-sm text-muted-foreground">
                    {tab === "open"
                      ? "No open escalations. The system is running smoothly."
                      : "No resolved cases yet."}
                  </div>
                ) : (
                  <div className="divide-y divide-border">
                    {escalations.map((esc) => (
                      <CaseRow
                        key={esc.id}
                        data={esc}
                        isSelected={selected === esc.id}
                        onClick={() => setSelected(esc.id)}
                      />
                    ))}
                  </div>
                )}
              </CardContent>
            </Card>
          </div>

          {/* Detail panel */}
          <div>
            {selectedCase ? (
              <CaseDetail
                data={selectedCase}
                onResolve={(action, reason) =>
                  resolveMutation.mutate({
                    id: selectedCase.id,
                    action,
                    reason,
                  })
                }
                resolving={resolveMutation.isPending}
              />
            ) : (
              <Card>
                <CardContent className="py-12 text-center">
                  <p className="text-sm text-muted-foreground">
                    Select a case to view details
                  </p>
                </CardContent>
              </Card>
            )}
          </div>
        </div>
      </div>
    </DashboardShell>
  );
}

function CaseRow({
  data,
  isSelected,
  onClick,
}: {
  data: EscalationCase;
  isSelected: boolean;
  onClick: () => void;
}) {
  const age = getAge(data.created_at);

  return (
    <div
      className={`flex items-center gap-3 px-4 py-3 cursor-pointer transition-colors ${
        isSelected ? "bg-accent" : "hover:bg-accent/50"
      }`}
      onClick={onClick}
    >
      <ConfidenceRing confidence={data.confidence} />
      <div className="flex-1 min-w-0">
        <p className="text-sm font-medium truncate">{data.reason}</p>
        <div className="flex items-center gap-2 mt-0.5">
          <Badge variant="outline" className="text-[10px] capitalize">
            {data.axis}
          </Badge>
          <span className="text-[10px] text-muted-foreground">
            {data.call_id.slice(0, 8)}...
          </span>
        </div>
      </div>
      <div className="text-right shrink-0">
        <StatusBadge status={data.status} resolution={data.resolution} />
        <p className="text-[10px] text-muted-foreground mt-0.5">{age}</p>
      </div>
    </div>
  );
}

function CaseDetail({
  data,
  onResolve,
  resolving,
}: {
  data: EscalationCase;
  onResolve: (action: string, reason: string) => void;
  resolving: boolean;
}) {
  const [reason, setReason] = useState("");

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-sm font-medium">Case Detail</CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        {/* Meta */}
        <div className="grid grid-cols-2 gap-2 text-xs">
          <div>
            <span className="text-muted-foreground">Status</span>
            <div className="mt-0.5">
              <StatusBadge status={data.status} resolution={data.resolution} />
            </div>
          </div>
          <div>
            <span className="text-muted-foreground">Confidence</span>
            <p className="mt-0.5 font-medium">
              {(data.confidence * 100).toFixed(1)}%
            </p>
          </div>
          <div>
            <span className="text-muted-foreground">Axis</span>
            <p className="mt-0.5 capitalize">{data.axis}</p>
          </div>
          <div>
            <span className="text-muted-foreground">Age</span>
            <p className="mt-0.5">{getAge(data.created_at)}</p>
          </div>
        </div>

        {/* Reason */}
        <div>
          <span className="text-xs text-muted-foreground">Verdict Reason</span>
          <p className="mt-1 text-sm rounded-md border border-border bg-muted/30 p-2">
            {data.reason}
          </p>
        </div>

        {/* IDs */}
        <div className="space-y-1 text-[10px] font-mono text-muted-foreground">
          <p>Call: {data.call_id}</p>
          <p>Verdict: {data.verdict_id}</p>
          <p>App: {data.app_id}</p>
        </div>

        {/* Resolution actions (only for open cases) */}
        {data.status !== "resolved" && (
          <div className="border-t border-border pt-4 space-y-3">
            <textarea
              className="w-full rounded-md border border-input bg-background px-3 py-2 text-sm resize-none h-16"
              placeholder="Reason for resolution..."
              value={reason}
              onChange={(e) => setReason(e.target.value)}
            />
            <div className="flex gap-2">
              <button
                onClick={() => onResolve("confirm", reason)}
                disabled={resolving}
                className="flex-1 rounded-md border border-green-500/50 px-2 py-1.5 text-xs font-medium text-green-500 hover:bg-green-500/10 disabled:opacity-50 transition-colors"
              >
                Confirm
              </button>
              <button
                onClick={() => onResolve("override", reason)}
                disabled={resolving}
                className="flex-1 rounded-md border border-yellow-500/50 px-2 py-1.5 text-xs font-medium text-yellow-500 hover:bg-yellow-500/10 disabled:opacity-50 transition-colors"
              >
                Override
              </button>
              <button
                onClick={() => onResolve("dismiss", reason)}
                disabled={resolving}
                className="flex-1 rounded-md border border-muted-foreground/50 px-2 py-1.5 text-xs font-medium text-muted-foreground hover:bg-accent disabled:opacity-50 transition-colors"
              >
                Dismiss
              </button>
            </div>
            <div className="text-[10px] text-muted-foreground space-y-0.5">
              <p>
                <strong>Confirm</strong> — verdict was correct, feeds learning
              </p>
              <p>
                <strong>Override</strong> — verdict was wrong, adjusts thresholds
              </p>
              <p>
                <strong>Dismiss</strong> — false positive, marked for
                retraining
              </p>
            </div>
          </div>
        )}

        {/* Resolution details (for resolved cases) */}
        {data.status === "resolved" && data.resolution && (
          <div className="border-t border-border pt-4">
            <span className="text-xs text-muted-foreground">Resolution</span>
            <div className="mt-1 flex items-center gap-2">
              <Badge variant="outline" className="text-xs capitalize">
                {data.resolution}
              </Badge>
              {data.resolved_at && (
                <span className="text-[10px] text-muted-foreground">
                  {new Date(data.resolved_at).toLocaleString()}
                </span>
              )}
            </div>
            {data.resolution_reason && (
              <p className="mt-2 text-sm text-muted-foreground">
                {data.resolution_reason}
              </p>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function ConfidenceRing({ confidence }: { confidence: number }) {
  const pct = Math.round(confidence * 100);
  const color =
    pct >= 80
      ? "text-red-500"
      : pct >= 60
        ? "text-orange-500"
        : "text-yellow-500";

  return (
    <div className={`relative h-10 w-10 shrink-0 ${color}`}>
      <svg viewBox="0 0 36 36" className="h-full w-full -rotate-90">
        <circle
          cx="18"
          cy="18"
          r="15"
          fill="none"
          stroke="currentColor"
          strokeWidth="3"
          opacity={0.2}
        />
        <circle
          cx="18"
          cy="18"
          r="15"
          fill="none"
          stroke="currentColor"
          strokeWidth="3"
          strokeDasharray={`${pct} ${100 - pct}`}
          strokeLinecap="round"
        />
      </svg>
      <span className="absolute inset-0 flex items-center justify-center text-[9px] font-bold">
        {pct}
      </span>
    </div>
  );
}

function StatusBadge({
  status,
  resolution,
}: {
  status: string;
  resolution: string | null;
}) {
  if (status === "resolved") {
    const resColors: Record<string, string> = {
      confirm: "bg-green-500/10 text-green-500 border-green-500/20",
      override: "bg-yellow-500/10 text-yellow-500 border-yellow-500/20",
      dismiss: "bg-gray-500/10 text-gray-500 border-gray-500/20",
    };
    return (
      <Badge
        className={`text-[10px] capitalize ${resColors[resolution ?? ""] ?? ""}`}
      >
        {resolution ?? "resolved"}
      </Badge>
    );
  }

  if (status === "in_review") {
    return (
      <Badge className="text-[10px] bg-blue-500/10 text-blue-500 border-blue-500/20">
        In Review
      </Badge>
    );
  }

  return (
    <Badge className="text-[10px] bg-orange-500/10 text-orange-500 border-orange-500/20">
      Open
    </Badge>
  );
}

function getAge(dateStr: string): string {
  const diff = Date.now() - new Date(dateStr).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}
