"use client";

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
        description: "Forward a request to the upstream AI model through the governance proxy. The request is intercepted, checked by fast-path rules, and the response is inspected before delivery.",
        auth: true,
        body: `{
  "model": "qwen2.5:1.5b",
  "messages": [
    { "role": "user", "content": "What is 2+2?" }
  ],
  "max_tokens": 256
}`,
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
        path: "/api/v1/verdicts/recent?limit=50",
        description: "Fetch recent verdicts from the database. Supports optional app_id and outcome filters.",
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
        path: "/api/v1/audit/verify",
        description: "Verify the integrity of the audit hash chain. Returns the number of broken links.",
        auth: false,
      },
      {
        method: "GET",
        path: "/api/v1/cost/summary",
        description: "Get token usage and cost summary for the last 24 hours.",
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
        path: "/api/v1/escalations?status=open&limit=50",
        description: "List escalation cases. Filter by status (open, in_review, resolved).",
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
