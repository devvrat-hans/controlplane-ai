import { DashboardShell } from "@/components/layout/dashboard-shell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

export default function PoliciesPage() {
  return (
    <DashboardShell>
      <div className="space-y-6">
        <div>
          <h2 className="text-2xl font-bold tracking-tight">Policies</h2>
          <p className="text-muted-foreground">
            Configure detection thresholds and actions per app.
          </p>
        </div>

        <div className="grid gap-4 md:grid-cols-3">
          <PolicyCard
            axis="Performance"
            description="Groundedness scoring, hallucination detection"
            threshold="0.6"
            action="Escalate"
          />
          <PolicyCard
            axis="Cost"
            description="Token budgets, retry detection, verbosity"
            threshold="4096 tokens/req"
            action="Block on exceed"
          />
          <PolicyCard
            axis="Responsibility"
            description="PII/secrets, bias, unsafe content"
            threshold="0.7 confidence"
            action="Edit (PII) / Block (unsafe)"
          />
        </div>
      </div>
    </DashboardShell>
  );
}

function PolicyCard({
  axis,
  description,
  threshold,
  action,
}: {
  axis: string;
  description: string;
  threshold: string;
  action: string;
}) {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">{axis}</CardTitle>
        <p className="text-sm text-muted-foreground">{description}</p>
      </CardHeader>
      <CardContent className="space-y-2">
        <div className="flex justify-between text-sm">
          <span className="text-muted-foreground">Threshold</span>
          <span className="font-mono">{threshold}</span>
        </div>
        <div className="flex justify-between text-sm">
          <span className="text-muted-foreground">Action</span>
          <span>{action}</span>
        </div>
      </CardContent>
    </Card>
  );
}
