import { DashboardShell } from "@/components/layout/dashboard-shell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

export default function CostPage() {
  return (
    <DashboardShell>
      <div className="space-y-6">
        <div>
          <h2 className="text-2xl font-bold tracking-tight">Cost Analytics</h2>
          <p className="text-muted-foreground">
            Token usage, spend tracking, and anomaly detection.
          </p>
        </div>

        <div className="grid gap-4 md:grid-cols-3">
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm font-medium text-muted-foreground">
                Today&apos;s Spend
              </CardTitle>
            </CardHeader>
            <CardContent>
              <div className="text-2xl font-bold">$42.18</div>
              <p className="text-xs text-muted-foreground">+8% vs yesterday</p>
            </CardContent>
          </Card>
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm font-medium text-muted-foreground">
                Tokens Used
              </CardTitle>
            </CardHeader>
            <CardContent>
              <div className="text-2xl font-bold">2.4M</div>
              <p className="text-xs text-muted-foreground">Across all apps</p>
            </CardContent>
          </Card>
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm font-medium text-muted-foreground">
                Anomalies
              </CardTitle>
            </CardHeader>
            <CardContent>
              <div className="text-2xl font-bold text-orange-500">2</div>
              <p className="text-xs text-muted-foreground">Above baseline</p>
            </CardContent>
          </Card>
        </div>

        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-medium">
              Token Usage Over Time
            </CardTitle>
          </CardHeader>
          <CardContent className="h-64 flex items-center justify-center">
            <p className="text-sm text-muted-foreground">
              Chart will render here with live data from the cost API.
            </p>
          </CardContent>
        </Card>
      </div>
    </DashboardShell>
  );
}
