"use client";

import { DashboardShell } from "@/components/layout/dashboard-shell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { useUser, canAccessSettings } from "@/lib/auth";

export default function SettingsPage() {
  const user = useUser();
  const hasAccess = user ? canAccessSettings(user.role) : false;

  return (
    <DashboardShell>
      <div className="space-y-6">
        <div>
          <h2 className="text-[24px] font-semibold tracking-tighter-brand">Settings</h2>
          <p className="text-[14px] text-muted-foreground mt-1">
            System configuration and preferences.
          </p>
        </div>

        {!hasAccess && (
          <Card>
            <CardContent className="py-8 text-center">
              <p className="text-muted-foreground">
                Admin access required to modify settings.
              </p>
              <p className="text-sm text-muted-foreground/60 mt-1">
                Logged in as: {user?.role ?? "unknown"}
              </p>
            </CardContent>
          </Card>
        )}

        {hasAccess && (
          <div className="grid gap-4 md:grid-cols-2">
            <Card className="shadow-vercel-sm">
              <CardHeader>
                <CardTitle className="text-[12px] font-medium text-muted-foreground uppercase tracking-wide font-mono">
                  API Configuration
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-3">
                <div>
                  <label className="text-xs text-muted-foreground">
                    Dashboard API URL
                  </label>
                  <input
                    type="text"
                    defaultValue="http://localhost:8081"
                    className="mt-1 w-full rounded-md border border-input bg-background px-3 py-2 text-sm"
                    readOnly
                  />
                </div>
                <div>
                  <label className="text-xs text-muted-foreground">
                    Proxy Address
                  </label>
                  <input
                    type="text"
                    defaultValue="http://localhost:8900"
                    className="mt-1 w-full rounded-md border border-input bg-background px-3 py-2 text-sm"
                    readOnly
                  />
                </div>
                <div>
                  <label className="text-xs text-muted-foreground">
                    Upstream (AI Provider)
                  </label>
                  <input
                    type="text"
                    defaultValue="http://localhost:9999"
                    className="mt-1 w-full rounded-md border border-input bg-background px-3 py-2 text-sm"
                    readOnly
                  />
                </div>
              </CardContent>
            </Card>

            <Card className="shadow-vercel-sm">
              <CardHeader>
                <CardTitle className="text-[12px] font-medium text-muted-foreground uppercase tracking-wide font-mono">
                  Notifications
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-3">
                <div className="flex items-center justify-between">
                  <span className="text-sm">Slack Webhook</span>
                  <span className="text-xs rounded-full bg-muted px-2 py-0.5 text-muted-foreground">
                    Not configured
                  </span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-sm">Generic Webhooks</span>
                  <span className="text-xs rounded-full bg-muted px-2 py-0.5 text-muted-foreground">
                    0 endpoints
                  </span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-sm">Rate Limit</span>
                  <span className="text-xs rounded-full bg-muted px-2 py-0.5 text-muted-foreground">
                    1 per 60s per app
                  </span>
                </div>
              </CardContent>
            </Card>

            <Card className="shadow-vercel-sm">
              <CardHeader>
                <CardTitle className="text-[12px] font-medium text-muted-foreground uppercase tracking-wide font-mono">
                  Event Bus
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-3">
                <div className="flex items-center justify-between">
                  <span className="text-sm">Mode</span>
                  <span className="text-xs rounded-full bg-emerald-500/10 text-emerald-500 border border-emerald-500/20 px-2 py-0.5">
                    In-Process
                  </span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-sm">Capacity</span>
                  <span className="text-sm font-mono text-muted-foreground">4096</span>
                </div>
              </CardContent>
            </Card>

            <Card className="shadow-vercel-sm">
              <CardHeader>
                <CardTitle className="text-[12px] font-medium text-muted-foreground uppercase tracking-wide font-mono">
                  Database
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-3">
                <div className="flex items-center justify-between">
                  <span className="text-sm">Status</span>
                  <span className="text-xs rounded-full bg-emerald-500/10 text-emerald-500 border border-emerald-500/20 px-2 py-0.5">
                    Connected
                  </span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-sm">Engine</span>
                  <span className="text-sm font-mono text-muted-foreground">PostgreSQL 18</span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-sm">Database</span>
                  <span className="text-sm font-mono text-muted-foreground">controlplane</span>
                </div>
              </CardContent>
            </Card>
          </div>
        )}
      </div>
    </DashboardShell>
  );
}
