"use client";

import { useState, useEffect } from "react";
import { useRouter } from "next/navigation";
import { useQuery } from "@tanstack/react-query";
import { DashboardShell } from "@/components/layout/dashboard-shell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Select } from "@/components/ui/select";
import { getRequests } from "@/lib/api";
import type { RequestListItem } from "@/lib/api";
import { useApp } from "@/components/providers/app-provider";

const API_BASE = process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080";

const OUTCOME_STYLES: Record<string, string> = {
  pass: "bg-[#0070f3]/10 text-[#0070f3] border-[#0070f3]/20",
  edit: "bg-[#f5a623]/10 text-[#ab570a] border-[#f5a623]/20",
  block: "bg-[#ee0000]/10 text-[#ee0000] border-[#ee0000]/20",
  escalate: "bg-[#7928ca]/10 text-[#7928ca] border-[#7928ca]/20",
};

const PAGE_SIZE_OPTIONS = [25, 100, 200, 500] as const;

export default function RequestsPage() {
  const { selectedAppId } = useApp();
  const [pageSize, setPageSize] = useState<number>(25);
  const [offset, setOffset] = useState(0);
  const [outcomeFilter, setOutcomeFilter] = useState<string>("all");
  const [search, setSearch] = useState("");
  const [modelFilter, setModelFilter] = useState<string>("all");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [compareMode, setCompareMode] = useState(false);

  const { data, isLoading } = useQuery({
    queryKey: ["requests-list", offset, search, modelFilter, outcomeFilter, pageSize, selectedAppId],
    queryFn: () => getRequests({ limit: pageSize, offset, search, model: modelFilter, outcome: outcomeFilter, app_id: selectedAppId }),
    refetchInterval: 10000,
  });

  const requests = data?.requests ?? [];
  const total = data?.total ?? 0;

  // Collect unique models from current page for the filter dropdown
  const uniqueModels = Array.from(new Set(requests.map((r) => r.model))).sort();

  return (
    <DashboardShell>
      <div className="space-y-6">
        <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-3">
          <div>
            <h2 className="text-2xl font-bold tracking-tight">Requests</h2>
            <p className="text-muted-foreground">
              Intercepted API calls through the proxy. {total} total.
            </p>
          </div>
          <div className="flex items-center gap-2 flex-wrap">
            <input
              type="text"
              value={search}
              onChange={(e) => { setSearch(e.target.value); setOffset(0); }}
              placeholder="Search app, model, id..."
              className="rounded-md border border-input bg-background px-3 py-1.5 text-xs w-48 focus:outline-none focus:ring-2 focus:ring-ring/20"
            />
            <Select
              ariaLabel="Filter by model"
              value={modelFilter}
              onValueChange={(v) => { setModelFilter(v); setOffset(0); }}
              options={[
                { value: "all", label: "All models" },
                ...uniqueModels.map((m) => ({ value: m, label: m })),
              ]}
            />
            {["all", "pass", "edit", "block", "escalate"].map((f) => (
              <button
                key={f}
                onClick={() => setOutcomeFilter(f)}
                className={`px-3 py-1.5 text-xs font-medium rounded-md transition-colors ${
                  outcomeFilter === f
                    ? "bg-primary text-primary-foreground"
                    : "text-muted-foreground hover:bg-accent"
                }`}
              >
                {f === "all" ? "All" : f.charAt(0).toUpperCase() + f.slice(1)}
              </button>
            ))}
            <a
              href={`${API_BASE}/api/v1/requests/export/csv`}
              target="_blank"
              rel="noopener noreferrer"
              className="px-3 py-1.5 text-xs font-medium rounded-md border border-border text-muted-foreground hover:bg-accent transition-colors"
            >
              ↓ CSV
            </a>
            <button
              onClick={() => { setCompareMode(!compareMode); setSelected(new Set()); }}
              className={`px-3 py-1.5 text-xs font-medium rounded-md transition-colors ${
                compareMode ? "bg-primary text-primary-foreground" : "border border-border text-muted-foreground hover:bg-accent"
              }`}
            >
              {compareMode ? `Compare (${selected.size})` : "Compare"}
            </button>
          </div>
        </div>

        {/* Compare panel */}
        {compareMode && selected.size === 2 && (
          <ComparePanel ids={Array.from(selected)} />
        )}

        <Card>
          <CardContent className="p-0">
            {isLoading ? (
              <div className="divide-y divide-border">
                {Array.from({ length: 8 }).map((_, i) => (
                  <div key={i} className="px-4 py-3 animate-pulse">
                    <div className="flex items-center gap-3">
                      <div className="h-5 w-16 rounded bg-muted" />
                      <div className="h-4 w-24 rounded bg-muted" />
                      <div className="h-4 w-20 rounded bg-muted" />
                    </div>
                  </div>
                ))}
              </div>
            ) : requests.length === 0 ? (
              <div className="py-12 text-center text-sm text-muted-foreground">
                {search || outcomeFilter !== "all" || modelFilter !== "all"
                  ? "No requests match your filters."
                  : "No requests yet. Send traffic through the proxy to populate."}
              </div>
            ) : (
              <div className="divide-y divide-border">
                {requests.map((r) => (
                  <RequestRow
                    key={r.id}
                    request={r}
                    selectable={compareMode}
                    selected={selected.has(r.id)}
                    onToggle={() => {
                      setSelected((prev) => {
                        const next = new Set(prev);
                        if (next.has(r.id)) next.delete(r.id);
                        else if (next.size < 2) next.add(r.id);
                        return next;
                      });
                    }}
                  />
                ))}
              </div>
            )}
          </CardContent>
        </Card>

        {/* Pagination */}
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <p className="text-sm text-muted-foreground">
              Showing {offset + 1}–{Math.min(offset + pageSize, total)} of{" "}
              {total}
            </p>
            <div className="flex items-center gap-1.5">
              <span className="text-xs text-muted-foreground">Per page:</span>
              <Select
                ariaLabel="Rows per page"
                value={String(pageSize)}
                onValueChange={(v) => { setPageSize(Number(v)); setOffset(0); }}
                options={PAGE_SIZE_OPTIONS.map((s) => ({ value: String(s), label: String(s) }))}
              />
            </div>
          </div>
          {total > pageSize && (
            <div className="flex gap-2">
              <button
                onClick={() => setOffset(Math.max(0, offset - pageSize))}
                disabled={offset === 0}
                className="rounded-md border border-border px-3 py-1.5 text-xs font-medium text-muted-foreground hover:bg-accent disabled:opacity-40 disabled:cursor-not-allowed"
              >
                ← Previous
              </button>
              <button
                onClick={() =>
                  setOffset(Math.min(total - pageSize, offset + pageSize))
                }
                disabled={offset + pageSize >= total}
                className="rounded-md border border-border px-3 py-1.5 text-xs font-medium text-muted-foreground hover:bg-accent disabled:opacity-40 disabled:cursor-not-allowed"
              >
                Next →
              </button>
            </div>
          )}
        </div>
      </div>
    </DashboardShell>
  );
}

function RequestRow({ request, selectable, selected, onToggle }: { request: RequestListItem; selectable?: boolean; selected?: boolean; onToggle?: () => void }) {
  const router = useRouter();
  const time = new Date(request.created_at).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
  const totalTokens =
    (request.token_count_input ?? 0) + (request.token_count_output ?? 0);

  return (
    <div
      className={`flex items-center justify-between px-4 py-3 transition-colors cursor-pointer ${
        selected ? "bg-primary/5 border-l-2 border-l-primary" : "hover:bg-accent/30"
      }`}
      onClick={(e) => {
        if (selectable) return;
        router.push(`/requests/${request.id}`);
      }}
    >
      <div className="flex items-center gap-3 min-w-0 flex-1">
        {selectable && (
          <input
            type="checkbox"
            checked={selected}
            onChange={onToggle}
            onClick={(e) => e.stopPropagation()}
            className="h-4 w-4 rounded border-border shrink-0"
          />
        )}
        <Badge
          className={`text-[10px] capitalize shrink-0 ${OUTCOME_STYLES[request.outcome] ?? ""}`}
        >
          {request.outcome}
        </Badge>
        <span className="text-[11px] text-muted-foreground shrink-0">
          {time}
        </span>
        <span className="text-[12px] font-mono text-muted-foreground truncate">
          {request.app_id}
        </span>
        <span className="text-[11px] text-muted-foreground hidden sm:inline">
          {request.model}
        </span>
      </div>
      <div className="flex items-center gap-4 shrink-0">
        {totalTokens > 0 && (
          <span className="text-[11px] font-mono text-muted-foreground">
            {(totalTokens / 1000).toFixed(1)}K
          </span>
        )}
        {request.fast_path_latency_ms != null && (
          <span className="text-[11px] font-mono text-muted-foreground">
            {request.fast_path_latency_ms}ms
          </span>
        )}
        <span className="text-muted-foreground/40 text-xs">→</span>
      </div>
    </div>
  );
}

function ComparePanel({ ids }: { ids: string[] }) {
  const [left, setLeft] = useState<RequestListItem | null>(null);
  const [right, setRight] = useState<RequestListItem | null>(null);

  useEffect(() => {
    const severityOrder: Record<string, number> = { block: 0, escalate: 1, edit: 2, pass: 3 };
    const worstOutcome = (verdicts: { outcome: string }[]): string => {
      if (!verdicts?.length) return "pass";
      return verdicts.reduce((worst, v) =>
        (severityOrder[v.outcome] ?? 3) < (severityOrder[worst.outcome] ?? 3) ? v : worst
      ).outcome;
    };
    const mapToItem = (d: { call: Record<string, unknown>; verdicts: { outcome: string }[] }): RequestListItem | null => {
      const c = d.call;
      if (!c) return null;
      return {
        id: c.id as string, app_id: c.app_id as string, model: c.model as string,
        token_count_input: c.token_count_input as number | null,
        token_count_output: c.token_count_output as number | null,
        upstream_latency_ms: c.upstream_latency_ms as number | null,
        fast_path_latency_ms: c.fast_path_latency_ms as number | null,
        outcome: worstOutcome(d.verdicts), created_at: c.created_at as string,
      };
    };
    fetch(`${API_BASE}/api/v1/requests/${ids[0]}`)
      .then(r => r.json())
      .then(d => { const item = mapToItem(d); if (item) setLeft(item); })
      .catch(() => {});
    fetch(`${API_BASE}/api/v1/requests/${ids[1]}`)
      .then(r => r.json())
      .then(d => { const item = mapToItem(d); if (item) setRight(item); })
      .catch(() => {});
  }, [ids[0], ids[1]]);

  if (!left || !right) return null;

  const fields: [string, (r: RequestListItem) => string][] = [
    ["Outcome", (r) => r.outcome],
    ["Model", (r) => r.model],
    ["App", (r) => r.app_id],
    ["Input Tokens", (r) => String(r.token_count_input ?? 0)],
    ["Output Tokens", (r) => String(r.token_count_output ?? 0)],
    ["Upstream Latency", (r) => `${r.upstream_latency_ms ?? 0}ms`],
    ["Fast-Path Latency", (r) => `${r.fast_path_latency_ms ?? 0}ms`],
    ["Time", (r) => new Date(r.created_at).toLocaleString()],
  ];

  return (
    <Card className="border-primary/20">
      <CardHeader>
        <CardTitle className="text-sm font-medium">Request Comparison</CardTitle>
      </CardHeader>
      <CardContent className="p-0">
        <table className="w-full text-xs">
          <thead>
            <tr className="border-b border-border">
              <th className="px-4 py-2 text-left text-muted-foreground font-medium w-1/3">Field</th>
              <th className="px-4 py-2 text-left font-mono">Request A</th>
              <th className="px-4 py-2 text-left font-mono">Request B</th>
            </tr>
          </thead>
          <tbody>
            {fields.map(([label, fn]) => {
              const leftVal = fn(left);
              const rightVal = fn(right);
              const diff = leftVal !== rightVal;
              return (
                <tr key={label} className={`border-b border-border/50 ${diff ? "bg-primary/5" : ""}`}>
                  <td className="px-4 py-2 text-muted-foreground">{label}</td>
                  <td className={`px-4 py-2 font-mono ${diff ? "font-bold" : ""}`}>{leftVal}</td>
                  <td className={`px-4 py-2 font-mono ${diff ? "font-bold" : ""}`}>{rightVal}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </CardContent>
    </Card>
  );
}
