"use client";

import { useQuery, useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { DashboardShell } from "@/components/layout/dashboard-shell";
import { Card, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Select } from "@/components/ui/select";
import { fetchApi } from "@/lib/api";
import { copyText } from "@/lib/clipboard";

const API_BASE = process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080";

interface AuditRecord {
  id: string;
  call_id: string;
  verdict_id: string;
  action_taken: string;
  record_hash: string;
  prev_hash: string;
  metadata?: { axis?: string; check_name?: string; app_id?: string } | string;
  created_at: string;
}

interface AuditResponse {
  records: AuditRecord[];
  total: number;
  next_cursor: string | null;
}

interface VerifyResult {
  valid: boolean;
  records_checked: number;
  first_broken_at: string | null;
}

export default function AuditPage() {
  const [filters, setFilters] = useState({
    app_id: "",
    outcome: "",
    axis: "",
    limit: 25,
    cursor: "",
  });
  const [expanded, setExpanded] = useState<string | null>(null);

  const { data, isLoading } = useQuery<AuditResponse>({
    queryKey: ["audit", filters],
    queryFn: () => {
      const params = new URLSearchParams();
      if (filters.app_id) params.set("app_id", filters.app_id);
      if (filters.outcome) params.set("outcome", filters.outcome);
      if (filters.axis) params.set("axis", filters.axis);
      params.set("limit", String(filters.limit));
      if (filters.cursor) params.set("cursor", filters.cursor);
      return fetchApi<AuditResponse>(`/api/v1/audit?${params.toString()}`);
    },
    refetchInterval: 10000,
  });

  const verifyMutation = useMutation({
    mutationFn: () => fetchApi<VerifyResult>("/api/v1/audit/verify"),
  });

  const handleExport = (format: "csv" | "json") => {
    const params = new URLSearchParams();
    if (filters.app_id) params.set("app_id", filters.app_id);
    if (filters.outcome) params.set("outcome", filters.outcome);
    params.set("format", format);
    window.open(`${API_BASE}/api/v1/audit/export?${params.toString()}`, "_blank");
  };

  const records = data?.records ?? [];
  const nextCursor = data?.next_cursor;
  const total = data?.total ?? 0;

  const loadMore = () => {
    if (nextCursor) {
      setFilters((f) => ({ ...f, cursor: nextCursor }));
    }
  };

  const resetFilters = () => {
    setFilters({ app_id: "", outcome: "", axis: "", limit: 25, cursor: "" });
  };

  return (
    <DashboardShell>
      <div className="space-y-4">
        <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-3">
          <div>
            <h2 className="text-2xl font-bold tracking-tight">Audit Trail</h2>
            <p className="text-muted-foreground">
              Tamper-evident, hash-chained record of all decisions.
            </p>
          </div>
          <div className="flex items-center gap-2">
            <button
              onClick={() => verifyMutation.mutate()}
              disabled={verifyMutation.isPending}
              className="rounded-md border border-border px-3 py-1.5 text-xs font-medium hover:bg-accent transition-colors disabled:opacity-50"
            >
              {verifyMutation.isPending ? "Verifying..." : "Verify Integrity"}
            </button>
            <button
              onClick={() => handleExport("csv")}
              className="rounded-md border border-border px-3 py-1.5 text-xs font-medium hover:bg-accent transition-colors"
            >
              Export CSV
            </button>
            <button
              onClick={() => handleExport("json")}
              className="rounded-md border border-border px-3 py-1.5 text-xs font-medium hover:bg-accent transition-colors"
            >
              Export JSON
            </button>
          </div>
        </div>

        {/* Verify result */}
        {verifyMutation.data && (
          <Card
            className={
              verifyMutation.data.valid
                ? "border-green-500/30"
                : "border-red-500/30"
            }
          >
            <CardContent className="py-3 flex items-center gap-3">
              <div
                className={`h-3 w-3 rounded-full ${verifyMutation.data.valid ? "bg-green-500" : "bg-red-500"}`}
              />
              <span className="text-sm">
                {verifyMutation.data.valid
                  ? `Chain verified: ${verifyMutation.data.records_checked} records intact`
                  : `Chain BROKEN at record ${verifyMutation.data.first_broken_at}`}
              </span>
            </CardContent>
          </Card>
        )}

        {/* Filters */}
        <div className="flex flex-wrap items-center gap-3">
          <Select
            value={filters.outcome}
            onValueChange={(v) => setFilters((f) => ({ ...f, outcome: v }))}
            placeholder="All Outcomes"
            options={[
              { value: "pass", label: "Pass" },
              { value: "edit", label: "Edit" },
              { value: "block", label: "Block" },
              { value: "escalate", label: "Escalate" },
            ]}
          />
          <Select
            value={filters.axis}
            onValueChange={(v) => setFilters((f) => ({ ...f, axis: v }))}
            placeholder="All Axes"
            options={[
              { value: "performance", label: "Performance" },
              { value: "cost", label: "Cost" },
              { value: "responsibility", label: "Responsibility" },
            ]}
          />
          <input
            type="text"
            placeholder="App ID..."
            value={filters.app_id}
            onChange={(e) =>
              setFilters((f) => ({ ...f, app_id: e.target.value }))
            }
            className="h-8 w-48 rounded-md border border-input bg-background px-2 text-xs"
          />
          <span className="self-center text-xs text-muted-foreground">
            {records.length} / {total} records
          </span>
          {filters.cursor && (
            <button
              onClick={resetFilters}
              className="text-xs text-muted-foreground hover:text-foreground underline"
            >
              Reset pagination
            </button>
          )}
        </div>

        {/* Hash Chain Visual */}
        {records.length > 1 && (
          <Card className="bg-muted/20">
            <CardContent className="py-3 px-4">
              <div className="flex items-center gap-1 overflow-x-auto">
                {records.slice(0, 20).map((rec, i) => (
                  <div key={rec.id} className="flex items-center">
                    <div
                      className={`group relative h-5 w-5 rounded-sm border flex items-center justify-center cursor-pointer text-[7px] font-mono transition-colors ${
                        rec.action_taken === "pass"
                          ? "border-green-500/40 bg-green-500/10 text-green-500"
                          : rec.action_taken === "block"
                            ? "border-red-500/40 bg-red-500/10 text-red-500"
                            : rec.action_taken === "escalate"
                              ? "border-purple-500/40 bg-purple-500/10 text-purple-500"
                              : "border-blue-500/40 bg-blue-500/10 text-blue-500"
                      }`}
                      onClick={() => setExpanded(expanded === rec.id ? null : rec.id)}
                    >
                      {i + 1}
                      <div className="absolute bottom-6 left-1/2 -translate-x-1/2 hidden group-hover:block z-10 whitespace-nowrap rounded bg-popover border border-border px-2 py-1 text-[8px] font-mono shadow-md">
                        {rec.record_hash.slice(0, 16)}…
                      </div>
                    </div>
                    {i < Math.min(records.length, 20) - 1 && (
                      <div className="w-2 h-px bg-border" />
                    )}
                  </div>
                ))}
                {records.length > 20 && (
                  <span className="text-[9px] text-muted-foreground ml-1">+{records.length - 20}</span>
                )}
              </div>
            </CardContent>
          </Card>
        )}

        {/* Records table */}
        <Card>
          <CardContent className="p-0">
            {isLoading ? (
              <div className="divide-y divide-border">
                {Array.from({ length: 5 }).map((_, i) => (
                  <div key={i} className="px-4 py-3 animate-pulse">
                    <div className="flex items-center gap-3">
                      <div className="h-3 w-16 rounded bg-muted" />
                      <div className="h-5 w-14 rounded bg-muted" />
                      <div className="h-5 w-16 rounded bg-muted" />
                      <div className="h-3 flex-1 rounded bg-muted" />
                      <div className="h-3 w-16 rounded bg-muted" />
                    </div>
                  </div>
                ))}
              </div>
            ) : records.length === 0 ? (
              <div className="p-8 text-center text-sm text-muted-foreground">
                No audit records found.
              </div>
            ) : (
              <>
                <div className="divide-y divide-border">
                  {records.map((rec) => (
                    <AuditRow
                      key={rec.id}
                      data={rec}
                      expanded={expanded === rec.id}
                      onToggle={() =>
                        setExpanded(expanded === rec.id ? null : rec.id)
                      }
                    />
                  ))}
                </div>
                {nextCursor && (
                  <div className="p-3 text-center border-t border-border">
                    <button
                      onClick={loadMore}
                      className="text-xs text-primary hover:underline"
                    >
                      Load more records →
                    </button>
                  </div>
                )}
              </>
            )}
          </CardContent>
        </Card>
      </div>
    </DashboardShell>
  );
}

function AuditRow({
  data,
  expanded,
  onToggle,
}: {
  data: AuditRecord;
  expanded: boolean;
  onToggle: () => void;
}) {
  const outcomeColors: Record<string, string> = {
    pass: "text-green-500",
    edit: "text-yellow-500",
    block: "text-red-500",
    escalate: "text-orange-500",
  };

  const meta = typeof data.metadata === "string"
    ? JSON.parse(data.metadata)
    : data.metadata ?? {};
  const axis = meta.axis ?? "";
  const checkName = meta.check_name ?? data.action_taken;

  return (
    <div
      className="px-4 py-3 hover:bg-accent/30 cursor-pointer transition-colors"
      onClick={onToggle}
    >
      <div className="flex items-center gap-3">
        <span className="text-xs font-mono text-muted-foreground w-20 shrink-0">
          {new Date(data.created_at).toLocaleTimeString()}
        </span>
        <Badge
          variant="outline"
          className={`text-[10px] capitalize ${outcomeColors[data.action_taken] ?? ""}`}
        >
          {data.action_taken}
        </Badge>
        {axis && (
          <Badge variant="outline" className="text-[10px] capitalize">
            {axis}
          </Badge>
        )}
        <span className="text-sm truncate flex-1">{checkName}</span>
        <span className="text-[10px] font-mono text-muted-foreground">
          {data.record_hash.slice(0, 8)}...
        </span>
      </div>

      {expanded && (
        <div className="mt-3 ml-20 space-y-2 rounded-md border border-border bg-muted/30 p-3 text-xs">
          <div className="grid grid-cols-2 gap-2">
            <div>
              <span className="text-muted-foreground">Record ID:</span>
              <code className="ml-1">{data.id}</code>
            </div>
            <div>
              <span className="text-muted-foreground">Call ID:</span>
              <code className="ml-1">{data.call_id}</code>
            </div>
            <div>
              <span className="text-muted-foreground">Verdict ID:</span>
              <code className="ml-1">{data.verdict_id}</code>
            </div>
            {meta.app_id && (
              <div>
                <span className="text-muted-foreground">App ID:</span>
                <code className="ml-1">{meta.app_id}</code>
              </div>
            )}
          </div>
          <div className="border-t border-border pt-2">
            <span className="text-muted-foreground">Hash Chain:</span>
            <div className="mt-1 font-mono text-[10px] space-y-0.5">
              <p className="flex items-center gap-1">
                <span className="text-muted-foreground">prev:</span>
                <code className="break-all">{data.prev_hash}</code>
                <button
                  onClick={(e) => { e.stopPropagation(); void copyText(data.prev_hash); }}
                  className="text-muted-foreground/50 hover:text-foreground shrink-0"
                  title="Copy"
                >
                  📋
                </button>
              </p>
              <p className="flex items-center gap-1">
                <span className="text-muted-foreground">curr:</span>
                <code className="break-all">{data.record_hash}</code>
                <button
                  onClick={(e) => { e.stopPropagation(); void copyText(data.record_hash); }}
                  className="text-muted-foreground/50 hover:text-foreground shrink-0"
                  title="Copy"
                >
                  📋
                </button>
              </p>
            </div>
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
