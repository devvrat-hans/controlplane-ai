"use client";

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { DashboardShell } from "@/components/layout/dashboard-shell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { getRequests } from "@/lib/api";
import type { RequestListItem } from "@/lib/api";

const API_BASE = process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080";

const OUTCOME_STYLES: Record<string, string> = {
  pass: "bg-[#0070f3]/10 text-[#0070f3] border-[#0070f3]/20",
  edit: "bg-[#f5a623]/10 text-[#ab570a] border-[#f5a623]/20",
  block: "bg-[#ee0000]/10 text-[#ee0000] border-[#ee0000]/20",
  escalate: "bg-[#7928ca]/10 text-[#7928ca] border-[#7928ca]/20",
};

const PAGE_SIZE = 25;

export default function RequestsPage() {
  const [offset, setOffset] = useState(0);
  const [outcomeFilter, setOutcomeFilter] = useState<string>("all");
  const [search, setSearch] = useState("");
  const [modelFilter, setModelFilter] = useState<string>("all");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [compareMode, setCompareMode] = useState(false);

  const { data, isLoading } = useQuery({
    queryKey: ["requests-list", offset, search, modelFilter, outcomeFilter],
    queryFn: () => getRequests({ limit: PAGE_SIZE, offset, search, model: modelFilter, outcome: outcomeFilter }),
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
            <select
              value={modelFilter}
              onChange={(e) => { setModelFilter(e.target.value); setOffset(0); }}
              className="rounded-md border border-input bg-background px-2 py-1.5 text-xs focus:outline-none focus:ring-2 focus:ring-ring/20"
            >
              <option value="all">All models</option>
              {uniqueModels.map((m) => (
                <option key={m} value={m}>{m}</option>
              ))}
            </select>
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
        {total > PAGE_SIZE && (
          <div className="flex items-center justify-between">
            <p className="text-sm text-muted-foreground">
              Showing {offset + 1}–{Math.min(offset + PAGE_SIZE, total)} of{" "}
              {total}
            </p>
            <div className="flex gap-2">
              <button
                onClick={() => setOffset(Math.max(0, offset - PAGE_SIZE))}
                disabled={offset === 0}
                className="rounded-md border border-border px-3 py-1.5 text-xs font-medium text-muted-foreground hover:bg-accent disabled:opacity-40 disabled:cursor-not-allowed"
              >
                ← Previous
              </button>
              <button
                onClick={() =>
                  setOffset(Math.min(total - PAGE_SIZE, offset + PAGE_SIZE))
                }
                disabled={offset + PAGE_SIZE >= total}
                className="rounded-md border border-border px-3 py-1.5 text-xs font-medium text-muted-foreground hover:bg-accent disabled:opacity-40 disabled:cursor-not-allowed"
              >
                Next →
              </button>
            </div>
          </div>
        )}
      </div>
    </DashboardShell>
  );
}

function RequestRow({ request, selectable, selected, onToggle }: { request: RequestListItem; selectable?: boolean; selected?: boolean; onToggle?: () => void }) {
  const time = new Date(request.created_at).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
  const totalTokens =
    (request.token_count_input ?? 0) + (request.token_count_output ?? 0);

  return (
    <div className={`flex items-center justify-between px-4 py-3 transition-colors ${
      selected ? "bg-primary/5 border-l-2 border-l-primary" : "hover:bg-accent/30"
    }`}>
      <div className="flex items-center gap-3 min-w-0 flex-1">
        {selectable && (
          <input
            type="checkbox"
            checked={selected}
            onChange={onToggle}
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
      </div>
    </div>
  );
}

function ComparePanel({ ids }: { ids: string[] }) {
  const [left, setLeft] = useState<RequestListItem | null>(null);
  const [right, setRight] = useState<RequestListItem | null>(null);

  // Fetch both request details
  useState(() => {
    fetch(`${API_BASE}/api/v1/requests?search=${ids[0]}`)
      .then(r => r.json())
      .then(d => setLeft(d.requests?.[0] ?? null))
      .catch(() => {});
    fetch(`${API_BASE}/api/v1/requests?search=${ids[1]}`)
      .then(r => r.json())
      .then(d => setRight(d.requests?.[0] ?? null))
      .catch(() => {});
  });

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
