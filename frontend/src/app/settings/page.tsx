"use client";

import { useState, useEffect } from "react";
import { DashboardShell } from "@/components/layout/dashboard-shell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { useUser, canAccessSettings } from "@/lib/auth";
import { fetchApi } from "@/lib/api";
import { copyText } from "@/lib/clipboard";

const API_BASE = process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080";

interface SystemConfig {
  api_port: number;
  proxy_addr: string;
  upstream_provider: string;
  upstream_model: string;
  upstream_base_url: string;
  event_bus_mode: string;
  database_connected: boolean;
  database_engine: string;
  database_name: string;
}

interface UserProfile {
  id: string;
  email: string;
  name: string | null;
  role: string;
  created_at: string;
}

interface ApiKey {
  id: string;
  name: string;
  key_prefix: string;
  scopes: string[];
  status: string;
  created_at: string;
  last_used: string;
}

const AVAILABLE_SCOPES = [
  { id: "proxy:read", label: "Proxy Read" },
  { id: "proxy:write", label: "Proxy Write" },
  { id: "admin:read", label: "Admin Read" },
  { id: "admin:write", label: "Admin Write" },
  { id: "audit:read", label: "Audit Read" },
  { id: "cost:read", label: "Cost Read" },
];

export default function SettingsPage() {
  const user = useUser();
  const hasAccess = user ? canAccessSettings(user.role) : false;
  const [config, setConfig] = useState<SystemConfig | null>(null);
  const [profile, setProfile] = useState<UserProfile | null>(null);
  const [displayName, setDisplayName] = useState("");
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [keys, setKeys] = useState<ApiKey[]>([]);
  const [showCreateKey, setShowCreateKey] = useState(false);
  const [newKeyName, setNewKeyName] = useState("");
  const [newKeyScopes, setNewKeyScopes] = useState(["proxy:read", "proxy:write"]);
  const [newFullKey, setNewFullKey] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    Promise.all([
      fetch(`${API_BASE}/api/v1/system/config`).then((r) => r.json()).catch(() => null),
      fetchApi<UserProfile>("/api/v1/users/me").catch(() => null),
      fetch(`${API_BASE}/api/v1/api-keys`).then((r) => r.json()).catch(() => ({ keys: [] })),
    ]).then(([cfg, prof, keyData]) => {
      if (cfg) setConfig(cfg);
      if (prof) {
        setProfile(prof);
        setDisplayName(prof.name || prof.email.split("@")[0].replace(/^\w/, (c) => c.toUpperCase()));
      }
      if (keyData?.keys) setKeys(keyData.keys);
    }).finally(() => setLoading(false));
  }, []);

  const handleSaveProfile = async () => {
    if (!displayName.trim()) return;
    setSaving(true);
    try {
      const res = await fetch(`${API_BASE}/api/v1/users/me`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ name: displayName.trim() }),
      });
      if (res.ok) {
        setSaved(true);
        if (user) sessionStorage.setItem("cp-user", JSON.stringify({ ...user, displayName: displayName.trim() }));
        setTimeout(() => setSaved(false), 2000);
      }
    } catch {
      if (user) sessionStorage.setItem("cp-user", JSON.stringify({ ...user, displayName: displayName.trim() }));
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } finally {
      setSaving(false);
    }
  };

  const handleCreateKey = async () => {
    if (!newKeyName.trim()) return;
    try {
      const res = await fetch(`${API_BASE}/api/v1/api-keys`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ name: newKeyName, scopes: newKeyScopes }),
      });
      if (res.ok) {
        const data = await res.json();
        setNewFullKey(data.key);
        setNewKeyName("");
        setShowCreateKey(false);
        const keyData = await fetch(`${API_BASE}/api/v1/api-keys`).then((r) => r.json()).catch(() => ({ keys: [] }));
        if (keyData?.keys) setKeys(keyData.keys);
      }
    } catch { /* ignore */ }
  };

  const handleRevokeKey = async (id: string) => {
    try {
      const res = await fetch(`${API_BASE}/api/v1/api-keys/${id}/revoke`, { method: "POST" });
      if (res.ok) {
        const keyData = await fetch(`${API_BASE}/api/v1/api-keys`).then((r) => r.json()).catch(() => ({ keys: [] }));
        if (keyData?.keys) setKeys(keyData.keys);
      }
    } catch { /* ignore */ }
  };

  const initials = displayName ? displayName.slice(0, 2).toUpperCase() : user ? user.email.split("@")[0].slice(0, 2).toUpperCase() : "??";

  if (loading) {
    return (
      <DashboardShell>
        <div className="flex items-center justify-center py-20">
          <div className="h-6 w-6 animate-spin rounded-full border-2 border-foreground border-t-transparent" />
        </div>
      </DashboardShell>
    );
  }

  return (
    <DashboardShell>
      <div className="space-y-8 max-w-4xl">
        <div>
          <h2 className="text-[24px] font-semibold tracking-tighter-brand">Settings</h2>
          <p className="text-[14px] text-muted-foreground mt-1">System configuration, profile, API keys, and MCP integration.</p>
        </div>

        {/* ═══ Profile Section ═══ */}
        <section className="space-y-4">
          <h3 className="text-[16px] font-semibold tracking-tight">Profile</h3>
          <Card className="shadow-vercel-sm">
            <CardContent className="py-6 space-y-5">
              <div className="flex items-center gap-4">
                <div className="h-14 w-14 rounded-full bg-gradient-to-br from-chart-1 to-chart-3 flex items-center justify-center">
                  <span className="text-lg font-semibold text-white">{initials}</span>
                </div>
                <div>
                  <p className="text-[15px] font-medium">{displayName}</p>
                  <p className="text-[13px] text-muted-foreground">{user?.email}</p>
                </div>
              </div>
              <div>
                <label className="text-xs text-muted-foreground">Display Name</label>
                <input
                  type="text"
                  value={displayName}
                  onChange={(e) => setDisplayName(e.target.value)}
                  className="mt-1 w-full rounded-md border border-input bg-background px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-ring/20"
                />
              </div>
              <div className="flex items-center gap-3">
                <button
                  onClick={handleSaveProfile}
                  disabled={saving || !displayName.trim()}
                  className="rounded-md bg-primary px-4 py-2 text-[13px] font-medium text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-50"
                >
                  {saving ? "Saving..." : "Save Profile"}
                </button>
                {saved && <span className="text-[13px] text-emerald-500">✓ Saved</span>}
              </div>
              <div className="flex items-center gap-4 pt-2 border-t border-border">
                <div className="text-sm"><span className="text-muted-foreground">Role:</span> <Badge variant="outline" className="ml-1 text-[10px] capitalize">{user?.role ?? "unknown"}</Badge></div>
                <div className="text-sm"><span className="text-muted-foreground">Email:</span> <span className="ml-1 font-mono text-xs">{user?.email}</span></div>
              </div>
            </CardContent>
          </Card>
        </section>

        {/* ═══ System Configuration ═══ */}
        <section className="space-y-4">
          <h3 className="text-[16px] font-semibold tracking-tight">System Configuration</h3>
          {!hasAccess ? (
            <Card><CardContent className="py-6 text-center text-muted-foreground">Admin access required.</CardContent></Card>
          ) : (
            <div className="grid gap-4 md:grid-cols-2">
              <Card className="shadow-vercel-sm">
                <CardHeader><CardTitle className="text-[12px] font-medium text-muted-foreground uppercase tracking-wide font-mono">API</CardTitle></CardHeader>
                <CardContent className="space-y-2">
                  <ConfigRow label="API Port" value={String(config?.api_port ?? "...")} />
                  <ConfigRow label="Proxy Address" value={config?.proxy_addr ?? "..."} />
                  <ConfigRow label="Provider" value={config?.upstream_provider ?? "..."} />
                  <ConfigRow label="Model" value={config?.upstream_model ?? "..."} />
                  <ConfigRow label="Upstream URL" value={config?.upstream_base_url ?? "..."} />
                </CardContent>
              </Card>
              <Card className="shadow-vercel-sm">
                <CardHeader><CardTitle className="text-[12px] font-medium text-muted-foreground uppercase tracking-wide font-mono">Infrastructure</CardTitle></CardHeader>
                <CardContent className="space-y-2">
                  <ConfigRow label="Event Bus" value={config?.event_bus_mode ?? "..."} />
                  <div className="flex items-center justify-between">
                    <span className="text-sm">Database</span>
                    <Badge variant="outline" className={`text-[10px] ${config?.database_connected ? "bg-emerald-500/10 text-emerald-500 border-emerald-500/20" : "bg-red-500/10 text-red-500 border-red-500/20"}`}>
                      {config?.database_connected ? "Connected" : "Disconnected"}
                    </Badge>
                  </div>
                  <ConfigRow label="DB Engine" value={config?.database_engine ?? "..."} />
                  <ConfigRow label="DB Name" value={config?.database_name ?? "..."} />
                </CardContent>
              </Card>
            </div>          )}
        </section>

        {/* ═══ System Health ═══ */}
        <section className="space-y-4">
          <h3 className="text-[16px] font-semibold tracking-tight">System Health</h3>
          <SystemHealthCard />
        </section>

        {/* ═══ API Keys ═══ */}
        <section className="space-y-4">
          <div className="flex items-center justify-between">
            <h3 className="text-[16px] font-semibold tracking-tight">API Keys</h3>
            <button onClick={() => setShowCreateKey(!showCreateKey)} className="rounded-md bg-primary px-3 py-1.5 text-[12px] font-medium text-primary-foreground hover:bg-primary/90 transition-colors">
              {showCreateKey ? "Cancel" : "+ Create Key"}
            </button>
          </div>

          {newFullKey && (
            <Card className="border-emerald-500/30 bg-emerald-500/5">
              <CardContent className="py-3 flex items-center justify-between">
                <div>
                  <p className="text-[12px] font-medium text-emerald-500">Key created — copy it now, it won&apos;t be shown again.</p>
                  <code className="text-[11px] font-mono break-all">{newFullKey}</code>
                </div>
                <button onClick={() => { void copyText(newFullKey); }} className="rounded-md border border-border px-2 py-1 text-[11px] hover:bg-accent">Copy</button>
              </CardContent>
            </Card>
          )}

          {showCreateKey && (
            <Card className="shadow-vercel-sm">
              <CardContent className="py-4 space-y-3">
                <input type="text" value={newKeyName} onChange={(e) => setNewKeyName(e.target.value)} placeholder="Key name" className="w-full rounded-md border border-input bg-background px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-ring/20" />
                <div className="flex flex-wrap gap-2">
                  {AVAILABLE_SCOPES.map((s) => (
                    <label key={s.id} className={`text-[11px] px-2 py-1 rounded-md border cursor-pointer transition-colors ${newKeyScopes.includes(s.id) ? "border-primary/50 bg-primary/5" : "border-border"}`}>
                      <input type="checkbox" checked={newKeyScopes.includes(s.id)} onChange={() => setNewKeyScopes((p) => p.includes(s.id) ? p.filter((x) => x !== s.id) : [...p, s.id])} className="mr-1" />
                      {s.label}
                    </label>
                  ))}
                </div>
                <button onClick={handleCreateKey} disabled={!newKeyName.trim()} className="rounded-md bg-primary px-3 py-1.5 text-[12px] font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50">Generate</button>
              </CardContent>
            </Card>
          )}

          <Card className="shadow-vercel-sm">
            <CardContent className="p-0">
              {keys.length === 0 ? (
                <p className="py-6 text-center text-sm text-muted-foreground">No API keys yet.</p>
              ) : (
                <div className="divide-y divide-border">
                  {keys.map((key) => (
                    <div key={key.id} className="flex items-center justify-between px-4 py-3">
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2">
                          <span className="text-[13px] font-medium">{key.name}</span>
                          <Badge variant="outline" className={`text-[10px] ${key.status === "active" ? "bg-emerald-500/10 text-emerald-500 border-emerald-500/20" : "bg-muted text-muted-foreground"}`}>{key.status}</Badge>
                        </div>
                        <p className="text-[11px] font-mono text-muted-foreground mt-0.5">{key.key_prefix}</p>
                        <div className="flex gap-2 mt-1">{key.scopes.map((s) => <span key={s} className="text-[9px] rounded-full bg-muted px-1.5 py-0.5 text-muted-foreground">{s}</span>)}</div>
                      </div>
                      {key.status === "active" && (
                        <button onClick={() => handleRevokeKey(key.id)} className="rounded-md border border-red-500/30 px-2 py-1 text-[11px] font-medium text-red-500 hover:bg-red-500/5 transition-colors ml-3">Revoke</button>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </CardContent>
          </Card>
        </section>

        {/* ═══ MCP Integration ═══ */}
        <McpIntegrationSection />
      </div>
    </DashboardShell>
  );
}

// ─── MCP Integration ─────────────────────────────────────────────────────────
// Mirrors services/mcp-server: Streamable HTTP on :8090 (POST /mcp) plus stdio,
// static role tokens from MCP_AUTH_TOKENS (docker-compose dev defaults below).

const MCP_URL = process.env.NEXT_PUBLIC_MCP_URL || "http://localhost:8090";

const MCP_ROLES = [
  { token: "dev-viewer", role: "viewer", can: "Read dashboards, requests, audit, cost" },
  { token: "dev-reviewer", role: "reviewer", can: "Viewer + resolve escalations" },
  { token: "dev-admin", role: "admin", can: "Reviewer + change policies, profiles, send governed prompts" },
] as const;

const MCP_TOOL_GROUPS = [
  {
    title: "Read · all roles",
    tools: [
      "list_apps", "get_policy", "list_profiles", "list_requests", "get_request", "list_verdicts",
      "get_stats_overview", "get_policy_stats", "get_detection_quality", "get_feedback_effectiveness",
      "get_judge_agreement", "get_latency_timeseries", "get_cost_summary", "get_cost_timeseries",
      "get_cost_daily", "get_cost_anomalies", "list_escalations", "get_session_thread", "get_precedents", "list_audit",
      "verify_audit_chain", "get_system_config", "get_health", "get_ready", "scan_content*",
    ],
  },
  { title: "Resolve · reviewer, admin", tools: ["resolve_escalation"] },
  {
    title: "Write · admin",
    tools: ["evaluate_prompt", "update_policy", "set_governance_level", "apply_profile", "update_profile"],
  },
];

const MCP_EXAMPLE_PROMPTS = [
  "Are there any cost anomalies right now, and which app is driving spend?",
  "Verify the audit chain and summarise the last 20 audit records.",
  "List open escalations and show the precedents for the oldest one.",
  "Show ChatBot-Prod's policy and turn off verbosity checks. (admin)",
];

type McpClientTab = "claude-code" | "json" | "desktop" | "test-bash" | "test-powershell";

const MCP_TEST_BODY = '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"verify_audit_chain","arguments":{}}}';

function mcpSnippet(tab: McpClientTab, token: string): string {
  switch (tab) {
    case "claude-code":
      // One line: pastes unchanged into bash, zsh, PowerShell and cmd.
      return `claude mcp add --transport http controlplane ${MCP_URL}/mcp --header "Authorization: Bearer ${token}"`;
    case "json":
      return JSON.stringify(
        { mcpServers: { controlplane: { url: `${MCP_URL}/mcp`, headers: { Authorization: `Bearer ${token}` } } } },
        null,
        2,
      );
    case "desktop":
      return JSON.stringify(
        {
          mcpServers: {
            controlplane: {
              command: "docker",
              args: ["exec", "-i", "-e", "MCP_TRANSPORT=stdio", "-e", `MCP_TOKEN=${token}`, "controlplane-mcp", "controlplane-mcp"],
            },
          },
        },
        null,
        2,
      );
    case "test-bash":
      // One line; single-quoted JSON is passed through verbatim by bash/zsh/Git Bash.
      return `curl -s -X POST ${MCP_URL}/mcp -H "Authorization: Bearer ${token}" -H "Content-Type: application/json" -d '${MCP_TEST_BODY}'`;
    case "test-powershell":
      // Windows PowerShell 5.1 strips inner double quotes from arguments passed to
      // native programs (curl.exe), which corrupts the JSON; Invoke-RestMethod doesn't.
      return `$body = '${MCP_TEST_BODY}'; (Invoke-RestMethod -Method Post -Uri ${MCP_URL}/mcp -Headers @{ Authorization = "Bearer ${token}" } -ContentType "application/json" -Body $body).result.structuredContent`;
  }
}

const MCP_TABS: { id: McpClientTab; label: string; hint: string }[] = [
  { id: "claude-code", label: "Claude Code", hint: "Run once in a terminal; the tools appear in every Claude Code session." },
  { id: "json", label: "HTTP clients", hint: "For clients that accept a URL + headers (Cursor, VS Code, custom agents): add to their MCP config." },
  { id: "desktop", label: "Claude Desktop", hint: "Add to claude_desktop_config.json, then restart. Uses stdio through the running container." },
  { id: "test-bash", label: "Test (bash)", hint: "Quick check from bash, zsh or Git Bash that the server and your token work — returns the audit chain status." },
  { id: "test-powershell", label: "Test (PowerShell)", hint: "Same check from Windows PowerShell or pwsh — prints first_broken_at, records_checked and valid." },
];

function McpIntegrationSection() {
  const [status, setStatus] = useState<"checking" | "reachable" | "unreachable">("checking");
  const [token, setToken] = useState<string>(MCP_ROLES[0].token);
  const [tab, setTab] = useState<McpClientTab>("claude-code");
  const [copied, setCopied] = useState<"idle" | "copied" | "failed">("idle");

  useEffect(() => {
    // The MCP server sends no CORS headers, so an opaque no-cors probe is used:
    // it resolves when the server answers at all and rejects when it is down.
    fetch(`${MCP_URL}/health`, { mode: "no-cors", cache: "no-store" })
      .then(() => setStatus("reachable"))
      .catch(() => setStatus("unreachable"));
  }, []);

  const snippet = mcpSnippet(tab, token);
  const copy = () => {
    void copyText(snippet).then((ok) => {
      setCopied(ok ? "copied" : "failed");
      setTimeout(() => setCopied("idle"), 1500);
    });
  };

  return (
    <section className="space-y-4">
      <div className="flex items-center justify-between gap-3">
        <h3 className="text-[16px] font-semibold tracking-tight">MCP Integration</h3>
        <div className="flex items-center gap-1.5">
          <div className={`h-2 w-2 rounded-full ${status === "reachable" ? "bg-emerald-500" : status === "checking" ? "bg-yellow-500 animate-pulse" : "bg-red-500"}`} />
          <span className="text-[12px] text-muted-foreground">
            {status === "reachable" ? "MCP server reachable" : status === "checking" ? "Checking…" : "MCP server not reachable"}
          </span>
        </div>
      </div>
      <p className="text-[13px] text-muted-foreground -mt-2">
        ControlPlane ships a Model Context Protocol server, so AI assistants and agents can query governance data and
        act on it — with the same role checks, rate limits and redaction as the dashboard.
      </p>

      {/* Connection details + roles */}
      <div className="grid gap-4 md:grid-cols-2">
        <Card className="shadow-vercel-sm">
          <CardHeader><CardTitle className="text-[12px] font-medium text-muted-foreground uppercase tracking-wide font-mono">Connection</CardTitle></CardHeader>
          <CardContent className="space-y-2">
            <ConfigRow label="Endpoint" value={`${MCP_URL}/mcp`} />
            <ConfigRow label="Transports" value="Streamable HTTP · stdio" />
            <ConfigRow label="Auth header" value="Authorization: Bearer" />
            <ConfigRow label="Rate limit" value="120 req/min per token" />
            <p className="text-[11px] text-muted-foreground pt-1">
              Starts with <code className="font-mono">docker compose up</code> as <code className="font-mono">mcp-server</code>.
              Health: <code className="font-mono">{MCP_URL}/health</code>
            </p>
          </CardContent>
        </Card>
        <Card className="shadow-vercel-sm">
          <CardHeader><CardTitle className="text-[12px] font-medium text-muted-foreground uppercase tracking-wide font-mono">Tokens &amp; Roles</CardTitle></CardHeader>
          <CardContent className="space-y-2">
            {MCP_ROLES.map((r) => (
              <div key={r.token} className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <code className="text-[12px] font-mono">{r.token}</code>
                  <p className="text-[11px] text-muted-foreground">{r.can}</p>
                </div>
                <Badge variant="outline" className="text-[10px] capitalize shrink-0">{r.role}</Badge>
              </div>
            ))}
            <p className="text-[11px] text-muted-foreground pt-1 border-t border-border">
              These are local dev defaults. Set your own with{" "}
              <code className="font-mono">MCP_AUTH_TOKENS=token:role,…</code> in <code className="font-mono">.env</code>{" "}
              (append <code className="font-mono">:app_id</code> to scope a token to one app).
            </p>
          </CardContent>
        </Card>
      </div>

      {/* How to connect */}
      <Card className="shadow-vercel-sm">
        <CardHeader className="pb-3">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <CardTitle className="text-[12px] font-medium text-muted-foreground uppercase tracking-wide font-mono">Connect a client</CardTitle>
            <label className="flex items-center gap-2 text-[12px] text-muted-foreground">
              Token
              <select
                value={token}
                onChange={(e) => setToken(e.target.value)}
                className="rounded-md border border-input bg-background px-2 py-1 text-[12px] font-mono focus:outline-none focus:ring-2 focus:ring-ring/20"
              >
                {MCP_ROLES.map((r) => <option key={r.token} value={r.token}>{r.token} ({r.role})</option>)}
              </select>
            </label>
          </div>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="flex flex-wrap gap-1" role="tablist">
            {MCP_TABS.map((t) => (
              <button
                key={t.id}
                role="tab"
                aria-selected={tab === t.id}
                onClick={() => setTab(t.id)}
                className={`rounded-md px-3 py-1.5 text-[12px] font-medium transition-colors ${tab === t.id ? "bg-primary text-primary-foreground" : "border border-border hover:bg-accent"}`}
              >
                {t.label}
              </button>
            ))}
          </div>
          <p className="text-[12px] text-muted-foreground">{MCP_TABS.find((t) => t.id === tab)?.hint}</p>
          <div className="relative">
            <pre className="overflow-x-auto rounded-md border border-border bg-muted/50 p-3 pr-24 text-[11px] font-mono leading-relaxed whitespace-pre-wrap break-all select-all">{snippet}</pre>
            <button
              onClick={copy}
              className="absolute right-2 top-2 rounded-md border border-border bg-background px-2 py-1 text-[11px] hover:bg-accent"
            >
              {copied === "copied" ? "Copied" : copied === "failed" ? "Select & copy" : "Copy"}
            </button>
          </div>
        </CardContent>
      </Card>

      {/* What it exposes */}
      <div className="grid gap-4 md:grid-cols-2">
        <Card className="shadow-vercel-sm">
          <CardHeader><CardTitle className="text-[12px] font-medium text-muted-foreground uppercase tracking-wide font-mono">Tools ({MCP_TOOL_GROUPS.reduce((n, g) => n + g.tools.length, 0)})</CardTitle></CardHeader>
          <CardContent className="space-y-3">
            {MCP_TOOL_GROUPS.map((g) => (
              <div key={g.title}>
                <p className="text-[11px] font-medium mb-1.5">{g.title} <span className="text-muted-foreground font-normal">({g.tools.length})</span></p>
                <div className="flex flex-wrap gap-1">
                  {g.tools.map((t) => (
                    <span key={t} className="text-[10px] font-mono rounded-full bg-muted px-1.5 py-0.5 text-muted-foreground">{t}</span>
                  ))}
                </div>
              </div>
            ))}
            <p className="text-[10px] text-muted-foreground">
              * <code className="font-mono">scan_content</code> is off unless <code className="font-mono">MCP_ENABLE_INTERNAL_SCANS=true</code>.
              Read-only resources are also exposed under <code className="font-mono">controlplane://</code> (apps, profiles, open escalations, recent audit, metrics).
            </p>
          </CardContent>
        </Card>
        <Card className="shadow-vercel-sm">
          <CardHeader><CardTitle className="text-[12px] font-medium text-muted-foreground uppercase tracking-wide font-mono">Try asking your assistant</CardTitle></CardHeader>
          <CardContent>
            <ul className="space-y-2">
              {MCP_EXAMPLE_PROMPTS.map((p) => (
                <li key={p} className="text-[13px] rounded-md border border-border px-3 py-2">&ldquo;{p}&rdquo;</li>
              ))}
            </ul>
            <p className="text-[11px] text-muted-foreground mt-3">
              Payloads are redacted and credentials are never returned. Actions a token&apos;s role doesn&apos;t allow are refused, not silently ignored.
            </p>
          </CardContent>
        </Card>
      </div>
    </section>
  );
}

function ConfigRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between">
      <span className="text-sm">{label}</span>
      <span className="text-sm font-mono text-muted-foreground">{value}</span>
    </div>
  );
}

function SystemHealthCard() {
  const [health, setHealth] = useState<{ status: string; version?: string; uptime_secs?: number; checks?: Record<string, unknown> } | null>(null);
  const [latency, setLatency] = useState<number | null>(null);
  const [dbStatus, setDbStatus] = useState<string>("checking");
  const [latencyHistory, setLatencyHistory] = useState<number[]>([]);
  const [pingCount, setPingCount] = useState(0);

  useEffect(() => {
    const check = async () => {
      const start = performance.now();
      try {
        const res = await fetch(`${API_BASE}/health`);
        const ms = Math.round(performance.now() - start);
        setLatency(ms);
        setLatencyHistory((prev) => [...prev.slice(-29), ms]);
        setPingCount((c) => c + 1);
        if (res.ok) {
          const data = await res.json();
          setHealth(data);
        }
      } catch {
        setLatency(null);
        setHealth(null);
        setLatencyHistory((prev) => [...prev.slice(-29), 0]);
      }
      try {
        const res = await fetch(`${API_BASE}/ready`);
        setDbStatus(res.ok ? "connected" : "error");
      } catch {
        setDbStatus("unreachable");
      }
    };
    check();
    const id = setInterval(check, 10000);
    return () => clearInterval(id);
  }, []);

  const uptimeStr = health?.uptime_secs
    ? `${Math.floor(health.uptime_secs / 3600)}h ${Math.floor((health.uptime_secs % 3600) / 60)}m`
    : "—";

  const avgLatency = latencyHistory.length > 0
    ? Math.round(latencyHistory.reduce((a, b) => a + b, 0) / latencyHistory.length)
    : 0;

  // Simple sparkline using SVG
  const sparkline = latencyHistory.length > 1 ? (
    <svg width="120" height="24" viewBox="0 0 120 24" className="mt-1">
      <polyline
        fill="none"
        stroke="#3b82f6"
        strokeWidth="1.5"
        points={latencyHistory.map((v, i) => {
          const maxVal = Math.max(...latencyHistory, 1);
          const x = (i / (latencyHistory.length - 1)) * 118 + 1;
          const y = 22 - (v / maxVal) * 20;
          return `${x},${y}`;
        }).join(" ")}
      />
    </svg>
  ) : null;

  return (
    <Card className="shadow-vercel-sm">
      <CardContent className="py-6">
        <div className="grid grid-cols-2 md:grid-cols-4 gap-6">
          <div>
            <p className="text-xs text-muted-foreground">API Status</p>
            <div className="flex items-center gap-2 mt-1">
              <div className={`h-2 w-2 rounded-full ${health ? "bg-emerald-500" : "bg-red-500"}`} />
              <span className="text-sm font-medium">{health ? "Healthy" : "Offline"}</span>
            </div>
          </div>
          <div>
            <p className="text-xs text-muted-foreground">API Latency (avg {avgLatency}ms)</p>
            <div className="flex items-center gap-2">
              <p className="text-sm font-medium">{latency !== null ? `${latency}ms` : "—"}</p>
              {sparkline}
            </div>
            <p className="text-[9px] text-muted-foreground mt-0.5">{pingCount} pings</p>
          </div>
          <div>
            <p className="text-xs text-muted-foreground">Database</p>
            <div className="flex items-center gap-2 mt-1">
              <div className={`h-2 w-2 rounded-full ${dbStatus === "connected" ? "bg-emerald-500" : dbStatus === "checking" ? "bg-yellow-500 animate-pulse" : "bg-red-500"}`} />
              <span className="text-sm font-medium capitalize">{dbStatus}</span>
            </div>
          </div>
          <div>
            <p className="text-xs text-muted-foreground">Version / Uptime</p>
            <p className="text-sm font-mono font-medium mt-1">v{health?.version ?? "?"}</p>
            <p className="text-[11px] text-muted-foreground">{uptimeStr}</p>
          </div>
        </div>
        {/* Component health checks */}
        {health?.checks && (
          <div className="mt-4 pt-3 border-t border-border">
            <p className="text-[10px] text-muted-foreground mb-2 uppercase tracking-wide">Component Checks</p>
            <div className="flex flex-wrap gap-3">
              {Object.entries(health.checks).map(([name, status]) => (
                <div key={name} className="flex items-center gap-1.5">
                  <div className={`h-1.5 w-1.5 rounded-full ${
                    typeof status === "string" && status === "healthy" ? "bg-emerald-500"
                    : typeof status === "string" && status === "unhealthy" ? "bg-red-500"
                    : typeof status === "object" && status !== null && (status as Record<string, unknown>).status === "healthy" ? "bg-emerald-500"
                    : "bg-yellow-500"
                  }`} />
                  <span className="text-[10px] text-muted-foreground font-mono">{name}</span>
                </div>
              ))}
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
