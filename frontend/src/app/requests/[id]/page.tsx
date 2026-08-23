"use client";

import { useEffect, useState } from "react";
import { useParams } from "next/navigation";
import { DashboardShell } from "@/components/layout/dashboard-shell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { useUser, canEditPolicies } from "@/lib/auth";

const API_BASE = process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080";

interface CallDetail {
  id: string;
  correlation_id: string;
  app_id: string;
  model: string;
  token_count_input: number | null;
  token_count_output: number | null;
  upstream_latency_ms: number | null;
  fast_path_latency_ms: number | null;
  request_payload: any;
  response_payload: any;
  created_at: string;
}

interface VerdictDetail {
  id: string;
  axis: string;
  path: string;
  outcome: string;
  confidence: number;
  reason: string;
  check_name: string;
  created_at: string;
}

interface AuditDetail {
  id: string;
  action_taken: string;
  record_hash: string;
  prev_hash: string;
  metadata: any;
  created_at: string;
}

interface EscalationDetail {
  id: string;
  status: string;
  axis: string;
  confidence: number;
  reason: string;
  assigned_to: string | null;
  resolution: string | null;
  resolution_reason: string | null;
  created_at: string;
  resolved_at: string | null;
}

interface RequestDetail {
  call: CallDetail;
  verdicts: VerdictDetail[];
  audit_records: AuditDetail[];
  escalation: EscalationDetail | null;
}

const OUTCOME_STYLES: Record<string, string> = {
  pass: "bg-[#0070f3]/10 text-[#0070f3] border-[#0070f3]/20",
  edit: "bg-[#f5a623]/10 text-[#ab570a] border-[#f5a623]/20",
  block: "bg-[#ee0000]/10 text-[#ee0000] border-[#ee0000]/20",
  escalate: "bg-[#7928ca]/10 text-[#7928ca] border-[#7928ca]/20",
};

const STATUS_STYLES: Record<string, string> = {
  open: "bg-yellow-500/10 text-yellow-500 border-yellow-500/20",
  in_review: "bg-blue-500/10 text-blue-500 border-blue-500/20",
  resolved: "bg-green-500/10 text-green-500 border-green-500/20",
};

export default function RequestDetailPage() {
  const params = useParams();
  const callId = params.id as string;
  const user = useUser();
  const isAdmin = user ? canEditPolicies(user.role) : false;

  const [data, setData] = useState<RequestDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [resolving, setResolving] = useState(false);
  const [resolveReason, setResolveReason] = useState("");

  useEffect(() => {
    if (!callId) return;
    fetch(`${API_BASE}/api/v1/requests/${callId}`)
      .then((r) => {
        if (!r.ok) throw new Error("Request not found");
        return r.json();
      })
      .then((d) => setData(d))
      .catch((e) => setError(e.message))
      .finally(() => setLoading(false));
  }, [callId]);

  const handleResolve = async (resolution: string) => {
    if (!data?.escalation) return;
    setResolving(true);
    try {
      await fetch(`${API_BASE}/api/v1/escalations/${data.escalation.id}/resolve`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ resolution, reason: resolveReason || `Admin ${resolution}` }),
      });
      // Refresh data
      const res = await fetch(`${API_BASE}/api/v1/requests/${callId}`);
      const d = await res.json();
      setData(d);
      setResolveReason("");
    } catch (e) {
      console.error("Failed to resolve:", e);
    } finally {
      setResolving(false);
    }
  };

  if (loading) {
    return (
      <DashboardShell>
        <div className="flex items-center justify-center py-20">
          <div className="h-8 w-8 animate-spin rounded-full border-2 border-primary border-t-transparent" />
        </div>
      </DashboardShell>
    );
  }

  if (error || !data) {
    return (
      <DashboardShell>
        <div className="flex flex-col items-center justify-center py-20 text-center">
          <p className="text-sm text-muted-foreground">{error || "Request not found"}</p>
        </div>
      </DashboardShell>
    );
  }

  const { call, verdicts, audit_records, escalation } = data;

  return (
    <DashboardShell>
      <div className="space-y-6">
        {/* Header */}
        <div className="flex items-start justify-between gap-4">
          <div>
            <h2 className="text-2xl font-bold tracking-tight">Request Detail</h2>
            <p className="text-muted-foreground text-sm mt-1">
              Complete breakdown of request <code className="text-xs bg-muted px-1.5 py-0.5 rounded">{call.id.slice(0, 8)}...</code>
            </p>
          </div>
          <Badge variant="outline" className="text-xs">
            {verdicts.length} verdict{verdicts.length !== 1 ? "s" : ""}
          </Badge>
        </div>

        {/* Call Overview */}
        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-medium">Intercepted Call</CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
              <div>
                <span className="text-muted-foreground text-xs">Call ID</span>
                <p className="font-mono text-xs mt-0.5 break-all">{call.id}</p>
              </div>
              <div>
                <span className="text-muted-foreground text-xs">Correlation ID</span>
                <p className="font-mono text-xs mt-0.5 break-all">{call.correlation_id}</p>
              </div>
              <div>
                <span className="text-muted-foreground text-xs">App ID</span>
                <p className="font-mono text-xs mt-0.5 break-all">{call.app_id}</p>
              </div>
              <div>
                <span className="text-muted-foreground text-xs">Model</span>
                <p className="font-medium text-xs mt-0.5">{call.model}</p>
              </div>
            </div>

            <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
              <div>
                <span className="text-muted-foreground text-xs">Input Tokens</span>
                <p className="font-mono text-xs mt-0.5">{call.token_count_input?.toLocaleString() ?? "—"}</p>
              </div>
              <div>
                <span className="text-muted-foreground text-xs">Output Tokens</span>
                <p className="font-mono text-xs mt-0.5">{call.token_count_output?.toLocaleString() ?? "—"}</p>
              </div>
              <div>
                <span className="text-muted-foreground text-xs">Upstream Latency</span>
                <p className="font-mono text-xs mt-0.5">{call.upstream_latency_ms ?? "—"}ms</p>
              </div>
              <div>
                <span className="text-muted-foreground text-xs">Fast-Path Latency</span>
                <p className="font-mono text-xs mt-0.5">{call.fast_path_latency_ms ?? "—"}ms</p>
              </div>
            </div>

            <div>
              <span className="text-muted-foreground text-xs">Timestamp</span>
              <p className="text-xs mt-0.5">{new Date(call.created_at).toLocaleString()}</p>
            </div>
          </CardContent>
        </Card>

        {/* Request / Response Payloads */}
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
          <Card>
            <CardHeader>
              <CardTitle className="text-sm font-medium">Request Payload</CardTitle>
            </CardHeader>
            <CardContent>
              <pre className="text-xs bg-muted/50 rounded-md p-4 overflow-x-auto max-h-64 overflow-y-auto font-mono">
                {call.request_payload ? JSON.stringify(call.request_payload, null, 2) : "No payload captured"}
              </pre>
            </CardContent>
          </Card>
          <Card>
            <CardHeader>
              <CardTitle className="text-sm font-medium">Response Payload</CardTitle>
            </CardHeader>
            <CardContent>
              <pre className="text-xs bg-muted/50 rounded-md p-4 overflow-x-auto max-h-64 overflow-y-auto font-mono">
                {call.response_payload ? JSON.stringify(call.response_payload, null, 2) : "No payload captured"}
              </pre>
            </CardContent>
          </Card>
        </div>

        {/* Verdicts */}
        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-medium">Verdicts ({verdicts.length})</CardTitle>
          </CardHeader>
          <CardContent className="p-0">
            {verdicts.length === 0 ? (
              <p className="text-sm text-muted-foreground p-4">No verdicts recorded</p>
            ) : (
              <div className="divide-y divide-border">
                {verdicts.map((v) => (
                  <div key={v.id} className="px-4 py-3 hover:bg-accent/30 transition-colors">
                    <div className="flex items-center gap-3">
                      <Badge className={`text-[10px] capitalize ${OUTCOME_STYLES[v.outcome] ?? ""}`}>
                        {v.outcome}
                      </Badge>
                      <span className="text-sm font-medium">{v.check_name}</span>
                      <Badge variant="outline" className="text-[10px]">{v.axis}</Badge>
                      <Badge variant="outline" className={`text-[10px] ${v.path === "fast" ? "border-blue-500/50 text-blue-500" : "border-purple-500/50 text-purple-500"}`}>
                        {v.path}
                      </Badge>
                      <span className="text-[10px] text-muted-foreground ml-auto">{(v.confidence * 100).toFixed(0)}%</span>
                    </div>
                    <p className="text-xs text-muted-foreground mt-1.5 ml-0.5">{v.reason}</p>
                    <div className="flex items-center gap-4 mt-1.5 text-[10px] text-muted-foreground/60">
                      <span>ID: {v.id.slice(0, 8)}...</span>
                      <span>{new Date(v.created_at).toLocaleString()}</span>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </CardContent>
        </Card>

        {/* Audit Trail */}
        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-medium">Audit Trail ({audit_records.length})</CardTitle>
          </CardHeader>
          <CardContent className="p-0">
            {audit_records.length === 0 ? (
              <p className="text-sm text-muted-foreground p-4">No audit records</p>
            ) : (
              <div className="divide-y divide-border">
                {audit_records.map((a) => (
                  <div key={a.id} className="px-4 py-3 hover:bg-accent/30 transition-colors">
                    <div className="flex items-center gap-3">
                      <Badge className={`text-[10px] capitalize ${OUTCOME_STYLES[a.action_taken] ?? ""}`}>
                        {a.action_taken}
                      </Badge>
                      <span className="text-[10px] text-muted-foreground">{new Date(a.created_at).toLocaleString()}</span>
                      {a.metadata?.axis && (
                        <Badge variant="outline" className="text-[10px]">{a.metadata.axis}</Badge>
                      )}
                      {a.metadata?.check_name && (
                        <span className="text-xs">{a.metadata.check_name}</span>
                      )}
                    </div>
                    <div className="mt-2 text-[10px] font-mono text-muted-foreground/60 space-y-0.5">
                      <p>Record: {a.record_hash.slice(0, 16)}...</p>
                      <p>Prev:   {a.prev_hash.slice(0, 16)}...</p>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </CardContent>
        </Card>

        {/* Escalation Case */}
        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-medium">Escalation Case</CardTitle>
          </CardHeader>
          <CardContent>
            {!escalation ? (
              <p className="text-sm text-muted-foreground">No escalation case for this request</p>
            ) : (
              <div className="space-y-4">
                <div className="grid grid-cols-2 md:grid-cols-3 gap-4 text-sm">
                  <div>
                    <span className="text-muted-foreground text-xs">Status</span>
                    <div className="mt-1">
                      <Badge className={`text-[10px] capitalize ${STATUS_STYLES[escalation.status] ?? ""}`}>
                        {escalation.status.replace("_", " ")}
                      </Badge>
                    </div>
                  </div>
                  <div>
                    <span className="text-muted-foreground text-xs">Axis</span>
                    <p className="text-xs mt-1">{escalation.axis}</p>
                  </div>
                  <div>
                    <span className="text-muted-foreground text-xs">Confidence</span>
                    <p className="text-xs mt-1">{(escalation.confidence * 100).toFixed(1)}%</p>
                  </div>
                  <div className="col-span-full">
                    <span className="text-muted-foreground text-xs">Reason</span>
                    <p className="text-xs mt-1">{escalation.reason}</p>
                  </div>
                  {escalation.resolution && (
                    <div>
                      <span className="text-muted-foreground text-xs">Resolution</span>
                      <p className="text-xs mt-1 capitalize">{escalation.resolution}</p>
                    </div>
                  )}
                  {escalation.resolution_reason && (
                    <div className="col-span-2">
                      <span className="text-muted-foreground text-xs">Resolution Reason</span>
                      <p className="text-xs mt-1">{escalation.resolution_reason}</p>
                    </div>
                  )}
                  {escalation.resolved_at && (
                    <div>
                      <span className="text-muted-foreground text-xs">Resolved At</span>
                      <p className="text-xs mt-1">{new Date(escalation.resolved_at).toLocaleString()}</p>
                    </div>
                  )}
                </div>

                {/* Admin Actions */}
                {isAdmin && escalation.status !== "resolved" && (
                  <div className="border-t border-border pt-4 space-y-3">
                    <p className="text-xs font-medium text-muted-foreground">Admin Actions</p>
                    <input
                      type="text"
                      placeholder="Resolution reason (optional)"
                      value={resolveReason}
                      onChange={(e) => setResolveReason(e.target.value)}
                      className="w-full rounded-md border border-border bg-background px-3 py-2 text-sm"
                    />
                    <div className="flex gap-2">
                      <button
                        onClick={() => handleResolve("confirm")}
                        disabled={resolving}
                        className="rounded-lg bg-green-500 px-4 py-2 text-xs font-semibold text-white hover:bg-green-400 disabled:opacity-50 transition-colors"
                      >
                        Confirm
                      </button>
                      <button
                        onClick={() => handleResolve("override")}
                        disabled={resolving}
                        className="rounded-lg bg-yellow-500 px-4 py-2 text-xs font-semibold text-white hover:bg-yellow-400 disabled:opacity-50 transition-colors"
                      >
                        Override
                      </button>
                      <button
                        onClick={() => handleResolve("dismiss")}
                        disabled={resolving}
                        className="rounded-lg bg-muted px-4 py-2 text-xs font-semibold text-muted-foreground hover:bg-accent disabled:opacity-50 transition-colors"
                      >
                        Dismiss
                      </button>
                    </div>
                  </div>
                )}
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </DashboardShell>
  );
}
