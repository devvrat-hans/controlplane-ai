"use client";

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useState, useEffect } from "react";
import { DashboardShell } from "@/components/layout/dashboard-shell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { fetchApi } from "@/lib/api";
import { useUser, canResolveEscalations } from "@/lib/auth";

const API_BASE = process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080";

interface RequestDetail {
  call: {
    id: string;
    model: string;
    request_payload: unknown;
    response_payload: unknown;
    token_count_input: number | null;
    token_count_output: number | null;
    upstream_latency_ms: number | null;
    created_at: string;
  };
  verdicts: Array<{
    id: string;
    axis: string;
    path: string;
    outcome: string;
    confidence: number;
    reason: string;
    check_name: string;
  }>;
}

interface SessionThread {
  session_id: string | null;
  turns: Array<{
    id: string;
    model: string;
    request_payload: unknown;
    response_payload: unknown;
    created_at: string;
  }>;
  total_turns: number;
}

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

const PAGE_SIZE_OPTIONS = [25, 50, 100, 200] as const;

export default function EscalationsPage() {
  const user = useUser();
  const canResolve = user ? canResolveEscalations(user.role) : false;
  const [tab, setTab] = useState<"open" | "resolved">("open");
  const [selected, setSelected] = useState<string | null>(null);
  const [resolvedToast, setResolvedToast] = useState<string | null>(null);
  const [pageSize, setPageSize] = useState<number>(25);
  const [offset, setOffset] = useState(0);

  const queryClient = useQueryClient();

  // Reset pagination when switching tabs
  useEffect(() => { setOffset(0); }, [tab]);

  const { data, isLoading } = useQuery<EscalationListResponse>({
    queryKey: ["escalations", tab, pageSize, offset],
    queryFn: () =>
      fetchApi<EscalationListResponse>(
        `/api/v1/escalations?status=${tab === "open" ? "all_open" : "resolved"}&limit=${pageSize}&offset=${offset}`
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
      setResolvedToast("Case resolved — your decision has been recorded as a precedent for future calls.");
      setTimeout(() => setResolvedToast(null), 5000);
    },
  });

  const escalations = data?.escalations ?? [];
  const selectedCase = escalations.find((e) => e.id === selected);

  // Keyboard navigation: arrow keys to move between cases
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (escalations.length === 0) return;
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;

      const currentIndex = escalations.findIndex((esc) => esc.id === selected);
      if (e.key === "ArrowDown" || e.key === "j") {
        e.preventDefault();
        const nextIndex = currentIndex < escalations.length - 1 ? currentIndex + 1 : 0;
        setSelected(escalations[nextIndex].id);
      } else if (e.key === "ArrowUp" || e.key === "k") {
        e.preventDefault();
        const prevIndex = currentIndex > 0 ? currentIndex - 1 : escalations.length - 1;
        setSelected(escalations[prevIndex].id);
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [escalations, selected]);

  return (
    <DashboardShell>
      <div className="space-y-4">
        {resolvedToast && (
          <div className="rounded-md border border-emerald-500/30 bg-emerald-500/10 px-4 py-3 text-sm text-emerald-600 flex items-center gap-2">
            <svg className="h-4 w-4 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"><path d="M20 6 9 17l-5-5"/></svg>
            {resolvedToast}
          </div>
        )}
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
                  <div className="p-8 text-center">
                    <div className="mx-auto mb-3 h-12 w-12 rounded-full bg-emerald-500/10 flex items-center justify-center">
                      <svg className="h-6 w-6 text-emerald-500" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                        <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14" />
                        <polyline points="22 4 12 14.01 9 11.01" />
                      </svg>
                    </div>
                    <p className="text-sm font-medium">All clear</p>
                    <p className="text-xs text-muted-foreground mt-1">
                      {tab === "open"
                        ? "No open escalations — the system is running smoothly."
                        : "No resolved cases yet."}
                    </p>
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

            {/* Pagination */}
            {(data?.total ?? 0) > 0 && (
              <div className="flex items-center justify-between px-1 py-2 text-xs text-muted-foreground">
                <span>
                  Showing {offset + 1}&ndash;{Math.min(offset + pageSize, data?.total ?? 0)} of {data?.total ?? 0}
                </span>
                <div className="flex items-center gap-3">
                  <span className="flex items-center gap-1.5">
                    Per page:
                    <select
                      value={pageSize}
                      onChange={(e) => { setPageSize(Number(e.target.value)); setOffset(0); }}
                      className="rounded border border-border bg-background px-1.5 py-0.5 text-xs"
                    >
                      {PAGE_SIZE_OPTIONS.map((s) => (
                        <option key={s} value={s}>{s}</option>
                      ))}
                    </select>
                  </span>
                  <div className="flex gap-1">
                    <button
                      disabled={offset === 0}
                      onClick={() => setOffset(Math.max(0, offset - pageSize))}
                      className="rounded border border-border px-2 py-0.5 hover:bg-accent disabled:opacity-30 disabled:cursor-not-allowed"
                    >
                      Prev
                    </button>
                    <button
                      disabled={offset + pageSize >= (data?.total ?? 0)}
                      onClick={() => setOffset(offset + pageSize)}
                      className="rounded border border-border px-2 py-0.5 hover:bg-accent disabled:opacity-30 disabled:cursor-not-allowed"
                    >
                      Next
                    </button>
                  </div>
                </div>
              </div>
            )}
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
                canResolve={canResolve}
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

function SessionReplay({
  turns,
  currentCallId,
}: {
  turns: SessionThread["turns"];
  currentCallId: string;
}) {
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  const toggle = (id: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  };

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <span className="text-xs text-muted-foreground">
          Session Replay ({turns.length} turns)
        </span>
        <button
          onClick={() => setExpanded(expanded.size === turns.length ? new Set() : new Set(turns.map(t => t.id)))}
          className="text-[10px] text-muted-foreground hover:text-foreground"
        >
          {expanded.size === turns.length ? "Collapse all" : "Expand all"}
        </button>
      </div>
      <div className="max-h-64 overflow-y-auto space-y-1.5 rounded-md border border-border p-2">
        {turns.map((turn, i) => {
          const turnUser = extractUserMessage(turn.request_payload);
          const turnAssistant = extractAssistantMessage(turn.response_payload);
          const isCurrent = turn.id === currentCallId;
          const isExpanded = expanded.has(turn.id);
          return (
            <div
              key={turn.id}
              className={`text-[11px] rounded p-1.5 cursor-pointer transition-colors ${
                isCurrent
                  ? "bg-orange-500/10 border border-orange-500/20"
                  : "bg-muted/20 hover:bg-muted/40"
              }`}
              onClick={() => toggle(turn.id)}
            >
              <div className="flex items-center gap-2">
                <span className="text-muted-foreground font-mono">T{i + 1}</span>
                {isCurrent && <Badge variant="destructive" className="text-[8px] px-1 py-0">flagged</Badge>}

              </div>
              {turnUser && (
                <p className={`text-blue-400 mt-0.5 ${isExpanded ? "whitespace-pre-wrap break-words" : "truncate"}`}>
                  Q: {turnUser}
                </p>
              )}
              {turnAssistant && (
                <p className={`text-green-400 mt-0.5 ${isExpanded ? "whitespace-pre-wrap break-words" : "truncate"}`}>
                  A: {turnAssistant}
                </p>
              )}
            </div>
          );
        })}
      </div>
    </div>
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
  const priority = getPriority(data.axis, data.confidence);

  return (
    <div
      className={`flex items-center gap-3 px-4 py-3 cursor-pointer transition-colors ${
        isSelected ? "bg-accent" : "hover:bg-accent/50"
      }`}
      onClick={onClick}
    >
      <ConfidenceRing confidence={data.confidence} />
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <p className="text-sm font-medium truncate">{data.reason}</p>
          {priority === "high" && (
            <Badge className="text-[9px] bg-red-500/10 text-red-500 border-red-500/20 shrink-0">HIGH</Badge>
          )}
        </div>
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
  canResolve = true,
}: {
  data: EscalationCase;
  onResolve: (action: string, reason: string) => void;
  resolving: boolean;
  canResolve?: boolean;
}) {
  const [reason, setReason] = useState("");

  const { data: requestDetail } = useQuery<RequestDetail>({
    queryKey: ["request-detail", data.call_id],
    queryFn: () => fetchApi<RequestDetail>(`/api/v1/requests/${data.call_id}`),
    enabled: !!data.call_id,
  });

  const { data: sessionThread } = useQuery<SessionThread>({
    queryKey: ["session-thread", data.call_id],
    queryFn: () => fetchApi<SessionThread>(`/api/v1/sessions/${data.call_id}/thread`),
    enabled: !!data.call_id,
  });

  // Reviewer precedents from the RAG learning store — similar past cases and
  // how humans resolved them, shown while deciding this case.
  interface Precedent {
    id: string;
    call_id: string;
    axis: string;
    model_outcome: string;
    reviewer_action: string;
    reviewer_reason: string | null;
    score: number;
  }
  const { data: precedentsData } = useQuery<{ precedents: Precedent[] }>({
    queryKey: ["reviewer-precedents", data.id],
    queryFn: () =>
      fetchApi<{ precedents: Precedent[] }>(
        `/api/v1/feedback/precedents?escalation_id=${data.id}`
      ),
    enabled: !!data.id && data.status !== "resolved",
  });
  const precedents = precedentsData?.precedents ?? [];

  const userMessage = extractUserMessage(requestDetail?.call?.request_payload);
  const assistantMessage = extractAssistantMessage(requestDetail?.call?.response_payload);

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
            <div className="mt-0.5 flex items-center gap-2">
              <p className="font-medium">
                {(data.confidence * 100).toFixed(1)}%
              </p>
              <SessionRiskBadge
                turnCount={sessionThread?.total_turns ?? 0}
                verdictCount={requestDetail?.verdicts?.length ?? 0}
                confidence={data.confidence}
              />
            </div>
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

        {/* Question & Answer */}
        {(userMessage || assistantMessage) && (
          <div className="space-y-2">
            {userMessage && (
              <div>
                <span className="text-xs text-muted-foreground">User Message</span>
                <p className="mt-1 text-sm rounded-md border border-blue-500/20 bg-blue-500/5 p-2">
                  {userMessage}
                </p>
              </div>
            )}
            {assistantMessage && (
              <div>
                <span className="text-xs text-muted-foreground">Model Response</span>
                <p className="mt-1 text-sm rounded-md border border-green-500/20 bg-green-500/5 p-2 whitespace-pre-wrap">
                  {assistantMessage}
                </p>
              </div>
            )}
          </div>
        )}

        {/* Session Replay (multi-turn context) */}
        {sessionThread && sessionThread.total_turns > 1 && (
          <SessionReplay turns={sessionThread.turns} currentCallId={data.call_id} />
        )}

        {/* Reason */}
        <div>
          <span className="text-xs text-muted-foreground">Verdict Reason</span>
          {data.reason.includes("Compound risk") && (
            <div className="mt-1 mb-1 flex items-center gap-2">
              <Badge variant="destructive" className="text-[10px]">
                Compound Risk
              </Badge>
              <span className="text-[10px] text-muted-foreground">
                Multiple axes triggered — overlapping risk detected
              </span>
            </div>
          )}
          <p className="mt-1 text-sm rounded-md border border-border bg-muted/30 p-2">
            {data.reason}
          </p>
        </div>

        {/* All triggered checks from verdicts */}
        {requestDetail?.verdicts && requestDetail.verdicts.length > 1 && (
          <div>
            <span className="text-xs text-muted-foreground">All Triggered Checks</span>
            <div className="mt-1 space-y-1">
              {requestDetail.verdicts
                .filter(v => v.outcome !== "pass")
                .map(v => (
                  <div key={v.id} className="flex items-center gap-2 text-xs rounded bg-muted/30 px-2 py-1">
                    <Badge variant="outline" className="text-[9px] capitalize">{v.axis}</Badge>
                    <span className="font-mono text-[10px]">{v.check_name}</span>
                    <span className="text-muted-foreground ml-auto">{(v.confidence * 100).toFixed(0)}%</span>
                    <Badge
                      variant={v.outcome === "block" ? "destructive" : "outline"}
                      className="text-[9px]"
                    >
                      {v.outcome}
                    </Badge>
                  </div>
                ))}
            </div>
          </div>
        )}

        {/* IDs */}
        <div className="space-y-1 text-[10px] font-mono text-muted-foreground">
          <p>Call: {data.call_id}</p>
          <p>Verdict: {data.verdict_id}</p>
          <p>App: {data.app_id}</p>
        </div>

        {/* Reviewer precedents — learned context from similar past cases */}
        {data.status !== "resolved" && (
          <div>
            <span className="text-xs text-muted-foreground">
              Similar Past Cases ({precedents.length})
            </span>
            {precedents.length === 0 ? (
              <p className="mt-1 text-[11px] text-muted-foreground/60 rounded-md border border-dashed border-border p-2">
                No similar past reviewer decisions yet — resolving this case creates the first precedent.
              </p>
            ) : (
              <div className="mt-1 space-y-1">
                {precedents.map((p) => (
                  <div
                    key={p.id}
                    className="text-[11px] rounded-md border border-border bg-muted/20 px-2 py-1.5 space-y-0.5"
                  >
                    <div className="flex items-center gap-2">
                      <Badge
                        variant={p.reviewer_action === "confirm" ? "outline" : "secondary"}
                        className={`text-[9px] capitalize ${
                          p.reviewer_action === "confirm"
                            ? "border-green-500/40 text-green-500"
                            : p.reviewer_action === "override"
                              ? "border-yellow-500/40 text-yellow-500"
                              : ""
                        }`}
                      >
                        {p.reviewer_action}ed
                      </Badge>
                      <span className="font-mono text-[10px] text-muted-foreground">
                        {Math.round(p.score * 100)}% similar
                      </span>
                      <Badge variant="outline" className="text-[9px]">{p.axis}</Badge>
                    </div>
                    {p.reviewer_reason && (
                      <p className="text-muted-foreground italic truncate">
                        "{p.reviewer_reason}"
                      </p>
                    )}
                  </div>
                ))}
                <p className="text-[10px] text-muted-foreground/50">
                  Retrieved automatically from past reviewer decisions on similar content — your resolution here becomes a precedent for future calls.
                </p>
              </div>
            )}
          </div>
        )}

        {/* Resolution actions (only for open cases + authorized users) */}
        {data.status !== "resolved" && canResolve && (
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
        {data.status !== "resolved" && !canResolve && (
          <div className="border-t border-border pt-4">
            <p className="text-sm text-muted-foreground">
              You need admin or reviewer access to resolve escalations.
            </p>
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

function SessionRiskBadge({
  turnCount,
  verdictCount,
  confidence,
}: {
  turnCount: number;
  verdictCount: number;
  confidence: number;
}) {
  // Session risk score: more turns + more verdicts + higher confidence = higher risk
  const riskScore = Math.min(100, Math.round(
    (turnCount * 15) + (verdictCount * 20) + (confidence * 30)
  ));

  if (riskScore < 30) return null;

  const color =
    riskScore >= 70
      ? "bg-red-500/10 text-red-500 border-red-500/20"
      : riskScore >= 40
        ? "bg-orange-500/10 text-orange-500 border-orange-500/20"
        : "bg-yellow-500/10 text-yellow-500 border-yellow-500/20";

  return (
    <span className={`text-[9px] font-medium px-1.5 py-0.5 rounded-full border ${color}`}>
      Risk: {riskScore}
    </span>
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

function getPriority(axis: string, confidence: number): "high" | "medium" | "low" {
  const axisWeight = axis === "responsibility" ? 3 : axis === "performance" ? 2 : 1;
  const score = axisWeight * confidence;
  if (score >= 2.0) return "high";
  if (score >= 1.0) return "medium";
  return "low";
}

function extractUserMessage(payload: unknown): string | null {
  if (!payload || typeof payload !== "object") return null;
  const p = payload as Record<string, unknown>;
  const messages = p.messages as Array<{ role: string; content: string }> | undefined;
  if (!messages) return null;
  const userMsg = messages.filter((m) => m.role === "user").pop();
  return userMsg?.content ?? null;
}

function extractAssistantMessage(payload: unknown): string | null {
  if (!payload || typeof payload !== "object") return null;
  const p = payload as Record<string, unknown>;
  // OpenAI/Ollama format: choices[0].message.content
  const choices = p.choices as Array<{ message?: { content?: string } }> | undefined;
  if (choices?.[0]?.message?.content) return choices[0].message.content;
  // Anthropic format: content[0].text
  const content = p.content as Array<{ type: string; text: string }> | undefined;
  if (content?.[0]?.text) return content[0].text;
  return null;
}
