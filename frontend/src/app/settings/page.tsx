"use client";

import { useState, useEffect } from "react";
import { DashboardShell } from "@/components/layout/dashboard-shell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { useUser, canAccessSettings } from "@/lib/auth";
import { fetchApi } from "@/lib/api";

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
          <p className="text-[14px] text-muted-foreground mt-1">System configuration, profile, and API keys.</p>
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
            </div>
          )}
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
                <button onClick={() => { navigator.clipboard.writeText(newFullKey); }} className="rounded-md border border-border px-2 py-1 text-[11px] hover:bg-accent">Copy</button>
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
      </div>
    </DashboardShell>
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
