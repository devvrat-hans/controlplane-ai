"use client";

import { useQuery, useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { DashboardShell } from "@/components/layout/dashboard-shell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { fetchApi } from "@/lib/api";

const API_BASE = process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080";

interface AuditRecord {
  id: string;
  call_id: string;
  verdict_id: string;
  action_taken: string;
  outcome: string;
  axis: string;
  app_id: string;
  record_hash: string;
  prev_hash: string;
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
        <div className="flex flex-wrap gap-3">
          <select
            value={filters.outcome}
            onChange={(e) =>
              setFilters((f) => ({ ...f, outcome: e.target.value }))
            }
            className="h-8 rounded-md border border-input bg-background px-2 text-xs"
          >
            <option value="">All Outcomes</option>
            <option value="pass">Pass</option>
            <option value="edit">Edit</option>
            <option value="block">Block</option>
            <option value="escalate">Escalate</option>
          </select>
          <select
            value={filters.axis}
            onChange={(e) =>
              setFilters((f) => ({ ...f, axis: e.target.value }))
            }
            className="h-8 rounded-md border border-input bg-background px-2 text-xs"
          >
            <option value="">All Axes</option>
            <option value="performance">Performance</option>
            <option value="cost">Cost</option>
            <option value="responsibility">Responsibility</option>
          </select>
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
            {records.length} records
          </span>
        </div>

        {/* Records table */}
        <Card>
          <CardContent className="p-0">
            {isLoading ? (
              <div className="p-8 text-center">
                <div className="mx-auto h-8 w-8 animate-spin rounded-full border-2 border-primary border-t-transparent" />
              </div>
            ) : records.length === 0 ? (
              <div className="p-8 text-center text-sm text-muted-foreground">
                No audit records found.
              </div>
            ) : (
              <div className="divide-y divide-border max-h-[calc(100vh-350px)] overflow-y-auto">
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
          className={`text-[10px] capitalize ${outcomeColors[data.outcome] ?? ""}`}
        >
          {data.outcome}
        </Badge>
        <Badge variant="outline" className="text-[10px] capitalize">
          {data.axis}
        </Badge>
        <span className="text-sm truncate flex-1">{data.action_taken}</span>
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
            <div>
              <span className="text-muted-foreground">App ID:</span>
              <code className="ml-1">{data.app_id}</code>
            </div>
          </div>
          <div className="border-t border-border pt-2">
            <span className="text-muted-foreground">Hash Chain:</span>
            <div className="mt-1 font-mono text-[10px] space-y-0.5">
              <p>
                <span className="text-muted-foreground">prev:</span>{" "}
                {data.prev_hash}
              </p>
              <p>
                <span className="text-muted-foreground">curr:</span>{" "}
                {data.record_hash}
              </p>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
