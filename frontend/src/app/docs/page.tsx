"use client";

import { useState } from "react";
import { DashboardShell } from "@/components/layout/dashboard-shell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";

const API_BASE = process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080";

interface EndpointGroup {
  title: string;
  description: string;
  endpoints: Endpoint[];
}

interface Endpoint {
  method: string;
  path: string;
  description: string;
  auth: boolean;
  body?: string;
  response?: string;
}

const API_GROUPS: EndpointGroup[] = [
  {
    title: "Proxy",
    description: "Send AI requests through the governance layer",
    endpoints: [
      {
        method: "POST",
        path: "/v1/messages",
        description: "Forward a request to the upstream AI model through the governance proxy. Supports app_id and session_id for routing and multi-turn tracking. The request is intercepted, checked by fast-path rules, and the response is inspected before delivery.",
        auth: true,
        body: `{
  "model": "qwen2.5:1.5b",
  "app_id": "10000000-0000-0000-0000-000000000001",
  "session_id": "optional-session-uuid",
  "messages": [
    { "role": "user", "content": "What is 2+2?" }
  ],
  "max_tokens": 256
}
// app_id: Routes to specific app policies (fallback: default app)
// session_id: Links multi-turn conversations (fallback: derived from messages)
// Also accepted via headers: X-App-Id, X-Session-Id`,
        response: `{
  "id": "msg_abc123",
  "content": [{ "type": "text", "text": "4" }],
  "model": "qwen2.5:1.5b",
  "usage": { "input_tokens": 12, "output_tokens": 5 }
}
// Headers:
// X-ControlPlane-Correlation-Id: <uuid>
// X-ControlPlane-Latency-Ms: <ms>`,
      },
    ],
  },
  {
    title: "Dashboard API",
    description: "Backend endpoints for the dashboard frontend",
    endpoints: [
      {
        method: "GET",
        path: "/api/v1/stats/overview",
        description: "Get aggregate statistics: total calls, verdicts, blocks, escalations in the last 24 hours.",
        auth: false,
        response: `{
  "total_calls_24h": 150,
  "total_verdicts_24h": 150,
  "blocks_24h": 12,
  "escalations_24h": 8,
  "passes_24h": 120,
  "open_escalations": 3,
  "avg_fast_path_latency_ms": 3.2,
  "top_blocked_axes": [{ "axis": "responsibility", "count": 10 }]
}`,
      },
      {
        method: "GET",
        path: "/api/v1/verdicts/recent?limit=2000",
        description: "Fetch recent verdicts from the database. Supports up to 10000 limit. Filters: app_id, outcome.",
        auth: false,
      },
      {
        method: "GET",
        path: "/api/v1/verdicts/stream",
        description: "SSE endpoint that streams real-time verdicts as they flow through the proxy.",
        auth: false,
      },
      {
        method: "GET",
        path: "/api/v1/requests/{call_id}",
        description: "Get full details for a single request: intercepted call, all verdicts, audit trail, and escalation case.",
        auth: false,
      },
      {
        method: "GET",
        path: "/api/v1/audit?limit=50",
        description: "Query the hash-chained audit trail. Supports outcome, axis, and app_id filters.",
        auth: false,
      },
      {
        method: "GET",
        path: "/api/v1/audit/export?format=csv",
        description: "Export audit records as CSV or JSON. Supports app_id and outcome filters.",
        auth: false,
      },
      {
        method: "GET",
        path: "/api/v1/audit/verify",
        description: "Verify the integrity of the audit hash chain. Returns the number of broken links.",
        auth: false,
      },
      {
        method: "GET",
        path: "/api/v1/stats/policy?app_id=<uuid>&window_hours=24",
        description: "Policy-wise effectiveness stats: per-check blocked/escalated/edited/passed counts, FP rate, reviewer resolution outcomes.",
        auth: false,
      },
      {
        method: "GET",
        path: "/api/v1/metrics/detection-quality",
        description: "Detection quality metrics: trust score, precision by axis, FP/FN breakdown, 7-day trend.",
        auth: false,
      },
      {
        method: "GET",
        path: "/api/v1/metrics/feedback-effectiveness",
        description: "Feedback loop effectiveness: patterns promoted, overrides applied, resolution distribution, escalation trend.",
        auth: false,
      },
      {
        method: "GET",
        path: "/api/v1/feedback/precedents",
        description: "Retrieve reviewer precedents (similar past decisions). Pass ?escalation_id=UUID or ?call_id=UUID as query param.",
        auth: false,
      },
      {
        method: "GET",
        path: "/api/v1/requests?limit=50&offset=0&search=&model=&outcome=&app_id=",
        description: "List intercepted API calls with server-side search, model filter, outcome filter, app filter, and pagination (up to 500).",
        auth: false,
      },
      {
        method: "GET",
        path: "/api/v1/sessions/{call_id}/thread",
        description: "Get the full conversation thread for a session — all turns, verdicts, and accumulated risk.",
        auth: false,
      },
      {
        method: "GET",
        path: "/api/v1/metrics/latency-timeseries",
        description: "Hourly fast-path latency buckets (avg + p99) for the last 24 hours — powers the overview sparkline.",
        auth: false,
      },
      {
        method: "GET",
        path: "/api/v1/cost/summary",
        description: "Get token usage and cost summary for the last 24 hours, including per-model cost breakdown.",
        auth: false,
      },
      {
        method: "GET",
        path: "/api/v1/cost/timeseries",
        description: "Hourly token usage breakdown for charts.",
        auth: false,
      },
      {
        method: "GET",
        path: "/api/v1/escalations?status=all_open&limit=50",
        description: "List escalation cases. Status: open, in_review, resolved, or all_open (open + in_review).",
        auth: false,
      },
      {
        method: "POST",
        path: "/api/v1/escalations/{id}/resolve",
        description: "Resolve an escalation case with a resolution (confirm, override, dismiss).",
        auth: false,
        body: `{
  "resolution": "confirm",
  "reason": "Verdict was correct"
}`,
      },
    ],
  },
  {
    title: "Policies",
    description: "Manage governance policies per application",
    endpoints: [
      {
        method: "GET",
        path: "/api/v1/apps",
        description: "List all registered applications.",
        auth: false,
      },
      {
        method: "GET",
        path: "/api/v1/policies/{app_id}",
        description: "Get the governance policy for an application (thresholds, enabled checks).",
        auth: false,
      },
      {
        method: "PUT",
        path: "/api/v1/policies/{app_id}",
        description: "Update the governance policy for an application.",
        auth: false,
        body: `{
  "block_threshold": 0.9,
  "escalate_threshold": 0.6,
  "unsafe_content_enabled": true,
  "secret_detection_enabled": true,
  "prompt_injection_enabled": true,
  "hallucination_detection_enabled": true
}`,
      },
      {
        method: "PUT",
        path: "/api/v1/apps/{app_id}/governance",
        description: "Update the data governance level (high/medium/low) for an application. Adjusts policy thresholds accordingly.",
        auth: false,
        body: `{ "level": "low" }
// high: stricter (lower block/escalate thresholds, higher groundedness)
// medium: balanced defaults
// low: relaxed (higher thresholds needed to trigger)`,
      },
      {
        method: "GET",
        path: "/api/v1/profiles",
        description: "List available regulatory profiles (EU-Financial, US-Healthcare, India-General, etc.) with their default thresholds.",
        auth: false,
      },
      {
        method: "PUT",
        path: "/api/v1/profiles/{profile_id}",
        description: "Update a regulatory profile's default thresholds independently. Does not affect apps until explicitly applied.",
        auth: false,
        body: `{
  "responsibility": { "block_threshold": 0.85, "escalate_threshold": 0.5 },
  "performance": { "hallucination_threshold": 0.7, "groundedness_min": 0.6 },
  "cost": { "max_tokens_per_request": 4000, "retry_max": 3 }
}`,
      },
      {
        method: "POST",
        path: "/api/v1/policies/{app_id}/profile",
        description: "Apply a regulatory profile to an application, copying its thresholds to the app's runtime policies.",
        auth: false,
        body: `{ "profile_id": "eu-financial" }`,
      },
    ],
  },
  {
    title: "API Keys",
    description: "Manage API keys for authentication",
    endpoints: [
      {
        method: "GET",
        path: "/api/v1/api-keys",
        description: "List all API keys (shows prefix, status, scopes — never the full key).",
        auth: false,
      },
      {
        method: "POST",
        path: "/api/v1/api-keys",
        description: "Create a new API key. The full key is shown only once.",
        auth: false,
        body: `{
  "name": "My App",
  "scopes": ["proxy:read", "proxy:write"]
}`,
        response: `{
  "status": "created",
  "id": "uuid",
  "key": "cp_xxxx...full_key...",
  "key_prefix": "cp_xxxx••••••••",
  "message": "Copy this key now - it won't be shown again"
}`,
      },
      {
        method: "POST",
        path: "/api/v1/api-keys/{id}/revoke",
        description: "Revoke an API key. It can no longer be used for authentication.",
        auth: false,
      },
      {
        method: "GET",
        path: "/api/v1/api-keys/{id}/analytics",
        description: "Get usage analytics for a specific API key: requests, tokens, costs, verdicts.",
        auth: false,
      },
    ],
  },
  {
    title: "Settings",
    description: "System configuration and user profile",
    endpoints: [
      {
        method: "GET",
        path: "/api/v1/system/config",
        description: "Get system configuration (provider, model, database status, etc.).",
        auth: false,
      },
      {
        method: "GET",
        path: "/api/v1/users/me",
        description: "Get the current user's profile.",
        auth: false,
      },
      {
        method: "PUT",
        path: "/api/v1/users/me",
        description: "Update the current user's display name.",
        auth: false,
        body: `{ "name": "New Name" }`,
      },
    ],
  },
];

const METHOD_COLORS: Record<string, string> = {
  GET: "bg-green-500/10 text-green-500 border-green-500/20",
  POST: "bg-blue-500/10 text-blue-500 border-blue-500/20",
  PUT: "bg-yellow-500/10 text-yellow-500 border-yellow-500/20",
  DELETE: "bg-red-500/10 text-red-500 border-red-500/20",
};

export default function DocsPage() {
  return (
    <DashboardShell>
      <div className="space-y-6">
        <div>
          <h2 className="text-2xl font-bold tracking-tight">API Documentation</h2>
          <p className="text-muted-foreground">
            REST API reference for ControlPlane.ai. Base URL:{" "}
            <code className="text-xs bg-muted px-1.5 py-0.5 rounded">{API_BASE}</code>
          </p>
        </div>

        {/* Authentication */}
        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-medium">Authentication</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3 text-sm">
            <p>
              The proxy endpoint (<code>/v1/messages</code>) accepts API key authentication via the{" "}
              <code>X-API-Key</code> header or <code>Authorization: Bearer</code> header.
            </p>
            <p>
              Dashboard API endpoints do not require authentication in demo mode. In production, they would require a valid JWT token.
            </p>
            <div className="bg-muted/50 rounded-md p-3 font-mono text-xs">
              <p className="text-muted-foreground"># Using API key</p>
              <p>curl -H &quot;X-API-Key: cp_your_key_here&quot; http://localhost:8900/v1/messages ...</p>
              <p className="mt-2 text-muted-foreground"># Using Bearer token</p>
              <p>curl -H &quot;Authorization: Bearer cp_your_key_here&quot; http://localhost:8900/v1/messages ...</p>
            </div>
          </CardContent>
        </Card>

        {/* Rate Limits */}
        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-medium">Rate Limits</CardTitle>
          </CardHeader>
          <CardContent className="text-sm">
            <p>
              When using Ollama (local), there are <strong>no rate limits</strong>. All requests are processed immediately.
            </p>
            <p className="mt-2">
              When using cloud providers (OpenCode, Anthropic), rate limits depend on your provider plan.
            </p>
          </CardContent>
        </Card>

        {/* Endpoint Groups */}
        {API_GROUPS.map((group) => (
          <Card key={group.title}>
            <CardHeader>
              <CardTitle className="text-sm font-medium">{group.title}</CardTitle>
              <p className="text-xs text-muted-foreground">{group.description}</p>
            </CardHeader>
            <CardContent className="p-0">
              <div className="divide-y divide-border">
                {group.endpoints.map((ep) => (
                  <div key={`${ep.method}-${ep.path}`} className="px-4 py-3 hover:bg-accent/30 transition-colors">
                    <div className="flex items-center gap-3">
                      <Badge className={`text-[10px] font-mono ${METHOD_COLORS[ep.method] ?? ""}`}>
                        {ep.method}
                      </Badge>
                      <code className="text-sm font-mono">{ep.path}</code>
                      {ep.auth && (
                        <Badge variant="outline" className="text-[10px]">Auth required</Badge>
                      )}
                    </div>
                    <p className="text-xs text-muted-foreground mt-1.5">{ep.description}</p>

                    {ep.body && (
                      <details className="mt-2">
                        <summary className="text-xs text-muted-foreground cursor-pointer hover:text-foreground">
                          Request body
                        </summary>
                        <pre className="text-[11px] bg-muted/50 rounded-md p-3 mt-1 overflow-x-auto font-mono">
                          {ep.body}
                        </pre>
                      </details>
                    )}

                    {ep.response && (
                      <details className="mt-2">
                        <summary className="text-xs text-muted-foreground cursor-pointer hover:text-foreground">
                          Response
                        </summary>
                        <pre className="text-[11px] bg-muted/50 rounded-md p-3 mt-1 overflow-x-auto font-mono">
                          {ep.response}
                        </pre>
                      </details>
                    )}

                    {/* Try it button for GET endpoints */}
                    {ep.method === "GET" && (
                      <TryItButton path={ep.path} />
                    )}
                  </div>
                ))}
              </div>
            </CardContent>
          </Card>
        ))}
      </div>
    </DashboardShell>
  );
}

function TryItButton({ path }: { path: string }) {
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<{ status: number; body: string } | null>(null);
  const [error, setError] = useState<string | null>(null);

  const tryIt = async () => {
    setLoading(true);
    setResult(null);
    setError(null);
    try {
      let url = path;

      // For endpoints needing a real call_id, fetch one from the DB
      if (url.includes("{call_id}") || (url.includes("{id}") && url.includes("/requests"))) {
        const res = await fetch(`${API_BASE}/api/v1/requests?limit=1`);
        const data = await res.json();
        const realId = data?.requests?.[0]?.id;
        if (realId) {
          url = url.replace(/\{call_id\}/g, realId).replace(/\{id\}/g, realId);
        }
      }

      // For escalation endpoints needing a real escalation ID
      if (url.includes("{id}") && url.includes("/escalations")) {
        const res = await fetch(`${API_BASE}/api/v1/escalations?status=all_open&limit=1`);
        const data = await res.json();
        const realId = data?.cases?.[0]?.id;
        if (realId) url = url.replace(/\{id\}/g, realId);
      }

      // For api-keys endpoints needing a real key ID
      if (url.includes("{id}") && url.includes("/api-keys")) {
        const res = await fetch(`${API_BASE}/api/v1/api-keys`);
        const data = await res.json();
        const realId = data?.keys?.[0]?.id;
        if (realId) url = url.replace(/\{id\}/g, realId);
        else url = url.replace(/\{id\}/g, "no-keys-exist");
      }

      // Replace {app_id} with a known demo app
      url = url.replace(/\{app_id\}/g, "10000000-0000-0000-0000-000000000001");
      // Replace {profile_id} with a known demo profile
      url = url.replace(/\{profile_id\}/g, "eu-financial");
      // Replace remaining path params and <angle> params
      url = url.replace(/\{[^}]+\}/g, "10000000-0000-0000-0000-000000000001");
      url = url.replace(/<[^>]+>/g, "10000000-0000-0000-0000-000000000001");
      // For feedback/precedents, fetch a real call_id to use as query param
      if (url.includes("/feedback/precedents")) {
        const res = await fetch(`${API_BASE}/api/v1/requests?limit=1`);
        const data = await res.json();
        const realCallId = data?.requests?.[0]?.id;
        if (realCallId) {
          url = `/api/v1/feedback/precedents?call_id=${realCallId}`;
        }
      }

      // Strip query param placeholders but keep real values
      const qIdx = url.indexOf("?");
      if (qIdx > -1) {
        const base = url.slice(0, qIdx);
        const params = new URLSearchParams(url.slice(qIdx + 1));
        const cleaned = new URLSearchParams();
        params.forEach((v, k) => {
          if (k === "limit") cleaned.set(k, "5");
          else if (k === "status") cleaned.set(k, v);
          else if (k === "window_hours") cleaned.set(k, v);
          else if (k === "format") cleaned.set(k, v);
          else if (k === "call_id") cleaned.set(k, v);
          else if (k === "escalation_id") cleaned.set(k, v);
          else if (k === "app_id") cleaned.set(k, v);
        });
        url = cleaned.toString() ? `${base}?${cleaned}` : base;
      }

      const fullUrl = `${API_BASE}${url}`;
      const start = performance.now();
      const res = await fetch(fullUrl);
      const elapsed = Math.round(performance.now() - start);
      const text = await res.text();
      let formatted = text;
      try { formatted = JSON.stringify(JSON.parse(text), null, 2); } catch { /* not JSON */ }
      setResult({ status: res.status, body: `[${elapsed}ms] ${formatted}` });
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="mt-2">
      <button
        onClick={tryIt}
        disabled={loading}
        className="rounded-md border border-border px-2 py-1 text-[11px] font-medium text-muted-foreground hover:bg-accent transition-colors disabled:opacity-50"
      >
        {loading ? "Trying..." : "▶ Try it"}
      </button>
      {result && (
        <pre className="text-[10px] bg-muted/50 rounded-md p-2 mt-1 overflow-x-auto font-mono max-h-48 overflow-y-auto">
          <span className={result.status < 400 ? "text-green-500" : "text-red-500"}>
            {result.status}
          </span>{" "}
          {result.body}
        </pre>
      )}
      {error && (
        <p className="text-[10px] text-red-500 mt-1">{error}</p>
      )}
    </div>
  );
}
