import { DashboardShell } from "@/components/layout/dashboard-shell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";

export default function EscalationsPage() {
  return (
    <DashboardShell>
      <div className="space-y-6">
        <div>
          <h2 className="text-2xl font-bold tracking-tight">Escalations</h2>
          <p className="text-muted-foreground">
            Cases requiring human review — only genuinely ambiguous verdicts appear here.
          </p>
        </div>

        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-medium">
              Open Cases (3)
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="space-y-3">
              <EscalationRow
                id="ESC-001"
                axis="responsibility"
                confidence={0.62}
                reason="Potential bias detected in response about hiring practices"
                age="12m ago"
              />
              <EscalationRow
                id="ESC-002"
                axis="performance"
                confidence={0.45}
                reason="Low groundedness — response makes claims not in context"
                age="28m ago"
              />
              <EscalationRow
                id="ESC-003"
                axis="cost"
                confidence={0.58}
                reason="Retry storm: 8 similar requests in 30s window"
                age="1h ago"
              />
            </div>
          </CardContent>
        </Card>
      </div>
    </DashboardShell>
  );
}

function EscalationRow({
  id,
  axis,
  confidence,
  reason,
  age,
}: {
  id: string;
  axis: string;
  confidence: number;
  reason: string;
  age: string;
}) {
  return (
    <div className="flex items-center gap-3 rounded-md border border-border p-3">
      <span className="text-xs font-mono text-muted-foreground">{id}</span>
      <Badge variant="outline" className="text-xs capitalize">
        {axis}
      </Badge>
      <span className="text-xs text-muted-foreground">
        conf: {(confidence * 100).toFixed(0)}%
      </span>
      <span className="flex-1 text-sm truncate">{reason}</span>
      <span className="text-xs text-muted-foreground">{age}</span>
    </div>
  );
}
