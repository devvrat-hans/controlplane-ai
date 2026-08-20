import { DashboardShell } from "@/components/layout/dashboard-shell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

export default function SettingsPage() {
  return (
    <DashboardShell>
      <div className="space-y-6">
        <div>
          <h2 className="text-2xl font-bold tracking-tight">Settings</h2>
          <p className="text-muted-foreground">
            System configuration and preferences.
          </p>
        </div>

        <div className="grid gap-4 md:grid-cols-2">
          <Card>
            <CardHeader>
              <CardTitle className="text-sm font-medium">
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
                  defaultValue="http://localhost:8080"
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
                  defaultValue="http://localhost:3100"
                  className="mt-1 w-full rounded-md border border-input bg-background px-3 py-2 text-sm"
                  readOnly
                />
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className="text-sm font-medium">
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
            </CardContent>
          </Card>
        </div>
      </div>
    </DashboardShell>
  );
}
