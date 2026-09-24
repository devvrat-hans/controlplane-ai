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
  latency_ms: number | null;
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

  // Learned context: precedents retrieved from past reviewer decisions
  interface Precedent {
    id: string;
    axis: string;
    reviewer_action: string;
    reviewer_reason: string | null;
    score: number;
  }
  const [precedents, setPrecedents] = useState<Precedent[]>([]);

  // Judge status, reported honestly: `off` means no judge call was made, and
  // `calibrationVersion === null` means the judge's probabilities are raw.
  interface JudgeStatus {
    mode: string;
    calibrationVersion: number | null;
    fusionEnabled: boolean;
  }
  const [judgeStatus, setJudgeStatus] = useState<JudgeStatus | null>(null);

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

    fetch(`${API_BASE}/api/v1/feedback/precedents?call_id=${callId}`)
      .then((r) => (r.ok ? r.json() : { precedents: [] }))
      .then((d) => setPrecedents(d.precedents || []))
      .catch((err) => console.error("Failed to load precedents:", err));

    fetch(`${API_BASE}/api/v1/system/config`)
      .then((r) => (r.ok ? r.json() : null))
      .then((cfg) => {
        if (!cfg) return;
        setJudgeStatus({
          mode: (cfg.decision_judge as string) ?? "off",
          calibrationVersion: (cfg.calibration_version as number | null) ?? null,
          fusionEnabled: Boolean(cfg.fusion_enabled),
        });
      })
      .catch(() => {
        /* the panel simply stays hidden when we cannot report the status honestly */
      });
  }, [callId]);

  const handleResolve = async (resolution: string) => {
    if (!data?.escalation) return;
    setResolving(true);
    try {
      await fetch(`${API_BASE}/api/v1/escalations/${data.escalation.id}/resolve`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action: resolution, reason: resolveReason || `Admin ${resolution}` }),
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

        {/* Lifecycle Timeline */}
        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-medium">Request Lifecycle</CardTitle>
          </CardHeader>
          <CardContent>
            <LifecycleTimeline call={call} verdicts={verdicts} auditRecords={audit_records} escalation={escalation} />
          </CardContent>
        </Card>

        {/* Judge panel — only when the judge actually produced readings for this call */}
        <JudgePanel verdicts={verdicts} status={judgeStatus} />

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

        {/* Learned Context — precedents from the reviewer feedback loop */}
        {precedents.length > 0 && (
          <Card>
            <CardHeader className="flex flex-row items-center justify-between">
              <CardTitle className="text-sm font-medium">Learned Context</CardTitle>
              <Badge variant="outline" className="text-[10px]">
                {precedents.length} precedent{precedents.length !== 1 ? "s" : ""} consulted
              </Badge>
            </CardHeader>
            <CardContent className="space-y-2">
              <p className="text-xs text-muted-foreground">
                Past reviewer decisions on content similar to this response, automatically retrieved from the retraining store.
              </p>
              {precedents.map((p) => (
                <div key={p.id} className="flex items-center gap-3 text-xs rounded-md border border-border bg-muted/20 px-3 py-2">
                  <Badge
                    variant="outline"
                    className={`text-[9px] capitalize shrink-0 ${
                      p.reviewer_action === "confirm"
                        ? "border-green-500/40 text-green-500"
                        : p.reviewer_action === "override"
                          ? "border-yellow-500/40 text-yellow-500"
                          : ""
                    }`}
                  >
                    {p.reviewer_action}
                  </Badge>
                  {p.reviewer_reason && (
                    <span className="italic text-muted-foreground truncate flex-1">"{p.reviewer_reason}"</span>
                  )}
                  <span className="font-mono text-[10px] text-muted-foreground shrink-0">
                    {Math.round(p.score * 100)}% similar
                  </span>
                </div>
              ))}
            </CardContent>
          </Card>
        )}

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
              <>
                {/* Visual hash chain timeline */}
                {audit_records.length > 1 && (
                  <div className="px-4 py-3 border-b border-border bg-muted/30">
                    <p className="text-[10px] text-muted-foreground mb-2 font-medium">Hash Chain</p>
                    <div className="flex items-center gap-0 overflow-x-auto">
                      {audit_records.map((a, i) => (
                        <div key={a.id} className="flex items-center">
                          <div className="group relative">
                            <div className={`h-6 w-6 rounded-full border-2 flex items-center justify-center text-[8px] font-mono cursor-pointer
                              ${a.action_taken === "pass" ? "border-green-500 bg-green-500/10" :
                                a.action_taken === "block" ? "border-red-500 bg-red-500/10" :
                                a.action_taken === "escalate" ? "border-purple-500 bg-purple-500/10" :
                                "border-blue-500 bg-blue-500/10"}`}>
                              {i + 1}
                            </div>
                            <div className="absolute bottom-8 left-1/2 -translate-x-1/2 hidden group-hover:block z-10 whitespace-nowrap rounded bg-popover border border-border px-2 py-1 text-[9px] font-mono shadow-md">
                              {a.record_hash.slice(0, 20)}…
                            </div>
                          </div>
                          {i < audit_records.length - 1 && (
                            <div className="w-8 h-px bg-border" />
                          )}
                        </div>
                      ))}
                    </div>
                  </div>
                )}

                {/* Audit record list */}
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
              </>
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

        {/* Policies, Confidence & Latency */}
        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-medium">Policies, Confidence & Latency</CardTitle>
            <p className="text-xs text-muted-foreground mt-1">
              All governance checks applied to this request with their results
            </p>
          </CardHeader>
          <CardContent className="p-0">
            <PolicyCheckTable verdicts={verdicts} fastPathLatency={call.fast_path_latency_ms} />
          </CardContent>
        </Card>
      </div>
    </DashboardShell>
  );
}

// All governance checks in the system (static definition)
const ALL_CHECKS = [
  { name: "secret_detection", axis: "responsibility", path: "fast", description: "API keys, tokens, credentials" },
  { name: "cost_cap", axis: "cost", path: "fast", description: "Token budget enforcement" },
  { name: "retry_detection", axis: "performance", path: "fast", description: "Infinite loop / retry patterns" },
  { name: "unsafe_content", axis: "responsibility", path: "fast", description: "Harmful content heuristics" },
  { name: "session_risk", axis: "responsibility", path: "fast", description: "Multi-turn risk accumulator" },
  { name: "tool_use_detection", axis: "responsibility", path: "fast", description: "Agent action detection (1.5x multiplier)" },
  { name: "prompt_injection", axis: "responsibility", path: "shadow", description: "Adversarial input detection (25+ patterns)" },
  { name: "groundedness", axis: "performance", path: "shadow", description: "Hallucination / NLI scoring" },
  { name: "bias_classification", axis: "responsibility", path: "shadow", description: "Gender, race, religion, disability bias" },
  { name: "verbosity", axis: "performance", path: "shadow", description: "Excessive response length detection" },
  { name: "semantic_pii", axis: "responsibility", path: "shadow", description: "Re-identification risk (quasi-identifiers)" },
  { name: "pii", axis: "responsibility", path: "shadow", description: "Presidio NER-based PII detection" },
  { name: "toxicity", axis: "responsibility", path: "shadow", description: "Toxic content classification" },
  { name: "hallucination", axis: "performance", path: "shadow", description: "DeepEval LLM-as-a-judge" },
];

function LifecycleTimeline({
  call,
  verdicts,
  auditRecords,
  escalation,
}: {
  call: CallDetail;
  verdicts: VerdictDetail[];
  auditRecords: AuditDetail[];
  escalation: EscalationDetail | null;
}) {
  const hasEscalation = !!escalation;
  const hasBlock = verdicts.some(v => v.outcome === "block");
  const hasEdit = verdicts.some(v => v.outcome === "edit");
  const finalOutcome = hasBlock ? "block" : hasEdit ? "edit" : escalation ? "escalate" : "pass";

  const steps = [
    { label: "Captured", sub: `${call.model}`, done: true, color: "bg-blue-500" },
    { label: "Fast-Path", sub: verdicts.filter(v => v.path === "fast").length > 0 ? `${verdicts.filter(v => v.path === "fast").length} checks` : "skipped", done: true, color: "bg-emerald-500" },
    { label: "Shadow", sub: verdicts.filter(v => v.path === "shadow").length > 0 ? `${verdicts.filter(v => v.path === "shadow").length} checks` : "pending", done: verdicts.some(v => v.path === "shadow"), color: "bg-purple-500" },
    { label: "Decision", sub: finalOutcome, done: true, color: finalOutcome === "pass" ? "bg-blue-500" : finalOutcome === "block" ? "bg-red-500" : finalOutcome === "escalate" ? "bg-purple-500" : "bg-amber-500" },
    { label: "Audit", sub: auditRecords.length > 0 ? "recorded" : "pending", done: auditRecords.length > 0, color: "bg-emerald-500" },
    { label: "Resolved", sub: escalation?.status ?? (hasEscalation ? "pending" : "n/a"), done: escalation?.status === "resolved", color: escalation?.status === "resolved" ? "bg-emerald-500" : "bg-muted" },
  ];

  return (
    <div className="flex items-center gap-0 overflow-x-auto py-2">
      {steps.map((step, i) => (
        <div key={step.label} className="flex items-center">
          <div className="group relative flex flex-col items-center">
            <div className={`h-8 w-8 rounded-full border-2 flex items-center justify-center text-[10px] font-bold ${
              step.done ? `${step.color} border-transparent text-white` : "border-border bg-muted text-muted-foreground"
            }`}>
              {i + 1}
            </div>
            <span className="text-[10px] font-medium mt-1.5 whitespace-nowrap">{step.label}</span>
            <span className="text-[9px] text-muted-foreground whitespace-nowrap capitalize">{step.sub}</span>
            <div className="absolute bottom-10 left-1/2 -translate-x-1/2 hidden group-hover:block z-10 whitespace-nowrap rounded bg-popover border border-border px-2 py-1 text-[9px] shadow-md">
              {step.label}: {step.sub}
            </div>
          </div>
          {i < steps.length - 1 && (
            <div className={`w-8 sm:w-12 h-0.5 ${step.done ? "bg-emerald-500/40" : "bg-border"} mx-1 mt-[-20px]`} />
          )}
        </div>
      ))}
    </div>
  );
}

function PolicyCheckTable({ verdicts, fastPathLatency }: { verdicts: VerdictDetail[]; fastPathLatency: number | null }) {
  // Merge static check list with actual verdicts
  const verdictMap = new Map(verdicts.map(v => [v.check_name, v]));

  // Estimate per-check latency for fast-path (total / count)
  const fastCheckCount = ALL_CHECKS.filter(c => c.path === "fast").length;
  const estimatedFastLatency = fastPathLatency && fastCheckCount > 0
    ? Math.round(fastPathLatency / fastCheckCount)
    : null;

  const rows = ALL_CHECKS.map(check => {
    const verdict = verdictMap.get(check.name);
    return {
      name: check.name,
      description: check.description,
      axis: verdict?.axis ?? check.axis,
      path: verdict?.path ?? check.path,
      confidence: verdict?.confidence ?? 0,
      outcome: verdict?.outcome ?? "pass",
      latency: verdict?.latency_ms ?? (check.path === "fast" ? estimatedFastLatency : null),
      hasVerdict: !!verdict,
    };
  });

  // Also include any verdicts with check names not in our static list
  for (const v of verdicts) {
    if (!ALL_CHECKS.find(c => c.name === v.check_name)) {
      rows.push({
        name: v.check_name,
        description: v.reason.slice(0, 50),
        axis: v.axis,
        path: v.path,
        confidence: v.confidence,
        outcome: v.outcome,
        latency: v.latency_ms,
        hasVerdict: true,
      });
    }
  }

  const triggeredCount = rows.filter(r => r.outcome !== "pass").length;
  const passedCount = rows.filter(r => r.outcome === "pass").length;

  return (
    <div className="overflow-x-auto">
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b border-border bg-muted/30">
            <th className="text-left px-4 py-3 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Check Name</th>
            <th className="text-left px-4 py-3 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Axis</th>
            <th className="text-center px-4 py-3 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Path</th>
            <th className="text-center px-4 py-3 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Confidence</th>
            <th className="text-center px-4 py-3 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Latency</th>
            <th className="text-center px-4 py-3 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Verdict</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-border">
          {rows.map((row) => (
            <tr
              key={row.name}
              className={`transition-colors ${
                row.outcome !== "pass" ? "bg-red-500/3 hover:bg-red-500/8" : "hover:bg-accent/20"
              }`}
            >
              <td className="px-4 py-3">
                <div>
                  <span className="font-medium text-xs">{row.name}</span>
                  <p className="text-[10px] text-muted-foreground mt-0.5">{row.description}</p>
                </div>
              </td>
              <td className="px-4 py-3">
                <Badge variant="outline" className="text-[10px] capitalize">{row.axis}</Badge>
              </td>
              <td className="px-4 py-3 text-center">
                <Badge
                  variant="outline"
                  className={`text-[10px] ${
                    row.path === "fast"
                      ? "border-blue-500/50 text-blue-500 bg-blue-500/5"
                      : "border-purple-500/50 text-purple-500 bg-purple-500/5"
                  }`}
                >
                  {row.path === "fast" ? "Fast Path" : "Shadow Path"}
                </Badge>
              </td>
              <td className="px-4 py-3 text-center">
                <ConfidenceBar confidence={row.confidence} isPass={row.outcome === "pass"} />
              </td>
              <td className="px-4 py-3 text-center">
                <span className="font-mono text-xs text-muted-foreground">
                  {row.latency != null ? `${row.latency}ms` : "<1ms"}
                </span>
              </td>
              <td className="px-4 py-3 text-center">
                <Badge className={`text-[10px] uppercase font-bold ${OUTCOME_STYLES[row.outcome] ?? ""}`}>
                  {row.outcome}
                </Badge>
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      {/* Summary Footer */}
      <div className="border-t border-border bg-muted/20 px-4 py-3 flex items-center justify-between">
        <div className="flex items-center gap-3">
          <span className="text-xs text-muted-foreground">
            {rows.length} checks applied
          </span>
          <Badge variant="outline" className="text-[10px] border-green-500/50 text-green-500">
            {passedCount} passed
          </Badge>
          {triggeredCount > 0 && (
            <Badge variant="outline" className="text-[10px] border-red-500/50 text-red-500">
              {triggeredCount} triggered
            </Badge>
          )}
        </div>
        <div className="flex items-center gap-4 text-xs text-muted-foreground">
          <span>
            Fast: {rows.filter(r => r.path === "fast").length}
          </span>
          <span>
            Shadow: {rows.filter(r => r.path === "shadow").length}
          </span>
          {fastPathLatency != null && (
            <span>
              Fast-path total:{" "}
              <span className="font-mono font-medium text-foreground">
                {fastPathLatency}ms
              </span>
            </span>
          )}
        </div>
      </div>
    </div>
  );
}

function ConfidenceBar({ confidence, isPass }: { confidence: number; isPass?: boolean }) {
  const pct = Math.round(confidence * 100);

  if (isPass && pct === 0) {
    return (
      <div className="flex items-center gap-2 justify-center">
        <div className="w-16 h-2 rounded-full bg-green-500/20 overflow-hidden">
          <div className="h-full rounded-full bg-green-500" style={{ width: "100%" }} />
        </div>
        <span className="font-mono text-[11px] font-medium w-8 text-right text-green-500">OK</span>
      </div>
    );
  }

  const color =
    pct >= 80
      ? "bg-[#ee0000]"
      : pct >= 60
        ? "bg-[#f5a623]"
        : pct >= 30
          ? "bg-[#0070f3]"
          : "bg-muted-foreground/40";

  return (
    <div className="flex items-center gap-2 justify-center">
      <div className="w-16 h-2 rounded-full bg-muted overflow-hidden">
        <div className={`h-full rounded-full ${color}`} style={{ width: `${pct}%` }} />
      </div>
      <span className="font-mono text-[11px] font-medium w-8 text-right">{pct}%</span>
    </div>
  );
}

/**
 * Judge panel — the calibrated decision-model readings for this call, side by side with
 * the heuristics on the same axis.
 *
 * See docs/analysis/laya-integration-plan.md §5.6 / §11.2. Two honesty rules apply here:
 *
 *  - The panel renders only when the judge actually produced readings for this call.
 *    An empty panel is not an empty state to be filled with a "enabled" badge.
 *  - `-evidence` readings are labelled as evidence: they are sub-threshold probabilities
 *    that took no action. Presenting them as findings would overstate what happened.
 */
const JUDGE_PREFIX = "laya-";
const EVIDENCE_SUFFIX = "-evidence";
/** Mirrors the decision engine's JUDGE_DISAGREEMENT_DELTA default. */
const DISAGREEMENT_DELTA = 0.4;

function JudgePanel({
  verdicts,
  status,
}: {
  verdicts: VerdictDetail[];
  status: { mode: string; calibrationVersion: number | null; fusionEnabled: boolean } | null;
}) {
  const judge = verdicts.filter((v) => v.check_name.startsWith(JUDGE_PREFIX));
  if (judge.length === 0) return null;

  const isEvidence = (v: VerdictDetail) => v.check_name.endsWith(EVIDENCE_SUFFIX);
  const actionable = judge.filter((v) => !isEvidence(v));
  const evidence = judge.filter(isEvidence);
  const heuristic = verdicts.filter((v) => !v.check_name.startsWith(JUDGE_PREFIX) && v.outcome !== "pass");

  const axes = Array.from(new Set(judge.map((v) => v.axis))).sort();

  const perAxis = axes.map((axis) => {
    const judgeP = Math.max(0, ...judge.filter((v) => v.axis === axis).map((v) => v.confidence));
    const heuristicP = Math.max(0, ...heuristic.filter((v) => v.axis === axis).map((v) => v.confidence));
    const delta = Math.abs(judgeP - heuristicP);
    return {
      axis,
      judgeP,
      heuristicP,
      delta,
      disagreement: judgeP > 0 && heuristicP > 0 && delta >= DISAGREEMENT_DELTA,
    };
  });

  const hasDisagreement = perAxis.some((a) => a.disagreement);

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between">
        <CardTitle className="text-sm font-medium">Judge Panel</CardTitle>
        <div className="flex items-center gap-2">
          <Badge variant="outline" className="text-[10px] font-mono">
            {status?.mode && status.mode !== "off" ? status.mode : "laya"}
          </Badge>
          {status && (
            <Badge variant="outline" className="text-[10px] font-mono">
              {status.calibrationVersion !== null
                ? `calibration v${status.calibrationVersion}`
                : "uncalibrated"}
            </Badge>
          )}
          {hasDisagreement && (
            <Badge className="text-[10px] bg-[#7928ca]/10 text-[#7928ca] border-[#7928ca]/20">
              disagreement
            </Badge>
          )}
        </div>
      </CardHeader>
      <CardContent className="space-y-4">
        {!status || status.mode === "off" ? (
          <p className="text-[11px] text-muted-foreground/70">
            These readings were recorded with <code>DECISION_JUDGE</code> enabled; it is not set on
            the currently running server.
          </p>
        ) : null}

        {status && status.calibrationVersion === null && (
          <p className="text-[11px] text-muted-foreground/70">
            No calibration fit exists, so these probabilities are <strong>raw</strong> model
            confidence — not calibrated ones. The calibrated fusion is therefore off and the
            deterministic aggregator produced the decision.
          </p>
        )}

        {/* Judge vs heuristic, per axis */}
        <div className="space-y-2">
          <p className="text-[11px] font-medium text-muted-foreground/70">
            Judge probability vs the strongest heuristic on the same axis
          </p>
          {perAxis.map((row) => (
            <div key={row.axis} className="flex items-center gap-3 text-xs">
              <span className="w-28 shrink-0 capitalize">{row.axis}</span>
              <span className="font-mono tabular-nums w-16 text-right" title="judge p">
                p={(row.judgeP * 100).toFixed(0)}%
              </span>
              <span className="text-muted-foreground/50">vs</span>
              <span className="font-mono tabular-nums w-16" title="strongest heuristic p">
                {(row.heuristicP * 100).toFixed(0)}%
              </span>
              <span className="text-muted-foreground/50 font-mono w-16">d={row.delta.toFixed(2)}</span>
              {row.disagreement && (
                <Badge className="text-[10px] bg-[#7928ca]/10 text-[#7928ca] border-[#7928ca]/20">
                  routed to review
                </Badge>
              )}
            </div>
          ))}
        </div>

        {/* Actionable judge verdicts */}
        {actionable.length > 0 && (
          <div className="space-y-1.5">
            <p className="text-[11px] font-medium text-muted-foreground/70">
              Judge findings ({actionable.length})
            </p>
            {actionable.map((v) => (
              <div key={v.id} className="flex items-start gap-2 text-xs">
                <Badge className={`text-[10px] capitalize shrink-0 ${OUTCOME_STYLES[v.outcome] ?? ""}`}>
                  {v.outcome}
                </Badge>
                <span className="font-mono text-[11px] shrink-0">{v.check_name}</span>
                <span className="text-muted-foreground/70 line-clamp-2">{v.reason}</span>
              </div>
            ))}
          </div>
        )}

        {/* Evidence-only readings — explicitly not findings */}
        {evidence.length > 0 && (
          <div className="space-y-1.5">
            <p className="text-[11px] font-medium text-muted-foreground/70">
              Evidence only ({evidence.length}) — sub-threshold readings that took no action
            </p>
            {evidence.map((v) => (
              <div key={v.id} className="flex items-start gap-2 text-xs opacity-70">
                <Badge variant="outline" className="text-[10px] shrink-0 font-mono">
                  {(v.confidence * 100).toFixed(0)}%
                </Badge>
                <span className="font-mono text-[11px] shrink-0">{v.check_name}</span>
                <span className="text-muted-foreground/70 line-clamp-2">{v.reason}</span>
              </div>
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
